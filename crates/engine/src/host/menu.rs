// The game window's menus, the same on every desktop host: what each item is, whether it is checked or available, and
// what choosing it does. The hosts draw them (host/menu_win.rs with Win32, host/display.rs with AppKit) and handle the
// few things only their window can do (Action). The menus follow the layers of a Switch title running here, so where an
// option is says what it changes:
//
//   nxport    the app: folders, restart, quit
//   System    what Horizon tells the title about the console (am: docked or handheld)
//   Display   how frames reach the screen (vi, and the host's window and swapchain): vsync, the frame-rate limit
//   Graphics  the GPU library the title calls (nvn): shader compilation
//   Audio     audren / audout output on the host's device
//   Input     hid: the controller layout the title sees, vibration, the touch screen, the key and controller bindings
//   <title>   the running title's own code and data: nxport's patches for it, its mods, its save data
//   Tools     diagnostics
//
// Switches apply at once and are kept in the game's settings file (host/live.rs, config::remember).

use crate::host::{input, live, overlay};
use crate::settings::FpsMode;

/// Menu command ids. Choices of one value are a base plus the value (frame-rate limit + fps, volume + percent, ...).
pub mod id {
    pub const DATA_FOLDER: u16 = 1;
    pub const RESTART: u16 = 2;
    pub const QUIT: u16 = 3;
    pub const DOCKED: u16 = 10;
    pub const HANDHELD: u16 = 11;
    pub const VSYNC: u16 = 20;
    pub const FULL_SCREEN: u16 = 21;
    pub const FPS_TITLE: u16 = 22;
    pub const PERF_OVERLAY: u16 = 23;
    pub const LOW_LATENCY: u16 = 24;
    /// + the limit in fps.
    pub const LIMIT: u16 = 100;
    /// + the window height (720, 1080, 1440).
    pub const SIZE: u16 = 1000;
    pub const ASYNC_SHADERS: u16 = 30;
    pub const SHADER_CACHE: u16 = 31;
    pub const MUTE: u16 = 40;
    /// + the volume in percent.
    pub const VOLUME: u16 = 300;
    pub const BY_LABEL: u16 = 50;
    pub const BY_POSITION: u16 = 51;
    pub const VIBRATION: u16 = 52;
    pub const TOUCH: u16 = 53;
    pub const MOUSE_LOOK: u16 = 54;
    /// + the strength in percent.
    pub const STRENGTH: u16 = 500;
    pub const REAL_TIME: u16 = 60;
    pub const DYNAMIC_ASPECT: u16 = 62;
    pub const EXACT_MOUSE_AIM: u16 = 63;
    pub const SAVE_FOLDER: u16 = 61;
    pub const CAPTURE: u16 = 70;
    pub const LOG: u16 = 71;
    /// + the input's index in input::SWITCH_INPUTS. Clear of the ranges above (STRENGTH's covers 500..=700: at 600 these
    /// were taken for vibration strengths, and no click started a capture).
    pub const BIND: u16 = 800;
    pub const RESET_CONTROLS: u16 = 900;
    /// The Controls window (macOS; Windows lists the bindings in the menu instead).
    pub const CONTROLS: u16 = 901;
}

pub const LIMITS: [u32; 5] = [30, 60, 90, 120, 144];
pub const SIZES: [u32; 3] = [720, 1080, 1440];
pub const VOLUMES: [u32; 4] = [25, 50, 75, 100];
pub const STRENGTHS: [u32; 3] = [50, 100, 150];

