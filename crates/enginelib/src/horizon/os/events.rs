// nn::os events: light and timer events, message queues and multi-waits, as the game calls them (their state lives
// in objects.rs).

use super::*;

pub(crate) unsafe extern "C" fn initialize_light_message_queue(
    mq: *mut (),
    _buf: *mut u64,
    count: usize,
) {
    light_mq_table()
        .lock()
        .unwrap()
        .insert(mq as usize, light_mq_new(count));
}

pub(crate) unsafe extern "C" fn finalize_light_message_queue(mq: *mut ()) {
    light_mq_table().lock().unwrap().remove(&(mq as usize));
}

pub(crate) unsafe extern "C" fn send_light_message_queue(mq: *mut (), data: u64) {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.len() >= inner.capacity {
        inner = state.not_full.wait(inner).unwrap();
    }
    inner.queue.push_back(data);
    state.not_empty.notify_all();
}

pub(crate) unsafe extern "C" fn try_send_light_message_queue(mq: *mut (), data: u64) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.len() >= inner.capacity {
        return false;
    }
    inner.queue.push_back(data);
    state.not_empty.notify_all();
    true
}

pub(crate) unsafe extern "C" fn jam_light_message_queue(mq: *mut (), data: u64) {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.len() >= inner.capacity {
        inner = state.not_full.wait(inner).unwrap();
    }
    inner.queue.push_front(data);
    state.not_empty.notify_all();
}

pub(crate) unsafe extern "C" fn try_jam_light_message_queue(mq: *mut (), data: u64) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.len() >= inner.capacity {
        return false;
    }
    inner.queue.push_front(data);
    state.not_empty.notify_all();
    true
}

pub(crate) unsafe extern "C" fn receive_light_message_queue(out: *mut u64, mq: *mut ()) {
    let fp = crate::guest_reg!(x29);
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    let _blocked = if inner.queue.is_empty() {
        Some(BlockGuard::new("MsgQueueRecv", mq as usize, fp))
    } else {
        None
    };
    while inner.queue.is_empty() {
        inner = state.not_empty.wait(inner).unwrap();
    }
    let v = inner.queue.pop_front().unwrap();
    state.not_full.notify_one();
    drop(inner);
    if !out.is_null() {
        *out = v;
    }
    present_paced_wait();
}

