// Guest code -> C through nxport-lifter, for hosts without the native runtime (docs/windows-port.md).
//
// lift_game lifts what the runtime will run: the patched image (relocations, ExeFS patches, SDK interposition, game
// hooks; nxport::guest_modules). lift_nso lifts a module as its NSO file holds it. Function starts are seeded from
// .eh_frame_hdr, the dynamic symbol table's functions and the code pointers R_AARCH64_RELATIVE relocations store in
// data; the lifter adds call and branch targets.

use std::collections::BTreeSet;
use std::path::Path;

use nxport_lifter::{ehframe, lift_module, Module, Options, Overlay};
pub use nxport_lifter::Output;

const R_AARCH64_RELATIVE: u64 = 1027;
const STT_FUNC: u8 = 2;

/// Lifts the NSO at `path`, loaded at guest address `base`, into `out_dir` (C files and the runtime files).
pub fn lift_nso(path: &Path, name: &str, base: u64, out_dir: &Path) -> Result<Output, Box<dyn std::error::Error>> {
    let n = nxport::nso::Nso::read(path)?;
    let mut img = vec![0u8; n.image_size()];
    for s in [&n.text, &n.ro, &n.data] {
        img[s.mem_off..s.mem_off + s.data.len()].copy_from_slice(&s.data);
    }
    let text = base + n.text.mem_off as u64..base + (n.text.mem_off + n.text.data.len()) as u64;
    let out = lift_image(name, base, &img, text, n.dynamic().ok(), BTreeSet::new(), None);
    write_files(out_dir, &[&out])?;
    Ok(out)
}

/// Lifts the game in `s.files.game_dir`, loaded and patched as the runtime loads it, into `out_dir`: the C files of each
/// module and the runtime files. Returns each module's name and what was lifted, in the order the runtime registers
/// them (lifted::GuestLibrary::load). `on_module(i, n, name)` is told before each of the n modules is lifted.
/// `patched`: the image a run with plugin mods dumped (`--aot-dump`), whose code replaces the load's (nxport::guest_modules).
#[cfg(any(target_vendor = "apple", windows))]
pub fn lift_game(
    s: nxport::settings::Settings,
    patched: Option<&Path>,
    out_dir: &Path,
    on_module: &dyn Fn(usize, usize, &str),
) -> Result<Vec<(String, Output)>, Box<dyn std::error::Error>> {
    let mut lifted = Vec::new();
    let image = patched.map(|p| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))).transpose()?;
    let modules = nxport::guest_modules(s, image.as_deref())?;
    let seeds = cross_module_seeds(&modules);
    let n = modules.len();
    for ((i, m), seeds) in modules.into_iter().enumerate().zip(seeds) {
        on_module(i, n, &m.name);
        let dynamic = m.dynamic.then(|| nxport::nso::image_dynamic(&m.image).ok()).flatten();
        let out = lift_image(&m.name, m.base, &m.image, m.text, dynamic, seeds, None);
        lifted.push((m.name, out));
    }
    write_files(out_dir, &lifted.iter().map(|(_, o)| o).collect::<Vec<_>>())?;
    let names: String = lifted.iter().map(|(n, _)| format!("{n}\n")).collect();
    std::fs::write(out_dir.join(MODULES_FILE), names)?;
    Ok(lifted)
}

/// The code a guest library was lifted from, beside it: what lift_overlay compares against.
pub const BASE_CODE_FILE: &str = "base_code.bin";

/// Writes the code of the game loaded with `s`, as lift_game would lift it, for lift_overlay.
#[cfg(any(target_vendor = "apple", windows))]
pub fn write_base_code(s: nxport::settings::Settings, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = Vec::new();
    for m in nxport::guest_modules(s, None)? {
        let code = &m.image[(m.text.start - m.base) as usize..(m.text.end - m.base) as usize];
        f.extend_from_slice(&(m.name.len() as u64).to_le_bytes());
        f.extend_from_slice(m.name.as_bytes());
        f.extend_from_slice(&m.text.start.to_le_bytes());
        f.extend_from_slice(&(code.len() as u64).to_le_bytes());
        f.extend_from_slice(code);
    }
    std::fs::write(out, f)?;
    Ok(())
}

