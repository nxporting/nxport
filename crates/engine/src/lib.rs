// Until lifted hosts run games (docs/windows-port.md, steps 5 and 6), much of the runtime has no caller there: what
// only the game loop and the GPU backend reach.
#![cfg_attr(all(nx_runtime, not(nx_native)), allow(dead_code))]

// Two layers, see build.rs and docs/windows-port.md: cfg(nx_runtime) hosts load and run games (guest/, horizon/, nvn/,
// gpu/), and of those cfg(nx_native) ones run guest code on the host CPU (the code rewriting in cpu/, host/, the Metal
// backend). Other hosts get the platform-independent parts the builder and the command line use.
#[cfg(nx_runtime)]
pub mod capture;
#[cfg(nx_runtime)]
mod clock;
pub mod config;
#[cfg(nx_runtime)]
mod cpu;
pub mod game;
pub mod hle_abi;
pub mod lifted;
pub mod os;
#[cfg(nx_runtime)]
mod gpu;
#[cfg(nx_runtime)]
mod guest;
#[cfg(not(nx_runtime))]
mod guest {
    pub mod nso;
}
/// NSO parsing, for tools that read a game's modules (the lifter in nxport-build).
pub use guest::nso;
#[cfg(nx_runtime)]
mod horizon;
#[cfg(nx_native)]
mod host;
#[cfg(all(nx_runtime, not(nx_native)))]
#[path = "host/headless.rs"]
mod host;
#[cfg(target_os = "macos")]
pub use host::ui;
#[cfg(nx_native)]
pub mod metal;
/// The Metal backend's shader tooling that does not need Metal itself (the builder specializes MSL on any host).
#[cfg(not(nx_native))]
pub mod metal {
    pub mod msl_specialize;
}
#[cfg(nx_runtime)]
mod nvn;
#[cfg(nx_runtime)]
pub mod vulkan;
pub mod profile;
pub mod settings;

#[cfg(nx_native)]
use std::thread;

#[cfg(nx_runtime)]
use guest::loader::GuestMemorySpace;

#[cfg(nx_runtime)]
/// Builds the guest code image (cpu/aot.rs): the game's code loaded and patched with `settings`, as the app will patch
/// it at startup, without running any of it. iOS apps get it signed; where guest code is lifted it is what the lifter
/// lifts. Plugin mods (exefs/subsdkN modules) patch code from their initialisers, so an image with them needs those to
/// run: not done here. Once per process.
pub fn guest_image(s: settings::Settings) -> Result<Vec<u8>, String> {
    cpu::aot::capture_image()?;
    settings::try_install(s)?;
    let l = load().map_err(|e| e.to_string())?;
    if let Some((label, _)) = l.mod_inits.first() {
        return Err(format!("{label} is a plugin mod: its initialiser patches the game's code, so the image needs it run"));
    }
    unsafe { cpu::aot::finalize() }?;
    cpu::aot::take_image().ok_or_else(|| "no image was built".to_string())
}

/// Guest code as the lifter takes it (nxport-build's lift): a module's memory from `base`, with its executable range.
#[cfg(nx_runtime)]
pub struct GuestModule {
    /// A C identifier: the lifter prefixes the module's tables with it.
    pub name: String,
    pub base: u64,
    /// The whole image for main and the sdk module (their rodata holds jump tables), the code alone for other regions.
    pub image: Vec<u8>,
    pub text: std::ops::Range<u64>,
    /// Whether `image` holds a module's dynamic tables (symbols, relocations, .eh_frame_hdr), which name functions.
    pub dynamic: bool,
}

/// The game's code loaded and patched with `settings` (as guest_image), as modules to lift: main, the sdk module, then
/// every other code region the load made (islands, stubs), except the HLE trampolines, which lifted code reaches
/// through the host (lifted::host_call). The guest sits at its fixed base, so the addresses hold in the run that uses
/// the lifted code. Once per process.
#[cfg(nx_runtime)]
pub fn guest_modules(s: settings::Settings) -> Result<Vec<GuestModule>, String> {
    settings::try_install(s)?;
    let l = load().map_err(|e| e.to_string())?;
    if let Some((label, _)) = l.mod_inits.first() {
        return Err(format!("{label} is a plugin mod: its initialiser patches the game's code, so lifting needs it run"));
    }
    let copy = |start: usize, len: usize| unsafe { std::slice::from_raw_parts(start as *const u8, len) }.to_vec();
    let main = l.mem_space.base_ptr as usize;
    let mut modules = vec![GuestModule {
        name: "main".into(),
        base: main as u64,
        image: copy(main, l.mem_space.total_size),
        text: main as u64..(main + l.mem_space.text_size) as u64,
        dynamic: true,
    }];
    if let Some(sdk) = &l.sdk {
        modules.push(GuestModule {
            name: "sdk".into(),
            base: sdk.base as u64,
            image: copy(sdk.base, sdk.total_size),
            text: sdk.text_start as u64..(sdk.text_start + sdk.text_size) as u64,
            dynamic: true,
        });
    }
    let covered = |a: usize| modules.iter().any(|m| m.text.contains(&(a as u64)));
    let extra: Vec<GuestModule> = cpu::aot::regions()
        .into_iter()
        .filter(|(name, start, _)| name != "hle.trampolines" && !covered(*start))
        .map(|(name, start, len)| GuestModule {
            name: name.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '_' }).collect(),
            base: start as u64,
            image: copy(start, len),
            text: start as u64..(start + len) as u64,
            dynamic: false,
        })
        .collect();
    modules.extend(extra);
    Ok(modules)
}

