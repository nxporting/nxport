// Ahead-of-time guest code images (for iOS, where an app cannot make memory executable unless it is code-signed).
//
// Startup patches the guest modules in ordinary memory exactly as on macOS (relocations, tpidr / x18 rewrites, SDK
// interposition, HLE trampolines). Every region that would then become executable is registered here instead of being
// mprotect'ed directly (`make_exec`). The patched bytes do not depend on where things land in a run (code refers to
// per-run addresses only through data words: `literal`), so they are the same on every launch:
//   crate::guest_image   builds the image in memory without running the game (nxport-cli wraps it in a signed dylib,
//                          section __TEXT,__guest); --aot-dump=<file> writes it from a run (with plugin mods, after
//                          they patched the code);
//   --aot-image=<dylib> loads that dylib (so the system registers its code signature), checks that each region's
//                          patched bytes equal the image's, and maps the image's pages over the region read+execute.
// Image layout: a 16 KiB header page ("NXAOTIM1", region count, then {name[48], offset u64, len u64} per region), a page
// listing the pages mods patch (count u64, then {region index u32, page index u32}), then each region's bytes at 16 KiB-
// aligned offsets.
//
// Mods (exlaunch plugins such as UltraCam) patch guest code through writable views of it (horizon_svc), from their
// initialisers and for a while after (a one-shot hook on nnMain that removes itself, more hooks over the first frames),
// and read back what they wrote. iOS maps signed code once: an iPhone refused to map a different signed page
// where signed code had been mapped before, even after unmapping it (EACCES, 2026-10-11), so the image holds one static
// state of that code and nothing is swapped while the game runs. The dump run forces the mods' views of image code into
// copy mode (writes reach the code only at flushes, at every map/unmap syscall) and records each content each page goes
// through (`record_page`); the image holds the code as it stands at the dump frame, plus every patch the mods removed
// again (the one-shot nnMain hook: nnMain runs once, so keeping it does no harm). A loading run maps that state from the
// start; the mods' initialisers still run (they set up the state their hooks use) and their code writes are checked
// against the image and dropped (horizon::svc::flush_one).

use std::sync::Mutex;

const PAGE: usize = 0x4000;
const MAGIC: &[u8; 8] = b"NXAOTIM1";

#[derive(Clone, PartialEq)]
pub enum Mode {
    Off,
    Dump(DumpTo),
    Load(String),
}

/// Where a dump run's image goes.
#[derive(Clone, PartialEq)]
pub enum DumpTo {
    File(String),
    /// Kept for `take_image` (crate::guest_image).
    Memory,
}

static MODE: std::sync::OnceLock<Mode> = std::sync::OnceLock::new();
static CAPTURED: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Makes this process build an image in memory instead of what the settings say; before anything asks for the mode.
pub fn capture_image() -> Result<(), String> {
    MODE.set(Mode::Dump(DumpTo::Memory))
        .map_err(|_| "the AOT mode is already set in this process".to_string())
}

/// The image a `DumpTo::Memory` run built.
pub fn take_image() -> Option<Vec<u8>> {
    CAPTURED.lock().unwrap().take()
}

pub fn mode() -> &'static Mode {
    MODE.get_or_init(|| {
        let g = &crate::settings::get().guest;
        match (&g.aot_image, &g.aot_dump) {
            (Some(p), _) => Mode::Load(p.display().to_string()),
            (None, Some(p)) => Mode::Dump(DumpTo::File(p.display().to_string())),
            (None, None) => Mode::Off,
        }
    })
}

pub fn loading() -> bool {
    matches!(mode(), Mode::Load(_))
}

/// Dumping or loading an image: mods' initialisers run around `finalize` (main.rs) instead of right before nnMain.
pub fn active() -> bool {
    !matches!(mode(), Mode::Off)
}

/// Copies of the registered regions before mods patch them (dump run, right before the mods' initialisers).
static PREMOD: Mutex<Vec<(usize, Vec<u8>)>> = Mutex::new(Vec::new());
/// Dump run: every distinct content of every 16 KiB page mods patched, in the order they appeared (page address,
/// bytes); a page's first entry is its unpatched code.
static VERSIONS: Mutex<Vec<(usize, Vec<u8>)>> = Mutex::new(Vec::new());

