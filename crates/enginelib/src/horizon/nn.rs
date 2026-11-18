// Shared by the nn:: services (one module each in horizon/): the success result and no-op entry points several SDK tables
// bind.

pub(crate) const RESULT_SUCCESS: u32 = 0;

pub(crate) unsafe extern "C" fn noop_void() {}
pub(crate) unsafe extern "C" fn noop_obj(_this: *mut u8) {}

