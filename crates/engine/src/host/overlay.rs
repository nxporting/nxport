// In-game lean performance overlay and dynamic runtime settings.
//
// Maintains rolling frame times and GPU performance stats, rendered directly onto the Metal
// drawable during presentation when enabled. Provides atomic runtime switches for:
// - Performance overlay HUD (FPS, 1% low, avg, frame-time graph, GPU & wait metrics)
// - Dynamic aspect ratio (window-following FOV/HUD vs fixed 16:9)
// - FPS mode (Unlocked vs Stock 30)
// - Display mode (Docked 1080p vs Handheld 720p)
// - VSync
// - Asynchronous shader pipeline compilation

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use crate::settings::FpsMode;

static OVERLAY_ENABLED: AtomicBool = AtomicBool::new(false);
static DYNAMIC_ASPECT: AtomicBool = AtomicBool::new(true);
static FPS_MODE: AtomicU8 = AtomicU8::new(1); // 0 = Stock30, 1 = Unlocked
static VSYNC: AtomicBool = AtomicBool::new(true);
static ASYNC_SHADERS: AtomicBool = AtomicBool::new(true);
static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub fn init() {
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(s) = crate::settings::try_get() {
        DYNAMIC_ASPECT.store(s.totk.v1_1_2.dynamic_aspect_ratio, Ordering::Relaxed);
        let fps_val = if s.system.fps == FpsMode::Stock30 { 0 } else { 1 };
        FPS_MODE.store(fps_val, Ordering::Relaxed);
        VSYNC.store(!s.display.no_vsync, Ordering::Relaxed);
        ASYNC_SHADERS.store(s.graphics.async_pipelines, Ordering::Relaxed);
    }
}

pub fn is_overlay_enabled() -> bool {
    OVERLAY_ENABLED.load(Ordering::Relaxed)
}

pub fn set_overlay_enabled(val: bool) {
    OVERLAY_ENABLED.store(val, Ordering::Relaxed);
}

pub fn toggle_overlay() -> bool {
    let next = !is_overlay_enabled();
    set_overlay_enabled(next);
    next
}

pub fn is_dynamic_aspect() -> bool {
    init();
    DYNAMIC_ASPECT.load(Ordering::Relaxed)
}

pub fn set_dynamic_aspect(val: bool) {
    init();
    DYNAMIC_ASPECT.store(val, Ordering::Relaxed);
    if !val {
        crate::profile::screen_aspect_changed(16.0 / 9.0);
    } else {
        #[cfg(target_os = "macos")]
        if let Some(aspect) = crate::host::display::current_window_aspect() {
            crate::profile::screen_aspect_changed(aspect);
        }
    }
}

pub fn toggle_dynamic_aspect() -> bool {
    let next = !is_dynamic_aspect();
    set_dynamic_aspect(next);
    next
}

pub fn fps_mode() -> FpsMode {
    init();
    if FPS_MODE.load(Ordering::Relaxed) == 0 {
        FpsMode::Stock30
    } else {
        FpsMode::Unlocked
    }
}

pub fn set_fps_mode(mode: FpsMode) {
    init();
    FPS_MODE.store(if mode == FpsMode::Stock30 { 0 } else { 1 }, Ordering::Relaxed);
}

pub fn toggle_fps_mode() -> FpsMode {
    let next = if fps_mode() == FpsMode::Stock30 {
        FpsMode::Unlocked
    } else {
        FpsMode::Stock30
    };
    set_fps_mode(next);
    next
}

pub fn is_vsync() -> bool {
    init();
    VSYNC.load(Ordering::Relaxed)
}

pub fn set_vsync(val: bool) {
    init();
    VSYNC.store(val, Ordering::Relaxed);
}

pub fn toggle_vsync() -> bool {
    let next = !is_vsync();
    set_vsync(next);
    next
}

pub fn is_async_shaders() -> bool {
    init();
    ASYNC_SHADERS.load(Ordering::Relaxed)
}

pub fn set_async_shaders(val: bool) {
    init();
    ASYNC_SHADERS.store(val, Ordering::Relaxed);
}

pub fn toggle_async_shaders() -> bool {
    let next = !is_async_shaders();
    set_async_shaders(next);
    next
}

// ---------------------------------------------------------------------------
// Frame Performance Metrics
// ---------------------------------------------------------------------------

pub const HISTORY_LEN: usize = 120;

#[derive(Clone, Copy, Debug, Default)]
pub struct FrameStats {
    pub fps: f32,
    pub avg_fps: f32,
    pub p1_low_fps: f32,
    pub frame_time_ms: f32,
    pub gpu_time_ms: f32,
    pub submit_wait_ms: f32,
    pub vsync_wait_ms: f32,
    pub submits: u64,
    pub draws: u64,
    pub passes: u64,
    pub memory_mb: u64,
}

