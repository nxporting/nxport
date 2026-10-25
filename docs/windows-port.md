# Windows (and other non-Apple hosts)

nxport runs guest code natively: the game's arm64 code is patched in place and executed by the host CPU. That
works on Apple Silicon and could work on Windows on arm64, but not on x86-64. This note describes how nxport gets
to Windows: which CPU path each host uses, how the runtime talks to guest code independently of that path, the
lifter that makes the x86-64 path, and the pieces besides the CPU (OS layer, GPU, host I/O, builder).

LAS Recomp (https://github.com/peiikai/LASRecomp, MIT; its author allows referencing it) is a working static
recompilation of a Switch game to Windows, macOS and Linux, and is the reference for much of this.

## Hosts

| Host | CPU path | GPU | Status |
|---|---|---|---|
| macOS, iOS (arm64) | native | Metal | ships |
| Windows arm64 | native (x18 trampolines always on) | Vulkan | planned |
| Windows / Linux x86-64 | lifted | Vulkan | in progress |

Lifted code also runs on macOS (`--cpu=lifted`, planned): there the native path is the reference to compare it
against, with nothing else changed (see Testing).

## Build configuration

`crates/engine/build.rs` sets two cfgs. `nx_runtime` marks hosts that load and run games: the guest image and its
loading (guest/), the OS the guest sees (horizon/), NVN (nvn/) and the GPU interface (gpu/), on Apple + aarch64 and
Windows so far. `nx_native` marks the hosts among them that run guest code natively: the code rewriting and trap
handling in cpu/ (all but cpu/aot.rs's image building, which the lifted target needs too), host/ and the Metal
backend. Lifted hosts get host/headless.rs instead of host/ (no window or input until SDL3; audio is host/audio.rs
with an SDL3 device). Without either the
engine builds only its platform-independent parts: settings, profiles, game identification, NSO parsing
(`nxport::nso`) and the MSL tooling the builder uses. `nxport::run` returns an error on lifted hosts until the lifted
runtime exists. Apple packaging (`ipa`, `pack`, `shader_cache_build`, `pipeline_archive` in nxport-build, the
matching nxport-cli commands, the builder window) is `cfg(target_os = "macos")`. On x86_64-pc-windows-msvc the
workspace builds and the engine's tests pass.

## The guest boundary

Everything the runtime does with guest code goes through four kinds of crossing. Today each one is arm64 machine
code; the plan is to name each as an interface that both CPU paths implement.

1. **Guest calls host (HLE).** `GuestMemorySpace::make_hle_trampoline(rust_fn as *const ())` (~100 call sites:
   sdk_loader, lib.rs, loader, the TotK profile) emits an arm64 stub that switches to a host stack and calls an
   `extern "C"` Rust function with the guest's registers as its arguments (945 such functions). That relies on
   the guest and host sharing AAPCS64. On x86-64 a function pointer alone is not enough: the caller has to know
   the signature to move x0-x7 / v0-v7 into Win64 or SysV argument registers.

   Done (`hle_abi`): a trait `HleFn`, implemented for `unsafe extern "C" fn(A0, .., An) -> R` with n <= 10
   (integers and pointers from x0-x7, floats from v0-v7, the rest from the guest stack; R into x0 / v0; `-> !`
   too), gives each function an adapter that works on a `Cpu`. Code that hands out an HLE function's address takes
   it through `crate::hle!(f(_, _))`, which returns the address as before and records the adapter under it; the
   functions themselves do not change (about 840 bindings, including the NVN lookup tables and the movie vtables).
   The SDK registry's static tables (horizon/sdk) write `func!(f(_, _))`: the entry holds the adapter, built at
   compile time, and a lookup records it. Variadic host functions (`printf`) cannot have one and are bound as
   `Bind::Raw`.
   Trampolines keep their guest addresses in both paths, since the guest stores them in vtables and function
   pointers: the native path emits the arm64 stub as before, and a lifted runtime looks the adapter up by the
   function's address. Native trampolines report a function bound without an adapter, so a native run checks the
   coverage. Guest function pointers passed as arguments (callbacks) are handed over as addresses; a lifted runtime
   calls them through `guest::call` (below).

2. **Host calls guest.** The engine enters guest code in two places: `guest::call(addr, &[u64]) -> u64` (module
   DT_INIT and constructors, mod initialisers, nninitStartup, svc threads; on the calling thread's stack) and
   `horizon::thread_stack::run` (nnMain and nn::os threads, on the stack buffer the game gave the thread). Native:
   a call through a function pointer, and the stack switch thread_stack.rs does now. Lifted: set x0-x7, sp and x30 =
   RETURN_TO_HOST in the thread's `Cpu`, dispatch, return x0. One guest entry does not go through either yet: the
   destructor a mod passes to `__syscall_tls_create` is handed to pthread_key_create, so the host calls it at thread
   exit; a lifted runtime needs host-side destructors that call it through `guest::call`.

   The `asm!` blocks in HLE code (hle.rs, nvn/, sync.rs, svc.rs and others) do not call guest code: they read the
   guest's registers at HLE entry (x29 / x30 for logs and backtraces, x16 / x17 from the trampoline, TPIDR_EL0 /
   TPIDRRO_EL0). A lifted runtime reads those from the thread's `Cpu`, which needs a small accessor in place of each.

3. **Supervisor calls.** module_loader rewrites `svc #n` in mods into stubs that call `horizon::svc` with x0-x7.
   Lifted: `svc #n` becomes `rt_svc(cpu, n)` calling the same handler with the same register array.

4. **Thread state.** Native: TPIDR_EL0 lives in a TSD slot (cpu/tls.rs, tpidr_rewrite.rs) and x18 in another
   (trampoline.rs), because XNU clobbers both. Lifted: they are fields of the thread's `Cpu` (tpidr, tpidrro, x18 is
   just x[18]); none of the rewriting applies. Guest TLS blocks and the fake thread-local region stay as they are.

What does not change: guest addresses are host addresses in both paths (the fixed window at `FIXED_GUEST_BASE`
= 0x70_0010_0000; LAS Recomp reserves the same region on Windows), so HLE code, NVN, the Metal and Vulkan
backends, dirty tracking and page guards all keep using guest pointers directly. On x86 the fault handler sees host
PCs, so reports map them to guest functions through the lifter's function table.

## The lifter

AArch64 to C, compiled by clang (bundled zig cc for players, as LAS Recomp does: no game code ships). It is a Rust
crate (`crates/lifter`) so the builder runs it without Python. Design follows LAS Recomp's `tools/lifter.py` and
`gen.py`:

- Guest state in a `Cpu` struct (x0-x30, sp, pc, nzcv, tpidr, tpidrro, exclusive monitor, fpcr/fpsr, v0-v31).
  `runtime/cpu.h` (C) and `crates/lifter/src/abi.rs` (Rust) share the layout.
- One C function per guest function, `void f_<addr>(Cpu *c)`. Direct branches inside a function are gotos. Direct
  calls are C calls. Indirect branches and returns to unknown targets go through `rt_dispatch`, which tail-calls
  (`musttail`) so jump chains do not grow the host stack.
- Integer, memory and branch instructions become C. FP/SIMD, crypto and CRC32 become portable C over clang vector
  types (`simd.rs`, runtime/simd.h and simd_fp.h from LAS Recomp): ARM NaN propagation, FPCR.DN / FZ / RMode,
  saturating conversions, fused multiply-add, the ARM estimate tables. FPSR cumulative bits, QC and FP16
  arithmetic (ARMv8.2) are not modelled. The generated C must be built with hardware FMA (x86-64-v3): MinGW's
  software `fma` rounds denormal products wrongly, which the differential test caught.
- Exclusives (ldxr/stxr) are emulated with compare-and-swap; acquire/release become atomics.
- Function boundaries come from symbols, `.eh_frame`, call targets and HLE bindings. Jump tables are resolved by
  pattern plus emulation. A branch the lifter could not resolve falls back to an interpreter at runtime, not a fatal
  error. LAS Recomp stops with `no function at ...` there, which is fine for one game but not for a generic tool.

**Input: the patched image, not the raw NSO.** `nxport::guest_image` (the iOS AOT path, cpu/aot.rs) already loads
every module, applies relocations, ExeFS patches, SDK interposition, HLE bindings and the game profile's hooks,
without running anything, and returns the code as it will run. The lifter lifts that. Game hooks
(`VersionProfile::install_hooks`) keep working unchanged: they write arm64 instructions and branches to HLE
trampolines, and the lifter lifts those like any other code. For the lifted target the load skips the native-only
rewrites (tpidr, x18, svc stubs), because lifted code handles those registers itself. This needs the load pipeline
(guest/loader.rs, module_loader.rs, sdk_loader.rs) to run on non-native hosts: it only maps memory and writes bytes,
so it moves onto the OS layer below. It does not execute anything.

**Plugin mods** (exlaunch subsdk modules such as UltraCam) are the hard case. Their code is lifted like the game's,
but their initialisers patch game code while the game runs, and statically lifted code cannot see those writes.
The plan has two parts:
1. A two-pass build, the same idea as the iOS dump run (`--aot-dump`): lift without the mods' runtime patches, run
   the game headless through the mods' initialisers and first frames, and record the code they write (svc.rs
   already funnels mod code writes through `flush_one`). Then re-lift with those writes applied.
