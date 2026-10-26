// Extra NSO modules from mods (exefs/subsdk0..9, e.g. exlaunch plugins such as UltraCam).
//
// Like rtld: decompress and map the module, apply its relocations (imports resolved by name through `resolve`, the
// same targets the game's own imports got, so a mod calling nn::fs::OpenFile reaches the engine's HLE), then make it
// runnable on macOS the way the engine treats the game's code:
//   - x18 uses are rewritten when the kernel does not preserve x18 (cpu/trampoline.rs)
//   - `mrs xN, tpidr_el0` loads the guest TLS pointer (as cpu/tpidr_rewrite.rs does for main/SDK)
//   - `mrs xN, tpidrro_el0` loads the thread's fake Horizon thread-local region (IPC buffer; cpu::tls::FAKE_TLR_SLOT)
//     instead of Darwin's TSD
//   - `svc #n` branches to a stub that calls svc::svc_dispatch with the saved x0-x7
// Stubs live in the module's own mapping (after its image), so every site is within branch range. The caller runs
// the module's DT_INIT (exlaunch's module entry) before the game's nnMain.

#[cfg(nx_native)]
use crate::cpu::aot::encode_b;
use crate::horizon::svc::{self, Region};
use std::ptr;

pub struct LoadedModule {
    pub base: usize,
    pub image_size: usize,
    pub init: Option<usize>,
    /// DT_INIT_ARRAY (address, entry count): static constructors, for modules whose DT_INIT does not run them.
    pub init_array: (usize, usize),
    /// Defined dynamic symbols (name -> address).
    pub exports: std::collections::HashMap<String, usize>,
}

const STUB_AREA: usize = 0x2_0000;
// System register reads native_rewrites redirects.
#[cfg(nx_native)]
const MRS_TPIDR: u32 = 0xD53B_D040;
#[cfg(nx_native)]
const MRS_TPIDRRO: u32 = 0xD53B_D060;
#[cfg(nx_native)]
const MRS_CTR: u32 = 0xD53B_0020;

fn r_u32(d: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(d[off..off + 4].try_into().unwrap())
}