/// What only the host's window can do.
pub enum Action {
    FullScreen,
    /// A window of this height (16:9, unless the game renders at the window's shape).
    Size(u32),
    /// Start setting the binding of SWITCH_INPUTS[i] from the next key, mouse or controller button (Windows; macOS opens
    /// the Controls window).
    Bind(#[cfg_attr(not(windows), allow(dead_code))] usize),
    /// Show the Controls window.
    Controls,
}

/// The running title as the menu names it: its name and version ("Tears of the Kingdom 1.1.2").
pub fn title_label() -> String {
    let p = crate::profile::active();
    let name = p.name.strip_prefix("The Legend of Zelda: ").unwrap_or(p.name);
    match crate::profile::active_version() {
        Some(v) => format!("{name} {}", v.version_string),
        None => name.to_string(),
    }
}

/// The app's mods (one folder each, or one mod's romfs/exefs at the top), and whether each has code (ExeFS: lifted into
/// the app when it was built on Windows, so it changes only with a new build there; loaded at start natively).
pub fn mods() -> Vec<(String, bool)> {
    let s = crate::settings::get();
    let Some(dir) = s.files.mods_dir.as_ref().filter(|_| !s.files.no_mods) else { return Vec::new() };
    let has_code = |d: &std::path::Path| ["exefs", "ExeFS"].iter().any(|e| d.join(e).is_dir()) || crate::mods::has_exefs_nsp(d);
    if crate::mods::is_mod_dir(dir) {
        let name = dir.file_name().map_or_else(|| "Mod".to_string(), |n| n.to_string_lossy().into_owned());
        return vec![(name, has_code(dir))];
    }
    let mut list: Vec<(String, bool)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| (e.file_name().to_string_lossy().into_owned(), has_code(&e.path())))
        .collect();
    list.sort();
    list
}

/// A mod as the Mods menu lists it.
pub fn mod_label(name: &str, code: bool) -> String {
    if code && cfg!(windows) {
        format!("{name} (code: set when the app was built)")
    } else {
        name.to_string()
    }
}

/// Why the limit `fps` cannot be reached, if it cannot: above the title's own rate only where its game time follows real
/// time (otherwise it would run fast), and with vsync not above the display's refresh.
pub fn unreachable(fps: u32) -> Option<&'static str> {
    let refresh = live::refresh_rate();
    if fps > 60 && !crate::profile::real_time() {
        Some("needs Variable Frame Rate")
    } else if overlay::is_vsync() && refresh != 0 && fps > refresh + 1 {
        Some("above the display's refresh rate: VSync is on")
    } else {
        None
    }
}

pub fn limit_label(fps: u32) -> String {
    if fps == 30 {
        "30 fps (stock)".to_string()
    } else {
        format!("{fps} fps")
    }
}

/// A limit's label with why it is greyed, if it is (menus have no tooltips).
pub fn limit_item_label(fps: u32) -> String {
    match unreachable(fps) {
        Some(w) => format!("{} ({w})", limit_label(fps)),
        None => limit_label(fps),
    }
}

/// The limit the menu shows: the one in effect (30 at the stock setting; with vsync, no more than the display's refresh).
pub fn limit() -> u32 {
    if overlay::fps_mode() == FpsMode::Stock30 {
        return 30;
    }
    let (fps, refresh) = (live::frame_limit(), live::refresh_rate());
    if overlay::is_vsync() && refresh != 0 && fps > refresh + 1 {
        LIMITS.iter().copied().filter(|&l| l <= refresh + 1).max().unwrap_or(30)
    } else {
        fps
    }
}

/// A Controls entry: the input, then (right-aligned) what it is bound to.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn bind_label(s: input::SwitchInput) -> String {
    format!("{}\t{}", s.label(), input::binding_label(input::bindings()[s.index()]))
}