pub(crate) unsafe extern "C" fn try_receive_light_message_queue(out: *mut u64, mq: *mut ()) -> bool {
    let state = light_mq_get(mq);
    let mut inner = state.inner.lock().unwrap();
    match inner.queue.pop_front() {
        Some(v) => {
            state.not_full.notify_one();
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn try_peek_light_message_queue(out: *mut u64, mq: *const ()) -> bool {
    let state = light_mq_get(mq as *mut ());
    let inner = state.inner.lock().unwrap();
    match inner.queue.front() {
        Some(&v) => {
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn peek_light_message_queue(out: *mut u64, mq: *const ()) {
    let state = light_mq_get(mq as *mut ());
    let mut inner = state.inner.lock().unwrap();
    while inner.queue.is_empty() {
        inner = state.not_empty.wait(inner).unwrap();
    }
    if !out.is_null() {
        *out = inner.queue[0];
    }
}

pub(crate) unsafe extern "C" fn timed_send_light_message_queue(mq: *mut (), data: u64, timeout_ns: i64) -> bool {
    light_mq_timed_put(mq, data, false, timeout_ns)
}

pub(crate) unsafe extern "C" fn timed_jam_light_message_queue(mq: *mut (), data: u64, timeout_ns: i64) -> bool {
    light_mq_timed_put(mq, data, true, timeout_ns)
}

pub(crate) unsafe extern "C" fn timed_receive_light_message_queue(out: *mut u64, mq: *mut (), timeout_ns: i64) -> bool {
    let state = light_mq_get(mq);
    let inner = state.inner.lock().unwrap();
    let (mut inner, _) = state
        .not_empty
        .wait_timeout_while(inner, std::time::Duration::from_nanos(timeout_ns.max(0) as u64), |i| i.queue.is_empty())
        .unwrap();
    match inner.queue.pop_front() {
        Some(v) => {
            state.not_full.notify_one();
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn timed_peek_light_message_queue(
    out: *mut u64,
    mq: *const (),
    timeout_ns: i64,
) -> bool {
    let state = light_mq_get(mq as *mut ());
    let mut inner = state.inner.lock().unwrap();
    if inner.queue.is_empty() && timeout_ns > 0 {
        let (guard, _timed_out) = state
            .not_empty
            .wait_timeout(inner, std::time::Duration::from_nanos(timeout_ns as u64))
            .unwrap();
        inner = guard;
    }
    match inner.queue.front() {
        Some(&v) => {
            if !out.is_null() {
                *out = v;
            }
            true
        }
        None => false,
    }
}

pub(crate) unsafe extern "C" fn initialize_light_event(
    ev: *mut (),
    initially_signaled: bool,
    clear_mode: i32,
) {
    // In place when the object already has state: a thread may already be waiting on it (a wait before the
    // initialization creates the state lazily), and a replaced state would never see the signals meant for it.
    let mut table = light_event_table().lock().unwrap();
    match table.get(&(ev as usize)) {
        Some(st) => {
            let st = st.clone();
            drop(table);
            st.auto_clear
                .store(clear_mode == 1, std::sync::atomic::Ordering::Relaxed);
            let mut g = st.signaled.lock().unwrap();
            *g = initially_signaled;
            if initially_signaled {
                st.signals.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                st.cv.notify_all();
            }
        }
        None => {
            table.insert(ev as usize, light_event_new(initially_signaled, clear_mode));
        }
    }
}

pub(crate) unsafe extern "C" fn finalize_light_event(ev: *mut ()) {
    light_event_table().lock().unwrap().remove(&(ev as usize));
}

pub(crate) unsafe extern "C" fn signal_light_event(ev: *mut ()) {
    let state = light_event_get(ev);
    {
        let mut g = state.signaled.lock().unwrap();
        *g = true;
        state.signals.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        state.cv.notify_one();
    } else {
        state.cv.notify_all();
    }
}

pub(crate) unsafe extern "C" fn clear_light_event(ev: *mut ()) {
    let state = light_event_get(ev);
    *state.signaled.lock().unwrap() = false;
}

pub(crate) unsafe extern "C" fn wait_light_event(ev: *mut ()) {
    let fp = crate::guest_reg!(x29);
    let state = light_event_get(ev);
    light_event_spin(&state);
    let mut guard = state.signaled.lock().unwrap();
    let _blocked = if *guard {
        None
    } else {
        Some(BlockGuard::new("LightEvent", ev as usize, fp))
    };
    let started = std::time::Instant::now();
    let mut reported = false;
    // Manual clear: a signal while waiting releases the waiter even if the event was cleared again since.
    let manual = !state.auto_clear.load(std::sync::atomic::Ordering::Relaxed);
    let signals0 = state.signals.load(std::sync::atomic::Ordering::Relaxed);
    while !*guard && !(manual && state.signals.load(std::sync::atomic::Ordering::Relaxed) != signals0) {
        let (g, t) = state
            .cv
            .wait_timeout(guard, std::time::Duration::from_secs(5))
            .unwrap();
        guard = g;
        if t.timed_out() && !*guard && !reported && !(manual && state.signals.load(std::sync::atomic::Ordering::Relaxed) != signals0) {
            reported = true;
            report_long_wait("WaitLightEvent", ev as usize, fp, started.elapsed());
        }
    }
    if state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        *guard = false;
    }
}

pub(crate) unsafe extern "C" fn try_wait_light_event(ev: *mut ()) -> bool {
    let state = light_event_get(ev);
    let mut guard = state.signaled.lock().unwrap();
    let was = *guard;
    if was && state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        *guard = false;
    }
    was
}

pub(crate) unsafe extern "C" fn timed_wait_light_event(ev: *mut (), timeout_ns: i64) -> bool {
    let state = light_event_get(ev);
    let guard = state.signaled.lock().unwrap();
    let manual = !state.auto_clear.load(std::sync::atomic::Ordering::Relaxed);
    let signals0 = state.signals.load(std::sync::atomic::Ordering::Relaxed);
    let (mut guard, timeout_result) = state
        .cv
        .wait_timeout_while(
            guard,
            std::time::Duration::from_nanos(timeout_ns.max(0) as u64),
            |signaled| {
                !*signaled && !(manual && state.signals.load(std::sync::atomic::Ordering::Relaxed) != signals0)
            },
        )
        .unwrap();
    let was_signaled = !timeout_result.timed_out();
    if was_signaled && state.auto_clear.load(std::sync::atomic::Ordering::Relaxed) {
        *guard = false;
    }
    was_signaled
}

pub(crate) unsafe extern "C" fn initialize_timer_event(ev: *mut (), clear_mode: i32) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    libc::memset(p as *mut libc::c_void, 0, 0x40);
    *p.add(0x10) = 1; // _state = Initialized
    *p.add(0x11) = clear_mode as u8; // _clearMode
    *p.add(0x12) = 0; // _signaled = false
    *p.add(0x13) = 0; // _timerMode = 0 (stopped)

    timer_event_table()
        .lock()
        .unwrap()
        .insert(ev as usize, timer_event_new(clear_mode as u8));
}

pub(crate) unsafe extern "C" fn finalize_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x10) = 0; // _state = 0
    timer_event_table().lock().unwrap().remove(&(ev as usize));
}

pub(crate) unsafe extern "C" fn start_one_shot_timer_event(ev: *mut (), first_time: i64) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x13) = 1; // _timerMode = 1
    *(p.add(0x28) as *mut i64) = first_time;
    *(p.add(0x30) as *mut i64) = 0;

    let first_dur = std::time::Duration::from_nanos(first_time.max(0) as u64);
    let target = std::time::Instant::now() + first_dur;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.timer_mode = 1;
        inner.first_time_ns = first_time;
        inner.interval_ns = 0;
        inner.next_wake = Some(target);
        inner.signaled = false;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn start_periodic_timer_event(
    ev: *mut (),
    first_time: i64,
    interval: i64,
) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x13) = 2; // _timerMode = 2
    *(p.add(0x28) as *mut i64) = first_time;
    *(p.add(0x30) as *mut i64) = interval;

    let first_dur = std::time::Duration::from_nanos(first_time.max(0) as u64);
    let target = std::time::Instant::now() + first_dur;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.timer_mode = 2;
        inner.first_time_ns = first_time;
        inner.interval_ns = interval;
        inner.next_wake = Some(target);
        inner.signaled = false;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn stop_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x13) = 0; // _timerMode = 0
    *p.add(0x12) = 0; // _signaled = 0

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.timer_mode = 0;
        inner.next_wake = None;
        inner.signaled = false;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn signal_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x12) = 1;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.signaled = true;
    }
    state.cv.notify_all();
}