2. At runtime, writes to lifted code pages (pages stay read-only and the fault handler sees writes) invalidate the
   affected functions, and those run in the interpreter from then on. The same interpreter covers unresolved
   indirect branches.

**Output and caching:** C sources in chunks (TotK's main module is large: compile time on the player's machine
matters), compiled to `nxport_guest.dll` / `.so` / `.dylib`, cached by module build ids, enabled mods and lifter
version. The library exports only the function tables (`<module>_fn_count`, `_fn_addrs`, `_fn_ptrs`): a game has
more functions than a PE export table can hold (64K).

**Measured on Super Mario Odyssey 1.3.0** (`nxport-cli lift`, the NSO as stored, not yet the patched image):

| | main | sdk | subsdk0 |
|---|---|---|---|
| functions / instructions | 54,215 / 2.67 M | 34,472 / 1.36 M | 13,636 / 0.90 M |
| not lifted | 0 | 0 | 1 (data after a `ret`) |
| jump tables resolved / unresolved | 265 / 51 | 430 / 99 | 320 / 24 |
| C | 171 MB, 68 files | 91 MB, 35 files | 59 MB, 24 files |

All three compile with zig cc -O1 -march=x86-64-v3 in 5 minutes on 16 threads and link into one 102 MB DLL.

## OS layer