/// The rewrites guest code needs to run natively (docs/windows-port.md): x18 uses (cpu::trampoline), svc into
/// calls of horizon::svc, reads of TPIDR_EL0 / TPIDRRO_EL0 / CTR_EL0 into stubs (in `stubs`, STUB_AREA bytes), and
/// load-exclusives into acquire forms. Returns what was rewritten, for the log.
#[cfg(nx_native)]
unsafe fn native_rewrites(name: &str, text: usize, text_len: usize, stubs: usize, svc_entry: usize) -> Result<String, String> {
    if crate::cpu::trampoline::x18_trampolines() {
        crate::cpu::trampoline::rewrite_module(name, text, text_len).map_err(|e| e.to_string())?;
    }
    let mut next = stubs;
    let (mut n_svc, mut n_tp, mut n_tpro) = (0, 0, 0);
    for off in (0..text_len & !3).step_by(4) {
        let site = text + off;
        let w = ptr::read(site as *const u32);
        let stub_words: Vec<u32>;
        if w & 0xFFE0_001F == 0xD400_0001 {
            let n = (w >> 5) & 0xFFFF;
            // Save x0-x7 and lr, call svc_dispatch(regs, n) through the HLE trampoline, reload, continue.
            let mut v = vec![
                0xD10143FF,            // sub sp, sp, #0x50
                0xA90007E0,            // stp x0, x1, [sp]
                0xA9010FE2,            // stp x2, x3, [sp, #16]
                0xA90217E4,            // stp x4, x5, [sp, #32]
                0xA9031FE6,            // stp x6, x7, [sp, #48]
                0xF90023FE,            // str x30, [sp, #64]
                0x910003E0,            // mov x0, sp
                0xD2800001 | (n << 5), // movz x1, #n
                0x58000150,            // ldr x16, <literal at +40>
                0xD63F0200,            // blr x16
                0xA94007E0,            // ldp x0, x1, [sp]
                0xA9410FE2,            // ldp x2, x3, [sp, #16]
                0xA94217E4,            // ldp x4, x5, [sp, #32]
                0xA9431FE6,            // ldp x6, x7, [sp, #48]
                0xF94023FE,            // ldr x30, [sp, #64]
                0x910143FF,            // add sp, sp, #0x50
            ];
            let back = encode_b(next + v.len() * 4, site + 4).ok_or("stub out of branch range")?;
            v.push(back);
            v.push(0xD503201F); // nop (literal alignment)
            v.push(svc_entry as u32);
            v.push((svc_entry >> 32) as u32);
            stub_words = v;
            n_svc += 1;
        } else if w & 0xFFFF_FFE0 == MRS_CTR {
            // CTR_EL0 traps on macOS; report the Switch CPU's (Cortex-A57: 64-byte lines) for __clear_cache loops.
            let rt = w & 0x1f;
            let back = encode_b(next + 8, site + 4).ok_or("stub out of branch range")?;
            stub_words = vec![
                0xD298_0080 | rt, /* movz xt, #0xc004 */
                0xF2B0_8880 | rt, /* movk xt, #0x8444, lsl #16 */
                back,
            ];
        } else if w & 0xFFFF_FFE0 == MRS_TPIDR && w & 0x1f != 31 {
            let rt = w & 0x1f;
            let back = encode_b(next + 8, site + 4).ok_or("stub out of branch range")?;
            stub_words = vec![
                MRS_TPIDRRO | rt,
                0xF940_0000
                    | (((crate::cpu::tls::guest_tls_offset() / 8) as u32) << 10)
                    | (rt << 5)
                    | rt,
                back,
            ];
            n_tp += 1;
        } else if w & 0xFFFF_FFE0 == MRS_TPIDRRO && w & 0x1f != 31 {
            let rt = w & 0x1f;
            let slot = crate::cpu::tls::FAKE_TLR_SLOT as u32;
            let back = encode_b(next + 12, site + 4).ok_or("stub out of branch range")?;
            stub_words = vec![
                MRS_TPIDRRO | rt,
                0xF940_0000
                    | (((crate::cpu::tls::guest_tls_offset() / 8) as u32) << 10)
                    | (rt << 5)
                    | rt, // guest TLS block
                0xF940_0000 | ((slot / 8) << 10) | (rt << 5) | rt, // its fake thread-local region
                back,
            ];
            n_tpro += 1;
        } else {
            continue;
        }
        if next + stub_words.len() * 4 + 8 > stubs + STUB_AREA {
            return Err("stub area full".into());
        }
        // Literal pools need 8-byte alignment: svc stubs start aligned.
        if next % 8 != 0 {
            ptr::write(next as *mut u32, 0xD503201F);
            next += 4;
        }
        let start = next;
        for (i, word) in stub_words.iter().enumerate() {
            ptr::write((start + i * 4) as *mut u32, *word);
        }
        // Re-encode the branch back now that the stub's final position is known.
        let back_idx = stub_words
            .iter()
            .position(|&x| x & 0xFC00_0000 == 0x1400_0000)
            .unwrap();
        ptr::write(
            (start + back_idx * 4) as *mut u32,
            encode_b(start + back_idx * 4, site + 4).ok_or("stub out of branch range")?,
        );
        ptr::write(
            site as *mut u32,
            encode_b(site, start).ok_or("stub out of branch range")?,
        );
        next = start + stub_words.len() * 4;
    }
    let n_ldx = crate::cpu::exclusive_rewrite::rewrite(&[(text, text_len, false)]);
    Ok(format!("{n_svc} svc, {n_tp} tpidr_el0, {n_tpro} tpidrro_el0, {n_ldx} load-exclusive sites"))
}

