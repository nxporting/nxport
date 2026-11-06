// The game window's menu bar (Win32's: SDL has none). Its menus follow the layers of a Switch title running here, so
// where an option is says what it changes:
//
//   nxport    the app: folders, restart, quit
//   System    what Horizon tells the title about the console (am: docked or handheld)
//   Display   how frames reach the screen (vi, and the host's window and swapchain): vsync, the frame-rate limit
//   Graphics  the GPU library the title calls (nvn), done with Vulkan: shader compilation
//   Audio     audren / audout output on the host's device
//   Input     hid: the controller layout the title sees, vibration, the touch screen
//   <title>   the running title's own code and data: nxport's patches for it, its mods, its save data
//   Tools     diagnostics
//
// Switches apply at once and are kept in the game's settings file (host/live.rs, config::remember). Its commands reach
// the window procedure while SDL pumps messages, on the main thread: the procedure is wrapped to see them, and what needs
// the SDL window is handed to the main loop (take_request). SDL swallows the Alt and F10 menu activation, so game keys
// never open the menu.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicUsize, Ordering};

use crate::host::{live, overlay};
use crate::settings::FpsMode;

#[link(name = "user32")]
extern "system" {
    fn CreateMenu() -> *mut c_void;
    fn CreatePopupMenu() -> *mut c_void;
    fn AppendMenuW(menu: *mut c_void, flags: u32, id: usize, text: *const u16) -> i32;
    fn CheckMenuItem(menu: *mut c_void, id: u32, flags: u32) -> u32;
    fn ModifyMenuW(menu: *mut c_void, at: u32, flags: u32, id: usize, text: *const u16) -> i32;
    fn EnableMenuItem(menu: *mut c_void, id: u32, flags: u32) -> i32;
    fn SetMenu(hwnd: usize, menu: *mut c_void) -> i32;
    fn DrawMenuBar(hwnd: usize) -> i32;
    fn SetWindowLongPtrW(hwnd: usize, index: i32, value: isize) -> isize;
    fn CallWindowProcW(prev: isize, hwnd: usize, msg: u32, wparam: usize, lparam: isize) -> isize;
}

const WM_COMMAND: u32 = 0x0111;
const WM_INITMENUPOPUP: u32 = 0x0117;
const WM_ENTERMENULOOP: u32 = 0x0211;
const GWLP_WNDPROC: i32 = -4;
const MF_POPUP: u32 = 0x10;
const MF_SEPARATOR: u32 = 0x800;
const MF_CHECKED: u32 = 8;
const MF_GRAYED: u32 = 1;
const MFT_RADIOCHECK: u32 = 0x200;

/// Menu command ids. Choices of one value are a base plus the value (frame-rate limit + fps, volume + percent, ...).
mod id {
    pub const DATA_FOLDER: u16 = 1;
    pub const RESTART: u16 = 2;
    pub const QUIT: u16 = 3;
    pub const DOCKED: u16 = 10;
    pub const HANDHELD: u16 = 11;
    pub const VSYNC: u16 = 20;
    pub const FULL_SCREEN: u16 = 21;
    pub const FPS_TITLE: u16 = 22;
    pub const PERF_OVERLAY: u16 = 23;
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
    /// + the strength in percent.
    pub const STRENGTH: u16 = 500;
    pub const REAL_TIME: u16 = 60;
    pub const DYNAMIC_ASPECT: u16 = 62;
    pub const SAVE_FOLDER: u16 = 61;
    pub const CAPTURE: u16 = 70;
    pub const LOG: u16 = 71;
}

const LIMITS: [u32; 5] = [30, 60, 90, 120, 144];
const SIZES: [u32; 3] = [720, 1080, 1440];
const VOLUMES: [u32; 4] = [25, 50, 75, 100];
const STRENGTHS: [u32; 3] = [50, 100, 150];

