// Direct In-Place Memory Loader, Relocation Engine, & GOT Linker for the game's main module (layout and dynamic
// tables come from the NSO, see guest/nso.rs; --game-dir picks the dump, see game.rs).

use std::ptr;

use crate::guest::sdk_loader::SdkModule;
use crate::horizon::hle;

/// Distance from main's end to the SDK (rtld's order is main, subsdk*, sdk; mods address the SDK as the last module).
pub const SDK_GAP: usize = 0x0200_0000;
/// AArch64 relocation types of GOT entries.
pub const GLOB_DAT: u32 = 1025;
pub const JUMP_SLOT: u32 = 1026;
pub const SDK_SLOT_SIZE: usize = 0x0100_0000;
pub const ISLAND_SLOT_SIZE: usize = 0x0010_0000;
/// Generated-code area at the end of the module gap (just below the SDK slot): code the engine writes (HLE trampolines,
/// the SDK's short-function island) and the literal pool that code loads per-run addresses from (cpu::aot::literal). At a
/// fixed distance from main and the SDK, so the generated code is the same on every launch.
pub const GEN_SIZE: usize = 0x0010_0000;
/// Guest base for AOT images with mods (GuestMemorySpace::allocate). It must be free in both address spaces when the
/// reservation (main, the module gap, the SDK: ~150 MB) is made, first thing in main. Both kernels start an address
/// range at 0x70_0000_0000 (macOS pushes hints just below it up to it; an iPhone on iOS 27 ignored a hint at
/// 0x4_0000_0000 and put the reservation at 0x70_0000_0000, 2026-10-11): the rtld placeholder slot (RTLD_SLOT_SIZE)
/// takes the range's first MB and main follows.
pub const FIXED_GUEST_BASE: usize = 0x70_0010_0000;
/// The slot right below main for the placeholder rtld module mods expect (main.rs), reserved with a fixed guest base.
pub const RTLD_SLOT_SIZE: usize = 0x10_0000;
static MAIN_BASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Where main's image starts (guest addresses in logs are main-relative), or 0 before it is allocated.
pub fn main_base() -> usize {
    MAIN_BASE.load(std::sync::atomic::Ordering::Relaxed)
}
static RTLD_SLOT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// The reserved rtld placeholder slot (PROT_NONE until main.rs maps it), or 0.
pub fn rtld_slot() -> usize {
    RTLD_SLOT.load(std::sync::atomic::Ordering::Relaxed)
}
/// Offsets in the generated-code area.
pub const GEN_SHORT_ISLAND: usize = 0;
pub const GEN_LITERALS: usize = 0x4000;
pub const GEN_LITERALS_SIZE: usize = 0xC000;
pub const GEN_TRAMPOLINES: usize = 0x10000;
/// Trampoline literals (rust_fn per entry; entry 0's second word: the x18 value), 16 bytes each.
pub const GEN_TRAMPOLINE_LITERALS: usize = 0x20000;
/// HLE trampoline entries: 32 bytes each (room for a mod's 16-byte hook patch), 2048 of them.
const TRAMPOLINE_WORDS: usize = 8;
const TRAMPOLINE_CAPACITY: usize = 2048;
/// Trampoline pool entries taken by the shared HLE entry stub (22 words).
const HLE_STUB_ENTRIES: usize = 3;

/// x18 stub island (cpu/trampoline.rs) just below the generated-code area: within branch range of main (TotK's main text
/// is ~45 MB) and the SDK, at a fixed distance from both. TotK main's ~31k x18 sites need ~1.3 MB.
pub const X18_ISLAND_SIZE: usize = 0x0080_0000;

static GEN_AREA: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub fn x18_island_area() -> usize {
    match gen_area() {
        0 => 0,
        g => g - X18_ISLAND_SIZE,
    }
}
pub fn gen_area() -> usize {
    GEN_AREA.load(std::sync::atomic::Ordering::Relaxed)
}

