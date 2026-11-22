// Unity games (IL2CPP): Unity's own quality settings, changed while the game runs as a game's options menu would change
// them, through the engine functions Unity registers for its scripts ("internal calls", looked up by name).
//
// The lookup (il2cpp::vm::InternalCalls::Resolve) is found in main's code, not at a per-version address: IL2CPP's
// generated code calls il2cpp_codegen_resolve_icall with x0 = "UnityEngine.<Class>::<method>(<signature>)" (adrp x0;
// add x0; bl), and that function's first call is the lookup, which returns null for a name it does not have (the caller
// throws). Calls run on the game's main thread (profile::main_thread_tick) once a frame was presented: Unity registers
// its internal calls and loads its QualitySettings (from globalgamemanagers) before it starts its graphics, and calls
// made earlier crash on an empty table or a null QualitySettings.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicU32, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

/// The lookup's address, if main is a Unity IL2CPP game (found once).
fn resolver() -> Option<usize> {
    static R: OnceLock<Option<usize>> = OnceLock::new();
    *R.get_or_init(|| unsafe { find_resolver() })
}

/// Whether the running game is Unity (IL2CPP).
pub fn detected() -> bool {
    resolver().is_some()
}

unsafe fn find_resolver() -> Option<usize> {
    let base = crate::guest::loader::main_base();
    let len = crate::guest::loader::main_text_size();
    if base == 0 || len < 16 {
        return None;
    }
    let text = std::slice::from_raw_parts(base as *const u32, len / 4);
    let sx = |v: u32, bits: u32| ((v << (32 - bits)) as i32 >> (32 - bits)) as i64;
    let mut wrappers: HashMap<usize, u32> = HashMap::new();
    for i in 0..text.len().saturating_sub(2) {
        let (w, w2, w3) = (text[i], text[i + 1], text[i + 2]);
        // adrp x0, page; add x0, x0, #lo; bl target
        if w & 0x9f00_001f != 0x9000_0000 || w2 & 0xffc0_03ff != 0x9100_0000 || w3 & 0xfc00_0000 != 0x9400_0000 {
            continue;
        }
        let pc = base + i * 4;
        let page = (pc & !0xfff) as i64 + (sx(((w >> 5) & 0x7ffff) << 2 | (w >> 29) & 3, 21) << 12);
        let s = (page + ((w2 >> 10) & 0xfff) as i64) as usize;
        if !is_icall_name(s, base, len) {
            continue;
        }
        let target = (pc as i64 + 8 + sx(w3 & 0x03ff_ffff, 26) * 4) as usize;
        *wrappers.entry(target).or_default() += 1;
    }
    let (&wrapper, &n) = wrappers.iter().max_by_key(|(_, &n)| n)?;
    if n < 3 {
        return None;
    }
    // The wrapper's first call is the lookup.
    let first = (wrapper..wrapper + 64).step_by(4).find(|&a| *(a as *const u32) & 0xfc00_0000 == 0x9400_0000)?;
    let w = *(first as *const u32);
    let lookup = (first as i64 + sx(w & 0x03ff_ffff, 26) * 4) as usize;
    println!(
        "[+] Unity: internal-call lookup at main+{:#x} (resolve_icall main+{:#x}, {n} call sites)",
        lookup - base,
        wrapper - base
    );
    Some(lookup)
}

