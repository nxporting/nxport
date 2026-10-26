// The builder's window on Windows: SDL3 (the SDL3.dll the engine links against, vendor/SDL3) with Dear ImGui drawn
// through SDL_Renderer. The renderer and the input translation are here rather than in a backend crate: those build
// SDL from source with CMake, which a Windows machine building nxport need not have. Textures (the font atlas pages)
// are created and updated as Dear ImGui requests them, as the Metal renderer on macOS does (host/ui/renderer.rs).

use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use dear_imgui_rs as imgui;
use imgui::render::{DrawCmd, PendingFrame, SnapshotTextureId, SynchronousRendererConsumer, TextureOp};
use imgui::{BackendFlags, TextureFormat, TextureId};
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::sys::pixels::{SDL_FColor, SDL_PIXELFORMAT_RGBA32};
use sdl3::sys::render::{self as sr, SDL_Renderer, SDL_Texture, SDL_TEXTUREACCESS_STATIC};
use sdl3::sys::rect::SDL_Rect;

pub use nxport::settings::edit::{put_path, request_path, take_path, take_request, PathKind};

pub type Frame = Box<dyn FnMut(&imgui::Ui) -> bool>;

/// The window's background, as the macOS window clears it.
const CLEAR: sdl3::pixels::Color = sdl3::pixels::Color::RGB(23, 23, 26);

fn load_font(ctx: &mut imgui::Context, points: f32) {
    let paths = ["C:\\Windows\\Fonts\\segoeui.ttf", "C:\\Windows\\Fonts\\arial.ttf"];
    let source = paths
        .iter()
        .find_map(|p| imgui::StbTrueTypeFontData::from_file(p).ok())
        .map(imgui::FontSource::stb_truetype)
        .unwrap_or_else(imgui::FontSource::default_font);
    ctx.font_atlas().add_font(&[source]);
    ctx.style_mut().set_font_size_base(points);
}

fn new_context() -> imgui::Context {
    let mut ctx = imgui::Context::create();
    let _ = ctx.set_ini_filename(None::<String>);
    load_font(&mut ctx, 15.0);
    ctx
}

/// Dear ImGui draw data on an SDL_Renderer.
struct Renderer {
    renderer: *mut SDL_Renderer,
    consumer: SynchronousRendererConsumer,
    /// Dear ImGui's textures by its snapshot id, with the id draw commands name them by.
    textures: HashMap<SnapshotTextureId, (TextureId, *mut SDL_Texture)>,
    by_id: HashMap<u64, *mut SDL_Texture>,
    next_id: u64,
}

impl Renderer {
    fn new(renderer: *mut SDL_Renderer, ctx: &mut imgui::Context) -> Result<Self, String> {
        ctx.set_renderer_name(Some("nxport-sdl-renderer")).map_err(|e| e.to_string())?;
        let mut flags = ctx.io().backend_flags();
        flags.insert(BackendFlags::RENDERER_HAS_TEXTURES);
        flags.insert(BackendFlags::RENDERER_HAS_VTX_OFFSET);
        ctx.io_mut().set_backend_flags(flags);
        let consumer = ctx.create_synchronous_renderer_consumer().map_err(|e| e.to_string())?;
        Ok(Renderer { renderer, consumer, textures: HashMap::new(), by_id: HashMap::new(), next_id: 1 })
    }

    fn consumer(&self) -> &SynchronousRendererConsumer {
        &self.consumer
    }

