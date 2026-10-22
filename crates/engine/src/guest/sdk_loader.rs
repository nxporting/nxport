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
    /// The reservation the module was given when it did not fit the slot reserved with main (that one belongs to the
    /// GuestMemorySpace).
    own: Option<(usize, usize)>,
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
        if crate::settings::get().log.list_hle_imports {
            println!("[interpose] {name}");
        }
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
            let _ = crate::os::protect(page as usize, span, crate::os::Prot::ReadWrite);
            let p = addr as *mut u32;
            // The target comes from a literal (data), not the code: patched code must be the same on every launch
            // (AOT images, cpu/aot.rs).
            let [adrp, ldr] = crate::cpu::aot::adrp_ldr(addr, crate::cpu::aot::literal(target), 16);
            ptr::write(p, adrp);
            ptr::write(p.add(1), ldr);
            ptr::write(p.add(2), 0xD61F_0200); // br x16
            ptr::write(p.add(3), 0xD503_201F); // nop
            crate::cpu::aot::protect_rx(page, span);
            crate::os::flush_icache(addr as usize, 16);
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
        let Some(b) = crate::cpu::aot::encode_b(addr, stub) else {
            return false;
        };
        // Stub: adrp x16; ldr x16, <literal: target>; br x16; nop
        let page = stub & !0x3fff;
        let _ = crate::os::protect(page as usize, 0x4000, crate::os::Prot::ReadWrite);
        let p = stub as *mut u32;
        let [adrp, ldr] = crate::cpu::aot::adrp_ldr(stub, crate::cpu::aot::literal(target), 16);
        ptr::write(p, adrp);
        ptr::write(p.add(1), ldr);
        ptr::write(p.add(2), 0xD61F_0200);
        ptr::write(p.add(3), 0xD503_201F);
        crate::cpu::aot::make_exec("sdk.short_island", page, 0x4000);
        crate::os::flush_icache(stub as usize, 16);
        let fpage = addr & !0x3fff;
        let _ = crate::os::protect(fpage as usize, 0x4000, crate::os::Prot::ReadWrite);
        ptr::write(addr as *mut u32, b);
        crate::cpu::aot::protect_rx(fpage, 0x4000);
        crate::os::flush_icache(addr as usize, 4);
        true
    }
}

