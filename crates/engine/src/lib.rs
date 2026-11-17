// Until lifted hosts run games (docs/windows-port.md, steps 5 and 6), much of the runtime has no caller there: what
// only the game loop and the GPU backend reach.
#![cfg_attr(all(nx_runtime, not(nx_native)), allow(dead_code))]

// Two layers, see build.rs and docs/windows-port.md: cfg(nx_runtime) hosts load and run games (guest/, horizon/, nvn/,
// gpu/), and of those cfg(nx_native) ones run guest code on the host CPU (the code rewriting in cpu/, host/, the Metal
// backend). Other hosts get the platform-independent parts the builder and the command line use.
#[cfg(nx_runtime)]
pub mod capture;
pub mod config;
pub mod game;
pub mod platform;
#[cfg(nx_runtime)]
mod gpu;
mod guest;
/// The lifted guest library and its calling convention (public as before the move into guest/exec/).
pub use guest::exec::{hle_abi, lifted};
/// NSO parsing, for tools that read a game's modules (the lifter in nxport-build).
pub use guest::nso;
#[cfg(nx_runtime)]
mod horizon;
/// How mods are found, for the tools that package them.
#[cfg(nx_runtime)]
pub mod mods {
    pub use crate::horizon::fs::{discover_mods, has_exefs_nsp, is_mod_dir, mod_subdir, patch_sets};
    pub use crate::guest::mod_patches::for_other_build;
}
#[path = "host/perf_stats.rs"]
pub mod perf_stats;
#[cfg(nx_native)]
mod host;
#[cfg(all(nx_runtime, not(nx_native)))]
#[path = "host/headless.rs"]
mod host;
#[cfg(target_os = "macos")]
pub use host::ui;
#[cfg(nx_native)]
pub use gpu::metal;
/// The Metal backend's shader tooling that does not need Metal itself (the builder specializes MSL on any host).
#[cfg(not(nx_native))]
pub mod metal {
    #[path = "../gpu/metal/msl_specialize.rs"]
    pub mod msl_specialize;
    #[path = "../gpu/metal/source_pack.rs"]
    pub mod source_pack;
}
#[cfg(nx_runtime)]
mod nvn;
#[cfg(nx_runtime)]
pub use gpu::vulkan;
pub mod profile;
pub mod settings;
/// Dear ImGui windows over SDL3 on Windows (the builder's and the game's settings window), and the SDL3.dll the
/// builder carries: host/ui_sdl.rs, host/sdl_library.rs, kept at these paths for the builder.
#[cfg(windows)]
pub use host::{sdl_library, ui_sdl};

mod startup;
pub use startup::*;
