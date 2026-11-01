// The game's settings window (nxport > Settings…): every setting, in the sections of the menu bar (menu_win.rs), then
// the developer ones. A change applies at once where the setting changes while the game runs (host/live.rs, as the
// menu's switches do) and is kept in the game's settings file; the others are marked "(restart)" and take effect the next
// time the game starts (a Restart button appears once one changed). Dear ImGui through the builder's SDL renderer
// (ui_sdl.rs), on the main thread: the window's events come through the game window's event loop.

use std::time::{Duration, Instant};

use dear_imgui_rs as imgui;
use struct_patch::Patch;

use crate::host::live;
use crate::settings::edit::{Audience, Section};
use crate::settings::Settings;
use crate::ui_sdl::{self, Renderer};

pub struct SettingsWindow {
    canvas: sdl3::render::WindowCanvas,
    ctx: imgui::Context,
    renderer: Renderer,
    /// What the window shows: the settings as they are, with the edits that wait for a restart.
    edited: Settings,
    /// A setting that takes effect after a restart was changed.
    restart: bool,
    drawn: Instant,
}

/// Frames a second the window is drawn at: the game's event loop draws it, and must not wait for the display.
const FRAME: Duration = Duration::from_millis(33);

impl SettingsWindow {
    pub fn open(video: &sdl3::VideoSubsystem) -> Result<Self, String> {
        let window = video
            .window(&format!("{} Settings", crate::profile::app_title()), 680, 760)
            .position_centered()
            .resizable()
            .high_pixel_density()
            .build()
            .map_err(|e| e.to_string())?;
        video.text_input().start(&window);
        let canvas = window.into_canvas();
        let mut ctx = ui_sdl::new_context();
        let renderer = Renderer::new(canvas.raw(), &mut ctx)?;
        crate::settings::edit::set_live(live::is_live);
        Ok(SettingsWindow { canvas, ctx, renderer, edited: live::current(), restart: false, drawn: Instant::now() - FRAME })
    }

    pub fn id(&self) -> u32 {
        self.canvas.window().id()
    }

    pub fn event(&mut self, e: &sdl3::event::Event) {
        ui_sdl::input(self.ctx.io_mut(), e);
    }

    pub fn raise(&mut self) {
        self.canvas.window_mut().raise();
    }

    /// Draws the window when a frame is due.
    pub fn frame(&mut self) {
        if self.drawn.elapsed() < FRAME {
            return;
        }
        let now = Instant::now();
        let dt = (now - self.drawn).as_secs_f32();
        self.drawn = now;
        // What the menu changed meanwhile.
        live::refresh(&mut self.edited);
        let (w, h) = self.canvas.window().size();
        let scale = self.canvas.window().pixel_density();
        let io = self.ctx.io_mut();
        io.set_display_size([w as f32, h as f32]);
        io.set_display_framebuffer_scale([scale, scale]);
        io.set_delta_time(dt.max(1e-4));
        let f = self.ctx.begin_frame();
        let before = self.edited.clone();
        contents(f.ui(), &mut self.edited, &mut self.restart);
        if self.edited != before {
            live::apply(&self.edited);
            crate::config::remember_patch(&self.edited.clone().into_patch_by_diff(before));
        }
        self.canvas.set_draw_color(ui_sdl::CLEAR);
        self.canvas.clear();
        let pending = f.render(self.renderer.consumer());
        if let Err(e) = self.renderer.render(pending, scale) {
            eprintln!("[!] Settings window: {e}");
        }
        self.canvas.present();
    }
}

/// The window's contents: sections in the menu bar's order, the title's own, then the developer settings.
fn contents(ui: &imgui::Ui, s: &mut Settings, restart: &mut bool) {
    let size = ui.io().display_size();
    ui.window("settings")
        .position([0.0, 0.0], imgui::Condition::Always)
        .size(size, imgui::Condition::Always)
        .flags(imgui::WindowFlags::NO_DECORATION | imgui::WindowFlags::NO_MOVE | imgui::WindowFlags::NO_SAVED_SETTINGS)
        .build(|| {
            let _w = ui.push_item_width(-220.0);
            ui.text_wrapped("Changes take effect at once and are kept for this game. Those marked (restart) take effect the next time it starts.");
            if *restart {
                ui.text_colored([1.0, 0.75, 0.3, 1.0], "Some changes take effect after a restart.");
                ui.same_line();
                if ui.button("Restart Now") {
                    crate::host::display::restart();
                }
            }
            ui.separator();
            for section in [&mut s.system as &mut dyn Section, &mut s.display, &mut s.graphics, &mut s.audio, &mut s.input] {
                if section.has(Audience::User) && ui.collapsing_header(section.heading(), imgui::TreeNodeFlags::DEFAULT_OPEN) {
                    section.edit(ui, Audience::User);
                }
            }
            let profile = crate::profile::active();
            for (title, section) in crate::profile::option_sections(profile, s) {
                if section.has(Audience::User) && ui.collapsing_header(format!("{}: {title}", profile.name), imgui::TreeNodeFlags::DEFAULT_OPEN) {
                    section.edit(ui, Audience::User);
                }
            }
            if ui.collapsing_header("Developer", imgui::TreeNodeFlags::empty()) {
                ui.text_disabled("Diagnostics, A/B switches and internals.");
                let mut all: Vec<&mut dyn Section> = vec![
                    &mut s.system, &mut s.display, &mut s.graphics, &mut s.audio, &mut s.input, &mut s.files, &mut s.guest,
                    &mut s.replay, &mut s.trace, &mut s.dump, &mut s.log, &mut s.debug,
                ];
                for section in all.iter_mut() {
                    if section.has(Audience::Developer) {
                        if let Some(_n) = ui.tree_node(section.heading()) {
                            section.edit(ui, Audience::Developer);
                        }
                    }
                }
                drop(all);
                for (title, section) in crate::profile::option_sections(profile, s) {
                    if section.has(Audience::Developer) {
                        if let Some(_n) = ui.tree_node(format!("{}: {title}", profile.name)) {
                            section.edit(ui, Audience::Developer);
                        }
                    }
                }
            }
            if crate::settings::edit::take_restart_needed() {
                *restart = true;
            }
        });
}