/// Snapshots every registered region's current bytes (see PREMOD).
pub fn snapshot_premod() {
    let r = REGIONS.lock().unwrap();
    *PREMOD.lock().unwrap() = r
        .iter()
        .map(|x| {
            (
                x.start,
                unsafe { std::slice::from_raw_parts(x.start as *const u8, x.len) }.to_vec(),
            )
        })
        .collect();
}

/// Whether [addr, addr+len) lies in image code (a registered region, or a region of the loaded image).
pub fn in_region(addr: usize, len: usize) -> bool {
    REGIONS
        .lock()
        .unwrap()
        .iter()
        .any(|r| addr >= r.start && addr + len <= r.start + r.len)
        || LOADED.lock().unwrap().as_ref().map_or(false, |l| {
            l.regions
                .iter()
                .any(|(_, s, n, _)| addr >= *s && addr + len <= s + n)
        })
}

/// Dump run with mods: a flush just wrote into the 16 KiB page at `page`; remembers its content if it is new.
pub fn record_page(page: usize) {
    if !(matches!(mode(), Mode::Dump(_)) && DEFER_DUMP.load(std::sync::atomic::Ordering::Relaxed)) {
        return;
    }
    let premod = PREMOD.lock().unwrap();
    let Some((s, b)) = premod
        .iter()
        .find(|(s, b)| page >= *s && page + PAGE <= s + b.len())
    else {
        return;
    };
    let mut v = VERSIONS.lock().unwrap();
    if !v.iter().any(|(p, _)| *p == page) {
        v.push((page, b[page - s..page - s + PAGE].to_vec()));
    }
    let now = unsafe { std::slice::from_raw_parts(page as *const u8, PAGE) };
    if !v.iter().any(|(p, c)| *p == page && c.as_slice() == now) {
        v.push((page, now.to_vec()));
    }
}

struct Region {
    name: String,
    start: usize,
    len: usize,
}

static REGIONS: Mutex<Vec<Region>> = Mutex::new(Vec::new());
static FINALIZED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Makes [start, start+len) read+execute (page-rounded), or, when loading an image, only records it: the memory stays
/// writable until `finalize` maps the signed copy over it. Re-registering a region (same start) replaces it.
pub unsafe fn make_exec(name: &str, start: usize, len: usize) -> i32 {
    let s = start & !(PAGE - 1);
    let e = (start + len + PAGE - 1) & !(PAGE - 1);
    {
        let mut r = REGIONS.lock().unwrap();
        r.retain(|x| x.start != s);
        r.push(Region {
            name: name.to_string(),
            start: s,
            len: e - s,
        });
    }
    if loading() {
        if FINALIZED.load(std::sync::atomic::Ordering::Relaxed) {
            // Too late: nothing can become executable once the image is mapped (on iOS this code would fault).
            println!("[!!] AOT: code region {} ({:#x}+{:#x}) generated after the image was mapped; it is not executable", name, s, e - s);
        }
        return 0;
    }
    libc::mprotect(
        s as *mut libc::c_void,
        e - s,
        libc::PROT_READ | libc::PROT_EXEC,
    )
}

/// mprotect read+execute for code that is patched again later (flipped RW / RX around each write): a no-op when
/// loading an image, where nothing becomes executable before `finalize`.
pub unsafe fn protect_rx(start: usize, len: usize) -> i32 {
    if loading() {
        return 0;
    }
    libc::mprotect(
        start as *mut libc::c_void,
        len,
        libc::PROT_READ | libc::PROT_EXEC,
    )
}

/// Literal pool for per-run values (host function addresses, data pointers) that patched code loads with
/// `adrp x16, page; ldr x16, [x16, #off]`: code never embeds them, so it stays the same on every launch. The pool is a
/// page of the generated-code area (guest::loader::gen_area), at a fixed distance from the guest modules.
static LITERALS: Mutex<(usize, usize)> = Mutex::new((0, 0));

pub fn set_literal_pool(base: usize, len: usize) {
    *LITERALS.lock().unwrap() = (base, len);
}

/// A new literal slot holding `value`; returns its address.
pub fn literal(value: usize) -> usize {
    let mut g = LITERALS.lock().unwrap();
    assert!(g.0 != 0 && g.1 >= 8, "AOT literal pool not set up or full");
    let a = g.0;
    unsafe { *(a as *mut usize) = value };
    g.0 += 8;
    g.1 -= 8;
    a
}