#[cfg(nx_native)]
/// Runs a game: resolves the settings from `layers` (above the platform's, the installation's and the game's profiles),
/// loads and patches its code, then runs it. Must be called on the main thread; returns only on failure.
pub fn run(layers: Vec<settings::SettingsPatch>) -> Result<(), Box<dyn std::error::Error>> {
    let mut s = resolve_settings(layers);
    let vulkan = s.graphics.gpu == settings::GpuBackend::Vulkan;
    if vulkan {
        gpu::install(&vulkan::VULKAN);
    } else {
        gpu::install(&metal::backend::METAL);
    }
    let s = settings::install(s);
    // Write-tracked pages fault on writes (guest/dirty_track.rs), ours included.
    unsafe { cpu::crash_handler::install_signal_handlers() };
    if vulkan {
        vulkan::init(s.debug.validate)?;
    }
    #[cfg(target_os = "macos")]
    config::app_log();
    if s.debug.validate {
        // Metal's own switches, read when the device is created.
        std::env::set_var("MTL_SHADER_VALIDATION", "1");
        std::env::set_var("MTL_SHADER_VALIDATION_REPORT_TO_STDERR", "1");
    }
    // Open-file limit up to the hard limit: every loaded shader library keeps its file open (336 for Kirby), and the
    // soft limit is 256 on iOS ("Too many open files" for the last libraries).
    unsafe {
        let mut r = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if libc::getrlimit(libc::RLIMIT_NOFILE, &mut r) == 0 {
            r.rlim_cur = r.rlim_max.min(10240);
            libc::setrlimit(libc::RLIMIT_NOFILE, &r);
        }
    }
    if let Some(out) = &s.graphics.build_pipeline_archive {
        return metal::pipeline_archive::build(out, s.graphics.pipeline_archive_script).map_err(Into::into);
    }
    host::audio::init();
    let Loaded {
        mem_space,
        sdk: _sdk,
        mod_inits,
        game_subsdk_inits,
    } = load()?;

    // 5'. AOT images (cpu/aot.rs): write the patched code out (--aot-dump), or swap in a signed copy (--aot-image).
    //     With mods a dump run writes the image after their initialisers patched the code (step 7b).
    let dump_after_mods =
        matches!(cpu::aot::mode(), cpu::aot::Mode::Dump(_)) && !mod_inits.is_empty();
    if dump_after_mods {
        cpu::aot::defer_dump();
    }
    if let Err(e) = unsafe { cpu::aot::finalize() } {
        eprintln!("[!!] AOT: {}", e);
        std::process::exit(1);
    }
    if let cpu::aot::Mode::Dump(_) = cpu::aot::mode() {
        if !dump_after_mods {
            println!("[+] AOT: image written; exiting (--aot-dump)");
            std::process::exit(0);
        }
    }

    // ── Guest entry chain ─────────────────────────────────────────────────────────────────────────
    // TLS install, DT_INIT, static constructors, nninitStartup and nnMain must all run on ONE thread
    // (they share its guest TLS block): a dedicated "guest-main" thread, so the process main thread can own the window
    // and the Cocoa event loop.
    // The engine has no main-thread special cases: stack bounds come from the current pthread,
    // guest TLS is per-thread, and the nn::os thread object is thread-local.
    // iOS: the executor runs from the guest's first submit; the window's layer is attached once UIKit has created it
    // (host/display_ios.rs). macOS creates its window before the guest submits anything.
    #[cfg(target_os = "ios")]
    if !host::display::headless() {
        crate::metal::init(
            objc2_metal::MTLCreateSystemDefaultDevice().expect("no Metal device"),
            None,
        );
    }
    let guest_chain =
        AssertSend(move || guest_chain(mem_space, mod_inits, game_subsdk_inits, dump_after_mods));

    let handle = thread::Builder::new()
        .name("guest-main".into())
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            cpu::tls::mark_guest_thread();
            guest_chain.run().map_err(|e| e.to_string())
        })
        .expect("spawn guest-main");

    if host::display::headless() {
        if !vulkan {
            let device = objc2_metal::MTLCreateSystemDefaultDevice().expect("no Metal device");
            crate::metal::init(device, None);
        }
        return match handle.join() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Err("guest-main thread panicked".into()),
        };
    }
    // When the guest finishes (nnMain returned or errored) take the process down too.
    thread::Builder::new()
        .name("guest-watch".into())
        .spawn(move || {
            let code = match handle.join() {
                Ok(Ok(())) => 0,
                Ok(Err(e)) => {
                    eprintln!("[-] guest exited with error: {e}");
                    1
                }
                Err(_) => {
                    eprintln!("[-] guest-main thread panicked");
                    101
                }
            };
            std::process::exit(code);
        })
        .expect("spawn guest-watch");
    // The main thread owns the window and the Cocoa run loop; never returns.
    host::display::run_main_thread()
}

#[cfg(nx_runtime)]
/// Runs the frame capture in `dir` (capture::replay) on the backend `opts` names (Metal or Vulkan), headless, with the
/// settings from `layers`. Once per process: it maps the capture's memory at its addresses and installs its frontend
/// state.
pub fn replay_capture(
    dir: &std::path::Path,
    opts: &capture::replay::Options,
    layers: Vec<settings::SettingsPatch>,
) -> Result<capture::replay::Report, String> {
    let vulkan = match opts.backend.as_str() {
        #[cfg(nx_native)]
        "" | "metal" => false,
        #[cfg(not(nx_native))]
        "" => true,
        "vulkan" => true,
        b => return Err(format!("no backend {b} here (vulkan{})", if cfg!(nx_native) { ", metal" } else { "" })),
    };
    capture::replay::map_early(dir)?;
    if vulkan {
        gpu::install(&vulkan::VULKAN);
    } else {
        #[cfg(nx_native)]
        gpu::install(&metal::backend::METAL);
    }
    let mut s = resolve_settings(layers);
    s.display.headless = true;
    s.graphics.early_fence = false;
    s.graphics.report_guard = settings::ReportGuard::Off;
    s.graphics.async_pipelines = false;
    s.graphics.precompile_pipelines = false;
    s.graphics.record_pipelines = false;
    s.files.no_miss_dump = true;
    let s = settings::install(s);
    if vulkan {
        vulkan::init(s.debug.validate)?;
        let r = capture::replay::run(dir, opts);
        let (record, wait, gpu, flushes) = vulkan::timing();
        println!("[gpu-replay] vulkan: recording {record:.1} ms, GPU wait {wait:.1} ms, GPU {gpu:.1} ms (all passes), {flushes} overlap flushes");
        return r;
    }
    #[cfg(nx_native)]
    {
        if s.debug.validate {
            std::env::set_var("MTL_SHADER_VALIDATION", "1");
            std::env::set_var("MTL_SHADER_VALIDATION_REPORT_TO_STDERR", "1");
        }
        let device = objc2_metal::MTLCreateSystemDefaultDevice().ok_or("no Metal device")?;
        metal::init(device, None);
    }
    capture::replay::run(dir, opts)
}

#[cfg(nx_native)]
/// Records synthetic capture scenario `name` (capture::scenarios) and writes it to `dir`. Once per process.
pub fn write_scenario(name: &str, dir: &std::path::Path) -> Result<(), String> {
    gpu::install(&gpu::null::NULL);
    let mut s = settings::Settings::default();
    s.display.headless = true;
    settings::install(s);
    capture::scenarios::write(name, dir)
}

/// Moves the loaded guest (raw pointers into its memory) to the thread that runs it.
#[cfg(nx_runtime)]
struct AssertSend<F>(F);
#[cfg(nx_runtime)]
unsafe impl<F> Send for AssertSend<F> {}
#[cfg(nx_runtime)]
impl<F: FnOnce() -> Result<(), Box<dyn std::error::Error>>> AssertSend<F> {
    fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        (self.0)()
    }
}

