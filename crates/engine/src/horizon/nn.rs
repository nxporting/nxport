// HLE for nn:: SDK entry points that would otherwise run sdk module code needing the kernel
// (svc) or a sysmodule session (IPC): nn::os Event / MultiWait / MessageQueue / reader-writer
// locks, nn::time, nn::oe, nn::pl, nn::account, nn::err, nn::diag, nn::swkbd, nn::nfp, nn::hid
// (Npad/six-axis reads) and nn::prepo.
//
// Functions are bound
// by mangled name from the loader (see `lookup`), so they override the SDK symbol table.
//
// Synchronisation objects live in side tables keyed by the guest object's address, protected by
// one global mutex + condvar. That keeps them independent of the SDK's internal layouts (we never
// need to match sizeof/offsets) and makes cross-object waits (MultiWait) trivial.

use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard};

use crate::horizon::hle::hle_system_event_register_addr;

pub(crate) const RESULT_SUCCESS: u32 = 0;

// ── global sync state ────────────────────────────────────────────────────────────────────────
#[derive(Default)]
pub(crate) struct EventSt {
    pub(crate) signaled: bool,
    pub(crate) auto_clear: bool,
    /// Signals so far: a manual-clear waiter returns once one arrived after it began, even if cleared again since.
    pub(crate) signals: u64,
}

#[derive(Default)]
pub(crate) struct MsgQ {
    pub(crate) cap: usize,
    pub(crate) q: VecDeque<u64>,
}

#[derive(Default)]
pub(crate) struct RwSt {
    readers: u32,
    writer: usize, // owning thread token, 0 = none
    write_depth: u32,
}

#[derive(Default)]
pub(crate) struct Sync {
    pub(crate) events: HashMap<usize, EventSt>,
    pub(crate) holders: HashMap<usize, usize>, // holder addr -> event addr
    pub(crate) multiwaits: HashMap<usize, Vec<usize>>, // multiwait addr -> ordered holder addrs
    pub(crate) msgqs: HashMap<usize, MsgQ>,
    pub(crate) rwlocks: HashMap<usize, RwSt>,
}

static SYNC: Mutex<Option<Sync>> = Mutex::new(None);
pub(crate) static SYNC_CV: Condvar = Condvar::new();

pub(crate) fn lock() -> MutexGuard<'static, Option<Sync>> {
    let mut g = SYNC.lock().unwrap();
    if g.is_none() {
        *g = Some(Sync::default());
    }
    g
}

fn me() -> usize {
    unsafe { crate::horizon::os::get_current_thread() as usize }
}

/// Signals an nn::os::EventType the game initialised through the HLE (for HLE'd libraries that report to the game).
pub(crate) fn signal_event(ev: usize) {
    unsafe { crate::horizon::os::signal_event(ev as *mut u8) }
}

/// Waits for the event; a signaled auto-clear event is consumed. `deadline` None = forever. Returns whether it was
/// signaled. An event nothing initialised (no side-table entry) never signals.
pub(crate) fn event_wait(ev: usize, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    let signals0 = g.as_ref().unwrap().events.get(&ev).map(|e| e.signals);
    loop {
        if let Some(e) = g.as_mut().unwrap().events.get_mut(&ev) {
            if e.signaled {
                if e.auto_clear {
                    e.signaled = false;
                }
                return true;
            }
            if !e.auto_clear && signals0.is_some_and(|n| n != e.signals) {
                return true;
            }
        }
        match deadline {
            None => g = SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                g = SYNC_CV.wait_timeout(g, d - now).unwrap().0;
            }
        }
    }
}

/// MultiWaitHolder user data: an opaque value the caller attaches to a holder.
pub(crate) static HOLDER_USER_DATA: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);


/// Message queue operations with a deadline (None = block forever, Some(now) = try once). `front` jams the value at the
/// head instead of the tail. Returns false on timeout; an unknown queue fails at once.
pub(crate) fn mq_send(mq: usize, data: u64, front: bool, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    loop {
        match g.as_mut().unwrap().msgqs.get_mut(&mq) {
            Some(q) if q.q.len() < q.cap => {
                if front {
                    q.q.push_front(data)
                } else {
                    q.q.push_back(data)
                }
                drop(g);
                SYNC_CV.notify_all();
                return true;
            }
            Some(_) => {}
            None => return false,
        }
        g = match deadline {
            None => SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                SYNC_CV.wait_timeout(g, d - now).unwrap().0
            }
        };
    }
}
/// `peek`: read the head without removing it.
pub(crate) fn mq_receive(out: *mut u64, mq: usize, deadline: Option<std::time::Instant>, peek: bool) -> bool {
    let mut g = lock();
    loop {
        match g.as_mut().unwrap().msgqs.get_mut(&mq) {
            Some(q) => {
                let v = if peek { q.q.front().copied() } else { q.q.pop_front() };
                if let Some(v) = v {
                    if !out.is_null() {
                        unsafe { *out = v };
                    }
                    drop(g);
                    SYNC_CV.notify_all();
                    return true;
                }
            }
            None => return false,
        }
        g = match deadline {
            None => SYNC_CV.wait(g).unwrap(),
            Some(d) => {
                let now = std::time::Instant::now();
                if now >= d {
                    return false;
                }
                SYNC_CV.wait_timeout(g, d - now).unwrap().0
            }
        };
    }
}
pub(crate) fn deadline_ns(timeout_ns: i64) -> Option<std::time::Instant> {
    Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64))
}

