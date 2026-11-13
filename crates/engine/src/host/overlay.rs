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
// iOS has no menu bar: its settings screen sets some of these at launch, and the rest go unused there.
#![cfg_attr(target_os = "ios", allow(dead_code))]

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use crate::settings::FpsMode;

static FPS_MODE: AtomicU8 = AtomicU8::new(1); // 0 = Stock30, 1 = Unlocked
static VSYNC: AtomicBool = AtomicBool::new(true);
static LOW_LATENCY: AtomicBool = AtomicBool::new(false);
static ASYNC_SHADERS: AtomicBool = AtomicBool::new(true);
static INITIALIZED: AtomicBool = AtomicBool::new(false);

pub use crate::perf_stats::*;

pub fn init() {
    if INITIALIZED.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(s) = crate::settings::try_get() {
        let fps_val = if s.system.fps == FpsMode::Stock30 { 0 } else { 1 };
        FPS_MODE.store(fps_val, Ordering::Relaxed);
        VSYNC.store(!s.display.no_vsync, Ordering::Relaxed);
        LOW_LATENCY.store(s.display.low_latency, Ordering::Relaxed);
        ASYNC_SHADERS.store(s.graphics.async_pipelines, Ordering::Relaxed);
    }
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

pub fn is_vsync() -> bool {
    init();
    VSYNC.load(Ordering::Relaxed)
}

/// Whether at most one finished frame may wait for the screen (settings display.low_latency; Vulkan window.rs).
pub fn is_low_latency() -> bool {
    init();
    LOW_LATENCY.load(Ordering::Relaxed)
}

pub fn toggle_low_latency() -> bool {
    init();
    !LOW_LATENCY.fetch_xor(true, Ordering::Relaxed)
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
// The overlays on Metal (macOS and iOS): the performance window and the shader progress bar, drawn with Dear ImGui
// ---------------------------------------------------------------------------

#[cfg(any(target_os = "macos", target_os = "ios"))]
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
        // Taken at every present, drawn or not: the frame's skipped draws start again from zero.
        let progress = crate::gpu::pipeline_list::take_frame_progress();
        let perf = is_overlay_enabled();
        if !perf && progress.is_none() {
            return;
        }

        let (stats, buf, n) = snapshot();
        let ordered_times = (buf, n);

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
                if perf {
                    draw_window(ui, w, &stats, &ordered_times.0[..ordered_times.1]);
                }
                if let Some(listed) = progress {
                    crate::perf_stats::draw_shader_progress(ui, w, h, listed);
                }
            }

            let pending = frame.render(state.renderer.consumer());
            let _ = state.renderer.render(pending, cb, target, None);
        });
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use render_impl::render;

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
pub fn render(
    _device: &objc2::runtime::ProtocolObject<dyn objc2_metal::MTLDevice>,
    _cb: &objc2::runtime::ProtocolObject<dyn objc2_metal::MTLCommandBuffer>,
    _target: &objc2::runtime::ProtocolObject<dyn objc2_metal::MTLTexture>,
) {}
