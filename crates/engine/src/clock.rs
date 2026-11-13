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

/// t * a / b rounded down, as a 64-bit multiply and divide: (t / b) * a + (t % b) * a / b, exact while a * b fits in 64
/// bits (the remainder times a stays below b * a). A clock read converts with one: through 128 bits, x86-64 calls a
/// library division (__udivti3) every time, which totk-pc-recomp measured at a tenth of a guest thread's time in a fight.
/// A fraction too large for that keeps the 128-bit form. Results past 64 bits wrap, as the 128-bit form's `as u64` did.
#[derive(Clone, Copy, Debug)]
pub struct Scale {
    a: u64,
    b: u64,
    wide: bool,
}

impl Scale {
    pub fn new(a: u64, b: u64) -> Scale {
        let g = gcd(a, b.max(1));
        let (a, b) = (a / g, b.max(1) / g);
        Scale { a, b, wide: a.checked_mul(b).is_none() }
    }

    #[inline]
    pub fn apply(self, t: u64) -> u64 {
        if self.wide {
            return (t as u128 * self.a as u128 / self.b as u128) as u64;
        }
        (t / self.b).wrapping_mul(self.a).wrapping_add(t % self.b * self.a / self.b)
    }
}

/// Clock ticks to nanoseconds and back, and to `hz` ticks a second.
fn scales() -> &'static (Scale, Scale) {
    static S: OnceLock<(Scale, Scale)> = OnceLock::new();
    S.get_or_init(|| {
        let tb = timebase();
        (Scale::new(tb.numer as u64, tb.denom as u64), Scale::new(tb.denom as u64, tb.numer as u64))
    })
}

/// Clock ticks to a clock of `hz` ticks a second, in one step (rounded once). Lifted code's counter reads use it
/// (guest::regs::lifted_ticks); native guest code reads the hardware counter.
#[cfg_attr(nx_native, allow(dead_code))]
pub fn scale_to(hz: u64) -> Scale {
    let tb = timebase();
    let (a, b) = (hz as u128 * tb.numer as u128, tb.denom as u128 * 1_000_000_000);
    let g = gcd128(a, b);
    let (a, b) = (a / g, b / g);
    match (u64::try_from(a), u64::try_from(b)) {
        (Ok(a), Ok(b)) => Scale::new(a, b),
        // Not reducible to 64 bits (no clock seen does this): through nanoseconds.
        _ => Scale { a: hz, b: 1_000_000_000, wide: true },
    }
}

#[cfg_attr(nx_native, allow(dead_code))]
fn gcd128(a: u128, b: u128) -> u128 {
    if b == 0 {
        a.max(1)
    } else {
        gcd128(b, a % b)
    }
}

pub fn ticks_to_ns(t: u64) -> u64 {
    scales().0.apply(t)
}

pub fn ns_to_ticks(ns: u64) -> u64 {
    scales().1.apply(ns)
}

/// Sleeps `ns` nanoseconds of this clock, precisely (a frame's pacing depends on it). On Darwin a bare kernel trap,
/// which guest threads need (horizon::hle::hle_sleep_ns); on Windows a high-resolution waitable timer, as Sleep rounds
/// up to the 15.6 ms system tick, then a yield loop for its last part.
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
        // The timer wakes up to about half a millisecond late, so it covers all but the last TIMER_SLACK and the thread
        // yields its core until the deadline. Games sleep tens of microseconds while polling for other threads' work
        // (TotK's main thread a few times a frame), which the timer alone turned into half a millisecond each.
        const TIMER_SLACK: u64 = 700_000;
        let deadline = ticks().saturating_add(ns_to_ticks(ns));
        if ns > TIMER_SLACK {
            let timer = TIMER.with(|t| *t);
            // A negative due time is relative, in 100 ns units.
            let due = -(((ns - TIMER_SLACK) / 100).max(1) as i64);
            if timer.is_null() || unsafe { SetWaitableTimer(timer, &due, 0, std::ptr::null_mut(), std::ptr::null_mut(), 0) } == 0 {
                std::thread::sleep(std::time::Duration::from_nanos(ns));
                return;
            }
            unsafe { WaitForSingleObject(timer, INFINITE) };
        }
        // The wait is pause instructions, which leave the core to the thread on its other hardware thread (the game's
        // chain threads; TotK sleeps 20 us thousands of times a second). No yields: with every processor busy (other
        // programs), SwitchToThread hands the rest of the quantum to another process's thread and the sleeper gets its
        // core back tens of milliseconds later; one yield per 20 us sleep took TotK from 110 to ~1 fps that way.
        while ticks() < deadline {
            for _ in 0..16 {
                std::hint::spin_loop();
            }
        }
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
    fn scales_round_down_as_the_128_bit_form() {
        // Windows' 10 MHz, Apple silicon's 125/3, 1:1, a 3 MHz counter, and the lifted 19.2 MHz from each.
        let reference = |t: u64, a: u64, b: u64| (t as u128 * a as u128 / b as u128) as u64;
        let fractions = [(100u64, 1u64), (125, 3), (1, 1), (1000, 3), (48, 25), (8, 125), (192 * 125, 30 * 625)];
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        for (a, b) in fractions {
            let s = Scale::new(a, b);
            for t in [0, 1, b - 1, b, b + 1, u32::MAX as u64, 1 << 40, 1 << 50] {
                assert_eq!(s.apply(t), reference(t, a, b), "{a}/{b} at {t}");
            }
            for _ in 0..10_000 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let t = x >> 8; // centuries of nanoseconds
                assert_eq!(s.apply(t), reference(t, a, b), "{a}/{b} at {t}");
            }
        }
        // One step to another clock: the same as through nanoseconds, give or take the one rounding saved.
        let to = scale_to(19_200_000);
        let t = ticks();
        let via_ns = (ticks_to_ns(t) as u128 * 19_200_000 / 1_000_000_000) as u64;
        assert!(to.apply(t).abs_diff(via_ns) <= 1);
    }

    #[test]
    fn ticks_convert_and_sleep_is_precise() {
        assert!(frequency() > 1_000_000);
        assert_eq!(ticks_to_ns(ns_to_ticks(1_000_000_000)), 1_000_000_000);
        // 2 ms, which a sleep rounded to the 15.6 ms Windows tick would overshoot many times over, every time. A busy
        // machine (other tests in parallel) delays some sleeps too, so the fastest of a few counts. That bound is
        // Windows' concern: on macOS, GitHub's virtual machines coalesce a background process's timers (every one of five
        // 2 ms sleeps took 16-19 ms in a Release run), which a game in the foreground does not see.
        let took: Vec<u64> = (0..5)
            .map(|_| {
                let t0 = now_ns();
                sleep_ns(2_000_000);
                now_ns() - t0
            })
            .collect();
        assert!(took.iter().all(|&t| t >= 2_000_000), "slept {took:?} ns");
        #[cfg(windows)]
        assert!(*took.iter().min().unwrap() < 10_000_000, "slept {took:?} ns");
    }
}