/// Loads `path`, placing it at or above `hint` when possible. `svc_entry` is an HLE trampoline to
/// svc::svc_dispatch; `unresolved` is called for imports `resolve` does not know.
pub unsafe fn load(
    path: &std::path::Path,
    name: &str,
    hint: usize,
    resolve: &dyn Fn(&str) -> Option<usize>,
    svc_entry: usize,
    unresolved: usize,
) -> Result<LoadedModule, String> {
    let file = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    if file.len() < 0x100 || &file[..4] != b"NSO0" {
        return Err(format!("{}: not an NSO", path.display()));
    }
    let flags = r_u32(&file, 0x0C);
    let seg = |i: usize| {
        let h = 0x10 + i * 0x10;
        (
            r_u32(&file, h) as usize,
            r_u32(&file, h + 4) as usize,
            r_u32(&file, h + 8) as usize,
            r_u32(&file, 0x60 + i * 4) as usize,
        )
    };
    let mut segs = Vec::new();
    for i in 0..3 {
        let (fo, mo, ds, cs) = seg(i);
        let raw = file.get(fo..fo + cs).ok_or("truncated NSO")?;
        let data = if flags >> i & 1 != 0 {
            lz4_flex::decompress(raw, ds).map_err(|e| format!("lz4: {}", e))?
        } else {
            raw.to_vec()
        };
        segs.push((mo, data));
    }
    let bss = r_u32(&file, 0x3C) as usize;
    let image_size = segs[2].0 + segs[2].1.len() + bss;
    let image_aligned = (image_size + 0x3fff) & !0x3fff;
    // For modules with gigantic bss (> 96 MB, e.g. Metroid Dread or Unreal Engine games),
    // placing stubs after image_aligned exceeds the ARM64 ±128 MB branch range.
    // Instead, place the stub area immediately BEFORE the module base.
    let front_stubs = image_aligned > 0x600_0000;
    let total = image_aligned + STUB_AREA;

    // Inside the reserved gap after main the spot is ours: commit it, so the layout is the same in every run. Elsewhere
    // only a hint is possible.
    let (gap, gap_end) = crate::guest::loader::mod_gap();
    let fixed = gap != 0 && hint >= gap && hint + total <= gap_end;
    let alloc_base = if fixed {
        hint
    } else {
        crate::os::reserve(crate::os::At::Near(hint), total).map_err(|e| format!("reserve: {e}"))?
    };
    crate::os::commit(alloc_base, total, crate::os::Prot::ReadWrite).map_err(|e| format!("commit: {e}"))?;
    if !fixed {
        println!(
            "[!] Mods: {} does not fit the reserved module gap; mapped at {:#x} (hint {:#x})",
            name, alloc_base, hint
        );
    }
    let (base, stubs) = if front_stubs {
        (alloc_base + STUB_AREA, alloc_base)
    } else {
        (alloc_base, alloc_base + image_aligned)
    };
    for (mo, data) in &segs {
        ptr::copy_nonoverlapping(data.as_ptr(), (base + mo) as *mut u8, data.len());
    }
    let (text_mo, text_len) = (segs[0].0, segs[0].1.len());
    let (ro_mo, ro_len) = (segs[1].0, segs[1].1.len());
    let data_mo = segs[2].0;

    // ── dynamic section via MOD0 ──
    let mod0 = r_u32(&segs[0].1, 4) as usize;
    let rd = |a: usize| ptr::read((base + a) as *const u64);
    let mut init = None;
    let mut init_array = (0usize, 0usize);
    let mut exports = std::collections::HashMap::new();
    if mod0 + 8 <= image_size
        && &*std::ptr::slice_from_raw_parts((base + mod0) as *const u8, 4) == b"MOD0"
    {
        let dyn_off = (mod0 as i64 + ptr::read((base + mod0 + 4) as *const i32) as i64) as usize;
        let mut tags = std::collections::HashMap::new();
        let mut o = dyn_off;
        loop {
            let (t, v) = (rd(o), rd(o + 8));
            if t == 0 || o > image_size {
                break;
            }
            tags.entry(t).or_insert(v as usize);
            o += 16;
        }
        let symtab = tags.get(&6).copied().unwrap_or(0);
        let strtab = tags.get(&5).copied().unwrap_or(0);
        init = tags.get(&12).map(|&v| base + v);
        if let (Some(&a), Some(&sz)) = (tags.get(&25), tags.get(&27)) {
            init_array = (base + a, sz / 8);
        }
        let sym_name = |idx: usize| -> (String, u16, usize) {
            let s = base + symtab + idx * 24;
            let st_name = ptr::read(s as *const u32) as usize;
            let shndx = ptr::read((s + 6) as *const u16);
            let value = ptr::read((s + 8) as *const u64) as usize;
            let n = std::ffi::CStr::from_ptr((base + strtab + st_name) as *const libc::c_char)
                .to_string_lossy()
                .into_owned();
            (n, shndx, value)
        };
        if symtab != 0 && strtab > symtab {
            for idx in 1..(strtab - symtab) / 24 {
                let (n, shndx, value) = sym_name(idx);
                if shndx != 0 && value != 0 && !n.is_empty() {
                    exports.insert(n, base + value);
                }
            }
        }
        let mut missing = Vec::new();
        let mut apply_table = |off: usize, size: usize| {
            for k in 0..size / 24 {
                let e = base + off + k * 24;
                let (r_offset, info, addend) = (
                    ptr::read(e as *const u64) as usize,
                    ptr::read((e + 8) as *const u64),
                    ptr::read((e + 16) as *const i64),
                );
                if r_offset + 8 > image_size {
                    continue;
                }
                let target = (base + r_offset) as *mut usize;
                match (info & 0xffff_ffff) as u32 {
                    1027 => ptr::write(target, (base as i64 + addend) as usize),
                    257 | 1025 | 1026 => {
                        let (n, shndx, value) = sym_name((info >> 32) as usize);
                        let s = if shndx != 0 {
                            base + value
                        } else {
                            match resolve(&n) {
                                Some(a) => a,
                                None => {
                                    if !missing.contains(&n) {
                                        missing.push(n.clone());
                                    }
                                    unresolved
                                }
                            }
                        };
                        ptr::write(target, (s as i64 + addend) as usize);
                    }
                    _ => {}
                }
            }
        };
        if let (Some(&rela), Some(&sz)) = (tags.get(&7), tags.get(&8)) {
            apply_table(rela, sz);
        }
        if let (Some(&jmprel), Some(&sz)) = (tags.get(&23), tags.get(&2)) {
            apply_table(jmprel, sz);
        }
        for n in missing.iter().take(8) {
            println!(
                "[!] Module {}: import {} is not available; calls go to a logging stub",
                name, n
            );
        }
        if missing.len() > 8 {
            println!(
                "[!] Module {}: ... and {} more unavailable imports",
                name,
                missing.len() - 8
            );
        }
    } else {
        println!(
            "[!] Module {}: no MOD0 header, loaded without relocations",
            name
        );
    }

    // ── make the code runnable here ──
    #[cfg(nx_native)]
    let rewritten = native_rewrites(name, base + text_mo, text_len, stubs, svc_entry)?;
    // Lifted code handles svc, the system registers and exclusives itself.
    #[cfg(not(nx_native))]
    let rewritten = {
        let _ = svc_entry;
        "no sites"
    };
    let text_pages = (text_len + 0x3fff) & !0x3fff;
    // Code regions go through cpu/aot.rs: on iOS they come from the signed image (mods included, cpu/aot.rs).
    let short: String = name
        .chars()
        .rev()
        .take(40)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    crate::cpu::aot::make_exec(&format!("{}.t", short), base + text_mo, text_pages);
    crate::cpu::aot::make_exec(&format!("{}.s", short), stubs, STUB_AREA);
    crate::os::flush_icache(((base + text_mo)) as usize, text_len);
    crate::os::flush_icache(stubs as usize, STUB_AREA);

    // Horizon reports each segment exactly (NSO segments are 4 KiB aligned): text, rodata, data+bss back to back.
    let _ = ro_len;
    svc::add_region(Region {
        start: base + text_mo,
        size: ro_mo - text_mo,
        mem_type: svc::MEM_CODE_STATIC,
        perm: svc::PERM_RX,
    });
    svc::add_region(Region {
        start: base + ro_mo,
        size: data_mo - ro_mo,
        mem_type: svc::MEM_CODE_STATIC,
        perm: svc::PERM_R,
    });
    svc::add_region(Region {
        start: base + data_mo,
        size: ((image_size + 0xfff) & !0xfff) - data_mo,
        mem_type: svc::MEM_CODE_MUTABLE,
        perm: svc::PERM_RW,
    });
    println!(
        "[+] Module {}: loaded at {:#x} ({:#x} bytes): {} rewritten",
        name, base, image_size, rewritten
    );
    Ok(LoadedModule {
        base,
        image_size,
        init,
        init_array,
        exports,
    })
}

