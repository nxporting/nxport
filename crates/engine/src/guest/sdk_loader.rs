// Loads the ExeFS sdk module alongside the game's main module.
//
// The sdk module is a position-independent shared library (NSO0 format) that
// TotK's RTLD loads before running the main binary.  Without it, every vtable
// slot that dispatches through a sead / nn::gfx / nn::audio function is null,
// causing immediate null-pointer crashes.
//
// We load it by:
//  1. LZ4-decompressing all three NSO sections
//  2. mmapping a ~14.5 MB region for the SDK
//  3. Applying SDK-internal R_AARCH64_RELATIVE relocations (via MOD0→DT_DYNAMIC)
//  4. Building a HashMap<name → host address> from the SDK's DYNSYM/DYNSTR
//  5. Patching x18 in SDK .text (macOS reserves x18; Switch uses it freely)
//  6. mprotecting SDK .text as PROT_READ|PROT_EXEC
//
// The caller (main.rs) passes &SdkModule to apply_relocations so that
// resolve_dynsym can resolve undefined symbols in the main binary by looking
// them up in the SDK symbol map instead of falling back to HLE stubs.

use crate::guest::loader::GuestMemorySpace;
use std::collections::HashMap;
use std::ptr;

pub struct SdkModule {
    pub base: usize,
    pub total_size: usize,
    /// SDK .text range (absolute): used to interpose HLE over SDK function entries.
    pub text_start: usize,
    pub text_size: usize,
    /// Symbol name → absolute host address in SDK memory.
    pub symbols: HashMap<String, usize>,
    /// Symbol address -> st_size from .dynsym (0 when unknown).
    pub sizes: HashMap<usize, usize>,
    /// Offsets of .rodata and .data (NSO memory offsets) and the image size (data + bss end).
    pub ro_off: usize,
    pub data_off: usize,
    pub image_size: usize,
}

impl SdkModule {
    /// True if `addr` lies inside this module's mapping.
    pub fn contains(&self, addr: usize) -> bool {
        addr >= self.base && addr < self.base + self.total_size
    }

    /// Overwrite the entry of SDK function `name` with `adrp x16; ldr x16, <literal: target>; br x16; nop`, so calls
    /// made *inside* the SDK (which bypass the game's GOT) reach the same HLE as the game's own
    /// calls. Returns false if the symbol isn't in the SDK's text.
    pub fn interpose(&self, name: &str, target: usize) -> bool {
        let Some(&addr) = self.symbols.get(name) else {
            return false;
        };
        // The long patch is 16 bytes: a shorter function (a thunk, a field setter) would have its neighbour's first
        // instructions overwritten. Kirby crashed with SIGILL in HardwareOpusDecoder::GetWorkBufferSize after the
        // 12-byte ResetHardwareOpusDecoder before it was patched. Short functions get a single `b` to a 16-byte stub in
        // a code island within branch range instead.
        if let Some(&size) = self.sizes.get(&addr) {
            if size != 0 && size < 16 {
                return unsafe { self.interpose_short(addr, target) };
            }
        }
        if addr < self.text_start || addr + 16 > self.text_start + self.text_size {
            return false;
        }
        unsafe {
            let page = addr & !0x3fff;
            let span = ((addr + 16 + 0x3fff) & !0x3fff) - page;
            libc::mprotect(
                page as *mut libc::c_void,
                span,
                libc::PROT_READ | libc::PROT_WRITE,
            );
            let p = addr as *mut u32;
            // The target comes from a literal (data), not the code: patched code must be the same on every launch
            // (AOT images, cpu/aot.rs).
            let [adrp, ldr] = crate::cpu::aot::adrp_ldr(addr, crate::cpu::aot::literal(target), 16);
            ptr::write(p, adrp);
            ptr::write(p.add(1), ldr);
            ptr::write(p.add(2), 0xD61F_0200); // br x16
            ptr::write(p.add(3), 0xD503_201F); // nop
            crate::cpu::aot::protect_rx(page, span);
            extern "C" {
                fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
            }
            sys_icache_invalidate(addr as *mut libc::c_void, 16);
        }
        true
    }
}

/// Code island for interposing short SDK functions: (base, used bytes).
static SHORT_ISLAND: std::sync::Mutex<(usize, usize)> = std::sync::Mutex::new((0, 0));
const SHORT_ISLAND_SIZE: usize = 0x4000;

impl SdkModule {
    /// The island: a page of the generated-code area (guest::loader::gen_area), just below the SDK slot, so within +-128 MiB
    /// (a `b`) of the whole SDK text and at the same distance from it on every launch.
    unsafe fn short_island(&self) -> Option<usize> {
        let mut g = SHORT_ISLAND.lock().unwrap();
        if g.0 != 0 {
            return Some(g.0);
        }
        let gen = crate::guest::loader::gen_area();
        if gen == 0 {
            return None;
        }
        *g = (gen + crate::guest::loader::GEN_SHORT_ISLAND, 0);
        Some(g.0)
    }

    unsafe fn interpose_short(&self, addr: usize, target: usize) -> bool {
        let Some(island) = self.short_island() else {
            println!("[!] SDK interposition: no code island for short functions");
            return false;
        };
        let stub = {
            let mut g = SHORT_ISLAND.lock().unwrap();
            if g.1 + 16 > SHORT_ISLAND_SIZE {
                return false;
            }
            let s = island + g.1;
            g.1 += 16;
            s
        };
        let Some(b) = crate::cpu::tpidr_rewrite::encode_b(addr, stub) else {
            return false;
        };
        // Stub: adrp x16; ldr x16, <literal: target>; br x16; nop
        let page = stub & !0x3fff;
        libc::mprotect(
            page as *mut libc::c_void,
            0x4000,
            libc::PROT_READ | libc::PROT_WRITE,
        );
        let p = stub as *mut u32;
        let [adrp, ldr] = crate::cpu::aot::adrp_ldr(stub, crate::cpu::aot::literal(target), 16);
        ptr::write(p, adrp);
        ptr::write(p.add(1), ldr);
        ptr::write(p.add(2), 0xD61F_0200);
        ptr::write(p.add(3), 0xD503_201F);
        crate::cpu::aot::make_exec("sdk.short_island", page, 0x4000);
        extern "C" {
            fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
        }
        sys_icache_invalidate(stub as *mut libc::c_void, 16);
        let fpage = addr & !0x3fff;
        libc::mprotect(
            fpage as *mut libc::c_void,
            0x4000,
            libc::PROT_READ | libc::PROT_WRITE,
        );
        ptr::write(addr as *mut u32, b);
        crate::cpu::aot::protect_rx(fpage, 0x4000);
        sys_icache_invalidate(addr as *mut libc::c_void, 4);
        true
    }
}

