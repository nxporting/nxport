// A game module -> C through nxport-lifter, for hosts without the native runtime (docs/windows-port.md).
//
// This lifts a module as its NSO file holds it. What the runtime will run is the patched image (relocations, ExeFS
// patches, SDK interposition, game hooks: nxport::guest_image's pipeline); lifting that comes once the load pipeline
// runs off Apple hosts. Function starts are seeded from .eh_frame_hdr, the dynamic symbol table's functions and the
// code pointers R_AARCH64_RELATIVE relocations store in data; the lifter adds call and branch targets.

use std::collections::BTreeSet;
use std::path::Path;

use nxport_lifter::{ehframe, lift_module, Module, Options, Output};

const R_AARCH64_RELATIVE: u64 = 1027;
const STT_FUNC: u8 = 2;

/// Lifts the NSO at `path`, loaded at guest address `base`, into `out_dir` (C files and cpu.h).
pub fn lift_nso(path: &Path, name: &str, base: u64, out_dir: &Path) -> Result<Output, Box<dyn std::error::Error>> {
    let n = nxport::nso::Nso::read(path)?;
    let mut img = vec![0u8; n.image_size()];
    for s in [&n.text, &n.ro, &n.data] {
        img[s.mem_off..s.mem_off + s.data.len()].copy_from_slice(&s.data);
    }
    let text = base + n.text.mem_off as u64..base + (n.text.mem_off + n.text.data.len()) as u64;
    let mut seeds = BTreeSet::from([text.start]);
    if let Some(hdr) = ehframe::mod0_eh_frame_hdr(&img) {
        seeds.extend(ehframe::hdr_functions(&img, base, hdr));
    }
    if let Ok(d) = n.dynamic() {
        let u64_at = |o: usize| img.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
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
    let m = Module { name: name.to_string(), base, image: &img, text, seeds };
    let out = lift_module(&m, &Options::default());
    std::fs::create_dir_all(out_dir)?;
    std::fs::write(out_dir.join("cpu.h"), nxport_lifter::CPU_H)?;
    for (file, body) in &out.files {
        std::fs::write(out_dir.join(file), body)?;
    }
    Ok(out)
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