/// The guest entry chain on the calling thread (guest-main): its TLS, DT_INIT, the mods' initialisers, nninitStartup,
/// the static constructors, then nnMain on its own stack. Returns when nnMain does.
#[cfg(nx_runtime)]
fn guest_chain(
    mem_space: GuestMemorySpace,
    mod_inits: Vec<(String, usize)>,
    game_subsdk_inits: Vec<(String, Option<usize>, (usize, usize))>,
    dump_after_mods: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    // 5a. Install TPIDR_EL0 (thread-local storage pointer) for this thread (lifted: its Cpu has it, lifted::thread_cpu).
    #[cfg(nx_native)]
    unsafe {
        cpu::tls::install_guest_tls();
        cpu::trampoline::init_worker_thread_x18();
        horizon::hle::register_main_sample_target();
    }
    unsafe { horizon::hle::hle_nn_os_GetCurrentThread() };

    // 5. Execute DT_INIT (main's first initialization function)
    if mem_space.dynamic.init != 0 {
        println!(
            "[+] Invoking native DT_INIT entrypoint at main+{:#x}...",
            mem_space.dynamic.init
        );
        unsafe { guest::call(mem_space.base_ptr as usize + mem_space.dynamic.init, &[]) };
        println!("[+] SUCCESS: Native DT_INIT executed and returned cleanly!");
    }

    // Block all maskable async signals before entering guest code.
    // macOS zeroes X18 (the platform register) on every signal delivery, which
    // corrupts the guest's use of X18 as a scratch register between adjacent
    // instructions.  The synchronous fault signals (SIGSEGV, SIGBUS, SIGILL,
    // SIGTRAP) cannot be masked, so our crash handler still fires on actual faults.
    #[cfg(nx_native)]
    {
        let mut guest_sigmask: libc::sigset_t = unsafe { std::mem::zeroed() };
        unsafe {
            libc::sigfillset(&mut guest_sigmask);
            // Keep synchronous CPU-fault signals unmasked so the crash handler fires.
            libc::sigdelset(&mut guest_sigmask, libc::SIGSEGV);
            libc::sigdelset(&mut guest_sigmask, libc::SIGBUS);
            libc::sigdelset(&mut guest_sigmask, libc::SIGILL);
            libc::sigdelset(&mut guest_sigmask, libc::SIGTRAP);
            // Keep SIGINT/SIGTERM so the user can still kill the process.
            libc::sigdelset(&mut guest_sigmask, libc::SIGINT);
            libc::sigdelset(&mut guest_sigmask, libc::SIGTERM);
            let mut old_mask: libc::sigset_t = std::mem::zeroed();
            libc::pthread_sigmask(libc::SIG_BLOCK, &guest_sigmask, &mut old_mask);
            println!(
                "[+] Masked async signals before guest entry (protects X18 from macOS zeroing)"
            );
        }
    }

    // Mod modules' initialisers. rtld initialises the modules main depends on before main's own initialisers, and
    // exlaunch installs its hooks here: with an AOT image (cpu/aot.rs) the hooks are in the code from the start, so the
    // mods must have set up the state their hooks use before any game code can reach them (main's static
    // constructors ran into an UltraCam trampoline whose target was still null). A dump run snapshots the code
    // first and writes the image once they patched it.
    macro_rules! run_mod_inits {
        () => {
    if dump_after_mods {
        cpu::aot::snapshot_premod();
    }
    for (label, init) in &mod_inits {
        println!("[+] Mods: running {} init at {:#x}", label, init);
        unsafe { guest::call(*init, &[0, 0]) };
        let pages = unsafe { horizon::svc::flush_code_views() };
        println!("[+] Mods: {} init returned ({} code pages written through its views)", label, pages);
    }
    if dump_after_mods {
        println!("[+] AOT: mods initialised; the image is written at frame {} (gfx present)", cpu::aot::deferred_dump_frame().unwrap_or(0));
    }
    if cpu::aot::loading() && !mod_inits.is_empty() {
        // Expected: the bytes of patches mods remove again (the dump run's "kept" count).
        let n = horizon::svc::IMAGE_MISMATCH.load(std::sync::atomic::Ordering::Relaxed);
        println!("[+] Mods: initialised; {} bytes of their code writes differ from the AOT image so far (dropped)", n);
    }

        };
    }
    run_mod_inits!();

    // 6. Execute nninitStartup (initializes heap & runtime allocators; exported by main). It runs before the
    // static constructors, which may allocate.
    let init_startup_addr = mem_space
        .export("nninitStartup")
        .expect("main exports no nninitStartup");
    let nn_main_addr = mem_space.export("nnMain").expect("main exports no nnMain");
    println!(
        "[+] Invoking native nninitStartup entrypoint at {:#x}...",
        init_startup_addr
    );
    unsafe { guest::call(init_startup_addr, &[]) };
    println!("[+] SUCCESS: Native nninitStartup executed and returned cleanly!");

    // Diagnostic memory state for known game versions after nninitStartup
    if let Some(v) = profile::active_version() {
        if let (Some(heap_off), Some(pool_off)) =
            (v.heap_instance_offset, v.allocator_pool_offset)
        {
            unsafe {
                let p_heap = *(mem_space.base_ptr.add(heap_off) as *const usize);
                let p_pool = *(mem_space.base_ptr.add(pool_off) as *const usize);
                let p_pool_sz = *(mem_space.base_ptr.add(pool_off + 8) as *const usize);
                println!("[+] Diagnostic memory state after nninitStartup:");
                println!(
                    "    [base + 0x{:08x}] = 0x{:016x} (HeapMgr::sInstance)",
                    heap_off, p_heap
                );
                println!(
                    "    [base + 0x{:08x}] = 0x{:016x} (StandardAllocator pool)",
                    pool_off, p_pool
                );
                println!(
                    "    [base + 0x{:08x}] = 0x{:016x} (StandardAllocator size)",
                    pool_off + 8,
                    p_pool_sz
                );
                if p_heap != 0 {
                    println!("    HeapMgr sInstance @ 0x{:016x} dump (32 words):", p_heap);
                    let mgr = p_heap as *const usize;
                    for i in 0..32 {
                        println!("      +0x{:03x}: 0x{:016x}", i * 8, *mgr.add(i));
                    }
                }
            }
        }
    }

    // 7. Execute DT_INIT_ARRAY (Static C++ Constructors)
    println!("[+] Running static C++ constructors from DT_INIT_ARRAY...");
    let init_array_ptr =
        unsafe { mem_space.base_ptr.add(mem_space.dynamic.init_array) as *const usize };
    let ctor_count = mem_space.dynamic.init_arraysz / 8;
    println!(
        "[+] DT_INIT_ARRAY at main+{:#x}: {} entries",
        mem_space.dynamic.init_array, ctor_count
    );
    let mut executed_ctors = 0;

    for i in 0..ctor_count {
        if i % 500 == 0 {
            println!(
                "  [+] Progress: executed {} / {} constructors...",
                i, ctor_count
            );
        }
        let ctor_addr = unsafe { *init_array_ptr.add(i) };
        if ctor_addr != 0 {
            unsafe { guest::call(ctor_addr, &[]) };
            executed_ctors += 1;
        }
    }
    println!(
        "[+] SUCCESS: Executed {} native static C++ constructors!",
        executed_ctors
    );

    // 7a. The game's own subsdk libraries (other games; TotK's are not run): DT_INIT, then the static constructors.
    for (label, init, (arr, n)) in &game_subsdk_inits {
        if let Some(init) = init {
            println!("[+] {}: running DT_INIT at {:#x}", label, init);
            unsafe { guest::call(*init, &[]) };
        }
        for i in 0..*n {
            let ctor = unsafe { *((arr + i * 8) as *const usize) };
            if ctor != 0 {
                unsafe { guest::call(ctor, &[]) };
            }
        }
        println!("[+] {}: {} static constructors run", label, n);
    }

    // 8. Execute nnMain (the game's entry point, exported by main)
    println!(
        "[+] Invoking native nnMain entrypoint at {:#x}...",
        nn_main_addr
    );
    // On its own stack, like the game's other threads (horizon/thread_stack.rs).
    let (stack_lo, stack_hi) = horizon::thread_stack::allocate(8 << 20);
    unsafe {
        let t = horizon::hle::hle_nn_os_GetCurrentThread();
        (*t).stack = stack_lo as *mut ();
        (*t).original_stack = (*t).stack;
        (*t).stack_size = stack_hi - stack_lo;
        horizon::thread_stack::run(stack_lo, stack_hi, nn_main_addr, 0);
    }
    println!("[+] SUCCESS: Native nnMain returned cleanly!");

    // nnMain returning means the game exited; the caller shuts the process down.
    Ok(())
}