    /// Applies the frame's texture requests, then draws it (in points, `scale` pixels each).
    fn render(&mut self, frame: PendingFrame<'_>, scale: f32) -> Result<(), String> {
        let mut feedback = Vec::with_capacity(frame.texture_requests().len());
        for request in frame.texture_requests() {
            let fb = match request.operation() {
                TextureOp::Create { format, width, height, row_pitch, pixels } => {
                    let tex = unsafe {
                        sr::SDL_CreateTexture(self.renderer, SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STATIC, *width as i32, *height as i32)
                    };
                    if tex.is_null() {
                        return Err(format!("imgui texture: {}", sdl3::get_error()));
                    }
                    unsafe { sr::SDL_SetTextureBlendMode(tex, sdl3::sys::blendmode::SDL_BLENDMODE_BLEND) };
                    upload(tex, *format, 0, 0, *width as usize, *height as usize, *row_pitch, pixels);
                    let id = TextureId::new(self.next_id);
                    self.next_id += 1;
                    self.by_id.insert(id.id(), tex);
                    if let Some((old, t)) = self.textures.insert(request.texture(), (id, tex)) {
                        self.by_id.remove(&old.id());
                        unsafe { sr::SDL_DestroyTexture(t) };
                    }
                    request.uploaded(id).map_err(|e| e.to_string())?
                }
                TextureOp::Update { format, rects, .. } => match self.textures.get(&request.texture()) {
                    Some(&(id, tex)) => {
                        for r in rects {
                            let rc = r.rect;
                            upload(tex, *format, rc.x as usize, rc.y as usize, rc.w as usize, rc.h as usize, r.row_pitch, &r.data);
                        }
                        request.uploaded(id).map_err(|e| e.to_string())?
                    }
                    None => request.retry(),
                },
                TextureOp::Destroy => {
                    if let Some((id, t)) = self.textures.remove(&request.texture()) {
                        self.by_id.remove(&id.id());
                        unsafe { sr::SDL_DestroyTexture(t) };
                    }
                    request.destroyed().map_err(|e| e.to_string())?
                }
            };
            feedback.push(fb);
        }
        let frame = frame.reconcile_texture_feedback(feedback).map_err(|e| e.to_string())?;
        self.draw(frame.draw_data(), scale);
        Ok(())
    }

    fn draw(&self, draw: &imgui::render::DrawData, scale: f32) {
        let [ox, oy] = draw.display_pos();
        let mut nearest = false;
        for list in draw.draw_lists() {
            let (vtx, idx) = (list.vtx_buffer(), list.idx_buffer());
            // SDL_RenderGeometryRaw takes positions, float colours and UVs as separate streams: the positions in
            // pixels, the colours unpacked from Dear ImGui's RGBA bytes.
            let xy: Vec<[f32; 2]> = vtx.iter().map(|v| [(v.pos[0] - ox) * scale, (v.pos[1] - oy) * scale]).collect();
            let col: Vec<SDL_FColor> = vtx
                .iter()
                .map(|v| {
                    let c = v.col.to_le_bytes();
                    SDL_FColor { r: c[0] as f32 / 255.0, g: c[1] as f32 / 255.0, b: c[2] as f32 / 255.0, a: c[3] as f32 / 255.0 }
                })
                .collect();
            for cmd in list.commands() {
                let (count, p) = match cmd {
                    DrawCmd::Elements { count, cmd_params } => (count, cmd_params),
                    DrawCmd::SetSamplerNearest => {
                        nearest = true;
                        continue;
                    }
                    DrawCmd::SetSamplerLinear | DrawCmd::SetSamplerFromTexture | DrawCmd::ResetRenderState => {
                        nearest = false;
                        continue;
                    }
                    DrawCmd::RawCallback(_) => continue,
                };
                let Some(&tex) = self.by_id.get(&p.texture_id.id()) else { continue };
                let [x0, y0, x1, y1] = p.clip_rect;
                let (x0, y0) = (((x0 - ox) * scale).max(0.0), ((y0 - oy) * scale).max(0.0));
                let (x1, y1) = ((x1 - ox) * scale, (y1 - oy) * scale);
                if x1 <= x0 || y1 <= y0 || p.vtx_offset >= vtx.len() {
                    continue;
                }
                let clip = SDL_Rect { x: x0 as i32, y: y0 as i32, w: (x1 - x0) as i32, h: (y1 - y0) as i32 };
                let mode = if nearest { sdl3::sys::surface::SDL_SCALEMODE_NEAREST } else { sdl3::sys::surface::SDL_SCALEMODE_LINEAR };
                let v = p.vtx_offset;
                let uv = vtx[v..].as_ptr() as *const f32;
                unsafe {
                    sr::SDL_SetRenderClipRect(self.renderer, &clip);
                    sr::SDL_SetTextureScaleMode(tex, mode);
                    sr::SDL_RenderGeometryRaw(
                        self.renderer,
                        tex,
                        xy[v..].as_ptr() as *const f32,
                        std::mem::size_of::<[f32; 2]>() as i32,
                        col[v..].as_ptr(),
                        std::mem::size_of::<SDL_FColor>() as i32,
                        // DrawVert is {pos, uv, col}: the UVs start 8 bytes into each vertex.
                        uv.add(2),
                        std::mem::size_of::<imgui::render::DrawVert>() as i32,
                        (vtx.len() - v) as i32,
                        idx[p.idx_offset..].as_ptr() as *const std::ffi::c_void,
                        count as i32,
                        std::mem::size_of::<imgui::render::DrawIdx>() as i32,
                    );
                }
            }
        }
        unsafe { sr::SDL_SetRenderClipRect(self.renderer, std::ptr::null()) };
    }
}

