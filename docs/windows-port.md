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

`crates/engine/build.rs` sets `cfg(nx_native)` on hosts that run guest code natively (Apple + aarch64 so far).
Without it the engine builds only its platform-independent parts: settings, profiles, game identification, NSO
parsing (`nxport::nso`) and the MSL tooling the builder uses. `nxport::run` returns an error there until the lifted
runtime exists. Apple packaging (`ipa`, `pack`, `shader_cache_build` in nxport-build, the matching nxport-cli
commands, the builder window) is `cfg(target_os = "macos")`. `cargo check --workspace --all-targets` passes on
x86_64-pc-windows-msvc.

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

2. **Host calls guest.** Thread entries (`svc.rs:735`, `hle.rs:2078/2370`), callbacks and mod initialisers are
   called through transmuted function pointers and inline asm (22 `asm!` blocks in hle.rs, more in nvn/, sync.rs,
   thread_stack.rs). Plan: one primitive, `guest::call(addr, &[u64]) -> u64` (plus a float-returning variant). Native:
   what the asm does now. Lifted: set x0-x7 and lr in the thread's `Cpu`, dispatch, return x0.

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

A Vulkan backend behind `gpu::Backend` (13 methods; the NVN frontend only mentions Metal in comments). sass2spv
already produces SPIR-V, so Vulkan takes it directly; the MSL step (spirv-cross, msl_specialize) is Metal's only.
LAS Recomp's `runtime/nvn/vulkan/` (device.c, exec.c, textures.c) maps NVN onto Vulkan and is the reference.
Zero-copy buffers over guest memory use `VK_EXT_external_memory_host`, or copies with write-watch-driven
invalidation where that is missing.

## Host I/O and the builder

SDL3 for window, input, controllers, rumble and audio (LAS Recomp does the same). Paths under
`~/Library/Application Support/nxport` become `%APPDATA%\nxport`. The builder's window is AppKit + imgui; on
Windows it becomes imgui over SDL3, with a "Windows app" target: a folder with nxport.exe, the lifted guest
library, the shader cache and the settings.

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

## Order of work

1. Workspace compiles on non-Apple hosts. **Done.**
2. Lifter crate: integer, memory, branch and system instructions, function discovery, jump tables, C emission,
   `nxport-cli lift`. **Done**, checked against Unicorn (18,000 random instruction cases, no differences) and on
   SMO. FP / AdvSIMD / crypto / CRC32: **done**, checked against Unicorn (32,000 FP/SIMD cases over 406
   mnemonics, random FPCR modes and NaN / denormal / infinity operands, no differences). Left: the remaining
   jump-table shapes.
3. HLE adapters (`HleFn`) and `guest::call`, on the native path first (no behaviour change, testable on the Mac).
   Adapters: **done** (`hle_abi`; every binding takes its address through `crate::hle!(f(_, _))`, and native
   trampolines warn about a function without one). `guest::call`: next.
4. OS layer; the load pipeline runs on any host and produces the patched image for the lifted target.
5. Lifted runtime on macOS (`--cpu=lifted`), checked against native with boundary traces.
6. Windows x86-64: lifted runtime headless (a null GPU backend that signals fences, for booting), then Vulkan,
   then SDL3 host I/O, then the builder's Windows target.
7. Plugin mods: two-pass lifting, then the interpreter fallback.
8. Windows arm64 on the native path.