#[cfg(nx_runtime)]
/// The settings a run with `layers` uses: the platform's profile, where the program is installed, the game's profile
/// (found from the dump the other layers name), then above it the settings the app was built with or the iOS Settings
/// app's choices, then `layers`.
fn resolve_settings(layers: Vec<settings::SettingsPatch>) -> settings::Settings {
    use settings::profiles::{self, PLATFORM};
    #[cfg(target_os = "ios")]
    let (platform_env, user) = {
        let (env, user) = host::ios_env::setup();
        (Some(env), user)
    };
    #[cfg(not(target_os = "ios"))]
    let (platform_env, user): (
        Option<settings::SettingsPatch>,
        Vec<settings::SettingsPatch>,
    ) = (None, config::app_bundle_settings().into_iter().collect());
    let base: Vec<_> = [profiles::platform(PLATFORM), config::app_bundle_layer()]
        .into_iter()
        .chain(platform_env)
        .collect();
    let provisional = settings::resolve(
        base.iter()
            .cloned()
            .chain(user.iter().cloned())
            .chain(layers.iter().cloned()),
    );
    let game_layers = provisional
        .files
        .game_dir
        .as_deref()
        .and_then(game::title_id_of)
        .and_then(profile::for_title)
        .map(|p| profile::layers(p, PLATFORM))
        .unwrap_or_default();
    settings::resolve(
        base.into_iter()
            .chain(game_layers)
            .chain(user)
            .chain(layers),
    )
}

#[cfg(nx_runtime)]
struct Loaded {
    mem_space: GuestMemorySpace,
    /// The sdk module: it must outlive the guest (dropping it unmaps the module's memory).
    sdk: Option<guest::sdk_loader::SdkModule>,
    /// The mods' module initialisers (label, address), run before main's.
    mod_inits: Vec<(String, usize)>,
    /// The game's own subsdk modules' initialisers (--run-game-subsdk): label, DT_INIT, init array.
    game_subsdk_inits: Vec<(String, Option<usize>, (usize, usize))>,
}