/// (module, code start, code) of a base_code.bin.
#[cfg(windows)]
fn read_base_code(path: &Path) -> Result<Vec<(String, u64, Vec<u8>)>, Box<dyn std::error::Error>> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (mut p, mut out) = (0, Vec::new());
    let u64_at = |o: usize| u64::from_le_bytes(d[o..o + 8].try_into().unwrap());
    while p < d.len() {
        let n = u64_at(p) as usize;
        let name = String::from_utf8_lossy(&d[p + 8..p + 8 + n]).into_owned();
        p += 8 + n;
        let (start, len) = (u64_at(p), u64_at(p + 8) as usize);
        out.push((name, start, d[p + 16..p + 16 + len].to_vec()));
        p += 16 + len;
    }
    Ok(out)
}

/// Lifts what mods change against `base_library` (a guest library of the game without them, its base_code.bin beside
/// it), into `out_dir`: the modules the base does not have, whole, and of those it has only the functions that differ
/// (as `ov_<module>`). The engine points the base's functions at these (--guest-overlay). `patched` as lift_game's.
#[cfg(windows)]
pub fn lift_overlay(
    s: nxport::settings::Settings,
    patched: Option<&Path>,
    base_library: &Path,
    out_dir: &Path,
) -> Result<Vec<(String, Output)>, Box<dyn std::error::Error>> {
    let run_game_subsdk = s.guest.run_game_subsdk;
    let base = read_base_code(&base_library.with_file_name(BASE_CODE_FILE))?;
    let library = nxport::os::Library::open(base_library)?;
    let image = patched.map(|p| std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))).transpose()?;
    let modules = nxport::guest_modules(s, image.as_deref())?;
    let seeds = cross_module_seeds(&modules);
    let mut lifted = Vec::new();
    for (m, mut seeds) in modules.into_iter().zip(seeds) {
        // The game's own subsdk modules are in the map only for the module indices mods count by; nothing runs them.
        if m.name.starts_with("game_subsdk") && !run_game_subsdk {
            continue;
        }
        let dynamic = m.dynamic.then(|| nxport::nso::image_dynamic(&m.image).ok()).flatten();
        let Some((_, start, code)) = base.iter().find(|(n, ..)| *n == m.name) else {
            let out = lift_image(&m.name, m.base, &m.image, m.text, dynamic, seeds, None);
            lifted.push((m.name, out));
            continue;
        };
        let now = &m.image[(m.text.start - m.base) as usize..(m.text.end - m.base) as usize];
        if *start != m.text.start || code.len() != now.len() {
            return Err(format!("{}: the base library holds other code ({start:#x}+{:#x}, now {:#x}+{:#x})", m.name, code.len(), m.text.start, now.len()).into());
        }
        let changed: Vec<u64> = now.chunks(4).zip(code.chunks(4)).enumerate().filter(|(_, (a, b))| a != b).map(|(i, _)| *start + i as u64 * 4).collect();
        let count = library.symbol(&format!("{}_fn_count", m.name));
        let addrs = library.symbol(&format!("{}_fn_addrs", m.name));
        if count.is_null() || addrs.is_null() {
            return Err(format!("{}: {} has no table for it", m.name, base_library.display()).into());
        }
        let base_starts: BTreeSet<u64> = unsafe { std::slice::from_raw_parts(addrs as *const u64, *(count as *const u64) as usize) }.iter().copied().collect();
        if changed.is_empty() && seeds.iter().all(|a| !m.text.contains(a) || base_starts.contains(a)) {
            println!("[+] {}: same as the base", m.name);
            continue;
        }
        println!("[+] {}: {} instructions differ from the base", m.name, changed.len());
        seeds.extend(base_starts.iter().copied());
        let name = format!("ov_{}", m.name);
        let out = lift_image(&name, m.base, &m.image, m.text, dynamic, seeds, Some(Overlay { base_starts, changed }));
        lifted.push((name, out));
    }
    write_files(out_dir, &lifted.iter().map(|(_, o)| o).collect::<Vec<_>>())?;
    let names: String = lifted.iter().map(|(n, _)| format!("{n}\n")).collect();
    std::fs::write(out_dir.join(MODULES_FILE), names)?;
    Ok(lifted)
}