The native runtime calls libc/Mach directly (mmap/mprotect/MAP_FIXED in ~17 files, Mach VM in 9, sigaction,
pthreads, dlopen). An `os` module takes over, its interface modelled on LAS Recomp's `runtime/platform.h`:

- memory: reserve the guest window at a fixed address, commit, protect, decommit; write watch (Windows
  `MEM_WRITE_WATCH`) as an alternative to fault-based dirty tracking.
- faults: install a handler (sigaction / `AddVectoredExceptionHandler`) that gets the fault address, access kind
  and a register context it can modify.
- threads: host TLS, names, realtime timing threads; time: monotonic ns, precise sleep.
- dynamic libraries: open / symbol (the lifted guest library, opus, the movie shim).

Pages: the runtime assumes 16 KiB pages (~56 places). LAS Recomp keeps a 16 KiB logical page as a multiple of the
host page (4 KiB on Windows, with 64 KiB allocation granularity), and so can nxport.

## GPU

`crate::vulkan` implements `gpu::Backend` on Vulkan 1.3 (ash, the loader opened at run time); `--gpu=vulkan` selects
it (headless until there is a window for it: `--dump-frame` writes what it presents), and `nxport-cli gpu-replay
--backend vulkan` runs frame captures on it. It is developed against the Metal backend's results with the
conformance captures (Testing), on this Mac through MoltenVK.

- Execution: each submit is one command buffer recorded from the NVN command lists in order (CallCommands inlined) and
  waited for before the submit returns; images stay in the GENERAL layout with full barriers between passes and other
  work. Correct first: no overlap of frames, no batching.
- Memory: guest memory pools are imported as buffers (`VK_EXT_external_memory_host`), so vertex, index and storage data
  are the guest's own pages and GPU writes land in guest memory; uniform buffers are copied when bound (NVN updates
  them inline in the command stream), except ranges GPU work of the same submit writes. Storage buffers a device cannot
  use in place are copies written back after the submit.
- Shaders: sass2spv translates each stage for the pipeline's state (attribute and output types from the bound formats,
  NVN's clip conventions) and the device (`sass2spv::accessor::Host`: capabilities, one descriptor set per stage,
  checked constant buffer reads, discarding stages that write their depth). Bindings are fed as the Metal backend feeds
  them: the support buffer, the driver constant buffer (draw parameters; storage buffer descriptors in hardware stage
  order, fragment at 0x510), the shader's constant bank, bound uniform and storage buffers, textures and images through
  the pool registrations.