/// `adrp xd, target; ldr xd, [xd, #lo12(target)]` placed at `pc`: loads the 8-byte word at `target`.
pub fn adrp_ldr(pc: usize, target: usize, rd: u32) -> [u32; 2] {
    let pages = ((target & !0xfff) as i64 - (pc & !0xfff) as i64) >> 12;
    assert!(pages.abs() < (1 << 20), "adrp out of range");
    let lo = (pages & 3) as u32;
    let hi = ((pages >> 2) & 0x7ffff) as u32;
    let adrp = 0x9000_0000 | (lo << 29) | (hi << 5) | rd;
    let off = (target & 0xfff) as u32;
    assert!(off % 8 == 0, "unaligned literal");
    let ldr = 0xF940_0000 | ((off / 8) << 10) | (rd << 5) | rd;
    [adrp, ldr]
}

/// `ldr xt, <target>` (PC-relative literal load, +-1 MiB) placed at `pc`.
pub fn ldr_literal(pc: usize, target: usize, rt: u32) -> u32 {
    let d = target as i64 - pc as i64;
    assert!(d % 4 == 0 && d.abs() < (1 << 20), "literal out of range");
    0x5800_0000 | ((((d >> 2) as u32) & 0x7ffff) << 5) | rt
}

static DEFER_DUMP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Dump run with mods: `finalize` writes nothing; the image is written by `dump_after_mods` once the mods' initialisers
/// (run on the guest's main thread, main.rs) patched the code.
pub fn defer_dump() {
    DEFER_DUMP.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// Dump run with mods: the image is written at this presented frame (--aot-dump-frame, default 300). UltraCam keeps
/// patching code after its initialiser (111 code views in one run, the last before frame 50), so the image must hold
/// the code as it stands once the game is running.
pub fn deferred_dump_frame() -> Option<u64> {
    if !(matches!(mode(), Mode::Dump(_)) && DEFER_DUMP.load(std::sync::atomic::Ordering::Relaxed)) {
        return None;
    }
    Some(crate::settings::get().guest.aot_dump_frame)
}

/// Dump run with mods: writes the image with the page versions recorded so far.
pub unsafe fn dump_after_mods() -> Result<(), String> {
    let regions = std::mem::take(&mut *REGIONS.lock().unwrap());
    let Mode::Dump(to) = mode() else {
        return Ok(());
    };
    dump(to, &regions)
}

/// Called once all guest code is patched, right before any of it runs.
pub unsafe fn finalize() -> Result<(), String> {
    if matches!(mode(), Mode::Dump(_)) && DEFER_DUMP.load(std::sync::atomic::Ordering::Relaxed) {
        return Ok(()); // dump_after_mods
    }
    let regions = std::mem::take(&mut *REGIONS.lock().unwrap());
    let r = match mode() {
        Mode::Off => Ok(()),
        Mode::Dump(to) => dump(to, &regions),
        Mode::Load(path) => load(path, &regions),
    };
    FINALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
    if r.is_ok() && (loading() || crate::settings::get().guest.aot_audit) {
        audit_exec("after startup");
        // Again in play: code generated later (it would not run on iOS) shows up here.
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(90));
            audit_exec("90 s in");
        });
    }
    r
}

