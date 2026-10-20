// The host's monotonic clock (mach absolute time), which the guest's system tick, NVN timestamps and the engine's own
// timing all use, so their values agree.

use std::sync::OnceLock;

#[repr(C)]
#[derive(Clone, Copy)]
struct Timebase {
    numer: u32,
    denom: u32,
}

extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
}

fn timebase() -> Timebase {
    static TB: OnceLock<Timebase> = OnceLock::new();
    *TB.get_or_init(|| {
        let mut tb = Timebase { numer: 0, denom: 0 };
        unsafe { mach_timebase_info(&mut tb) };
        Timebase {
            numer: tb.numer.max(1),
            denom: tb.denom.max(1),
        }
    })
}

/// Clock ticks now.
pub fn ticks() -> u64 {
    unsafe { mach_absolute_time() }
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

pub fn now_ns() -> u64 {
    ticks_to_ns(ticks())
}

pub fn now_ms() -> f64 {
    now_ns() as f64 / 1e6
}

pub fn now_secs() -> f64 {
    now_ns() as f64 / 1e9
}