- NVN state objects become pipeline state; viewport, scissor, stencil values, blend color and depth bias are dynamic.
  An upper-left window origin flips the viewport; a GL depth mode uses depth clip control (or the vertex stage converts).
- Textures: one image per guest texture description, filled from guest memory, zeros where the guest's bytes cannot be
  uploaded (depth formats); views per binding; a draw sampling an attachment of its own pass reads a copy taken after
  the last barrier that invalidates textures (render-target feedback).
- MoltenVK specifics, each checked against the Metal backend: vertex streams with unaligned strides or attribute
  offsets are repacked (MoltenVK claims it handles them; Apple GPUs fetch wrong data), uniform buffer reads are bounds-
  checked in the shader (MoltenVK does not), discarding fragment stages write their depth.
- Status on MoltenVK: the synthetic scenarios match their Metal references exactly; the TotK Hateno capture matches
  visually, all 252 targets within the capture's `[vulkan]` tolerance (max difference 0.1, one-pixel neighbourhood,
  0.2% of pixels: the two shader compilers round differently along edges and in normals).
- Not yet: occlusion queries (SAMPLES_PASSED reads as every sample passing), buffer textures, quads and line loops,
  aliases of one texture's memory (separate images per description), the render-target write-back to guest memory the
  Metal backend does for CPU readers, presentation to a window, partial texture clears, depth formats in captures.

## Host I/O and the builder

SDL3 for window, input, controllers, rumble and audio (LAS Recomp does the same):
- Audio: **done**. host/audio.rs is shared by all hosts: the ring the audio renderer and AudioOut push into, the trim
  and underrun accounting, `--audio-wav`, and the rule that `--headless` and `--audio=0` stay silent. Only the device
  differs: an AudioQueue on Apple (audio_queue.rs), an SDL3 audio stream elsewhere (audio_sdl.rs: 48 kHz stereo
  float on the default playback device, 10 ms device buffers, its callback filling from the ring). `audio::init`
  runs on the main thread before any guest thread, because the sdl3 crate takes the thread of its first `sdl3::init`
  as SDL's main thread, which the window has to be created on; it initialises only the audio subsystem, and the
  device is resumed at the first pushed frame as the AudioQueue is started. host/headless.rs includes the module as
  it is.
- **Window & Event Loop (`host/display_sdl.rs`):** The process main thread initializes SDL3 and runs the event pump,
  while the guest threads run independently. Handles window close, focus loss, resizing, and fullscreen toggles
  (F11 or Alt+Enter). Title displays the presented FPS counter. `--headless` skips window creation entirely.
- **Vulkan Window Presentation (`vulkan/window.rs`, `device.rs`):** Presents into the window's HWND using
  `VK_KHR_win32_surface`. The presented NVN texture is blitted and centered with black letterboxing to preserve the
  aspect ratio. Swapchain recreates automatically upon window resizing or vsync changes (FIFO vs MAILBOX/IMMEDIATE).
- **Input (`host/input.rs`, `display_sdl.rs`):** SDL keyboard scancodes are mapped to macOS virtual key codes so that
  `%APPDATA%\nxport\controls.cfg` shares the exact same format and key mappings as macOS. The left mouse button acts as
  the single-touch capacitive touch screen. SDL game controllers are polled every 4 ms with radial deadzone handling,
  mapping buttons and sticks to Switch HID Npad state.
- **Rumble (`host/rumble_sdl.rs`):** Translates Switch HD Rumble vibration values (`nn::hid::SendVibrationValue[s]`) to
  SDL dual-motor gamepad rumble (low frequency on left, high frequency on right), with safety decay and periodic renewal.
- Paths under `~/Library/Application Support/nxport` become `%APPDATA%\nxport`.
- The builder's window is AppKit + imgui; on Windows it becomes imgui over SDL3, with a "Windows app" target: a folder
  with nxport.exe, the lifted guest library, the shader cache and the settings.

## Windows on arm64

Windows on arm64 can use the native path, with differences from Darwin: x18 is the TEB pointer there (always use
the x18 trampolines, and reach thread state through the TEB, not tpidrro_el0); whether Windows preserves
TPIDR_EL0 for user code has to be tested; faults come through vectored exception handlers; executable memory needs
`VirtualProtect` + `FlushInstructionCache`.