impl Drop for SdkModule {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.base as *mut libc::c_void, self.total_size);
        }
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

#[inline]
fn r_u32(d: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(d[off..off + 4].try_into().unwrap())
}
#[inline]
fn r_i32(d: &[u8], off: usize) -> i32 {
    i32::from_le_bytes(d[off..off + 4].try_into().unwrap())
}

fn decompress_section(
    file: &[u8],
    file_off: usize,
    comp_sz: usize,
    dec_sz: usize,
    compressed: bool,
    name: &str,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    if !compressed {
        return Ok(file[file_off..file_off + dec_sz].to_vec());
    }
    let src = &file[file_off..file_off + comp_sz];
    lz4_flex::decompress(src, dec_sz).map_err(|e| format!("LZ4 decompress .{}: {}", name, e).into())
}

unsafe extern "C" fn sdk_aligned_alloc(alignment: usize, size: usize) -> *mut libc::c_void {
    let align = alignment
        .max(std::mem::size_of::<*mut ()>())
        .next_power_of_two();
    let mut ptr: *mut libc::c_void = std::ptr::null_mut();
    if libc::posix_memalign(&mut ptr, align, size) == 0 {
        ptr
    } else {
        std::ptr::null_mut()
    }
}

unsafe extern "C" fn sdk_default_exception_stack() -> *mut libc::c_void {
    static mut STACK_BUF: [u8; 0x1000] = [0; 0x1000];
    (&raw mut STACK_BUF).cast()
}

unsafe extern "C" fn sdk_unimplemented_stub() -> i64 {
    let lr: usize;
    std::arch::asm!("mov {}, x30", out(reg) lr);
    eprintln!(
        "[!] CRITICAL: SDK called UNRESOLVED EXTERNAL STUB! Return address LR = 0x{:x}",
        lr
    );
    0
}

// ── main entry point ────────────────────────────────────────────────────────

static NVN_LIBRARY_VERSION: std::sync::OnceLock<Option<(u32, u32, u32)>> = std::sync::OnceLock::new();

/// The version of the NVN library the game's SDK module was built with, once the SDK is loaded.
pub fn nvn_library_version() -> Option<(u32, u32, u32)> {
    NVN_LIBRARY_VERSION.get().copied().flatten()
}

/// The version in an SDK middleware string such as "SDK MW+Nintendo+NintendoSDK_NVN-7_3_2-Release", by its `lib` prefix.
fn middleware_version(ro: &[u8], lib: &[u8]) -> Option<(u32, u32, u32)> {
    let tag = [b"SDK MW+Nintendo+".as_slice(), lib].concat();
    let at = ro.windows(tag.len()).position(|w| w == tag)? + tag.len();
    let end = ro[at..].iter().position(|&c| c == b'-' || c == 0)?;
    let mut parts = std::str::from_utf8(&ro[at..at + end]).ok()?.split('_').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}

