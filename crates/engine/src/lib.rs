// The native runtime (guest code running on the host CPU) is cfg(nx_native), see build.rs; on other hosts this crate
// provides the platform-independent parts the builder and the command line use (docs/windows-port.md).
#[cfg(nx_native)]
mod clock;
pub mod config;
#[cfg(nx_native)]
mod cpu;
pub mod game;
pub mod hle_abi;
#[cfg(nx_native)]
mod gpu;
#[cfg(nx_native)]
mod guest;
#[cfg(not(nx_native))]
mod guest {
    pub mod nso;
}
/// NSO parsing, for tools that read a game's modules (the lifter in nxport-build).
pub use guest::nso;
#[cfg(nx_native)]
mod horizon;
#[cfg(nx_native)]
mod host;
#[cfg(target_os = "macos")]
pub use host::ui;
#[cfg(nx_native)]
pub mod metal;
/// The Metal backend's shader tooling that does not need Metal itself (the builder specializes MSL on any host).
#[cfg(not(nx_native))]
pub mod metal {
    pub mod msl_specialize;
    pub mod spirv_cross;
}
#[cfg(nx_native)]
mod nvn;
pub mod profile;
pub mod settings;

#[cfg(nx_native)]
use std::thread;

#[cfg(nx_native)]
use guest::loader::GuestMemorySpace;

#[cfg(nx_native)]
/// Runs a game. `layers` are the caller's settings (a front end's choices, or settings::from_args for a command line),
/// in increasing priority; they go above the platform's and the game's profiles. Must be called on the main thread;
/// returns only on failure.
/// Builds the signed-code image an iOS app needs (cpu/aot.rs): the game's code loaded and patched with `settings`, as
/// the app will patch it at startup, without running any of it. Plugin mods (exefs/subsdkN modules) patch code from
/// their initialisers, so an image with them needs those to run: not done here. Once per process.
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