// ── runtime services for mods ────────────────────────────────────────────────────────────────────

static SYMBOLS: std::sync::OnceLock<std::collections::HashMap<String, usize>> =
    std::sync::OnceLock::new();

/// The name -> address table mods resolve against (game exports, the game's final import targets, SDK exports,
/// plus the overrides below).
pub fn set_symbols(map: std::collections::HashMap<String, usize>) {
    let _ = SYMBOLS.set(map);
}

pub unsafe extern "C" fn ro_initialize() -> u32 {
    0
}

pub unsafe extern "C" fn ro_finalize() {}

pub unsafe extern "C" fn ro_get_buffer_size(out: *mut usize, image: *const u8) -> u32 {
    if image.is_null() {
        return 0x816; // RO_NOT_NRO
    }
    let magic = std::slice::from_raw_parts(image.add(0x10), 4);
    if magic != b"NRO0" {
        return 0x816;
    }
    let bss_size = ptr::read_unaligned(image.add(0x38) as *const u32) as usize;
    if !out.is_null() {
        *out = bss_size;
    }
    0
}

pub unsafe extern "C" fn ro_lookup_module_symbol(
    out: *mut usize,
    _mod: *const (),
    name: *const libc::c_char,
) -> u32 {
    ro_lookup_symbol(out, name)
}