pub fn load_sdk(
    sdk_path: &str,
    mem_space: &mut GuestMemorySpace,
) -> Result<SdkModule, Box<dyn std::error::Error>> {
    println!("[+] Loading SDK NSO: {}", sdk_path);

    let file = std::fs::read(sdk_path)?;

    // ── NSO header fields ───────────────────────────────────────────────────
    let flags = r_u32(&file, 0x0C);
    let text_fo = r_u32(&file, 0x10) as usize;
    let text_mo = r_u32(&file, 0x14) as usize;
    let text_ds = r_u32(&file, 0x18) as usize;
    let ro_fo = r_u32(&file, 0x20) as usize;
    let ro_mo = r_u32(&file, 0x24) as usize;
    let ro_ds = r_u32(&file, 0x28) as usize;
    let data_fo = r_u32(&file, 0x30) as usize;
    let data_mo = r_u32(&file, 0x34) as usize;
    let data_ds = r_u32(&file, 0x38) as usize;
    let bss_sz = r_u32(&file, 0x3C) as usize;
    let text_cs = r_u32(&file, 0x60) as usize;
    let ro_cs = r_u32(&file, 0x64) as usize;
    let data_cs = r_u32(&file, 0x68) as usize;
    // DynStr/DynSym offsets within .rodata memory (from NSO header 0x90-0x9C)
    let dynstr_ro = r_u32(&file, 0x90) as usize;
    let dynsym_ro = r_u32(&file, 0x98) as usize;
    let dynsym_sz = r_u32(&file, 0x9C) as usize;

    // ── Decompress ─────────────────────────────────────────────────────────
    let text_raw = decompress_section(&file, text_fo, text_cs, text_ds, flags & 1 != 0, "text")?;
    let ro_raw = decompress_section(&file, ro_fo, ro_cs, ro_ds, flags & 2 != 0, "rodata")?;
    let data_raw = decompress_section(&file, data_fo, data_cs, data_ds, flags & 4 != 0, "data")?;
    let _ = NVN_LIBRARY_VERSION.set(middleware_version(&ro_raw, b"NintendoSDK_NVN-"));
    if let Some((x, y, z)) = nvn_library_version() {
        println!("[+] SDK: NVN library {x}.{y}.{z}");
    }

    // ── Allocate SDK VM region ──────────────────────────────────────────────
    let bss_start = data_mo + data_ds;
    let total_vm = bss_start + bss_sz;
    let total_aligned = (total_vm + 0x3FFF) & !0x3FFF;

    // Above main with a gap for subsdk modules: rtld's order is main, subsdk*, sdk, and mods (exlaunch) address the
    // SDK as the last module. Close enough to main for one tpidr rewrite island to reach both.
    // Its slot was reserved together with main (guest::loader::SDK_SLOT_SIZE); a hint alone could land anywhere.
    let base_ptr = if total_aligned <= crate::guest::loader::SDK_SLOT_SIZE {
        mem_space.sdk_slot as *mut libc::c_void
    } else {
        let hint =
            (mem_space.base_ptr as usize + mem_space.total_size + crate::guest::loader::SDK_GAP)
                & !0x3fff;
        unsafe {
            libc::mmap(
                hint as *mut libc::c_void,
                total_aligned,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        }
    };
    if base_ptr == libc::MAP_FAILED {
        return Err("mmap for SDK memory failed".into());
    }
    let base = base_ptr as usize;

    // ── Copy sections ───────────────────────────────────────────────────────
    unsafe {
        ptr::copy_nonoverlapping(text_raw.as_ptr(), (base + text_mo) as *mut u8, text_ds);
        ptr::copy_nonoverlapping(ro_raw.as_ptr(), (base + ro_mo) as *mut u8, ro_ds);
        ptr::copy_nonoverlapping(data_raw.as_ptr(), (base + data_mo) as *mut u8, data_ds);
        ptr::write_bytes((base + bss_start) as *mut u8, 0, bss_sz);
    }
    println!(
        "[+] SDK at 0x{:x}: text={} ro={} data={} bss={} total={}",
        base, text_ds, ro_ds, data_ds, bss_sz, total_vm
    );

    // ── Build symbol table (DynSym entries with st_shndx != 0 are defined) ──
    // DynSym and DynStr live inside .rodata, so offsets are from ro_mo.
    let dynsym_host = base + ro_mo + dynsym_ro;
    let dynstr_host = base + ro_mo + dynstr_ro;
    let sym_count = dynsym_sz / 24;

    let mut symbols: HashMap<String, usize> = HashMap::with_capacity(sym_count);
    let mut sizes: HashMap<usize, usize> = HashMap::with_capacity(sym_count);
    unsafe {
        for i in 1..sym_count {
            let sym = (dynsym_host + i * 24) as *const u8;
            let st_name = ptr::read(sym as *const u32) as usize;
            let _st_info = ptr::read(sym.add(4));
            let st_shndx = ptr::read(sym.add(6) as *const u16);
            let st_value = ptr::read(sym.add(8) as *const u64) as usize;

            if st_shndx == 0 || st_value == 0 {
                continue;
            }

            let name_ptr = (dynstr_host + st_name) as *const std::os::raw::c_char;
            let name = std::ffi::CStr::from_ptr(name_ptr)
                .to_string_lossy()
                .into_owned();
            if !name.is_empty() {
                symbols.insert(name, base + st_value);
                sizes.insert(
                    base + st_value,
                    ptr::read(sym.add(16) as *const u64) as usize,
                );
            }
        }
    }
    symbols.insert(
        "_ZN2nn2sm10InitializeEv".into(),
        mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_sm_Initialize as *const ())
            as usize,
    );
    symbols.insert(
        "_ZN2nn2sm8FinalizeEv".into(),
        mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_sm_Finalize as *const ())
            as usize,
    );
    symbols.insert(
        "_ZN2nn2sm16GetServiceHandleEPNS_3svc6HandleEPKcm".into(),
        mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_sm_GetServiceHandle as *const ())
            as usize,
    );
    symbols.insert(
        "_ZN2nn4diag6detail5AbortEPKNS_6ResultE".into(),
        mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_diag_detail_Abort as *const ())
            as usize,
    );

    let hid_symbols: &[(&str, *const ())] = &[
        ("_ZN2nn3hid15InitializeMouseEv", crate::horizon::hle::hle_nn_hid_InitializeMouse as *const ()),
        ("_ZN2nn3hid6detail15InitializeMouseEv", crate::horizon::hle::hle_nn_hid_InitializeMouse as *const ()),
        ("_ZN2nn3hid18InitializeKeyboardEv", crate::horizon::hle::hle_nn_hid_InitializeKeyboard as *const ()),
        ("_ZN2nn3hid6detail18InitializeKeyboardEv", crate::horizon::hle::hle_nn_hid_InitializeKeyboard as *const ()),
        ("_ZN2nn3hid21InitializeTouchScreenEv", crate::horizon::hle::hle_nn_hid_InitializeTouchScreen as *const ()),
        ("_ZN2nn3hid6detail21InitializeTouchScreenEv", crate::horizon::hle::hle_nn_hid_InitializeTouchScreen as *const ()),
        ("_ZN2nn3hid13GetMouseStateEPNS0_10MouseStateE", crate::horizon::hle::hle_nn_hid_GetMouseState as *const ()),
        ("_ZN2nn3hid14GetMouseStatesEPNS0_10MouseStateEi", crate::horizon::hle::hle_nn_hid_GetMouseStates as *const ()),
        ("_ZN2nn3hid6detail13GetMouseStateEPNS0_10MouseStateE", crate::horizon::hle::hle_nn_hid_GetMouseState as *const ()),
        ("_ZN2nn3hid6detail14GetMouseStatesEPiPNS0_10MouseStateEi", crate::horizon::hle::hle_nn_hid_GetMouseStates as *const ()),
        ("_ZN2nn3hid16GetKeyboardStateEPNS0_13KeyboardStateE", crate::horizon::hle::hle_nn_hid_GetKeyboardState as *const ()),
        ("_ZN2nn3hid17GetKeyboardStatesEPNS0_13KeyboardStateEi", crate::horizon::hle::hle_nn_hid_GetKeyboardStates as *const ()),
        ("_ZN2nn3hid6detail16GetKeyboardStateEPNS0_13KeyboardStateE", crate::horizon::hle::hle_nn_hid_GetKeyboardState as *const ()),
        ("_ZN2nn3hid6detail17GetKeyboardStatesEPiPNS0_13KeyboardStateEi", crate::horizon::hle::hle_nn_hid_GetKeyboardStates as *const ()),

        ("_ZN2nn2os20InitializeTimerEventEPNS0_14TimerEventTypeENS0_14EventClearModeE", crate::horizon::hle::hle_nn_os_InitializeTimerEvent as *const ()),
        ("nnosInitializeTimerEvent", crate::horizon::hle::hle_nn_os_InitializeTimerEvent as *const ()),
        ("_ZN2nn2os18FinalizeTimerEventEPNS0_14TimerEventTypeE", crate::horizon::hle::hle_nn_os_FinalizeTimerEvent as *const ()),
        ("nnosFinalizeTimerEvent", crate::horizon::hle::hle_nn_os_FinalizeTimerEvent as *const ()),
        ("_ZN2nn2os22StartOneShotTimerEventEPNS0_14TimerEventTypeENS_8TimeSpanE", crate::horizon::hle::hle_nn_os_StartOneShotTimerEvent as *const ()),
        ("nnosStartOneShotTimerEvent", crate::horizon::hle::hle_nn_os_StartOneShotTimerEvent as *const ()),
        ("_ZN2nn2os23StartPeriodicTimerEventEPNS0_14TimerEventTypeENS_8TimeSpanES3_", crate::horizon::hle::hle_nn_os_StartPeriodicTimerEvent as *const ()),
        ("nnosStartPeriodicTimerEvent", crate::horizon::hle::hle_nn_os_StartPeriodicTimerEvent as *const ()),
        ("_ZN2nn2os14StopTimerEventEPNS0_14TimerEventTypeE", crate::horizon::hle::hle_nn_os_StopTimerEvent as *const ()),
        ("nnosStopTimerEvent", crate::horizon::hle::hle_nn_os_StopTimerEvent as *const ()),
        ("_ZN2nn2os14WaitTimerEventEPNS0_14TimerEventTypeE", crate::horizon::hle::hle_nn_os_WaitTimerEvent as *const ()),
        ("nnosWaitTimerEvent", crate::horizon::hle::hle_nn_os_WaitTimerEvent as *const ()),
        ("_ZN2nn2os17TryWaitTimerEventEPNS0_14TimerEventTypeE", crate::horizon::hle::hle_nn_os_TryWaitTimerEvent as *const ()),
        ("nnosTryWaitTimerEvent", crate::horizon::hle::hle_nn_os_TryWaitTimerEvent as *const ()),
        ("_ZN2nn2os16SignalTimerEventEPNS0_14TimerEventTypeE", crate::horizon::hle::hle_nn_os_SignalTimerEvent as *const ()),
        ("nnosSignalTimerEvent", crate::horizon::hle::hle_nn_os_SignalTimerEvent as *const ()),
        ("_ZN2nn2os15ClearTimerEventEPNS0_14TimerEventTypeE", crate::horizon::hle::hle_nn_os_ClearTimerEvent as *const ()),
        ("nnosClearTimerEvent", crate::horizon::hle::hle_nn_os_ClearTimerEvent as *const ()),

        ("nnosGetCurrentThread", crate::horizon::hle::hle_nn_os_GetCurrentThread as *const ()),
        ("nnosGetThreadPriority", crate::horizon::hle::hle_nn_os_GetThreadPriority as *const ()),
        ("nnosCreateThread", crate::horizon::hle::hle_nn_os_CreateThread as *const ()),
        ("nnosDestroyThread", crate::horizon::hle::hle_nn_os_DestroyThread as *const ()),
        ("nnosGetThreadCoreMask", crate::horizon::hle::hle_nn_os_GetThreadCoreMask as *const ()),
        ("nnosSetThreadCoreMask", crate::horizon::hle::hle_nn_os_SetThreadCoreMask as *const ()),
        ("nnosSetThreadNamePointer", crate::horizon::hle::hle_nn_os_SetThreadNamePointer as *const ()),
        ("nnosInitializeMutex", crate::horizon::hle::hle_nn_os_InitializeMutex as *const ()),
        ("nnosFinalizeMutex", crate::horizon::hle::hle_nn_os_FinalizeMutex as *const ()),
        ("nnosLockMutex", crate::horizon::hle::hle_nn_os_LockMutex as *const ()),
        ("nnosUnlockMutex", crate::horizon::hle::hle_nn_os_UnlockMutex as *const ()),
        ("nnosStartThread", crate::horizon::hle::hle_nn_os_StartThread as *const ()),
        ("nnosWaitThread", crate::horizon::hle::hle_nn_os_WaitThread as *const ()),
        ("nnosSleepThread", crate::horizon::hle::hle_nn_os_SleepThread as *const ()),
        ("nnosGetSystemTick", crate::horizon::hle::hle_nn_os_GetSystemTick as *const ()),
        ("nnosGetSystemTickFrequency", crate::horizon::hle::hle_nn_os_GetSystemTickFrequency as *const ()),
        ("nnosConvertTickToNanoSeconds", crate::horizon::hle::hle_nn_os_ConvertToTimeSpan as *const ()),
        ("nnosInitializeEvent", crate::horizon::nn::os_initialize_event as *const ()),
        ("nnosFinalizeEvent", crate::horizon::nn::os_finalize_event as *const ()),
        ("nnosSignalEvent", crate::horizon::nn::os_signal_event as *const ()),
        ("nnosWaitEvent", crate::horizon::nn::os_wait_event as *const ()),

        ("_ZN2nn3hid25InitializeVibrationDeviceERKNS0_21VibrationDeviceHandleE", crate::horizon::hle::hle_nn_hid_InitializeVibrationDevice as *const ()),
        ("_ZN2nn3hid6detail25InitializeVibrationDeviceERKNS0_21VibrationDeviceHandleE", crate::horizon::hle::hle_nn_hid_InitializeVibrationDevice as *const ()),
        ("_ZN2nn3hid18SendVibrationValueERKNS0_21VibrationDeviceHandleERKNS0_14VibrationValueE", crate::horizon::hle::hle_nn_hid_SendVibrationValue as *const ()),
        ("_ZN2nn3hid6detail18SendVibrationValueERKNS0_21VibrationDeviceHandleERKNS0_14VibrationValueE", crate::horizon::hle::hle_nn_hid_SendVibrationValue as *const ()),
        ("_ZN2nn3hid19SendVibrationValuesEPKNS0_21VibrationDeviceHandleEPKNS0_14VibrationValueEi", crate::horizon::hle::hle_nn_hid_SendVibrationValues as *const ()),
        ("_ZN2nn3hid6detail19SendVibrationValuesEPKNS0_21VibrationDeviceHandleEPKNS0_14VibrationValueEi", crate::horizon::hle::hle_nn_hid_SendVibrationValues as *const ()),
        ("_ZN2nn3hid20IsVibrationPermittedEPb", crate::horizon::hle::hle_nn_hid_IsVibrationPermitted as *const ()),
        ("_ZN2nn3hid6detail20IsVibrationPermittedEPb", crate::horizon::hle::hle_nn_hid_IsVibrationPermitted as *const ()),
        ("_ZN2nn3hid25GetVibrationDeviceHandlesEPNS0_21VibrationDeviceHandleEiRKjNS_4util10BitFlagSetILi32ENS0_12NpadStyleTagEEE", crate::horizon::hle::hle_nn_hid_GetVibrationDeviceHandles as *const ()),
        ("_ZN2nn3hid22GetVibrationDeviceInfoEPNS0_19VibrationDeviceInfoERKNS0_21VibrationDeviceHandleE", crate::horizon::hle::hle_nn_hid_GetVibrationDeviceInfo as *const ()),
        ("_ZN2nn3hid6detail22GetVibrationDeviceInfoEPNS0_19VibrationDeviceInfoERKNS0_21VibrationDeviceHandleE", crate::horizon::hle::hle_nn_hid_GetVibrationDeviceInfo as *const ()),
    ];
    for &(sname, func) in hid_symbols {
        symbols.insert(sname.into(), mem_space.make_hle_trampoline(func) as usize);
    }
    println!("[+] SDK: {} exported symbols indexed", symbols.len());

    // ── Find MOD0 and apply R_AARCH64_RELATIVE relocations ─────────────────
    //
    // NSO .text layout:
    //   text[0..4]  = AArch64 instruction / placeholder
    //   text[4..8]  = u32: offset from text[0] to MOD0 struct
    //   text[8..12] = "MOD0" magic (when text[4..8] == 8)
    //   text[12..16]= i32 dynamic_offset (self-relative: from &text[12] to DT_DYNAMIC)
    //
    let mod0_off = r_u32(&text_raw, 4) as usize;

    if mod0_off + 8 > text_raw.len() || &text_raw[mod0_off..mod0_off + 4] != b"MOD0" {
        // Scan first 256 bytes as fallback
        let mut found = None;
        for off in (0..text_raw.len().min(256)).step_by(4) {
            if &text_raw[off..off + 4] == b"MOD0" {
                found = Some(off);
                break;
            }
        }
        if let Some(off) = found {
            eprintln!(
                "[!] SDK MOD0 offset field said 0x{:x}, but found MOD0 at text+0x{:x} via scan",
                mod0_off, off
            );
            apply_relocations_from_mod0(
                base,
                total_aligned,
                text_mo,
                off,
                &text_raw,
                dynsym_host,
                dynstr_host,
                mem_space,
            );
        } else {
            eprintln!("[!] SDK MOD0 not found — relying on pre-relocated SDK or symbol-only mode");
        }
    } else {
        apply_relocations_from_mod0(
            base,
            total_aligned,
            text_mo,
            mod0_off,
            &text_raw,
            dynsym_host,
            dynstr_host,
            mem_space,
        );
    }

    // x18 uses stay as they are here: macOS preserves x18 for this process, elsewhere main.rs rewrites them
    // (cpu/trampoline.rs).

    // ── mprotect SDK .text ──────────────────────────────────────────────────
    let text_pages = (text_ds + 0x3FFF) & !0x3FFF;
    let rc = unsafe { crate::cpu::aot::make_exec("sdk.text", base + text_mo, text_pages) };
    if rc != 0 {
        eprintln!(
            "[!] SDK: mprotect text failed: {} (base {:#x} text +{:#x} len {:#x})",
            std::io::Error::last_os_error(),
            base,
            text_mo,
            text_pages
        );
    } else {
        println!(
            "[+] SDK .text marked PROT_READ|PROT_EXEC ({} bytes)",
            text_pages
        );
    }

    Ok(SdkModule {
        base,
        total_size: total_aligned,
        text_start: base + text_mo,
        text_size: text_ds,
        symbols,
        sizes,
        ro_off: ro_mo,
        data_off: data_mo,
        image_size: total_vm,
    })
}

