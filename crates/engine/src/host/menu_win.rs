// The game window's menu bar (Win32's: SDL has none), drawn from the menus every desktop host shares (host/menu.rs).
// Its commands reach the window procedure while SDL pumps messages, on the main thread: the procedure is wrapped to see
// them, and what needs the SDL window is handed to the main loop (take_request). SDL swallows the Alt and F10 menu
// activation, so game keys never open the menu.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicUsize, Ordering};

use crate::host::input;
use crate::host::live;
use crate::host::menu::{self, id, LIMITS, SIZES, STRENGTHS, VOLUMES};

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
            item(m, id::LOW_LATENCY, "Low Latency");
            item(m, id::PERF_OVERLAY, "Performance Overlay");
            sub(m, "Frame Rate Limit", |l| {
                for fps in LIMITS {
                    radio(l, id::LIMIT + fps as u16, &menu::limit_label(fps));
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
            item(m, id::MOUSE_LOOK, "Mouse Look (F8 frees the mouse)");
            separator(m);
            sub(m, "Controls", |l| {
                for (i, s) in input::SWITCH_INPUTS.iter().enumerate() {
                    item(l, id::BIND + i as u16, &menu::bind_label(*s));
                }
                separator(l);
                item(l, id::RESET_CONTROLS, "Reset to Defaults");
            });
        });
        sub(bar, &menu::title_label(), |m| {
            if crate::profile::is_totk() {
                item(m, id::REAL_TIME, "Variable Frame Rate");
                item(m, id::DYNAMIC_ASPECT, "Dynamic Aspect Ratio");
                item(m, id::EXACT_MOUSE_AIM, "Exact Mouse Aim (Experimental)");
                separator(m);
            }
            sub(m, "Mods", |l| {
                let list = menu::mods();
                if list.is_empty() {
                    AppendMenuW(l, MF_GRAYED, 0, wide("None").as_ptr());
                }
                for (name, code) in list {
                    AppendMenuW(l, MF_GRAYED, 0, wide(&menu::mod_label(&name, code)).as_ptr());
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

/// Check marks, greyed items and labels from the current values (as the macOS menu validates its items when it opens).
unsafe fn refresh(m: *mut c_void) {
    let check = |id: u16| {
        if let Some(on) = menu::checked(id) {
            CheckMenuItem(m, id as u32, if on { MF_CHECKED } else { 0 });
        }
        EnableMenuItem(m, id as u32, if menu::enabled(id) { 0 } else { MF_GRAYED });
    };
    for id in [
        id::DOCKED, id::HANDHELD, id::VSYNC, id::LOW_LATENCY, id::FPS_TITLE, id::PERF_OVERLAY, id::ASYNC_SHADERS, id::MUTE,
        id::BY_LABEL, id::BY_POSITION, id::VIBRATION, id::TOUCH, id::REAL_TIME, id::DYNAMIC_ASPECT, id::MOUSE_LOOK,
        id::EXACT_MOUSE_AIM,
    ] {
        check(id);
    }
    for fps in LIMITS {
        let id = id::LIMIT + fps as u16;
        ModifyMenuW(m, id as u32, MFT_RADIOCHECK, id as usize, wide(&menu::limit_item_label(fps)).as_ptr());
        check(id);
    }
    for v in VOLUMES {
        check(id::VOLUME + v as u16);
    }
    for v in STRENGTHS {
        check(id::STRENGTH + v as u16);
    }
    for (i, s) in input::SWITCH_INPUTS.iter().enumerate() {
        let id = id::BIND + i as u16;
        ModifyMenuW(m, id as u32, 0, id as usize, wide(&menu::bind_label(*s)).as_ptr());
    }
}

fn command(cmd: u16) -> bool {
    match menu::command(cmd) {
        Ok(handled) => handled,
        Err(menu::Action::FullScreen) => {
            REQUEST.store(1, Ordering::Relaxed);
            true
        }
        Err(menu::Action::Size(h)) => {
            REQUEST.store(h, Ordering::Relaxed);
            true
        }
        // The next key, mouse button or controller button in the window sets it (host/display_sdl.rs).
        Err(menu::Action::Bind(i)) => {
            input::begin_capture(i);
            true
        }
        Err(menu::Action::Controls) => false,
    }
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
