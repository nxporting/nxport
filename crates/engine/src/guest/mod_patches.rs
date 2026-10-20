// ExeFS code mods: IPS / IPS32 / pchtxt patches from the enabled mods' exefs/ folders (see horizon::fs::mods), applied to a
// loaded module selected by its NSO build id, as Atmosphere and Ryujinx do.
//
// - `<build id>.ips` (IPS: "PATCH" ... "EOF") and IPS32 ("IPS32" ... "EEOF"). The file name is the module's build id
//   in hex (Atmosphere trims trailing zero bytes; any prefix of at least 16 hex digits matches). Offsets count the
//   0x100-byte NSO header, so module offset = patch offset - 0x100.
// - `*.pchtxt`: text patches. `@nsobid-<build id>` selects the module, `@flag offset_shift <n>` sets the header shift
//   (default 0x100 like IPS... pchtxt files usually say `@flag offset_shift 0x100` themselves), `@enabled` / `@disabled`
//   toggle the following patches, lines `<offset> <hex bytes>` patch, `@stop` ends the file.
//
// Patches are applied right after the module is relocated, before the engine's own instruction rewriting, so patched
// code goes through the same tpidr/svc handling as the game's. --no-exefs-patches skips them.

pub use super::nso::nso_build_id;

fn id_matches(file_id: &str, build_id: &str) -> bool {
    let file_id = file_id.trim().to_ascii_uppercase();
    file_id.len() >= 16 && file_id.len() <= build_id.len() && build_id.starts_with(&file_id)
}

/// One patch: module offset (header already removed) and the bytes to write.
struct Patch {
    offset: usize,
    bytes: Vec<u8>,
}

fn parse_ips(data: &[u8]) -> Option<Vec<Patch>> {
    let (ips32, mut p) = if data.starts_with(b"IPS32") {
        (true, 5)
    } else if data.starts_with(b"PATCH") {
        (false, 5)
    } else {
        return None;
    };
    let mut out = Vec::new();
    loop {
        if ips32 && data.get(p..p + 4) == Some(b"EEOF")
            || !ips32 && data.get(p..p + 3) == Some(b"EOF")
        {
            return Some(out);
        }
        let off_len = if ips32 { 4 } else { 3 };
        let off_bytes = data.get(p..p + off_len)?;
        let offset = off_bytes.iter().fold(0usize, |a, &b| (a << 8) | b as usize);
        p += off_len;
        let size = u16::from_be_bytes([*data.get(p)?, *data.get(p + 1)?]) as usize;
        p += 2;
        let bytes = if size == 0 {
            // RLE record: count, value
            let count = u16::from_be_bytes([*data.get(p)?, *data.get(p + 1)?]) as usize;
            let value = *data.get(p + 2)?;
            p += 3;
            vec![value; count]
        } else {
            let b = data.get(p..p + size)?.to_vec();
            p += size;
            b
        };
        if offset < 0x100 {
            continue; // patches the NSO header itself: nothing in memory
        }
        out.push(Patch {
            offset: offset - 0x100,
            bytes,
        });
    }
}

/// Patches in a pchtxt file that apply to `build_id`.
fn parse_pchtxt(text: &str, build_id: &str) -> Vec<Patch> {
    let mut out = Vec::new();
    let mut matches = false;
    let mut enabled = true;
    let mut shift: i64 = 0;
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if let Some(id) = line.strip_prefix("@nsobid-") {
            matches = id_matches(id, build_id);
            continue;
        }
        if line == "@stop" {
            break;
        }
        if line == "@enabled" {
            enabled = true;
            continue;
        }
        if line == "@disabled" {
            enabled = false;
            continue;
        }
        if let Some(rest) = line.strip_prefix("@flag") {
            let mut it = rest.split_whitespace();
            if it.next() == Some("offset_shift") {
                if let Some(v) = it.next() {
                    shift = i64::from_str_radix(
                        v.trim_start_matches("0x"),
                        if v.starts_with("0x") { 16 } else { 10 },
                    )
                    .unwrap_or(0);
                }
            }
            continue;
        }
        if line.starts_with('@') || !matches || !enabled {
            continue;
        }
        let mut it = line.split_whitespace();
        let (Some(off), Some(hex)) = (it.next(), it.next()) else {
            continue;
        };
        let Ok(off) = i64::from_str_radix(off.trim_start_matches("0x"), 16) else {
            continue;
        };
        let hex: String = hex.chars().filter(|c| c.is_ascii_hexdigit()).collect();
        if hex.len() % 2 != 0 {
            continue;
        }
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect();
        let offset = off + shift - 0x100;
        if offset >= 0 {
            out.push(Patch {
                offset: offset as usize,
                bytes,
            });
        }
    }
    out
}

/// Applies every enabled mod's exefs patches for the module with `build_id`, loaded at `base` with `size` bytes of
/// image. `text` is the module's text range (start, len) and `text_prot` its current protection, restored after each
/// write into it. Returns the number of patches applied.
pub unsafe fn apply(
    module_name: &str,
    build_id: &str,
    base: usize,
    size: usize,
    text: (usize, usize),
    text_prot: i32,
) -> usize {
    if crate::settings::get().files.no_exefs_patches {
        return 0;
    }
    let mut applied = 0;
    for (mod_name, dir) in crate::horizon::fs::mods() {
        let exefs = dir.join("exefs");
        let Ok(rd) = std::fs::read_dir(&exefs) else {
            continue;
        };
        let mut files: Vec<_> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for path in files {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let patches = match path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .as_deref()
            {
                Some("ips")
                    if id_matches(
                        name.trim_end_matches(".ips").trim_end_matches(".IPS"),
                        build_id,
                    ) =>
                {
                    match std::fs::read(&path).ok().as_deref().and_then(parse_ips) {
                        Some(p) => p,
                        None => {
                            println!(
                                "[!] Mods: '{}' {}: not a valid IPS/IPS32 file",
                                mod_name, name
                            );
                            continue;
                        }
                    }
                }
                Some("pchtxt") => parse_pchtxt(
                    &std::fs::read_to_string(&path).unwrap_or_default(),
                    build_id,
                ),
                _ => continue,
            };
            if patches.is_empty() {
                continue;
            }
            let mut n = 0;
            for p in &patches {
                if p.offset + p.bytes.len() > size {
                    println!(
                        "[!] Mods: '{}' {}: patch at {:#x} is outside {}",
                        mod_name,
                        name,
                        p.offset + 0x100,
                        module_name
                    );
                    continue;
                }
                write_bytes(base + p.offset, &p.bytes, text, text_prot);
                n += 1;
            }
            println!(
                "[+] Mods: '{}' {}: {} patches applied to {}",
                mod_name, name, n, module_name
            );
            applied += n;
        }
    }
    applied
}

unsafe fn write_bytes(addr: usize, bytes: &[u8], text: (usize, usize), text_prot: i32) {
    extern "C" {
        fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
    }
    let in_text = addr < text.0 + text.1 && addr + bytes.len() > text.0;
    let page = addr & !0x3fff;
    let span = ((addr + bytes.len() + 0x3fff) & !0x3fff) - page;
    if in_text {
        libc::mprotect(
            page as *mut libc::c_void,
            span,
            libc::PROT_READ | libc::PROT_WRITE,
        );
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
    if in_text {
        libc::mprotect(page as *mut libc::c_void, span, text_prot);
        sys_icache_invalidate(addr as *mut libc::c_void, bytes.len());
    }
}
