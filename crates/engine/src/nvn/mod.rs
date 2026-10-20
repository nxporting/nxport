// The NVN graphics API as the guest sees it: object state, texture layouts, recorded command buffers and shader
// programs. Execution belongs to the GPU backend (crate::metal).

pub mod api;
pub mod cmd;
pub mod layout;
pub mod state;