/// Unity's frame interval in seconds (a float in main's data), if found. Its NVN device waits each frame, one vsync
/// event at a time, until 0.9 x the interval has passed since the previous frame; the setter makes the interval
/// max(1, vSyncCount) / 60 with 60 written in its code (mov w9, #0x42700000; fmov s1, w9; str w1, [x8]; adrp x8;
/// add x8, x8, #lo; fdiv s0, s0, s1; str s0, [x8]), whatever Application.targetFrameRate says.
fn frame_interval() -> Option<usize> {
    static F: OnceLock<Option<usize>> = OnceLock::new();
    *F.get_or_init(|| unsafe {
        let base = crate::guest::loader::main_base();
        let len = crate::guest::loader::main_text_size();
        if base == 0 || len < 32 {
            return None;
        }
        let text = std::slice::from_raw_parts(base as *const u32, len / 4);
        let sx = |v: u32, bits: u32| ((v << (32 - bits)) as i32 >> (32 - bits)) as i64;
        let mut found = None;
        for i in 0..text.len().saturating_sub(7) {
            let w = &text[i..i + 7];
            if w[0] != 0x52a8_4e09 || w[1] != 0x1e27_0121 || w[2] != 0xb900_0101 || w[3] & 0x9f00_001f != 0x9000_0008 {
                continue;
            }
            if w[4] & 0xffc0_03ff != 0x9100_0108 || w[5] != 0x1e21_1800 || w[6] != 0xbd00_0100 {
                continue;
            }
            if found.is_some() {
                println!("[!] Unity: two frame-interval setters; the frame rate stays the game's above 60");
                return None;
            }
            let pc = base + (i + 3) * 4;
            let page = (pc & !0xfff) as i64 + (sx(((w[3] >> 5) & 0x7ffff) << 2 | (w[3] >> 29) & 3, 21) << 12);
            found = Some((page + ((w[4] >> 10) & 0xfff) as i64) as usize);
        }
        match found {
            Some(a) => println!("[+] Unity: frame interval at main+{:#x} ({} s)", a - base, *(a as *const f32)),
            None => println!("[!] Unity: no frame-interval setter found; the frame rate stays at most 60"),
        }
        found
    })
}

/// Whether `s` (outside main's code, in the image) holds "UnityEngine.<...>::<...>(...)".
unsafe fn is_icall_name(s: usize, base: usize, text_len: usize) -> bool {
    let end = crate::guest::loader::guest_end();
    if s < base + text_len || end == 0 || s + 128 > end {
        return false;
    }
    let b = std::slice::from_raw_parts(s as *const u8, 128);
    let Some(len) = b.iter().position(|&c| c == 0) else { return false };
    let name = &b[..len];
    name.starts_with(b"UnityEngine.") && name.ends_with(b")") && name.windows(2).any(|w| w == b"::")
}

/// Unity's engine function for internal call `name` ("UnityEngine.Class::method(signature)"), or None.
fn lookup(name: &str) -> Option<usize> {
    static CACHE: Mutex<Option<HashMap<String, Option<usize>>>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(&f) = cache.get(name) {
        return f;
    }
    let resolve: unsafe extern "C" fn(*const u8) -> usize = unsafe { std::mem::transmute(resolver()?) };
    let c = std::ffi::CString::new(name).ok()?;
    let f = Some(unsafe { resolve(c.as_ptr() as *const u8) }).filter(|&f| f != 0);
    if f.is_none() {
        println!("[!] Unity: no internal call {name}");
    }
    cache.insert(name.to_string(), f);
    f
}

/// One of Unity's int settings: what was last set and the game's own value (i32::MIN: not set / not read yet).
struct IntSetting {
    set: &'static str,
    get: &'static str,
    applied: AtomicI32,
    game: AtomicI32,
}
impl IntSetting {
    const fn new(set: &'static str, get: &'static str) -> Self {
        IntSetting { set, get, applied: AtomicI32::new(i32::MIN), game: AtomicI32::new(i32::MIN) }
    }

    /// Sets `want` (None: the game's own) when it changed. The game's own value is read before the first change, to go
    /// back to.
    fn apply(&self, want: Option<i32>) {
        // i32::MIN: never set; i32::MIN + 1: the game's own.
        let (want, own) = match want {
            Some(v) => (v, false),
            None => (i32::MIN + 1, true),
        };
        let last = self.applied.load(Relaxed);
        if want == last || (own && last == i32::MIN) {
            return;
        }
        if self.game.load(Relaxed) == i32::MIN {
            let Some(get) = lookup(self.get) else { return };
            let get: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(get) };
            self.game.store(unsafe { get() }, Relaxed);
        }
        self.applied.store(want, Relaxed);
        let value = if own { self.game.load(Relaxed) } else { want };
        let Some(set) = lookup(self.set) else { return };
        let set: unsafe extern "C" fn(i32) = unsafe { std::mem::transmute(set) };
        unsafe { set(value) };
        let what = self.set.split("::set_").nth(1).and_then(|s| s.split('(').next()).unwrap_or(self.set);
        println!("[+] Unity: {what} = {value}{}", if own { " (the game's own)" } else { "" });
    }
}