/// Lists executable memory that is not backed by a file. iOS refuses to run such pages without JIT; macOS does not
/// enforce it for ad-hoc signed binaries, so this audit is how an AOT run on the Mac shows it would be acceptable there.
/// Returns the number of anonymous executable bytes.
pub fn audit_exec(when: &str) -> usize {
    // vm_region_submap_info_64 is declared under #pragma pack(4) (mach/vm_region.h).
    #[repr(C, packed(4))]
    #[derive(Default)]
    struct SubmapInfo64 {
        protection: i32,
        max_protection: i32,
        inheritance: u32,
        offset: u64,
        user_tag: u32,
        pages_resident: u32,
        pages_shared_now_private: u32,
        pages_swapped_out: u32,
        pages_dirtied: u32,
        ref_count: u32,
        shadow_depth: u16,
        external_pager: u8,
        share_mode: u8,
        is_submap: i32,
        behavior: i32,
        object_id: u32,
        user_wired_count: u16,
        pages_reusable: u32,
        object_id_full: u64,
    }
    extern "C" {
        static mach_task_self_: u32;
        fn mach_vm_region_recurse(
            task: u32,
            addr: *mut u64,
            size: *mut u64,
            depth: *mut u32,
            info: *mut SubmapInfo64,
            count: *mut u32,
        ) -> i32;
    }
    let mut addr = 0u64;
    let mut depth = 0u32;
    let (mut anon, mut file) = (0usize, 0usize);
    let mut shown = 0;
    loop {
        let mut size = 0u64;
        let mut info = SubmapInfo64::default();
        let mut count = (std::mem::size_of::<SubmapInfo64>() / 4) as u32;
        if unsafe {
            mach_vm_region_recurse(
                mach_task_self_,
                &mut addr,
                &mut size,
                &mut depth,
                &mut info,
                &mut count,
            )
        } != 0
        {
            break;
        }
        if info.is_submap != 0 {
            depth += 1;
            continue;
        }
        if info.protection & 4 != 0 {
            if info.external_pager != 0 {
                file += size as usize;
            } else {
                anon += size as usize;
                if shown < 8 {
                    println!(
                        "[!] AOT audit: anonymous executable memory at {:#x}+{:#x} (tag {})",
                        addr, size, info.user_tag
                    );
                    shown += 1;
                }
            }
        }
        addr += size;
    }
    println!(
        "[+] AOT audit ({}): executable memory {} MB file-backed, {} KB anonymous{}",
        when,
        file >> 20,
        anon >> 10,
        if anon == 0 {
            " (OK for iOS)"
        } else {
            " (iOS would refuse to run it)"
        }
    );
    anon
}

/// The static state of a page mods patched: its content at the dump, plus every patched byte they later restored to
/// the unpatched value (taken from the last version that had it patched).
fn merged_page(versions: &[&Vec<u8>], now: &[u8]) -> Vec<u8> {
    let first = versions[0];
    let mut out = now.to_vec();
    for i in 0..PAGE {
        if now[i] == first[i] {
            if let Some(v) = versions.iter().rev().find(|v| v[i] != first[i]) {
                out[i] = v[i];
            }
        }
    }
    out
}

fn dump(to: &DumpTo, regions: &[Region]) -> Result<(), String> {
    let versions = VERSIONS.lock().unwrap();
    let mut pages: Vec<usize> = versions.iter().map(|(p, _)| *p).collect();
    pages.sort();
    pages.dedup();
    if 8 + pages.len() * 8 > PAGE {
        return Err(format!(
            "{} mod-patched pages do not fit the image header",
            pages.len()
        ));
    }
    let head = 2 * PAGE;
    let mut header = vec![0u8; head];
    header[..8].copy_from_slice(MAGIC);
    header[8..16].copy_from_slice(&(regions.len() as u64).to_le_bytes());
    header[PAGE..PAGE + 8].copy_from_slice(&(pages.len() as u64).to_le_bytes());
    let mut body = Vec::new();
    let mut kept = 0;
    let mut k = 0;
    for (i, r) in regions.iter().enumerate() {
        let e = 16 + i * 64;
        assert!(e + 64 <= PAGE && r.name.len() < 48);
        header[e..e + r.name.len()].copy_from_slice(r.name.as_bytes());
        header[e + 48..e + 56].copy_from_slice(&((head + body.len()) as u64).to_le_bytes());
        header[e + 56..e + 64].copy_from_slice(&(r.len as u64).to_le_bytes());
        let start = body.len();
        body.extend_from_slice(unsafe { std::slice::from_raw_parts(r.start as *const u8, r.len) });
        for &page in pages
            .iter()
            .filter(|&&p| p >= r.start && p < r.start + r.len)
        {
            let vs: Vec<&Vec<u8>> = versions
                .iter()
                .filter(|(p, _)| *p == page)
                .map(|(_, c)| c)
                .collect();
            let at = start + (page - r.start);
            let m = merged_page(&vs, &body[at..at + PAGE]);
            kept += m
                .iter()
                .zip(&body[at..at + PAGE])
                .filter(|(a, b)| a != b)
                .count();
            body[at..at + PAGE].copy_from_slice(&m);
            let o = PAGE + 8 + k * 8;
            header[o..o + 4].copy_from_slice(&(i as u32).to_le_bytes());
            header[o + 4..o + 8].copy_from_slice(&(((page - r.start) / PAGE) as u32).to_le_bytes());
            k += 1;
        }
    }
    if !pages.is_empty() {
        println!("[+] AOT: mods patched {} code pages ({} versions; {} bytes of patches they removed again are kept)", pages.len(), versions.len(), kept);
    }
    drop(versions);
    header.extend_from_slice(&body);
    for r in regions {
        println!("[+] AOT: {:<20} {:#x}+{:#x}", r.name, r.start, r.len);
    }
    let size = header.len();
    match to {
        DumpTo::File(path) => {
            std::fs::write(path, &header).map_err(|e| format!("writing {}: {}", path, e))?;
            println!(
                "[+] AOT: wrote {} regions ({} MB) to {}",
                regions.len(),
                size >> 20,
                path
            );
        }
        DumpTo::Memory => *CAPTURED.lock().unwrap() = Some(header),
    }
    Ok(())
}