pub unsafe extern "C" fn ro_register_module_info(_info: *mut (), nrr: *const u8) -> u32 {
    if nrr.is_null() {
        return 0x816;
    }
    let magic = std::slice::from_raw_parts(nrr, 4);
    if magic != b"NRR0" {
        return 0x816;
    }
    0
}

pub unsafe extern "C" fn ro_unregister_module_info(_info: *const ()) -> u32 {
    0
}

#[repr(C)]
pub struct RoModule {
    pub module: *mut (),
    pub state: u32,
    pub pad: u32,
    pub image: *const u8,
    pub buffer: *mut u8,
}

pub unsafe extern "C" fn ro_load_module(
    rm: *mut RoModule,
    image: *const u8,
    buf: *mut u8,
    _size: usize,
    _flag: i32,
) -> u32 {
    if image.is_null() {
        return 0x816;
    }
    let magic = std::slice::from_raw_parts(image.add(0x10), 4);
    if magic != b"NRO0" {
        return 0x816;
    }
    if !rm.is_null() {
        (*rm).module = image as *mut ();
        (*rm).state = 1;
        (*rm).image = image;
        (*rm).buffer = buf;
    }
    0
}

pub unsafe extern "C" fn ro_load_module_sdk19(
    rm: *mut RoModule,
    image: *const u8,
    buf: *mut u8,
    size: usize,
    flag: i32,
    _b: bool,
) -> u32 {
    ro_load_module(rm, image, buf, size, flag)
}

pub unsafe extern "C" fn ro_unload_module(rm: *mut RoModule) {
    if !rm.is_null() {
        (*rm).state = 0;
        (*rm).module = std::ptr::null_mut();
    }
}

pub fn lookup(name: &str) -> Option<usize> {
    SYMBOLS.get()?.get(name).copied()
}

/// nn::ro::LookupSymbol(uintptr_t* out, const char* name) -> Result.
pub unsafe extern "C" fn ro_lookup_symbol(out: *mut usize, name: *const libc::c_char) -> u32 {
    let n = std::ffi::CStr::from_ptr(name).to_string_lossy();
    match lookup(&n) {
        Some(a) => {
            if !out.is_null() {
                *out = a;
            }
            if crate::settings::get().log.log_svc {
                println!("[svc] nn::ro::LookupSymbol('{}') -> {:#x}", n, a);
            }
            0
        }
        None => match placeholder_for(&n) {
            // Nothing provides it (e.g. the game's own subsdk0 libraries are not loaded): give the mod a private
            // do-nothing function so installing a hook on it succeeds without touching anything shared.
            Some(a) => {
                println!("[!] nn::ro::LookupSymbol('{}'): not available; returning a private placeholder", n);
                if !out.is_null() {
                    *out = a;
                }
                0
            }
            None => {
                println!("[!] nn::ro::LookupSymbol('{}'): not found", n);
                0x0000_4A0B // ro: ResultNotFound-like
            }
        },
    }
}

/// Distinct do-nothing functions (HLE trampolines to `unresolved_import`, made before the pool is sealed) handed out by
/// LookupSymbol for names nothing provides, one per name.
static PLACEHOLDERS: std::sync::Mutex<(Vec<usize>, Vec<(String, usize)>)> =
    std::sync::Mutex::new((Vec::new(), Vec::new()));

pub fn set_placeholders(addrs: Vec<usize>) {
    PLACEHOLDERS.lock().unwrap().0 = addrs;
}

fn placeholder_for(name: &str) -> Option<usize> {
    let mut g = PLACEHOLDERS.lock().unwrap();
    if let Some(&(_, a)) = g.1.iter().find(|(n, _)| n == name) {
        return Some(a);
    }
    let a = g.0.pop()?;
    g.1.push((name.to_string(), a));
    Some(a)
}

/// Called through imports nothing provides; returns 0 so the caller sees "no result" rather than jumping to null.
pub unsafe extern "C" fn unresolved_import() -> usize {
    println!("[!] a mod called an import that is not available (see the module's load messages)");
    0
}