/// Prints what was lifted of `what` (a module), with the most frequent reasons instructions were not lifted.
pub fn report(what: &str, r: &Output) {
    println!(
        "[+] {}: {} functions, {} instructions, {} jump tables, {} unresolved computed branches, {} C files",
        what,
        r.functions,
        r.instructions,
        r.jump_tables,
        r.unresolved_computed,
        r.files.len()
    );
    let mut unsupported: Vec<_> = r.unsupported.iter().collect();
    unsupported.sort_by(|a, b| b.1.cmp(a.1));
    for (reason, n) in unsupported.iter().take(20) {
        println!("    {n:>9} not lifted: {reason}");
    }
}

/// The modules lift_game lifted, one name per line, in the directory of the C files and next to the library compiled
/// from them (the runtime registers their tables by these names).
pub const MODULES_FILE: &str = "modules.txt";

/// Compiles the C files in `c_dir` (lift_game's or lift_nso's) into the guest library `out` (a DLL / shared library),
/// running `cc` (a C compiler command line: clang, or `zig cc`) on `jobs` files at once. The objects stay in
/// `c_dir/obj` and are reused while newer than their source. `profile`: an LLVM profile to optimise by; it matches
/// functions by name and control flow, so one the C has changed, or does not have, is just built without. `on_file(done,
/// total)` is told as each file is done (compiled or reused), from the compiling threads.
pub fn compile(
    c_dir: &Path,
    out: &Path,
    cc: &[String],
    jobs: usize,
    profile: Option<&Path>,
    on_file: &(dyn Fn(usize, usize) + Sync),
) -> Result<(), Box<dyn std::error::Error>> {
    let (prog, pre) = cc.split_first().ok_or("no C compiler")?;
    // The x86-64-v3 features, spelled out: above all hardware FMA, which the lifted floating-point code needs to round
    // like the guest's. zig cc builds any of -O1 to -O3 as clang -O2, tunes for generic x86-64 once any -m flag is given,
    // and keeps frame pointers (a register) unless told not to.
    // -grow-region-complexity-budget: a few lifted functions of megabytes of C spent most of a build in the register
    // allocator's region splitting (TotK main_033: 923 s against 411 s with the budget at 100; the whole build about 19
    // minutes against 13.4). The same scene ran at the same speed (99.4 and 99.3 fps against 100.1 and 100.2).
    let flags = [
        "-O1", "-g0", "-fno-strict-aliasing", "-fomit-frame-pointer", "-mavx2", "-mfma", "-mbmi", "-mbmi2", "-mlzcnt",
        "-mmovbe", "-mf16c", "-mpopcnt", "-msse4.2", "-mcx16", "-mllvm", "-grow-region-complexity-budget=100",
    ];
    // The profile's hash is a flag too: zig's cache of objects need not look inside a file the flags name.
    let profile = profile.map(std::path::absolute).transpose()?;
    let profile_flags: Vec<String> = match &profile {
        Some(p) => {
            use sha2::Digest;
            let hash = sha2::Sha256::digest(std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?);
            let hash: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
            vec![format!("-fprofile-use={}", p.display()), format!("-DNX_PROFILE=0x{hash}")]
        }
        None => Vec::new(),
    };
    let c_dir_full = std::path::absolute(c_dir)?;
    outside_git(c_dir)?;
    outside_git(out.parent().unwrap_or(Path::new(".")))?;
    let obj_dir = c_dir.join("obj");
    std::fs::create_dir_all(&obj_dir)?;
    let mut sources: Vec<_> = std::fs::read_dir(c_dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "c"))
        .collect();
    // Largest first, so the long compiles do not all start last.
    sources.sort_by_key(|p| std::cmp::Reverse(p.metadata().map_or(0, |m| m.len())));
    let objects: Vec<_> = sources.iter().map(|s| obj_dir.join(s.file_name().unwrap()).with_extension("o")).collect();
    // Every C file includes the runtime's headers (lift_game rewrites them only when they change): an object older than
    // the newest of them is compiled again too.
    let headers = std::fs::read_dir(c_dir)?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "h"))
        .filter_map(|e| e.metadata().and_then(|m| m.modified()).ok())
        .max();
    // Objects compiled with other flags (and the compiler they came from) are compiled again: flags.txt holds them
    // (none: objects from before it, taken as they are).
    let stamp = obj_dir.join("flags.txt");
    let stamp_text = format!("{}\n{}\n{}\n", cc.join(" "), flags.join(" "), profile_flags.join(" "));
    let flags_changed = std::fs::read_to_string(&stamp).is_ok_and(|t| t != stamp_text);
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let finished = || on_file(done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1, sources.len());
    let failed = std::sync::Mutex::new(Vec::new());
    let unmatched = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..jobs.max(1) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let (Some(src), Some(obj)) = (sources.get(i), objects.get(i)) else { break };
                let newer = |a: &Path, b: &Path| -> bool {
                    match (a.metadata().and_then(|m| m.modified()), b.metadata().and_then(|m| m.modified())) {
                        (Ok(a), Ok(b)) => a > b,
                        _ => true,
                    }
                };
                let stale_headers = match (headers, obj.metadata().and_then(|m| m.modified())) {
                    (Some(h), Ok(o)) => h > o,
                    _ => false,
                };
                if obj.is_file() && !newer(src, obj) && !stale_headers && !flags_changed {
                    finished();
                    continue;
                }
                // In the C directory, by the file's bare name: a profile names static functions by the path they were
                // compiled from, and one made on another machine must find them.
                let r = std::path::absolute(obj).and_then(|obj| {
                    command(prog)
                        .current_dir(&c_dir_full)
                        .args(pre)
                        .args(flags)
                        .args(&profile_flags)
                        .arg("-c")
                        .arg(src.file_name().unwrap())
                        .arg("-o")
                        .arg(obj)
                        .output()
                });
                match r {
                    Ok(o) if o.status.success() => {
                        let warnings = String::from_utf8_lossy(&o.stderr);
                        unmatched.fetch_add(warnings.matches("(hash mismatch)").count(), std::sync::atomic::Ordering::Relaxed);
                    }
                    Ok(o) => failed.lock().unwrap().push(format!("{}: {}", src.display(), String::from_utf8_lossy(&o.stderr))),
                    Err(e) => failed.lock().unwrap().push(format!("{}: {e}", src.display())),
                }
                finished();
            });
        }
    });
    let failed = failed.into_inner().unwrap();
    if let Some(f) = failed.first() {
        return Err(format!("{} of {} C files failed to compile; the first: {f}", failed.len(), sources.len()).into());
    }
    if let Some(p) = &profile {
        println!("  Optimised by {}: {} functions changed since it was made, built without it", p.display(), unmatched.into_inner());
    }
    std::fs::write(&stamp, &stamp_text)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // A response file: hundreds of object paths overflow a Windows command line.
    let rsp = obj_dir.join("link.rsp");
    let quoted: String = objects.iter().map(|o| format!("\"{}\"\n", o.display().to_string().replace('\\', "/"))).collect();
    std::fs::write(&rsp, quoted)?;
    let o = command(prog)
        .args(pre)
        .arg("-shared")
        .arg("-o")
        .arg(out)
        .arg(format!("@{}", rsp.display()))
        .output()?;
    if !o.status.success() {
        return Err(format!("linking {}: {}", out.display(), String::from_utf8_lossy(&o.stderr)).into());
    }
    // Not onto itself when the library goes in the C directory (Windows refuses the copy).
    let (modules, dest) = (c_dir.join(MODULES_FILE), out.with_file_name(MODULES_FILE));
    if modules.is_file() && std::fs::canonicalize(&modules).ok() != std::fs::canonicalize(&dest).ok() {
        std::fs::copy(&modules, dest)?;
    }
    Ok(())
}

