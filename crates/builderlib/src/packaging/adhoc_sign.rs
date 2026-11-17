// Ad-hoc code signatures without Apple's tools, for building iOS and macOS apps on hosts that have no codesign: what
// `codesign -f -s -` writes into a Mach-O file, and a bundle's _CodeSignature/CodeResources. Sideloading tools replace
// an iOS app's with their own signatures, so these only keep it well formed until then; a macOS app runs with them.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

const PAGE_LOG2: u8 = 12;
const LC_SEGMENT_64: u32 = 0x19;
const LC_CODE_SIGNATURE: u32 = 0x1d;
const MH_EXECUTE: u32 = 2;

fn sha256(d: &[u8]) -> [u8; 32] {
    Sha256::digest(d).into()
}

fn u32_at(d: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
}

fn u64_at(d: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(d[o..o + 8].try_into().unwrap())
}

/// A blob: {magic, length, payload}, big-endian as all of a signature is.
fn blob(magic: u32, payload: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(8 + payload.len());
    b.extend_from_slice(&magic.to_be_bytes());
    b.extend_from_slice(&(8 + payload.len() as u32).to_be_bytes());
    b.extend_from_slice(payload);
    b
}

fn der_len(n: usize) -> Vec<u8> {
    if n < 0x80 {
        vec![n as u8]
    } else {
        let bytes: Vec<u8> = n.to_be_bytes().into_iter().skip_while(|&b| b == 0).collect();
        let mut v = vec![0x80 | bytes.len() as u8];
        v.extend(bytes);
        v
    }
}

fn der(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(der_len(content.len()));
    v.extend_from_slice(content);
    v
}

/// The DER form of entitlements that are all `true` (iOS 15 and later read this form): `[APPLICATION 16] { 1, [16] {
/// {key, true}... } }`, keys sorted.
fn der_entitlements(keys: &[String]) -> Vec<u8> {
    let mut sorted: Vec<&String> = keys.iter().collect();
    sorted.sort();
    let mut pairs = Vec::new();
    for k in sorted {
        let mut pair = der(0x0c, k.as_bytes());
        pair.extend(der(0x01, &[0xff]));
        pairs.extend(der(0x30, &pair));
    }
    let mut inner = der(0x02, &[1]);
    inner.extend(der(0xb0, &pairs));
    der(0x70, &inner)
}

/// The keys of an entitlements plist; only `true` values are accepted, as nothing here needs others.
fn entitlement_keys(plist: &str) -> Result<Vec<String>, String> {
    let re = regex::Regex::new(r"<key>([^<]+)</key>\s*<([a-z]+)/?>").unwrap();
    let mut keys = Vec::new();
    for c in re.captures_iter(plist) {
        if &c[2] != "true" {
            return Err(format!("entitlement {}: only true values can be signed here", &c[1]));
        }
        keys.push(c[1].to_string());
    }
    Ok(keys)
}

pub struct Inputs<'a> {
    pub identifier: &'a str,
    /// An entitlements plist.
    pub entitlements: Option<&'a str>,
    pub info_plist: Option<[u8; 32]>,
    pub code_resources: Option<[u8; 32]>,
}

