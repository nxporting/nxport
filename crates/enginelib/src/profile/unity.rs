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

    /// Sets `want` (negative: the game's own) when it changed. The game's own value is read before the first change, to
    /// go back to.
    fn apply(&self, want: i32) {
        let last = self.applied.load(Relaxed);
        if want == last || (want < 0 && last == i32::MIN) {
            return;
        }
        if self.game.load(Relaxed) == i32::MIN {
            let Some(get) = lookup(self.get) else { return };
            let get: unsafe extern "C" fn() -> i32 = unsafe { std::mem::transmute(get) };
            self.game.store(unsafe { get() }, Relaxed);
        }
        self.applied.store(want, Relaxed);
        let value = if want < 0 { self.game.load(Relaxed) } else { want };
        let Some(set) = lookup(self.set) else { return };
        let set: unsafe extern "C" fn(i32) = unsafe { std::mem::transmute(set) };
        unsafe { set(value) };
        let what = self.set.split("::set_").nth(1).and_then(|s| s.split('(').next()).unwrap_or(self.set);
        println!("[+] Unity: {what} = {value}{}", if want < 0 { " (the game's own)" } else { "" });
    }
}

/// Texture size: QualitySettings.globalTextureMipmapLimit (0 full size .. 3 an eighth of each side).
static TEXTURE: IntSetting = IntSetting::new(
    "UnityEngine.QualitySettings::set_globalTextureMipmapLimit(System.Int32)",
    "UnityEngine.QualitySettings::get_globalTextureMipmapLimit()",
);
/// Frame rate: QualitySettings.vSyncCount, vblanks per frame (1: 60 fps, 2: 30 fps).
static VSYNC_COUNT: IntSetting =
    IntSetting::new("UnityEngine.QualitySettings::set_vSyncCount(System.Int32)", "UnityEngine.QualitySettings::get_vSyncCount()");
/// LOD bias: what was last set (percent of the game's own; i32::MIN none) and the game's own (f32 bits; 0: not read).
static LOD_BIAS: AtomicI32 = AtomicI32::new(i32::MIN);
static GAME_LOD_BIAS: AtomicU32 = AtomicU32::new(0);

/// Applies the Unity settings that changed (main thread, from profile::main_thread_tick).
pub fn tick() {
    // A lifted game's code is not called directly; see the top of this file for the frame.
    if !cfg!(nx_native) || resolver().is_none() || crate::gpu::try_backend().map_or(0, |b| b.presented_frames()) == 0 {
        return;
    }
    let s = crate::host::live::unity();
    TEXTURE.apply(s.texture_size);
    VSYNC_COUNT.apply(match s.frame_rate {
        60 => 1,
        30 => 2,
        _ => -1,
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
