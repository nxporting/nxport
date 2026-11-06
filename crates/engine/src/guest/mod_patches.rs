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
        // What falls in the NSO header is nothing in memory.
        let skip = 0x100usize.saturating_sub(offset).min(bytes.len());
        if skip < bytes.len() {
            out.push(Patch {
                offset: (offset + skip) - 0x100,
                bytes: bytes[skip..].to_vec(),
            });
        }
    }
}

/// Patches in a pchtxt file that apply to `build_id`.
fn parse_pchtxt(text: &str, build_id: &str) -> Vec<Patch> {
    let mut out = Vec::new();
    let mut matches = false;
    let mut enabled = true;
    let mut little_endian = false;
    let mut shift: i64 = 0;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if let Some(id) = line.strip_prefix("@nsobid-") {
            matches = id_matches(strip_comment(id).trim(), build_id);
            continue;
        }
        let bare = strip_comment(line).trim();
        match bare {
            "" => continue,
            "@stop" => break,
            "@enabled" => {
                enabled = true;
                continue;
            }
            "@disabled" => {
                enabled = false;
                continue;
            }
            "@little-endian" => {
                little_endian = true;
                continue;
            }
            "@big-endian" => {
                little_endian = false;
                continue;
            }
            _ => {}
        }
        if let Some(rest) = bare.strip_prefix("@flag") {
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
        if bare.starts_with('@') || !matches || !enabled {
            continue;
        }
        let Some((off, value)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Ok(off) = i64::from_str_radix(off.trim_start_matches("0x"), 16) else {
            continue;
        };
        let value = value.trim_start();
        let bytes = if let Some(quoted) = value.strip_prefix('"') {
            match quoted_bytes(quoted) {
                Some(b) => b,
                None => {
                    println!("[!] Mods: pchtxt: an unclosed string in '{line}': skipped");
                    continue;
                }
            }
        } else {
            let hex: String = strip_comment(value).chars().filter(|c| !c.is_whitespace()).collect();
            let valid = hex.len() % 2 == 0 && !hex.is_empty() && hex.chars().all(|c| c.is_ascii_hexdigit());
            if !valid {
                println!("[!] Mods: pchtxt: not hex in '{line}': skipped");
                continue;
            }
            let mut b: Vec<u8> = (0..hex.len()).step_by(2).filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect();
            if little_endian {
                b.reverse();
            }
            b
        };
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

/// `line` up to a `//` comment.
fn strip_comment(line: &str) -> &str {
    line.split("//").next().unwrap_or("")
}

/// The bytes of a string whose opening quote has gone, up to its closing quote: no terminator is added.
fn quoted_bytes(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut chars = s.chars();
    let mut buf = [0u8; 4];
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push(b'\n'),
                'r' => out.push(b'\r'),
                't' => out.push(b'\t'),
                '0' => out.push(0),
                other => out.extend_from_slice(other.encode_utf8(&mut buf).as_bytes()),
            },
            _ => out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes()),
        }
    }
    None
}

/// Whether a patch file names only other builds: `name` an .ips file's name, or `pchtxt` its text. A file naming no build
/// says nothing about its version.
fn for_other_build(name: &str, pchtxt: Option<&str>, build_ids: &[&str]) -> bool {
    let named: Vec<&str> = match pchtxt {
        Some(text) => text.lines().filter_map(|l| l.trim().strip_prefix("@nsobid-")).map(|id| strip_comment(id).trim()).collect(),
        None => name.rsplit_once('.').map(|(stem, _)| stem).into_iter().filter(|s| s.len() >= 16 && s.chars().all(|c| c.is_ascii_hexdigit())).collect(),
    };
    !named.is_empty() && !named.iter().any(|id| build_ids.iter().any(|b| id_matches(id, b)))
}