/// The reserved range between main (+ island slot) and the SDK slot that subsdk modules are mapped into.
static MOD_GAP: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MOD_GAP_END: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Size of main's .text (read by the crash handler, which runs in signal context).
static MAIN_TEXT_SIZE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub fn mod_gap() -> (usize, usize) {
    (
        MOD_GAP.load(std::sync::atomic::Ordering::Relaxed),
        MOD_GAP_END.load(std::sync::atomic::Ordering::Relaxed),
    )
}

pub struct GuestMemorySpace {
    pub base_ptr: *mut u8,
    /// Reserved with main, within branch range of main and the SDK: the tpidr rewrite island (ISLAND_SLOT_SIZE) and
    /// the SDK image (SDK_SLOT_SIZE). Hinted mmaps used to land elsewhere when the spot was taken.
    pub island_slot: usize,
    pub sdk_slot: usize,
    /// (start, size) of the address space reserved for all of the above (crate::os::reserve).
    reservation: (usize, usize),
    pub total_size: usize,
    pub text_size: usize,
    pub rodata_size: usize,
    pub data_size: usize,
    pub bss_size: usize,
    /// Image offsets of .rodata / .data / .bss.
    pub ro_off: usize,
    pub data_off: usize,
    pub bss_off: usize,
    /// Dynamic-linking tables of main (image offsets).
    pub dynamic: crate::guest::nso::Dynamic,
    pub build_id: [u8; 32],
    /// The decompressed segments until load_sections copies them in.
    nso: Option<crate::guest::nso::Nso>,
    /// (symbol, relocation type) -> GOT slots, built on first use (got_slots).
    got_map: std::cell::RefCell<std::collections::HashMap<(String, u32), Vec<usize>>>,
    // Trampoline pool: mmap'd region of x18- and tpidr_el0-restoring HLE call stubs.
    // Each entry is 16 words (64 bytes). See make_hle_trampoline().
    trampoline_pool: *mut u32,
    trampoline_next: usize,
    /// shared_hle_trampoline's trampolines by function.
    trampolines: std::collections::HashMap<usize, usize>,
    trampoline_capacity: usize,
    pub guest_x9_value: usize,
    pub default_stub_ptr: usize,
}