#[cfg(nx_runtime)]
/// Loads the game's modules and the mods' into guest memory and patches them for this host: relocations, HLE bindings,
/// SDK interposition, x18 / tpidr rewrites, game hooks, then makes the code executable. Nothing of the guest has run yet.
fn load() -> Result<Loaded, Box<dyn std::error::Error>> {
    // The TSD keys whose offsets patched code embeds (fixed numbers, cpu/tls.rs), before anything else creates keys.
    #[cfg(nx_native)]
    {
        let _ = (
            cpu::tls::guest_tls_offset(),
            cpu::tls::errno_save_offset(),
            cpu::tls::x18_slot_offset(),
            cpu::tls::hle_stack_offset(),
        );
        unsafe {
            horizon::hle::capture_darwin_tsd_shadow();
        }
    }

    // 1. Allocate & populate guest memory space
    let mut mem_space = GuestMemorySpace::allocate()?;

    // 2. Install native Darwin crash & trap handler
    #[cfg(nx_native)]
    unsafe { cpu::crash_handler::install_signal_handlers() };

    mem_space.load_sections()?;
    #[cfg(nx_native)]
    cpu::crash_handler::set_main_size(mem_space.bss_off + mem_space.bss_size);

    // 2a. Guest x18 starts at 0: guest code uses it as an ordinary register (cpu::trampoline), macOS preserves
    // it, and the HLE trampolines reload this value after each call. (It used to point at a 32 KiB block whose every
    // slot pointed into it, from an x9 <-> x18 register swap removed on 2026-10-08, so stray loads through x18 could not
    // fault; booting to gameplay without it changed nothing, 2026-10-15.)
    let x18_ctx_ptr: usize = 0;

    // 2b. The trampoline pool for all HLE calls (make_hle_trampoline): restores the guest's TLS register and x18 on return.
    mem_space
        .init_trampoline_pool(x18_ctx_ptr)
        .map_err(|e| format!("trampoline pool: {e}"))?;

    // Helper macro: bind a Rust HLE function through the trampoline pool.
    // Binds an HLE function to every GOT slot of main's relocations of that kind naming `name`.

    // 2c. Initialize Global Offset Table (GOT) PLT entries with x9-restoring default HLE stub before relocations
    println!("[+] Initializing Global Offset Table (GOT) shims...");
    let default_stub = mem_space.make_hle_trampoline(crate::hle!(horizon::hle::hle_default_stub()));
    mem_space.init_plt_got(default_stub);

    // 2d. Load SDK — must happen before apply_relocations so that undefined
    //     symbols in the main binary that are defined in the SDK (sead, nn::gfx,
    //     libc++ internals, etc.) resolve to live SDK addresses rather than 0.
    //     Also binds SDK's external relocations (malloc, free, etc.) via mem_space trampolines.
    let sdk_path_buf = game::exefs_file("sdk");
    let sdk_path = sdk_path_buf.to_str().unwrap();
    let sdk = match guest::sdk_loader::load_sdk(sdk_path, &mut mem_space) {
        Ok(m) => {
            println!("[+] SDK loaded successfully ({} symbols)", m.symbols.len());
            Some(m)
        }
        Err(e) => {
            eprintln!(
                "[!] SDK load failed ({}); proceeding without SDK (expect null vtable crashes)",
                e
            );
            None
        }
    };

    // 2e. Apply dynamic relocations (now with SDK symbols available and .rela.plt included)
    let rel_count = mem_space.apply_relocations(sdk.as_ref());
    println!(
        "[+] Dynamic Linker: Applied {} in-memory AArch64 relocations.",
        rel_count
    );

    // Program size for nn::os::QueryMemoryInfo (the loaded images; games budget memory with it).
    horizon::hle::set_program_size(
        mem_space.bss_off + mem_space.bss_size + sdk.as_ref().map_or(0, |m| m.total_size),
    );
    let (stubs, stubs_len) = mem_space.trampoline_range();
    metal::set_hle_stub_range(stubs, stubs + stubs_len);
    // Guest code: main, the mod modules mapped after it, the SDK (--report-guard waits only for faults from there).
    gpu::report_guard::set_guest_code_range(
        mem_space.base_ptr as usize,
        sdk.as_ref()
            .map_or(mem_space.base_ptr as usize + mem_space.total_size, |m| {
                m.base + m.total_size
            }),
    );

    // 2f. ExeFS code mods (IPS / IPS32 / pchtxt) for main and the SDK, before the engine's own code rewriting.
    unsafe {
        let exefs_buf = game::exefs();
        let exefs = exefs_buf.to_str().unwrap();
        if let Some(id) = guest::mod_patches::nso_build_id(&format!("{}/main", exefs)) {
            profile::set_main_build_id(&id);
            profile::log_version(&id);
            let base = mem_space.base_ptr as usize;
            let image = mem_space.bss_off + mem_space.bss_size;
            guest::mod_patches::apply(
                "main",
                &id,
                base,
                image,
                (base, mem_space.text_size),
                os::Prot::ReadWrite,
            );
            horizon::uemalloc::install(base, &id);
        }
        if let (Some(id), Some(sdk_mod)) =
            (guest::mod_patches::nso_build_id(sdk_path), sdk.as_ref())
        {
            guest::mod_patches::apply(
                "sdk",
                &id,
                sdk_mod.base,
                sdk_mod.total_size,
                (sdk_mod.text_start, sdk_mod.text_size),
                os::Prot::ReadExec,
            );
        }
    }

    // 3. Bind specific GOT entries to our native HLE shims via x9-restoring trampolines.

    // Bind specific routines

    // Bind C++ ABI guards
    // nn::util::BitArray::reset is pure computation in the sdk module
    // (confirmed by disassembly: it only calls the SDK's own internal
    // memset, no svc, no external dependency) -- let it run for real instead
    // of the Rust stand-in, same reasoning as StandardAllocator below.
    // bind_import!("_ZN2nn4util8BitArray5resetEv", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_util_BitArray_reset);

    // Bind Horizon Heap & Allocator routines

    // Bind nn::oe and nn::account

    // Bind Threading, TLS, and System GOT symbols
    // GetSystemTick / GetSystemTickFrequency / ConvertToTimeSpan need no kernel, but the sdk module's frequency is the
    // Switch's fixed 19.2 MHz while its tick would read the host counter: all three are HLE'd so they agree
    // (crate::clock).
    // The SDK-internal variant (Super Mario Bros. Wonder's static constructors): the SDK's own walks its thread list,
    // which is empty under the HLE threads.

    // C ABI bindings for Horizon OS (used by SMO and middleware)

    // Bind ConvertToTimeSpan & Human Interface Device (nn::hid)

    // Bind nn::os::TimerEvent

    // Bind Stack, Mutex & RWLock

    // Bind File System (nn::fs)

    // Bind Graphics (NV, VI, NVN)

    // Bind Libc string & memory routines

    // Bind libc output (pure libc string/math/formatting functions resolve to the SDK's own definitions)


    // GLOB_DAT system globals (nn::util math constants resolve to the SDK's own definitions)
    println!(
        "[+] Successfully bound core libc, nn::os, math constants, and NVN entrypoints into GOT."
    );

    // 4. x18 (cpu/trampoline.rs): macOS keeps x18 for this process (linked against a macOS SDK < 13, .cargo/config.toml);
    //    where the kernel does not (iOS, the simulator, a macOS build without that link flag) every x18 use of main
    //    and the SDK is rewritten into an island of stubs that keep the value in a TSD slot. Before the SDK
    //    interposition and the tpidr rewrite, so `mrs x18, tpidr_el0` sites are handled by the x18 stubs.
    #[cfg(nx_native)]
    if cpu::trampoline::x18_trampolines() {
        let island = cpu::trampoline::TrampolineIsland::new(
            guest::loader::x18_island_area(),
            guest::loader::X18_ISLAND_SIZE,
        )
        .map_err(|e| format!("x18 island: {e}"))?;
        island.set_current_thread_x18(x18_ctx_ptr);
        *cpu::trampoline::ISLAND.lock().unwrap() = Some(island);
        unsafe {
            cpu::trampoline::rewrite_module(
                "main",
                mem_space.base_ptr as usize,
                mem_space.text_size,
            )
        }
        .map_err(|e| format!("x18: {e}"))?;
        if let Some(sdk_mod) = sdk.as_ref() {
            // The SDK's text is already read+execute (sdk_loader): writable while it is rewritten, then registered again.
            let pages = (sdk_mod.text_size + 0x3fff) & !0x3fff;
            let _ = unsafe { os::protect(sdk_mod.text_start, pages, os::Prot::ReadWrite) };
            unsafe {
                cpu::trampoline::rewrite_module("sdk", sdk_mod.text_start, sdk_mod.text_size)
            }
            .map_err(|e| format!("x18: {e}"))?;
            unsafe { cpu::aot::make_exec("sdk.text", sdk_mod.text_start, pages) };
        }
    } else {
        println!("[+] x18: preserved by the kernel for this process; guest x18 uses run unchanged");
    }

    // 4a. SDK interposition: calls inside the sdk module to its own functions bypass every GOT, so its definitions of
    //     the functions horizon::sdk binds are patched to jump to the engine's (through trampolines, which restore the
    //     guest's x18 / tpidr_el0 and switch stacks). Otherwise sdk code would open services over IPC, or run cache
    //     instructions Darwin traps (FlushDataCache).
    if let Some(sdk_mod) = sdk.as_ref() {
        let default_stub = mem_space.default_stub_ptr;
        // --list-hle-imports: report every game import not bound to the SDK's own definition.
        if crate::settings::get().log.list_hle_imports {
            for (off, name) in mem_space.imports() {
                let target = unsafe { *(mem_space.base_ptr.add(off) as *const usize) };
                if !sdk_mod.contains(target) {
                    let how = if target == 0 {
                        "null"
                    } else if target == default_stub {
                        "default stub"
                    } else {
                        "HLE"
                    };
                    println!("[import] {} {}", how, name);
                }
            }
        }
        let mut names: Vec<&String> = sdk_mod.symbols.keys().collect();
        names.sort();
        let mut interposed = 0usize;
        for name in names {
            if let Some(horizon::sdk::Target::Func(f)) = horizon::sdk::lookup(name) {
                let tramp = mem_space.shared_hle_trampoline(f as *const ()) as usize;
                if sdk_mod.interpose(name, tramp) {
                    interposed += 1;
                }
            }
        }
        println!("[+] SDK interposition: {} SDK functions now route to the engine's", interposed);
        // The audio renderer service object's vtable is called by native SDK code; route it through
        // trampolines too (the pool is sealed before the game opens the renderer).
        horizon::audio::renderer::set_trampolines(
            mem_space.make_hle_trampoline(horizon::audio::renderer::request_update_fn()) as usize,
            mem_space.make_hle_trampoline(horizon::audio::renderer::unexpected_call_fn()) as usize,
        );
        horizon::movie::set_trampolines(&mut |f| mem_space.make_hle_trampoline(f) as usize);
        let images = [
            (mem_space.base_ptr as usize, mem_space.bss_off),
            (sdk_mod.base, sdk_mod.data_off),
        ];
        nvn::api::set_guest_entries(&images, &mut |f| mem_space.make_hle_trampoline(f) as usize);
    }

    // 4a''. Mod modules (exefs/subsdk0..9 of the enabled mods, e.g. exlaunch plugins like UltraCam). Their imports
    //       resolve to whatever the game's own imports resolved to (so they reach the same HLE), then SDK exports and
    //       what horizon::sdk binds. QueryMemory (horizon_svc) presents the guest
    //       modules in rtld/main/mods/sdk order, as exlaunch expects when it locates the game's code.
    let mut mod_inits: Vec<(String, usize)> = Vec::new();
    // --run-game-subsdk (experiment): bind main's imports to the game's own subsdk libraries and run their
    // initialisers, like rtld (Kirby: subsdk0 = the movie library). Off by default: the real movie library calls into
    // the SDK's multimedia stack, which needs the SDK's static constructors (--sdk-ctors) and, past those, the
    // hardware video decoder services.
    let run_game_subsdk = crate::settings::get().guest.run_game_subsdk;
    let mut game_subsdk_inits: Vec<(String, Option<usize>, (usize, usize))> = Vec::new();
    if run_game_subsdk
        || crate::horizon::fs::mods()
            .iter()
            .any(|(_, d)| (0..10).any(|i| d.join("exefs").join(format!("subsdk{}", i)).is_file()))
    {
        use horizon::svc::{
            add_region, Region, MEM_CODE_MUTABLE, MEM_CODE_STATIC, PERM_R, PERM_RW, PERM_RX,
        };
        let main_base = mem_space.base_ptr as usize;
        unsafe {
            // A placeholder rtld module below main (exlaunch counts modules from rtld = 0, main = 1).
            // Its slot was reserved directly below main in GuestMemorySpace::allocate.
            let p = guest::loader::rtld_slot();
            if p != 0 && crate::os::commit(p, 0x4000, crate::os::Prot::ReadWrite).is_ok() {
                *(p as *mut u32) = 0xD65F03C0; // ret
                cpu::aot::make_exec("rtld.placeholder", p, 0x4000);
                // text, rodata and data like a real module (exlaunch reads the two regions after each text).
                add_region(Region {
                    start: p,
                    size: 0x1000,
                    mem_type: MEM_CODE_STATIC,
                    perm: PERM_RX,
                });
                add_region(Region {
                    start: p + 0x1000,
                    size: 0x1000,
                    mem_type: MEM_CODE_STATIC,
                    perm: PERM_R,
                });
                add_region(Region {
                    start: p + 0x2000,
                    size: 0x2000,
                    mem_type: MEM_CODE_MUTABLE,
                    perm: PERM_RW,
                });
            } else {
                println!("[!] Mods: no room for the rtld placeholder below main; exlaunch may misidentify modules");
            }
        }
        let (ro, data, end) = (mem_space.ro_off, mem_space.data_off, mem_space.total_size);
        add_region(Region {
            start: main_base,
            size: ro,
            mem_type: MEM_CODE_STATIC,
            perm: PERM_RX,
        });
        add_region(Region {
            start: main_base + ro,
            size: data - ro,
            mem_type: MEM_CODE_STATIC,
            perm: PERM_R,
        });
        add_region(Region {
            start: main_base + data,
            size: end - data,
            mem_type: MEM_CODE_MUTABLE,
            perm: PERM_RW,
        });
        if let Some(m) = sdk.as_ref() {
            // text, rodata, data+bss exactly as the NSO lays them out (exlaunch counts a module only with all three).
            add_region(Region {
                start: m.text_start,
                size: m.ro_off - (m.text_start - m.base),
                mem_type: MEM_CODE_STATIC,
                perm: PERM_RX,
            });
            add_region(Region {
                start: m.base + m.ro_off,
                size: m.data_off - m.ro_off,
                mem_type: MEM_CODE_STATIC,
                perm: PERM_R,
            });
            add_region(Region {
                start: m.base + m.data_off,
                size: ((m.image_size + 0xfff) & !0xfff) - m.data_off,
                mem_type: MEM_CODE_MUTABLE,
                perm: PERM_RW,
            });
        }
        let (pool, pool_len) = mem_space.trampoline_range();
        horizon::svc::add_hookable(pool, pool_len);
        let mut syms = mem_space.symbol_targets();
        // Only targets mods may patch: guest code and the HLE trampoline pool. Some game imports are bound straight to
        // engine (Rust) functions; those names fall through to the SDK's own export below.
        {
            let sdk_range = sdk
                .as_ref()
                .map(|m| (m.base, m.base + m.total_size))
                .unwrap_or((0, 0));
            let ok = |a: usize| {
                (a >= main_base && a < main_base + mem_space.total_size)
                    || (a >= sdk_range.0 && a < sdk_range.1)
                    || (a >= pool && a < pool + pool_len)
            };
            syms.retain(|_, a| ok(*a));
        }
        if let Some(sdk_mod) = sdk.as_ref() {
            for (k, v) in &sdk_mod.symbols {
                syms.entry(k.clone()).or_insert(*v);
            }
        }
        // Everything horizon::sdk binds, as the game's imports get it.
        for name in horizon::sdk::names() {
            match horizon::sdk::lookup(name) {
                Some(horizon::sdk::Target::Func(f)) => {
                    syms.insert(name.to_string(), mem_space.shared_hle_trampoline(f as *const ()) as usize);
                }
                Some(horizon::sdk::Target::Data(a)) => {
                    syms.insert(name.to_string(), a);
                }
                None => {}
            }
        }
        for name in [
            "_ZN2nn2ro6detail15g_pAutoLoadListE",
            "_ZN2nn2ro6detail35g_LookupGlobalManualFunctionPointerE",
        ] {
            syms.entry(name.to_string())
                .or_insert(guest::module_loader::RO_DATA_ZERO.as_ptr() as usize);
        }
        syms.insert(
            "_ZN4sead4QuatIfE4unitE".to_string(),
            guest::module_loader::SEAD_QUAT_UNIT.as_ptr() as usize,
        );
        syms.insert(
            "_ZN4sead7Vector3IfE4zeroE".to_string(),
            guest::module_loader::SEAD_VEC3_ZERO.as_ptr() as usize,
        );
        let svc_entry =
            mem_space.make_hle_trampoline(crate::hle!(horizon::svc::svc_dispatch(_, _))) as usize;
        let unresolved = mem_space
            .make_hle_trampoline(crate::hle!(guest::module_loader::unresolved_import()))
            as usize;
        guest::module_loader::set_placeholders(
            (0..16)
                .map(|_| {
                    mem_space
                        .make_hle_trampoline(crate::hle!(guest::module_loader::unresolved_import()))
                        as usize
                })
                .collect(),
        );
        // The reserved gap after main and its island slot (guest/loader.rs): modules are mapped there with MAP_FIXED.
        let mut hint = guest::loader::mod_gap().0;
        // The game's own subsdk modules (TotK: subsdk0 = the movie library) come before the mods' in rtld's order,
        // and mods address modules by index. Load them into the map (relocated, runnable), but leave the game's imports
        // and initialisers as they were: the engine does not run these libraries.
        for i in 0..10 {
            let path = game::exefs_file(&format!("subsdk{}", i));
            if !path.is_file() {
                continue;
            }
            let label = format!("game/subsdk{}", i);
            match unsafe {
                guest::module_loader::load(
                    &path,
                    &label,
                    hint,
                    &|n| syms.get(n).copied(),
                    svc_entry,
                    unresolved,
                )
            } {
                Ok(m) => {
                    hint = (m.base + m.image_size + 0x20_0000) & !0x3fff;
                    if let Some(sdk_mod) = sdk.as_ref() {
                        if m.base > sdk_mod.base {
                            println!("[!] {} landed above the SDK; mods that address modules by index may misbehave", label);
                        }
                    }
                    for (k, v) in m.exports.iter() {
                        syms.entry(k.clone()).or_insert(*v);
                    }
                    if run_game_subsdk {
                        // Like rtld: main's imports the library defines are bound to it (horizon::movie:: except the decoder,
                        // which needs the hardware video decoder and stays HLE'd, see hle_nn), and its initialisers
                        // run before nnMain.
                        let mut bound = 0;
                        for (off, name) in mem_space.imports() {
                            let Some(&addr) = m.exports.get(&name) else {
                                continue;
                            };
                            if name.starts_with("_ZN5movie7Decoder")
                                || name.starts_with("_ZNK5movie7Decoder")
                            {
                                continue;
                            }
                            mem_space.bind_got_symbol(off, addr as *const ());
                            bound += 1;
                        }
                        println!("[+] {}: {} of main's imports bound to it", label, bound);
                        game_subsdk_inits.push((label.clone(), m.init, m.init_array));
                    }
                }
                Err(e) => println!("[!] {} not loaded: {}", label, e),
            }
        }
        for (mod_name, dir) in crate::horizon::fs::mods() {
            for i in 0..10 {
                let path = dir.join("exefs").join(format!("subsdk{}", i));
                if !path.is_file() {
                    continue;
                }
                let label = format!("{}/subsdk{}", mod_name, i);
                // Imports that land in raw SDK code (not an HLE the game's imports use) may reach kernel services the
                // engine does not emulate: list them.
                let sdk_range = sdk
                    .as_ref()
                    .map(|m| (m.base, m.base + m.total_size))
                    .unwrap_or((0, 0));
                let resolve = |n: &str| {
                    let a = syms.get(n).copied();
                    if let Some(a) = a {
                        if a >= sdk_range.0 && a < sdk_range.1 {
                            println!(
                                "[!] Mods: {} imports {} from the SDK itself (not HLE'd)",
                                label, n
                            );
                        }
                    }
                    a
                };
                match unsafe {
                    guest::module_loader::load(&path, &label, hint, &resolve, svc_entry, unresolved)
                } {
                    Ok(m) => {
                        hint = (m.base + m.image_size + 0x20_0000) & !0x3fff;
                        if let Some(init) = m.init {
                            mod_inits.push((label, init));
                        }
                    }
                    Err(e) => println!("[!] Mods: {} not loaded: {}", label, e),
                }
            }
        }
        guest::module_loader::set_symbols(syms);
    }

    #[cfg(nx_native)]
    // 4a'. Guest reads of tpidr_el0 get XNU's per-CPU value after a reschedule; point them at the stable
    //      copy of the guest TLS pointer (cpu/tpidr_rewrite.rs). Main text is still writable here; the SDK's is R-X.
    {
        let mut ranges = vec![(mem_space.base_ptr as usize, mem_space.text_size, false)];
        if let Some(sdk_mod) = sdk.as_ref() {
            ranges.push((sdk_mod.text_start, sdk_mod.text_size, true));
        }
        match unsafe {
            cpu::tpidr_rewrite::rewrite(
                &ranges,
                (mem_space.island_slot, guest::loader::ISLAND_SLOT_SIZE),
            )
        } {
            Ok(n) => println!("[+] tpidr_el0 rewrite: {} sites", n),
            // Without it guest TLS reads return XNU's CPU number after any reschedule: the game crashes sooner or
            // later. Fail now.
            Err(e) => return Err(format!("tpidr_el0 rewrite failed: {e}").into()),
        }
    }

    #[cfg(nx_native)]
    // Plain load-exclusives become acquire loads (cpu/exclusive_rewrite.rs).
    {
        let mut ranges = vec![(mem_space.base_ptr as usize, mem_space.text_size, false)];
        if let Some(sdk_mod) = sdk.as_ref() {
            ranges.push((sdk_mod.text_start, sdk_mod.text_size, true));
        }
        let n = unsafe { cpu::exclusive_rewrite::rewrite(&ranges) };
        println!("[+] load-exclusive rewrite: {n} sites now acquire");
    }

    // 4b. Install inline HLE trampolines & version hooks via profile.
    if let Some(version) = profile::active_version() {
        if let Some(installer) = version.install_hooks {
            println!(
                "[+] Installing hooks for {} v{}",
                profile::active().name,
                version.version_string
            );
            unsafe {
                installer(mem_space.base_ptr as usize, &mut mem_space);
            }
        }
    }

    #[cfg(nx_native)]
    // Debug tracer: --trace=0x2b2120,0xcb228c logs registers each time those guest instructions execute (see
    // cpu::crash_handler::install_trace_point; settings::TracePoint for dumping memory at a register).
    {
        for tp in &crate::settings::get().trace.trace {
            let (rel, dump_reg, off_part) = (tp.rel, tp.reg.map(usize::from), tp.deref);
            let pc = mem_space.base_ptr as usize + rel;
            // --trace-from-frame=N: arm when frame N is presented (tracing is slow; skips startup).
            if crate::settings::get().trace.trace_from_frame.is_some() {
                cpu::crash_handler::defer_trace_point(pc, dump_reg, off_part);
                continue;
            }
            if unsafe { cpu::crash_handler::install_trace_point(pc, dump_reg, off_part) } {
                println!("[+] TRACE point installed at guest+{:#x}", rel);
            }
        }
    }

    // 4c. Finalize trampolines (W^X + icache flush) before marking .text exec.
    mem_space.finalize_trampoline_pool();
    #[cfg(nx_native)]
    if let Some(island) = cpu::trampoline::ISLAND.lock().unwrap().as_ref() {
        island.finalize().map_err(|e| format!("x18 island: {e}"))?;
    }

    // 5. Mark .text executable (W^X)
    mem_space.protect_text_executable()?;
    Ok(Loaded {
        mem_space,
        sdk,
        mod_inits,
        game_subsdk_inits,
    })
}

