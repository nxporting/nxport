// The game window's menu bar (Win32's: SDL has none), drawn from the menus every desktop host shares (host/menu.rs).
// Its commands reach the window procedure while SDL pumps messages, on the main thread: the procedure is wrapped to see
// them, and what needs the SDL window is handed to the main loop (take_request). SDL swallows the Alt and F10 menu
// activation, so game keys never open the menu.

use std::ffi::c_void;
use std::sync::atomic::{AtomicIsize, AtomicU32, AtomicUsize, Ordering};

use crate::host::input;
use crate::host::live;
use crate::host::menu::{self, id, LIMITS};

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

/// The menus of menu::bar as Win32 popups; the shortcuts shown beside items are Windows' own (F11, F9).
unsafe fn fill(menu_handle: *mut c_void, entries: &[menu::Entry]) {
    for e in entries {
        match e {
            menu::Entry::Item { label, id, choice } => {
                let text = match *id {
                    id::FULL_SCREEN => format!("{label}\tF11"),
                    id::CAPTURE => format!("{label}\tF9"),
                    _ => label.clone(),
                };
                if *choice {
                    radio(menu_handle, *id, &text);
                } else {
                    item(menu_handle, *id, &text);
                }
            }
            menu::Entry::Note(text) => {
                AppendMenuW(menu_handle, MF_GRAYED, 0, wide(text).as_ptr());
            }
            menu::Entry::Separator => separator(menu_handle),
            menu::Entry::Sub(label, inner) => sub(menu_handle, label, |m| fill(m, inner)),
            // No Controls window here: each input in turn, set by the next key, mouse or controller button.
            menu::Entry::Controls => sub(menu_handle, "Controls", |l| {
                for (i, s) in input::SWITCH_INPUTS.iter().enumerate() {
                    item(l, id::BIND + i as u16, &menu::bind_label(*s));
                }
                separator(l);
                item(l, id::RESET_CONTROLS, "Reset to Defaults");
            }),
        }
    }
}

pub fn attach(hwnd: usize) {
    unsafe {
        let bar = CreateMenu();
        fill(bar, &menu::bar());
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
    // Every command the menus hold (menu::bar), the bindings' own ids aside.
    for id in menu::ids(&menu::bar()) {
        check(id);
    }
    for fps in LIMITS {
        let id = id::LIMIT + fps as u16;
        ModifyMenuW(m, id as u32, MFT_RADIOCHECK, id as usize, wide(&menu::limit_item_label(fps)).as_ptr());
        check(id);
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