impl GuestMemorySpace {
    pub fn allocate() -> Result<Self, Box<dyn std::error::Error>> {
        if crate::game::is_32bit_of(crate::game::dir()) {
            return Err("this game is a 32-bit program: Apple CPUs cannot run 32-bit ARM code, so it is not supported".into());
        }
        let path = crate::game::exefs_file("main");
        let nso = crate::guest::nso::Nso::read(&path)?;
        let dynamic = nso.dynamic()?;
        println!("[+] Main NSO {}: text {:#x}+{:#x}, rodata {:#x}+{:#x}, data {:#x}+{:#x}, bss {:#x}+{:#x}; {:x?}",
            path.display(), nso.text.mem_off, nso.text.data.len(), nso.ro.mem_off, nso.ro.data.len(),
            nso.data.mem_off, nso.data.data.len(), nso.bss_off(), nso.bss_size, dynamic);
        let total_size = nso.image_size();
        MAIN_TEXT_SIZE.store(nso.text.data.len(), std::sync::atomic::Ordering::Relaxed);
        // One contiguous reservation: rtld slot | main | island slot | gap for subsdk modules (released below) | SDK slot.
        // Slots start on host (16 KiB) pages: main's size is not a multiple of one, and mprotect needs page alignment.
        let main_end = (total_size + 0x3fff) & !0x3fff;
        let reserve = main_end + SDK_GAP + SDK_SLOT_SIZE;
        let total_reserve = RTLD_SLOT_SIZE + reserve;

        // AOT images with mods (cpu/aot.rs): exlaunch plugins write absolute addresses (trampoline pointers) into their
        // code, so the guest must sit at the same address in the run that dumps the image (macOS) and the runs that load
        // it (iOS). A fixed spot both address spaces leave free; any other run lets the kernel choose.
        let fixed = (crate::cpu::aot::active()
            && crate::horizon::fs::mods().iter().any(|(_, d)| {
                (0..10).any(|i| d.join("exefs").join(format!("subsdk{}", i)).is_file())
            }))
            || crate::settings::get().guest.fixed_guest_base;

        let expected_base = FIXED_GUEST_BASE - RTLD_SLOT_SIZE;
        let at = if fixed { crate::os::At::Exactly(expected_base) } else { crate::os::At::Anywhere };
        let slot = match unsafe { crate::os::reserve(at, total_reserve) } {
            Ok(p) => p,
            Err(e) if fixed => {
                eprintln!("[-] the guest needs {:#x} (AOT image with mods): {}", expected_base, e);
                return Err("guest address space: fixed base unavailable".into());
            }
            Err(e) => {
                eprintln!("[-] reserving the guest address space failed: {}", e);
                return Err("Failed to allocate guest virtual memory space".into());
            }
        };
        let ptr = slot + RTLD_SLOT_SIZE;
        // The whole range stays reserved; what is usable now is committed (fresh pages read as zero):
        //   rtld slot   reserved until the placeholder module is mapped into it (lib.rs)
        //   main        main's image, then the island slot
        //   gap         reserved until module_loader maps the subsdk modules into it. It used to be unmapped for hinted
        //               mmaps; when another allocation took the spot first, the game's subsdk0 landed a few MB higher
        //               and UltraCam's patcher wrote main's patches through a view of the wrong module (intermittent
        //               SIGBUS at startup).
        //   x18 island, generated code, the SDK slot
        RTLD_SLOT.store(slot, std::sync::atomic::Ordering::Relaxed);
        MAIN_BASE.store(ptr, std::sync::atomic::Ordering::Relaxed);
        let gap = ptr + main_end + ISLAND_SLOT_SIZE;
        let gen = ptr + main_end + SDK_GAP - GEN_SIZE;
        let after_gap = gen - X18_ISLAND_SIZE;
        let committed = unsafe {
            crate::os::commit(ptr, gap - ptr, crate::os::Prot::ReadWrite).and_then(|()| {
                crate::os::commit(after_gap, slot + total_reserve - after_gap, crate::os::Prot::ReadWrite)
            })
        };
        if let Err(e) = committed {
            unsafe { crate::os::release(slot, total_reserve) };
            eprintln!("[-] committing guest memory failed: {}", e);
            return Err("Failed to allocate guest virtual memory space".into());
        }
        GEN_AREA.store(gen, std::sync::atomic::Ordering::Relaxed);
        crate::cpu::aot::set_literal_pool(gen + GEN_LITERALS, GEN_LITERALS_SIZE);
        MOD_GAP.store(gap, std::sync::atomic::Ordering::Relaxed);
        MOD_GAP_END.store(after_gap, std::sync::atomic::Ordering::Relaxed);

        Ok(Self {
            base_ptr: ptr as *mut u8,
            island_slot: ptr + main_end,
            sdk_slot: ptr + main_end + SDK_GAP,
            reservation: (slot, total_reserve),
            total_size,
            text_size: nso.text.data.len(),
            rodata_size: nso.ro.data.len(),
            data_size: nso.data.data.len(),
            bss_size: nso.bss_size,
            ro_off: nso.ro.mem_off,
            data_off: nso.data.mem_off,
            bss_off: nso.bss_off(),
            dynamic,
            build_id: nso.build_id,
            nso: Some(nso),
            got_map: Default::default(),
            trampoline_pool: ptr::null_mut(),
            trampoline_next: 0,
            trampolines: std::collections::HashMap::new(),
            trampoline_capacity: 0,
            guest_x9_value: 0,
            default_stub_ptr: 0,
        })
    }

    pub fn load_sections(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        println!(
            "[+] Loading decompressed sections into guest memory space at {:p}...",
            self.base_ptr
        );
        let nso = self.nso.take().ok_or("main sections already loaded")?;
        for (name, seg) in [
            (".text", &nso.text),
            (".rodata", &nso.ro),
            (".data", &nso.data),
        ] {
            unsafe {
                ptr::copy_nonoverlapping(
                    seg.data.as_ptr(),
                    self.base_ptr.add(seg.mem_off),
                    seg.data.len(),
                )
            };
            println!(
                "    - {:8} loaded : {:#010x} ({} bytes)",
                name,
                seg.mem_off,
                seg.data.len()
            );
        }
        unsafe { ptr::write_bytes(self.base_ptr.add(self.bss_off), 0, self.bss_size) };
        println!(
            "    - .bss     cleared: {:#010x} ({} bytes)",
            self.bss_off, self.bss_size
        );
        Ok(())
    }