struct FrameHistory {
    times: [f32; HISTORY_LEN],
    head: usize,
    count: usize,
    last_present: Option<Instant>,
    stats: FrameStats,
}

impl FrameHistory {
    const fn new() -> Self {
        Self {
            times: [0.0; HISTORY_LEN],
            head: 0,
            count: 0,
            last_present: None,
            stats: FrameStats {
                fps: 0.0,
                avg_fps: 0.0,
                p1_low_fps: 0.0,
                frame_time_ms: 0.0,
                gpu_time_ms: 0.0,
                submit_wait_ms: 0.0,
                vsync_wait_ms: 0.0,
                submits: 0,
                draws: 0,
                passes: 0,
                memory_mb: 0,
            },
        }
    }
}

static HISTORY: Mutex<FrameHistory> = Mutex::new(FrameHistory::new());

/// Record performance metrics for the frame just presented.
pub fn record_present(
    gpu_ms: f32,
    submit_wait_ms: f32,
    vsync_wait_ms: f32,
    submits: u64,
    draws: u64,
    passes: u64,
    memory_bytes: u64,
) {
    let now = Instant::now();
    let mut h = HISTORY.lock().unwrap();
    let dt_ms = match h.last_present {
        Some(last) => now.duration_since(last).as_secs_f32() * 1000.0,
        None => 16.67,
    };
    h.last_present = Some(now);

    let idx = h.head;
    h.times[idx] = dt_ms;
    h.head = (idx + 1) % HISTORY_LEN;
    if h.count < HISTORY_LEN {
        h.count += 1;
    }

    let n = h.count;
    if n > 0 {
        let mut sum = 0.0f32;
        let mut sorted = [0.0f32; HISTORY_LEN];
        for i in 0..n {
            let t = h.times[i];
            sum += t;
            sorted[i] = t;
        }
        let avg_dt = sum / n as f32;
        let avg_fps = if avg_dt > 0.0 { 1000.0 / avg_dt } else { 0.0 };
        let inst_fps = if dt_ms > 0.0 { 1000.0 / dt_ms } else { 0.0 };

        // 1% low: 99th percentile frame duration
        let slice = &mut sorted[..n];
        slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let p99_idx = ((n as f32 * 0.99) as usize).min(n - 1);
        let p99_dt = slice[p99_idx];
        let p1_low_fps = if p99_dt > 0.0 { 1000.0 / p99_dt } else { 0.0 };

        h.stats = FrameStats {
            fps: inst_fps,
            avg_fps,
            p1_low_fps,
            frame_time_ms: dt_ms,
            gpu_time_ms: gpu_ms,
            submit_wait_ms,
            vsync_wait_ms,
            submits,
            draws,
            passes,
            memory_mb: memory_bytes / (1024 * 1024),
        };
    }
}

#[allow(dead_code)]
pub fn current_stats() -> FrameStats {
    HISTORY.lock().unwrap().stats
}

// ---------------------------------------------------------------------------
// macOS ImGui Overlay Renderer
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
mod render_impl {
    use super::*;
    use std::cell::RefCell;
    use objc2::runtime::ProtocolObject;
    use objc2_metal::{MTLCommandBuffer, MTLDevice, MTLPixelFormat, MTLTexture};
    use dear_imgui_rs as imgui;
    use crate::host::ui::renderer::Renderer;

    struct OverlayState {
        ctx: imgui::Context,
        renderer: Renderer,
    }

    thread_local! {
        static OVERLAY_STATE: RefCell<Option<OverlayState>> = const { RefCell::new(None) };
    }