/// File offset of section `sect` in a Mach-O file.
fn section_offset(file: &[u8], sect: &str) -> Option<usize> {
    let u32_at = |o: usize| u32::from_le_bytes(file[o..o + 4].try_into().unwrap());
    if u32_at(0) != 0xfeed_facf {
        return None;
    }
    let ncmds = u32_at(16) as usize;
    let mut p = 32;
    for _ in 0..ncmds {
        let (cmd, size) = (u32_at(p), u32_at(p + 4) as usize);
        if cmd == 0x19 {
            // LC_SEGMENT_64: sections follow the 72-byte command, 80 bytes each.
            let nsects = u32_at(p + 64) as usize;
            for i in 0..nsects {
                let s = p + 72 + i * 80;
                let name = &file[s..s + 16];
                let n = name.iter().position(|&b| b == 0).unwrap_or(16);
                if &name[..n] == sect.as_bytes() {
                    return Some(u32_at(s + 48) as usize);
                }
            }
        }
        p += size;
    }
    None
}

unsafe fn load(path: &str, regions: &[Region]) -> Result<(), String> {
    // dlopen first: that is what registers the dylib's code signature with the kernel, so its pages may be mapped
    // executable (iOS); the copy dyld maps is not used.
    let c = std::ffi::CString::new(path.as_bytes()).unwrap();
    if libc::dlopen(c.as_ptr(), libc::RTLD_NOW).is_null() {
        let e = std::ffi::CStr::from_ptr(libc::dlerror())
            .to_string_lossy()
            .into_owned();
        return Err(format!("dlopen {}: {}", path, e));
    }
    let file = std::fs::read(path).map_err(|e| format!("reading {}: {}", path, e))?;
    let sect =
        section_offset(&file, "__guest").ok_or("no __TEXT,__guest section in the AOT image")?;
    if sect % PAGE != 0 || &file[sect..sect + 8] != MAGIC {
        return Err("AOT image section is not a 16 KiB-aligned guest image (rebuild it)".into());
    }
    let img = &file[sect..];
    let count = u64::from_le_bytes(img[8..16].try_into().unwrap()) as usize;
    let mut entries = std::collections::HashMap::new();
    for i in 0..count {
        let e = 16 + i * 64;
        let n = img[e..e + 48].iter().position(|&b| b == 0).unwrap_or(48);
        let name = String::from_utf8_lossy(&img[e..e + n]).into_owned();
        let off = u64::from_le_bytes(img[e + 48..e + 56].try_into().unwrap()) as usize;
        let len = u64::from_le_bytes(img[e + 56..e + 64].try_into().unwrap()) as usize;
        entries.insert(name, (off, len));
    }
    // Pages mods patch: this run has not run the mods yet, so they are not checked here (horizon_svc checks the mods'
    // writes against the image).
    let npatched = u64::from_le_bytes(img[PAGE..PAGE + 8].try_into().unwrap()) as usize;
    let mut skip: std::collections::HashSet<(usize, usize)> = std::collections::HashSet::new();
    for k in 0..npatched.min((PAGE - 8) / 8) {
        let o = PAGE + 8 + k * 8;
        let idx = u32::from_le_bytes(img[o..o + 4].try_into().unwrap()) as usize;
        let page = u32::from_le_bytes(img[o + 4..o + 8].try_into().unwrap()) as usize;
        skip.insert((idx, page));
    }
    let order: Vec<String> = (0..count)
        .map(|i| {
            let e = 16 + i * 64;
            let n = img[e..e + 48].iter().position(|&b| b == 0).unwrap_or(48);
            String::from_utf8_lossy(&img[e..e + n]).into_owned()
        })
        .collect();
    let skip: std::collections::HashSet<(String, usize)> = skip
        .into_iter()
        .filter_map(|(i, p)| order.get(i).map(|n| (n.clone(), p)))
        .collect();
    if entries.len() != regions.len() {
        return Err(format!(
            "AOT image has {} regions, this run patched {}: rebuild the image",
            entries.len(),
            regions.len()
        ));
    }
    // Check every region before mapping any: a stale image must not run.
    let mut bad = Vec::new();
    for r in regions {
        let Some(&(off, len)) = entries.get(&r.name) else {
            return Err(format!("AOT image lacks region {}", r.name));
        };
        if len != r.len {
            return Err(format!(
                "AOT region {}: image {:#x} bytes, this run {:#x}: rebuild the image",
                r.name, len, r.len
            ));
        }
        let mem = std::slice::from_raw_parts(r.start as *const u8, r.len);
        let pages: Vec<usize> = (0..len / PAGE)
            .filter(|&p| {
                !skip.contains(&(r.name.clone(), p))
                    && mem[p * PAGE..(p + 1) * PAGE] != img[off + p * PAGE..off + (p + 1) * PAGE]
            })
            .collect();
        if !pages.is_empty() {
            let first = pages[0] * PAGE;
            let at = (0..PAGE)
                .find(|&i| mem[first + i] != img[off + first + i])
                .unwrap_or(0);
            bad.push(format!(
                "{}: {} of {} pages differ (first at +{:#x})",
                r.name,
                pages.len(),
                len / PAGE,
                first + at
            ));
        }
    }
    if !bad.is_empty() {
        return Err(format!(
            "AOT image does not match this run's patched code (rebuild it): {}",
            bad.join("; ")
        ));
    }
    let fd = libc::open(c.as_ptr(), libc::O_RDONLY);
    if fd < 0 {
        return Err(format!(
            "open {}: {}",
            path,
            std::io::Error::last_os_error()
        ));
    }
    // The image stays mapped read-only (file-backed) for image_bytes.
    let view = libc::mmap(
        std::ptr::null_mut(),
        file.len(),
        libc::PROT_READ,
        libc::MAP_PRIVATE,
        fd,
        0,
    );
    if view == libc::MAP_FAILED {
        return Err(format!(
            "mapping {}: {}",
            path,
            std::io::Error::last_os_error()
        ));
    }
    *LOADED.lock().unwrap() = Some(Loaded {
        regions: regions
            .iter()
            .map(|r| (r.name.clone(), r.start, r.len, entries[&r.name].0))
            .collect(),
    });
    for r in regions {
        let (off, _) = entries[&r.name];
        let p = libc::mmap(
            r.start as *mut libc::c_void,
            r.len,
            libc::PROT_READ | libc::PROT_EXEC,
            libc::MAP_FIXED | libc::MAP_PRIVATE,
            fd,
            (sect + off) as libc::off_t,
        );
        if p == libc::MAP_FAILED || p as usize != r.start {
            return Err(format!(
                "mapping AOT region {} at {:#x}: {}",
                r.name,
                r.start,
                std::io::Error::last_os_error()
            ));
        }
        println!(
            "[+] AOT: {:<20} {:#x}+{:#x} mapped from the signed image",
            r.name, r.start, r.len
        );
    }
    if npatched > 0 {
        println!(
            "[+] AOT: {} code pages hold the mods' patches from the start",
            npatched
        );
    }
    libc::close(fd);
    Ok(())
}

/// A loaded image: its read-only view and regions (name, start, len, image offset).
struct Loaded {
    regions: Vec<(String, usize, usize, usize)>,
}
static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);
