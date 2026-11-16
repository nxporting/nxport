// NVN: the bootstrap loader the game links against, and the functions nvnDeviceGetProcAddress hands out by name (one
// table per object family; the implementations are in crate::nvn).

mod buffer;
mod command_buffer;
mod device;
mod memory_pool;
mod program;
mod queue;
mod sampler;
mod state;
mod sync;
mod texture;
mod window;

use super::super::{Bind, Entry};

pub(super) static TABLE: &[Entry] = &[
    Entry("nvnBootstrapLoader", func!(crate::nvn::api::hle_nvnBootstrapLoader(_))),
    Entry("nvnPrivate0", Bind::Data(|| std::ptr::addr_of_mut!(crate::horizon::hle::NVN_PRIVATE0) as usize)),
];

pub(super) static PROCS: &[&[Entry]] = &[
    buffer::PROCS,
    command_buffer::PROCS,
    device::PROCS,
    memory_pool::PROCS,
    program::PROCS,
    queue::PROCS,
    sampler::PROCS,
    state::PROCS,
    sync::PROCS,
    texture::PROCS,
    window::PROCS,
];