## Testing

- Lifter: unit tests in the crate; instruction-level differential tests against an arm64 reference (Unicorn, or
  real hardware on an arm64 Mac): random operands, compare registers, flags and memory. The differential harness
  is not part of the repository.
- Boundary traces: a flag that logs each guest thread's registers (x0-x7, sp, lr) at every HLE call and svc. Guest
  addresses are fixed, so a native run and a lifted run of the same game produce comparable traces, on the same Mac
  or across machines; the first difference points to the lifted function that is wrong.
- Smallest game first (Kirby), mods off, `--headless`, then `--audio-wav` and frame dumps for end-to-end comparisons.
- GPU backends: conformance tests with the Metal backend as the reference, so a new backend (Vulkan) is developed
  against recorded frames instead of the running game, and a graphics fix in one backend shows up as a failing test in
  the others (crates/engine/src/capture, crates/cli/tests/gpu_conformance.rs).
  - A frame capture (`nxport --capture-frame=N`, default destination the game's data directory's captures/): the
    frame's decoded NVN command lists, the NVN frontend's state the backend consults (memory pools, virtual mappings,
    texture snapshots and pool registrations, samplers, programs), the guest memory the frame reaches at its addresses,
    and the textures only the GPU held (read back through `gpu::Backend::gpu_only_textures` in the guest's texel
    encoding). It is taken after the frame's last submit with the GPU drained, the game's fence wait held, as
    `--replay` re-runs frames.
  - `nxport-cli gpu-replay DIR` maps the memory back at the same addresses (`os::reserve`, `At::Exactly`), writes the
    GPU-only textures into it in guest layout (where the Switch's GPU would have left them, so a backend uploads them like
    any guest data), installs the frontend state, flattens the frame's command stream and cuts it into passes (runs with
    the same color targets bound). Each pass is submitted and waited for alone and its color targets are read back
    (`gpu::Backend::read_texture`); `--record` keeps them as references, a check compares against them with the
    capture's tolerance.toml, stops at the first differing pass and writes diff images.
  - Two tiers: game captures hold game data and stay outside the repository (the test checks the ones in the games'
    data directories and skips without any); synthetic scenarios (`nxport-cli gpu-scenario`: short NVN sequences
    recorded through the frontend, with hand-encoded Maxwell shaders that go through sass2spv) are committed with their
    references (crates/cli/tests/gpu).
  - A fix found in one backend: re-record the affected references (the diff images show what changed), then make
    the other backends match.
  - Measured on TotK (Hateno): 1.2 GB of memory (578 MB compressed) and 3,196 GPU-only texture subresources per capture,
    138 passes and 252 targets; a replay takes ~10 s. Two Metal replays differ in a few dozen pixels (GPU particles),
    which the capture's tolerance allows.
  - Not covered yet: depth formats Metal widens (D24, D24S8, D32FS8) are neither compared nor restored, multisample
    targets are not read back, and memory is the state after the frame (an inline uniform update over a range the frame
    read before it shows the newer data in a replay).

## Order of work

1. Workspace compiles on non-Apple hosts. **Done.**
2. Lifter crate: integer, memory, branch and system instructions, function discovery, jump tables, C emission,
   `nxport-cli lift`. **Done**, checked against Unicorn (18,000 random instruction cases, no differences) and on
   SMO. FP / AdvSIMD / crypto / CRC32: **done**, checked against Unicorn (32,000 FP/SIMD cases over 406
   mnemonics, random FPCR modes and NaN / denormal / infinity operands, no differences). Left: the remaining
   jump-table shapes.
3. HLE adapters (`HleFn`) and `guest::call`, on the native path first (no behaviour change, testable on the Mac).
   Adapters: **done** (`hle_abi`; every binding takes its address through `crate::hle!(f(_, _))`, and native
   trampolines warn about a function without one). `guest::call`: **done** (the TLS destructors and the register
   reads in HLE code are left for the lifted runtime, above).