/// Downloads the gzipped file at `url` to `dest`, unpacked. A failure leaves nothing at `dest`.
pub fn fetch_gzipped(url: &str, dest: &Path) -> Result<(), String> {
    let dir = dest.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let gz = dest.with_extension("gz.download");
    let tmp = dest.with_extension("download");
    let result = (|| {
        let got = command(CURL)
            .args(["-fsL", "--retry", "2", "--max-time", "300", "-o"])
            .arg(&gz)
            .arg(url)
            .status()
            .map_err(|e| format!("curl: {e}"))?;
        if !got.success() {
            return Err(format!("{url}: not available"));
        }
        let file = std::fs::File::open(&gz).map_err(|e| e.to_string())?;
        let mut unpacked = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        std::io::copy(&mut flate2::read::GzDecoder::new(file), &mut unpacked).map_err(|e| format!("{url}: {e}"))?;
        drop(unpacked);
        std::fs::rename(&tmp, dest).map_err(|e| e.to_string())
    })();
    let _ = std::fs::remove_file(&gz);
    let _ = std::fs::remove_file(&tmp);
    result
}

/// What identifies the lifted C in `c_dir`, which is what a profile is made for: the names and contents of its C files
/// and headers, hashed.
pub fn lifted_hash(c_dir: &Path) -> Result<String, String> {
    use sha2::Digest;
    let mut files: Vec<_> = std::fs::read_dir(c_dir)
        .map_err(|e| format!("{}: {e}", c_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "c" || e == "h"))
        .collect();
    files.sort();
    let mut hash = sha2::Sha256::new();
    for f in files {
        hash.update(f.file_name().unwrap_or_default().as_encoded_bytes());
        hash.update(std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?);
    }
    Ok(hash.finalize().iter().take(8).map(|b| format!("{b:02x}")).collect())
}