pub(crate) unsafe extern "C" fn clear_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let p = ev as *mut u8;
    *p.add(0x12) = 0;

    let state = timer_event_get(ev);
    {
        let mut inner = state.inner.lock().unwrap();
        inner.signaled = false;
    }
}

pub(crate) unsafe extern "C" fn wait_timer_event(ev: *mut ()) {
    if (ev as usize) < 0x10000 {
        return;
    }
    let fp = crate::guest_reg!(x29);
    let _blocked = BlockGuard::new("TimerEvent", ev as usize, fp);
    let p = ev as *mut u8;
    let state = timer_event_get(ev);
    let mut inner = state.inner.lock().unwrap();
    loop {
        if inner.signaled {
            if inner.clear_mode == 1 {
                inner.signaled = false;
                *p.add(0x12) = 0;
            }
            break;
        }
        match inner.timer_mode {
            0 => {
                inner = state.cv.wait(inner).unwrap();
            }
            1 => {
                if let Some(target) = inner.next_wake {
                    let now = std::time::Instant::now();
                    if now >= target {
                        inner.timer_mode = 0;
                        *p.add(0x13) = 0;
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                    let timeout = target - now;
                    let (new_inner, timeout_res) = state.cv.wait_timeout(inner, timeout).unwrap();
                    inner = new_inner;
                    if timeout_res.timed_out() {
                        inner.timer_mode = 0;
                        *p.add(0x13) = 0;
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                } else {
                    inner = state.cv.wait(inner).unwrap();
                }
            }
            2 => {
                if let Some(target) = inner.next_wake {
                    let now = std::time::Instant::now();
                    if now >= target {
                        let interval = std::time::Duration::from_nanos(
                            inner.interval_ns.max(1_000_000) as u64,
                        );
                        let mut next = target + interval;
                        while next <= now {
                            next += interval;
                        }
                        inner.next_wake = Some(next);
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                    let timeout = target - now;
                    let (new_inner, timeout_res) = state.cv.wait_timeout(inner, timeout).unwrap();
                    inner = new_inner;
                    if timeout_res.timed_out() {
                        let interval = std::time::Duration::from_nanos(
                            inner.interval_ns.max(1_000_000) as u64,
                        );
                        let now = std::time::Instant::now();
                        let mut next = target + interval;
                        while next <= now {
                            next += interval;
                        }
                        inner.next_wake = Some(next);
                        if inner.clear_mode == 1 {
                            inner.signaled = false;
                            *p.add(0x12) = 0;
                        }
                        break;
                    }
                } else {
                    inner = state.cv.wait(inner).unwrap();
                }
            }
            _ => break,
        }
    }
}

pub(crate) unsafe extern "C" fn try_wait_timer_event(ev: *mut ()) -> bool {
    if (ev as usize) < 0x10000 {
        return false;
    }
    let p = ev as *mut u8;
    let state = timer_event_get(ev);
    let mut inner = state.inner.lock().unwrap();
    if inner.signaled {
        if inner.clear_mode == 1 {
            inner.signaled = false;
            *p.add(0x12) = 0;
        }
        return true;
    }
    if let Some(target) = inner.next_wake {
        let now = std::time::Instant::now();
        if now >= target {
            if inner.timer_mode == 1 {
                inner.timer_mode = 0;
                *p.add(0x13) = 0;
            } else if inner.timer_mode == 2 {
                let interval =
                    std::time::Duration::from_nanos(inner.interval_ns.max(1_000_000) as u64);
                let mut next = target + interval;
                while next <= now {
                    next += interval;
                }
                inner.next_wake = Some(next);
            }
            if inner.clear_mode == 1 {
                inner.signaled = false;
                *p.add(0x12) = 0;
            }
            return true;
        }
    }
    false
}

pub(crate) unsafe extern "C" fn try_wait_system_event(event: *mut ()) -> u8 {
    record_hle_call("os::try_wait_system_event");
    match hle_system_event_lookup(event as usize) {
        Some(ev) => std::mem::replace(&mut *ev.signaled.lock().unwrap(), false) as u8,
        None => {
            static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
            let mut seen = SEEN.lock().unwrap();
            if !seen.contains(&(event as usize)) {
                seen.push(event as usize);
                eprintln!(
                    "[!] nn::os::TryWaitSystemEvent on unregistered event {:p} -> false",
                    event
                );
            }
            0
        }
    }
}

/// nn::os::TimedWaitSystemEvent(SystemEventType*, TimeSpan): WaitSystemEvent with a timeout (nanoseconds); true when
/// the event was signaled (and, as an auto-clear event, consumed).
pub(crate) unsafe extern "C" fn timed_wait_system_event(event: *mut (), timeout_ns: i64) -> bool {
    record_hle_call("os::timed_wait_system_event");
    if crate::settings::get().log.log_sysevent_waits {
        static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
        let mut seen = SEEN.lock().unwrap();
        if !seen.contains(&(event as usize)) {
            seen.push(event as usize);
            let fp = crate::guest_reg!(x29);
            println!(
                "[sysevent] TimedWait {:p} timeout {} ns registered={} callers: {}",
                event,
                timeout_ns,
                hle_system_event_lookup(event as usize).is_some(),
                guest_chain(fp)
            );
        }
    }
    let timeout = std::time::Duration::from_nanos(timeout_ns.max(0) as u64);
    match hle_system_event_lookup(event as usize) {
        Some(ev) => {
            let g = ev.signaled.lock().unwrap();
            let (mut g, _) = ev.cv.wait_timeout_while(g, timeout, |s| !*s).unwrap();
            std::mem::replace(&mut *g, false)
        }
        None => {
            static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
            let mut seen = SEEN.lock().unwrap();
            if !seen.contains(&(event as usize)) {
                seen.push(event as usize);
                eprintln!(
                    "[!] nn::os::TimedWaitSystemEvent on unregistered event {:p} -> times out",
                    event
                );
            }
            drop(seen);
            hle_sleep_ns(timeout.as_nanos().min(u64::MAX as u128) as u64);
            false
        }
    }
}

pub(crate) unsafe extern "C" fn wait_system_event(event: *mut ()) {
    record_hle_call("os::wait_system_event");
    let fp = crate::guest_reg!(x29);
    match hle_system_event_lookup(event as usize) {
        Some(ev) => {
            let mut g = ev.signaled.lock().unwrap();
            let _blocked = if *g {
                None
            } else {
                Some(BlockGuard::new("SystemEvent", event as usize, fp))
            };
            while !*g {
                g = ev.cv.wait(g).unwrap();
            }
            *g = false;
        }
        None => {
            // An event nothing initialised through HLE. Log once per address and rate-limit instead of
            // spinning: callers loop on this, and returning instantly pegs a core and floods the log.
            static SEEN: StdMutex<Vec<usize>> = StdMutex::new(Vec::new());
            let mut seen = SEEN.lock().unwrap();
            if !seen.contains(&(event as usize)) {
                seen.push(event as usize);
                eprintln!("[!] nn::os::WaitSystemEvent on unregistered event {:p} (returning after a short sleep from now on)", event);
            }
            drop(seen);
            hle_sleep_ns(5_000_000);
        }
    }
}

pub(crate) unsafe extern "C" fn destroy_system_event(event: *mut ()) {
    record_hle_call("os::destroy_system_event");
    HLE_SYSTEM_EVENTS
        .lock()
        .unwrap()
        .retain(|(a, _)| *a != event as usize);
}

// ── nn::os::Event ────────────────────────────────────────────────────────────────────────────
// EventClearMode: 0 = ManualClear, 1 = AutoClear.
pub(crate) unsafe extern "C" fn initialize_event(ev: *mut u8, initially_signaled: bool, clear_mode: i32) {
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

pub(crate) unsafe extern "C" fn finalize_event(ev: *mut u8) {
    lock().as_mut().unwrap().events.remove(&(ev as usize));
}

pub(crate) unsafe extern "C" fn signal_event(ev: *mut u8) {
    let mut g = lock();
    if let Some(e) = g.as_mut().unwrap().events.get_mut(&(ev as usize)) {
        e.signaled = true;
        e.signals += 1;
    }
    sync_wake(g.as_ref().unwrap(), ev as usize);
}

pub(crate) unsafe extern "C" fn clear_event(ev: *mut u8) {
    if let Some(e) = lock().as_mut().unwrap().events.get_mut(&(ev as usize)) {
        e.signaled = false;
    }
}

pub(crate) unsafe extern "C" fn wait_event(ev: *mut u8) {
    event_wait(ev as usize, None);
}

pub(crate) unsafe extern "C" fn try_wait_event(ev: *mut u8) -> bool {
    event_wait(ev as usize, Some(std::time::Instant::now()))
}

pub(crate) unsafe extern "C" fn timed_wait_event(ev: *mut u8, timeout_ns: i64) -> bool {
    event_wait(
        ev as usize,
        Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64)),
    )
}

// ── nn::os::MultiWait ────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn initialize_multiwait(mw: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .multiwaits
        .insert(mw as usize, Vec::new());
}

pub(crate) unsafe extern "C" fn finalize_multiwait(mw: *mut u8) {
    lock().as_mut().unwrap().multiwaits.remove(&(mw as usize));
}

pub(crate) unsafe extern "C" fn initialize_multiwait_holder_event(holder: *mut u8, ev: *mut u8) {
    lock()
        .as_mut()
        .unwrap()
        .holders
        .insert(holder as usize, ev as usize);
}

pub(crate) unsafe extern "C" fn finalize_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    let s = g.as_mut().unwrap();
    s.holders.remove(&(holder as usize));
    for v in s.multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}

