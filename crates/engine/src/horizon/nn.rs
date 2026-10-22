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
    /// Signals so far: a manual-clear waiter returns once one arrived after it began, even if cleared again since.
    signals: u64,
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
            signals: 0,
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
        e.signals += 1;
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
pub unsafe extern "C" fn os_wait_event(ev: *mut u8) {
    event_wait(ev as usize, None);
}
pub(crate) unsafe extern "C" fn os_try_wait_event(ev: *mut u8) -> bool {
    event_wait(ev as usize, Some(std::time::Instant::now()))
}
pub(crate) unsafe extern "C" fn os_timed_wait_event(ev: *mut u8, timeout_ns: i64) -> bool {
    event_wait(
        ev as usize,
        Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64)),
    )
}

// ── nn::os::MultiWait ────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn os_initialize_multiwait(mw: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .multiwaits
        .insert(mw as usize, Vec::new());
}
pub(crate) unsafe extern "C" fn os_finalize_multiwait(mw: *mut u8) {
    lock().as_mut().unwrap().multiwaits.remove(&(mw as usize));
}
pub(crate) unsafe extern "C" fn os_initialize_multiwait_holder_event(holder: *mut u8, ev: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .holders
        .insert(holder as usize, ev as usize);
}
pub(crate) unsafe extern "C" fn os_finalize_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    let s = g.as_mut().unwrap();
    s.holders.remove(&(holder as usize));
    for v in s.multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}