static PREVIOUS: AtomicIsize = AtomicIsize::new(0);
static BAR: AtomicUsize = AtomicUsize::new(0);
/// What the main loop is asked to do with the SDL window (Request as u32).
static REQUEST: AtomicU32 = AtomicU32::new(0);
/// What only the main loop can do: it owns the SDL window.
pub enum Request {
    FullScreen,
    /// A window of this height (16:9, unless the game renders at the window's shape).
    Size(u32),
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

unsafe fn item(menu: *mut c_void, id: u16, text: &str) {
    AppendMenuW(menu, 0, id as usize, wide(text).as_ptr());
}

unsafe fn radio(menu: *mut c_void, id: u16, text: &str) {
    AppendMenuW(menu, MFT_RADIOCHECK, id as usize, wide(text).as_ptr());
}

unsafe fn separator(menu: *mut c_void) {
    AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
}

unsafe fn sub(parent: *mut c_void, text: &str, fill: impl FnOnce(*mut c_void)) {
    let m = CreatePopupMenu();
    fill(m);
    AppendMenuW(parent, MF_POPUP, m as usize, wide(text).as_ptr());
}

/// The running title as the menu names it: its name and version ("Tears of the Kingdom 1.1.2").
fn title_label() -> String {
    let p = crate::profile::active();
    let name = p.name.strip_prefix("The Legend of Zelda: ").unwrap_or(p.name);
    match crate::profile::active_version() {
        Some(v) => format!("{name} {}", v.version_string),
        None => name.to_string(),
    }
}

/// The app's mods (one folder each, or one mod's romfs/exefs at the top), and whether each has code (ExeFS: lifted into
/// the app when it was built, so it changes only with a new build).
fn mods() -> Vec<(String, bool)> {
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

pub fn attach(hwnd: usize) {
    unsafe {
        let bar = CreateMenu();
        sub(bar, "nxport", |m| {
            item(m, id::DATA_FOLDER, "Open Game Data Folder");
            separator(m);
            item(m, id::RESTART, "Restart");
            item(m, id::QUIT, "Quit");
        });
        sub(bar, "System", |m| {
            radio(m, id::DOCKED, "Docked (1080p)");
            radio(m, id::HANDHELD, "Handheld (720p)");
        });
        sub(bar, "Display", |m| {
            item(m, id::VSYNC, "VSync");
            item(m, id::PERF_OVERLAY, "Performance Overlay");
            sub(m, "Frame Rate Limit", |l| {
                for fps in LIMITS {
                    radio(l, id::LIMIT + fps as u16, &limit_label(fps));
                }
            });
            separator(m);
            item(m, id::FULL_SCREEN, "Full Screen\tF11");
            sub(m, "Window Size", |l| {
                for h in SIZES {
                    item(l, id::SIZE + h as u16, &format!("{}p", h));
                }
            });
            item(m, id::FPS_TITLE, "Frame Rate in Title Bar");
        });
        sub(bar, "Graphics", |m| {
            item(m, id::ASYNC_SHADERS, "Asynchronous Shader Compilation");
            separator(m);
            item(m, id::SHADER_CACHE, "Open Shader Cache Folder");
        });
        sub(bar, "Audio", |m| {
            item(m, id::MUTE, "Mute");
            sub(m, "Volume", |l| {
                for v in VOLUMES {
                    radio(l, id::VOLUME + v as u16, &format!("{v}%"));
                }
            });
        });
        sub(bar, "Input", |m| {
            sub(m, "Button Layout", |l| {
                radio(l, id::BY_LABEL, "By Label (A is the button marked A)");
                radio(l, id::BY_POSITION, "By Position (as on a Switch controller)");
            });
            item(m, id::VIBRATION, "Vibration");
            sub(m, "Vibration Strength", |l| {
                for v in STRENGTHS {
                    radio(l, id::STRENGTH + v as u16, &format!("{v}%"));
                }
            });
            separator(m);
            item(m, id::TOUCH, "Touch Screen with Mouse");
        });
        sub(bar, &title_label(), |m| {
            if crate::profile::is_totk() {
                item(m, id::REAL_TIME, "Variable Frame Rate");
                item(m, id::DYNAMIC_ASPECT, "Dynamic Aspect Ratio");
                separator(m);
            }
            sub(m, "Mods", |l| {
                let list = mods();
                if list.is_empty() {
                    AppendMenuW(l, MF_GRAYED, 0, wide("None").as_ptr());
                }
                for (name, code) in list {
                    let text = if code { format!("{name} (code: set when the app was built)") } else { name };
                    AppendMenuW(l, MF_GRAYED, 0, wide(&text).as_ptr());
                }
            });
            item(m, id::SAVE_FOLDER, "Open Save Data Folder");
        });
        sub(bar, "Tools", |m| {
            item(m, id::CAPTURE, "Capture Frame History\tF9");
            item(m, id::LOG, "Open Log");
        });
        BAR.store(bar as usize, Ordering::Relaxed);
        let wrapped: unsafe extern "system" fn(usize, u32, usize, isize) -> isize = window_proc;
        PREVIOUS.store(SetWindowLongPtrW(hwnd, GWLP_WNDPROC, wrapped as usize as isize), Ordering::Release);
        SetMenu(hwnd, bar);
        DrawMenuBar(hwnd);
    }
}

/// Shows or hides the menu bar (full screen has none).
pub fn show(hwnd: usize, on: bool) {
    unsafe {
        SetMenu(hwnd, if on { BAR.load(Ordering::Relaxed) as *mut c_void } else { std::ptr::null_mut() });
        DrawMenuBar(hwnd);
    }
}

/// Whether the title bar shows the frame rate.
pub fn fps_in_title() -> bool {
    live::fps_in_title()
}

/// What the menu asked the main loop to do since the last call.
pub fn take_request() -> Option<Request> {
    match REQUEST.swap(0, Ordering::Relaxed) {
        0 => None,
        1 => Some(Request::FullScreen),
        h => Some(Request::Size(h)),
    }
}

/// Why the limit `fps` cannot be reached, if it cannot: above the title's own rate only where its game time follows real
/// time (otherwise it would run fast), and with vsync not above the display's refresh.
fn unreachable(fps: u32) -> Option<&'static str> {
    let refresh = live::refresh_rate();
    if fps > 60 && !crate::profile::real_time() {
        Some("needs Variable Frame Rate")
    } else if overlay::is_vsync() && refresh != 0 && fps > refresh + 1 {
        Some("above the display's refresh rate: VSync is on")
    } else {
        None
    }
}

fn limit_label(fps: u32) -> String {
    if fps == 30 {
        "30 fps (stock)".to_string()
    } else {
        format!("{fps} fps")
    }
}

/// The limit the menu shows: the one in effect (30 at the stock setting; with vsync, no more than the display's refresh).
fn limit() -> u32 {
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

/// Check marks and greyed items from the current values (as the macOS menu validates its items when it opens).
unsafe fn refresh(menu: *mut c_void) {
    let check = |id: u16, on: bool| {
        CheckMenuItem(menu, id as u32, if on { MF_CHECKED } else { 0 });
    };
    let enable = |id: u16, on: bool| {
        EnableMenuItem(menu, id as u32, if on { 0 } else { MF_GRAYED });
    };
    let handheld = crate::horizon::hle::is_handheld();
    check(id::DOCKED, !handheld);
    check(id::HANDHELD, handheld);
    check(id::VSYNC, overlay::is_vsync());
    check(id::FPS_TITLE, fps_in_title());
    check(id::PERF_OVERLAY, overlay::is_overlay_enabled());
    let current = limit();
    for fps in LIMITS {
        let why = unreachable(fps);
        // The label says why a limit is greyed (menus have no tooltips).
        let text = match why {
            Some(w) => format!("{} ({w})", limit_label(fps)),
            None => limit_label(fps),
        };
        const MF_BYCOMMAND_STRING_RADIO: u32 = MFT_RADIOCHECK;
        ModifyMenuW(menu, (id::LIMIT + fps as u16) as u32, MF_BYCOMMAND_STRING_RADIO, (id::LIMIT + fps as u16) as usize, wide(&text).as_ptr());
        check(id::LIMIT + fps as u16, fps == current);
        enable(id::LIMIT + fps as u16, why.is_none());
    }
    check(id::ASYNC_SHADERS, overlay::is_async_shaders());
    check(id::MUTE, live::muted());
    for v in VOLUMES {
        check(id::VOLUME + v as u16, v == live::volume());
    }
    let (ab, _) = live::swaps();
    check(id::BY_LABEL, !ab);
    check(id::BY_POSITION, ab);
    check(id::VIBRATION, live::rumble());
    for v in STRENGTHS {
        check(id::STRENGTH + v as u16, v == live::rumble_percent());
        enable(id::STRENGTH + v as u16, live::rumble());
    }
    check(id::TOUCH, live::touch());
    check(id::REAL_TIME, live::real_time_speed());
    check(id::DYNAMIC_ASPECT, live::dynamic_aspect());
}

/// Opens `dir` in Explorer.
fn open(dir: Option<std::path::PathBuf>) {
    match dir {
        Some(d) => {
            let _ = std::fs::create_dir_all(&d);
            let _ = std::process::Command::new("explorer.exe").arg(&d).spawn();
        }
        None => println!("[!] Menu: no such folder for this game"),
    }
}

fn command(cmd: u16) -> bool {
    let data = || crate::profile::data_dir().map(|d| d.to_path_buf());
    match cmd {
        id::DATA_FOLDER => open(data()),
        id::RESTART => super::restart(),
        id::QUIT => std::process::exit(0),
        id::DOCKED | id::HANDHELD => {
            let handheld = cmd == id::HANDHELD;
            crate::horizon::hle::set_handheld(handheld);
            crate::config::remember(|p| p.system.handheld = Some(handheld));
        }
        id::VSYNC => {
            let on = overlay::toggle_vsync();
            crate::config::remember(|p| p.display.no_vsync = Some(!on));
        }
        id::FULL_SCREEN => REQUEST.store(1, Ordering::Relaxed),
        id::PERF_OVERLAY => {
            let on = overlay::toggle_overlay();
            crate::config::remember(|p| p.display.perf_overlay = Some(on));
        }
        id::FPS_TITLE => {
            live::set_fps_in_title(!live::fps_in_title());
        }
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
        c if (id::SIZE..id::SIZE + 2000).contains(&c) => REQUEST.store((c - id::SIZE) as u32, Ordering::Relaxed),
        id::ASYNC_SHADERS => {
            let on = overlay::toggle_async_shaders();
            crate::config::remember(|p| p.graphics.async_pipelines = Some(on));
        }
        id::SHADER_CACHE => open(crate::settings::get().files.shader_cache.clone().or_else(|| crate::profile::data_subdir("vulkan_stages"))),
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
        id::SAVE_FOLDER => open(crate::settings::get().files.save_dir.clone().or_else(|| crate::profile::data_subdir("save"))),
        id::CAPTURE => {
            crate::gpu::history::request();
            println!("[+] Capture requested: the frame history is written at the next present");
        }
        id::LOG => open(data()),
        _ => return false,
    }
    true
}

unsafe extern "system" fn window_proc(hwnd: usize, msg: u32, wparam: usize, lparam: isize) -> isize {
    match msg {
        // A menu's command has 0 in the high word.
        WM_COMMAND if wparam >> 16 == 0 && command(wparam as u16) => return 0,
        WM_INITMENUPOPUP => refresh(wparam as *mut c_void),
        // The open menu takes the key and button releases: drop what is held, as when focus is lost.
        WM_ENTERMENULOOP => {
            crate::host::input::clear_keys();
            crate::host::input::touch_ended(0);
        }
        _ => {}
    }
    CallWindowProcW(PREVIOUS.load(Ordering::Acquire), hwnd, msg, wparam, lparam)
}