pub(crate) unsafe extern "C" fn link_multiwait_holder(mw: *mut u8, holder: *mut u8) {
    let mut g = lock();
    g.as_mut()
        .unwrap()
        .multiwaits
        .entry(mw as usize)
        .or_default()
        .push(holder as usize);
    sync_wake(g.as_ref().unwrap(), mw as usize); // a waiter may be blocked on a list that just grew
}

pub(crate) unsafe extern "C" fn set_multiwait_holder_user_data(holder: *mut u8, data: usize) {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(holder as usize, data);
}

pub(crate) unsafe extern "C" fn get_multiwait_holder_user_data(holder: *const u8) -> usize {
    HOLDER_USER_DATA
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&(holder as usize)).copied())
        .unwrap_or(0)
}

pub(crate) unsafe extern "C" fn unlink_multiwait_holder(holder: *mut u8) {
    let mut g = lock();
    for v in g.as_mut().unwrap().multiwaits.values_mut() {
        v.retain(|h| *h != holder as usize);
    }
}

/// Blocks until any linked holder's event is signaled; returns that holder (auto-clear events are consumed).
pub(crate) unsafe extern "C" fn wait_any(mw: *mut u8) -> *mut u8 {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        let holders: Vec<usize> = s
            .multiwaits
            .get(&(mw as usize))
            .cloned()
            .unwrap_or_default();
        let mut keys = vec![mw as usize];
        for h in holders {
            if let Some(&ev) = s.holders.get(&h) {
                keys.push(ev);
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
        g = sync_wait(g, &keys, None);
    }
}

// ── nn::os::MessageQueue ─────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn initialize_message_queue(mq: *mut u8, _buf: *mut u64, count: usize) {
    lock().as_mut().unwrap().msgqs.insert(
        mq as usize,
        MsgQ {
            cap: count.max(1),
            q: VecDeque::new(),
        },
    );
}