pub(crate) unsafe extern "C" fn os_link_multiwait_holder(mw: *mut u8, holder: *mut u8) {
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
pub(crate) unsafe extern "C" fn os_set_multiwait_holder_user_data(holder: *mut u8, data: usize) {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(holder as usize, data);
}
pub(crate) unsafe extern "C" fn os_get_multiwait_holder_user_data(holder: *const u8) -> usize {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&(holder as usize)).copied())
        .unwrap_or(0)
}
pub(crate) unsafe extern "C" fn os_unlink_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    for v in g.as_mut().unwrap().multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}
/// Blocks until any linked holder's event is signaled; returns that holder (auto-clear events are consumed).
pub(crate) unsafe extern "C" fn os_wait_any(mw: *mut u8) -> *mut u8 {
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
pub(crate) unsafe extern "C" fn os_initialize_message_queue(mq: *mut u8, _buf: *mut u64, count: usize) {
    lock().as_mut().unwrap().msgqs.insert(
        mq as usize,
        MsgQ {
            cap: count.max(1),
            q: VecDeque::new(),
        },
    );
}
pub(crate) unsafe extern "C" fn os_finalize_message_queue(mq: *mut u8) {
    lock().as_mut().unwrap().msgqs.remove(&(mq as usize));
}
pub(crate) unsafe extern "C" fn os_send_message_queue(mq: *mut u8, data: u64) {
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
pub(crate) unsafe extern "C" fn os_receive_message_queue(out: *mut u64, mq: *mut u8) {
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
/// `peek`: read the head without removing it.
fn mq_receive(out: *mut u64, mq: usize, deadline: Option<std::time::Instant>, peek: bool) -> bool {
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
pub(crate) unsafe extern "C" fn os_try_send_message_queue(mq: *mut u8, data: u64) -> bool {
    mq_send(mq as usize, data, false, Some(std::time::Instant::now()))
}
pub(crate) unsafe extern "C" fn os_jam_message_queue(mq: *mut u8, data: u64) {
    mq_send(mq as usize, data, true, None);
}
pub(crate) unsafe extern "C" fn os_try_receive_message_queue(out: *mut u64, mq: *mut u8) -> bool {
    mq_receive(out, mq as usize, Some(std::time::Instant::now()), false)
}
pub(crate) unsafe extern "C" fn os_timed_receive_message_queue(
    out: *mut u64,
    mq: *mut u8,
    timeout_ns: i64,
) -> bool {
    mq_receive(
        out,
        mq as usize,
        Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64)),
        false,
    )
}
fn deadline_ns(timeout_ns: i64) -> Option<std::time::Instant> {
    Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64))
}
pub(crate) unsafe extern "C" fn os_peek_message_queue(out: *mut u64, mq: *const u8) {
    mq_receive(out, mq as usize, None, true);
}
pub(crate) unsafe extern "C" fn os_try_peek_message_queue(out: *mut u64, mq: *const u8) -> bool {
    mq_receive(out, mq as usize, Some(std::time::Instant::now()), true)
}
pub(crate) unsafe extern "C" fn os_timed_peek_message_queue(out: *mut u64, mq: *const u8, timeout_ns: i64) -> bool {
    mq_receive(out, mq as usize, deadline_ns(timeout_ns), true)
}
pub(crate) unsafe extern "C" fn os_try_jam_message_queue(mq: *mut u8, data: u64) -> bool {
    mq_send(mq as usize, data, true, Some(std::time::Instant::now()))
}
pub(crate) unsafe extern "C" fn os_timed_send_message_queue(mq: *mut u8, data: u64, timeout_ns: i64) -> bool {
    mq_send(mq as usize, data, false, deadline_ns(timeout_ns))
}
pub(crate) unsafe extern "C" fn os_timed_jam_message_queue(mq: *mut u8, data: u64, timeout_ns: i64) -> bool {
    mq_send(mq as usize, data, true, deadline_ns(timeout_ns))
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
pub(crate) unsafe extern "C" fn os_try_acquire_write_lock(rw: *mut u8) -> bool {
    rw_try_acquire_write(rw as usize)
}
pub(crate) unsafe extern "C" fn os_try_acquire_read_lock(rw: *mut u8) -> bool {
    rw_try_acquire_read(rw as usize)
}

// ── misc nn::os ──────────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn os_set_dying_message_region(_addr: usize, _size: usize) {}
/// nn::os::GetCurrentFiber: ThreadType::_currentFiber, which the SDK's native SwitchToFiber maintains.
pub(crate) unsafe extern "C" fn os_get_current_fiber() -> *mut u8 {
    let t = crate::horizon::hle::hle_nn_os_GetCurrentThread();
    if t.is_null() {
        std::ptr::null_mut()
    } else {
        (*t).current_fiber as *mut u8
    }
}
/// nn::os::ConvertToTick(TimeSpan) -> Tick, consistent with GetSystemTick (host mach ticks).
pub(crate) unsafe extern "C" fn os_convert_to_tick(ns: i64) -> i64 {
    crate::clock::ns_to_ticks(ns.max(0) as u64) as i64
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
    crate::clock::now_ns() as i64
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
pub(crate) unsafe extern "C" fn time_to_calendar_time_in_utc(posix: *const i64) -> u64 {
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
    libc::timegm(&mut tm) as i64
}

// ── nn::oe / nn::pl / nn::account / nn::err ──────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn oe_get_expected_volume_balance(app: *mut f32, sdk: *mut f32) {
    if !app.is_null() {
        *app = 1.0;
    }
    if !sdk.is_null() {
        *sdk = 1.0;
    }
}
pub(crate) unsafe extern "C" fn oe_set_expected_volume_balance(_app: f32, _sdk: f32) {}
pub(crate) unsafe extern "C" fn oe_finish_startup_logo() {}
/// Asks the system to deliver MessageExitRequest instead of terminating the program; no exit
/// requests are generated here (quitting the window ends the process), so entering / leaving the section changes nothing.
pub(crate) unsafe extern "C" fn oe_enter_exit_request_handling_section() {}
pub(crate) unsafe extern "C" fn oe_leave_exit_request_handling_section() {}
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
/// The launch parameter channel: a game started normally has none (the native call reached the applet manager's
/// storage objects, which do not exist here, and read through null).
pub(crate) unsafe extern "C" fn oe_try_pop_launch_parameter(out_size: *mut usize, _buf: *mut u8, _cap: usize) -> bool {
    if !out_size.is_null() {
        *out_size = 0;
    }
    false
}
pub(crate) unsafe extern "C" fn oe_try_pop_notification_message(out: *mut u32) -> bool {
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
pub(crate) unsafe extern "C" fn oe_pop_notification_message() -> u32 {
    oe_pop().unwrap_or(0)
}
pub(crate) unsafe extern "C" fn oe_get_program_total_active_time() -> i64 {
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
        all &= !font.is_empty();
        if !out.is_null() {
            let e = out.add(ty * 24);
            (e as *mut u32).write_unaligned(ty as u32);
            (e.add(8) as *mut usize).write_unaligned(pl_get_shared_font_address(ty as i32) as usize);
            (e.add(16) as *mut usize).write_unaligned(font.len());
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
    shared_font(ty).len()
}
pub(crate) unsafe extern "C" fn pl_get_shared_font_address(ty: i32) -> *const u8 {
    let font = shared_font(ty);
    if font.is_empty() {
        EMPTY_SHARED_FONT.as_ptr()
    } else {
        font.as_ptr()
    }
}
pub(crate) unsafe extern "C" fn pl_vr_mode() {}
pub(crate) unsafe extern "C" fn account_get_user_existence(out: *mut bool, _uid: *const u8) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    RESULT_SUCCESS
}
// ── nn::friends: an offline user with an empty friend list (the SDK's own Initialize opens friend:u itself, whose IPC
// is not emulated: Super Mario Bros. Wonder aborted there at boot) ──────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn friends_void() {}
pub(crate) unsafe extern "C" fn friends_ok() -> u32 {
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

// ── nn::account: one local user (the uid TryOpenPreselectedUser / GetUserId report, horizon/hle.rs), no network account ──────
const USER_UID: [u64; 2] = [0x0123456789ABCDEF, 0xFEDCBA9876543210];
pub(crate) unsafe extern "C" fn account_list_users(out_count: *mut i32, out: *mut [u64; 2], max: i32) -> u32 {
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
pub(crate) unsafe extern "C" fn account_get_user_count(out: *mut i32) -> u32 {
    if !out.is_null() {
        *out = 1;
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn account_open_user(out_handle: *mut u128, _uid: *const u8) -> u32 {
    if !out_handle.is_null() {
        *out_handle = 1; // as TryOpenPreselectedUser
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn account_show_user_selector(
    out_uid: *mut [u64; 2],
    _settings: *const u8,
) -> u32 {
    if !out_uid.is_null() {
        *out_uid = USER_UID;
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn account_out_false(out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = false;
    }
    RESULT_SUCCESS
}
pub(crate) unsafe extern "C" fn account_load_profile_image(out_size: *mut usize) -> u32 {
    if !out_size.is_null() {
        *out_size = 0;
    }
    124 | (100 << 9) // an account-module failure: the user has no profile image
}
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

/// nn::oe::GetPseudoDeviceId(nn::util::Uuid*): a fixed id (the SDK's own asks a service).
pub(crate) unsafe extern "C" fn oe_get_pseudo_device_id(out: *mut u8) {
    if !out.is_null() {
        let id: [u8; 16] = *b"nxport-device-id";
        std::ptr::copy_nonoverlapping(id.as_ptr(), out, 16);
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

// ── nn::diag ─────────────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn diag_get_symbol_name(buf: *mut u8, size: usize, _addr: usize) -> usize {
    if !buf.is_null() && size > 0 {
        *buf = 0;
    }
    0
}
pub(crate) unsafe extern "C" fn diag_get_backtrace(_out: *mut usize, _count: i32) -> i32 {
    0
}
/// nn::diag::detail::PutImpl(const LogMetaData&, const char* msg, size_t len): the game's log sink.
pub(crate) unsafe extern "C" fn diag_put_impl(_meta: *const u8, msg: *const u8, len: usize) {
    if !msg.is_null() && len > 0 && len < 0x10000 {
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(msg, len));
        println!("[nn::diag] {}", s.trim_end());
    }
}
pub(crate) unsafe extern "C" fn diag_abort_impl_noargs() {
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
pub(crate) unsafe extern "C" fn swkbd_make_preset(_cfg: *mut u8, _preset: i32) {}
pub(crate) unsafe extern "C" fn swkbd_show_keyboard(_out: *mut u8, _arg: *const u8) -> u32 {
    0x1 // no software keyboard applet: report failure so callers fall back / cancel
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
pub(crate) unsafe extern "C" fn hid_get_states_zero(_out: *mut u8, _count: i32, _id: *const u8) -> i32 {
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
pub(crate) unsafe extern "C" fn aoc_list(_out_indices: *mut i32, _offset: i32, _count: i32) -> i32 {
    0
}
/// nn::os::SystemEvent holds its SystemEventType first; the list never changes, so it is never signaled.
pub(crate) unsafe extern "C" fn aoc_list_changed_event(event: *mut u8) {
    crate::horizon::hle::hle_system_event_register_addr(event as usize);
}

/// Which handheld Joy-Cons count as an active controller (both / either / none); the engine's controller is the
/// keyboard / gamepad, so the mode is only kept for the game to read back.
static HANDHELD_ACTIVATION_MODE: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
pub(crate) unsafe extern "C" fn hid_set_handheld_activation_mode(mode: i64) {
    HANDHELD_ACTIVATION_MODE.store(mode, std::sync::atomic::Ordering::Relaxed);
}
pub(crate) unsafe extern "C" fn hid_get_handheld_activation_mode() -> i64 {
    HANDHELD_ACTIVATION_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

pub(crate) unsafe extern "C" fn hid_disconnect_npad(_id: *const u8) {}
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
const ACCOUNT_NSA_UNAVAILABLE: u32 = 124 | (200 << 9);
pub(crate) unsafe extern "C" fn account_nsa_unavailable_1(_a: *mut u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}
pub(crate) unsafe extern "C" fn account_nsa_unavailable_2(_a: *mut u8, _b: *const u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}
pub(crate) unsafe extern "C" fn account_nsa_unavailable_4(_a: *mut u8, _b: *const u8, _c: *mut u8, _d: usize) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}
pub(crate) unsafe extern "C" fn account_load_nsa_id_token_cache(
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
pub(crate) unsafe extern "C" fn account_async_has_done(_this: *mut u8, out: *mut bool) -> u32 {
    if !out.is_null() {
        *out = true;
    }
    0
}
pub(crate) unsafe extern "C" fn account_async_get_result(_this: *mut u8) -> u32 {
    ACCOUNT_NSA_UNAVAILABLE
}
pub(crate) unsafe extern "C" fn account_async_cancel(_this: *mut u8) -> u32 {
    0
}
pub(crate) unsafe extern "C" fn account_async_get_system_event(_this: *mut u8, out_event: *mut u8) -> u32 {
    if !out_event.is_null() {
        crate::horizon::hle::hle_system_event_register_signaled(out_event as usize);
    }
    0
}

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
pub(crate) unsafe extern "C" fn socket_get_last_errno() -> i32 {
    SOCKET_ENETDOWN
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