4. OS layer; the load pipeline runs on any host and produces the patched image for the lifted target.
   Memory: protection, icache flushes, plain allocations and address-space reservations (`os::reserve` / `commit` /
   `release`, with `At::Exactly` for the fixed guest base) are **done**; the load pipeline (loader, module_loader,
   sdk_loader, the rtld placeholder, dirty tracking's arrays) maps through them, and so do the runtime's own mappings
   (memory blocks, the address region arena, svc.rs's alias regions; `decommit` and `discard` for freed pages).
   Also in `os`: thread-local keys with destructors (fiber-local storage on Windows), dynamic libraries, local and UTC
   time, the current thread's stack and id, the library holding an address; crate::clock reads the performance
   counter and sleeps on a high-resolution timer there.
   guest/, horizon/ and nvn/ build without nx_native (**done**, cfg(nx_runtime)), and `nxport::guest_image` makes the
   patched image on Windows: the loads skip the native-only rewrites (x18, svc / system-register stubs, tpidr,
   load-exclusives), whose instructions lifted code handles itself. HLE code reads guest registers through
   guest::regs (`guest_reg!(x29)` and the like: the registers in place natively, the calling thread's Cpu lifted,
   which lifted::host_call records). Guest TLS blocks are guest::tls on both paths. Copy-mode code views stand in for
   svc.rs's mach_vm_remap aliases where there is none. Checked on Windows with TotK 1.0.0 (no mods): guest_image
   loads and patches main and the SDK and returns the image, and every HLE function the game binds has an adapter
   except printf (`Bind::Raw`).
   Left: svcExitThread on lifted threads (it stops with a message), and host destructors for mods' TLS keys.
5. Lifted runtime on macOS (`--cpu=lifted`), checked against native with boundary traces.
   Core: **done**. The lifter's runtime/rt.c, compiled into the guest library, has the dispatcher (a per-thread cache
   in front of the module tables) and forwards svc / brk / udf / counters and branches to non-lifted addresses to
   the host, whose callbacks it gets at load (a DLL cannot import from the program loading it). `lifted.rs` loads
   the library, registers the module tables and runs guest code; a branch to an HLE trampoline (or an HLE function's
   own address) runs the function's adapter. Checked end to end on Windows with a small program: integer and float
   HLE calls through trampoline addresses and a supervisor call. Each host thread has its own `Cpu`
   (lifted::thread_cpu), which guest::call and guest threads (horizon::thread_stack::run) run on. Left: the
   `--cpu=lifted` switch on macOS, and the boundary-trace comparison with native.
6. Windows x86-64: lifted runtime headless (a null GPU backend that signals fences, for booting), then Vulkan,
   then SDL3 host I/O, then the builder's Windows target.
   Headless: **done**. `nxport-cli lift-game <dump> -o <dir>` lifts the game as the runtime loads and patches it
   (nxport::guest_modules: main, the SDK, the code islands) and writes `modules.txt`; `nxport-cli compile-guest
   <dir> -o guest.dll --cc "zig cc"` compiles the C files in parallel (objects are reused while their C file is
   unchanged, and lift-game leaves unchanged files alone) and links the library; `nxport <dump>
   --guest-library=guest.dll` runs it with `gpu::null`. On TotK 1.0.0 the lift takes 20 s (12.7 M instructions,
   about 630,000 functions, many of them pieces of functions split at computed branches) and the compile about 5
   minutes on 16 threads with zig cc 0.13; the game boots, loads its resources, and plays its opening (event flows
   Dm_OP_0017 to 0034, voice and music streaming) for 10 minutes without a fault, presenting about 7 frames a second
   to the null backend.
   Vulkan: **done**, running lifted games with page guards via vectored exception handling.
   SDL3 host layer (window, Vulkan swapchain, input, rumble): **done**. SDL3 statically linked from source provides
   the window and event pump on the process main thread (`host/display_sdl.rs`), presenting via `VK_KHR_win32_surface`
   into the HWND (`vulkan/window.rs`, `device.rs`) with letterboxing, vsync, dynamic resizing, and fullscreen toggles.
   Keyboard and gamepads map to Switch HID (`host/input.rs`) and HD rumble drives dual-motor rumble (`host/rumble_sdl.rs`).
   Audio: **done**. Shared ring in crates/engine/src/host/audio.rs refactored with target/max latency trim and underrun
   accounting; Apple uses audio_queue.rs (AudioToolbox) and lifted/Windows hosts use audio_sdl.rs (SDL3 playback
   stream at 48 kHz stereo float on default device with 10 ms buffers). Tested on TotK, streaming smoothly with zero
   underruns post-startup, while --headless and --audio=0 remain completely silent.
7. Plugin mods: two-pass lifting, then the interpreter fallback.
8. Windows arm64 on the native path.