    pub fn render(
        device: &ProtocolObject<dyn MTLDevice>,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        target: &ProtocolObject<dyn MTLTexture>,
    ) {
        if !is_overlay_enabled() {
            return;
        }

        let (stats, ordered_times) = {
            let h = HISTORY.lock().unwrap();
            let mut buf = [0.0f32; HISTORY_LEN];
            let n = h.count;
            if n > 0 {
                let start = (h.head + HISTORY_LEN - n) % HISTORY_LEN;
                for i in 0..n {
                    buf[i] = h.times[(start + i) % HISTORY_LEN];
                }
            }
            (h.stats, (buf, n))
        };

        OVERLAY_STATE.with(|st| {
            let mut lock = st.borrow_mut();
            if lock.is_none() {
                let mut ctx = imgui::Context::create();
                let _ = ctx.set_ini_filename(None::<String>);
                ctx.io_mut().set_config_macosx_behaviors(true);

                // Load system font or fall back
                let font_paths = [
                    "/System/Library/Fonts/SFNS.ttf",
                    "/System/Library/Fonts/Helvetica.ttc",
                ];
                let source = font_paths
                    .iter()
                    .find_map(|p| imgui::StbTrueTypeFontData::from_file(p).ok())
                    .map(imgui::FontSource::stb_truetype)
                    .unwrap_or_else(imgui::FontSource::default_font);
                ctx.font_atlas().add_font(&[source]);
                ctx.style_mut().set_font_size_base(14.0);

                let renderer = match Renderer::new(device, &mut ctx, MTLPixelFormat::BGRA8Unorm) {
                    Ok(r) => r,
                    Err(_) => return,
                };
                *lock = Some(OverlayState { ctx, renderer });
            }

            let Some(state) = lock.as_mut() else { return };

            let w = target.width() as f32;
            let h = target.height() as f32;
            let io = state.ctx.io_mut();
            io.set_display_size([w, h]);
            io.set_display_framebuffer_scale([1.0, 1.0]);
            io.set_delta_time(0.016);

            let frame = state.ctx.begin_frame();
            {
                let ui = frame.ui();
                let _bg = ui.push_style_color(imgui::StyleColor::WindowBg, [0.05, 0.05, 0.08, 0.78]);
                let _border = ui.push_style_color(imgui::StyleColor::Border, [0.35, 0.35, 0.45, 0.6]);
                let _rounding = ui.push_style_var(imgui::StyleVar::WindowRounding(6.0));
                let _padding = ui.push_style_var(imgui::StyleVar::WindowPadding([10.0, 8.0]));

                unsafe {
                    imgui::sys::igSetNextWindowPos(
                        imgui::sys::ImVec2 { x: w - 16.0, y: 16.0 },
                        imgui::Condition::Always as i32,
                        imgui::sys::ImVec2 { x: 1.0, y: 0.0 },
                    );
                }

                ui.window("Performance##Overlay")
                    .flags(
                        imgui::WindowFlags::NO_TITLE_BAR
                            | imgui::WindowFlags::NO_RESIZE
                            | imgui::WindowFlags::NO_MOVE
                            | imgui::WindowFlags::NO_SCROLLBAR
                            | imgui::WindowFlags::ALWAYS_AUTO_RESIZE
                            | imgui::WindowFlags::NO_SAVED_SETTINGS
                            | imgui::WindowFlags::NO_FOCUS_ON_APPEARING
                            | imgui::WindowFlags::NO_NAV_INPUTS
                            | imgui::WindowFlags::NO_NAV_FOCUS
                            | imgui::WindowFlags::NO_MOUSE_INPUTS,
                    )
                    .build(|| {
                        // Line 1: Colored FPS indicator + instantaneous frame time
                        let fps_color = if stats.fps >= 58.0 {
                            [0.25, 0.95, 0.35, 1.0]
                        } else if stats.fps >= 28.0 {
                            [0.95, 0.85, 0.2, 1.0]
                        } else {
                            [0.95, 0.3, 0.2, 1.0]
                        };
                        ui.text_colored(fps_color, format!("{:.1} FPS", stats.fps));
                        ui.same_line();
                        ui.text_disabled(format!("({:.2} ms)", stats.frame_time_ms));
                        ui.same_line();
                        ui.text_disabled(format!("|  1% Low: {:.1}  |  Avg: {:.1}", stats.p1_low_fps, stats.avg_fps));

                        // Line 2: Frame-time plot history
                        let (vals, count) = ordered_times;
                        if count > 1 {
                            ui.plot_lines_config("##frametime", &vals[..count])
                                .scale_min(0.0)
                                .scale_max(35.0)
                                .graph_size([250.0, 40.0])
                                .overlay_text(format!("{:.1} ms", stats.frame_time_ms))
                                .build();
                        }

                        // Line 3 & 4: Hardware & Presentation statistics
                        ui.text_disabled(format!(
                            "GPU: {:.1} ms   Submit Wait: {:.1} ms   VSync Wait: {:.1} ms",
                            stats.gpu_time_ms, stats.submit_wait_ms, stats.vsync_wait_ms
                        ));
                        ui.text_disabled(format!(
                            "Submits: {}   Draws: {}   Passes: {}   Memory: {} MB",
                            stats.submits, stats.draws, stats.passes, stats.memory_mb
                        ));
                    });
            }

            let pending = frame.render(state.renderer.consumer());
            let _ = state.renderer.render(pending, cb, target, None);
        });
    }
}

#[cfg(target_os = "macos")]
pub use render_impl::render;

#[cfg(not(target_os = "macos"))]
pub fn render(
    _device: &objc2::runtime::ProtocolObject<dyn objc2_metal::MTLDevice>,
    _cb: &objc2::runtime::ProtocolObject<dyn objc2_metal::MTLCommandBuffer>,
    _target: &objc2::runtime::ProtocolObject<dyn objc2_metal::MTLTexture>,
) {}