// ── helper: apply relocations once MOD0 offset is known ────────────────────

fn apply_relocations_from_mod0(
    base: usize,
    total_size: usize,
    text_mo: usize,
    mod0_off: usize,
    text_raw: &[u8],
    dynsym_host: usize,
    dynstr_host: usize,
    mem_space: &mut GuestMemorySpace,
) {
    // MOD0's fields are offsets from the MOD0 header itself (magic at +0, .dynamic offset at +4); .dynamic holds
    // standard Elf64_Dyn entries {i64 d_tag, u64 d_val}.
    let mod0_va = text_mo + mod0_off;
    let dyn_rel = r_i32(text_raw, mod0_off + 4);
    let dyn_va = (mod0_va as i64 + dyn_rel as i64) as usize;
    let dyn_addr = base + dyn_va;

    println!(
        "[+] SDK MOD0 @ text+0x{:x}, DT_DYNAMIC @ 0x{:x} (sdk+0x{:x})",
        mod0_off, dyn_addr, dyn_va
    );

    let (mut rela, mut relasz, mut jmprel, mut pltrelsz) = (None, 0usize, None, 0usize);
    let mut i = 0usize;
    while i < 4096 && dyn_va + (i + 1) * 16 <= total_size {
        let entry = dyn_addr + i * 16;
        let tag = unsafe { ptr::read_unaligned(entry as *const i64) };
        let val = unsafe { ptr::read_unaligned((entry + 8) as *const u64) } as usize;
        match tag {
            0 => break,
            2 => pltrelsz = val,
            7 => rela = Some(val),
            8 => relasz = val,
            23 => jmprel = Some(val),
            _ => {}
        }
        i += 1;
    }
    println!(
        "[+] SDK DT_DYNAMIC: {} entries, rela={:x?}+0x{:x} jmprel={:x?}+0x{:x}",
        i, rela, relasz, jmprel, pltrelsz
    );
    if rela.is_none() && jmprel.is_none() {
        eprintln!("[!] SDK: no DT_RELA / DT_JMPREL in DT_DYNAMIC");
        return;
    }

    // Copy the tables before applying them: some entries target .rodata (vtables), which would corrupt entries not
    // read yet if applied in place.
    let mut rela_bytes: Vec<u8> = Vec::new();
    for (off, size) in [(rela, relasz), (jmprel, pltrelsz)] {
        let Some(off) = off else { continue };
        if off.saturating_add(size) > total_size {
            eprintln!("[!] SDK: relocation table sdk+0x{off:x}+0x{size:x} outside the module");
            continue;
        }
        rela_bytes.extend_from_slice(unsafe { std::slice::from_raw_parts((base + off) as *const u8, size) });
    }
    let entry_count = rela_bytes.len() / 24;
    println!("[+] SDK: applying {} RELA entries", entry_count);

    let mut rel_count = 0usize;
    for j in 0..entry_count {
        let off = j * 24;
        let r_offset = u64::from_le_bytes(rela_bytes[off..off + 8].try_into().unwrap()) as usize;
        let r_info = u64::from_le_bytes(rela_bytes[off + 8..off + 16].try_into().unwrap());
        let r_addend_bits = i64::from_le_bytes(rela_bytes[off + 16..off + 24].try_into().unwrap());
        let r_type = (r_info & 0xFFFF_FFFF) as u32;
        let r_sym = (r_info >> 32) as usize;

        // Bounds check: r_offset must be a valid slot inside the SDK image
        if r_offset.wrapping_add(8) > total_size {
            continue;
        }

        match r_type {
            1027 => {
                // R_AARCH64_RELATIVE: *slot = base + addend
                unsafe {
                    ptr::write(
                        (base + r_offset) as *mut usize,
                        (base as i64 + r_addend_bits) as usize,
                    );
                }
                rel_count += 1;
            }
            257 | 1025 | 1026 => {
                // R_AARCH64_ABS64 / R_AARCH64_GLOB_DAT / R_AARCH64_JUMP_SLOT
                if r_sym > 0 {
                    let sym = (dynsym_host + r_sym * 24) as *const u8;
                    let st_shndx = unsafe { ptr::read(sym.add(6) as *const u16) };
                    let st_value = unsafe { ptr::read(sym.add(8) as *const u64) } as usize;
                    let st_name = unsafe { ptr::read(sym as *const u32) } as usize;
                    let name_ptr = (dynstr_host + st_name) as *const std::os::raw::c_char;
                    let name = unsafe { std::ffi::CStr::from_ptr(name_ptr) }.to_string_lossy();

                    let hle_override: Option<usize> = match name.as_ref() {
                        "_ZN2nn2sm10InitializeEv" => Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_sm_Initialize as *const ()) as usize),
                        "_ZN2nn2sm8FinalizeEv" => Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_sm_Finalize as *const ()) as usize),
                        "_ZN2nn2sm16GetServiceHandleEPNS_3svc6HandleEPKcm" => Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_sm_GetServiceHandle as *const ()) as usize),
                        "_ZN2nn4diag6detail5AbortEPKNS_6ResultE" => Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_diag_detail_Abort as *const ()) as usize),
                        "_ZN2nn3hid15InitializeMouseEv" | "_ZN2nn3hid6detail15InitializeMouseEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_InitializeMouse as *const ()) as usize),
                        "_ZN2nn3hid18InitializeKeyboardEv" | "_ZN2nn3hid6detail18InitializeKeyboardEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_InitializeKeyboard as *const ()) as usize),
                        "_ZN2nn3hid21InitializeTouchScreenEv" | "_ZN2nn3hid6detail21InitializeTouchScreenEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_InitializeTouchScreen as *const ()) as usize),
                        "_ZN2nn3hid13GetMouseStateEPNS0_10MouseStateE" | "_ZN2nn3hid6detail13GetMouseStateEPNS0_10MouseStateE" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetMouseState as *const ()) as usize),
                        "_ZN2nn3hid14GetMouseStatesEPNS0_10MouseStateEi" | "_ZN2nn3hid6detail14GetMouseStatesEPiPNS0_10MouseStateEi" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetMouseStates as *const ()) as usize),
                        "_ZN2nn3hid16GetKeyboardStateEPNS0_13KeyboardStateE" | "_ZN2nn3hid6detail16GetKeyboardStateEPNS0_13KeyboardStateE" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetKeyboardState as *const ()) as usize),
                        "_ZN2nn3hid17GetKeyboardStatesEPNS0_13KeyboardStateEi" | "_ZN2nn3hid6detail17GetKeyboardStatesEPiPNS0_13KeyboardStateEi" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetKeyboardStates as *const ()) as usize),
                        "_ZN2nn3hid18InitializeDebugPadEv" | "_ZN2nn3hid6detail18InitializeDebugPadEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_InitializeDebugPad as *const ()) as usize),
                        "_ZN2nn3hid16GetDebugPadStateEPNS0_13DebugPadStateE" | "_ZN2nn3hid6detail16GetDebugPadStateEPNS0_13DebugPadStateE" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetDebugPadState as *const ()) as usize),
                        "_ZN2nn4diag18IsDebuggerAttachedEv" | "_ZN2nn4diag6detail18IsDebuggerAttachedEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_diag_IsDebuggerAttached as *const ()) as usize),

                        "_ZN2nn2os20InitializeTimerEventEPNS0_14TimerEventTypeENS0_14EventClearModeE" | "nnosInitializeTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_InitializeTimerEvent as *const ()) as usize),
                        "_ZN2nn2os18FinalizeTimerEventEPNS0_14TimerEventTypeE" | "nnosFinalizeTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_FinalizeTimerEvent as *const ()) as usize),
                        "_ZN2nn2os22StartOneShotTimerEventEPNS0_14TimerEventTypeENS_8TimeSpanE" | "nnosStartOneShotTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_StartOneShotTimerEvent as *const ()) as usize),
                        "_ZN2nn2os23StartPeriodicTimerEventEPNS0_14TimerEventTypeENS_8TimeSpanES3_" | "nnosStartPeriodicTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_StartPeriodicTimerEvent as *const ()) as usize),
                        "_ZN2nn2os14StopTimerEventEPNS0_14TimerEventTypeE" | "nnosStopTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_StopTimerEvent as *const ()) as usize),
                        "_ZN2nn2os14WaitTimerEventEPNS0_14TimerEventTypeE" | "nnosWaitTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_WaitTimerEvent as *const ()) as usize),
                        "_ZN2nn2os17TryWaitTimerEventEPNS0_14TimerEventTypeE" | "nnosTryWaitTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_TryWaitTimerEvent as *const ()) as usize),
                        "_ZN2nn2os16SignalTimerEventEPNS0_14TimerEventTypeE" | "nnosSignalTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_SignalTimerEvent as *const ()) as usize),
                        "_ZN2nn2os15ClearTimerEventEPNS0_14TimerEventTypeE" | "nnosClearTimerEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_ClearTimerEvent as *const ()) as usize),

                        "_ZN2nn2os17GetThreadCoreMaskEPiPmPKNS0_10ThreadTypeE" | "nnosGetThreadCoreMask" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_GetThreadCoreMask as *const ()) as usize),
                        "_ZN2nn2os17SetThreadCoreMaskEPNS0_10ThreadTypeEim" | "nnosSetThreadCoreMask" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_SetThreadCoreMask as *const ()) as usize),
                        "_ZN2nn2os16GetCurrentThreadEv" | "nnosGetCurrentThread" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_GetCurrentThread as *const ()) as usize),
                        "_ZN2nn2os17GetThreadPriorityEPKNS0_10ThreadTypeE" | "nnosGetThreadPriority" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_GetThreadPriority as *const ()) as usize),
                        "_ZN2nn2os12CreateThreadEPNS0_10ThreadTypeEPFvPvES2_S2_mi" | "nnosCreateThread" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_CreateThread as *const ()) as usize),
                        "_ZN2nn2os13DestroyThreadEPNS0_10ThreadTypeE" | "nnosDestroyThread" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_DestroyThread as *const ()) as usize),
                        "_ZN2nn2os20SetThreadNamePointerEPNS0_10ThreadTypeEPKc" | "nnosSetThreadNamePointer" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_SetThreadNamePointer as *const ()) as usize),
                        "_ZN2nn2os15InitializeMutexEPNS0_9MutexTypeEbi" | "nnosInitializeMutex" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_InitializeMutex as *const ()) as usize),
                        "_ZN2nn2os13FinalizeMutexEPNS0_9MutexTypeE" | "nnosFinalizeMutex" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_FinalizeMutex as *const ()) as usize),
                        "_ZN2nn2os9LockMutexEPNS0_9MutexTypeE" | "nnosLockMutex" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_LockMutex as *const ()) as usize),
                        "_ZN2nn2os11UnlockMutexEPNS0_9MutexTypeE" | "nnosUnlockMutex" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_UnlockMutex as *const ()) as usize),
                        "_ZN2nn2os11StartThreadEPNS0_10ThreadTypeE" | "nnosStartThread" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_StartThread as *const ()) as usize),
                        "_ZN2nn2os10WaitThreadEPNS0_10ThreadTypeE" | "nnosWaitThread" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_WaitThread as *const ()) as usize),
                        "_ZN2nn2os11SleepThreadENS_8TimeSpanE" | "nnosSleepThread" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_SleepThread as *const ()) as usize),
                        "_ZN2nn2os13GetSystemTickEv" | "nnosGetSystemTick" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_GetSystemTick as *const ()) as usize),
                        "_ZN2nn2os22GetSystemTickFrequencyEv" | "nnosGetSystemTickFrequency" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_GetSystemTickFrequency as *const ()) as usize),
                        "_ZN2nn2os17ConvertToTimeSpanENS0_4TickE" | "nnosConvertTickToNanoSeconds" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_os_ConvertToTimeSpan as *const ()) as usize),
                        "_ZN2nn2os15InitializeEventEPNS0_9EventTypeEbNS0_14EventClearModeE" | "nnosInitializeEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::nn::os_initialize_event as *const ()) as usize),
                        "_ZN2nn2os13FinalizeEventEPNS0_9EventTypeE" | "nnosFinalizeEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::nn::os_finalize_event as *const ()) as usize),
                        "_ZN2nn2os11SignalEventEPNS0_9EventTypeE" | "nnosSignalEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::nn::os_signal_event as *const ()) as usize),
                        "_ZN2nn2os9WaitEventEPNS0_9EventTypeE" | "nnosWaitEvent" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::nn::os_wait_event as *const ()) as usize),

                        "_ZN2nn3hid25InitializeVibrationDeviceERKNS0_21VibrationDeviceHandleE" | "_ZN2nn3hid6detail25InitializeVibrationDeviceERKNS0_21VibrationDeviceHandleE" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_InitializeVibrationDevice as *const ()) as usize),
                        "_ZN2nn3hid18SendVibrationValueERKNS0_21VibrationDeviceHandleERKNS0_14VibrationValueE" | "_ZN2nn3hid6detail18SendVibrationValueERKNS0_21VibrationDeviceHandleERKNS0_14VibrationValueE" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_SendVibrationValue as *const ()) as usize),
                        "_ZN2nn3hid19SendVibrationValuesEPKNS0_21VibrationDeviceHandleEPKNS0_14VibrationValueEi" | "_ZN2nn3hid6detail19SendVibrationValuesEPKNS0_21VibrationDeviceHandleEPKNS0_14VibrationValueEi" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_SendVibrationValues as *const ()) as usize),
                        "_ZN2nn3hid20IsVibrationPermittedEPb" | "_ZN2nn3hid6detail20IsVibrationPermittedEPb" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_IsVibrationPermitted as *const ()) as usize),
                        "_ZN2nn3hid25GetVibrationDeviceHandlesEPNS0_21VibrationDeviceHandleEiRKjNS_4util10BitFlagSetILi32ENS0_12NpadStyleTagEEE" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetVibrationDeviceHandles as *const ()) as usize),
                        "_ZN2nn3hid22GetVibrationDeviceInfoEPNS0_19VibrationDeviceInfoERKNS0_21VibrationDeviceHandleE" | "_ZN2nn3hid6detail22GetVibrationDeviceInfoEPNS0_19VibrationDeviceInfoERKNS0_21VibrationDeviceHandleE" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetVibrationDeviceInfo as *const ()) as usize),
                        "_ZN2nn3hid28InitializeSevenSixAxisSensorEPvm" | "_ZN2nn3hid6detail28InitializeSevenSixAxisSensorEPvm" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_InitializeSevenSixAxisSensor as *const ()) as usize),
                        "_ZN2nn3hid26FinalizeSevenSixAxisSensorEv" | "_ZN2nn3hid6detail26FinalizeSevenSixAxisSensorEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_FinalizeSevenSixAxisSensor as *const ()) as usize),
                        "_ZN2nn3hid23StartSevenSixAxisSensorEv" | "_ZN2nn3hid6detail23StartSevenSixAxisSensorEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_StartSevenSixAxisSensor as *const ()) as usize),
                        "_ZN2nn3hid22StopSevenSixAxisSensorEv" | "_ZN2nn3hid6detail22StopSevenSixAxisSensorEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_StopSevenSixAxisSensor as *const ()) as usize),
                        "_ZN2nn3hid26IsSevenSixAxisSensorAtRestEv" | "_ZN2nn3hid6detail26IsSevenSixAxisSensorAtRestEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_IsSevenSixAxisSensorAtRest as *const ()) as usize),
                        "_ZN2nn3hid27GetSevenSixAxisSensorStatesEPNS0_23SevenSixAxisSensorStateEi" | "_ZN2nn3hid6detail27GetSevenSixAxisSensorStatesEPNS0_23SevenSixAxisSensorStateEi" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_GetSevenSixAxisSensorStates as *const ()) as usize),
                        "_ZN2nn3hid32ResetSevenSixAxisSensorTimestampEv" | "_ZN2nn3hid6detail32ResetSevenSixAxisSensorTimestampEv" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_ResetSevenSixAxisSensorTimestamp as *const ()) as usize),
                        "_ZN2nn3hid28SetNpadJoyAssignmentModeDualERKj" | "_ZN2nn3hid6detail28SetNpadJoyAssignmentModeDualERKj" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_assignment_noop as *const ()) as usize),
                        "_ZN2nn3hid23MergeSingleJoyAsDualJoyERKjS2_" | "_ZN2nn3hid6detail23MergeSingleJoyAsDualJoyERKjS2_" =>
                            Some(mem_space.make_hle_trampoline(crate::horizon::hle::hle_nn_hid_assignment_noop as *const ()) as usize),
                        _ => None,
                    };

                    if let Some(addr) = hle_override {
                        let final_val = (addr as i64 + r_addend_bits) as usize;
                        unsafe {
                            ptr::write((base + r_offset) as *mut usize, final_val);
                        }
                        rel_count += 1;
                    } else if st_shndx != 0 && st_value != 0 {
                        let addr = (base + st_value) as i64 + r_addend_bits;
                        unsafe {
                            ptr::write((base + r_offset) as *mut usize, addr as usize);
                        }
                        rel_count += 1;
                    } else {
                        let resolved_addr: Option<usize> = match name.as_ref() {
                            n @ ("malloc" | "free" | "calloc" | "realloc" | "aligned_alloc") => {
                                if let Some(a) = mem_space.export(n) {
                                    println!(
                                        "[+] SDK import {} -> main+{:#x}",
                                        n,
                                        a - mem_space.base_ptr as usize
                                    );
                                    Some(a)
                                } else {
                                    match n {
                                        "malloc" => Some(mem_space.make_hle_trampoline(libc::malloc as *const ()) as usize),
                                        "free" => Some(mem_space.make_hle_trampoline(libc::free as *const ()) as usize),
                                        "calloc" => Some(mem_space.make_hle_trampoline(libc::calloc as *const ()) as usize),
                                        "realloc" => Some(mem_space.make_hle_trampoline(libc::realloc as *const ()) as usize),
                                        "aligned_alloc" => Some(mem_space.make_hle_trampoline(sdk_aligned_alloc as *const ()) as usize),
                                        _ => unreachable!(),
                                    }
                                }
                            }
                            // Defined by the game's main module.
                            n @ ("nnMain"
                            | "nninitStartup"
                            | "__nnDetailNintendoSdkRuntimeObjectFile") => {
                                let a = mem_space.export(n);
                                println!(
                                    "[+] SDK import {} -> main+{:#x}",
                                    n,
                                    a.map_or(0, |a| a - mem_space.base_ptr as usize)
                                );
                                a
                            }
                            "nnosGetDefaultUserExceptionStackForApplication" => Some(
                                mem_space
                                    .make_hle_trampoline(sdk_default_exception_stack as *const ())
                                    as usize,
                            ),
                            "__rel_plt_start" => jmprel.map(|v| base + v),
                            "__rel_plt_end" => jmprel.map(|v| base + v + pltrelsz),
                            "__rel_dyn_start" => rela.map(|v| base + v),
                            "__rel_dyn_end" => rela.map(|v| base + v + relasz),
                            other => {
                                eprintln!("[!] SDK: Unresolved external symbol '{}' at offset 0x{:x} - binding to fallback stub", other, r_offset);
                                Some(
                                    mem_space
                                        .make_hle_trampoline(sdk_unimplemented_stub as *const ())
                                        as usize,
                                )
                            }
                        };

                        if let Some(addr) = resolved_addr {
                            let final_val = (addr as i64 + r_addend_bits) as usize;
                            unsafe {
                                ptr::write((base + r_offset) as *mut usize, final_val);
                            }
                            rel_count += 1;
                            println!(
                                "[+] SDK: bound external symbol '{}' at slot 0x{:x} -> 0x{:x}",
                                name, r_offset, final_val
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }
    println!(
        "[+] SDK: applied {} of {} relocations",
        rel_count, entry_count
    );
}