impl Drop for Renderer {
    fn drop(&mut self) {
        for (_, (_, t)) in self.textures.drain() {
            unsafe { sr::SDL_DestroyTexture(t) };
        }
    }
}

/// Writes a w x h region at (x, y). Alpha8 data becomes white with that alpha, so both formats draw alike.
#[allow(clippy::too_many_arguments)]
fn upload(tex: *mut SDL_Texture, format: TextureFormat, x: usize, y: usize, w: usize, h: usize, row_pitch: usize, data: &[u8]) {
    if w == 0 || h == 0 {
        return;
    }
    let rgba: Vec<u8> = match format {
        TextureFormat::RGBA32 => (0..h).flat_map(|r| data[r * row_pitch..r * row_pitch + w * 4].iter().copied()).collect(),
        TextureFormat::Alpha8 => (0..h)
            .flat_map(|r| data[r * row_pitch..r * row_pitch + w].iter().flat_map(|&a| [255, 255, 255, a]))
            .collect(),
    };
    let rect = SDL_Rect { x: x as i32, y: y as i32, w: w as i32, h: h as i32 };
    unsafe { sr::SDL_UpdateTexture(tex, &rect, rgba.as_ptr() as *const std::ffi::c_void, (w * 4) as i32) };
}

/// Feeds one SDL event to Dear ImGui (positions in points: SDL's window coordinates).
fn input(io: &mut imgui::Io, event: &Event) {
    match *event {
        Event::MouseMotion { x, y, .. } => io.add_mouse_pos_event([x, y]),
        Event::MouseButtonDown { mouse_btn, x, y, .. } | Event::MouseButtonUp { mouse_btn, x, y, .. } => {
            let down = matches!(event, Event::MouseButtonDown { .. });
            let b = match mouse_btn {
                sdl3::mouse::MouseButton::Left => imgui::MouseButton::Left,
                sdl3::mouse::MouseButton::Right => imgui::MouseButton::Right,
                sdl3::mouse::MouseButton::Middle => imgui::MouseButton::Middle,
                _ => return,
            };
            io.add_mouse_pos_event([x, y]);
            io.add_mouse_button_event(b, down);
        }
        Event::MouseWheel { x, y, .. } => io.add_mouse_wheel_event([-x, y]),
        Event::TextInput { ref text, .. } => text.chars().filter(|c| !c.is_control()).for_each(|c| io.add_input_character(c)),
        Event::KeyDown { keycode, keymod, .. } | Event::KeyUp { keycode, keymod, .. } => {
            let down = matches!(event, Event::KeyDown { .. });
            io.add_key_event(imgui::Key::ModShift, keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD));
            io.add_key_event(imgui::Key::ModCtrl, keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD));
            io.add_key_event(imgui::Key::ModAlt, keymod.intersects(Mod::LALTMOD | Mod::RALTMOD));
            io.add_key_event(imgui::Key::ModSuper, keymod.intersects(Mod::LGUIMOD | Mod::RGUIMOD));
            if let Some(k) = keycode.and_then(key_for) {
                io.add_key_event(k, down);
            }
        }
        Event::Window { win_event: sdl3::event::WindowEvent::FocusLost, .. } => io.add_focus_event(false),
        Event::Window { win_event: sdl3::event::WindowEvent::FocusGained, .. } => io.add_focus_event(true),
        Event::Window { win_event: sdl3::event::WindowEvent::MouseLeave, .. } => io.add_mouse_pos_event([-f32::MAX, -f32::MAX]),
        _ => {}
    }
}

/// The keys Dear ImGui's widgets use (navigation, editing, and the letters of its shortcuts: select all, copy, paste).
fn key_for(k: Keycode) -> Option<imgui::Key> {
    use imgui::Key as K;
    Some(match k {
        Keycode::Tab => K::Tab,
        Keycode::Left => K::LeftArrow,
        Keycode::Right => K::RightArrow,
        Keycode::Up => K::UpArrow,
        Keycode::Down => K::DownArrow,
        Keycode::PageUp => K::PageUp,
        Keycode::PageDown => K::PageDown,
        Keycode::Home => K::Home,
        Keycode::End => K::End,
        Keycode::Insert => K::Insert,
        Keycode::Delete => K::Delete,
        Keycode::Backspace => K::Backspace,
        Keycode::Space => K::Space,
        Keycode::Return => K::Enter,
        Keycode::KpEnter => K::KeypadEnter,
        Keycode::Escape => K::Escape,
        Keycode::A => K::A,
        Keycode::C => K::C,
        Keycode::V => K::V,
        Keycode::X => K::X,
        Keycode::Y => K::Y,
        Keycode::Z => K::Z,
        _ => return None,
    })
}