impl Drop for SdkModule {
    fn drop(&mut self) {
        if let Some((base, size)) = self.own {
            unsafe { crate::os::release(base, size) };
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

/// The C heap the SDK's malloc family uses when main does not define its own. One family allocates and frees every
/// block, aligned_alloc's included.
#[cfg(unix)]
mod c_heap {
    pub use libc::{calloc, free, malloc, realloc};

    pub unsafe extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut libc::c_void {
        let align = alignment.max(std::mem::size_of::<*mut ()>()).next_power_of_two();
        let mut ptr: *mut libc::c_void = std::ptr::null_mut();
        if libc::posix_memalign(&mut ptr, align, size) == 0 {
            ptr
        } else {
            std::ptr::null_mut()
        }
    }
}

/// The C heap on Windows, whose CRT free cannot take an aligned block: every block carries its layout in a header
/// right below it, so free and realloc work on any of them.
#[cfg(windows)]
mod c_heap {
    use libc::c_void;
    use std::alloc::{alloc, dealloc, Layout};

    /// malloc's alignment on 64-bit hosts, and the header's size: (block size, alignment).
    const MIN_ALIGN: usize = 16;

    unsafe fn allocate(size: usize, align: usize) -> *mut c_void {
        let align = align.max(MIN_ALIGN).next_power_of_two();
        // The header sits in the last 16 bytes of the `align` bytes in front of the block.
        let Some(Ok(layout)) = size.checked_add(align).map(|t| Layout::from_size_align(t, align)) else {
            return std::ptr::null_mut();
        };
        let raw = alloc(layout);
        if raw.is_null() {
            return std::ptr::null_mut();
        }
        let p = raw.add(align);
        *(p.sub(16) as *mut [usize; 2]) = [size, align];
        p as *mut c_void
    }

    unsafe fn layout_of(p: *mut c_void) -> (usize, usize) {
        let [size, align] = *((p as *mut u8).sub(16) as *const [usize; 2]);
        (size, align)
    }

    pub unsafe extern "C" fn malloc(size: usize) -> *mut c_void {
        allocate(size, MIN_ALIGN)
    }

    pub unsafe extern "C" fn aligned_alloc(alignment: usize, size: usize) -> *mut c_void {
        allocate(size, alignment)
    }

    pub unsafe extern "C" fn calloc(n: usize, size: usize) -> *mut c_void {
        let Some(total) = n.checked_mul(size) else {
            return std::ptr::null_mut();
        };
        let p = malloc(total);
        if !p.is_null() {
            std::ptr::write_bytes(p as *mut u8, 0, total);
        }
        p
    }

    pub unsafe extern "C" fn free(p: *mut c_void) {
        if p.is_null() {
            return;
        }
        let (size, align) = layout_of(p);
        dealloc((p as *mut u8).sub(align), Layout::from_size_align_unchecked(size + align, align));
    }

    pub unsafe extern "C" fn realloc(p: *mut c_void, size: usize) -> *mut c_void {
        if p.is_null() {
            return malloc(size);
        }
        let q = malloc(size);
        if !q.is_null() {
            std::ptr::copy_nonoverlapping(p as *const u8, q as *mut u8, layout_of(p).0.min(size));
            free(p);
        }
        q
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn blocks_of_any_alignment_free_and_grow() {
            unsafe {
                let a = aligned_alloc(4096, 100);
                assert_eq!(a as usize % 4096, 0);
                *(a as *mut u8).add(99) = 9;
                let b = realloc(a, 10_000);
                assert_eq!(b as usize % MIN_ALIGN, 0);
                assert_eq!(*(b as *mut u8).add(99), 9);
                free(b);
                let c = calloc(3, 5) as *mut u8;
                assert!((0..15).all(|i| *c.add(i) == 0));
                free(c as *mut c_void);
                free(std::ptr::null_mut());
            }
        }
    }
}

unsafe extern "C" fn sdk_default_exception_stack() -> *mut libc::c_void {
    static mut STACK_BUF: [u8; 0x1000] = [0; 0x1000];
    (&raw mut STACK_BUF).cast()
}

unsafe extern "C" fn sdk_unimplemented_stub() -> i64 {
    let lr = crate::guest_reg!(x30);
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

use super::nso::middleware_version;

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
    let sdk_version = middleware_version(&ro_raw, b"NintendoSdk_nnSdk-");
    crate::horizon::sdk::set_version(sdk_version.map(|(a, b, c)| crate::horizon::sdk::SdkVersion(a, b, c)));
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
    let (base, own) = if total_aligned <= crate::guest::loader::SDK_SLOT_SIZE {
        (mem_space.sdk_slot, None)
    } else {
        let hint =
            (mem_space.base_ptr as usize + mem_space.total_size + crate::guest::loader::SDK_GAP)
                & !0x3fff;
        let base = unsafe {
            crate::os::reserve(crate::os::At::Near(hint), total_aligned).and_then(|b| {
                crate::os::commit(b, total_aligned, crate::os::Prot::ReadWrite).map(|()| b)
            })
        }
        .map_err(|e| format!("SDK memory: {e}"))?;
        (base, Some((base, total_aligned)))
    };

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
        own,
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

                    let hle_override: Option<usize> = match crate::horizon::sdk::lookup(&name) {
                        Some(crate::horizon::sdk::Target::Func(f)) => {
                            Some(mem_space.shared_hle_trampoline(f as *const ()) as usize)
                        }
                        // Data symbols are for the game's imports; the sdk module keeps its own (its stdout, ...).
                        Some(crate::horizon::sdk::Target::Data(_)) | None => None,
                    };

                    if let Some(addr) = hle_override {
                        if crate::settings::get().log.list_hle_imports {
                            println!("[sdk-ref] HLE {name}");
                        }
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
                                        "malloc" => Some(mem_space.make_hle_trampoline(crate::hle!(c_heap::malloc(_))) as usize),
                                        "free" => Some(mem_space.make_hle_trampoline(crate::hle!(c_heap::free(_))) as usize),
                                        "calloc" => Some(mem_space.make_hle_trampoline(crate::hle!(c_heap::calloc(_, _))) as usize),
                                        "realloc" => Some(mem_space.make_hle_trampoline(crate::hle!(c_heap::realloc(_, _))) as usize),
                                        "aligned_alloc" => Some(mem_space.make_hle_trampoline(crate::hle!(c_heap::aligned_alloc(_, _))) as usize),
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
                                    .make_hle_trampoline(crate::hle!(sdk_default_exception_stack()))
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
                                        .make_hle_trampoline(crate::hle!(sdk_unimplemented_stub()))
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