/// Signs a thin arm64 Mach-O in place, replacing any signature it has.
pub fn sign_macho(d: &mut Vec<u8>, i: &Inputs) -> Result<(), String> {
    if d.len() < 32 || u32_at(d, 0) != 0xfeed_facf {
        return Err("not a 64-bit Mach-O file".into());
    }
    let (filetype, ncmds, sizeofcmds) = (u32_at(d, 12), u32_at(d, 16) as usize, u32_at(d, 20) as usize);
    let (mut p, mut linkedit, mut sig_cmd, mut exec_seg, mut first_section) = (32, None, None, None, usize::MAX);
    for _ in 0..ncmds {
        let (cmd, size) = (u32_at(d, p), u32_at(d, p + 4) as usize);
        if cmd == LC_SEGMENT_64 {
            let name = &d[p + 8..p + 24];
            if name.starts_with(b"__LINKEDIT\0") {
                linkedit = Some(p);
            } else if name.starts_with(b"__TEXT\0") {
                exec_seg = Some((u64_at(d, p + 40), u64_at(d, p + 48)));
                for s in 0..u32_at(d, p + 64) as usize {
                    let off = u32_at(d, p + 72 + s * 80 + 48) as usize;
                    if off != 0 {
                        first_section = first_section.min(off);
                    }
                }
            }
        } else if cmd == LC_CODE_SIGNATURE {
            sig_cmd = Some(p);
        }
        p += size;
    }
    let linkedit = linkedit.ok_or("no __LINKEDIT segment")?;
    let (exec_base, exec_limit) = exec_seg.ok_or("no __TEXT segment")?;
    let code_limit = match sig_cmd {
        Some(c) => u32_at(d, c + 8) as usize,
        None => d.len(),
    }
    .next_multiple_of(16);
    d.resize(code_limit, 0);

    // The blobs other than the code directory.
    let requirements = blob(0xfade_0c01, &0u32.to_be_bytes());
    let cms = blob(0xfade_0b01, &[]);
    let (ent, der_ent) = match i.entitlements {
        Some(e) => {
            let keys = entitlement_keys(e)?;
            (Some(blob(0xfade_7171, e.as_bytes())), Some(blob(0xfade_7172, &der_entitlements(&keys))))
        }
        None => (None, None),
    };
    let mut special = [[0u8; 32]; 7];
    if let Some(h) = i.info_plist {
        special[0] = h;
    }
    special[1] = sha256(&requirements);
    if let Some(h) = i.code_resources {
        special[2] = h;
    }
    if let (Some(e), Some(de)) = (&ent, &der_ent) {
        special[4] = sha256(e);
        special[6] = sha256(de);
    }
    let n_special = special.iter().rposition(|h| *h != [0u8; 32]).map_or(0, |x| x + 1);

    // Sizes first: the headers the code hashes cover hold where the signature is and how large.
    let page = 1usize << PAGE_LOG2;
    let n_code = code_limit.div_ceil(page);
    let header = 88 + i.identifier.len() + 1;
    let cd_len = header + n_special * 32 + n_code * 32;
    let mut slots: Vec<(u32, usize)> = vec![(0, cd_len), (2, requirements.len())];
    if let (Some(e), Some(de)) = (&ent, &der_ent) {
        slots.push((5, e.len()));
        slots.push((7, de.len()));
    }
    slots.push((0x1_0000, cms.len()));
    let index_len = 12 + slots.len() * 8;
    let sig_len = index_len + slots.iter().map(|s| s.1).sum::<usize>();

    let sig_cmd = match sig_cmd {
        Some(c) => c,
        None => {
            let at = 32 + sizeofcmds;
            if at + 16 > first_section.min(d.len()) {
                return Err("no room in the Mach-O header for a code signature command".into());
            }
            d[at..at + 4].copy_from_slice(&LC_CODE_SIGNATURE.to_le_bytes());
            d[at + 4..at + 8].copy_from_slice(&16u32.to_le_bytes());
            d[16..20].copy_from_slice(&(ncmds as u32 + 1).to_le_bytes());
            d[20..24].copy_from_slice(&(sizeofcmds as u32 + 16).to_le_bytes());
            at
        }
    };
    d[sig_cmd + 8..sig_cmd + 12].copy_from_slice(&(code_limit as u32).to_le_bytes());
    d[sig_cmd + 12..sig_cmd + 16].copy_from_slice(&(sig_len as u32).to_le_bytes());
    let le_off = u64_at(d, linkedit + 40) as usize;
    let le_size = code_limit + sig_len - le_off;
    d[linkedit + 48..linkedit + 56].copy_from_slice(&(le_size as u64).to_le_bytes());
    d[linkedit + 32..linkedit + 40].copy_from_slice(&(le_size.next_multiple_of(0x4000) as u64).to_le_bytes());

    // The code directory.
    let mut cd = Vec::with_capacity(cd_len);
    let w32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_be_bytes());
    let w64 = |v: &mut Vec<u8>, x: u64| v.extend_from_slice(&x.to_be_bytes());
    w32(&mut cd, 0xfade_0c02);
    w32(&mut cd, cd_len as u32);
    w32(&mut cd, 0x20400); // version: up to the executable segment fields
    w32(&mut cd, 0x2); // flags: ad hoc
    w32(&mut cd, (header + n_special * 32) as u32); // hashOffset
    w32(&mut cd, 88); // identOffset
    w32(&mut cd, n_special as u32);
    w32(&mut cd, n_code as u32);
    w32(&mut cd, code_limit as u32);
    cd.extend_from_slice(&[32, 2, 0, PAGE_LOG2]); // hashSize, hashType SHA-256, platform, pageSize
    w32(&mut cd, 0); // spare2
    w32(&mut cd, 0); // scatterOffset
    w32(&mut cd, 0); // teamOffset
    w32(&mut cd, 0); // spare3
    w64(&mut cd, 0); // codeLimit64: codeLimit is used
    w64(&mut cd, exec_base);
    w64(&mut cd, exec_limit);
    w64(&mut cd, if filetype == MH_EXECUTE { 1 } else { 0 }); // execSegFlags: main binary
    debug_assert_eq!(cd.len(), 88);
    cd.extend_from_slice(i.identifier.as_bytes());
    cd.push(0);
    for h in special[..n_special].iter().rev() {
        cd.extend_from_slice(h);
    }
    for chunk in d.chunks(page) {
        cd.extend_from_slice(&sha256(chunk));
    }
    debug_assert_eq!(cd.len(), cd_len);

    // The superblob.
    let mut out = Vec::with_capacity(sig_len);
    w32(&mut out, 0xfade_0cc0);
    w32(&mut out, sig_len as u32);
    w32(&mut out, slots.len() as u32);
    let mut at = index_len;
    for &(ty, len) in &slots {
        w32(&mut out, ty);
        w32(&mut out, at as u32);
        at += len;
    }
    out.extend_from_slice(&cd);
    out.extend_from_slice(&requirements);
    if let (Some(e), Some(de)) = (&ent, &der_ent) {
        out.extend_from_slice(e);
        out.extend_from_slice(de);
    }
    out.extend_from_slice(&cms);
    debug_assert_eq!(out.len(), sig_len);
    d.extend_from_slice(&out);
    Ok(())
}