/// Whether item `cmd` shows a check mark (None: it is not a switch or a choice).
pub fn checked(cmd: u16) -> Option<bool> {
    let handheld = crate::horizon::hle::is_handheld();
    let (ab, _) = live::swaps();
    Some(match cmd {
        id::DOCKED => !handheld,
        id::HANDHELD => handheld,
        id::VSYNC => overlay::is_vsync(),
        id::LOW_LATENCY => overlay::is_low_latency(),
        id::FPS_TITLE => live::fps_in_title(),
        id::PERF_OVERLAY => overlay::is_overlay_enabled(),
        c if (id::LIMIT..id::LIMIT + 200).contains(&c) => (c - id::LIMIT) as u32 == limit(),
        id::ASYNC_SHADERS => overlay::is_async_shaders(),
        id::MUTE => live::muted(),
        c if (id::VOLUME..id::VOLUME + 101).contains(&c) => (c - id::VOLUME) as u32 == live::volume(),
        id::BY_LABEL => !ab,
        id::BY_POSITION => ab,
        id::VIBRATION => live::rumble(),
        c if (id::STRENGTH..id::STRENGTH + 201).contains(&c) => (c - id::STRENGTH) as u32 == live::rumble_percent(),
        id::TOUCH => live::touch(),
        id::REAL_TIME => live::real_time_speed(),
        id::DYNAMIC_ASPECT => live::dynamic_aspect(),
        id::MOUSE_LOOK => live::mouse_look(),
        id::EXACT_MOUSE_AIM => live::exact_mouse_aim(),
        _ => return None,
    })
}

/// Whether item `cmd` can be chosen now.
pub fn enabled(cmd: u16) -> bool {
    match cmd {
        c if (id::LIMIT..id::LIMIT + 200).contains(&c) => unreachable((c - id::LIMIT) as u32).is_none(),
        c if (id::STRENGTH..id::STRENGTH + 201).contains(&c) => live::rumble(),
        // Exact aim works through mouse look.
        id::EXACT_MOUSE_AIM => live::mouse_look(),
        _ => true,
    }
}

/// Opens `dir` in the platform's file browser.
fn open(dir: Option<std::path::PathBuf>) {
    match dir {
        Some(d) => {
            let _ = std::fs::create_dir_all(&d);
            let browser = if cfg!(windows) { "explorer.exe" } else { "open" };
            let _ = std::process::Command::new(browser).arg(&d).spawn();
        }
        None => println!("[!] Menu: no such folder for this game"),
    }
}

/// Starts this program again with the same arguments, then leaves (the guest has no clean shutdown).
pub fn restart() -> ! {
    match std::env::current_exe() {
        Ok(exe) => {
            if let Err(e) = std::process::Command::new(exe).args(std::env::args_os().skip(1)).spawn() {
                println!("[!] Restart: {e}");
            }
        }
        Err(e) => println!("[!] Restart: {e}"),
    }
    std::process::exit(0)
}