/// Runs a game. This host has no runtime (docs/windows-port.md).
#[cfg(not(nx_runtime))]
pub fn run(_layers: Vec<settings::SettingsPatch>) -> Result<(), Box<dyn std::error::Error>> {
    Err("nxport cannot run games on this host yet (see docs/windows-port.md)".into())
}

/// Runs a game whose code is lifted (docs/windows-port.md): resolves the settings as the native run does, loads and
/// patches the game, then runs its entry chain in the guest library `files.guest_library` (nxport-cli lift-game and
/// compile-guest, with the same settings: the code was lifted as these patch it). The main thread then runs the window
/// (host/display_sdl.rs) and never returns; with --headless it returns when nnMain does.
#[cfg(all(nx_runtime, not(nx_native)))]
pub fn run(layers: Vec<settings::SettingsPatch>) -> Result<(), Box<dyn std::error::Error>> {
    let s = resolve_settings(layers);
    if s.graphics.gpu != settings::GpuBackend::Vulkan {
        return Err(format!("no {:?} backend here: --gpu=vulkan", s.graphics.gpu).into());
    }
    gpu::install(&vulkan::VULKAN);
    let s = settings::install(s);
    #[cfg(windows)]
    config::app_log();
    // Write-tracked pages and the Vulkan write guard's fault on writes, as natively (cpu::crash_handler).
    #[cfg(windows)]
    os::on_access_fault(lifted_access_fault);
    vulkan::init(s.debug.validate)?;
    // On this thread, before any guest thread: SDL's subsystems belong to the main thread.
    host::audio::init();
    let lib = s.files.guest_library.clone().ok_or("no guest library: --guest-library=<the library compile-guest made>")?;
    let list = lib.with_file_name("modules.txt");
    let names = std::fs::read_to_string(&list).map_err(|e| format!("{}: {e}", list.display()))?;
    let Loaded {
        mem_space,
        sdk: _sdk,
        mod_inits,
        game_subsdk_inits,
    } = load()?;
    let modules: Vec<&str> = names.lines().filter(|l| !l.is_empty()).collect();
    lifted::install(&lib, &modules, lifted_handlers())?;
    println!("[+] Lifted: {} ({})", lib.display(), modules.join(", "));
    let guest_main = std::thread::Builder::new()
        .name("guest-main".into())
        .stack_size(256 << 20)
        .spawn({
            let chain = AssertSend(move || guest_chain(mem_space, mod_inits, game_subsdk_inits, false));
            move || {
                guest::tls::mark_guest_thread();
                chain.run().map_err(|e| e.to_string())
            }
        })?;
    if host::display::headless() {
        return match guest_main.join() {
            Ok(r) => r.map_err(Into::into),
            Err(_) => Err("guest-main thread panicked".into()),
        };
    }
    // As natively: the guest finishing takes the process down, and the main thread owns the window and its event loop.
    std::thread::Builder::new().name("guest-watch".into()).spawn(move || {
        let code = match guest_main.join() {
            Ok(Ok(())) => 0,
            Ok(Err(e)) => {
                eprintln!("[-] guest exited with error: {e}");
                1
            }
            Err(_) => {
                eprintln!("[-] guest-main thread panicked");
                101
            }
        };
        std::process::exit(code);
    })?;
    host::display::run_main_thread()
}

