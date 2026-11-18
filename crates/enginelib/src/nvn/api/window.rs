// NVN windows: textures, crop, present interval, acquiring the next texture; window origin and depth mode.

use super::*;

// ── remaining window / queue / device setters (no rendering state to keep yet) ──────────────
// Clip-space conventions set on the NVNdevice (NVNwindowOriginMode LOWER_LEFT=0 / UPPER_LEFT=1,
// NVNdepthMode NEAR_IS_MINUS_W=0 / NEAR_IS_ZERO=1). The executor needs them to map NVN clip space onto Metal's.
pub(crate) static WINDOW_ORIGIN_MODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub(crate) static DEPTH_MODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

pub fn window_origin_mode() -> i32 {
    WINDOW_ORIGIN_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn depth_mode() -> i32 {
    DEPTH_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Vblanks each presented frame stays on screen as the game asked (nvnWindowSetPresentInterval; 1 = 60 Hz, 2 = 30 Hz,
/// 0 = no wait).
pub(crate) static PRESENT_INTERVAL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);

/// The interval presents follow: the game's, at least 2 at the 30 fps setting. Read each frame, so changing the setting
/// while the game runs takes effect at once (the games' frame-rate hooks unlock their pacer whatever the setting).
pub fn present_interval() -> i32 {
    let asked = PRESENT_INTERVAL.load(std::sync::atomic::Ordering::Relaxed);
    if crate::host::overlay::fps_mode() == crate::settings::FpsMode::Stock30 {
        asked.max(2)
    } else {
        asked
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowSetPresentInterval(_window: usize, interval: i32) {
    #[cfg(all(nx_runtime, windows))]
    crate::guest::exec::live_patch::present_interval_set(interval);
    // Debug: --log-pacing prints the inputs of TotK's frame-pacing decision (guest+0x8f6670): the measured frame
    // duration in ticks at obj+0x118, the tick frequency it divides by, and the state at obj+0x248 (max, min, bias,
    // current interval). obj is the caller's saved x19 (stored at its frame pointer + 0x38).
    static LOG_PACING: OnceLock<bool> = OnceLock::new();
    if *LOG_PACING.get_or_init(|| crate::settings::get().log.log_pacing) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let fp = crate::guest_reg!(x29);
        let base = crate::guest::loader::main_base();
        let caller_fp = *(fp as *const usize);
        if n % 10 == 0
            && base != 0
            && caller_fp > 0x1_0000_0000
            && *((caller_fp + 8) as *const usize) == base + 0x8f633c
        {
            let obj = *((caller_fp + 0x38) as *const usize);
            let ticks = *((obj + 0x118) as *const i64);
            let freq = **((base + 0x462ab50) as *const *const i64);
            let st = obj + 0x248;
            let (mx, mn, bias, cur) = (
                *(st as *const i32),
                *((st + 4) as *const i32),
                *((st + 8) as *const f32),
                *((st + 0x14) as *const i32),
            );
            // Listeners that may veto lowering the interval: list at st+0x30 (sentinel), node {.., next @+8, obj @+0x10},
            // callback = (*(obj+0x28))[0] called with x0 = obj+0x28.
            let mut cbs = Vec::new();
            let head = st + 0x30;
            let mut node = *((head + 8) as *const usize);
            while node != head && node > 0x1_0000_0000 && cbs.len() < 8 {
                let obj = *((node + 0x10) as *const usize);
                let vt = *((obj + 0x28) as *const usize);
                let f = *(vt as *const usize);
                let fmt = |f: usize| {
                    if f >= base && f < base + 0x5000000 {
                        format!("guest+{:#x}", f - base)
                    } else {
                        format!("{:#x}", f)
                    }
                };
                // Delegate (guest+0x2aea334): object at d+8, member pointer at d+0x10 (virtual if d+0x18 is odd).
                let d = obj + 0x28;
                let (o, mp, adj) = (
                    *((d + 8) as *const usize),
                    *((d + 0x10) as *const usize),
                    *((d + 0x18) as *const isize),
                );
                let this = (o as isize + (adj >> 1)) as usize;
                let target = if adj & 1 != 0 && this > 0x1_0000_0000 {
                    *((*(this as *const usize) + mp) as *const usize)
                } else {
                    mp
                };
                cbs.push(format!("{} -> {}", fmt(f), fmt(target)));
                node = *((node + 8) as *const usize);
            }
            println!("[pacing] call {} set {} ticks {} freq {} -> {:.2} vblanks (+bias {:.2}) min {} max {} cur {} lowest-allowed {} cooldown {} listeners {:?}",
                n, interval, ticks, freq, ticks as f64 / freq as f64 * 60.0, bias, mx, mn, cur, *((st + 0x18) as *const i32), *((st + 0x20) as *const i32), cbs);
        }
    }
    if PRESENT_INTERVAL.swap(interval, std::sync::atomic::Ordering::Relaxed) != interval {
        let fp = crate::guest_reg!(x29);
        println!(
            "[+] NVN: present interval = {} (guest callers: {})",
            interval,
            guest_chain(fp)
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetPresentInterval(_window: usize) -> i32 {
    present_interval()
}

// ── Builder/object getters and the setters they report (values as set, defaults from SetDefaults) ──
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetNumActiveTextures(
    b: *mut NvnWindowBuilder,
    n: i32,
) {
    if !b.is_null() {
        (*b).num_active = n;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetDevice(b: *const NvnWindowBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetNumTextures(b: *const NvnWindowBuilder) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).texture_count
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetTexture(
    b: *const NvnWindowBuilder,
    id: i32,
) -> usize {
    if b.is_null() || id < 0 || id >= (*b).texture_count.min(4) {
        0
    } else {
        (*b).textures[id as usize]
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetNumActiveTextures(
    b: *const NvnWindowBuilder,
) -> i32 {
    if b.is_null() {
        0
    } else if (*b).num_active > 0 {
        (*b).num_active
    } else {
        (*b).texture_count
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetNumTextures(w: *const NvnWindow) -> i32 {
    if w.is_null() {
        0
    } else {
        (*w).num_textures
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetNumActiveTextures(w: *const NvnWindow) -> i32 {
    if w.is_null() {
        0
    } else {
        (*w).texture_count
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetCrop(w: *const NvnWindow, crop: *mut [i32; 4]) {
    if !w.is_null() && !crop.is_null() {
        *crop = (*w).crop;
    }
}

// ---------------------------------------------------------------------------
// NVN Window family. NVNwindow is 384 bytes, NVNwindowBuilder 0x40 (64)
// bytes. This is the actual framebuffer presentation path -- Present pushes
// a real packet into the compositor channel that already exists
// (compositor.rs) but has never been fed real data before now. Nothing
// consumes that channel into real pixels yet (that's the dedicated Metal-
// translation follow-up this session has flagged twice already), but the
// data now genuinely flows end-to-end from the game's present call instead
// of being silently dropped.
#[repr(C)]
pub(crate) struct NvnWindowBuilder {
    device: usize,
    textures: [usize; 4], // NVN swap chains are typically double/triple buffered; 4 is generous
    texture_count: i32,
    num_active: i32, // nvnWindowBuilderSetNumActiveTextures; 0 = all
}

#[repr(C)]
pub(crate) struct NvnWindow {
    pub(crate) textures: [usize; 4],
    /// Active textures (the ones AcquireTexture cycles through).
    pub(crate) texture_count: i32,
    pub(crate) next_index: u32,
    pub(crate) num_textures: i32,
    pub(crate) device: usize,
    pub(crate) crop: [i32; 4],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetDefaults(b: *mut NvnWindowBuilder) {
    record_hle_call("hle_nvnWindowBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetDevice(b: *mut NvnWindowBuilder, device: usize) {
    record_hle_call("hle_nvnWindowBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetTextures(
    b: *mut NvnWindowBuilder,
    count: i32,
    textures: *const usize,
) {
    record_hle_call("hle_nvnWindowBuilderSetTextures");
    if b.is_null() || textures.is_null() {
        return;
    }
    let n = (count.max(0) as usize).min(4);
    (*b).texture_count = n as i32;
    for i in 0..n {
        (*b).textures[i] = *textures.add(i);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowInitialize(
    w: *mut NvnWindow,
    builder: *const NvnWindowBuilder,
) -> u8 {
    record_hle_call("hle_nvnWindowInitialize");
    if w.is_null() || builder.is_null() {
        return 0;
    }
    let b = &*builder;
    let active = if b.num_active > 0 {
        b.num_active.min(b.texture_count)
    } else {
        b.texture_count
    };
    // Default crop: the whole first texture.
    let crop = if b.textures[0] != 0 {
        let t = &*(b.textures[0] as *const NvnTexture);
        [0, 0, t.width, t.height]
    } else {
        [0; 4]
    };
    std::ptr::write(
        w,
        NvnWindow {
            textures: b.textures,
            texture_count: active,
            next_index: 0,
            num_textures: b.texture_count,
            device: b.device,
            crop,
        },
    );
    if crate::settings::get().system.fps == crate::settings::FpsMode::Stock30 {
        PRESENT_INTERVAL.store(2, std::sync::atomic::Ordering::Relaxed);
    }
    println!(
        "[+] NVN: Window initialized with {} swap textures ({} active)",
        b.texture_count, active
    );
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowFinalize(w: *mut NvnWindow) {
    record_hle_call("hle_nvnWindowFinalize");
    if !w.is_null() {
        std::ptr::write_bytes(w, 0, 1);
    }
}

/// nvnWindowAcquireTexture calls and time spent in its vsync wait (reported on the gfx fps line).
pub static ACQUIRE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub static ACQUIRE_SLEEP_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The highest frame rate with VSync off.
pub(crate) const UNPACED_MAX_FPS: u64 = 144;

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowAcquireTexture(
    w: *mut NvnWindow,
    _fence: usize,
    out_index: *mut i32,
) -> i32 {
    record_hle_call("hle_nvnWindowAcquireTexture");
    if w.is_null() || (*w).texture_count == 0 {
        return 1; // NVN_WINDOW_ACQUIRE_TEXTURE_RESULT_NATIVE_ERROR
    }
    // Vsync: a texture becomes available `present interval` vblanks (60 Hz) after the previous one. Absolute deadlines
    // keep the average rate exact; if we fall far behind, resync instead of bursting.
    {
        static NEXT: StdMutex<Option<std::time::Instant>> = StdMutex::new(None);
        // Interval 0 is NVN's "no vsync": no wait here (frame-rate mods such as UltraCam use it with their own limiter;
        // a window's CAMetalLayer still paces presentation at the display's refresh rate).
        let interval = present_interval();
        let is_stock30 = crate::host::overlay::fps_mode() == crate::settings::FpsMode::Stock30;
        let vsync = crate::host::overlay::is_vsync();
        // The frame-rate limit paces (vsync only chooses how the swapchain presents). A title whose game time does not
        // follow real time is also held to its interval: running faster would run it fast. VSync off still stops at
        // UNPACED_MAX_FPS: above the audio renderer's 200 frames a second TotK starts no sounds at all.
        let frame = {
            let mut fps = crate::host::live::frame_limit().clamp(30, UNPACED_MAX_FPS as u32);
            if is_stock30 {
                fps = 30;
            } else if !crate::profile::real_time() && interval > 0 {
                fps = fps.min(60 / interval as u32);
            }
            // A mod's own interval, which vsync holds a Switch to. The game goes on seeing the one it asked for.
            #[cfg(all(nx_runtime, windows))]
            if let Some(forced) = crate::guest::exec::live_patch::forced_interval() {
                if !is_stock30 {
                    fps = fps.min(60 / forced as u32);
                }
            }
            // With vsync at or above the display's refresh, the swapchain's wait for the vblank paces: a limiter above it
            // let frames out faster than the display takes them, and the swapchain then held the game back unevenly
            // (microstutter: game time follows those uneven frames); at it, two clocks of the same rate drift apart.
            let refresh = crate::host::live::refresh_rate();
            if vsync && refresh != 0 && fps + 1 >= refresh {
                std::time::Duration::ZERO
            } else {
                std::time::Duration::from_nanos(1_000_000_000 / fps.max(1) as u64)
            }
        };
        let wait = if frame.is_zero() {
            #[cfg(nx_native)]
            let paced = interval == 0 && crate::gpu::metal::display_pacing() && crate::host::overlay::is_vsync();
            #[cfg(not(nx_native))]
            let paced = false;
            if paced {
                #[cfg(nx_native)]
                let w = crate::gpu::metal::display_pacing_wait().unwrap_or_default();
                #[cfg(not(nx_native))]
                let w = std::time::Duration::ZERO;
                w
            } else {
                std::time::Duration::ZERO
            }
        } else {
            let mut g = NEXT.lock().unwrap();
            let now = std::time::Instant::now();
            let deadline = match *g {
                Some(d) if d > now && d < now + frame * 2 => d,
                _ => now,
            };
            *g = Some(deadline + frame);
            deadline.saturating_duration_since(now)
        };
        ACQUIRE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !wait.is_zero() {
            ACQUIRE_SLEEP_US.fetch_add(
                wait.as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            hle_sleep_ns(wait.as_nanos() as u64);
        }
    }
    #[cfg(nx_native)]
    crate::gpu::metal::frame_started();
    let idx = (*w).next_index % ((*w).texture_count as u32);
    (*w).next_index = (*w).next_index.wrapping_add(1);
    if !out_index.is_null() {
        *out_index = idx as i32;
    }
    0 // NVN_WINDOW_ACQUIRE_TEXTURE_RESULT_SUCCESS
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowSetCrop(
    _w: *mut NvnWindow,
    _x: i32,
    _y: i32,
    _w2: i32,
    _h: i32,
) {
    record_hle_call("hle_nvnWindowSetCrop");
    if !_w.is_null() {
        if _w2 <= 0 || _h <= 0 {
            let (tw, th) = if (*_w).textures[0] != 0 {
                let t = &*((*_w).textures[0] as *const NvnTexture);
                (t.width, t.height)
            } else {
                (0, 0)
            };
            (*_w).crop = [0, 0, tw, th];
        } else {
            (*_w).crop = [_x, _y, _w2, _h];
        }
        println!(
            "[+] NVN: WindowSetCrop x={} y={} w={} h={} (effective: {:?})",
            _x,
            _y,
            _w2,
            _h,
            (*_w).crop
        );
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowSetNumActiveTextures(w: *mut NvnWindow, count: i32) {
    record_hle_call("hle_nvnWindowSetNumActiveTextures");
    if !w.is_null() {
        (*w).texture_count = count.max(1).min((*w).num_textures.max(1));
    }
}