fn base64(d: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(d.len().div_ceil(3) * 4);
    for c in d.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        s.push(T[(n >> 18) as usize & 63] as char);
        s.push(T[(n >> 12) as usize & 63] as char);
        s.push(if c.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        s.push(if c.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    s
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for e in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let p = e.path();
        if p.is_dir() {
            files_under(&p, out)?;
        } else {
            out.push(p);
        }
    }
    Ok(())
}

/// _CodeSignature/CodeResources: the SHA-256 of every file of a bundle but its executable and its signature.
fn code_resources(bundle: &Path, executable: &str) -> Result<String, String> {
    let mut files = Vec::new();
    files_under(bundle, &mut files)?;
    let mut rel: Vec<(String, PathBuf)> = files
        .into_iter()
        .map(|p| (p.strip_prefix(bundle).unwrap().to_string_lossy().replace('\\', "/"), p))
        .filter(|(r, _)| r != executable && !r.starts_with("_CodeSignature/"))
        .collect();
    rel.sort();
    let mut entries = String::new();
    for (r, p) in &rel {
        let hash = sha256(&fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?);
        entries += &format!("<key>{}</key><dict><key>hash2</key><data>{}</data></dict>\n", xml_escape(r), base64(&hash));
    }
    let rules = "<dict><key>^.*</key><true/></dict>";
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict>\n<key>files</key><dict/>\n<key>files2</key><dict>\n{entries}</dict>\n<key>rules</key>{rules}\n<key>rules2</key>{rules}\n</dict></plist>\n"
    ))
}

fn plist_string(plist: &str, key: &str) -> Option<String> {
    let re = regex::Regex::new(&format!(r"<key>{}</key>\s*<string>([^<]*)</string>", regex::escape(key))).unwrap();
    re.captures(plist).map(|c| c[1].to_string())
}

