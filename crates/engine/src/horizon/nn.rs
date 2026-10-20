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

use crate::horizon::hle::{hle_nn_os_GetCurrentThread, hle_system_event_register_addr};

const RESULT_SUCCESS: u32 = 0;

// ── global sync state ────────────────────────────────────────────────────────────────────────
#[derive(Default)]
struct EventSt {
    signaled: bool,
    auto_clear: bool,
}

#[derive(Default)]
struct MsgQ {
    cap: usize,
    q: VecDeque<u64>,
}

#[derive(Default)]
struct RwSt {
    readers: u32,
    writer: usize, // owning thread token, 0 = none
    write_depth: u32,
}

#[derive(Default)]
struct Sync {
    events: HashMap<usize, EventSt>,
    holders: HashMap<usize, usize>, // holder addr -> event addr
    multiwaits: HashMap<usize, Vec<usize>>, // multiwait addr -> ordered holder addrs
    msgqs: HashMap<usize, MsgQ>,
    rwlocks: HashMap<usize, RwSt>,
}

static SYNC: Mutex<Option<Sync>> = Mutex::new(None);
static SYNC_CV: Condvar = Condvar::new();

fn lock() -> MutexGuard<'static, Option<Sync>> {
    let mut g = SYNC.lock().unwrap();
    if g.is_none() {
        *g = Some(Sync::default());
    }
    g
}

fn me() -> usize {
    unsafe { hle_nn_os_GetCurrentThread() as usize }
}

// ── nn::os::Event ────────────────────────────────────────────────────────────────────────────
// EventClearMode: 0 = ManualClear, 1 = AutoClear.
pub unsafe extern "C" fn os_initialize_event(ev: *mut u8, initially_signaled: bool, clear_mode: i32) {
    let mut g = lock();
    g.as_mut().unwrap().events.insert(
        ev as usize,
        EventSt {
            signaled: initially_signaled,
            auto_clear: clear_mode == 1,
        },
    );
}
pub unsafe extern "C" fn os_finalize_event(ev: *mut u8) {
    lock().as_mut().unwrap().events.remove(&(ev as usize));
}
/// Signals an nn::os::EventType the game initialised through the HLE (for HLE'd libraries that report to the game).
pub(crate) fn signal_event(ev: usize) {
    unsafe { os_signal_event(ev as *mut u8) }
}
pub unsafe extern "C" fn os_signal_event(ev: *mut u8) {
    let mut g = lock();
    if let Some(e) = g.as_mut().unwrap().events.get_mut(&(ev as usize)) {
        e.signaled = true;
    }
    drop(g);
    SYNC_CV.notify_all();
}
pub unsafe extern "C" fn os_clear_event(ev: *mut u8) {
    if let Some(e) = lock().as_mut().unwrap().events.get_mut(&(ev as usize)) {
        e.signaled = false;
    }
}