    /// The RELA entries of .rela.dyn and .rela.plt as (r_offset, r_info, r_addend), copied out first: some entries
    /// target .rodata, where the tables live.
    fn rela_entries(&self) -> Vec<(usize, u64, i64)> {
        let d = &self.dynamic;
        let mut tables = vec![(d.rela, d.relasz)];
        if d.jmprel != 0 && !(d.jmprel >= d.rela && d.jmprel < d.rela + d.relasz) {
            tables.push((d.jmprel, d.pltrelsz));
        }
        let mut v = Vec::new();
        for (off, size) in tables {
            let p = unsafe { self.base_ptr.add(off) as *const u64 };
            for i in 0..size / 24 {
                unsafe {
                    v.push((
                        *p.add(i * 3) as usize,
                        *p.add(i * 3 + 1),
                        *p.add(i * 3 + 2) as i64,
                    ))
                };
            }
        }
        v
    }

    pub fn apply_relocations(&mut self, sdk: Option<&SdkModule>) -> usize {
        let base = self.base_ptr as usize;
        let d = self.dynamic;
        // Linker-defined table bounds, also referenced by name (__rel_plt_start etc.).
        let (plt_start, plt_end) = (base + d.jmprel, base + d.jmprel + d.pltrelsz);
        let (dyn_start, dyn_end) = (base + d.rela, base + d.rela + d.relasz);
        let mut applied = 0;
        let mut entries: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        for (r_offset, r_info, r_addend) in self.rela_entries() {
            let r_type = (r_info & 0xFFFFFFFF) as u32;
            let r_sym = (r_info >> 32) as usize;
            let resolved: Option<i64> = match r_type {
                // R_AARCH64_RELATIVE: *target = base + addend
                1027 => Some(base as i64 + r_addend),
                // R_AARCH64_ABS64 / R_AARCH64_GLOB_DAT / R_AARCH64_JUMP_SLOT: *target = S + addend,
                // where S is the resolved address of the entry's symbol.
                257 | 1025 | 1026 => unsafe {
                    self.resolve_dynsym(r_sym, base, plt_start, plt_end, dyn_start, dyn_end, sdk)
                }
                .map(|(s, engine_fn)| {
                    // Engine functions are entered through a trampoline like every other HLE call.
                    let s = if engine_fn {
                        match entries.get(&s) {
                            Some(&t) => t,
                            None => {
                                let t = self.shared_hle_trampoline(s as *const ()) as usize;
                                entries.insert(s, t);
                                t
                            }
                        }
                    } else {
                        s
                    };
                    s as i64 + r_addend
                }),
                _ => None,
            };
            if let Some(value) = resolved {
                unsafe { ptr::write(self.base_ptr.add(r_offset) as *mut usize, value as usize) };
                applied += 1;
            }
        }
        applied
    }

    /// Dynamic symbol `idx`: (name, st_info, st_shndx, st_value).
    unsafe fn dynsym(&self, idx: usize) -> (String, u8, u16, usize) {
        let sym = self.base_ptr.add(self.dynamic.symtab + idx * 24);
        let st_name = ptr::read(sym as *const u32) as usize;
        let n = std::ffi::CStr::from_ptr(
            self.base_ptr.add(self.dynamic.strtab + st_name) as *const std::os::raw::c_char
        )
        .to_string_lossy()
        .into_owned();
        (
            n,
            ptr::read(sym.add(4)),
            ptr::read(sym.add(6) as *const u16),
            ptr::read(sym.add(8) as *const u64) as usize,
        )
    }

    /// Number of dynamic symbols (.dynsym sits right before .dynstr, as in every NSO seen).
    fn dynsym_count(&self) -> usize {
        self.dynamic.strtab.saturating_sub(self.dynamic.symtab) / 24
    }

