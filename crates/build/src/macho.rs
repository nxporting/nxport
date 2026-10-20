// A code-signed dylib that carries a guest code image (the engine's --aot-dump output) in section __TEXT,__guest at a
// 16 KiB-aligned file offset. The engine dlopens it, which registers its code signature, then maps the section's
// pages executable over the patched guest code (crates/engine/src/cpu/aot.rs).

use std::path::Path;

const PAGE: usize = 0x4000;

#[derive(Clone, Copy)]
pub enum Platform {
    MacOS,
    IOS,
}

struct W(Vec<u8>);

impl W {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn name16(&mut self, s: &str) {
        let mut b = [0u8; 16];
        b[..s.len()].copy_from_slice(s.as_bytes());
        self.0.extend_from_slice(&b);
    }
    fn pad(&mut self, align: usize) {
        while self.0.len() % align != 0 {
            self.0.push(0);
        }
    }
}

/// Load command with a path string (LC_ID_DYLIB / LC_LOAD_DYLIB): {cmd, size, name offset 24, timestamp, current,
/// compatibility version}, then the name, padded to 8.
fn dylib_cmd(cmd: u32, name: &str, current: u32, compat: u32) -> Vec<u8> {
    let mut w = W(Vec::new());
    let size = (24 + name.len() + 1).next_multiple_of(8);
    w.u32(cmd);
    w.u32(size as u32);
    w.u32(24);
    w.u32(2);
    w.u32(current);
    w.u32(compat);
    w.0.extend_from_slice(name.as_bytes());
    w.0.push(0);
    w.pad(8);
    w.0
}

