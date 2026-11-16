// The guest operating system: kernel calls, the nn:: libraries that reach it, and services.

pub mod audio;
#[cfg(nx_native)]
pub mod cores;
pub mod fs;
pub mod heap_layout;
pub mod hle;
pub mod mii;
pub mod sdk;
pub mod movie;
pub mod nn;
pub mod os;
pub mod hid;
pub mod ldn;
pub mod time;
pub mod bcat;
pub mod nfp;
pub mod friends;
pub mod ec;
pub mod pl;
pub mod swkbd;
pub mod socket;
pub mod prepo;
pub mod nifm;
pub mod codec;
pub mod oe;
pub mod account;
pub mod vi;
pub mod sm;
pub mod diag;
mod packed_mods;
pub mod svc;
pub mod sync;
pub mod thread_stack;
pub mod uemalloc;