    /// Address of a symbol main defines (its dynamic symbol table), e.g. nnMain for the SDK's startup code.
    pub fn export(&self, name: &str) -> Option<usize> {
        (1..self.dynsym_count()).find_map(|i| {
            let (n, _, shndx, value) = unsafe { self.dynsym(i) };
            (shndx != 0 && n == name).then_some(self.base_ptr as usize + value)
        })
    }

    /// GOT slots (image offsets) of main's relocations of `kind` (GLOB_DAT / JUMP_SLOT) naming `name`.
    pub fn got_slots(&self, name: &str, kind: u32) -> Vec<usize> {
        if self.got_map.borrow().is_empty() {
            let mut m: std::collections::HashMap<(String, u32), Vec<usize>> =
                std::collections::HashMap::new();
            for (off, info, _) in self.rela_entries() {
                let ty = (info & 0xffff_ffff) as u32;
                if ty == GLOB_DAT || ty == JUMP_SLOT || ty == 257 {
                    let (n, ..) = unsafe { self.dynsym((info >> 32) as usize) };
                    m.entry((n, ty)).or_default().push(off);
                }
            }
            *self.got_map.borrow_mut() = m;
        }
        self.got_map
            .borrow()
            .get(&(name.to_string(), kind))
            .cloned()
            .unwrap_or_default()
    }

    /// Main's function imports: (GOT slot offset, symbol name) of every JUMP_SLOT entry naming an undefined symbol.
    pub fn imports(&self) -> Vec<(usize, String)> {
        self.rela_entries()
            .into_iter()
            .filter(|(_, info, _)| (info & 0xffff_ffff) as u32 == 1026)
            .filter_map(|(off, info, _)| {
                let (n, _, shndx, _) = unsafe { self.dynsym((info >> 32) as usize) };
                (shndx == 0 && !n.is_empty()).then_some((off, n))
            })
            .collect()
    }

    /// Names this module exports (defined dynamic symbols) and, for its imports, the address each GOT slot finally
    /// holds (after relocation and the engine's HLE bindings): what a mod loaded later should get for the same name.
    pub fn symbol_targets(&self) -> std::collections::HashMap<String, usize> {
        let base = self.base_ptr as usize;
        let mut map = std::collections::HashMap::new();
        for idx in 1..self.dynsym_count() {
            let (n, _, shndx, value) = unsafe { self.dynsym(idx) };
            if shndx != 0 && value != 0 && !n.is_empty() {
                map.insert(n, base + value);
            }
        }
        // GLOB_DAT / JUMP_SLOT entries of .rela.dyn + .rela.plt.
        for (r_offset, info, addend) in self.rela_entries() {
            let ty = (info & 0xffff_ffff) as u32;
            if ty != 1025 && ty != 1026 {
                continue;
            }
            let (n, _, shndx, _) = unsafe { self.dynsym((info >> 32) as usize) };
            if shndx != 0 || n.is_empty() {
                continue;
            }
            let v = unsafe { ptr::read(self.base_ptr.add(r_offset) as *const usize) };
            // Imports bound to the shared default stub are unresolved, not targets (a mod hooking one would hook
            // every unimplemented function): leave them to the SDK's exports.
            if v != 0 && (v as i64 - addend) as usize != self.default_stub_ptr {
                map.entry(n).or_insert((v as i64 - addend) as usize);
            }
        }
        map
    }