/// Says which patch files apply to none of `build_ids`, the modules loaded: a mod for another version of the game does
/// nothing otherwise, and looks like a bug in the mod or in this program. Atmosphere and the emulators skip them too.
pub fn report_other_builds(build_ids: &[&str]) {
    if crate::settings::get().files.no_exefs_patches {
        return;
    }
    for (mod_name, exefs) in crate::horizon::fs::exefs_patch_dirs() {
        let Ok(rd) = std::fs::read_dir(&exefs) else { continue };
        for path in rd.filter_map(|e| e.ok()).map(|e| e.path()) {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let ext = path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
            let other = match ext.as_deref() {
                Some("ips") => for_other_build(&name, None, build_ids),
                Some("pchtxt") => for_other_build(&name, Some(&String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default())), build_ids),
                _ => false,
            };
            if other {
                println!("[!] Mods: '{mod_name}' {name} matches no loaded module, so it is probably for another version of the game: not applied");
            }
        }
    }
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
    text_prot: crate::os::Prot,
) -> usize {
    if crate::settings::get().files.no_exefs_patches {
        return 0;
    }
    let mut applied = 0;
    for (mod_name, exefs) in crate::horizon::fs::exefs_patch_dirs() {
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
                // Lossy: comments are often in another encoding, and the patches are ASCII.
                Some("pchtxt") => parse_pchtxt(
                    &String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default()),
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

unsafe fn write_bytes(addr: usize, bytes: &[u8], text: (usize, usize), text_prot: crate::os::Prot) {
    let in_text = addr < text.0 + text.1 && addr + bytes.len() > text.0;
    let page = addr & !0x3fff;
    let span = ((addr + bytes.len() + 0x3fff) & !0x3fff) - page;
    if in_text {
        let _ = crate::os::protect(page as usize, span, crate::os::Prot::ReadWrite);
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), addr as *mut u8, bytes.len());
    if in_text {
        let _ = crate::os::protect(page, span, text_prot);
        crate::os::flush_icache(addr as usize, bytes.len());
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;
    const ID: &str = "AABBCCDDEEFF00112233445566778899AABBCCDD";

    fn bytes(text: &str) -> Vec<(usize, Vec<u8>)> {
        parse_pchtxt(text, ID).into_iter().map(|p| (p.offset, p.bytes)).collect()
    }

    #[test]
    fn pchtxt_values() {
        let head = "\u{feff}@nsobid-AABBCCDDEEFF0011\n@flag offset_shift 0x100\n";
        assert_eq!(bytes(&format!("{head}00000010 01020304 // note\n")), [(0x10, vec![1, 2, 3, 4])]);
        assert_eq!(bytes(&format!("{head}00000010 01 02\t03\n")), [(0x10, vec![1, 2, 3])]);
        assert_eq!(bytes(&format!("{head}@little-endian\n00000010 01020304\n@big-endian\n00000020 0506\n")), [(0x10, vec![4, 3, 2, 1]), (0x20, vec![5, 6])]);
        assert_eq!(bytes(&format!("{head}00000010 \"a//b\\n\"\n")), [(0x10, b"a//b\n".to_vec())]);
        // Not read as the hex digits a string holds.
        assert_eq!(bytes(&format!("{head}00000010 \"abcd\"\n")), [(0x10, b"abcd".to_vec())]);
        assert!(bytes(&format!("{head}00000010 \"open\n00000020 zz\n00000030 123\n")).is_empty());
    }

    #[test]
    fn patch_files_for_other_builds() {
        let ids = [ID, "1122334455667788000000000000000000000000"];
        assert!(for_other_build("0011223344556677.ips", None, &ids));
        assert!(!for_other_build("AABBCCDDEEFF0011.ips", None, &ids));
        assert!(!for_other_build("aabbccddeeff0011.IPS", None, &ids));
        assert!(!for_other_build("readme.ips", None, &ids));
        assert!(for_other_build("x.pchtxt", Some("@nsobid-0011223344556677\n"), &ids));
        assert!(!for_other_build("x.pchtxt", Some("@nsobid-0011223344556677\n@nsobid-1122334455667788 // 1.1\n"), &ids));
        assert!(!for_other_build("x.pchtxt", Some("00000010 01\n"), &ids));
    }

    #[test]
    fn pchtxt_selection() {
        assert!(bytes("@nsobid-0011223344556677\n@flag offset_shift 0x100\n00000010 01\n").is_empty());
        assert_eq!(bytes("@nsobid-AABBCCDDEEFF0011 // v1.1\n@flag offset_shift 0x100\n@disabled\n00000010 01\n@enabled\n00000020 02\n@stop\n00000030 03\n"), [(0x20, vec![2])]);
    }

    #[test]
    fn ips_record_across_the_header() {
        let mut f = b"PATCH".to_vec();
        f.extend_from_slice(&[0x00, 0x00, 0xFE, 0x00, 0x04, 1, 2, 3, 4]);
        f.extend_from_slice(&[0x00, 0x00, 0x10, 0x00, 0x01, 9]);
        f.extend_from_slice(b"EOF");
        let p = parse_ips(&f).unwrap();
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].offset, p[0].bytes.clone()), (0, vec![3, 4]));
    }
}