// Network: mods' optional companion-app links (UltraCam's NX Optimizer connection) see no network.
pub unsafe extern "C" fn nifm_initialize() -> u32 {
    0
}
pub unsafe extern "C" fn nifm_submit_network_request() {}
pub unsafe extern "C" fn nifm_false() -> bool {
    false
}
pub unsafe extern "C" fn socket_initialize(_cfg: *const ()) -> u32 {
    0
}
pub unsafe extern "C" fn socket_initialize_pool(
    _pool: *mut libc::c_void,
    _pool_size: usize,
    _alloc_pool_size: usize,
    _concurrency: i32,
) -> u32 {
    0
}
pub unsafe extern "C" fn socket_finalize() -> u32 {
    0
}
pub unsafe extern "C" fn socket_close(_fd: i32) -> i32 {
    0
}
pub unsafe extern "C" fn socket_fail() -> i32 {
    -1
}
pub unsafe extern "C" fn socket_get_last_errno() -> i32 {
    100 // ENetDown
}
pub unsafe extern "C" fn socket_inet_aton(_s: *const libc::c_char, _out: *mut u32) -> i32 {
    0
}

/// --mod-log-socket: mods see a network and a socket that connects, and what they send is printed ("[mod-net]").
/// UltraCam reports its heap increases and resolution checks to the NX Optimizer app this way.
fn mod_log_socket() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().files.mod_log_socket)
}
pub unsafe extern "C" fn nifm_available() -> bool {
    mod_log_socket()
}
pub unsafe extern "C" fn socket_socket(_domain: i32, _ty: i32, _proto: i32) -> i32 {
    if mod_log_socket() {
        3
    } else {
        -1
    }
}
pub unsafe extern "C" fn socket_connect(_fd: i32, _addr: *const u8, _len: u32) -> i32 {
    if mod_log_socket() {
        0
    } else {
        -1
    }
}
pub unsafe extern "C" fn socket_send(_fd: i32, buf: *const u8, len: usize, _flags: i32) -> isize {
    if !mod_log_socket() || buf.is_null() {
        return -1;
    }
    let bytes = std::slice::from_raw_parts(buf, len);
    println!("[mod-net] {}", String::from_utf8_lossy(bytes).trim_end());
    len as isize
}
pub unsafe extern "C" fn socket_recv(_fd: i32, _buf: *mut u8, _len: usize, _flags: i32) -> isize {
    if mod_log_socket() {
        std::thread::sleep(std::time::Duration::from_millis(100)); // nothing ever arrives; don't let a reader spin
    }
    -1
}
pub unsafe extern "C" fn socket_inet_htons(v: u16) -> u16 {
    v.swap_bytes()
}
/// nn::fs::MountSdCard: mount SD card storage.
pub unsafe extern "C" fn fs_mount_sdcard(name: *const libc::c_char) -> u32 {
    let default_name = b"sd\0";
    let name_ptr = if name.is_null() {
        default_name.as_ptr() as *const libc::c_char
    } else {
        name
    };
    crate::horizon::fs::register_writable_mount("MountSdCard", name_ptr);
    0
}

/// nn::oe::GetDisplayVersion(DisplayVersion*): the game's version string (char[16]); our dump is 1.1.2.
pub unsafe extern "C" fn oe_get_display_version(out: *mut u8) {
    if !out.is_null() {
        std::ptr::write_bytes(out, 0, 16);
        std::ptr::copy_nonoverlapping(b"1.1.2".as_ptr(), out, 5);
    }
}

/// nn::hid::GetNpadState(State*, const NpadIdType&): the single-state form of the HLE'd GetNpadStates (the SDK's own
/// version needs hid state the engine does not create).
pub unsafe extern "C" fn hid_get_npad_state(
    out: *mut crate::horizon::hle::NpadFullKeyState,
    id: *const u32,
) {
    crate::horizon::hle::hle_nn_hid_GetNpadStates(out, 1, id);
}

/// Storage for nn::ro::detail data symbols mods may read (no auto-load list, no manual lookup hook).
pub static RO_DATA_ZERO: [u64; 4] = [0; 4];

// Newlib runtime hooks for mods built with devkitA64 / libnx (UltraCam 3). Their newlib calls these for its locks,
// condition variables, per-thread reent and thread-local keys. The types follow libnx: _LOCK_T is a Mutex (u32),
// _LOCK_RECURSIVE_T an RMutex { Mutex lock; u32 thread_tag; u32 counter }, _COND_T a CondVar (u32). The words live in
// the mod's memory; here a mutex holds its owner's tag (0 = free) and a condition variable a wake-up counter. Waits
// spin briefly, then sleep in short steps: mod code takes these locks rarely.

/// Nonzero tag of the calling thread.
fn lock_tag() -> u32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(1);
    thread_local! { static TAG: u32 = NEXT.fetch_add(1, Ordering::Relaxed); }
    TAG.with(|t| *t)
}

unsafe fn word<'a>(p: *mut u32) -> &'a std::sync::atomic::AtomicU32 {
    &*(p as *const std::sync::atomic::AtomicU32)
}