/// Waits for the event; a signaled auto-clear event is consumed. `deadline` None = forever. Returns whether it was
/// signaled. An event nothing initialised (no side-table entry) never signals.
fn event_wait(ev: usize, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    loop {
        if let Some(e) = g.as_mut().unwrap().events.get_mut(&ev) {
            if e.signaled {
                if e.auto_clear {
                    e.signaled = false;
                }
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
pub unsafe extern "C" fn os_wait_event(ev: *mut u8) {
    event_wait(ev as usize, None);
}
unsafe extern "C" fn os_try_wait_event(ev: *mut u8) -> bool {
    event_wait(ev as usize, Some(std::time::Instant::now()))
}
unsafe extern "C" fn os_timed_wait_event(ev: *mut u8, timeout_ns: i64) -> bool {
    event_wait(
        ev as usize,
        Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64)),
    )
}

// ── nn::os::MultiWait ────────────────────────────────────────────────────────────────────────
unsafe extern "C" fn os_initialize_multiwait(mw: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .multiwaits
        .insert(mw as usize, Vec::new());
}
unsafe extern "C" fn os_finalize_multiwait(mw: *mut u8) {
    lock().as_mut().unwrap().multiwaits.remove(&(mw as usize));
}
unsafe extern "C" fn os_initialize_multiwait_holder_event(holder: *mut u8, ev: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .holders
        .insert(holder as usize, ev as usize);
}
unsafe extern "C" fn os_finalize_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    let s = g.as_mut().unwrap();
    s.holders.remove(&(holder as usize));
    for v in s.multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}
unsafe extern "C" fn os_link_multiwait_holder(mw: *mut u8, holder: *mut u8) {
    let mut g = lock();
    g.as_mut()
        .unwrap()
        .multiwaits
        .entry(mw as usize)
        .or_default()
        .push(holder as usize);
    drop(g);
    SYNC_CV.notify_all(); // a waiter may be blocked on a list that just grew
}
/// MultiWaitHolder user data: an opaque value the caller attaches to a holder.
static HOLDER_USER_DATA: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);
unsafe extern "C" fn os_set_multiwait_holder_user_data(holder: *mut u8, data: usize) {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(holder as usize, data);
}
unsafe extern "C" fn os_get_multiwait_holder_user_data(holder: *const u8) -> usize {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&(holder as usize)).copied())
        .unwrap_or(0)
}
unsafe extern "C" fn os_unlink_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    for v in g.as_mut().unwrap().multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}
/// Blocks until any linked holder's event is signaled; returns that holder (auto-clear events are consumed).
unsafe extern "C" fn os_wait_any(mw: *mut u8) -> *mut u8 {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        let holders: Vec<usize> = s
            .multiwaits
            .get(&(mw as usize))
            .cloned()
            .unwrap_or_default();
        for h in holders {
            if let Some(&ev) = s.holders.get(&h) {
                if let Some(e) = s.events.get_mut(&ev) {
                    if e.signaled {
                        if e.auto_clear {
                            e.signaled = false;
                        }
                        return h as *mut u8;
                    }
                }
            }
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}

// ── nn::os::MessageQueue ─────────────────────────────────────────────────────────────────────
unsafe extern "C" fn os_initialize_message_queue(mq: *mut u8, _buf: *mut u64, count: usize) {
    lock().as_mut().unwrap().msgqs.insert(
        mq as usize,
        MsgQ {
            cap: count.max(1),
            q: VecDeque::new(),
        },
    );
}
unsafe extern "C" fn os_finalize_message_queue(mq: *mut u8) {
    lock().as_mut().unwrap().msgqs.remove(&(mq as usize));
}
unsafe extern "C" fn os_send_message_queue(mq: *mut u8, data: u64) {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        match s.msgqs.get_mut(&(mq as usize)) {
            Some(q) if q.q.len() >= q.cap => {}
            Some(q) => {
                q.q.push_back(data);
                drop(g);
                SYNC_CV.notify_all();
                return;
            }
            None => return,
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}
unsafe extern "C" fn os_receive_message_queue(out: *mut u64, mq: *mut u8) {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        match s.msgqs.get_mut(&(mq as usize)) {
            Some(q) => {
                if let Some(v) = q.q.pop_front() {
                    if !out.is_null() {
                        *out = v;
                    }
                    drop(g);
                    SYNC_CV.notify_all();
                    return;
                }
            }
            None => return,
        }
        g = SYNC_CV.wait(g).unwrap();
    }
}

/// Message queue operations with a deadline (None = block forever, Some(now) = try once). `front` jams the value at the
/// head instead of the tail. Returns false on timeout; an unknown queue fails at once.
fn mq_send(mq: usize, data: u64, front: bool, deadline: Option<std::time::Instant>) -> bool {
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
fn mq_receive(out: *mut u64, mq: usize, deadline: Option<std::time::Instant>) -> bool {
    let mut g = lock();
    loop {
        match g.as_mut().unwrap().msgqs.get_mut(&mq) {
            Some(q) => {
                if let Some(v) = q.q.pop_front() {
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
unsafe extern "C" fn os_try_send_message_queue(mq: *mut u8, data: u64) -> bool {
    mq_send(mq as usize, data, false, Some(std::time::Instant::now()))
}
unsafe extern "C" fn os_jam_message_queue(mq: *mut u8, data: u64) {
    mq_send(mq as usize, data, true, None);
}
unsafe extern "C" fn os_try_receive_message_queue(out: *mut u64, mq: *mut u8) -> bool {
    mq_receive(out, mq as usize, Some(std::time::Instant::now()))
}
unsafe extern "C" fn os_timed_receive_message_queue(
    out: *mut u64,
    mq: *mut u8,
    timeout_ns: i64,
) -> bool {
    mq_receive(
        out,
        mq as usize,
        Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64)),
    )
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
unsafe extern "C" fn os_try_acquire_write_lock(rw: *mut u8) -> bool {
    rw_try_acquire_write(rw as usize)
}
unsafe extern "C" fn os_try_acquire_read_lock(rw: *mut u8) -> bool {
    rw_try_acquire_read(rw as usize)
}

// ── misc nn::os ──────────────────────────────────────────────────────────────────────────────
unsafe extern "C" fn os_set_dying_message_region(_addr: usize, _size: usize) {}
/// nn::os::GetCurrentFiber: ThreadType::_currentFiber, which the SDK's native SwitchToFiber maintains.
unsafe extern "C" fn os_get_current_fiber() -> *mut u8 {
    let t = crate::horizon::hle::hle_nn_os_GetCurrentThread();
    if t.is_null() {
        std::ptr::null_mut()
    } else {
        (*t).current_fiber as *mut u8
    }
}
/// nn::os::ConvertToTick(TimeSpan) -> Tick, consistent with GetSystemTick (host mach ticks).
unsafe extern "C" fn os_convert_to_tick(ns: i64) -> i64 {
    crate::clock::ns_to_ticks(ns.max(0) as u64) as i64
}

// ── nn::time ─────────────────────────────────────────────────────────────────────────────────
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
unsafe extern "C" fn time_initialize() -> u32 {
    RESULT_SUCCESS
}
unsafe extern "C" fn time_finalize() -> u32 {
    RESULT_SUCCESS
}
unsafe extern "C" fn time_is_initialized() -> bool {
    true
}
unsafe extern "C" fn time_clock_get_current_time(out: *mut i64) -> u32 {
    if !out.is_null() {
        *out = unix_now();
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn time_steady_clock_now() -> i64 {
    crate::clock::now_ns() as i64
}
// ClockSnapshot is 208 bytes. Our layout: +0 user posix secs, +8 steady ns.
unsafe extern "C" fn time_clock_snapshot_ctor(snap: *mut u8) {
    if !snap.is_null() {
        std::ptr::write_bytes(snap, 0, 208);
    }
}
unsafe extern "C" fn time_clock_snapshot_create(snap: *mut u8) {
    if !snap.is_null() {
        std::ptr::write_bytes(snap, 0, 208);
        *(snap as *mut i64) = unix_now();
        *(snap.add(8) as *mut i64) = time_steady_clock_now();
    }
}
unsafe extern "C" fn time_calculate_span_between(
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
unsafe extern "C" fn time_user_clock_difference(from: *const u8, to: *const u8) -> i64 {
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
unsafe extern "C" fn time_to_calendar_time(cal: *mut u8, info: *mut u8, posix: *const i64) -> u32 {
    let t: libc::time_t = if posix.is_null() {
        0
    } else {
        *posix as libc::time_t
    };
    let mut tm: libc::tm = std::mem::zeroed();
    libc::localtime_r(&t, &mut tm);
    let name = if tm.tm_zone.is_null() {
        b"UTC".to_vec()
    } else {
        std::ffi::CStr::from_ptr(tm.tm_zone).to_bytes().to_vec()
    };
    fill_calendar(&tm, cal, info, &name, tm.tm_isdst > 0, tm.tm_gmtoff as i32);
    RESULT_SUCCESS
}
/// nn::time::ToCalendarTimeInUtc(const PosixTime&) -> CalendarTime (8-byte struct, returned in x0).
unsafe extern "C" fn time_to_calendar_time_in_utc(posix: *const i64) -> u64 {
    let t: libc::time_t = if posix.is_null() {
        0
    } else {
        *posix as libc::time_t
    };
    let mut tm: libc::tm = std::mem::zeroed();
    libc::gmtime_r(&t, &mut tm);
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
unsafe extern "C" fn time_to_posix_time_from_utc(cal: *const u8) -> i64 {
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
    libc::timegm(&mut tm) as i64
}

// ── nn::oe / nn::pl / nn::account / nn::err ──────────────────────────────────────────────────
unsafe extern "C" fn oe_get_expected_volume_balance(app: *mut f32, sdk: *mut f32) {
    if !app.is_null() {
        *app = 1.0;
    }
    if !sdk.is_null() {
        *sdk = 1.0;
    }
}
unsafe extern "C" fn oe_set_expected_volume_balance(_app: f32, _sdk: f32) {}
unsafe extern "C" fn oe_finish_startup_logo() {}
/// Asks the system to deliver MessageExitRequest instead of terminating the program; no exit
/// requests are generated here (quitting the window ends the process), so entering / leaving the section changes nothing.
unsafe extern "C" fn oe_enter_exit_request_handling_section() {}
unsafe extern "C" fn oe_leave_exit_request_handling_section() {}
/// Notification messages for the application. After launch the system reports the focus
/// state once (MessageFocusStateChanged = 15; GetCurrentFocusState then says InFocus), as Ryujinx's applet state
/// manager does. Kirby and the Forgotten Land (FocusHandlingMode_SuspendAndNotify) ignored all input without it.
static OE_MESSAGES: Mutex<Option<VecDeque<u32>>> = Mutex::new(None);
pub fn oe_push_notification_message(msg: u32) {
    let mut g = OE_MESSAGES.lock().unwrap();
    let q = g.get_or_insert_with(|| VecDeque::from([15u32]));
    q.push_back(msg);
}
fn oe_pop() -> Option<u32> {
    OE_MESSAGES
        .lock()
        .unwrap()
        .get_or_insert_with(|| VecDeque::from([15u32]))
        .pop_front()
}
unsafe extern "C" fn oe_try_pop_notification_message(out: *mut u32) -> bool {
    match oe_pop() {
        Some(m) => {
            if !out.is_null() {
                *out = m;
            }
            true
        }
        None => false,
    }
}
unsafe extern "C" fn oe_pop_notification_message() -> u32 {
    oe_pop().unwrap_or(0)
}
unsafe extern "C" fn oe_get_program_total_active_time() -> i64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START
        .get_or_init(std::time::Instant::now)
        .elapsed()
        .as_nanos() as i64
}
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
const HOST_FONT: &str = "/System/Library/Fonts/Supplemental/Arial.ttf";
static SHARED_FONTS: [std::sync::OnceLock<Vec<u8>>; 6] = [const { std::sync::OnceLock::new() }; 6];
static EMPTY_SHARED_FONT: [u8; 16] = [0; 16];

fn load_shared_font(ty: usize) -> Vec<u8> {
    let dir = crate::settings::get().files.shared_fonts.clone();
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    if let Some(d) = &dir {
        paths.push(d.join(SHARED_FONT_FILES[ty]));
        paths.push(d.join(SHARED_FONT_FILES[0]));
    }
    paths.push(HOST_FONT.into());
    for p in &paths {
        if let Ok(bytes) = std::fs::read(p) {
            println!("[+] nn::pl: shared font {} from {} ({} bytes)", ty, p.display(), bytes.len());
            return bytes;
        }
    }
    eprintln!("[!] nn::pl: no shared font {} (--shared-fonts)", SHARED_FONT_FILES[ty]);
    Vec::new()
}

fn shared_font(ty: i32) -> &'static [u8] {
    let ty = if (0..6).contains(&ty) { ty as usize } else { 0 };
    SHARED_FONTS[ty].get_or_init(|| load_shared_font(ty))
}

unsafe extern "C" fn pl_get_shared_font_load_state(_ty: i32) -> i32 {
    1
}
unsafe extern "C" fn pl_get_shared_font_size(ty: i32) -> usize {
    shared_font(ty).len()
}
unsafe extern "C" fn pl_get_shared_font_address(ty: i32) -> *const u8 {
    let font = shared_font(ty);
    if font.is_empty() {
        EMPTY_SHARED_FONT.as_ptr()
    } else {
        font.as_ptr()
    }
}
unsafe extern "C" fn pl_vr_mode() {}
unsafe extern "C" fn account_get_user_existence(out: *mut bool, _uid: *const u8) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    RESULT_SUCCESS
}
// ── nn::friends: an offline user with an empty friend list (the SDK's own Initialize opens friend:u itself, whose IPC
// is not emulated: Super Mario Bros. Wonder aborted there at boot) ──────────────────────────────────────────────────────
unsafe extern "C" fn friends_void() {}
unsafe extern "C" fn friends_ok() -> u32 {
    RESULT_SUCCESS
}
unsafe extern "C" fn friends_out_true(out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn friends_get_friend_list(out_count: *mut i32) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn friends_get_play_history_key(out_key: *mut u8) -> u32 {
    if !out_key.is_null() {
        std::ptr::write_bytes(out_key, 0, 0x40); // PlayHistoryRegistrationKey
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn friends_false() -> bool {
    false
}
unsafe extern "C" fn friends_zero() -> u64 {
    0
}
static FRIENDS_EMPTY: [u8; 0x100] = [0; 0x100];
unsafe extern "C" fn friends_empty_ref() -> *const u8 {
    FRIENDS_EMPTY.as_ptr()
}
unsafe extern "C" fn friends_not_found() -> u32 {
    121 | (1 << 9) // a friends-module failure: there is no friend to have an image
}

// ── nn::account: one local user (the uid TryOpenPreselectedUser / GetUserId report, horizon/hle.rs), no network account ──────
const USER_UID: [u64; 2] = [0x0123456789ABCDEF, 0xFEDCBA9876543210];
unsafe extern "C" fn account_list_users(out_count: *mut i32, out: *mut [u64; 2], max: i32) -> u32 {
    let n = if max > 0 && !out.is_null() {
        *out = USER_UID;
        1
    } else {
        0
    };
    if !out_count.is_null() {
        *out_count = n;
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn account_get_user_count(out: *mut i32) -> u32 {
    if !out.is_null() {
        *out = 1;
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn account_open_user(out_handle: *mut u128, _uid: *const u8) -> u32 {
    if !out_handle.is_null() {
        *out_handle = 1; // as TryOpenPreselectedUser
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn account_show_user_selector(
    out_uid: *mut [u64; 2],
    _settings: *const u8,
) -> u32 {
    if !out_uid.is_null() {
        *out_uid = USER_UID;
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn account_out_false(out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = false;
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn account_load_profile_image(out_size: *mut usize) -> u32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    124 | (100 << 9) // an account-module failure: the user has no profile image
}
unsafe extern "C" fn fs_is_save_data_existing(_uid: *const u8) -> bool {
    true // saves live in the save directory (horizon/fs.rs); EnsureSaveData / MountSaveData create it
}

/// nn::time::GetDeviceLocationName(LocationName*): 36-byte time zone name (the SDK's own asks the time service).
unsafe extern "C" fn time_get_device_location_name(out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 36);
        std::ptr::copy_nonoverlapping(b"UTC".as_ptr(), out, 3);
    }
}

/// nn::oe::GetPseudoDeviceId(nn::util::Uuid*): a fixed id (the SDK's own asks a service).
unsafe extern "C" fn oe_get_pseudo_device_id(out: *mut u8) {
    if !out.is_null() {
        let id: [u8; 16] = *b"nxport-device-id";
        std::ptr::copy_nonoverlapping(id.as_ptr(), out, 16);
    }
}

// ── nn::ngc::ProfanityFilter: every word checks clean (ngc:u is not emulated; the SDK's Initialize opens it) ──────────
unsafe extern "C" fn ngc_ok() -> u32 {
    RESULT_SUCCESS
}
unsafe extern "C" fn ngc_check_words(
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

unsafe extern "C" fn err_show_error(result: u32) {
    eprintln!(
        "[!] nn::err::ShowError(result={:#x}: module {}, description {})",
        result,
        result & 0x1ff,
        result >> 9
    );
}

// ── nn::diag ─────────────────────────────────────────────────────────────────────────────────
unsafe extern "C" fn diag_get_symbol_name(buf: *mut u8, size: usize, _addr: usize) -> usize {
    if !buf.is_null() && size > 0 {
        *buf = 0;
    }
    0
}
unsafe extern "C" fn diag_get_backtrace(_out: *mut usize, _count: i32) -> i32 {
    0
}
unsafe extern "C" fn diag_is_debugger_attached() -> bool {
    false
}
/// nn::diag::detail::PutImpl(const LogMetaData&, const char* msg, size_t len): the game's log sink.
unsafe extern "C" fn diag_put_impl(_meta: *const u8, msg: *const u8, len: usize) {
    if !msg.is_null() && len > 0 && len < 0x10000 {
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(msg, len));
        println!("[nn::diag] {}", s.trim_end());
    }
}
unsafe extern "C" fn diag_abort_impl_noargs() {
    // The HLE trampoline leaves the guest's return address in x17; x29 still holds the guest frame chain.
    let (lr, fp): (usize, usize);
    std::arch::asm!("mov {}, x17", "mov {}, x29", out(reg) lr, out(reg) fp);
    let base = crate::cpu::crash_handler::get_guest_base();
    let rel = |a: usize| {
        if a >= base && a < base + (256 << 20) {
            format!("main+{:#x}", a - base)
        } else {
            format!("{:#x}", a)
        }
    };
    let mut chain = vec![rel(lr)];
    let mut f = fp;
    for _ in 0..12 {
        if f < 0x10000 || f & 7 != 0 {
            break;
        }
        let ret = *((f + 8) as *const usize);
        chain.push(rel(ret));
        f = *(f as *const usize);
    }
    eprintln!(
        "[!] CRITICAL ABORT: nn::diag::detail::AbortImpl() called from {}",
        chain.join(" <- ")
    );
    std::process::abort();
}
unsafe extern "C" fn detail_unexpected_default_impl() {
    eprintln!(
        "[!] CRITICAL ABORT: nn::detail::UnexpectedDefaultImpl() (a switch default was reached)"
    );
    std::process::abort();
}

// ── nn::swkbd / nn::nfp / nn::hid / nn::prepo ────────────────────────────────────────────────
unsafe extern "C" fn swkbd_get_required_work_buffer_size(_is_unicode: bool) -> usize {
    0x4000
}
unsafe extern "C" fn swkbd_get_required_string_buffer_size() -> usize {
    0x2000
}
unsafe extern "C" fn swkbd_make_preset(_cfg: *mut u8, _preset: i32) {}
unsafe extern "C" fn swkbd_show_keyboard(_out: *mut u8, _arg: *const u8) -> u32 {
    0x1 // no software keyboard applet: report failure so callers fall back / cancel
}
// nfp: no NFC hardware. State 0 = None.
const NFP_RESULT_NOT_AVAILABLE: u32 = 0x1;
unsafe extern "C" fn nfp_initialize() -> u32 {
    RESULT_SUCCESS
}
unsafe extern "C" fn nfp_finalize() {}
unsafe extern "C" fn nfp_get_state() -> i32 {
    0
}
unsafe extern "C" fn nfp_attach_event(ev: *mut u8) {
    if !ev.is_null() {
        hle_system_event_register_addr(ev as usize);
    }
}
unsafe extern "C" fn nfp_attach_event_with_handle(ev: *mut u8, _handle: *const u8) -> u32 {
    if !ev.is_null() {
        hle_system_event_register_addr(ev as usize);
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn nfp_list_devices(_handles: *mut u8, out_count: *mut i32, _max: i32) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    RESULT_SUCCESS
}
unsafe extern "C" fn nfp_get_device_state(_handle: *const u8) -> i32 {
    0
}
unsafe extern "C" fn nfp_fail1(_a: *const u8) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}
unsafe extern "C" fn nfp_fail2(_a: *mut u8, _b: *const u8) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}
unsafe extern "C" fn nfp_mount(_h: *const u8, _m: i32, _t: i32) -> u32 {
    NFP_RESULT_NOT_AVAILABLE
}
unsafe extern "C" fn hid_get_states_zero(_out: *mut u8, _count: i32, _id: *const u8) -> i32 {
    crate::host::input::log_hid(
        "GetNpadStates(JoyLeft/JoyRight/SixAxis)",
        if _id.is_null() {
            u32::MAX
        } else {
            *(_id as *const u32)
        },
        _count,
    );
    0 // no controller input source yet: report zero new states
}
unsafe extern "C" fn hid_disconnect_npad(_id: *const u8) {}
unsafe extern "C" fn noop_void() {}
unsafe extern "C" fn noop_obj(_this: *mut u8) {}
unsafe extern "C" fn prepo_zero_u32() -> u32 {
    RESULT_SUCCESS
}
unsafe extern "C" fn prepo_save_result(_this: *mut u8) -> u32 {
    RESULT_SUCCESS
}
unsafe extern "C" fn prepo_get_size(_this: *const u8) -> usize {
    0
}

/// nn::web::ShowOfflineHtmlPage(OfflineHtmlPageReturnValue*, const ShowOfflineHtmlPageArg&): the
/// page "closes at once" as if Back was pressed. OfflineHtmlPageReturnValue: u64 exit reason
/// (OfflineExitReason_BackButtonPressed = 1), char lastUrl[4096], u64 lastUrlSize.
unsafe extern "C" fn web_show_offline_html_page(out: *mut u8, _arg: *const u8) -> u32 {
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

/// SDK-internal functions to interpose with HLE even though the game doesn't import them.
pub const SDK_INTERPOSE: &[(&str, unsafe extern "C" fn(*const u8, usize))] = &[
    ("_ZN2nn2os14FlushDataCacheEPKvm", cache_noop),
    ("_ZN2nn5audio3dsp14FlushDataCacheEPKvm", cache_noop),
    ("_ZN2nn5audio3dsp19InvalidateDataCacheEPvm", cache_noop),
];

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
unsafe extern "C" fn bcat_initialize() {}
unsafe extern "C" fn bcat_mount_delivery_cache_storage() -> u32 {
    0
}
unsafe extern "C" fn bcat_unmount_delivery_cache_storage() {}
unsafe extern "C" fn bcat_enumerate_delivery_cache_directory(
    out_count: *mut i32,
    _names: *mut u8,
    _count: i32,
) -> u32 {
    if !out_count.is_null() {
        *out_count = 0;
    }
    0
}
unsafe extern "C" fn bcat_request_sync_delivery_cache(_progress: *mut u8) -> u32 {
    BCAT_INTERNET_REQUEST_NOT_ACCEPTED
}
unsafe extern "C" fn bcat_progress_update(_this: *mut u8) {}
unsafe extern "C" fn bcat_progress_get_status(_this: *const u8) -> i32 {
    9
} // DeliveryCacheProgressStatus_Done
unsafe extern "C" fn bcat_progress_get_result(_this: *const u8) -> u32 {
    BCAT_INTERNET_REQUEST_NOT_ACCEPTED
}
unsafe extern "C" fn bcat_open_not_found(_this: *mut u8, _name: *const u8) -> u32 {
    BCAT_NOT_FOUND
}
unsafe extern "C" fn bcat_file_read(
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
unsafe extern "C" fn bcat_directory_read(
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
unsafe extern "C" fn bcat_close(_this: *mut u8) {}

/// nn::prepo::Initialize: would open the prepo service (play reports are discarded, see `lookup`).
unsafe extern "C" fn prepo_initialize() {}

// nn::ec (shop access): no network. The library initializes; requests fail
// with ResultInternetRequestNotAccepted (module 164, description 56). Constructors / destructors run
// in the SDK.
const EC_INTERNET_REQUEST_NOT_ACCEPTED: u32 = 164 | (56 << 9);
unsafe extern "C" fn ec_initialize_for_shop_service_accessors(_work: *mut u8, _size: usize) -> u32 {
    0
}
unsafe extern "C" fn ec_finalize_for_shop_service_accessors() {}
unsafe extern "C" fn ec_accessor_initialize(_this: *mut u8, _service: *const u8) -> u32 {
    0
}
unsafe extern "C" fn ec_accessor_finalize(_this: *mut u8) {}
unsafe extern "C" fn ec_accessor_request(
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
unsafe extern "C" fn ec_response_try_wait(_this: *mut u8) -> bool {
    true
}
unsafe extern "C" fn ec_response_cancel(_this: *mut u8) {}
unsafe extern "C" fn ec_response_get_size(_this: *mut u8, out: *mut usize) -> u32 {
    if !out.is_null() {
        *out = 0;
    }
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}
unsafe extern "C" fn ec_response_get(_this: *mut u8, _buf: *mut u8, _size: usize) -> u32 {
    EC_INTERNET_REQUEST_NOT_ACCEPTED
}
/// Opens the eShop product page applet; nothing to show.
unsafe extern "C" fn ec_show_shop_product_details(ns_uid: u64, _user: *const u8) {
    println!(
        "[+] HLE: nn::ec::ShowShopProductDetails(nsUid={:#x}): no eShop",
        ns_uid
    );
}

// nn::account network service account: the user has none linked / no network,
// ResultNetworkServiceAccountUnavailable (module 124, description 200). An AsyncContext the
// failed calls never started reports done with that result; its event is signaled.
const ACCOUNT_NSA_UNAVAILABLE: u32 = 124 | (200 << 9);
unsafe extern "C" fn account_nsa_unavailable_1(_a: *mut u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}
unsafe extern "C" fn account_nsa_unavailable_2(_a: *mut u8, _b: *const u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}
unsafe extern "C" fn account_load_nsa_id_token_cache(
    out_size: *mut usize,
    _buf: *mut u8,
    _size: usize,
    _user: *const u8,
) -> u32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    ACCOUNT_NSA_UNAVAILABLE
}
unsafe extern "C" fn account_async_has_done(_this: *mut u8, out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    0
}
unsafe extern "C" fn account_async_get_result(_this: *mut u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}
unsafe extern "C" fn account_async_cancel(_this: *mut u8) -> u32 {
    0
}
unsafe extern "C" fn account_async_get_system_event(_this: *mut u8, out_event: *mut u8) -> u32 {
    if !out_event.is_null() {
        crate::horizon::hle::hle_system_event_register_signaled(out_event as usize);
    }
    0
}

/// nn::pdm::QueryApplicationPlayStatistics(out, uid, appIds, count) -> number of entries written: no play records.
unsafe extern "C" fn pdm_query_application_play_statistics(
    _out: *mut u8,
    _uid: *const u8,
    _ids: *const u8,
    _count: i32,
) -> i32 {
    0
}

/// nn::err::ShowError(ErrorCode{category, number}): the error applet; logged.
unsafe extern "C" fn err_show_error_code(code: u64) {
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
unsafe extern "C" fn socket_fail() -> i32 {
    -1
}
unsafe extern "C" fn socket_ok() -> i32 {
    0
}
unsafe extern "C" fn socket_get_last_errno() -> i32 {
    SOCKET_ENETDOWN
}
unsafe extern "C" fn socket_get_addr_info(
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
unsafe extern "C" fn socket_free_addr_info(_ai: *mut u8) {}
/// nn::ssl without a network still works locally on a console: the library initializes, contexts are created and
/// certificates imported; connections fail at the socket level (above). Interposed on the SDK too (nn::ssl::Initialize
/// is called from inside it).
pub unsafe extern "C" fn ssl_ok() -> u32 {
    0
}
/// nifm connection queries without a connection: NoInternetConnection (module 110, description 300, as
/// Ryujinx reports it).
unsafe extern "C" fn nifm_no_internet_connection(_out: *mut u8) -> u32 {
    110 | (300 << 9)
}
unsafe extern "C" fn nifm_cancel_network_request() {}
/// HandleNetworkRequestErrorResult: false = the error was not resolved.
unsafe extern "C" fn nifm_handle_network_request_error_result() -> bool {
    false
}

/// Mangled-name -> HLE function address for the symbols implemented in this module.
pub fn lookup(name: &str) -> Option<usize> {
    if let Some(a) = crate::horizon::movie::lookup(name) {
        return Some(a);
    }
    if let Some(a) = crate::horizon::audio::out::lookup(name) {
        return Some(a);
    }
    macro_rules! f {
        ($f:expr) => {
            Some($f as *const () as usize)
        };
    }
    match name {
        // nn::web offline pages (Kirby's information page behind A on its first screen): library applets do not exist
        // here, and the SDK's applet code dereferenced null.
        "_ZN2nn3web19ShowOfflineHtmlPageEPNS0_26OfflineHtmlPageReturnValueERKNS0_22ShowOfflineHtmlPageArgE" => f!(web_show_offline_html_page),
        // nn::os Event
        "_ZN2nn2os15InitializeEventEPNS0_9EventTypeEbNS0_14EventClearModeE" => f!(os_initialize_event),
        "_ZN2nn2os13FinalizeEventEPNS0_9EventTypeE" => f!(os_finalize_event),
        "_ZN2nn2os11SignalEventEPNS0_9EventTypeE" => f!(os_signal_event),
        "_ZN2nn2os10ClearEventEPNS0_9EventTypeE" => f!(os_clear_event),
        // nn::os MultiWait
        "_ZN2nn2os19InitializeMultiWaitEPNS0_13MultiWaitTypeE" => f!(os_initialize_multiwait),
        "_ZN2nn2os17FinalizeMultiWaitEPNS0_13MultiWaitTypeE" => f!(os_finalize_multiwait),
        "_ZN2nn2os25InitializeMultiWaitHolderEPNS0_19MultiWaitHolderTypeEPNS0_9EventTypeE" => f!(os_initialize_multiwait_holder_event),
        "_ZN2nn2os23FinalizeMultiWaitHolderEPNS0_19MultiWaitHolderTypeE" => f!(os_finalize_multiwait_holder),
        "_ZN2nn2os19LinkMultiWaitHolderEPNS0_13MultiWaitTypeEPNS0_19MultiWaitHolderTypeE" => f!(os_link_multiwait_holder),
        "_ZN2nn2os21UnlinkMultiWaitHolderEPNS0_19MultiWaitHolderTypeE" => f!(os_unlink_multiwait_holder),
        "_ZN2nn2os7WaitAnyEPNS0_13MultiWaitTypeE" => f!(os_wait_any),
        // nn::os MessageQueue
        "_ZN2nn2os22InitializeMessageQueueEPNS0_16MessageQueueTypeEPmm" => f!(os_initialize_message_queue),
        "_ZN2nn2os20FinalizeMessageQueueEPNS0_16MessageQueueTypeE" => f!(os_finalize_message_queue),
        "_ZN2nn2os16SendMessageQueueEPNS0_16MessageQueueTypeEm" => f!(os_send_message_queue),
        "_ZN2nn2os19ReceiveMessageQueueEPmPNS0_16MessageQueueTypeE" => f!(os_receive_message_queue),
        "_ZN2nn2os19TrySendMessageQueueEPNS0_16MessageQueueTypeEm" => f!(os_try_send_message_queue),
        "_ZN2nn2os15JamMessageQueueEPNS0_16MessageQueueTypeEm" => f!(os_jam_message_queue),
        "_ZN2nn2os22TryReceiveMessageQueueEPmPNS0_16MessageQueueTypeE" => f!(os_try_receive_message_queue),
        "_ZN2nn2os24TimedReceiveMessageQueueEPmPNS0_16MessageQueueTypeENS_8TimeSpanE" => f!(os_timed_receive_message_queue),
        "_ZN2nn2os9WaitEventEPNS0_9EventTypeE" => f!(os_wait_event),
        "_ZN2nn2os12TryWaitEventEPNS0_9EventTypeE" => f!(os_try_wait_event),
        "_ZN2nn2os14TimedWaitEventEPNS0_9EventTypeENS_8TimeSpanE" => f!(os_timed_wait_event),
        "_ZN2nn2os26SetMultiWaitHolderUserDataEPNS0_19MultiWaitHolderTypeEm" => f!(os_set_multiwait_holder_user_data),
        "_ZN2nn2os26GetMultiWaitHolderUserDataEPKNS0_19MultiWaitHolderTypeE" => f!(os_get_multiwait_holder_user_data),
        // nn::os RW lock try-variants, misc
        "_ZN2nn2os19TryAcquireWriteLockEPNS0_20ReaderWriterLockTypeE" => f!(os_try_acquire_write_lock),
        "_ZN2nn2os18TryAcquireReadLockEPNS0_20ReaderWriterLockTypeE" => f!(os_try_acquire_read_lock),
        "_ZN2nn2os21SetDyingMessageRegionEmm" => f!(os_set_dying_message_region),
        "_ZN2nn2os15GetCurrentFiberEv" => f!(os_get_current_fiber),
        "_ZN2nn2os13ConvertToTickENS_8TimeSpanE" => f!(os_convert_to_tick),
        // nn::time
        "_ZN2nn4time10InitializeEv" => f!(time_initialize),
        "_ZN2nn4time21GetDeviceLocationNameEPNS0_12LocationNameE" => f!(time_get_device_location_name),
        "_ZN2nn4time8FinalizeEv" => f!(time_finalize),
        "_ZN2nn4time13IsInitializedEv" => f!(time_is_initialized),
        "_ZN2nn4time23StandardUserSystemClock14GetCurrentTimeEPNS0_9PosixTimeE" => f!(time_clock_get_current_time),
        "_ZN2nn4time26StandardNetworkSystemClock14GetCurrentTimeEPNS0_9PosixTimeE" => f!(time_clock_get_current_time),
        "_ZN2nn4time19StandardSteadyClock3nowEv" => f!(time_steady_clock_now),
        "_ZN2nn4time13ClockSnapshotC1Ev" => f!(time_clock_snapshot_ctor),
        "_ZN2nn4time13ClockSnapshot29CreateWithStandardSystemClockEPS1_" => f!(time_clock_snapshot_create),
        "_ZN2nn4time20CalculateSpanBetweenEPNS_8TimeSpanERKNS0_13ClockSnapshotES5_" => f!(time_calculate_span_between),
        "_ZN2nn4time48CalculateStandardUserSystemClockDifferenceByUserERKNS0_13ClockSnapshotES3_" => f!(time_user_clock_difference),
        "_ZN2nn4time14ToCalendarTimeEPNS0_12CalendarTimeEPNS0_22CalendarAdditionalInfoERKNS0_9PosixTimeE" => f!(time_to_calendar_time),
        "_ZN2nn4time19ToCalendarTimeInUtcERKNS0_9PosixTimeE" => f!(time_to_calendar_time_in_utc),
        "_ZN2nn4time18ToPosixTimeFromUtcERKNS0_12CalendarTimeE" => f!(time_to_posix_time_from_utc),
        // nn::oe / pl / account / err
        "_ZN2nn2oe24GetExpectedVolumeBalanceEPfS1_" => f!(oe_get_expected_volume_balance),
        "_ZN2nn2oe24SetExpectedVolumeBalanceEff" => f!(oe_set_expected_volume_balance),
        "_ZN2nn2oe17FinishStartupLogoEv" => f!(oe_finish_startup_logo),
        // Tells the system how to rotate captured screenshots (IPC to the applet manager); there is none to tell.
        "_ZN2nn2oe29SetScreenShotImageOrientationENS_5album16ImageOrientationE" => f!(noop_void),
        // nn::prepo::Initialize (the rest of prepo is below)
        "_ZN2nn5prepo10InitializeEv" => f!(prepo_initialize),
        // nn::ec
        "_ZN2nn2ec33InitializeForShopServiceAccessorsEPvm" => f!(ec_initialize_for_shop_service_accessors),
        "_ZN2nn2ec31FinalizeForShopServiceAccessorsEv" => f!(ec_finalize_for_shop_service_accessors),
        "_ZN2nn2ec19ShopServiceAccessor10InitializeERKNS0_11ShopServiceE" => f!(ec_accessor_initialize),
        "_ZN2nn2ec19ShopServiceAccessor8FinalizeEv" => f!(ec_accessor_finalize),
        "_ZN2nn2ec19ShopServiceAccessor7RequestEPNS1_13AsyncResponseERKNS_7account3UidENS0_11ShopService6MethodEPKcRKNS1_8PostDataENS_8TimeSpanE" => f!(ec_accessor_request),
        "_ZN2nn2ec19ShopServiceAccessor13AsyncResponse7TryWaitEv" => f!(ec_response_try_wait),
        "_ZN2nn2ec19ShopServiceAccessor13AsyncResponse6CancelEv" => f!(ec_response_cancel),
        "_ZN2nn2ec19ShopServiceAccessor13AsyncResponse7GetSizeEPm" => f!(ec_response_get_size),
        "_ZN2nn2ec19ShopServiceAccessor13AsyncResponse3GetEPvm" => f!(ec_response_get),
        "_ZN2nn2ec22ShowShopProductDetailsENS0_5NsUidERKNS_7account10UserHandleE" => f!(ec_show_shop_product_details),
        // nn::account network service account
        "_ZN2nn7account36EnsureNetworkServiceAccountAvailableERKNS0_10UserHandleE" => f!(account_nsa_unavailable_1),
        "_ZN2nn7account26GetNetworkServiceAccountIdEPNS0_23NetworkServiceAccountIdERKNS0_10UserHandleE" => f!(account_nsa_unavailable_2),
        "_ZN2nn7account44EnsureNetworkServiceAccountIdTokenCacheAsyncEPNS0_12AsyncContextERKNS0_10UserHandleE" => f!(account_nsa_unavailable_2),
        "_ZN2nn7account37LoadNetworkServiceAccountIdTokenCacheEPmPcmRKNS0_10UserHandleE" => f!(account_load_nsa_id_token_cache),
        "_ZN2nn7account12AsyncContext7HasDoneEPb" => f!(account_async_has_done),
        "_ZN2nn7account12AsyncContext9GetResultEv" => f!(account_async_get_result),
        "_ZN2nn7account12AsyncContext6CancelEv" => f!(account_async_cancel),
        "_ZN2nn7account12AsyncContext14GetSystemEventEPNS_2os11SystemEventE" => f!(account_async_get_system_event),
        // nn::pdm
        "_ZN2nn3pdm30QueryApplicationPlayStatisticsEPNS0_25ApplicationPlayStatisticsERKNS_7account3UidEPKNS_13ApplicationIdEi" => f!(pdm_query_application_play_statistics),
        // nn::err (ErrorCode overload)
        "_ZN2nn3err9ShowErrorENS0_9ErrorCodeE" => f!(err_show_error_code),
        // nn::socket (no network; Socket / Connect / Send / Recv / Initialize are module_loader's runtime overrides)
        "_ZN2nn6socket10InitializeEPvmmi" => f!(socket_ok),
        "_ZN2nn6socket10InitializeERKNS0_6ConfigE" => f!(socket_ok),
        "_ZN2nn6socket8FinalizeEv" => f!(socket_ok),
        "_ZN2nn6socket5CloseEi" => f!(socket_ok),
        "_ZN2nn6socket10GetSockOptEiiiPvPj" => f!(socket_fail),
        "_ZN2nn6socket10SetSockOptEiiiPKvj" => f!(socket_fail),
        "_ZN2nn6socket11GetSockNameEiP8sockaddrPj" => f!(socket_fail),
        "_ZN2nn6socket4BindEiPK8sockaddrj" => f!(socket_fail),
        "_ZN2nn6socket4PollEP6pollfdmi" => f!(socket_fail),
        "_ZN2nn6socket5FcntlEiiz" => f!(socket_fail),
        "_ZN2nn6socket6AcceptEiP8sockaddrPj" => f!(socket_fail),
        "_ZN2nn6socket6ListenEii" => f!(socket_fail),
        "_ZN2nn6socket6SendToEiPKvmiPK8sockaddrj" => f!(socket_fail),
        "_ZN2nn6socket8RecvFromEiPvmiP8sockaddrPj" => f!(socket_fail),
        "_ZN2nn6socket8ShutdownEii" => f!(socket_fail),
        "_ZN2nn6socket12GetLastErrnoEv" => f!(socket_get_last_errno),
        "_ZN2nn6socket11GetAddrInfoEPKcS2_PK8addrinfoPPS3_" => f!(socket_get_addr_info),
        "_ZN2nn6socket12FreeAddrInfoEP8addrinfo" => f!(socket_free_addr_info),
        // nn::ssl
        "_ZN2nn3ssl10InitializeEv" => f!(ssl_ok),
        "_ZN2nn3ssl7Context6CreateENS1_10SslVersionE" => f!(ssl_ok),
        "_ZN2nn3ssl7Context15ImportServerPkiEPmPKcjNS0_17CertificateFormatE" => f!(ssl_ok),
        // nn::nifm
        "_ZN2nn4nifm20CancelNetworkRequestEv" => f!(nifm_cancel_network_request),
        "_ZN2nn4nifm31HandleNetworkRequestErrorResultEv" => f!(nifm_handle_network_request_error_result),
        "_ZN2nn4nifm26GetCurrentPrimaryIpAddressEP7in_addr" => f!(nifm_no_internet_connection),
        "_ZN2nn4nifm22GetCurrentProxySettingEPNS0_12ProxySettingE" => f!(nifm_no_internet_connection),
        // nn::bcat
        "_ZN2nn4bcat10InitializeEv" => f!(bcat_initialize),
        "_ZN2nn4bcat25MountDeliveryCacheStorageEv" => f!(bcat_mount_delivery_cache_storage),
        "_ZN2nn4bcat27UnmountDeliveryCacheStorageEv" => f!(bcat_unmount_delivery_cache_storage),
        "_ZN2nn4bcat31EnumerateDeliveryCacheDirectoryEPiPNS0_13DirectoryNameEi" => f!(bcat_enumerate_delivery_cache_directory),
        "_ZN2nn4bcat24RequestSyncDeliveryCacheEPNS0_21DeliveryCacheProgressE" => f!(bcat_request_sync_delivery_cache),
        "_ZN2nn4bcat21DeliveryCacheProgress6UpdateEv" => f!(bcat_progress_update),
        "_ZNK2nn4bcat21DeliveryCacheProgress9GetStatusEv" => f!(bcat_progress_get_status),
        "_ZNK2nn4bcat21DeliveryCacheProgress9GetResultEv" => f!(bcat_progress_get_result),
        "_ZN2nn4bcat17DeliveryCacheFile4OpenEPKc" => f!(bcat_open_not_found),
        "_ZN2nn4bcat17DeliveryCacheFile4ReadEPmlPvm" => f!(bcat_file_read),
        "_ZN2nn4bcat17DeliveryCacheFile5CloseEv" => f!(bcat_close),
        "_ZN2nn4bcat22DeliveryCacheDirectory4OpenEPKc" => f!(bcat_open_not_found),
        "_ZN2nn4bcat22DeliveryCacheDirectory4ReadEPiPNS0_27DeliveryCacheDirectoryEntryEi" => f!(bcat_directory_read),
        "_ZN2nn4bcat22DeliveryCacheDirectory5CloseEv" => f!(bcat_close),
        "_ZN2nn2oe31EnterExitRequestHandlingSectionEv" => f!(oe_enter_exit_request_handling_section),
        "_ZN2nn2oe31LeaveExitRequestHandlingSectionEv" => f!(oe_leave_exit_request_handling_section),
        "_ZN2nn2oe25TryPopNotificationMessageEPj" => f!(oe_try_pop_notification_message),
        "_ZN2nn2oe22PopNotificationMessageEv" => f!(oe_pop_notification_message),
        "_ZN2nn2oe25GetProgramTotalActiveTimeEv" => f!(oe_get_program_total_active_time),
        "_ZN2nn2pl22GetSharedFontLoadStateENS0_14SharedFontTypeE" => f!(pl_get_shared_font_load_state),
        "_ZN2nn2pl17GetSharedFontSizeENS0_14SharedFontTypeE" => f!(pl_get_shared_font_size),
        "_ZN2nn2pl20GetSharedFontAddressENS0_14SharedFontTypeE" => f!(pl_get_shared_font_address),
        "_ZN2nn2pl11BeginVrModeEv" => f!(pl_vr_mode),
        "_ZN2nn2pl9EndVrModeEv" => f!(pl_vr_mode),
        "_ZN2nn7account16GetUserExistenceEPbRKNS0_3UidE" => f!(account_get_user_existence),
        "_ZN2nn3err9ShowErrorENS_6ResultE" => f!(err_show_error),
        "_ZN2nn2oe17GetPseudoDeviceIdEPNS_4util4UuidE" => f!(oe_get_pseudo_device_id),
        "_ZN2nn7account18ListQualifiedUsersEPiPNS0_3UidEi" => f!(account_list_users),
        "_ZN2nn7account12ListAllUsersEPiPNS0_3UidEi" => f!(account_list_users),
        "_ZN2nn7account12GetUserCountEPi" => f!(account_get_user_count),
        "_ZN2nn7account8OpenUserEPNS0_10UserHandleERKNS0_3UidE" => f!(account_open_user),
        "_ZN2nn7account16ShowUserSelectorEPNS0_3UidERKNS0_21UserSelectionSettingsE" => f!(account_show_user_selector),
        "_ZN2nn7account32IsNetworkServiceAccountAvailableEPbRKNS0_10UserHandleE" => f!(account_out_false),
        "_ZN2nn7account16LoadProfileImageEPmPvmRKNS0_3UidE" => f!(account_load_profile_image),
        "_ZN2nn2fs18IsSaveDataExistingERKNS_7account3UidE" => f!(fs_is_save_data_existing),
        // nn::ngc
        "_ZN2nn3ngc15ProfanityFilterC1Ev" => f!(friends_void),
        "_ZN2nn3ngc15ProfanityFilterD1Ev" => f!(friends_void),
        "_ZN2nn3ngc15ProfanityFilter10InitializeEPvmb" => f!(ngc_ok),
        "_ZN2nn3ngc15ProfanityFilter19CheckProfanityWordsEPjjPPKcm" => f!(ngc_check_words),
        "_ZN2nn3ngc15ProfanityFilter19CheckProfanityWordsEPjjPPKDsm" => f!(ngc_check_words),
        "_ZN2nn3ngc15ProfanityFilter19CheckProfanityWordsEPjjPPKtm" => f!(ngc_check_words),
        // nn::friends (offline, no friends)
        "_ZN2nn7friends10InitializeEv" => f!(friends_void),
        "_ZN2nn7friends12SetAllocatorEPFPvmEPFvS1_mE" => f!(friends_void),
        "_ZN2nn7friends12AsyncContextC1Ev" => f!(friends_void),
        "_ZN2nn7friends12AsyncContextD1Ev" => f!(friends_void),
        "_ZN2nn7friends12AsyncContext7HasDoneEPb" => f!(friends_out_true),
        "_ZN2nn7friends25EnsureFriendListAvailableEPNS0_12AsyncContextERKNS_7account3UidE" => f!(friends_ok),
        "_ZN2nn7friends27CheckFriendListAvailabilityEPbRKNS_7account3UidE" => f!(friends_out_true),
        "_ZN2nn7friends13GetFriendListEPiPNS0_6FriendERKNS_7account3UidEiiRKNS0_12FriendFilterE" => f!(friends_get_friend_list),
        "_ZN2nn7friends29GetPlayHistoryRegistrationKeyEPNS0_26PlayHistoryRegistrationKeyERKNS_7account3UidEb" => f!(friends_get_play_history_key),
        "_ZN2nn7friends14AddPlayHistoryERKNS_7account3UidERKNS0_26PlayHistoryRegistrationKeyERKNS0_15InAppScreenNameESA_" => f!(friends_ok),
        "_ZN2nn7friends6FriendC1Ev" => f!(friends_void),
        "_ZNK2nn7friends6Friend7IsValidEv" => f!(friends_false),
        "_ZNK2nn7friends6Friend12GetAccountIdEv" => f!(friends_zero),
        "_ZNK2nn7friends6Friend11GetNicknameEv" => f!(friends_empty_ref),
        "_ZNK2nn7friends6Friend11GetPresenceEv" => f!(friends_empty_ref),
        "_ZNK2nn7friends14FriendPresence9GetStatusEv" => f!(friends_zero),
        "_ZNK2nn7friends6Friend15GetProfileImageEPmPvm" => f!(friends_not_found),
        // nn::diag / nn::detail
        "_ZN2nn4diag13GetSymbolNameEPcmm" => f!(diag_get_symbol_name),
        "_ZN2nn4diag12GetBacktraceEPmi" => f!(diag_get_backtrace),
        "_ZN2nn4diag6detail6PutImplERKNS0_11LogMetaDataEPKcm" => f!(diag_put_impl),
        "_ZN2nn4diag18IsDebuggerAttachedEv" => f!(diag_is_debugger_attached),
        "_ZN2nn4diag6detail9AbortImplEv" => f!(diag_abort_impl_noargs),
        "_ZN2nn6detail21UnexpectedDefaultImplEv" => f!(detail_unexpected_default_impl),
        // nn::swkbd
        "_ZN2nn5swkbd25GetRequiredWorkBufferSizeEb" => f!(swkbd_get_required_work_buffer_size),
        "_ZN2nn5swkbd27GetRequiredStringBufferSizeEv" => f!(swkbd_get_required_string_buffer_size),
        "_ZN2nn5swkbd10MakePresetEPNS0_14KeyboardConfigENS0_6PresetE" => f!(swkbd_make_preset),
        "_ZN2nn5swkbd12ShowKeyboardEPNS0_6StringERKNS0_15ShowKeyboardArgE" => f!(swkbd_show_keyboard),
        // nn::nfp (no NFC hardware)
        "_ZN2nn3nfp10InitializeEv" => f!(nfp_initialize),
        "_ZN2nn3nfp8FinalizeEv" => f!(nfp_finalize),
        "_ZN2nn3nfp8GetStateEv" => f!(nfp_get_state),
        "_ZN2nn3nfp29AttachAvailabilityChangeEventEPNS_2os15SystemEventTypeE" => f!(nfp_attach_event),
        "_ZN2nn3nfp19AttachActivateEventEPNS_2os15SystemEventTypeERKNS0_12DeviceHandleE" => f!(nfp_attach_event_with_handle),
        "_ZN2nn3nfp21AttachDeactivateEventEPNS_2os15SystemEventTypeERKNS0_12DeviceHandleE" => f!(nfp_attach_event_with_handle),
        "_ZN2nn3nfp11ListDevicesEPNS0_12DeviceHandleEPii" => f!(nfp_list_devices),
        "_ZN2nn3nfp14GetDeviceStateERKNS0_12DeviceHandleE" => f!(nfp_get_device_state),
        "_ZN2nn3nfp14StartDetectionERKNS0_12DeviceHandleE" => f!(nfp_fail1),
        "_ZN2nn3nfp13StopDetectionERKNS0_12DeviceHandleE" => f!(nfp_fail1),
        "_ZN2nn3nfp9GetNpadIdEPjRKNS0_12DeviceHandleE" => f!(nfp_fail2),
        "_ZN2nn3nfp10GetTagInfoEPNS0_7TagInfoERKNS0_12DeviceHandleE" => f!(nfp_fail2),
        "_ZN2nn3nfp12GetModelInfoEPNS0_9ModelInfoERKNS0_12DeviceHandleE" => f!(nfp_fail2),
        "_ZN2nn3nfp5MountERKNS0_12DeviceHandleENS0_9ModelTypeENS0_11MountTargetE" => f!(nfp_mount),
        // nn::hid
        "_ZN2nn3hid13GetNpadStatesEPNS0_16NpadJoyLeftStateEiRKj" => f!(hid_get_states_zero),
        "_ZN2nn3hid13GetNpadStatesEPNS0_17NpadJoyRightStateEiRKj" => f!(hid_get_states_zero),
        "_ZN2nn3hid22GetSixAxisSensorStatesEPNS0_18SixAxisSensorStateEiRKNS0_19SixAxisSensorHandleE" => f!(hid_get_states_zero),
        "_ZN2nn3hid14DisconnectNpadERKj" => f!(hid_disconnect_npad),
        // nn::prepo (play reports go nowhere)
        "_ZN2nn5prepo10PlayReportC1Ev" => f!(noop_obj),
        "_ZN2nn5prepo10PlayReport5ClearEv" => f!(noop_obj),
        "_ZN2nn5prepo10PlayReport7GetSizeEv" => f!(prepo_get_size),
        // Reports are discarded (SetBuffer / Add* below never store), so they hold no key-value pairs.
        "_ZNK2nn5prepo10PlayReport8GetCountEv" => f!(prepo_get_size),
        "_ZN2nn5prepo10PlayReport4SaveEv" => f!(prepo_save_result),
        "_ZN2nn5prepo28RequestImmediateTransmissionEv" => f!(prepo_zero_u32),
        _ => {
            // PlayReport::Add* / SetBuffer / SetEventId / Save(Uid) / AddInternetConnectionStatus:
            // all discard their arguments.
            if name.starts_with("_ZN2nn5prepo10PlayReport3Add")
                || name.starts_with("_ZN2nn5prepo10PlayReport9SetBuffer")
                || name.starts_with("_ZN2nn5prepo10PlayReport10SetEventId")
                || name.starts_with("_ZN2nn5prepo10PlayReport4SaveERK")
                || name.starts_with("_ZN2nn5prepo27AddInternetConnectionStatus")
            {
                return f!(prepo_save_result);
            }
            let _ = noop_void;
            None
        }
    }
}
