// Window and Dear ImGui runner: AppKit on macOS (delegates to nxport::ui::window),
// SDL3 on Windows.

#[cfg(target_os = "macos")]
pub use nxport::ui::imgui;

#[cfg(target_os = "macos")]
pub use nxport::ui::window::{snapshot, Frame, PathKind, request_path, take_path};

#[cfg(target_os = "macos")]
pub fn run(title: &str, width: f64, height: f64, frame: Frame) -> Result<(), Box<dyn std::error::Error>> {
    nxport::ui::window::run(title, width, height, frame);
    Ok(())
}

#[cfg(windows)]
pub use dear_imgui_rs as imgui;

#[cfg(windows)]
pub use nxport::settings::edit::{put_path, request_path, take_path, take_request, PathKind};

#[cfg(windows)]
pub type Frame = Box<dyn FnMut(&imgui::Ui) -> bool>;

#[cfg(windows)]
use std::path::Path;

#[cfg(windows)]
fn load_font(ctx: &mut imgui::Context, points: f32) {
    let paths = [
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ];
    let source = paths
        .iter()
        .find_map(|p| imgui::StbTrueTypeFontData::from_file(p).ok())
        .map(imgui::FontSource::stb_truetype)
        .unwrap_or_else(imgui::FontSource::default_font);
    ctx.font_atlas().add_font(&[source]);
    ctx.style_mut().set_font_size_base(points);
}

#[cfg(windows)]
fn new_context() -> imgui::Context {
    let mut ctx = imgui::Context::create();
    let _ = ctx.set_ini_filename(None::<String>);
    load_font(&mut ctx, 15.0);
    ctx
}

#[cfg(windows)]
pub fn run(
    title: &str,
    width: f64,
    height: f64,
    mut frame: Frame,
) -> Result<(), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let window = video
        .window(title, width as u32, height as u32)
        .position_centered()
        .resizable()
        .build()?;
    let mut canvas = window.into_canvas();
    let mut ctx = new_context();
    let mut backend = unsafe { dear_imgui_sdl3::SdlRenderer3Backend::init(&mut ctx, &canvas)? };
    let mut event_pump = sdl.event_pump()?;

    'running: loop {
        for event in event_pump.poll_iter() {
            let _ = backend.process_event(&mut ctx, &event);
            match event {
                sdl3::event::Event::Quit { .. } => break 'running,
                _ => {}
            }
        }

        if let Some((key, kind)) = take_request() {
            let window_ref = canvas.window();
            match kind {
                PathKind::Directory => {
                    let k = key.clone();
                    sdl3::dialog::show_open_folder_dialog(
                        None::<&Path>,
                        false,
                        Some(window_ref),
                        Box::new(move |res, _| {
                            if let Ok(mut paths) = res {
                                if let Some(p) = paths.pop() {
                                    put_path(k.clone(), p);
                                }
                            }
                        }),
                    );
                }
                PathKind::File | PathKind::Either => {
                    let k = key.clone();
                    let _ = sdl3::dialog::show_open_file_dialog(
                        &[],
                        None::<&Path>,
                        false,
                        Some(window_ref),
                        Box::new(move |res, _| {
                            if let Ok(mut paths) = res {
                                if let Some(p) = paths.pop() {
                                    put_path(k.clone(), p);
                                }
                            }
                        }),
                    );
                }
            }
        }

        backend.new_frame(&mut ctx)?;
        let f = ctx.begin_frame();
        let keep_running = frame(f.ui());
        if !keep_running {
            break 'running;
        }

        canvas.set_draw_color(sdl3::pixels::Color::RGB(30, 30, 30));
        canvas.clear();
        let pending = f.render(backend.consumer());
        backend.render(pending, &canvas)?;
        canvas.present();

        std::thread::sleep(std::time::Duration::from_millis(8));
    }

    backend.shutdown(&mut ctx)?;
    Ok(())
}

#[cfg(windows)]
pub fn snapshot(
    width: f64,
    height: f64,
    scale: f64,
    frames: usize,
    mut frame: Frame,
) -> Result<(usize, usize, Vec<u8>), String> {
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let pw = (width * scale) as u32;
    let ph = (height * scale) as u32;
    let window = video
        .window("snapshot", pw, ph)
        .hidden()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas();
    let mut ctx = new_context();
    let mut backend = unsafe {
        dear_imgui_sdl3::SdlRenderer3Backend::init(&mut ctx, &canvas).map_err(|e| e.to_string())?
    };

    let mut last_surface = None;
    for _ in 0..frames.max(1) {
        let io = ctx.io_mut();
        io.set_display_size([width as f32, height as f32]);
        io.set_display_framebuffer_scale([scale as f32, scale as f32]);
        io.set_delta_time(1.0 / 60.0);
        backend.new_frame(&mut ctx).map_err(|e| e.to_string())?;
        let f = ctx.begin_frame();
        frame(f.ui());
        canvas.set_draw_color(sdl3::pixels::Color::RGB(30, 30, 30));
        canvas.clear();
        let pending = f.render(backend.consumer());
        backend.render(pending, &canvas).map_err(|e| e.to_string())?;
        last_surface = Some(canvas.read_pixels(None).map_err(|e| e.to_string())?);
    }

    let surface = last_surface.ok_or("no snapshot surface")?;
    let converted = surface
        .convert_format(sdl3::pixels::PixelFormat::BGRA32)
        .map_err(|e| e.to_string())?;
    let (w, h) = (converted.width() as usize, converted.height() as usize);
    let bytes = converted.with_lock(|data| data.to_vec());
    backend.shutdown(&mut ctx).map_err(|e| e.to_string())?;
    Ok((w, h, bytes))
}