/// Waits until `done` holds, or until `deadline` (then false).
fn wait_until(deadline: Option<std::time::Instant>, mut done: impl FnMut() -> bool) -> bool {
    let mut spins = 0u32;
    while !done() {
        if deadline.is_some_and(|d| std::time::Instant::now() >= d) {
            return false;
        }
        spins += 1;
        if spins < 64 {
            std::hint::spin_loop();
        } else if spins < 256 {
            std::thread::yield_now();
        } else {
            std::thread::sleep(std::time::Duration::from_micros(50));
        }
    }
    true
}

unsafe fn try_lock(lock: *mut u32) -> bool {
    use std::sync::atomic::Ordering;
    word(lock)
        .compare_exchange(0, lock_tag(), Ordering::Acquire, Ordering::Relaxed)
        .is_ok()
}

pub unsafe extern "C" fn sys_lock_acquire(lock: *mut u32) {
    if !lock.is_null() {
        wait_until(None, || try_lock(lock));
    }
}

pub unsafe extern "C" fn sys_lock_try_acquire(lock: *mut u32) -> i32 {
    // newlib: 0 when the lock was taken.
    if !lock.is_null() && try_lock(lock) {
        0
    } else {
        1
    }
}

pub unsafe extern "C" fn sys_lock_release(lock: *mut u32) {
    if !lock.is_null() {
        word(lock).store(0, std::sync::atomic::Ordering::Release);
    }
}

/// RMutex: lock word, owner tag, recursion count.
pub unsafe extern "C" fn sys_lock_acquire_recursive(l: *mut u32) {
    if l.is_null() {
        return;
    }
    if *l.add(1) == lock_tag() {
        *l.add(2) += 1;
        return;
    }
    sys_lock_acquire(l);
    *l.add(1) = lock_tag();
    *l.add(2) = 1;
}

pub unsafe extern "C" fn sys_lock_try_acquire_recursive(l: *mut u32) -> i32 {
    if l.is_null() {
        return 1;
    }
    if *l.add(1) == lock_tag() {
        *l.add(2) += 1;
        return 0;
    }
    if !try_lock(l) {
        return 1;
    }
    *l.add(1) = lock_tag();
    *l.add(2) = 1;
    0
}

pub unsafe extern "C" fn sys_lock_release_recursive(l: *mut u32) {
    if l.is_null() || *l.add(1) != lock_tag() {
        return;
    }
    *l.add(2) -= 1;
    if *l.add(2) == 0 {
        *l.add(1) = 0;
        sys_lock_release(l);
    }
}

pub unsafe extern "C" fn sys_cond_signal(cond: *mut u32) -> i32 {
    // Waking every waiter is allowed for a signal (spurious wake-ups).
    sys_cond_broadcast(cond)
}

pub unsafe extern "C" fn sys_cond_broadcast(cond: *mut u32) -> i32 {
    if !cond.is_null() {
        word(cond).fetch_add(1, std::sync::atomic::Ordering::Release);
    }
    0
}

const NEWLIB_ETIMEDOUT: i32 = 116;

unsafe fn cond_wait(
    cond: *mut u32,
    timeout_ns: u64,
    release: impl FnOnce(),
    reacquire: impl FnOnce(),
) -> i32 {
    if cond.is_null() {
        return 0;
    }
    let seen = word(cond).load(std::sync::atomic::Ordering::Acquire);
    release();
    let deadline = (timeout_ns != u64::MAX)
        .then(|| std::time::Instant::now() + std::time::Duration::from_nanos(timeout_ns));
    let woken = wait_until(deadline, || {
        word(cond).load(std::sync::atomic::Ordering::Acquire) != seen
    });
    reacquire();
    if woken {
        0
    } else {
        NEWLIB_ETIMEDOUT
    }
}

pub unsafe extern "C" fn sys_cond_wait(cond: *mut u32, lock: *mut u32, timeout_ns: u64) -> i32 {
    cond_wait(
        cond,
        timeout_ns,
        || sys_lock_release(lock),
        || sys_lock_acquire(lock),
    )
}

pub unsafe extern "C" fn sys_cond_wait_recursive(
    cond: *mut u32,
    l: *mut u32,
    timeout_ns: u64,
) -> i32 {
    // The whole recursion is released and restored, as libnx does.
    let depth = if l.is_null() { 0 } else { *l.add(2) };
    cond_wait(
        cond,
        timeout_ns,
        || {
            if !l.is_null() {
                *l.add(1) = 0;
                *l.add(2) = 0;
                sys_lock_release(l);
            }
        },
        || {
            if !l.is_null() {
                sys_lock_acquire(l);
                *l.add(1) = lock_tag();
                *l.add(2) = depth;
            }
        },
    )
}