pub(crate) unsafe extern "C" fn finalize_message_queue(mq: *mut u8) {
    lock().as_mut().unwrap().msgqs.remove(&(mq as usize));
}

pub(crate) unsafe extern "C" fn send_message_queue(mq: *mut u8, data: u64) {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        match s.msgqs.get_mut(&(mq as usize)) {
            Some(q) if q.q.len() >= q.cap => {}
            Some(q) => {
                q.q.push_back(data);
                sync_wake(g.as_ref().unwrap(), mq as usize);
                return;
            }
            None => return,
        }
        g = sync_wait(g, &[mq as usize], None);
    }
}

pub(crate) unsafe extern "C" fn receive_message_queue(out: *mut u64, mq: *mut u8) {
    let mut g = lock();
    loop {
        let s = g.as_mut().unwrap();
        match s.msgqs.get_mut(&(mq as usize)) {
            Some(q) => {
                if let Some(v) = q.q.pop_front() {
                    if !out.is_null() {
                        *out = v;
                    }
                    sync_wake(g.as_ref().unwrap(), mq as usize);
                    return;
                }
            }
            None => return,
        }
        g = sync_wait(g, &[mq as usize], None);
    }
}

pub(crate) unsafe extern "C" fn try_send_message_queue(mq: *mut u8, data: u64) -> bool {
    mq_send(mq as usize, data, false, Some(std::time::Instant::now()))
}