/// A fault on a page the GPU backend or write tracking protects, in the order cpu::crash_handler asks them. The
/// guest's lr and fp are in its Cpu, not in the faulting host context: the Fault leaves them 0.
#[cfg(all(nx_runtime, not(nx_native), windows))]
unsafe fn lifted_access_fault(addr: usize, write: bool, pc: usize) -> bool {
    let f = gpu::Fault { addr, pc, lr: 0, fp: 0, write };
    gpu::backend().access_fault(&f) || guest::dirty_track::handle_fault(addr) || gpu::backend().late_access_fault(&f)
}

/// What lifted code hands the host: supervisor calls (exlaunch mods' runtimes) go to horizon::svc as natively, the
/// counter is lifted_ticks, and traps and branches to code nothing lifted stop the process with the guest's state.
#[cfg(all(nx_runtime, not(nx_native)))]
fn lifted_handlers() -> lifted::Handlers {
    fn svc(cpu: &mut nxport_lifter::abi::Cpu, imm: u32, _pc: u64) {
        let mut regs: [u64; 8] = cpu.x[..8].try_into().unwrap();
        unsafe { horizon::svc::svc_dispatch(&mut regs, imm as u64) };
        cpu.x[..8].copy_from_slice(&regs);
    }
    fn stop(cpu: &nxport_lifter::abi::Cpu, what: &str) -> ! {
        let main = guest::loader::main_base() as u64;
        let rel = |a: u64| if a >= main { format!("{a:#x} (main+{:#x})", a - main) } else { format!("{a:#x}") };
        eprintln!("[-] Lifted: {what}; pc {}, lr {}, sp {:#x}", rel(cpu.pc), rel(cpu.x[30]), cpu.sp);
        for (i, r) in cpu.x.chunks(4).enumerate() {
            eprintln!("    x{:<2} {:#018x} {:#018x} {:#018x} {:#018x}", i * 4, r[0], r[1], r[2], r[3]);
        }
        std::process::exit(1);
    }
    fn trap(cpu: &mut nxport_lifter::abi::Cpu, kind: u32, imm: u32, pc: u64) -> ! {
        cpu.pc = pc;
        stop(cpu, &if kind == 0 { format!("brk #{imm:#x}") } else { "an instruction that was not lifted".to_string() })
    }
    fn unknown(cpu: &mut nxport_lifter::abi::Cpu) {
        stop(cpu, "a branch to code that was not lifted")
    }
    lifted::Handlers { svc, trap, unknown, ticks: guest::regs::lifted_ticks }
}
