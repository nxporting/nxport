// nn::ldn as the game calls it: local wireless play, reported as unavailable.

use super::nn::*;

pub(crate) fn set_state(new_state: u32) {
    LDN_STATE.store(new_state, std::sync::atomic::Ordering::SeqCst);
    let ev = LDN_STATE_EVENT.load(std::sync::atomic::Ordering::SeqCst);
    if ev != 0 {
        crate::horizon::os::objects::hle_system_event_signal_addr(ev);
    }
}

pub(crate) unsafe extern "C" fn initialize() -> u32 {
    crate::horizon::ldn::set_state(1); // State::Initialized
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn finalize() -> u32 {
    crate::horizon::ldn::set_state(0); // State::None
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_state() -> u32 {
    LDN_STATE.load(std::sync::atomic::Ordering::SeqCst)
}

pub(crate) unsafe extern "C" fn attach_state_change_event(event: *mut u8) {
    if !event.is_null() {
        LDN_STATE_EVENT.store(event as usize, std::sync::atomic::Ordering::SeqCst);
        crate::horizon::os::objects::hle_system_event_register_signaled(event as usize);
    }
}

pub(crate) unsafe extern "C" fn open_station() -> u32 {
    crate::horizon::ldn::set_state(4); // State::StationOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn close_station() -> u32 {
    crate::horizon::ldn::set_state(1); // State::Initialized
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn open_access_point() -> u32 {
    crate::horizon::ldn::set_state(2); // State::AccessPointOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn close_access_point() -> u32 {
    crate::horizon::ldn::set_state(1); // State::Initialized
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn create_network() -> u32 {
    crate::horizon::ldn::set_state(3); // State::AccessPointCreated
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn destroy_network() -> u32 {
    crate::horizon::ldn::set_state(2); // State::AccessPointOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn scan(
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

pub(crate) unsafe extern "C" fn connect() -> u32 {
    crate::horizon::ldn::set_state(5); // State::StationConnected
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn disconnect() -> u32 {
    crate::horizon::ldn::set_state(4); // State::StationOpened
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn get_ipv4_address(out_ip: *mut u32, out_mask: *mut u32) -> u32 {
    if !out_ip.is_null() {
        *out_ip = 0x0A000002; // 10.0.0.2
    }
    if !out_mask.is_null() {
        *out_mask = 0xFFFFFF00; // 255.255.255.0
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn zero_buffer_0x480(buf: *mut u8) -> u32 {
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 0x480);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn zero_buffer_0x48(buf: *mut u8) -> u32 {
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 0x48);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn zero_buffer_0x40(buf: *mut u8) -> u32 {
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 0x40);
    }
    RESULT_SUCCESS
}

pub(crate) unsafe extern "C" fn make_ipv4(a: u8, b: u8, c: u8, d: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((c as u32) << 8) | (d as u32)
}

pub(crate) unsafe extern "C" fn make_broadcast(ip: u32, mask: u32) -> u32 {
    ip | !mask
}

pub(crate) unsafe extern "C" fn make_network(ip: u32, mask: u32) -> u32 {
    ip & mask
}