    /// Resolves dynamic symbol table entry `idx` to an absolute host-callable
    /// address, and whether that is an engine function. Locally-defined symbols (st_shndx != 0) resolve directly to
    /// base + st_value, same as a real dynamic linker. Undefined (external)
    /// symbols are looked up by name against the small set this binary
    /// actually references -- see horizon/hle.rs section 12 for what each provides
    /// and why.
    unsafe fn resolve_dynsym(
        &self,
        idx: usize,
        base: usize,
        plt_start: usize,
        plt_end: usize,
        dyn_start: usize,
        dyn_end: usize,
        sdk: Option<&SdkModule>,
    ) -> Option<(usize, bool)> {
        let (name, st_info, st_shndx, st_value) = self.dynsym(idx);
        if st_shndx != 0 {
            // Defined within this module.
            return Some((base + st_value, false));
        }
        // Undefined: resolve by name.

        // The module's own relocation table bounds.
        let marker = match name.as_str() {
            "__rel_plt_start" => Some(plt_start),
            "__rel_plt_end" => Some(plt_end),
            "__rel_dyn_start" => Some(dyn_start),
            "__rel_dyn_end" => Some(dyn_end),
            _ => None,
        };
        if let Some(a) = marker {
            return Some((a, false));
        }
        match crate::horizon::sdk::lookup(&name) {
            Some(crate::horizon::sdk::Target::Func(f)) => return Some((f, true)),
            Some(crate::horizon::sdk::Target::Data(a)) => return Some((a, false)),
            None => {}
        }
        // glslc* are WEAK imports (STB_WEAK): retail has no glslc, so they stay null and nn::gfx's
        // GlslcDll::Initialize() fails exactly as on hardware.
        if let Some(sdk_mod) = sdk {
            if let Some(&addr) = sdk_mod.symbols.get::<str>(name.as_ref()) {
                return Some((addr, false));
            }
        }
        if (st_info >> 4) == 2 {
            // STB_WEAK undefined: null, per the ELF rules the game was linked under.
            println!("[+] Weak undefined symbol '{}' -> null", name);
            return Some((0, false));
        }
        println!("[!] Unresolved external relocation symbol '{}' -> binding to default HLE stub", name);
        Some(if self.default_stub_ptr != 0 {
            (self.default_stub_ptr, false)
        } else {
            (crate::hle!(hle::hle_default_stub()) as usize, true)
        })
    }

