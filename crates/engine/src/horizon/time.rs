// nn::time as the game calls it: clocks and time spans.

use super::nn::*;

pub(crate) unsafe extern "C" fn initialize() -> u32 {
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn finalize() -> u32 {
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn is_initialized() -> bool {
    true
}

pub(crate) unsafe extern "C" fn clock_get_current_time(out: *mut i64) -> u32 {
    if !out.is_null() {
        *out = unix_now();
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn steady_clock_now() -> i64 {
    crate::platform::clock::now_ns() as i64
}

// ClockSnapshot is 208 bytes. Our layout: +0 user posix secs, +8 steady ns.
pub(crate) unsafe extern "C" fn clock_snapshot_ctor(snap: *mut u8) {
    if !snap.is_null() {
        std::ptr::write_bytes(snap, 0, 208);
    }
}

pub(crate) unsafe extern "C" fn clock_snapshot_create(snap: *mut u8) {
    if !snap.is_null() {
        std::ptr::write_bytes(snap, 0, 208);
        *(snap as *mut i64) = unix_now();
        *(snap.add(8) as *mut i64) = crate::horizon::time::steady_clock_now();
    }
}

pub(crate) unsafe extern "C" fn calculate_span_between(
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

pub(crate) unsafe extern "C" fn user_clock_difference(from: *const u8, to: *const u8) -> i64 {
    if from.is_null() || to.is_null() {
        return 0;
    }
    (*(to as *const i64) - *(from as *const i64)) * 1_000_000_000
}

pub(crate) unsafe extern "C" fn to_calendar_time(cal: *mut u8, info: *mut u8, posix: *const i64) -> u32 {
    let t = if posix.is_null() { 0 } else { *posix };
    let (tm, name, dst, off) = crate::platform::os::local_time(t);
    fill_calendar(&tm, cal, info, name.as_bytes(), dst, off);
    RESULT_SUCCESS
}

/// nn::time::ToCalendarTimeInUtc(const PosixTime&) -> CalendarTime (8-byte struct, returned in x0).
pub(crate) unsafe extern "C" fn to_calendar_time_in_utc(posix: *const i64) -> u64 {
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
pub(crate) unsafe extern "C" fn to_posix_time_from_utc(cal: *const u8) -> i64 {
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

/// nn::time::GetDeviceLocationName(LocationName*): 36-byte time zone name (the SDK's own asks the time service).
pub(crate) unsafe extern "C" fn get_device_location_name(out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 36);
        std::ptr::copy_nonoverlapping(b"UTC".as_ptr(), out, 3);
    }
}
