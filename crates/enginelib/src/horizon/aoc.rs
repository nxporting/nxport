// nn::aoc as the game calls it: add-on content, listed from the game's own data.

pub(crate) unsafe extern "C" fn aoc_list(_out_indices: *mut i32, _offset: i32, _count: i32) -> i32 {
    0
}

/// nn::os::SystemEvent holds its SystemEventType first; the list never changes, so it is never signaled.
pub(crate) unsafe extern "C" fn aoc_list_changed_event(event: *mut u8) {
    crate::horizon::os::objects::hle_system_event_register_addr(event as usize);
}