#[cfg(nx_native)]
/// Runs a game: resolves the settings from `layers` (above the platform's, the installation's and the game's profiles),
/// loads and patches its code, then runs it. Does not return while the game runs.
pub fn run(layers: Vec<settings::SettingsPatch>) -> Result<(), Box<dyn std::error::Error>> {
    gpu::install(&metal::backend::METAL);
    let s = settings::install(resolve_settings(layers));
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
    struct AssertSend<F>(F);
    unsafe impl<F> Send for AssertSend<F> {}
    impl<F: FnOnce() -> Result<(), Box<dyn std::error::Error>>> AssertSend<F> {
        fn run(self) -> Result<(), Box<dyn std::error::Error>> {
            (self.0)()
        }
    }
    // iOS: the executor runs from the guest's first submit; the window's layer is attached once UIKit has created it
    // (host/display_ios.rs). macOS creates its window before the guest submits anything.
    #[cfg(target_os = "ios")]
    if !host::display::headless() {
        crate::metal::init(
            objc2_metal::MTLCreateSystemDefaultDevice().expect("no Metal device"),
            None,
        );
    }
    let guest_chain = AssertSend(move || -> Result<(), Box<dyn std::error::Error>> {
        // 5a. Install TPIDR_EL0 (thread-local storage pointer) for this thread.
        unsafe {
            cpu::tls::install_guest_tls();
            cpu::trampoline::init_worker_thread_x18();
            horizon::hle::hle_nn_os_GetCurrentThread();
            horizon::hle::register_main_sample_target();
        };

        // Helper: restore x18 = thread-ctx-block before every guest entry point.

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
    });

    let handle = thread::Builder::new()
        .name("guest-main".into())
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            cpu::tls::mark_guest_thread();
            guest_chain.run().map_err(|e| e.to_string())
        })
        .expect("spawn guest-main");

    if host::display::headless() {
        let device = objc2_metal::MTLCreateSystemDefaultDevice().expect("no Metal device");
        crate::metal::init(device, None);
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

#[cfg(nx_native)]
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

#[cfg(nx_native)]
struct Loaded {
    mem_space: GuestMemorySpace,
    /// The sdk module: it must outlive the guest (dropping it unmaps the module's memory).
    sdk: Option<guest::sdk_loader::SdkModule>,
    /// The mods' module initialisers (label, address), run before main's.
    mod_inits: Vec<(String, usize)>,
    /// The game's own subsdk modules' initialisers (--run-game-subsdk): label, DT_INIT, init array.
    game_subsdk_inits: Vec<(String, Option<usize>, (usize, usize))>,
}

#[cfg(nx_native)]
/// Loads the game's modules and the mods' into guest memory and patches them for this host: relocations, HLE bindings,
/// SDK interposition, x18 / tpidr rewrites, game hooks, then makes the code executable. Nothing of the guest has run yet.
fn load() -> Result<Loaded, Box<dyn std::error::Error>> {
    // The TSD keys whose offsets patched code embeds (fixed numbers, cpu/tls.rs), before anything else creates keys.
    let _ = (
        cpu::tls::guest_tls_offset(),
        cpu::tls::errno_save_offset(),
        cpu::tls::x18_slot_offset(),
        cpu::tls::hle_stack_offset(),
    );
    unsafe {
        horizon::hle::capture_darwin_tsd_shadow();
    }

    // 1. Allocate & populate guest memory space
    let mut mem_space = GuestMemorySpace::allocate()?;

    // 2. Install native Darwin crash & trap handler
    cpu::crash_handler::set_guest_base(mem_space.base_ptr as usize);
    unsafe { cpu::crash_handler::install_signal_handlers() };

    mem_space.load_sections()?;
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
    let mut missing_imports: Vec<&'static str> = Vec::new();
    macro_rules! bind_import {
        ($name:expr, $kind:expr, $fn:expr) => {{
            let slots = mem_space.got_slots($name, $kind);
            if slots.is_empty() {
                missing_imports.push($name);
            } else {
                let stub = mem_space.make_hle_trampoline($fn as *const ());
                for s in slots {
                    mem_space.bind_got_symbol(s, stub);
                }
            }
        }};
    }

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
    // Guest code: main, the mod modules mapped after it, the SDK (--report-guard waits only for faults from there).
    metal::set_guest_code_range(
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
            let base = mem_space.base_ptr as usize;
            let image = mem_space.bss_off + mem_space.bss_size;
            guest::mod_patches::apply(
                "main",
                &id,
                base,
                image,
                (base, mem_space.text_size),
                libc::PROT_READ | libc::PROT_WRITE,
            );
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
                libc::PROT_READ | libc::PROT_EXEC,
            );
        }
    }

    // 3. Bind specific GOT entries to our native HLE shims via x9-restoring trampolines.

    // Bind specific routines
    bind_import!(
        "__nnmusl_init_dso",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nnmusl_init_dso
    );
    bind_import!("memset", guest::loader::JUMP_SLOT, horizon::hle::hle_memset);
    bind_import!("memcpy", guest::loader::JUMP_SLOT, horizon::hle::hle_memcpy);
    bind_import!(
        "_ZN2nn2os9LockMutexEPNS0_9MutexTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_LockMutex
    );
    bind_import!(
        "_ZN2nn2os11UnlockMutexEPNS0_9MutexTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_UnlockMutex
    );
    bind_import!(
        "_ZN2nn2os15InitializeMutexEPNS0_9MutexTypeEbi",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeMutex
    );

    // Bind C++ ABI guards
    bind_import!(
        "__cxa_guard_acquire",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_cxa_guard_acquire
    );
    bind_import!(
        "__cxa_guard_release",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_cxa_guard_release
    );
    // nn::util::BitArray::reset is pure computation in the sdk module
    // (confirmed by disassembly: it only calls the SDK's own internal
    // memset, no svc, no external dependency) -- let it run for real instead
    // of the Rust stand-in, same reasoning as StandardAllocator below.
    // bind_import!("_ZN2nn4util8BitArray5resetEv", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_util_BitArray_reset);

    // Bind Horizon Heap & Allocator routines
    bind_import!(
        "_ZN2nn2oe15SetCpuBoostModeENS0_12CpuBoostModeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_SetCpuBoostMode
    );
    bind_import!(
        "_ZN2nn2os17SetMemoryHeapSizeEm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SetMemoryHeapSize
    );
    bind_import!(
        "_ZN2nn2os19AllocateMemoryBlockEPmm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AllocateMemoryBlock
    );
    bind_import!(
        "_ZN2nn2os15FreeMemoryBlockEmm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FreeMemoryBlock
    );
    bind_import!(
        "_ZN2nn2os32SetMemoryAllocatorForThreadLocalEPFPvmmEPFvS1_mE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SetMemoryAllocatorForThreadLocal
    );
    bind_import!("__errno_location", guest::loader::JUMP_SLOT, libc::__error);

    // Bind nn::oe and nn::account
    bind_import!(
        "_ZN2nn2oe20SetFocusHandlingModeENS0_17FocusHandlingModeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_SetFocusHandlingMode
    );
    bind_import!(
        "_ZN2nn2oe27SetPerformanceConfigurationENS0_15PerformanceModeEi",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_SetPerformanceConfiguration
    );
    bind_import!(
        "_ZN2nn2oe10InitializeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_Initialize
    );
    bind_import!(
        "_ZN2nn2oe42SetOperationModeChangedNotificationEnabledEb",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_SetOperationModeChangedNotificationEnabled
    );
    bind_import!(
        "_ZN2nn2oe44SetPerformanceModeChangedNotificationEnabledEb",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_SetPerformanceModeChangedNotificationEnabled
    );
    bind_import!(
        "_ZN2nn2oe16GetOperationModeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_GetOperationMode
    );
    bind_import!(
        "_ZN2nn7account10InitializeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_account_Initialize
    );
    bind_import!(
        "_ZN2nn7account22TryOpenPreselectedUserEPNS0_10UserHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_account_TryOpenPreselectedUser
    );
    bind_import!(
        "_ZN2nn7account19OpenPreselectedUserEPNS0_10UserHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_account_OpenPreselectedUser
    );
    bind_import!(
        "_ZN2nn7account9GetUserIdEPNS0_3UidERKNS0_10UserHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_account_GetUserId
    );
    bind_import!(
        "_ZN2nn7account17GetLastOpenedUserEPNS0_3UidE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_account_GetLastOpenedUser
    );
    bind_import!(
        "_ZN2nn7account11GetNicknameEPNS0_8NicknameERKNS0_3UidE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_account_GetNickname
    );
    bind_import!(
        "_ZN2nn7account9CloseUserERKNS0_10UserHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_account_CloseUser
    );
    bind_import!(
        "_ZN2nn2oe28SetResumeNotificationEnabledEb",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_SetResumeNotificationEnabled
    );
    bind_import!(
        "_ZN2nn2oe18GetPerformanceModeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_GetPerformanceMode
    );
    bind_import!(
        "_ZN2nn2oe27GetPerformanceConfigurationENS0_15PerformanceModeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_GetPerformanceConfiguration
    );
    bind_import!(
        "_ZN2nn2oe20GetCurrentFocusStateEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_GetCurrentFocusState
    );

    // Bind Threading, TLS, and System GOT symbols
    bind_import!("strcmp", guest::loader::JUMP_SLOT, horizon::hle::hle_strcmp);
    // GetSystemTick / GetSystemTickFrequency / ConvertToTimeSpan need no kernel, but the sdk module's frequency is the
    // Switch's fixed 19.2 MHz while its tick would read the host counter: all three are HLE'd so they agree
    // (crate::clock).
    bind_import!(
        "_ZN2nn2os13GetSystemTickEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetSystemTick
    );
    bind_import!(
        "_ZN2nn2os22GetSystemTickFrequencyEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetSystemTickFrequency
    );
    bind_import!(
        "_ZN2nn2os16GetCurrentThreadEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetCurrentThread
    );
    bind_import!(
        "_ZN2nn2os11GetThreadIdEPKNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetThreadId
    );
    bind_import!(
        "_ZN2nn2os20ChangeThreadPriorityEPNS0_10ThreadTypeEi",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ChangeThreadPriority
    );
    bind_import!(
        "_ZN2nn2os17GetThreadPriorityEPKNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetThreadPriority
    );
    bind_import!(
        "_ZN2nn2os11SetTlsValueENS0_7TlsSlotEm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SetTlsValue
    );
    bind_import!(
        "_ZN2nn2os15AllocateTlsSlotEPNS0_7TlsSlotEPFvmE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AllocateTlsSlot
    );
    // The SDK-internal variant (Super Mario Bros. Wonder's static constructors): the SDK's own walks its thread list,
    // which is empty under the HLE threads.
    bind_import!(
        "_ZN2nn2os18SdkAllocateTlsSlotEPNS0_7TlsSlotEPFvmE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AllocateTlsSlot
    );
    bind_import!(
        "_ZN2nn2os11FreeTlsSlotENS0_7TlsSlotE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FreeTlsSlot
    );
    bind_import!(
        "_ZN2nn2os26GetThreadAvailableCoreMaskEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetThreadAvailableCoreMask
    );
    bind_import!(
        "_ZN2nn2os11GetTlsValueENS0_7TlsSlotE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetTlsValue
    );
    bind_import!(
        "_ZN2nn2os15QueryMemoryInfoEPNS0_10MemoryInfoE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_QueryMemoryInfo
    );
    bind_import!(
        "_ZN2nn2os21AllocateAddressRegionEPmm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AllocateAddressRegion
    );
    bind_import!(
        "_ZN2nn2os19AllocateMemoryPagesEmm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AllocateMemoryPages
    );
    bind_import!(
        "_ZN2nn2os17FreeAddressRegionEm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FreeAddressRegion
    );
    bind_import!(
        "_ZN2nn2os15FreeMemoryPagesEmm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FreeMemoryPages
    );
    bind_import!(
        "_ZN2nn2os17SetThreadCoreMaskEPNS0_10ThreadTypeEim",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SetThreadCoreMask
    );
    bind_import!(
        "_ZN2nn2os11SleepThreadENS_8TimeSpanE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SleepThread
    );
    bind_import!(
        "_ZN2nn2os11YieldThreadEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_YieldThread
    );
    bind_import!(
        "_ZN2nn2os12CreateThreadEPNS0_10ThreadTypeEPFvPvES3_S3_mi",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_CreateThread
    );
    bind_import!(
        "_ZN2nn2os12CreateThreadEPNS0_10ThreadTypeEPFvPvES3_S3_mii",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_CreateThreadWithCore
    );
    bind_import!(
        "_ZN2nn2os13SetThreadNameEPNS0_10ThreadTypeEPKc",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SetThreadName
    );
    bind_import!(
        "_ZN2nn2os20SetThreadNamePointerEPNS0_10ThreadTypeEPKc",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SetThreadNamePointer
    );
    bind_import!(
        "_ZN2nn2os20GetThreadNamePointerEPKNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetThreadNamePointer
    );
    bind_import!(
        "_ZN2nn2os17GetThreadCoreMaskEPiPmPKNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetThreadCoreMask
    );
    bind_import!(
        "_ZN2nn2os26BroadcastConditionVariableEPNS0_21ConditionVariableTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_BroadcastConditionVariable
    );
    bind_import!("_ZN2nn2os26TimedWaitConditionVariableEPNS0_21ConditionVariableTypeEPNS0_9MutexTypeENS_8TimeSpanE", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_TimedWaitConditionVariable);
    bind_import!(
        "_ZN2nn2os13DestroyThreadEPNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_DestroyThread
    );
    bind_import!(
        "_ZN2nn2os11StartThreadEPNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_StartThread
    );
    bind_import!(
        "_ZN2nn2os10WaitThreadEPNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_WaitThread
    );
    bind_import!(
        "_ZN2nn2os13TryWaitThreadEPNS0_10ThreadTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TryWaitThread
    );
    bind_import!(
        "_ZN2nn2fs26SetPriorityOnCurrentThreadENS0_8PriorityE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_fs_SetPriorityOnCurrentThread
    );

    // C ABI bindings for Horizon OS (used by SMO and middleware)
    bind_import!("nnosGetCurrentThread", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_GetCurrentThread);
    bind_import!("nnosGetThreadPriority", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_GetThreadPriority);
    bind_import!("nnosCreateThread", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_CreateThread);
    bind_import!("nnosDestroyThread", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_DestroyThread);
    bind_import!("nnosGetThreadCoreMask", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_GetThreadCoreMask);
    bind_import!("nnosSetThreadCoreMask", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_SetThreadCoreMask);
    bind_import!("nnosSetThreadNamePointer", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_SetThreadNamePointer);
    bind_import!("nnosInitializeMutex", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_InitializeMutex);
    bind_import!("nnosFinalizeMutex", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_FinalizeMutex);
    bind_import!("nnosLockMutex", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_LockMutex);
    bind_import!("nnosUnlockMutex", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_UnlockMutex);
    bind_import!("nnosStartThread", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_StartThread);
    bind_import!("nnosWaitThread", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_WaitThread);
    bind_import!("nnosSleepThread", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_SleepThread);
    bind_import!("nnosGetSystemTick", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_GetSystemTick);
    bind_import!("nnosGetSystemTickFrequency", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_GetSystemTickFrequency);
    bind_import!("nnosConvertTickToNanoSeconds", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_os_ConvertToTimeSpan);
    bind_import!("nnosInitializeEvent", guest::loader::JUMP_SLOT, horizon::nn::os_initialize_event);
    bind_import!("nnosFinalizeEvent", guest::loader::JUMP_SLOT, horizon::nn::os_finalize_event);
    bind_import!("nnosSignalEvent", guest::loader::JUMP_SLOT, horizon::nn::os_signal_event);
    bind_import!("nnosWaitEvent", guest::loader::JUMP_SLOT, horizon::nn::os_wait_event);

    // Bind ConvertToTimeSpan & Human Interface Device (nn::hid)
    bind_import!(
        "_ZN2nn2os17ConvertToTimeSpanENS0_4TickE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ConvertToTimeSpan
    );
    bind_import!(
        "_ZN2nn3hid14InitializeNpadEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_InitializeNpad
    );
    bind_import!(
        "_ZN2nn3hid22SetSupportedNpadIdTypeEPKjm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_SetSupportedNpadIdType
    );
    bind_import!(
        "_ZN2nn3hid24SetSupportedNpadStyleSetENS_4util10BitFlagSetILi32ENS0_12NpadStyleTagEEE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_SetSupportedNpadStyleSet
    );
    bind_import!(
        "_ZN2nn3hid15GetNpadStyleSetERKj",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetNpadStyleSet
    );
    bind_import!(
        "_ZN2nn3hid13GetNpadStatesEPNS0_16NpadFullKeyStateEiRKj",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetNpadStates
    );
    bind_import!("_ZN2nn3hid23GetSixAxisSensorHandlesEPNS0_19SixAxisSensorHandleEiRKjNS_4util10BitFlagSetILi32ENS0_12NpadStyleTagEEE", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_hid_GetSixAxisSensorHandles);
    bind_import!("_ZN2nn3hid25GetVibrationDeviceHandlesEPNS0_21VibrationDeviceHandleEiRKjNS_4util10BitFlagSetILi32ENS0_12NpadStyleTagEEE", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_hid_GetVibrationDeviceHandles);
    bind_import!(
        "_ZN2nn3hid18StartSixAxisSensorERKNS0_19SixAxisSensorHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_StartSixAxisSensor
    );
    bind_import!(
        "_ZN2nn3hid25InitializeVibrationDeviceERKNS0_21VibrationDeviceHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_InitializeVibrationDevice
    );
    bind_import!(
        "_ZN2nn3hid13GetNpadStatesEPNS0_16NpadJoyDualStateEiRKj",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetNpadJoyDualStates
    );
    bind_import!(
        "_ZN2nn3hid13GetNpadStatesEPNS0_17NpadHandheldStateEiRKj",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetNpadHandheldStates
    );
    bind_import!(
        "_ZN2nn3hid18SetNpadJoyHoldTypeENS0_15NpadJoyHoldTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_SetNpadJoyHoldType
    );
    bind_import!(
        "_ZN2nn3hid18GetNpadJoyHoldTypeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetNpadJoyHoldType
    );
    bind_import!(
        "_ZN2nn3hid24GetSupportedNpadStyleSetEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetSupportedNpadStyleSet
    );
    bind_import!(
        "_ZN2nn3hid20GetNpadJoyAssignmentERKj",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetNpadJoyAssignment
    );
    bind_import!(
        "_ZN2nn3hid30SetNpadJoyAssignmentModeSingleERKjNS0_17NpadJoyDeviceTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_assignment_noop
    );
    bind_import!(
        "_ZN2nn3hid28SetNpadJoyAssignmentModeDualERKj",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_assignment_noop
    );
    bind_import!(
        "_ZN2nn3hid23MergeSingleJoyAsDualJoyERKjS2_",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_assignment_noop
    );
    bind_import!(
        "_ZN2nn3hid21StartLrAssignmentModeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_assignment_noop
    );
    bind_import!(
        "_ZN2nn3hid20StopLrAssignmentModeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_assignment_noop
    );
    bind_import!(
        "_ZN2nn3hid18SwapNpadAssignmentERKjS2_",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_assignment_noop
    );
    bind_import!(
        "_ZN2nn3hid24ShowControllerStrapGuideEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_assignment_noop
    );
    bind_import!(
        "_ZN2nn3hid19GetTouchScreenStateILm16EEEvPNS0_16TouchScreenStateIXT_EEE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetTouchScreenState
    );
    bind_import!(
        "_ZN2nn3hid21GetSixAxisSensorStateEPNS0_18SixAxisSensorStateERKNS0_19SixAxisSensorHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetSixAxisSensorState
    );
    bind_import!(
        "_ZN2nn3hid21IsSixAxisSensorAtRestERKNS0_19SixAxisSensorHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_IsSixAxisSensorAtRest
    );
    bind_import!(
        "_ZN2nn3hid17StopSixAxisSensorERKNS0_19SixAxisSensorHandleE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_StopSixAxisSensor
    );
    bind_import!(
        "_ZN2nn3hid18SendVibrationValueERKNS0_21VibrationDeviceHandleERKNS0_14VibrationValueE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_SendVibrationValue
    );
    bind_import!(
        "_ZN2nn3hid20ControllerSupportArg10SetDefaultEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_ControllerSupportArg_SetDefault
    );
    bind_import!("_ZN2nn3hid21ShowControllerSupportEPNS0_27ControllerSupportResultInfoERKNS0_20ControllerSupportArgE", guest::loader::JUMP_SLOT, horizon::hle::hle_nn_hid_ShowControllerSupport);
    bind_import!(
        "_ZN2nn3hid28InitializeSevenSixAxisSensorEPvm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_InitializeSevenSixAxisSensor
    );
    bind_import!(
        "_ZN2nn3hid26FinalizeSevenSixAxisSensorEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_FinalizeSevenSixAxisSensor
    );
    bind_import!(
        "_ZN2nn3hid23StartSevenSixAxisSensorEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_StartSevenSixAxisSensor
    );
    bind_import!(
        "_ZN2nn3hid22StopSevenSixAxisSensorEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_StopSevenSixAxisSensor
    );
    bind_import!(
        "_ZN2nn3hid26IsSevenSixAxisSensorAtRestEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_IsSevenSixAxisSensorAtRest
    );
    bind_import!(
        "_ZN2nn3hid27GetSevenSixAxisSensorStatesEPNS0_23SevenSixAxisSensorStateEi",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetSevenSixAxisSensorStates
    );
    bind_import!(
        "_ZN2nn3hid32ResetSevenSixAxisSensorTimestampEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_ResetSevenSixAxisSensorTimestamp
    );
    bind_import!(
        "_ZN2nn3hid15InitializeMouseEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_InitializeMouse
    );
    bind_import!(
        "_ZN2nn3hid18InitializeKeyboardEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_InitializeKeyboard
    );
    bind_import!(
        "_ZN2nn3hid14GetMouseStatesEPNS0_10MouseStateEi",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetMouseStates
    );
    bind_import!(
        "_ZN2nn3hid16GetKeyboardStateEPNS0_13KeyboardStateE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetKeyboardState
    );
    bind_import!(
        "_ZN2nn3hid18InitializeDebugPadEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_InitializeDebugPad
    );
    bind_import!(
        "_ZN2nn3hid16GetDebugPadStateEPNS0_13DebugPadStateE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetDebugPadState
    );
    bind_import!(
        "_ZN2nn3hid19GetTouchScreenStateILm1EEEvPNS0_16TouchScreenStateIXT_EEE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_hid_GetTouchScreenState1
    );
    bind_import!(
        "_ZN2nn4diag18IsDebuggerAttachedEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_diag_IsDebuggerAttached
    );
    bind_import!(
        "_ZN2nn2oe38SetUserInactivityDetectionTimeExtendedEb",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_SetUserInactivityDetectionTimeExtended
    );

    // Bind nn::os::TimerEvent
    bind_import!(
        "_ZN2nn2os18FinalizeTimerEventEPNS0_14TimerEventTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeTimerEvent
    );
    bind_import!(
        "_ZN2nn2os20InitializeTimerEventEPNS0_14TimerEventTypeENS0_14EventClearModeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeTimerEvent
    );
    bind_import!(
        "_ZN2nn2os23StartPeriodicTimerEventEPNS0_14TimerEventTypeENS_8TimeSpanES3_",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_StartPeriodicTimerEvent
    );
    bind_import!(
        "_ZN2nn2os14WaitTimerEventEPNS0_14TimerEventTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_WaitTimerEvent
    );

    // Bind Stack, Mutex & RWLock
    bind_import!(
        "_ZN2nn2os19GetCurrentStackInfoEPmS1_",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetCurrentStackInfo
    );
    bind_import!(
        "_ZN2nn2os20GetCurrentCoreNumberEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_GetCurrentCoreNumber
    );
    bind_import!(
        "_ZN2nn2os12TryLockMutexEPNS0_9MutexTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TryLockMutex
    );
    bind_import!(
        "_ZN2nn2os28IsMutexLockedByCurrentThreadEPKNS0_9MutexTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_IsMutexLockedByCurrentThread
    );
    bind_import!(
        "_ZN2nn2os13FinalizeMutexEPNS0_9MutexTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeMutex
    );
    bind_import!(
        "_ZN2nn2os26InitializeReaderWriterLockEPNS0_20ReaderWriterLockTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeReaderWriterLock
    );
    bind_import!(
        "_ZN2nn2os24FinalizeReaderWriterLockEPNS0_20ReaderWriterLockTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeReaderWriterLock
    );
    bind_import!(
        "_ZN2nn2os16AcquireWriteLockEPNS0_20ReaderWriterLockTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AcquireWriteLock
    );
    bind_import!(
        "_ZN2nn2os16ReleaseWriteLockEPNS0_20ReaderWriterLockTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ReleaseWriteLock
    );
    bind_import!(
        "_ZN2nn2os15AcquireReadLockEPNS0_20ReaderWriterLockTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AcquireReadLock
    );
    bind_import!(
        "_ZN2nn2os15ReleaseReadLockEPNS0_20ReaderWriterLockTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ReleaseReadLock
    );
    bind_import!(
        "_ZN2nn2os30IsWriteLockHeldByCurrentThreadEPKNS0_20ReaderWriterLockTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_IsWriteLockHeldByCurrentThread
    );
    bind_import!(
        "_ZN2nn2oe18GetDesiredLanguageEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_oe_GetDesiredLanguage
    );
    bind_import!(
        "_ZN2nn2os27InitializeLightMessageQueueEPNS0_21LightMessageQueueTypeEPmm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os25FinalizeLightMessageQueueEPNS0_21LightMessageQueueTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os24TrySendLightMessageQueueEPNS0_21LightMessageQueueTypeEm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TrySendLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os21SendLightMessageQueueEPNS0_21LightMessageQueueTypeEm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SendLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os24ReceiveLightMessageQueueEPmPNS0_21LightMessageQueueTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ReceiveLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os27TryReceiveLightMessageQueueEPmPNS0_21LightMessageQueueTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TryReceiveLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os26TimedPeekLightMessageQueueEPmPKNS0_21LightMessageQueueTypeENS_8TimeSpanE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TimedPeekLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os20JamLightMessageQueueEPNS0_21LightMessageQueueTypeEm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_JamLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os23TryJamLightMessageQueueEPNS0_21LightMessageQueueTypeEm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TryJamLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os24TryPeekLightMessageQueueEPmPKNS0_21LightMessageQueueTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TryPeekLightMessageQueue
    );
    bind_import!(
        "_ZN2nn2os23SignalConditionVariableEPNS0_21ConditionVariableTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SignalConditionVariable
    );
    bind_import!(
        "_ZN2nn2os21WaitConditionVariableEPNS0_21ConditionVariableTypeEPNS0_9MutexTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_WaitConditionVariable
    );
    bind_import!(
        "_ZN2nn2os27InitializeConditionVariableEPNS0_21ConditionVariableTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeConditionVariable
    );
    bind_import!(
        "_ZN2nn2os25FinalizeConditionVariableEPNS0_21ConditionVariableTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeConditionVariable
    );
    bind_import!(
        "_ZN2nn2os20InitializeLightEventEPNS0_14LightEventTypeEbNS0_14EventClearModeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeLightEvent
    );
    bind_import!(
        "_ZN2nn2os16SignalLightEventEPNS0_14LightEventTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_SignalLightEvent
    );
    bind_import!(
        "_ZN2nn2os14WaitLightEventEPNS0_14LightEventTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_WaitLightEvent
    );
    bind_import!(
        "_ZN2nn2os18FinalizeLightEventEPNS0_14LightEventTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeLightEvent
    );
    bind_import!(
        "_ZN2nn2os15ClearLightEventEPNS0_14LightEventTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ClearLightEvent
    );
    bind_import!(
        "_ZN2nn2os19TimedWaitLightEventEPNS0_14LightEventTypeENS_8TimeSpanE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_TimedWaitLightEvent
    );
    bind_import!(
        "_ZN2nn2os19InitializeSemaphoreEPNS0_13SemaphoreTypeEii",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeSemaphore
    );
    bind_import!(
        "_ZN2nn2os17FinalizeSemaphoreEPNS0_13SemaphoreTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeSemaphore
    );
    bind_import!(
        "_ZN2nn2os16AcquireSemaphoreEPNS0_13SemaphoreTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AcquireSemaphore
    );
    bind_import!(
        "_ZN2nn2os16ReleaseSemaphoreEPNS0_13SemaphoreTypeEi",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ReleaseSemaphore
    );
    bind_import!(
        "_ZN2nn2os24InitializeLightSemaphoreEPNS0_18LightSemaphoreTypeEii",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_InitializeLightSemaphore
    );
    bind_import!(
        "_ZN2nn2os22FinalizeLightSemaphoreEPNS0_18LightSemaphoreTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_FinalizeLightSemaphore
    );
    bind_import!(
        "_ZN2nn2os21AcquireLightSemaphoreEPNS0_18LightSemaphoreTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_AcquireLightSemaphore
    );
    bind_import!(
        "_ZN2nn2os21ReleaseLightSemaphoreEPNS0_18LightSemaphoreTypeE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_os_ReleaseLightSemaphore
    );

    // Bind File System (nn::fs)
    bind_import!(
        "_ZN2nn2fs22QueryMountRomCacheSizeEPm",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_QueryMountRomCacheSize
    );
    bind_import!(
        "_ZN2nn2fs8MountRomEPKcPvm",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_MountRom
    );
    bind_import!(
        "_ZN2nn2fs13OpenDirectoryEPNS0_15DirectoryHandleEPKci",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_OpenDirectory
    );
    bind_import!(
        "_ZN2nn2fs13ReadDirectoryEPlPNS0_14DirectoryEntryENS0_15DirectoryHandleEl",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_ReadDirectory
    );
    bind_import!(
        "_ZN2nn2fs14CloseDirectoryENS0_15DirectoryHandleE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_CloseDirectory
    );
    bind_import!(
        "_ZN2nn2fs22GetDirectoryEntryCountEPlNS0_15DirectoryHandleE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_GetDirectoryEntryCount
    );
    bind_import!(
        "_ZN2nn2fs8OpenFileEPNS0_10FileHandleEPKci",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_OpenFile
    );
    bind_import!(
        "_ZN2nn2fs11GetFileSizeEPlNS0_10FileHandleE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_GetFileSize
    );
    bind_import!(
        "_ZN2nn2fs9CloseFileENS0_10FileHandleE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_CloseFile
    );
    bind_import!(
        "_ZN2nn2fs8ReadFileEPmNS0_10FileHandleElPvm",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_ReadFile_no_option
    );
    bind_import!(
        "_ZN2nn2fs8ReadFileEPmNS0_10FileHandleElPvmRKNS0_10ReadOptionE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_ReadFile
    );
    bind_import!(
        "_ZN2nn2fs8ReadFileENS0_10FileHandleElPvm",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_ReadFile_simple
    );
    bind_import!(
        "_ZN2nn2fs12GetEntryTypeEPNS0_18DirectoryEntryTypeEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_GetEntryType
    );
    bind_import!(
        "_ZN2nn2fs6CommitEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_Commit
    );
    bind_import!(
        "_ZN2nn2fs14CommitSaveDataEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_Commit
    );
    bind_import!(
        "_ZN2nn2fs25CleanDirectoryRecursivelyEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_CleanDirectoryRecursively
    );
    bind_import!(
        "_ZN2nn2fs14EnsureSaveDataERKNS_7account3UidE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_EnsureSaveData
    );
    bind_import!(
        "_ZN2nn2fs7UnmountEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_Unmount
    );
    bind_import!(
        "_ZN2nn2fs18IsSaveDataExistingENS_13ApplicationIdERKNS_7account3UidE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_IsSaveDataExisting
    );
    bind_import!(
        "_ZN2nn2fs21MountSaveDataReadOnlyEPKcNS_13ApplicationIdERKNS_7account3UidE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_MountSaveDataReadOnly
    );
    bind_import!(
        "_ZN2nn2fs21MountTemporaryStorageEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_MountTemporaryStorage
    );
    bind_import!(
        "_ZN2nn2fs13MountSaveDataEPKcRKNS_7account3UidE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_MountSaveData
    );
    bind_import!(
        "_ZN2nn2fs10DeleteFileEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_DeleteFile
    );
    bind_import!(
        "_ZN2nn2fs26DeleteDirectoryRecursivelyEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_DeleteDirectoryRecursively
    );
    bind_import!(
        "_ZN2nn2fs15CreateDirectoryEPKc",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_CreateDirectory
    );
    bind_import!(
        "_ZN2nn2fs11SetFileSizeENS0_10FileHandleEl",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_SetFileSize
    );
    bind_import!(
        "_ZN2nn2fs10CreateFileEPKcl",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_CreateFile
    );
    bind_import!(
        "_ZN2nn2fs9FlushFileENS0_10FileHandleE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_FlushFile
    );
    bind_import!(
        "_ZN2nn2fs9WriteFileENS0_10FileHandleElPKvmRKNS0_11WriteOptionE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_WriteFile
    );
    bind_import!(
        "_ZN2nn2fs10RenameFileEPKcS2_",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_RenameFile
    );
    bind_import!(
        "_ZN2nn2fs20OpenCacheStorageListEPNS0_22CacheStorageListHandleE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_OpenCacheStorageList
    );
    bind_import!(
        "_ZN2nn2fs20ReadCacheStorageListEPiPNS0_16CacheStorageInfoENS0_22CacheStorageListHandleEi",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_ReadCacheStorageList
    );
    bind_import!(
        "_ZN2nn2fs21CloseCacheStorageListENS0_22CacheStorageListHandleE",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_CloseCacheStorageList
    );
    bind_import!(
        "_ZN2nn2fs17MountCacheStorageEPKci",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_MountCacheStorage
    );
    bind_import!(
        "_ZN2nn2fs18DeleteCacheStorageEi",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_DeleteCacheStorage
    );
    bind_import!(
        "_ZN2nn2fs18CreateCacheStorageEill",
        guest::loader::JUMP_SLOT,
        horizon::fs::hle_nn_fs_CreateCacheStorage
    );

    // Bind Graphics (NV, VI, NVN)
    bind_import!(
        "_ZN2nv20SetGraphicsAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nv_SetGraphicsAllocator
    );
    bind_import!(
        "_ZN2nv18InitializeGraphicsEPvm",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nv_InitializeGraphics
    );
    bind_import!(
        "_ZN2nv28SetGraphicsDevtoolsAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nv_SetGraphicsDevtoolsAllocator
    );
    bind_import!(
        "_ZN2nn2vi10InitializeEv",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_vi_Initialize
    );
    bind_import!(
        "_ZN2nn2vi18OpenDefaultDisplayEPPNS0_7DisplayE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_vi_OpenDefaultDisplay
    );
    bind_import!(
        "_ZN2nn2vi11CreateLayerEPPNS0_5LayerEPNS0_7DisplayE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_vi_CreateLayer
    );
    bind_import!(
        "_ZN2nn2vi20GetDisplayVsyncEventEPNS_2os15SystemEventTypeEPNS0_7DisplayE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_vi_GetDisplayVsyncEvent
    );
    bind_import!(
        "nvnBootstrapLoader",
        guest::loader::JUMP_SLOT,
        nvn::api::hle_nvnBootstrapLoader
    );
    bind_import!(
        "_ZN2nn2vi15GetNativeWindowEPPvPNS0_5LayerE",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_nn_vi_GetNativeWindow
    );

    // Bind Libc string & memory routines
    bind_import!("strlen", guest::loader::JUMP_SLOT, libc::strlen);
    bind_import!("memcmp", guest::loader::JUMP_SLOT, libc::memcmp);
    bind_import!(
        "memmove",
        guest::loader::JUMP_SLOT,
        horizon::hle::hle_memmove
    );
    bind_import!("time", guest::loader::JUMP_SLOT, libc::time);
    bind_import!("clock", guest::loader::JUMP_SLOT, clock);

    // Bind libc output (pure libc string/math/formatting functions resolve to the SDK's own definitions)
    extern "C" {
        fn clock() -> libc::clock_t;
        static mut __stdoutp: *mut libc::FILE;
    }

    bind_import!("printf", guest::loader::JUMP_SLOT, libc::printf);
    bind_import!("fflush", guest::loader::JUMP_SLOT, libc::fflush);

    // GLOB_DAT system globals (nn::util math constants resolve to the SDK's own definitions)
    bind_import!(
        "_ZN2nn2os20GetCurrentCoreNumberEv",
        guest::loader::GLOB_DAT,
        horizon::hle::hle_nn_os_GetCurrentCoreNumber
    );
    for slot in mem_space.got_slots("stdout", guest::loader::GLOB_DAT) {
        mem_space.bind_got_symbol(slot, &raw mut __stdoutp as *const ());
    }
    static mut NVN_PRIVATE_BLOCK: [u64; 64] = [0; 64];
    for slot in mem_space.got_slots("nvnPrivate0", guest::loader::GLOB_DAT) {
        mem_space.bind_got_symbol(slot, &raw mut NVN_PRIVATE_BLOCK as *const ());
    }
    // Overrides written for mod modules (nifm, socket, single-npad hid reads, ...) also cover the game's own imports of
    // the same functions (TotK imports none of them; Kirby imports several).
    for (name, f) in guest::module_loader::runtime_overrides() {
        let slots = mem_space.got_slots(name, guest::loader::JUMP_SLOT);
        if !slots.is_empty() {
            let stub = mem_space.make_hle_trampoline(f);
            for s in slots {
                mem_space.bind_got_symbol(s, stub);
            }
        }
    }
    if !missing_imports.is_empty() {
        println!(
            "[+] HLE bindings for symbols this game does not import: {}",
            missing_imports.len()
        );
    }
    println!(
        "[+] Successfully bound core libc, nn::os, math constants, and NVN entrypoints into GOT."
    );

    // 4. x18 (cpu/trampoline.rs): macOS keeps x18 for this process (linked against a macOS SDK < 13, .cargo/config.toml);
    //    where the kernel does not (iOS, the simulator, a macOS build without that link flag) every x18 use of main
    //    and the SDK is rewritten into an island of stubs that keep the value in a TSD slot. Before the SDK
    //    interposition and the tpidr rewrite, so `mrs x18, tpidr_el0` sites are handled by the x18 stubs.
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
            unsafe {
                libc::mprotect(
                    sdk_mod.text_start as *mut libc::c_void,
                    pages,
                    libc::PROT_READ | libc::PROT_WRITE,
                )
            };
            unsafe {
                cpu::trampoline::rewrite_module("sdk", sdk_mod.text_start, sdk_mod.text_size)
            }
            .map_err(|e| format!("x18: {e}"))?;
            unsafe { cpu::aot::make_exec("sdk.text", sdk_mod.text_start, pages) };
        }
    } else {
        println!("[+] x18: preserved by the kernel for this process; guest x18 uses run unchanged");
    }

    // 4a. SDK interposition: calls made inside the SDK to its own functions bypass the game's GOT, so
    //     sdk module code would run the kernel/IPC-backed implementations (and e.g. FlushDataCache
    //     executes cache instructions Darwin traps with SIGILL). Mirror every HLE-bound import onto
    //     the SDK's own definition of the same symbol.
    if let Some(sdk_mod) = sdk.as_ref() {
        let default_stub = mem_space.default_stub_ptr;
        let mut interposed = 0usize;
        let list_imports = crate::settings::get().log.list_hle_imports;
        {
            for (off, name) in mem_space.imports() {
                let target = unsafe { *(mem_space.base_ptr.add(off) as *const usize) };
                // --list-hle-imports: report every game import not bound to the SDK's own definition.
                if list_imports && !sdk_mod.contains(target) {
                    let how = if target == 0 {
                        "null"
                    } else if target == default_stub {
                        "default stub"
                    } else {
                        "HLE"
                    };
                    println!("[import] {} {}", how, name);
                }
                if target == 0 || target == default_stub || sdk_mod.contains(target) {
                    continue; // unresolved / default stub / already sdk module code
                }
                if sdk_mod.interpose(&name, target) {
                    interposed += 1;
                }
            }
        }
        for (name, f) in horizon::nn::SDK_INTERPOSE {
            if sdk_mod.interpose(name, *f as usize) {
                interposed += 1;
            }
        }
        // nn::codec hardware Opus decoder: the SDK's C++ HardwareOpusDecoder class (Kirby) calls the C API inside the SDK,
        // which opens the hwopus service over IPC (not emulated; Kirby aborted when its music started). Interpose the
        // C API onto the HLE decoder, which fills HardwareOpusDecoderType like the real one (the class keeps it at this+8
        // and its short methods, thunks onto the C API or field reads, run natively), plus the class Finalize, which
        // releases the service object through its vtable.
        let opus: [(&str, usize); 7] = [
            ("_ZN2nn5codec29InitializeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeEiiPvmi", crate::hle!(horizon::hle::hle_nn_codec_InitializeHardwareOpusDecoder(_, _, _, _, _, _)) as usize),
            ("_ZN2nn5codec27FinalizeHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", crate::hle!(horizon::hle::hle_nn_codec_FinalizeHardwareOpusDecoder(_)) as usize),
            ("_ZN2nn5codec24ResetHardwareOpusDecoderEPNS0_23HardwareOpusDecoderTypeE", crate::hle!(horizon::hle::hle_nn_codec_ResetHardwareOpusDecoder(_)) as usize),
            ("_ZN2nn5codec33DecodeOpusInterleavedWithHardwareEPNS0_23HardwareOpusDecoderTypeEPmPiPsmPKvm", crate::hle!(horizon::hle::hle_nn_codec_DecodeOpusInterleavedWithHardware(_, _, _, _, _, _, _)) as usize),
            ("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEii", crate::hle!(horizon::hle::hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize2(_, _)) as usize),
            ("_ZN2nn5codec36GetHardwareOpusDecoderWorkBufferSizeEiii", crate::hle!(horizon::hle::hle_nn_codec_GetHardwareOpusDecoderWorkBufferSize(_, _, _)) as usize),
            ("_ZN2nn5codec19HardwareOpusDecoder8FinalizeEv", crate::hle!(horizon::hle::hle_nn_codec_HardwareOpusDecoder_Finalize(_)) as usize),
        ];
        for (name, f) in opus {
            if sdk_mod.interpose(name, f) {
                interposed += 1;
            }
        }
        // nn::ssl::Initialize: called from inside the SDK (its own shim), so the game's import binding alone misses it.
        if sdk_mod.interpose(
            "_ZN2nn3ssl10InitializeEv",
            crate::hle!(horizon::nn::ssl_ok()) as usize,
        ) {
            interposed += 1;
        }
        // nn::settings::detail::MakeLanguageCode: the SDK would IPC to the settings service (see hle_nn).
        let tramp = mem_space
            .make_hle_trampoline(crate::hle!(horizon::nn::settings_make_language_code(_, _)))
            as usize;
        if sdk_mod.interpose(
            "_ZN2nn8settings6detail16MakeLanguageCodeEPNS0_12LanguageCodeENS0_8LanguageE",
            tramp,
        ) {
            interposed += 1;
        }
        let apm: [(&str, usize); 4] = [
            (
                "_ZN2nn2oe27GetPerformanceConfigurationENS0_15PerformanceModeE",
                mem_space.make_hle_trampoline(
                    crate::hle!(horizon::hle::hle_nn_oe_GetPerformanceConfiguration(_)),
                ) as usize,
            ),
            (
                "_ZN2nn3apm27GetPerformanceConfigurationENS0_15PerformanceModeE",
                mem_space.make_hle_trampoline(
                    crate::hle!(horizon::hle::hle_nn_oe_GetPerformanceConfiguration(_)),
                ) as usize,
            ),
            (
                "_ZN2nn2oe18GetPerformanceModeEv",
                mem_space.make_hle_trampoline(
                    crate::hle!(horizon::hle::hle_nn_oe_GetPerformanceMode()),
                ) as usize,
            ),
            (
                "_ZN2nn3apm18GetPerformanceModeEv",
                mem_space.make_hle_trampoline(
                    crate::hle!(horizon::hle::hle_nn_oe_GetPerformanceMode()),
                ) as usize,
            ),
        ];
        for (name, tramp) in apm {
            if sdk_mod.interpose(name, tramp) {
                interposed += 1;
            }
        }
        // The SDK's internal critical section / condition variable (behind SdkMutex & co.) use Horizon
        // kernel calls when contended; see horizon/sync.rs.
        for (name, f) in horizon::sync::interpositions() {
            // These can block, so they go through an HLE trampoline (restores x18 / tpidr_el0 on return).
            let tramp = mem_space.make_hle_trampoline(f as *const ()) as usize;
            if sdk_mod.interpose(name, tramp) {
                interposed += 1;
            } else {
                eprintln!("[!] SDK interposition: {} not found", name);
            }
        }
        println!("[+] SDK interposition: {} SDK functions now route to the same HLE as the game's imports", interposed);
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
    //       a few runtime services (guest::module_loader::runtime_overrides). QueryMemory (horizon_svc) presents the guest
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
            let slot = guest::loader::rtld_slot();
            let p = if slot != 0 {
                libc::mmap(
                    slot as *mut libc::c_void,
                    0x4000,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_FIXED | libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            } else {
                libc::mmap(
                    (main_base - 0x10_0000) as *mut libc::c_void,
                    0x4000,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            if p != libc::MAP_FAILED && (p as usize) < main_base {
                *(p as *mut u32) = 0xD65F03C0; // ret
                cpu::aot::make_exec("rtld.placeholder", p as usize, 0x4000);
                // text, rodata and data like a real module (exlaunch reads the two regions after each text).
                add_region(Region {
                    start: p as usize,
                    size: 0x1000,
                    mem_type: MEM_CODE_STATIC,
                    perm: PERM_RX,
                });
                add_region(Region {
                    start: p as usize + 0x1000,
                    size: 0x1000,
                    mem_type: MEM_CODE_STATIC,
                    perm: PERM_R,
                });
                add_region(Region {
                    start: p as usize + 0x2000,
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
        for (name, f) in guest::module_loader::runtime_overrides() {
            syms.insert(name.to_string(), mem_space.make_hle_trampoline(f) as usize);
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

/// Runs a game. This host has no runtime yet: guest code runs natively only on Apple Silicon, and the lifted runtime
/// for other hosts is not written yet (docs/windows-port.md).
#[cfg(not(nx_native))]
pub fn run(_layers: Vec<settings::SettingsPatch>) -> Result<(), Box<dyn std::error::Error>> {
    Err("nxport cannot run games on this host yet (see docs/windows-port.md)".into())
}
