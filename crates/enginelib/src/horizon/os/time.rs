// nn::os clocks: the system tick (the host clock, see crate::platform::clock) and conversions.


pub(crate) extern "C" fn get_system_tick() -> u64 {
    crate::platform::clock::ticks()
}

pub(crate) unsafe extern "C" fn convert_to_time_span(tick: i64) -> i64 {
    crate::platform::clock::ticks_to_ns(tick.max(0) as u64) as i64
}

pub(crate) unsafe extern "C" fn get_system_tick_frequency() -> u64 {
    crate::platform::clock::frequency()
}

/// nn::os::ConvertToTick(TimeSpan) -> Tick, consistent with GetSystemTick (host mach ticks).
pub(crate) unsafe extern "C" fn convert_to_tick(ns: i64) -> i64 {
    crate::platform::clock::ns_to_ticks(ns.max(0) as u64) as i64
}

pub(crate) fn deadline_ns(timeout_ns: i64) -> Option<std::time::Instant> {
    Some(std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns.max(0) as u64))
}