/// Texture size: QualitySettings.globalTextureMipmapLimit (0 full size .. 3 an eighth of each side).
static TEXTURE: IntSetting = IntSetting::new(
    "UnityEngine.QualitySettings::set_globalTextureMipmapLimit(System.Int32)",
    "UnityEngine.QualitySettings::get_globalTextureMipmapLimit()",
);
/// Frame rate: QualitySettings.vSyncCount, vblanks per frame (1: 60 fps, 2: 30 fps); above 60 the frame interval itself
/// (frame_interval).
static VSYNC_COUNT: IntSetting =
    IntSetting::new("UnityEngine.QualitySettings::set_vSyncCount(System.Int32)", "UnityEngine.QualitySettings::get_vSyncCount()");
/// The frame interval last written above 60 fps (f32 bits; u32::MAX: the game's).
static INTERVAL_SET: AtomicU32 = AtomicU32::new(u32::MAX);
/// LOD bias: what was last set (percent of the game's own; i32::MIN none) and the game's own (f32 bits; 0: not read).
static LOD_BIAS: AtomicI32 = AtomicI32::new(i32::MIN);
static GAME_LOD_BIAS: AtomicU32 = AtomicU32::new(0);

/// Whether the game runs above the console's 60 fps by its Frame Rate setting: Unity steps its time by the real time
/// a frame took (Time.deltaTime), so its presents need not keep the console's interval.
pub fn real_time() -> bool {
    let f = crate::host::live::unity().frame_rate;
    detected() && f > 60 && frame_interval().is_some()
}

/// Applies the Unity settings that changed (main thread, from profile::main_thread_tick).
pub fn tick() {
    // A lifted game's code is not called directly; see the top of this file for the frame.
    if !cfg!(nx_native) || resolver().is_none() || crate::gpu::try_backend().map_or(0, |b| b.presented_frames()) == 0 {
        return;
    }
    let s = crate::host::live::unity();
    TEXTURE.apply((s.texture_size >= 0).then_some(s.texture_size));
    let above = s.frame_rate > 60 && frame_interval().is_some();
    VSYNC_COUNT.apply(match s.frame_rate {
        30 => Some(2),
        60 => Some(1),
        _ if above => Some(1),
        _ => None,
    });
    // Above 60: the interval for the rate (0 unlimited), written every frame (Unity's setter writes it again when
    // vSyncCount changes); the emulated display refreshes at that rate, since the device waits for a new vsync event
    // each frame. Back at or below 60, the setter's own value for the current vSyncCount.
    if let Some(at) = frame_interval() {
        let p = at as *mut f32;
        if above {
            let v = if s.frame_rate == crate::settings::UNITY_UNLIMITED { 0.0 } else { 1.0 / s.frame_rate as f32 };
            unsafe { p.write_volatile(v) };
            if INTERVAL_SET.swap(v.to_bits(), Relaxed) != v.to_bits() {
                println!("[+] Unity: frame interval = {v} s");
            }
        } else if INTERVAL_SET.swap(u32::MAX, Relaxed) != u32::MAX {
            let get: Option<unsafe extern "C" fn() -> i32> =
                lookup("UnityEngine.QualitySettings::get_vSyncCount()").map(|f| unsafe { std::mem::transmute(f) });
            let n = get.map_or(1, |g| unsafe { g() }).max(1);
            unsafe { p.write_volatile(n as f32 / 60.0) };
            println!("[+] Unity: frame interval = {} s (the game's)", n as f32 / 60.0);
        }
    }
    crate::horizon::vi::set_vsync_rate(match s.frame_rate {
        crate::settings::UNITY_UNLIMITED if above => 1000,
        f if above => f,
        _ => 60,
    });
    let percent = s.lod_bias_percent as i32;
    let last = LOD_BIAS.load(Relaxed);
    if percent == last || (percent == 100 && last == i32::MIN) {
        return;
    }
    if GAME_LOD_BIAS.load(Relaxed) == 0 {
        let Some(get) = lookup("UnityEngine.QualitySettings::get_lodBias()") else { return };
        let get: unsafe extern "C" fn() -> f32 = unsafe { std::mem::transmute(get) };
        GAME_LOD_BIAS.store(unsafe { get() }.to_bits(), Relaxed);
    }
    LOD_BIAS.store(percent, Relaxed);
    let Some(set) = lookup("UnityEngine.QualitySettings::set_lodBias(System.Single)") else { return };
    let set: unsafe extern "C" fn(f32) = unsafe { std::mem::transmute(set) };
    let v = f32::from_bits(GAME_LOD_BIAS.load(Relaxed)) * percent as f32 / 100.0;
    unsafe { set(v) };
    println!("[+] Unity: lodBias = {v} ({percent}% of the game's)");
}
