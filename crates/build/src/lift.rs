// Guest code -> C through nxport-lifter, for hosts without the native runtime (docs/windows-port.md).
//
// lift_game lifts what the runtime will run: the patched image (relocations, ExeFS patches, SDK interposition, game
// hooks; nxport::guest_modules). lift_nso lifts a module as its NSO file holds it. Function starts are seeded from
// .eh_frame_hdr, the dynamic symbol table's functions and the code pointers R_AARCH64_RELATIVE relocations store in
// data; the lifter adds call and branch targets.

use std::collections::BTreeSet;
use std::path::Path;

use nxport_lifter::{ehframe, lift_module, Module, Options};
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
    let out = lift_image(name, base, &img, text, n.dynamic().ok(), BTreeSet::new());
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
        let out = lift_image(&m.name, m.base, &m.image, m.text, dynamic, seeds);
        lifted.push((m.name, out));
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
/// `c_dir/obj` and are reused while newer than their source. `on_file(done, total)` is told as each file is done
/// (compiled or reused), from the compiling threads.
pub fn compile(
    c_dir: &Path,
    out: &Path,
    cc: &[String],
    jobs: usize,
    on_file: &(dyn Fn(usize, usize) + Sync),
) -> Result<(), Box<dyn std::error::Error>> {
    let (prog, pre) = cc.split_first().ok_or("no C compiler")?;
    // The x86-64-v3 features, spelled out (zig cc 0.13 misreads -march=x86-64-v3): above all hardware FMA, which the
    // lifted floating-point code needs to round like the guest's. zig cc builds any of -O1 to -O3 as clang -O2.
    let flags = [
        "-O1", "-g0", "-fno-strict-aliasing", "-mavx2", "-mfma", "-mbmi", "-mbmi2", "-mlzcnt", "-mmovbe", "-mf16c",
        "-mpopcnt", "-msse4.2", "-mcx16",
    ];
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
    let next = std::sync::atomic::AtomicUsize::new(0);
    let done = std::sync::atomic::AtomicUsize::new(0);
    let finished = || on_file(done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1, sources.len());
    let failed = std::sync::Mutex::new(Vec::new());
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
                if obj.is_file() && !newer(src, obj) && !stale_headers {
                    finished();
                    continue;
                }
                let r = command(prog)
                    .args(pre)
                    .args(flags)
                    .arg("-c")
                    .arg(src)
                    .arg("-o")
                    .arg(obj)
                    .output();
                match r {
                    Ok(o) if o.status.success() => {}
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
    let at = std::fs::canonicalize(&at)?;
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
    lift_module(&m, &Options::default())
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