/// One zeroed reent per thread (newlib keeps errno and stdio state there).
pub unsafe extern "C" fn sys_getreent() -> *mut libc::c_void {
    #[repr(C, align(64))]
    struct Reent([u8; 4096]);
    thread_local! { static REENT: *mut Reent = Box::into_raw(Box::new(Reent([0; 4096]))); }
    REENT.with(|r| *r as *mut libc::c_void)
}

pub unsafe extern "C" fn sys_exit(code: i32) {
    println!("[mod] __syscall_exit({}): exiting", code);
    std::process::exit(code);
}

/// The mods' thread-local keys: small indices onto host keys.
static TLS_KEYS: std::sync::Mutex<Vec<crate::os::TlsKey>> = std::sync::Mutex::new(Vec::new());

fn tls_key(key: i32) -> Option<crate::os::TlsKey> {
    usize::try_from(key)
        .ok()
        .and_then(|i| TLS_KEYS.lock().unwrap().get(i).copied())
}

pub unsafe extern "C" fn sys_tls_create(
    key_out: *mut i32,
    dtor: Option<unsafe extern "C" fn(*mut libc::c_void)>,
) -> i32 {
    // The destructor is guest code: the host calls it directly only where guest code runs natively. A lifted runtime
    // needs a host destructor that calls it through guest::call (docs/windows-port.md, "Host calls guest").
    #[cfg(not(nx_native))]
    let dtor = {
        let _ = dtor;
        None
    };
    let Ok(k) = crate::os::tls_create(dtor) else {
        return -1;
    };
    let mut keys = TLS_KEYS.lock().unwrap();
    keys.push(k);
    if !key_out.is_null() {
        *key_out = (keys.len() - 1) as i32;
    }
    0
}

pub unsafe extern "C" fn sys_tls_delete(key: i32) {
    if let Some(k) = tls_key(key) {
        crate::os::tls_delete(k);
    }
}

pub unsafe extern "C" fn sys_tls_get(key: i32) -> *mut libc::c_void {
    tls_key(key).map_or(std::ptr::null_mut(), crate::os::tls_get)
}

pub unsafe extern "C" fn sys_tls_set(key: i32, val: *mut libc::c_void) -> i32 {
    tls_key(key).map_or(-1, |k| crate::os::tls_set(k, val).map_or(-1, |()| 0))
}

pub static SEAD_QUAT_UNIT: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
pub static SEAD_VEC3_ZERO: [f32; 4] = [0.0, 0.0, 0.0, 0.0];


#[cfg(test)]
mod newlib_tests {
    use super::*;

    #[test]
    fn locks_and_condition_variables() {
        unsafe {
            let mut m = [0u32; 3];
            sys_lock_acquire_recursive(m.as_mut_ptr());
            sys_lock_acquire_recursive(m.as_mut_ptr());
            assert_eq!(m[2], 2);
            sys_lock_release_recursive(m.as_mut_ptr());
            assert_ne!(m[0], 0);
            sys_lock_release_recursive(m.as_mut_ptr());
            assert_eq!(m, [0, 0, 0]);

            // A waiter wakes on broadcast; a timed wait without one times out, both with the lock held again.
            static mut LOCK: u32 = 0;
            static mut COND: u32 = 0;
            static mut FLAG: u32 = 0;
            let (lock, cond, flag) = (&raw mut LOCK, &raw mut COND, &raw mut FLAG);
            sys_lock_acquire(lock);
            assert_eq!(sys_cond_wait(cond, lock, 1_000_000), NEWLIB_ETIMEDOUT);
            assert_ne!(*lock, 0);
            let (lp, cp, fp) = (lock as usize, cond as usize, flag as usize);
            let t = std::thread::spawn(move || {
                sys_lock_acquire(lp as *mut u32);
                *(fp as *mut u32) = 1;
                sys_cond_broadcast(cp as *mut u32);
                sys_lock_release(lp as *mut u32);
            });
            while *flag == 0 {
                assert_eq!(sys_cond_wait(cond, lock, u64::MAX), 0);
            }
            sys_lock_release(lock);
            t.join().unwrap();
            assert_eq!(sys_lock_try_acquire(lock), 0);
            assert_eq!(sys_lock_try_acquire(lock), 1);
        }
    }
}