/// Writes the dylib (install name `install_name`) holding `image` and signs it ad hoc.
pub fn write_guest_dylib(image: &[u8], out: &Path, platform: Platform, install_name: &str) -> Result<(), String> {
    let img_len = image.len().next_multiple_of(PAGE);
    let text_size = PAGE + img_len;

    // __LINKEDIT: empty chained fixups (header, starts for two segments without fixups, an empty symbol pool), an
    // empty export trie and a one-byte string table.
    let mut le = W(Vec::new());
    let fixups_off = 0;
    le.u32(0); // fixups_version
    le.u32(32); // starts_offset
    le.u32(44); // imports_offset
    le.u32(44); // symbols_offset
    le.u32(0); // imports_count
    le.u32(1); // imports_format: DYLD_CHAINED_IMPORT
    le.u32(0); // symbols_format
    le.pad(32);
    le.u32(2); // seg_count
    le.u32(0);
    le.u32(0);
    le.u32(0); // symbol pool
    let fixups_size = le.0.len() - fixups_off;
    le.pad(8);
    let trie_off = le.0.len();
    le.0.extend_from_slice(&[0, 0]);
    le.pad(8);
    let trie_size = le.0.len() - trie_off;
    let str_off = le.0.len();
    le.0.extend_from_slice(&[b' ', 0]);
    le.pad(8);
    let str_size = le.0.len() - str_off;
    let linkedit_off = text_size;

    let (plat, minos) = match platform {
        Platform::MacOS => (1u32, 12 << 16),
        Platform::IOS => (2u32, 16 << 16),
    };
    let uuid = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        image.hash(&mut h);
        let a = h.finish();
        install_name.hash(&mut h);
        let b = h.finish();
        let mut u = [0u8; 16];
        u[..8].copy_from_slice(&a.to_le_bytes());
        u[8..].copy_from_slice(&b.to_le_bytes());
        u
    };

    let mut cmds: Vec<Vec<u8>> = Vec::new();
    // LC_SEGMENT_64 __TEXT with one section, __guest.
    let mut c = W(Vec::new());
    c.u32(0x19);
    c.u32(72 + 80);
    c.name16("__TEXT");
    c.u64(0);
    c.u64(text_size as u64);
    c.u64(0);
    c.u64(text_size as u64);
    c.u32(5); // maxprot r-x
    c.u32(5);
    c.u32(1);
    c.u32(0);
    c.name16("__guest");
    c.name16("__TEXT");
    c.u64(PAGE as u64);
    c.u64(image.len() as u64);
    c.u32(PAGE as u32);
    c.u32(14); // 2^14 alignment
    c.u32(0);
    c.u32(0);
    c.u32(0x8000_0400); // S_ATTR_PURE_INSTRUCTIONS | S_ATTR_SOME_INSTRUCTIONS
    c.u32(0);
    c.u32(0);
    c.u32(0);
    cmds.push(c.0);
    // LC_SEGMENT_64 __LINKEDIT.
    let mut c = W(Vec::new());
    c.u32(0x19);
    c.u32(72);
    c.name16("__LINKEDIT");
    c.u64(text_size as u64);
    c.u64(le.0.len().next_multiple_of(PAGE) as u64);
    c.u64(linkedit_off as u64);
    c.u64(le.0.len() as u64);
    c.u32(1); // r--
    c.u32(1);
    c.u32(0);
    c.u32(0);
    cmds.push(c.0);
    cmds.push(dylib_cmd(0xd, install_name, 0x10000, 0x10000)); // LC_ID_DYLIB
    for (cmd, off, size) in [(0x8000_0034u32, fixups_off, fixups_size), (0x8000_0033, trie_off, trie_size)] {
        // LC_DYLD_CHAINED_FIXUPS, LC_DYLD_EXPORTS_TRIE
        let mut c = W(Vec::new());
        c.u32(cmd);
        c.u32(16);
        c.u32((linkedit_off + off) as u32);
        c.u32(size as u32);
        cmds.push(c.0);
    }
    let mut c = W(Vec::new()); // LC_SYMTAB: no symbols
    c.u32(0x2);
    c.u32(24);
    c.u32((linkedit_off + str_off) as u32);
    c.u32(0);
    c.u32((linkedit_off + str_off) as u32);
    c.u32(str_size as u32);
    cmds.push(c.0);
    let mut c = W(Vec::new()); // LC_DYSYMTAB: empty
    c.u32(0xb);
    c.u32(80);
    c.0.extend_from_slice(&[0u8; 72]);
    cmds.push(c.0);
    let mut c = W(Vec::new()); // LC_UUID
    c.u32(0x1b);
    c.u32(24);
    c.0.extend_from_slice(&uuid);
    cmds.push(c.0);
    let mut c = W(Vec::new()); // LC_BUILD_VERSION, no tools
    c.u32(0x32);
    c.u32(24);
    c.u32(plat);
    c.u32(minos);
    c.u32(minos);
    c.u32(0);
    cmds.push(c.0);
    cmds.push(dylib_cmd(0xc, "/usr/lib/libSystem.B.dylib", 0x0547_0000, 0x10000)); // LC_LOAD_DYLIB

    let sizeofcmds: usize = cmds.iter().map(Vec::len).sum();
    if 32 + sizeofcmds > PAGE {
        return Err("load commands exceed the first page".into());
    }
    let mut f = W(Vec::with_capacity(text_size + le.0.len()));
    f.u32(0xfeed_facf);
    f.u32(0x0100_000c); // CPU_TYPE_ARM64
    f.u32(0);
    f.u32(6); // MH_DYLIB
    f.u32(cmds.len() as u32);
    f.u32(sizeofcmds as u32);
    f.u32(0x0010_0085); // MH_NOUNDEFS | MH_DYLDLINK | MH_TWOLEVEL | MH_NO_REEXPORTED_DYLIBS
    f.u32(0);
    for c in &cmds {
        f.0.extend_from_slice(c);
    }
    f.0.resize(PAGE, 0);
    f.0.extend_from_slice(image);
    f.0.resize(text_size, 0);
    f.0.extend_from_slice(&le.0);
    std::fs::write(out, &f.0).map_err(|e| format!("{}: {e}", out.display()))?;
    sign(out, None)
}

/// Ad-hoc code signature (macOS's codesign), optionally with entitlements.
pub fn sign(path: &Path, entitlements: Option<&Path>) -> Result<(), String> {
    let mut cmd = std::process::Command::new("/usr/bin/codesign");
    cmd.args(["-f", "-s", "-"]);
    if let Some(e) = entitlements {
        cmd.arg("--entitlements").arg(e);
    }
    let out = cmd.arg(path).output().map_err(|e| format!("codesign: {e}"))?;
    if !out.status.success() {
        return Err(format!("codesign {}: {}", path.display(), String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(())
}
