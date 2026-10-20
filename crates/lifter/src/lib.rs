// nxport-lifter: AArch64 -> C static recompilation of guest modules, for hosts that cannot run guest code natively
// (x86-64; docs/windows-port.md). The generated C includes runtime/cpu.h (`CPU_H`) and calls into the runtime through
// rt_dispatch / rt_svc / rt_brk / rt_udf / rt_ticks.

pub mod abi;
pub mod bits;
pub mod ehframe;
pub mod insn;
pub mod jt;
pub mod module;

pub use insn::{lift, Insn, Unsupported};
pub use module::{lift_module, Module, Options, Output};

/// The C header generated code includes (write it as `cpu.h` next to the generated files).
pub const CPU_H: &str = include_str!("../runtime/cpu.h");