/// The curl program: shipped with Windows (System32) since Windows 10 1803, and with macOS (/usr/bin).
pub const CURL: &str = if cfg!(windows) { "curl.exe" } else { "curl" };

/// A command for a tool run in the background: on Windows without a console window of its own (a builder started
/// from Explorer has none to share, and compile runs hundreds of compilers).
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut c = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// The settings a game in `dump` is lifted with (and must later run with: patches and hooks depend on them): the
/// platform's and the game's profiles, then `edits`, with the mods in `mods` (their ExeFS patches change the code it
/// runs), none without.
pub fn game_settings(dump: &Path, edits: nxport::settings::SettingsPatch, mods: Option<&Path>) -> nxport::settings::Settings {
    use nxport::settings::{profiles, SettingsPatch};
    let mut layers = vec![profiles::platform(profiles::PLATFORM)];
    if let Some(p) = nxport::profile::identify(dump).profile {
        layers.extend(nxport::profile::layers(p, profiles::PLATFORM));
    }
    layers.push(edits);
    let mut env = SettingsPatch::default();
    env.files.game_dir = Some(dump.to_path_buf());
    match mods {
        Some(m) => env.files.mods_dir = Some(m.to_path_buf()),
        None => env.files.no_mods = Some(true),
    }
    env.display.headless = Some(true);
    layers.push(env);
    nxport::settings::resolve(layers)
}