pub(crate) unsafe extern "C" fn jam_message_queue(mq: *mut u8, data: u64) {
    mq_send(mq as usize, data, true, None);
}

pub(crate) unsafe extern "C" fn try_receive_message_queue(out: *mut u64, mq: *mut u8) -> bool {
    mq_receive(out, mq as usize, Some(std::time::Instant::now()), false)
}

pub(crate) unsafe extern "C" fn timed_receive_message_queue(
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

pub(crate) unsafe extern "C" fn peek_message_queue(out: *mut u64, mq: *const u8) {
    mq_receive(out, mq as usize, None, true);
}

pub(crate) unsafe extern "C" fn try_peek_message_queue(out: *mut u64, mq: *const u8) -> bool {
    mq_receive(out, mq as usize, Some(std::time::Instant::now()), true)
}

pub(crate) unsafe extern "C" fn timed_peek_message_queue(out: *mut u64, mq: *const u8, timeout_ns: i64) -> bool {
    mq_receive(out, mq as usize, deadline_ns(timeout_ns), true)
}

pub(crate) unsafe extern "C" fn try_jam_message_queue(mq: *mut u8, data: u64) -> bool {
    mq_send(mq as usize, data, true, Some(std::time::Instant::now()))
}

pub(crate) unsafe extern "C" fn timed_send_message_queue(mq: *mut u8, data: u64, timeout_ns: i64) -> bool {
    mq_send(mq as usize, data, false, deadline_ns(timeout_ns))
}

pub(crate) unsafe extern "C" fn timed_jam_message_queue(mq: *mut u8, data: u64, timeout_ns: i64) -> bool {
    mq_send(mq as usize, data, true, deadline_ns(timeout_ns))
}