    /// Set up the pool of HLE trampolines (in the generated-code area) and record the value that x18 should have on
    /// every HLE return (the guest x18 context block).
    pub fn init_trampoline_pool(&mut self, x18_value: usize) -> Result<(), &'static str> {
        let capacity = TRAMPOLINE_CAPACITY;
        let gen = gen_area();
        if gen == 0 {
            return Err("generated-code area not allocated");
        }
        self.trampoline_pool = (gen + GEN_TRAMPOLINES) as *mut u32;
        self.trampoline_capacity = capacity;
        self.guest_x9_value = x18_value;
        // The arm64 entry code the trampolines branch to. A lifted runtime never runs trampolines: it recognises their
        // addresses and calls the function's adapter (hle_abi).
        #[cfg(nx_native)]
        unsafe {
            self.write_hle_entry_stub()
        };
        self.trampoline_next = HLE_STUB_ENTRIES;
        Ok(())
    }

    /// The code every HLE trampoline branches to, in the pool's first entries (x16 = the Rust function; the caller's
    /// x29 / x30 are pushed). On a thread whose guest code runs on its own stack (horizon/thread_stack.rs) the call
    /// runs on the thread's host stack, as Horizon's kernel calls do not use the caller's stack: our frames are far
    /// larger than the SDK's and overwrote memory the game still reads below its stack pointer. The TSD slot holding
    /// that stack is cleared for the call, so HLE calls from guest callbacks stay where they are. x29 is left alone:
    /// HLE code walks the guest's frame chain from it. Afterwards it restores the guest's tpidr_el0 and x18.
    ///   mov  x9, sp                  ; guest sp
    ///   mrs  x10, tpidrro_el0
    ///   ldr  x11, [x10, #hle stack]
    ///   cbz  x11, nested
    ///   str  xzr, [x10, #hle stack]
    ///   sub  x12, x11, #16
    ///   stp  x9, x11, [x12]          ; guest sp, stack top
    ///   mov  sp, x12
    ///   blr  x16
    ///   ldp  x9, x11, [sp]
    ///   mov  sp, x9
    ///   mrs  x10, tpidrro_el0
    ///   str  x11, [x10, #hle stack]
    ///   b    restore
    /// nested:
    ///   blr  x16
    /// restore:
    ///   mrs  x16, tpidrro_el0
    ///   ldr  x16, [x16, #guest TLS]   ; cpu::tls::guest_tls_offset
    ///   cbz  x16, #8
    ///   msr  tpidr_el0, x16
    ///   ldr  x18, <x18 value>         ; literal of entry 0
    ///   ldp  x29, x30, [sp], #16
    ///   ret                           ; x0..x7 / v0..v7 return values intact
    #[cfg(nx_native)]
    unsafe fn write_hle_entry_stub(&mut self) {
        let lit = gen_area() + GEN_TRAMPOLINE_LITERALS;
        ptr::write((lit + 8) as *mut usize, self.guest_x9_value);
        let slot = |off: usize| ((off / 8) as u32) << 10;
        let hle = slot(crate::cpu::tls::hle_stack_offset());
        let tls = slot(crate::cpu::tls::guest_tls_offset());
        ptr::write((lit + 16) as *mut usize, crate::horizon::cores::entry() as usize);
        let code: [u32; 23] = [
            0x9100_03E9,             // mov x9, sp
            0xD53B_D06A,             // mrs x10, tpidrro_el0
            0xF940_014B | hle,       // ldr x11, [x10, #hle]
            0xB400_018B,             // cbz x11, +48
            0xF900_015F | hle,       // str xzr, [x10, #hle]
            0xD100_416C,             // sub x12, x11, #16
            0xA900_2D89,             // stp x9, x11, [x12]
            0x9100_019F,             // mov sp, x12
            0,                       // ldr x17, <entry glue>  (cores::entry: x16 is the function)
            0xD63F_0220,             // blr x17
            0xA940_2FE9,             // ldp x9, x11, [sp]
            0x9100_013F,             // mov sp, x9
            0xD53B_D06A,             // mrs x10, tpidrro_el0
            0xF900_014B | hle,       // str x11, [x10, #hle]
            0x1400_0002,             // b +8
            0xD63F_0200,             // blr x16
            0xD53B_D070,             // mrs x16, tpidrro_el0
            0xF940_0210 | tls,       // ldr x16, [x16, #tls]
            0xB400_0050,             // cbz x16, +8
            0xD51B_D050,             // msr tpidr_el0, x16
            0,                       // ldr x18, <literal>
            0xA8C1_7BFD,             // ldp x29, x30, [sp], #16
            0xD65F_03C0,             // ret
        ];
        let base = self.trampoline_pool;
        for (k, w) in code.iter().enumerate() {
            ptr::write(base.add(k), *w);
        }
        ptr::write(base.add(8), crate::cpu::aot::ldr_literal(base.add(8) as usize, lit + 16, 17));
        ptr::write(base.add(20), crate::cpu::aot::ldr_literal(base.add(20) as usize, lit + 8, 18));
        for k in code.len()..HLE_STUB_ENTRIES * TRAMPOLINE_WORDS {
            ptr::write(base.add(k), 0xD420_0000u32); // brk #0
        }
    }

    /// (start, len) of the HLE trampoline pool.
    pub fn trampoline_range(&self) -> (usize, usize) {
        (self.trampoline_pool as usize, self.trampoline_capacity * TRAMPOLINE_WORDS * 4)
    }

    /// Emit one HLE trampoline for `rust_fn` and return its address. Entry i is 8 words; its literal (rust_fn) is at
    /// GEN_TRAMPOLINE_LITERALS + 16 i, in a data page, so the code holds no per-run addresses (cpu/aot.rs). The first
    /// instruction is not PC-relative: mods hook these entries by replacing it.
    ///   stp  x29, x30, [sp, #-16]!
    ///   ldr  x16, <rust_fn>
    ///   b    <entry stub>             ; write_hle_entry_stub
    /// The trampoline the registry's bindings of `rust_fn` share (horizon::sdk: imports, sdk references,
    /// interposition, mods); created once.
    pub fn shared_hle_trampoline(&mut self, rust_fn: *const ()) -> *const () {
        if let Some(&t) = self.trampolines.get(&(rust_fn as usize)) {
            return t as *const ();
        }
        let t = self.make_hle_trampoline(rust_fn);
        self.trampolines.insert(rust_fn as usize, t as usize);
        t
    }

    /// A new trampoline of its own (callers may use its spare words for code of their own).
    pub fn make_hle_trampoline(&mut self, rust_fn: *const ()) -> *const () {
        // A runtime without arm64 trampolines calls the function through its adapter (hle_abi): every function bound
        // here needs one.
        if crate::hle_abi::adapter(rust_fn).is_none() {
            eprintln!("[!] HLE: {rust_fn:p} has no adapter (take its address through hle_abi::hle)");
        }
        crate::hle_abi::bind_entry(
            unsafe { self.trampoline_pool.add(self.trampoline_next * TRAMPOLINE_WORDS) } as u64,
            rust_fn,
        );
        assert!(
            self.trampoline_next < self.trampoline_capacity,
            "trampoline pool exhausted ({} entries)",
            self.trampoline_capacity
        );
        let base = unsafe { self.trampoline_pool.add(self.trampoline_next * TRAMPOLINE_WORDS) };
        let lit = gen_area() + GEN_TRAMPOLINE_LITERALS + self.trampoline_next * 16;
        unsafe {
            ptr::write(lit as *mut usize, rust_fn as usize);
            ptr::write(base.add(0), 0xA9BF7BFDu32); // stp x29, x30, [sp, #-16]!
            ptr::write(base.add(1), crate::cpu::aot::ldr_literal(base.add(1) as usize, lit, 16));
            let back = (self.trampoline_pool as isize - base.add(2) as isize) / 4;
            ptr::write(base.add(2), 0x1400_0000 | (back as u32 & 0x03FF_FFFF)); // b <entry stub>
            for w in 3..TRAMPOLINE_WORDS {
                ptr::write(base.add(w), 0xD420_0000u32); // brk #0
            }
        }
        self.trampoline_next += 1;
        base as *const ()
    }

    /// Make the trampoline pool executable (W^X) and flush the icache.
    pub fn finalize_trampoline_pool(&self) {
        if self.trampoline_pool.is_null() {
            return;
        }
        let pool_bytes = self.trampoline_capacity * TRAMPOLINE_WORDS * 4;
        unsafe {
            crate::cpu::aot::make_exec(
                "hle.trampolines",
                self.trampoline_pool as usize,
                pool_bytes,
            );
            crate::os::flush_icache(self.trampoline_pool as usize, self.trampoline_next * TRAMPOLINE_WORDS * 4);
        }
        println!(
            "[+] Trampoline pool: {} entries ready at {:p}",
            self.trampoline_next, self.trampoline_pool
        );
    }

    pub fn init_plt_got(&mut self, default_stub: *const ()) {
        self.default_stub_ptr = default_stub as usize;
        // The GOT slots .rela.plt fills (JUMP_SLOT targets).
        let d = self.dynamic;
        let slots: Vec<usize> = (0..d.pltrelsz / 24)
            .map(|i| unsafe { *(self.base_ptr.add(d.jmprel + i * 24) as *const u64) as usize })
            .collect();
        for &slot in &slots {
            self.bind_got_symbol(slot, default_stub);
        }
        println!(
            "[+] Initialized all {} PLT GOT entries with default HLE stub.",
            slots.len()
        );
    }

    pub fn bind_got_symbol(&mut self, got_virtual_addr: usize, host_fn_ptr: *const ()) {
        unsafe {
            let got_slot = self.base_ptr.add(got_virtual_addr) as *mut usize;
            ptr::write_volatile(got_slot, host_fn_ptr as usize);
        }
    }

    pub fn protect_text_executable(&mut self) -> Result<(), &'static str> {
        let page_size = 16384;
        let aligned_text_size = (self.text_size + page_size - 1) & !(page_size - 1);

        let res = unsafe {
            crate::cpu::aot::make_exec("main.text", self.base_ptr as usize, aligned_text_size)
        };

        if res != 0 {
            return Err("Failed to mark .text as executable");
        }

        println!(
            "[+] Enforced W^X: .text marked PROT_READ | PROT_EXEC ({} bytes)",
            aligned_text_size
        );
        Ok(())
    }
}

impl Drop for GuestMemorySpace {
    fn drop(&mut self) {
        unsafe { crate::os::release(self.reservation.0, self.reservation.1) };
    }
}