/// Refuses `dir` inside a git work tree. Lifted C, objects and guest libraries are the game's code: they must never land
/// where they could be committed (nxport's own checkout above all).
fn outside_git(dir: &Path) -> std::io::Result<()> {
    let mut at = std::path::absolute(dir)?;
    // The nearest existing ancestor: the directory may not be created yet.
    while !at.exists() && at.pop() {}
    // Some mounted file systems cannot give a final path; `at` is absolute already.
    let at = std::fs::canonicalize(&at).unwrap_or(at);
    if let Some(repo) = at.ancestors().find(|a| a.join(".git").exists()) {
        return Err(std::io::Error::other(format!(
            "{} is inside the git work tree {}: game code must not be written there",
            dir.display(),
            repo.display().to_string().trim_start_matches(r"\\?\")
        )));
    }
    Ok(())
}

/// Writes the C files, leaving those whose contents did not change untouched: compile reuses their objects.
fn write_files(out_dir: &Path, outputs: &[&Output]) -> std::io::Result<()> {
    outside_git(out_dir)?;
    std::fs::create_dir_all(out_dir)?;
    let write = |file: &str, body: &str| -> std::io::Result<()> {
        let path = out_dir.join(file);
        if std::fs::read(&path).is_ok_and(|old| old == body.as_bytes()) {
            return Ok(());
        }
        std::fs::write(path, body)
    };
    for (file, body) in nxport_lifter::RUNTIME_FILES {
        write(file, body)?;
    }
    for (file, body) in outputs.iter().flat_map(|o| &o.files) {
        write(file, body)?;
    }
    Ok(())
}

/// Lifts module `name`: `img` is its memory from guest address `base`, `text` its code; `dynamic` its dynamic tables,
/// when it has them, which name its functions.
/// Function starts each module needs for the others (in `modules` order). Lifted code enters another module's code only
/// at its function starts (the runtime looks branches up by address), and plugin mods connect to the game by jumping
/// between modules: hooks patched over a game function branch into the mod, and the mod's trampolines run a hooked
/// function's first instructions and jump back into its middle. So: every direct branch (B / BL) into another module,
/// and, in the mods' modules and the code regions (where trampolines keep their targets as literals), every 8-byte
/// value that points at code.
#[cfg(any(target_vendor = "apple", windows))]
fn cross_module_seeds(modules: &[nxport::GuestModule]) -> Vec<BTreeSet<u64>> {
    let mut seeds = vec![BTreeSet::new(); modules.len()];
    let owner = |a: u64| modules.iter().position(|m| m.text.contains(&a));
    for (i, m) in modules.iter().enumerate() {
        let word = |a: u64| {
            let o = (a - m.base) as usize;
            m.image.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        };
        let mut a = m.text.start & !3;
        while a + 4 <= m.text.end {
            if let Some(w) = word(a) {
                // B / BL: imm26 words from the branch.
                if w & 0x7c00_0000 == 0x1400_0000 {
                    let t = a.wrapping_add(((((w & 0x03ff_ffff) << 6) as i32 >> 4) as i64) as u64);
                    if let Some(j) = owner(t).filter(|&j| j != i) {
                        seeds[j].insert(t);
                    }
                }
            }
            a += 4;
        }
        if i == 0 || m.name == "sdk" {
            continue; // the game's own modules: .eh_frame and relocations name their functions
        }
        // The code a mod wrote at run time (a lift with its image): its trampolines, entered through addresses its .bss
        // holds. Every instruction of it.
        for r in &m.patched {
            seeds[i].extend((r.start & !3..r.end).step_by(4).filter(|a| m.text.contains(a)));
        }
        // Addresses of code the module computes (ADR, ADRP + ADD): thread entries and callbacks a mod hands the system
        // or its hooks.
        let mut a = m.text.start & !3;
        while a + 4 <= m.text.end {
            if let Some(w) = word(a) {
                let imm21 = || ((((w >> 5) & 0x7ffff) << 2 | (w >> 29) & 3) as i64) << 43 >> 43;
                let target = if w & 0x9f00_0000 == 0x1000_0000 {
                    Some(a.wrapping_add(imm21() as u64))
                } else if w & 0x9f00_0000 == 0x9000_0000 {
                    let page = (a & !0xfff).wrapping_add((imm21() << 12) as u64);
                    // The ADD (64-bit immediate) that completes it, within the next few instructions.
                    (1..=4).filter_map(|k| word(a + 4 * k)).find_map(|n| {
                        (n & 0xff80_0000 == 0x9100_0000 && (n >> 5) & 31 == w & 31)
                            .then(|| page.wrapping_add((((n >> 10) & 0xfff) as u64) << (((n >> 22) & 1) * 12)))
                    })
                } else {
                    None
                };
                if let Some(t) = target.filter(|t| t % 4 == 0) {
                    if let Some(j) = owner(t) {
                        seeds[j].insert(t);
                    }
                }
            }
            a += 4;
        }
        for o in (0..m.image.len().saturating_sub(7)).step_by(8) {
            let v = u64::from_le_bytes(m.image[o..o + 8].try_into().unwrap());
            if v % 4 == 0 {
                if let Some(j) = owner(v) {
                    seeds[j].insert(v);
                }
            }
        }
    }
    seeds
}

fn lift_image(
    name: &str,
    base: u64,
    img: &[u8],
    text: std::ops::Range<u64>,
    dynamic: Option<nxport::nso::Dynamic>,
    extra_seeds: BTreeSet<u64>,
    overlay: Option<Overlay>,
) -> Output {
    let mut seeds = BTreeSet::from([text.start]);
    seeds.extend(extra_seeds);
    // Both found through MOD0, which a bare code region has none of.
    if let Some(d) = dynamic {
        if let Some(hdr) = ehframe::mod0_eh_frame_hdr(img) {
            seeds.extend(ehframe::hdr_functions(img, base, hdr));
        }
        let u64_at = |o: usize| img.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
        // The module's initialisers: DT_INIT (the runtime calls a mod's), and DT_INIT_ARRAY's entries, which a load has
        // relocated to addresses.
        if d.init != 0 {
            seeds.insert(base + d.init as u64);
        }
        for o in (d.init_array..d.init_array + d.init_arraysz).step_by(8) {
            match u64_at(o) {
                Some(a) if text.contains(&a) => {
                    seeds.insert(a);
                }
                _ => {}
            }
        }
        // Elf64_Sym (24 bytes): name u32, info u8, other u8, shndx u16, value u64, size u64. The string table follows
        // the symbol table in Switch modules, which bounds it.
        if d.strtab > d.symtab {
            for o in (d.symtab..d.strtab).step_by(24) {
                let (Some(&info), Some(shndx), Some(value)) =
                    (img.get(o + 4), img.get(o + 6..o + 8), u64_at(o + 8))
                else {
                    break;
                };
                if info & 0xf == STT_FUNC && shndx != [0, 0] {
                    seeds.insert(base + value);
                }
            }
        }
        // Elf64_Rela (24 bytes): offset, info, addend
        for o in (d.rela..d.rela + d.relasz).step_by(24) {
            let (Some(info), Some(addend)) = (u64_at(o + 8), u64_at(o + 16)) else { break };
            if info & 0xffff_ffff == R_AARCH64_RELATIVE {
                seeds.insert(base.wrapping_add(addend));
            }
        }
    }
    let m = Module { name: name.to_string(), base, image: img, text, seeds };
    lift_module(&m, &Options { overlay, ..Options::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An uncompressed NSO holding the lifter's sample module (no MOD0: no dynamic tables or .eh_frame_hdr).
    #[test]
    fn lifts_an_nso() {
        let (base, img, text) = nxport_lifter::module::sample_image();
        let text_len = (text.end - text.start) as usize;
        let mut f = vec![0u8; 0x100];
        f[0..4].copy_from_slice(b"NSO0");
        let seg = |f: &mut Vec<u8>, hdr: usize, file_off: usize, mem_off: usize, size: usize| {
            for (i, v) in [file_off, mem_off, size].into_iter().enumerate() {
                f[hdr + i * 4..hdr + i * 4 + 4].copy_from_slice(&(v as u32).to_le_bytes());
            }
        };
        seg(&mut f, 0x10, 0x100, 0, text_len);
        seg(&mut f, 0x20, 0x100 + text_len, text_len, img.len() - text_len);
        seg(&mut f, 0x30, 0x100 + img.len(), img.len(), 0);
        f.extend_from_slice(&img);
        let dir = std::env::temp_dir().join(format!("nxport-lift-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let nso = dir.join("main");
        std::fs::write(&nso, &f).unwrap();
        let out = lift_nso(&nso, "main", base, &dir.join("c")).unwrap();
        assert_eq!((out.functions, out.jump_tables), (3, 1));
        assert!(dir.join("c/cpu.h").is_file() && dir.join("c/main_table.c").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod fetch_tests {
    use super::fetch_gzipped;
    use std::io::Write;

    #[test]
    fn a_gzipped_file_arrives_unpacked_and_a_missing_one_leaves_nothing() {
        let dir = std::env::temp_dir().join(format!("nx_fetch_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let gz = dir.join("p.profdata.gz");
        let mut e = flate2::write::GzEncoder::new(std::fs::File::create(&gz).unwrap(), flate2::Compression::fast());
        e.write_all(b"profile").unwrap();
        e.finish().unwrap();
        // file:///C:/... on Windows, file:///tmp/... elsewhere.
        let url = |p: &std::path::Path| format!("file:///{}", p.display().to_string().replace('\\', "/").trim_start_matches('/'));

        let dest = dir.join("sub").join("p.profdata");
        fetch_gzipped(&url(&gz), &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"profile");

        let none = dir.join("none.profdata");
        assert!(fetch_gzipped(&url(&dir.join("missing.gz")), &none).is_err());
        assert!(!none.exists());

        // Not gzip: a failure, nothing left behind.
        let bad = dir.join("bad.gz");
        std::fs::write(&bad, b"not gzip").unwrap();
        let out = dir.join("bad.profdata");
        assert!(fetch_gzipped(&url(&bad), &out).is_err());
        assert!(!out.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