/// Does what item `cmd` says. Err: an action the host's window does; Ok(false): not a menu command.
pub fn command(cmd: u16) -> Result<bool, Action> {
    let data = || crate::profile::data_dir().map(|d| d.to_path_buf());
    match cmd {
        id::DATA_FOLDER => open(data()),
        id::RESTART => restart(),
        id::QUIT => {
            println!("[+] Display: quit requested; exiting");
            std::process::exit(0)
        }
        id::DOCKED | id::HANDHELD => {
            let handheld = cmd == id::HANDHELD;
            crate::horizon::hle::set_handheld(handheld);
            crate::config::remember(|p| p.system.handheld = Some(handheld));
        }
        id::VSYNC => {
            let on = overlay::toggle_vsync();
            crate::config::remember(|p| p.display.no_vsync = Some(!on));
        }
        id::LOW_LATENCY => {
            let on = overlay::toggle_low_latency();
            crate::config::remember(|p| p.display.low_latency = Some(on));
        }
        id::FULL_SCREEN => return Err(Action::FullScreen),
        id::PERF_OVERLAY => {
            let on = overlay::toggle_overlay();
            crate::config::remember(|p| p.display.perf_overlay = Some(on));
        }
        id::FPS_TITLE => live::set_fps_in_title(!live::fps_in_title()),
        c if (id::LIMIT..id::LIMIT + 200).contains(&c) => {
            let fps = (c - id::LIMIT) as u32;
            let mode = if fps == 30 { FpsMode::Stock30 } else { FpsMode::Unlocked };
            overlay::set_fps_mode(mode);
            crate::config::remember(|p| p.system.fps = Some(mode));
            if fps != 30 {
                live::set_frame_limit(fps);
            }
            println!("[+] Display: frame-rate limit {fps} fps");
        }
        c if (id::SIZE..id::SIZE + 2000).contains(&c) => return Err(Action::Size((c - id::SIZE) as u32)),
        id::ASYNC_SHADERS => {
            let on = overlay::toggle_async_shaders();
            crate::config::remember(|p| p.graphics.async_pipelines = Some(on));
        }
        id::SHADER_CACHE => {
            let own = if cfg!(target_vendor = "apple") && crate::settings::get().graphics.gpu != crate::settings::GpuBackend::Vulkan {
                "shader_cache"
            } else {
                "vulkan_stages"
            };
            open(crate::settings::get().files.shader_cache.clone().or_else(|| crate::profile::data_subdir(own)))
        }
        id::MUTE => live::set_muted(!live::muted()),
        c if (id::VOLUME..id::VOLUME + 101).contains(&c) => live::set_volume((c - id::VOLUME) as u32),
        id::BY_LABEL | id::BY_POSITION => {
            let p = cmd == id::BY_POSITION;
            live::set_swaps(p, p);
        }
        id::VIBRATION => live::set_rumble(!live::rumble()),
        c if (id::STRENGTH..id::STRENGTH + 201).contains(&c) => live::set_rumble_percent((c - id::STRENGTH) as u32),
        id::TOUCH => live::set_touch(!live::touch()),
        id::REAL_TIME => live::set_real_time_speed(!live::real_time_speed()),
        id::DYNAMIC_ASPECT => live::set_dynamic_aspect(!live::dynamic_aspect()),
        id::MOUSE_LOOK => live::set_mouse_look(!live::mouse_look()),
        id::EXACT_MOUSE_AIM => live::set_exact_mouse_aim(!live::exact_mouse_aim()),
        id::SAVE_FOLDER => open(crate::settings::get().files.save_dir.clone().or_else(|| crate::profile::data_subdir("save"))),
        id::CAPTURE => {
            crate::gpu::history::request();
            println!("[+] Capture requested: the frame history is written at the next present");
        }
        id::LOG => open(data()),
        c if (id::BIND..id::BIND + input::SWITCH_INPUTS.len() as u16).contains(&c) => return Err(Action::Bind((c - id::BIND) as usize)),
        id::RESET_CONTROLS => {
            input::set_bindings(input::default_bindings());
            println!("[+] Input: controls reset to the defaults");
        }
        id::CONTROLS => return Err(Action::Controls),
        _ => return Ok(false),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The id ranges `command` matches never overlap, or a click is taken for another item (the Controls items once
    /// set vibration strengths).
    #[test]
    fn menu_id_ranges_are_apart() {
        let n = input::SWITCH_INPUTS.len() as u16;
        let ranges = [
            ("limit", id::LIMIT, id::LIMIT + 200),
            ("size", id::SIZE, id::SIZE + 2000),
            ("volume", id::VOLUME, id::VOLUME + 101),
            ("strength", id::STRENGTH, id::STRENGTH + 201),
            ("bind", id::BIND, id::BIND + n),
            ("reset controls", id::RESET_CONTROLS, id::RESET_CONTROLS + 1),
            ("controls", id::CONTROLS, id::CONTROLS + 1),
        ];
        for (i, a) in ranges.iter().enumerate() {
            for b in &ranges[i + 1..] {
                assert!(a.2 <= b.1 || b.2 <= a.1, "{} and {} overlap", a.0, b.0);
            }
            for single in [id::VSYNC, id::LOW_LATENCY, id::PERF_OVERLAY, id::TOUCH, id::MOUSE_LOOK, id::EXACT_MOUSE_AIM, id::VIBRATION, id::LOG, id::CAPTURE] {
                assert!(!(a.1..a.2).contains(&single), "{} holds {single}", a.0);
            }
        }
    }
}
