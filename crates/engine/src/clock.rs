// The host's monotonic clock (mach absolute time; the performance counter on Windows), which the guest's system tick,
// NVN timestamps and the engine's own timing all use, so their values agree.

use std::sync::OnceLock;

/// Ticks per nanosecond as numer / denom (mach_timebase_info's form: ns = ticks * numer / denom).
#[repr(C)]
#[derive(Clone, Copy)]
struct Timebase {
    numer: u32,
    denom: u32,
}

#[cfg(target_vendor = "apple")]
extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
}

#[cfg(windows)]
extern "system" {
    fn QueryPerformanceCounter(count: *mut i64) -> i32;
    fn QueryPerformanceFrequency(freq: *mut i64) -> i32;
}

fn timebase() -> Timebase {
    static TB: OnceLock<Timebase> = OnceLock::new();
    *TB.get_or_init(|| {
        #[cfg(target_vendor = "apple")]
        let tb = {
            let mut tb = Timebase { numer: 0, denom: 0 };
            unsafe { mach_timebase_info(&mut tb) };
            tb
        };
        // 10 MHz on current Windows: 100 ns per tick.
        #[cfg(windows)]
        let tb = {
            let mut f = 0i64;
            unsafe { QueryPerformanceFrequency(&mut f) };
            let g = gcd(1_000_000_000, f as u64);
            Timebase { numer: (1_000_000_000 / g) as u32, denom: (f as u64 / g) as u32 }
        };
        Timebase {
            numer: tb.numer.max(1),
            denom: tb.denom.max(1),
        }
    })
}

#[cfg(windows)]
fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 {
        a.max(1)
    } else {
        gcd(b, a % b)
    }
}

/// Clock ticks now.
pub fn ticks() -> u64 {
    #[cfg(target_vendor = "apple")]
    return unsafe { mach_absolute_time() };
    #[cfg(windows)]
    {
        let mut t = 0i64;
        unsafe { QueryPerformanceCounter(&mut t) };
        t as u64
    }
}

/// Ticks per second.
pub fn frequency() -> u64 {
    let tb = timebase();
    (1_000_000_000u128 * tb.denom as u128 / tb.numer as u128) as u64
}

pub fn ticks_to_ns(t: u64) -> u64 {
    let tb = timebase();
    (t as u128 * tb.numer as u128 / tb.denom as u128) as u64
}

pub fn ns_to_ticks(ns: u64) -> u64 {
    let tb = timebase();
    (ns as u128 * tb.denom as u128 / tb.numer as u128) as u64
}

/// Sleeps `ns` nanoseconds of this clock, precisely (a frame's pacing depends on it). On Darwin a bare kernel trap,
/// which guest threads need (horizon::hle::hle_sleep_ns); on Windows a high-resolution waitable timer, as Sleep rounds
/// up to the 15.6 ms system tick.
pub fn sleep_ns(ns: u64) {
    #[cfg(target_vendor = "apple")]
    {
        extern "C" {
            fn mach_wait_until(deadline: u64) -> libc::c_int;
        }
        unsafe { mach_wait_until(ticks().saturating_add(ns_to_ticks(ns))) };
    }
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        const CREATE_WAITABLE_TIMER_HIGH_RESOLUTION: u32 = 0x2;
        const TIMER_ALL_ACCESS: u32 = 0x1F0003;
        const INFINITE: u32 = u32::MAX;
        extern "system" {
            fn CreateWaitableTimerExW(attrs: *mut c_void, name: *const u16, flags: u32, access: u32) -> *mut c_void;
            fn SetWaitableTimer(
                timer: *mut c_void,
                due: *const i64,
                period: i32,
                completion: *mut c_void,
                arg: *mut c_void,
                resume: i32,
            ) -> i32;
            fn WaitForSingleObject(handle: *mut c_void, ms: u32) -> u32;
        }
        thread_local! {
            // One timer per thread, for the thread's lifetime.
            static TIMER: *mut c_void = unsafe {
                CreateWaitableTimerExW(
                    std::ptr::null_mut(),
                    std::ptr::null(),
                    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
                    TIMER_ALL_ACCESS,
                )
            };
        }
        let timer = TIMER.with(|t| *t);
        // A negative due time is relative, in 100 ns units.
        let due = -((ns / 100).max(1) as i64);
        if timer.is_null() || unsafe { SetWaitableTimer(timer, &due, 0, std::ptr::null_mut(), std::ptr::null_mut(), 0) } == 0 {
            std::thread::sleep(std::time::Duration::from_nanos(ns));
            return;
        }
        unsafe { WaitForSingleObject(timer, INFINITE) };
    }
}

pub fn now_ns() -> u64 {
    ticks_to_ns(ticks())
}

pub fn now_ms() -> f64 {
    now_ns() as f64 / 1e6
}

pub fn now_secs() -> f64 {
    now_ns() as f64 / 1e9
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_convert_and_sleep_is_precise() {
        assert!(frequency() > 1_000_000);
        assert_eq!(ticks_to_ns(ns_to_ticks(1_000_000_000)), 1_000_000_000);
        // 2 ms, which a sleep rounded to the 15.6 ms Windows tick would overshoot many times over.
        let t0 = now_ns();
        sleep_ns(2_000_000);
        let took = now_ns() - t0;
        assert!((2_000_000..10_000_000).contains(&took), "slept {took} ns");
    }
}