/// Opens a window running `frame` every display refresh until it returns false or the window is closed.
pub fn run(title: &str, width: f64, height: f64, mut frame: Frame) -> Result<(), Box<dyn std::error::Error>> {
    let sdl = sdl3::init()?;
    let video = sdl.video()?;
    let window = video.window(title, width as u32, height as u32).position_centered().resizable().high_pixel_density().build()?;
    video.text_input().start(&window);
    let mut canvas = window.into_canvas();
    // Presents at the display's refresh rate instead of spinning.
    unsafe { sr::SDL_SetRenderVSync(canvas.raw(), 1) };
    let mut ctx = new_context();
    let mut renderer = Renderer::new(canvas.raw(), &mut ctx)?;
    let mut event_pump = sdl.event_pump()?;
    let mut last = Instant::now();

    'running: loop {
        for event in event_pump.poll_iter() {
            if let Event::Quit { .. } = event {
                break 'running;
            }
            input(ctx.io_mut(), &event);
        }

        if let Some((key, kind)) = take_request() {
            match kind {
                PathKind::Directory => {
                    sdl3::dialog::show_open_folder_dialog(
                        None::<&Path>,
                        false,
                        Some(canvas.window()),
                        Box::new(move |res, _| {
                            if let Some(p) = res.ok().and_then(|mut p| p.pop()) {
                                put_path(key.clone(), p);
                            }
                        }),
                    );
                }
                PathKind::File | PathKind::Either => {
                    let _ = sdl3::dialog::show_open_file_dialog(
                        &[],
                        None::<&Path>,
                        false,
                        Some(canvas.window()),
                        Box::new(move |res, _| {
                            if let Some(p) = res.ok().and_then(|mut p| p.pop()) {
                                put_path(key.clone(), p);
                            }
                        }),
                    );
                }
            }
        }

        let (w, h) = canvas.window().size();
        let scale = canvas.window().pixel_density();
        let now = Instant::now();
        let io = ctx.io_mut();
        io.set_display_size([w as f32, h as f32]);
        io.set_display_framebuffer_scale([scale, scale]);
        io.set_delta_time((now - last).as_secs_f32().max(1e-4));
        last = now;
        let f = ctx.begin_frame();
        if !frame(f.ui()) {
            break 'running;
        }
        canvas.set_draw_color(CLEAR);
        canvas.clear();
        let pending = f.render(renderer.consumer());
        if let Err(e) = renderer.render(pending, scale) {
            eprintln!("[!] UI: {e}");
        }
        canvas.present();
    }
    Ok(())
}

/// Renders `frames` frames of `frame` at `width` x `height` points, `scale` pixels each, into a hidden window and
/// returns the last one: (width, height, BGRA pixels).
pub fn snapshot(width: f64, height: f64, scale: f64, frames: usize, mut frame: Frame) -> Result<(usize, usize, Vec<u8>), String> {
    let sdl = sdl3::init().map_err(|e| e.to_string())?;
    let video = sdl.video().map_err(|e| e.to_string())?;
    let window = video
        .window("snapshot", (width * scale) as u32, (height * scale) as u32)
        .hidden()
        .build()
        .map_err(|e| e.to_string())?;
    let mut canvas = window.into_canvas();
    let mut ctx = new_context();
    let mut renderer = Renderer::new(canvas.raw(), &mut ctx)?;
    let mut last_surface = None;
    for _ in 0..frames.max(1) {
        let io = ctx.io_mut();
        io.set_display_size([width as f32, height as f32]);
        io.set_display_framebuffer_scale([scale as f32, scale as f32]);
        io.set_delta_time(1.0 / 60.0);
        let f = ctx.begin_frame();
        frame(f.ui());
        canvas.set_draw_color(CLEAR);
        canvas.clear();
        let pending = f.render(renderer.consumer());
        renderer.render(pending, scale as f32)?;
        last_surface = Some(canvas.read_pixels(None).map_err(|e| e.to_string())?);
    }
    let surface = last_surface.ok_or("no snapshot surface")?;
    let converted = surface.convert_format(sdl3::pixels::PixelFormat::BGRA32).map_err(|e| e.to_string())?;
    let (w, h) = (converted.width() as usize, converted.height() as usize);
    let bytes = converted.with_lock(|data| data.to_vec());
    Ok((w, h, bytes))
}