// ── nn::os::ReaderWriterLock (real exclusion) ────────────────────────────────────────────────
pub fn rw_init(addr: usize) {
    lock()
        .as_mut()
        .unwrap()
        .rwlocks
        .insert(addr, RwSt::default());
}
pub fn rw_acquire_write(addr: usize) {
    let t = me();
    let mut g = lock();
    loop {
        let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
        if r.writer == t {
            r.write_depth += 1;
            return;
        }
        if r.writer == 0 && r.readers == 0 {
            r.writer = t;
            r.write_depth = 1;
            return;
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}
pub fn rw_try_acquire_write(addr: usize) -> bool {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == t {
        r.write_depth += 1;
        true
    } else if r.writer == 0 && r.readers == 0 {
        r.writer = t;
        r.write_depth = 1;
        true
    } else {
        false
    }
}
pub fn rw_release_write(addr: usize) {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == t && r.write_depth > 0 {
        r.write_depth -= 1;
        if r.write_depth == 0 {
            r.writer = 0;
            drop(g);
            SYNC_CV.notify_all();
        }
    }
}
pub fn rw_acquire_read(addr: usize) {
    let t = me();
    let mut g = lock();
    loop {
        let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
        if r.writer == 0 || r.writer == t {
            r.readers += 1;
            return;
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}
pub fn rw_try_acquire_read(addr: usize) -> bool {
    let t = me();
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.writer == 0 || r.writer == t {
        r.readers += 1;
        true
    } else {
        false
    }
}
pub fn rw_release_read(addr: usize) {
    let mut g = lock();
    let r = g.as_mut().unwrap().rwlocks.entry(addr).or_default();
    if r.readers > 0 {
        r.readers -= 1;
        if r.readers == 0 {
            drop(g);
            SYNC_CV.notify_all();
        }
    }
}
pub fn rw_write_held_by_me(addr: usize) -> bool {
    let t = me();
    lock()
        .as_mut()
        .unwrap()
        .rwlocks
        .get(&addr)
        .map(|r| r.writer == t)
        .unwrap_or(false)
}


// ── nn::time ─────────────────────────────────────────────────────────────────────────────────
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
pub(crate) unsafe extern "C" fn time_initialize() -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn time_finalize() -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn time_is_initialized() -> bool {
    true
}
pub(crate) unsafe extern "C" fn time_clock_get_current_time(out: *mut i64) -> u32 {
    if !out.is_null() {
        *out = unix_now();
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn time_steady_clock_now() -> i64 {
    crate::platform::clock::now_ns() as i64
}
// ClockSnapshot is 208 bytes. Our layout: +0 user posix secs, +8 steady ns.
pub(crate) unsafe extern "C" fn time_clock_snapshot_ctor(snap: *mut u8) {
    if !snap.is_null() {
        std::ptr::write_bytes(snap, 0, 208);
    }
}
pub(crate) unsafe extern "C" fn time_clock_snapshot_create(snap: *mut u8) {
    if !snap.is_null() {
        std::ptr::write_bytes(snap, 0, 208);
        *(snap as *mut i64) = unix_now();
        *(snap.add(8) as *mut i64) = time_steady_clock_now();
    }
}
pub(crate) unsafe extern "C" fn time_calculate_span_between(
    out: *mut i64,
    from: *const u8,
    to: *const u8,
) -> bool {
    if out.is_null() || from.is_null() || to.is_null() {
        return false;
    }
    let (a, b) = (*(from.add(8) as *const i64), *(to.add(8) as *const i64));
    *out = b - a;
    true
}
pub(crate) unsafe extern "C" fn time_user_clock_difference(from: *const u8, to: *const u8) -> i64 {
    if from.is_null() || to.is_null() {
        return 0;
    }
    (*(to as *const i64) - *(from as *const i64)) * 1_000_000_000
}
// CalendarTime { i16 year; i8 month, day, hour, minute, second }  (8 bytes)
// CalendarAdditionalInfo { i32 dayOfWeek; i32 yearDay; TimeZone { char name[8]; bool dst; i32 utcOffset } }
unsafe fn fill_calendar(
    tm: &libc::tm,
    cal: *mut u8,
    info: *mut u8,
    name: &[u8],
    dst: bool,
    off: i32,
) {
    if !cal.is_null() {
        *(cal as *mut i16) = (tm.tm_year + 1900) as i16;
        *cal.add(2) = (tm.tm_mon + 1) as u8;
        *cal.add(3) = tm.tm_mday as u8;
        *cal.add(4) = tm.tm_hour as u8;
        *cal.add(5) = tm.tm_min as u8;
        *cal.add(6) = tm.tm_sec as u8;
    }
    if !info.is_null() {
        *(info as *mut i32) = tm.tm_wday;
        *(info.add(4) as *mut i32) = tm.tm_yday;
        std::ptr::write_bytes(info.add(8), 0, 16);
        let n = name.len().min(7);
        std::ptr::copy_nonoverlapping(name.as_ptr(), info.add(8), n);
        *info.add(16) = dst as u8;
        *(info.add(20) as *mut i32) = off;
    }
}
pub(crate) unsafe extern "C" fn time_to_calendar_time(cal: *mut u8, info: *mut u8, posix: *const i64) -> u32 {
    let t = if posix.is_null() { 0 } else { *posix };
    let (tm, name, dst, off) = crate::platform::os::local_time(t);
    fill_calendar(&tm, cal, info, name.as_bytes(), dst, off);
    RESULT_SUCCESS
}
/// nn::time::ToCalendarTimeInUtc(const PosixTime&) -> CalendarTime (8-byte struct, returned in x0).
pub(crate) unsafe extern "C" fn time_to_calendar_time_in_utc(posix: *const i64) -> u64 {
    let tm = crate::platform::os::utc_time(if posix.is_null() { 0 } else { *posix });
    let mut cal = [0u8; 8];
    fill_calendar(
        &tm,
        cal.as_mut_ptr(),
        std::ptr::null_mut(),
        b"UTC",
        false,
        0,
    );
    u64::from_le_bytes(cal)
}
/// nn::time::ToPosixTimeFromUtc(const CalendarTime&) -> PosixTime (8-byte struct, returned in x0).
pub(crate) unsafe extern "C" fn time_to_posix_time_from_utc(cal: *const u8) -> i64 {
    if cal.is_null() {
        return 0;
    }
    let mut tm: libc::tm = std::mem::zeroed();
    tm.tm_year = *(cal as *const i16) as i32 - 1900;
    tm.tm_mon = *cal.add(2) as i32 - 1;
    tm.tm_mday = *cal.add(3) as i32;
    tm.tm_hour = *cal.add(4) as i32;
    tm.tm_min = *cal.add(5) as i32;
    tm.tm_sec = *cal.add(6) as i32;
    crate::platform::os::from_utc_time(&mut tm)
}

/// Notification messages for the application. After launch the system reports the focus
/// state once (MessageFocusStateChanged = 15; GetCurrentFocusState then says InFocus), as Ryujinx's applet state
/// manager does. Kirby and the Forgotten Land (FocusHandlingMode_SuspendAndNotify) ignored all input without it.
pub(crate) static OE_MESSAGES: Mutex<Option<VecDeque<u32>>> = Mutex::new(None);
// ── nn::pl shared fonts ──────────────────────────────────────────────────────────────────────
// The system's shared fonts (Super Mario Odyssey renders its text with them; TotK ships its own). They are read from
// --shared-fonts (a .app's Resources/fonts), named by type: FontStandard.ttf, FontChineseSimplified.ttf,
// FontExtendedChineseSimplified.ttf, FontChineseTraditional.ttf, FontKorean.ttf, FontNintendoExtended.ttf. A missing
// font falls back to FontStandard.ttf, then to a host font with Latin glyphs.
const SHARED_FONT_FILES: [&str; 6] = [
    "FontStandard.ttf",
    "FontChineseSimplified.ttf",
    "FontExtendedChineseSimplified.ttf",
    "FontChineseTraditional.ttf",
    "FontKorean.ttf",
    "FontNintendoExtended.ttf",
];
const HOST_FONT_CANDIDATES: &[&str] = &[
    // macOS font paths
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/System/Library/Fonts/Supplemental/Helvetica.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
    // iOS font paths
    "/System/Library/Fonts/Core/Arial.ttf",
    "/System/Library/Fonts/CoreAddition/Arial.ttf",
    "/System/Library/Fonts/Core/Helvetica.ttc",
    "/System/Library/Fonts/CoreAddition/Helvetica.ttc",
    "/System/Library/Fonts/Core/HelveticaNeue.ttc",
    "/System/Library/Fonts/CoreAddition/HelveticaNeue.ttc",
    "/System/Library/Fonts/Core/TimesNewRoman.ttf",
    "/System/Library/Fonts/CoreAddition/TimesNewRoman.ttf",
    "/System/Library/Fonts/Core/CourierNew.ttf",
    "/System/Library/Fonts/CoreAddition/CourierNew.ttf",
    "/System/Library/Fonts/Core/PingFang.ttc",
    "/System/Library/Fonts/CoreAddition/PingFang.ttc",
    "/System/Library/Fonts/Core/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/CoreAddition/AppleSDGothicNeo.ttc",
];
static DUMMY_SHARED_FONT: &[u8] = include_bytes!("dummy_font.ttf");
static SHARED_FONTS: [std::sync::OnceLock<Vec<u8>>; 6] = [const { std::sync::OnceLock::new() }; 6];

fn load_shared_font(ty: usize) -> Vec<u8> {
    let dir = crate::settings::get().files.shared_fonts.clone();
    let mut font_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(d) = &dir {
        font_dirs.push(d.clone());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            font_dirs.push(parent.join("fonts"));
            font_dirs.push(parent.join("Resources/fonts"));
            if let Some(grandparent) = parent.parent() {
                font_dirs.push(grandparent.join("Resources/fonts"));
                font_dirs.push(grandparent.join("fonts"));
            }
        }
    }
    if let Some(game_dir) = &crate::settings::get().files.game_dir {
        font_dirs.push(game_dir.join("fonts"));
        font_dirs.push(game_dir.join("RomFS/fonts"));
    }
    if let Some(app_data) = crate::profile::data_dir() {
        if let Some(nxport_dir) = app_data.parent().and_then(|p| p.parent()) {
            font_dirs.push(nxport_dir.join("fonts"));
            font_dirs.push(nxport_dir.join("system_data/0100000000000811"));
        }
    }
    if let Some(sys_font_dir) = crate::horizon::fs::system_data_dir(0x0100000000000811) {
        font_dirs.push(sys_font_dir);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = std::path::PathBuf::from(home);
        font_dirs.push(p.join("Library/Application Support/nxport/fonts"));
        font_dirs.push(p.join("Documents/fonts"));
        font_dirs.push(p.join("Documents/Game/fonts"));
    }
    font_dirs.push(std::path::PathBuf::from("fonts"));

    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for d in &font_dirs {
        paths.push(d.join(SHARED_FONT_FILES[ty]));
        paths.push(d.join(SHARED_FONT_FILES[0]));
    }
    for candidate in HOST_FONT_CANDIDATES {
        paths.push((*candidate).into());
    }
    #[cfg(windows)]
    {
        let windows = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from);
        for name in ["arial.ttf", "segoeui.ttf", "tahoma.ttf", "verdana.ttf"] {
            paths.push(windows.join("Fonts").join(name));
        }
    }

    for p in &paths {
        if let Ok(bytes) = std::fs::read(p) {
            println!("[+] nn::pl: shared font {} from {} ({} bytes)", ty, p.display(), bytes.len());
            return bytes;
        }
    }
    eprintln!("[!] nn::pl: no shared font {} (--shared-fonts), falling back to embedded dummy font", SHARED_FONT_FILES[ty]);
    DUMMY_SHARED_FONT.to_vec()
}

fn shared_font(ty: i32) -> &'static [u8] {
    let ty = if (0..6).contains(&ty) { ty as usize } else { 0 };
    SHARED_FONTS[ty].get_or_init(|| load_shared_font(ty))
}

pub(crate) unsafe extern "C" fn pl_request_shared_font_load(ty: i32) {
    shared_font(ty);
}
/// GetSharedFont(int* count, SharedFontData[] {type, address, size}, int capacity, LanguageCode): every font in type
/// order whatever the language (as Ryujinx); true when all are loaded.
pub(crate) unsafe extern "C" fn pl_get_shared_font(out_count: *mut i32, out: *mut u8, capacity: i32, _language: u64) -> bool {
    let n = capacity.clamp(0, 6) as usize;
    let mut all = true;
    for ty in 0..n {
        let font = shared_font(ty as i32);
        let (addr, len) = if font.is_empty() {
            (DUMMY_SHARED_FONT.as_ptr(), DUMMY_SHARED_FONT.len())
        } else {
            (font.as_ptr(), font.len())
        };
        all &= len > 0;
        if !out.is_null() {
            let e = out.add(ty * 24);
            (e as *mut u32).write_unaligned(ty as u32);
            (e.add(8) as *mut usize).write_unaligned(addr as usize);
            (e.add(16) as *mut usize).write_unaligned(len);
        }
    }
    if !out_count.is_null() {
        *out_count = 6;
    }
    all
}
pub(crate) unsafe extern "C" fn pl_get_shared_font_load_state(_ty: i32) -> i32 {
    1
}
pub(crate) unsafe extern "C" fn pl_get_shared_font_size(ty: i32) -> usize {
    let len = shared_font(ty).len();
    if len == 0 {
        DUMMY_SHARED_FONT.len()
    } else {
        len
    }
}
pub(crate) unsafe extern "C" fn pl_get_shared_font_address(ty: i32) -> *const u8 {
    let font = shared_font(ty);
    if font.is_empty() {
        DUMMY_SHARED_FONT.as_ptr()
    } else {
        font.as_ptr()
    }
}
/// VR mode as the game set it (nn::pl::BeginVrMode / EndVrMode): the system only records it here; the game draws its own
/// split image.
static VR_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) unsafe extern "C" fn pl_begin_vr_mode() {
    VR_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
}
pub(crate) unsafe extern "C" fn pl_end_vr_mode() {
    VR_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
}
pub(crate) unsafe extern "C" fn pl_is_vr_mode() -> bool {
    VR_MODE.load(std::sync::atomic::Ordering::Relaxed)
}
/// nn::pl::IsRunningOnOunce: whether the console is a Switch 2. The engine presents the original Switch.
pub(crate) unsafe extern "C" fn pl_is_running_on_ounce() -> bool {
    false
}
// ── nn::friends: an offline user with an empty friend list (the SDK's own Initialize opens friend:u itself, whose IPC
// is not emulated: Super Mario Bros. Wonder aborted there at boot) ──────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn friends_void() {}
pub(crate) unsafe extern "C" fn friends_ok() -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn friends_get_system_event(event: *mut ()) -> u32 {
    if !event.is_null() {
        crate::horizon::hle::hle_system_event_register_signaled(event as usize);
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn friends_out_true(out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn friends_get_friend_list(out_count: *mut i32) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn friends_get_play_history_key(out_key: *mut u8) -> u32 {
    if !out_key.is_null() {
        std::ptr::write_bytes(out_key, 0, 0x40); // PlayHistoryRegistrationKey
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn friends_false() -> bool {
    false
}
pub(crate) unsafe extern "C" fn friends_zero() -> u64 {
    0
}
static FRIENDS_EMPTY: [u8; 0x100] = [0; 0x100];
pub(crate) unsafe extern "C" fn friends_empty_ref() -> *const u8 {
    FRIENDS_EMPTY.as_ptr()
}
pub(crate) unsafe extern "C" fn friends_not_found() -> u32 {
    121 | (1 << 9) // a friends-module failure: there is no friend to have an image
}

// ── nn::ldn: local wireless / ad-hoc networking ──────────────────────────────
static LDN_STATE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0); // 0 = None
static LDN_STATE_EVENT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn ldn_set_state(new_state: u32) {
    LDN_STATE.store(new_state, std::sync::atomic::Ordering::SeqCst);
    let ev = LDN_STATE_EVENT.load(std::sync::atomic::Ordering::SeqCst);
    if ev != 0 {
        crate::horizon::hle::hle_system_event_signal_addr(ev);
    }
}

pub(crate) unsafe extern "C" fn ldn_initialize() -> u32 {
    ldn_set_state(1); // State::Initialized
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_finalize() -> u32 {
    ldn_set_state(0); // State::None
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_get_state() -> u32 {
    LDN_STATE.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) unsafe extern "C" fn ldn_attach_state_change_event(event: *mut u8) {
    if !event.is_null() {
        LDN_STATE_EVENT.store(event as usize, std::sync::atomic::Ordering::SeqCst);
        crate::horizon::hle::hle_system_event_register_signaled(event as usize);
    }
}

pub(crate) unsafe extern "C" fn ldn_open_station() -> u32 {
    ldn_set_state(4); // State::StationOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_close_station() -> u32 {
    ldn_set_state(1); // State::Initialized
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_open_access_point() -> u32 {
    ldn_set_state(2); // State::AccessPointOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_close_access_point() -> u32 {
    ldn_set_state(1); // State::Initialized
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_create_network() -> u32 {
    ldn_set_state(3); // State::AccessPointCreated
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_destroy_network() -> u32 {
    ldn_set_state(2); // State::AccessPointOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_scan(
    _out_networks: *mut u8,
    out_count: *mut i32,
    _max_count: i32,
    _filter: *const u8,
    _channel: i32,
) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_connect() -> u32 {
    ldn_set_state(5); // State::StationConnected
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_disconnect() -> u32 {
    ldn_set_state(4); // State::StationOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_get_ipv4_address(out_ip: *mut u32, out_mask: *mut u32) -> u32 {
    if !out_ip.is_null() {
        *out_ip = 0x0A000002; // 10.0.0.2
    }
    if !out_mask.is_null() {
        *out_mask = 0xFFFFFF00; // 255.255.255.0
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_zero_buffer_0x480(buf: *mut u8) -> u32 {
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 0x480);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_zero_buffer_0x48(buf: *mut u8) -> u32 {
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 0x48);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_zero_buffer_0x40(buf: *mut u8) -> u32 {
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 0x40);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn ldn_make_ipv4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32)
}

pub(crate) unsafe extern "C" fn ldn_make_broadcast(ip: u32, mask: u32) -> u32 {
    ip | !mask
}

pub(crate) unsafe extern "C" fn ldn_make_network(ip: u32, mask: u32) -> u32 {
    ip & mask
}

// ── nn::account: one local user (the uid TryOpenPreselectedUser / GetUserId report, horizon/hle.rs), no network account ──────
pub(crate) const USER_UID: [u64; 2] = [0x0123456789ABCDEF, 0xFEDCBA9876543210];
pub(crate) unsafe extern "C" fn fs_is_save_data_existing(_uid: *const u8) -> bool {
    true // saves live in the save directory (horizon/fs.rs); EnsureSaveData / MountSaveData create it
}

/// nn::time::GetDeviceLocationName(LocationName*): 36-byte time zone name (the SDK's own asks the time service).
pub(crate) unsafe extern "C" fn time_get_device_location_name(out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 36);
        std::ptr::copy_nonoverlapping(b"UTC".as_ptr(), out, 3);
    }
}


// ── nn::ngc::ProfanityFilter: every word checks clean (ngc:u is not emulated; the SDK's Initialize opens it) ──────────
pub(crate) unsafe extern "C" fn ngc_ok() -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn ngc_check_words(
    _this: *mut u8,
    results: *mut u32,
    _patterns: u32,
    _words: *const usize,
    count: usize,
) -> u32 {
    if !results.is_null() && count < 0x10000 {
        std::ptr::write_bytes(results, 0, count);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn err_show_error(result: u32) {
    eprintln!(
        "[!] nn::err::ShowError(result={:#x}: module {}, description {})",
        result,
        result & 0x1ff,
        result >> 9
    );
}

pub(crate) unsafe extern "C" fn detail_unexpected_default_impl() {
    eprintln!(
        "[!] CRITICAL ABORT: nn::detail::UnexpectedDefaultImpl() (a switch default was reached)"
    );
    std::process::abort();
}

// ── nn::swkbd / nn::nfp / nn::hid / nn::prepo ────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn swkbd_get_required_work_buffer_size(_is_unicode: bool) -> usize {
    0x4000
}
pub(crate) unsafe extern "C" fn swkbd_get_required_string_buffer_size() -> usize {
    0x2000
}
pub(crate) unsafe extern "C" fn swkbd_make_preset(cfg: *mut u8, _preset: i32) {
    if !cfg.is_null() {
        std::ptr::write_bytes(cfg, 0, 0x4d0);
    }
}
pub(crate) unsafe extern "C" fn swkbd_show_keyboard(out: *mut u8, arg: *const u8) -> u32 {
    const SWKBD_CANCELED: u32 = 0x29f;
    if out.is_null() {
        return SWKBD_CANCELED;
    }
    let str_buf = *(out as *const usize);
    let str_cap = *((out as *const usize).add(1));
    if str_buf == 0 || str_cap == 0 {
        return SWKBD_CANCELED;
    }

    let (_max_len, _min_len, utf8, initial_text) = if !arg.is_null() {
        let max_l = *(arg.add(0x3ac) as *const u32) as usize;
        let min_l = *(arg.add(0x3b0) as *const u32) as usize;
        let is_u8 = *arg.add(0x3bd) != 0;

        let init_off = *(arg.add(0x3c0) as *const i32);
        let init_len = *(arg.add(0x3c4) as *const i32);
        let work = *(arg.add(0x4d0) as *const usize);
        let work_size = *(arg.add(0x4d8) as *const usize);

        let mut init_str = String::new();
        if init_len > 0
            && work != 0
            && init_off >= 0
            && (init_off as usize + (init_len as usize) * 2) <= work_size
        {
            let u16_slice = std::slice::from_raw_parts(
                (work + init_off as usize) as *const u16,
                init_len as usize,
            );
            init_str = String::from_utf16_lossy(u16_slice);
        }
        (max_l, min_l, is_u8, init_str)
    } else {
        (500, 0, false, String::new())
    };

    let answer = if let Some(val) = crate::settings::get().input.keyboard_text.clone() {
        if val == "<cancel>" {
            return SWKBD_CANCELED;
        }
        val
    } else if !initial_text.is_empty() {
        initial_text
    } else {
        "Player".to_string()
    };

    println!("[+] swkbd: ShowKeyboard returning \"{}\"", answer);

    if utf8 {
        let bytes = answer.as_bytes();
        let copy_len = bytes.len().min(str_cap.saturating_sub(1));
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), str_buf as *mut u8, copy_len);
        *((str_buf + copy_len) as *mut u8) = 0;
    } else {
        let u16_vec: Vec<u16> = answer.encode_utf16().collect();
        let max_u16 = (str_cap / 2).saturating_sub(1);
        let copy_len = u16_vec.len().min(max_u16);
        std::ptr::copy_nonoverlapping(u16_vec.as_ptr(), str_buf as *mut u16, copy_len);
        *((str_buf + copy_len * 2) as *mut u16) = 0;
    }

    RESULT_SUCCESS
}
// nfp: no NFC hardware. State 0 = None.
const NFP_RESULT_NOT_AVAILABLE: u32 = 0x1;
pub(crate) unsafe extern "C" fn nfp_initialize() -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn nfp_finalize() {}
pub(crate) unsafe extern "C" fn nfp_get_state() -> i32 {
    0
}
pub(crate) unsafe extern "C" fn nfp_attach_event(ev: *mut u8) {
    if !ev.is_null() {
        hle_system_event_register_addr(ev as usize);
    }
}
pub(crate) unsafe extern "C" fn nfp_attach_event_with_handle(ev: *mut u8, _handle: *const u8) -> u32 {
    if !ev.is_null() {
        hle_system_event_register_addr(ev as usize);
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn nfp_list_devices(_handles: *mut u8, out_count: *mut i32, _max: i32) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn nfp_get_device_state(_handle: *const u8) -> i32 {
    0
}
pub(crate) unsafe extern "C" fn nfp_fail1(_a: *const u8) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}
pub(crate) unsafe extern "C" fn nfp_fail2(_a: *mut u8, _b: *const u8) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}
pub(crate) unsafe extern "C" fn nfp_mount(_h: *const u8, _m: i32, _t: i32) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}
pub(crate) unsafe extern "C" fn aoc_list(_out_indices: *mut i32, _offset: i32, _count: i32) -> i32 {
    0
}
/// nn::os::SystemEvent holds its SystemEventType first; the list never changes, so it is never signaled.
pub(crate) unsafe extern "C" fn aoc_list_changed_event(event: *mut u8) {
    crate::horizon::hle::hle_system_event_register_addr(event as usize);
}

/// Which handheld Joy-Cons count as an active controller (both / either / none); the engine's controller is the
/// keyboard / gamepad, so the mode is only kept for the game to read back.
pub(crate) static HANDHELD_ACTIVATION_MODE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub(crate) unsafe extern "C" fn noop_void() {}
pub(crate) unsafe extern "C" fn noop_obj(_this: *mut u8) {}
pub(crate) unsafe extern "C" fn prepo_zero_u32() -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn prepo_save_result(_this: *mut u8) -> u32 {
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn prepo_get_size(_this: *const u8) -> usize {
    0
}

/// nn::web::ShowOfflineHtmlPage(OfflineHtmlPageReturnValue*, const ShowOfflineHtmlPageArg&): the
/// page "closes at once" as if Back was pressed. OfflineHtmlPageReturnValue: u64 exit reason
/// (OfflineExitReason_BackButtonPressed = 1), char lastUrl[4096], u64 lastUrlSize.
pub(crate) unsafe extern "C" fn web_show_offline_html_page(out: *mut u8, _arg: *const u8) -> u32 {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        println!(
            "[+] HLE: nn::web::ShowOfflineHtmlPage: no web applet, reported as closed with Back"
        )
    });
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 8 + 4096 + 8);
        *(out as *mut u64) = 1;
    }
    RESULT_SUCCESS
}

/// Data-cache maintenance (FlushDataCache etc.): the sdk module's versions execute `dc`/`mrs ctr_el0`
/// sequences that Darwin traps (SIGILL). Apple Silicon memory is coherent for our purposes, so no-op.
pub unsafe extern "C" fn cache_noop(_addr: *const u8, _size: usize) {}


/// nn::settings::detail::MakeLanguageCode(LanguageCode*, Language): the SDK asks the settings service (IPC to "set")
/// for the IETF tag of a Language value (Japanese = 0 ..
/// TraditionalChinese = 16; later versions add BrazilianPortuguese = 17).
/// Interposed onto the SDK's own definition (Kirby and the Forgotten Land calls it through
/// nn::settings::operator==(LanguageCode, Language)).
pub unsafe extern "C" fn settings_make_language_code(out: *mut [u8; 8], language: i32) -> u32 {
    const CODES: [&str; 18] = [
        "ja", "en-US", "fr", "de", "it", "es", "zh-CN", "ko", "nl", "pt", "ru", "zh-TW", "en-GB",
        "fr-CA", "es-419", "zh-Hans", "zh-Hant", "pt-BR",
    ];
    // An invalid value violates the documented precondition: say so and return an empty code.
    let code = usize::try_from(language)
        .ok()
        .and_then(|i| CODES.get(i))
        .copied()
        .unwrap_or_else(|| {
            println!(
                "[!] nn::settings::MakeLanguageCode: invalid language {} (precondition violated)",
                language
            );
            ""
        });
    let mut v = [0u8; 8];
    v[..code.len()].copy_from_slice(code.as_bytes());
    if !out.is_null() {
        *out = v;
    }
    0
}

// nn::bcat (data distribution / delivery cache): a console that never went online. The storage mounts and
// is empty, sync requests fail with no Internet connection, files and directories are not found.
// Result codes use module 122 (result = module | description << 9).
const BCAT_NOT_FOUND: u32 = 122 | (2 << 9);
const BCAT_NOT_OPENED: u32 = 122 | (7 << 9);
const BCAT_INTERNET_REQUEST_NOT_ACCEPTED: u32 = 122 | (8 << 9);
pub(crate) unsafe extern "C" fn bcat_initialize() {}
pub(crate) unsafe extern "C" fn bcat_mount_delivery_cache_storage() -> u32 {
    0
}
pub(crate) unsafe extern "C" fn bcat_unmount_delivery_cache_storage() {}
pub(crate) unsafe extern "C" fn bcat_enumerate_delivery_cache_directory(
    out_count: *mut i32,
    _names: *mut u8,
    _count: i32,
) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    0
}
pub(crate) unsafe extern "C" fn bcat_request_sync_delivery_cache(_progress: *mut u8) -> u32 {
    BCAT_INTERNET_REQUEST_NOT_ACCEPTED
}
pub(crate) unsafe extern "C" fn bcat_progress_update(_this: *mut u8) {}
pub(crate) unsafe extern "C" fn bcat_progress_get_status(_this: *const u8) -> i32 {
    9
} // DeliveryCacheProgressStatus_Done
pub(crate) unsafe extern "C" fn bcat_progress_get_result(_this: *const u8) -> u32 {
    BCAT_INTERNET_REQUEST_NOT_ACCEPTED
}
pub(crate) unsafe extern "C" fn bcat_open_not_found(_this: *mut u8, _name: *const u8) -> u32 {
    BCAT_NOT_FOUND
}
pub(crate) unsafe extern "C" fn bcat_file_read(
    _this: *mut u8,
    out_size: *mut usize,
    _offset: i64,
    _buf: *mut u8,
    _size: usize,
) -> u32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    BCAT_NOT_OPENED
}
pub(crate) unsafe extern "C" fn bcat_directory_read(
    _this: *mut u8,
    out_count: *mut i32,
    _entries: *mut u8,
    _count: i32,
) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    BCAT_NOT_OPENED
}
pub(crate) unsafe extern "C" fn bcat_close(_this: *mut u8) {}

/// nn::prepo::Initialize: would open the prepo service (play reports are discarded, see `lookup`).
pub(crate) unsafe extern "C" fn prepo_initialize() {}

// nn::ec (shop access): no network. The library initializes; requests fail
// with ResultInternetRequestNotAccepted (module 164, description 56). Constructors / destructors run
// in the SDK.
const EC_INTERNET_REQUEST_NOT_ACCEPTED: u32 = 164 | (56 << 9);
pub(crate) unsafe extern "C" fn ec_initialize_for_shop_service_accessors(_work: *mut u8, _size: usize) -> u32 {
    0
}
pub(crate) unsafe extern "C" fn ec_finalize_for_shop_service_accessors() {}
pub(crate) unsafe extern "C" fn ec_accessor_initialize(_this: *mut u8, _service: *const u8) -> u32 {
    0
}
pub(crate) unsafe extern "C" fn ec_accessor_finalize(_this: *mut u8) {}
pub(crate) unsafe extern "C" fn ec_accessor_request(
    _this: *mut u8,
    _resp: *mut u8,
    _uid: *const u8,
    _method: i32,
    _path: *const u8,
    _post: *const u8,
    _timeout: i64,
) -> u32 {
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}
pub(crate) unsafe extern "C" fn ec_response_try_wait(_this: *mut u8) -> bool {
    true
}
pub(crate) unsafe extern "C" fn ec_response_cancel(_this: *mut u8) {}
pub(crate) unsafe extern "C" fn ec_response_get_size(_this: *mut u8, out: *mut usize) -> u32 {
    if !out.is_null() {
        *out = 0;
    }
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}
pub(crate) unsafe extern "C" fn ec_response_get(_this: *mut u8, _buf: *mut u8, _size: usize) -> u32 {
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}
/// Opens the eShop product page applet; nothing to show.
pub(crate) unsafe extern "C" fn ec_show_shop_product_details(ns_uid: u64, _user: *const u8) {
    println!(
        "[+] HLE: nn::ec::ShowShopProductDetails(nsUid={:#x}): no eShop",
        ns_uid
    );
}

// nn::account network service account: the user has none linked / no network,
// ResultNetworkServiceAccountUnavailable (module 124, description 200). An AsyncContext the
// failed calls never started reports done with that result; its event is signaled.
pub(crate) const ACCOUNT_NSA_UNAVAILABLE: u32 = 124 | (200 << 9);

/// nn::pdm::QueryApplicationPlayStatistics(out, uid, appIds, count) -> number of entries written: no play records.
pub(crate) unsafe extern "C" fn pdm_query_application_play_statistics(
    _out: *mut u8,
    _uid: *const u8,
    _ids: *const u8,
    _count: i32,
) -> i32 {
    0
}

/// nn::err::ShowError(ErrorCode{category, number}): the error applet; logged.
pub(crate) unsafe extern "C" fn err_show_error_code(code: u64) {
    eprintln!(
        "[!] nn::err::ShowError(error code {:04}-{:04})",
        code as u32,
        (code >> 32) as u32
    );
}

// No network (consistent with nn::socket::Initialize failing and nifm reporting no connection): every socket operation
// fails with Errno::ENetDown (100); closing / finalizing succeed; name lookups fail with EAI_FAIL
// (4). Falling through to the sdk module here would not fail cleanly: its IPC requests are written to the
// thread's Horizon IPC buffer, which is Darwin's TLS on this host (the reply port in slot 2 gets overwritten).
const SOCKET_ENETDOWN: i32 = 100;
pub(crate) unsafe extern "C" fn socket_fail() -> i32 {
    -1
}
thread_local! {
    static SOCKET_ERRNO: std::cell::Cell<i32> = const { std::cell::Cell::new(SOCKET_ENETDOWN) };
}
/// The errno the calling thread's next nn::socket::GetLastErrno returns.
pub(crate) fn set_socket_errno(e: i32) {
    SOCKET_ERRNO.with(|c| c.set(e));
}
pub(crate) unsafe extern "C" fn socket_get_last_errno() -> i32 {
    SOCKET_ERRNO.with(|c| c.get())
}
pub(crate) unsafe extern "C" fn socket_get_addr_info(
    _node: *const u8,
    _svc: *const u8,
    _hints: *const u8,
    out: *mut *mut u8,
) -> i32 {
    if !out.is_null() {
        *out = std::ptr::null_mut();
    }
    4 // EAI_FAIL
}
pub(crate) unsafe extern "C" fn socket_free_addr_info(_ai: *mut u8) {}
/// nn::ssl without a network still works locally on a console: the library initializes, contexts are created and
/// certificates imported; connections fail at the socket level (above). Interposed on the SDK too (nn::ssl::Initialize
/// is called from inside it).
pub unsafe extern "C" fn ssl_ok() -> u32 {
    0
}
/// nifm connection queries without a connection: NoInternetConnection (module 110, description 300, as
/// Ryujinx reports it).
pub(crate) unsafe extern "C" fn nifm_no_internet_connection(_out: *mut u8) -> u32 {
    110 | (300 << 9)
}
pub(crate) unsafe extern "C" fn nifm_cancel_network_request() {}
/// HandleNetworkRequestErrorResult: false = the error was not resolved.
pub(crate) unsafe extern "C" fn nifm_handle_network_request_error_result() -> bool {
    false
}