/// What `codesign -f -s - [--entitlements e] path` does: a Mach-O file, or a bundle (its CodeResources and executable).
pub fn sign_path(path: &Path, entitlements: Option<&Path>) -> Result<(), String> {
    let err = |p: &Path, e: std::io::Error| format!("{}: {e}", p.display());
    let ent = entitlements.map(|e| fs::read_to_string(e).map_err(|x| err(e, x))).transpose()?;
    let (exe, identifier, info, res) = if path.is_dir() {
        // A macOS bundle keeps everything in Contents/ and its executable in Contents/MacOS/; an iOS bundle is flat.
        let (root, exe_dir) = if path.join("Contents/Info.plist").is_file() { (path.join("Contents"), "MacOS/") } else { (path.to_path_buf(), "") };
        let info_path = root.join("Info.plist");
        let info = fs::read(&info_path).map_err(|e| err(&info_path, e))?;
        let text = String::from_utf8_lossy(&info);
        let exe = plist_string(&text, "CFBundleExecutable").ok_or("Info.plist has no CFBundleExecutable")?;
        let id = plist_string(&text, "CFBundleIdentifier").ok_or("Info.plist has no CFBundleIdentifier")?;
        let exe = format!("{exe_dir}{exe}");
        let resources = code_resources(&root, &exe)?;
        let dir = root.join("_CodeSignature");
        fs::create_dir_all(&dir).map_err(|e| err(&dir, e))?;
        fs::write(dir.join("CodeResources"), &resources).map_err(|e| err(&dir, e))?;
        (root.join(exe), id, Some(sha256(&info)), Some(sha256(resources.as_bytes())))
    } else {
        let id = path.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        (path.to_path_buf(), id, None, None)
    };
    let mut d = fs::read(&exe).map_err(|e| err(&exe, e))?;
    sign_macho(&mut d, &Inputs { identifier: &identifier, entitlements: ent.as_deref(), info_plist: info, code_resources: res })?;
    fs::write(&exe, d).map_err(|e| err(&exe, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn be(d: &[u8], o: usize) -> u32 {
        u32::from_be_bytes(d[o..o + 4].try_into().unwrap())
    }

    /// Checks a signature the way the kernel does: the code directory's page hashes and special slots.
    fn verify(d: &[u8]) -> (String, usize) {
        let (mut p, mut sig) = (32, None);
        for _ in 0..u32_at(d, 16) {
            if u32_at(d, p) == LC_CODE_SIGNATURE {
                sig = Some((u32_at(d, p + 8) as usize, u32_at(d, p + 12) as usize));
            }
            p += u32_at(d, p + 4) as usize;
        }
        let (off, len) = sig.expect("signature command");
        assert_eq!(off + len, d.len());
        assert_eq!(off % 16, 0);
        let s = &d[off..];
        assert_eq!(be(s, 0), 0xfade_0cc0);
        assert_eq!(be(s, 4) as usize, len);
        let count = be(s, 8) as usize;
        let blob_at = |ty: u32| (0..count).find(|i| be(s, 12 + i * 8) == ty).map(|i| be(s, 16 + i * 8) as usize);
        let cd = &s[blob_at(0).unwrap()..];
        assert_eq!(be(cd, 0), 0xfade_0c02);
        let (hash_off, n_special, n_code, limit) = (be(cd, 16) as usize, be(cd, 24) as usize, be(cd, 28) as usize, be(cd, 32) as usize);
        assert_eq!(limit, off);
        assert_eq!(cd[37], 2);
        let page = 1usize << cd[39];
        assert_eq!(n_code, limit.div_ceil(page));
        for (i, chunk) in d[..limit].chunks(page).enumerate() {
            assert_eq!(&cd[hash_off + i * 32..hash_off + (i + 1) * 32], &sha256(chunk), "page {i}");
        }
        let req = &s[blob_at(2).unwrap()..];
        let req_len = be(req, 4) as usize;
        assert_eq!(&cd[hash_off - 2 * 32..hash_off - 32], &sha256(&req[..req_len]));
        for (ty, slot) in [(5, 5), (7, 7)] {
            if let Some(a) = blob_at(ty) {
                let b = &s[a..];
                assert_eq!(&cd[hash_off - slot * 32..hash_off - (slot - 1) * 32], &sha256(&b[..be(b, 4) as usize]), "slot {slot}");
            }
        }
        let ident_at = be(cd, 20) as usize;
        let n = cd[ident_at..].iter().position(|&b| b == 0).unwrap();
        (String::from_utf8_lossy(&cd[ident_at..ident_at + n]).into_owned(), n_special)
    }

    fn sample() -> Vec<u8> {
        // Header, __TEXT with one section at 0x4000, __LINKEDIT at 0x8000 with 16 bytes.
        let mut d = vec![0u8; 0x8010];
        d[0..4].copy_from_slice(&0xfeed_facfu32.to_le_bytes());
        d[12..16].copy_from_slice(&MH_EXECUTE.to_le_bytes());
        d[16..20].copy_from_slice(&2u32.to_le_bytes());
        d[20..24].copy_from_slice(&(152u32 + 72).to_le_bytes());
        let mut p = 32;
        d[p..p + 4].copy_from_slice(&LC_SEGMENT_64.to_le_bytes());
        d[p + 4..p + 8].copy_from_slice(&152u32.to_le_bytes());
        d[p + 8..p + 14].copy_from_slice(b"__TEXT");
        d[p + 48..p + 56].copy_from_slice(&0x8000u64.to_le_bytes());
        d[p + 64..p + 68].copy_from_slice(&1u32.to_le_bytes());
        d[p + 72 + 48..p + 72 + 52].copy_from_slice(&0x4000u32.to_le_bytes());
        p += 152;
        d[p..p + 4].copy_from_slice(&LC_SEGMENT_64.to_le_bytes());
        d[p + 4..p + 8].copy_from_slice(&72u32.to_le_bytes());
        d[p + 8..p + 18].copy_from_slice(b"__LINKEDIT");
        d[p + 40..p + 48].copy_from_slice(&0x8000u64.to_le_bytes());
        d[p + 48..p + 56].copy_from_slice(&0x10u64.to_le_bytes());
        d
    }

    #[test]
    fn signature_verifies_and_resigning_replaces_it() {
        let mut d = sample();
        let plist = "<plist><dict><key>a.b</key><true/></dict></plist>";
        let i = Inputs { identifier: "com.x.y", entitlements: Some(plist), info_plist: Some([7; 32]), code_resources: Some([9; 32]) };
        sign_macho(&mut d, &i).unwrap();
        assert_eq!(verify(&d), ("com.x.y".to_string(), 7));
        assert_eq!(u32_at(&d, 16), 3);
        let once = d.clone();
        sign_macho(&mut d, &i).unwrap();
        assert_eq!(d, once);
    }

    #[test]
    fn plain_signature_has_requirements_slot_only() {
        let mut d = sample();
        sign_macho(&mut d, &Inputs { identifier: "x", entitlements: None, info_plist: None, code_resources: None }).unwrap();
        assert_eq!(verify(&d), ("x".to_string(), 2));
    }

    #[test]
    fn macos_bundle_is_sealed_from_contents() {
        let app = std::env::temp_dir().join(format!("nxport-sign-{}.app", std::process::id()));
        let _ = fs::remove_dir_all(&app);
        let contents = app.join("Contents");
        fs::create_dir_all(contents.join("MacOS")).unwrap();
        fs::create_dir_all(contents.join("Resources")).unwrap();
        let plist = "<plist><dict><key>CFBundleExecutable</key><string>nxport</string><key>CFBundleIdentifier</key><string>com.nxport.t</string></dict></plist>";
        fs::write(contents.join("Info.plist"), plist).unwrap();
        fs::write(contents.join("MacOS/nxport"), sample()).unwrap();
        fs::write(contents.join("Resources/settings.toml"), "x").unwrap();
        sign_path(&app, None).unwrap();
        let res = fs::read_to_string(contents.join("_CodeSignature/CodeResources")).unwrap();
        assert!(res.contains("<key>Resources/settings.toml</key>") && res.contains("<key>Info.plist</key>"));
        assert!(!res.contains("MacOS/nxport"));
        let d = fs::read(contents.join("MacOS/nxport")).unwrap();
        assert_eq!(verify(&d), ("com.nxport.t".to_string(), 3));
        let _ = fs::remove_dir_all(&app);
    }

    #[test]
    fn der_entitlements_shape() {
        let d = der_entitlements(&["b".to_string(), "a".to_string()]);
        // [APPLICATION 16] { INTEGER 1, [16] { SEQ { "a", true }, SEQ { "b", true } } }
        let pair = |k: u8| vec![0x30, 6, 0x0c, 1, k, 0x01, 1, 0xff];
        let mut inner = vec![0x02, 1, 1, 0xb0, 16];
        inner.extend(pair(b'a'));
        inner.extend(pair(b'b'));
        let mut want = vec![0x70, inner.len() as u8];
        want.extend(inner);
        assert_eq!(d, want);
    }

    #[test]
    fn base64_matches_known_values() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }

    #[test]
    fn refuses_values_it_cannot_encode() {
        assert!(entitlement_keys("<key>a</key><string>x</string>").is_err());
    }
}
