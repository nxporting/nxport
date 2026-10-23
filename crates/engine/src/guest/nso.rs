// NSO executable parsing (format: https://switchbrew.org/wiki/NSO): segments, build id, and the dynamic-linking
// tables found through MOD0 -> .dynamic. Used for the game's main module, so the loader takes its layout from the
// file instead of one game's constants.

pub struct Segment {
    pub mem_off: usize,
    pub data: Vec<u8>,
}

pub struct Nso {
    pub text: Segment,
    pub ro: Segment,
    pub data: Segment,
    pub bss_size: usize,
    pub build_id: [u8; 32],
}

/// Locations (image offsets) and sizes of the dynamic-linking tables, from DT_* entries.
#[derive(Debug, Default, Clone, Copy)]
pub struct Dynamic {
    pub rela: usize,
    pub relasz: usize,
    pub jmprel: usize,
    pub pltrelsz: usize,
    pub symtab: usize,
    pub strtab: usize,
    pub strsz: usize,
    pub init: usize,
    pub init_array: usize,
    pub init_arraysz: usize,
}

/// The build id (32 bytes) of an NSO file, as uppercase hex.
pub fn nso_build_id(nso_path: &str) -> Option<String> {
    let mut f = std::fs::File::open(nso_path).ok()?;
    let mut head = [0u8; 0x60];
    std::io::Read::read_exact(&mut f, &mut head).ok()?;
    if &head[..4] != b"NSO0" {
        return None;
    }
    Some(
        head[0x40..0x60]
            .iter()
            .map(|b| format!("{:02X}", b))
            .collect(),
    )
}

fn u32_at(d: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(d[off..off + 4].try_into().unwrap())
}

impl Nso {
    pub fn read(path: &std::path::Path) -> Result<Nso, Box<dyn std::error::Error>> {
        let f = std::fs::read(path)?;
        if f.len() < 0x100 || &f[0..4] != b"NSO0" {
            return Err(format!("{}: not an NSO file", path.display()).into());
        }
        let flags = u32_at(&f, 0x0C);
        let seg = |hdr: usize,
                   comp_size_off: usize,
                   bit: u32,
                   name: &str|
         -> Result<Segment, Box<dyn std::error::Error>> {
            let (file_off, mem_off, size) = (
                u32_at(&f, hdr) as usize,
                u32_at(&f, hdr + 4) as usize,
                u32_at(&f, hdr + 8) as usize,
            );
            let data = if flags & bit != 0 {
                let comp = u32_at(&f, comp_size_off) as usize;
                lz4_flex::decompress(&f[file_off..file_off + comp], size)
                    .map_err(|e| format!("{}: LZ4 .{}: {}", path.display(), name, e))?
            } else {
                f[file_off..file_off + size].to_vec()
            };
            Ok(Segment { mem_off, data })
        };
        Ok(Nso {
            text: seg(0x10, 0x60, 1, "text")?,
            ro: seg(0x20, 0x64, 2, "rodata")?,
            data: seg(0x30, 0x68, 4, "data")?,
            bss_size: u32_at(&f, 0x3C) as usize,
            build_id: f[0x40..0x60].try_into().unwrap(),
        })
    }

    /// Start of .bss: the end of .data, rounded up to a page (Horizon maps segments on 4 KiB pages).
    pub fn bss_off(&self) -> usize {
        (self.data.mem_off + self.data.data.len() + 0xfff) & !0xfff
    }

    /// Size of the loaded image (through .bss, whole pages).
    pub fn image_size(&self) -> usize {
        (self.bss_off() + self.bss_size + 0xfff) & !0xfff
    }

    /// Reads `len` bytes at image offset `off` from whichever segment holds them.
    fn bytes_at(&self, off: usize, len: usize) -> Option<&[u8]> {
        [&self.text, &self.ro, &self.data]
            .into_iter()
            .find_map(|s| {
                let end = s.mem_off + s.data.len();
                (off >= s.mem_off && off + len <= end)
                    .then(|| &s.data[off - s.mem_off..off - s.mem_off + len])
            })
    }

    /// The dynamic tables. MOD0's location is the u32 at text+4; its fields are offsets from the MOD0 header itself
    /// (magic at +0, .dynamic offset at +4).
    pub fn dynamic(&self) -> Result<Dynamic, Box<dyn std::error::Error>> {
        parse_dynamic(self.text.mem_off, |off, len| self.bytes_at(off, len))
    }
}

/// The dynamic tables of a module image as it is loaded (text at offset 0), as `Nso::dynamic` reads them from a file.
pub fn image_dynamic(image: &[u8]) -> Result<Dynamic, Box<dyn std::error::Error>> {
    parse_dynamic(0, |off, len| image.get(off..off.checked_add(len)?))
}

fn parse_dynamic<'a>(
    text_off: usize,
    bytes_at: impl Fn(usize, usize) -> Option<&'a [u8]>,
) -> Result<Dynamic, Box<dyn std::error::Error>> {
    let mod0 = text_off + u32_at(bytes_at(text_off, 8).ok_or("no text")?, 4) as usize;
    if bytes_at(mod0, 4) != Some(b"MOD0".as_slice()) {
        return Err("MOD0 header not found".into());
    }
    let rel = i32::from_le_bytes(bytes_at(mod0 + 4, 4).ok_or("truncated MOD0")?.try_into().unwrap());
    let dyn_off = (mod0 as i64 + rel as i64) as usize;
    let mut d = Dynamic::default();
    for i in 0..4096 {
        let Some(e) = bytes_at(dyn_off + i * 16, 16) else {
            break;
        };
        let tag = i64::from_le_bytes(e[0..8].try_into().unwrap());
        let val = u64::from_le_bytes(e[8..16].try_into().unwrap()) as usize;
        match tag {
            0 => break,
            2 => d.pltrelsz = val,
            5 => d.strtab = val,
            6 => d.symtab = val,
            7 => d.rela = val,
            8 => d.relasz = val,
            10 => d.strsz = val,
            12 => d.init = val,
            23 => d.jmprel = val,
            25 => d.init_array = val,
            27 => d.init_arraysz = val,
            _ => {}
        }
    }
    if d.symtab == 0 || d.strtab == 0 {
        return Err("no DT_SYMTAB / DT_STRTAB".into());
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TotK 1.1.2's main against the layout the loader used to hard-code. Needs a dump: NXPORT_TEST_GAME_DIR names it
    /// (skipped without).
    #[test]
    fn main_matches_known_layout() {
        let Some(dir) = std::env::var_os("NXPORT_TEST_GAME_DIR") else {
            return;
        };
        let p = std::path::Path::new(&dir).join("ExeFS/main");
        if !p.exists() {
            return;
        }
        let p = p.as_path();
        let n = Nso::read(p).unwrap();
        assert_eq!(
            (n.text.mem_off, n.ro.mem_off, n.data.mem_off, n.bss_off()),
            (0, 0x02B18000, 0x041A7000, 0x0466D000)
        );
        assert_eq!(
            (
                n.text.data.len(),
                n.ro.data.len(),
                n.data.data.len(),
                n.bss_size
            ),
            (45_185_840, 23_650_556, 5_005_280, 761_888)
        );
        let d = n.dynamic().unwrap();
        assert_eq!(
            (d.rela, d.relasz, d.jmprel, d.pltrelsz, d.symtab, d.strtab),
            (0x02B18050, 0x00BAD308, 0x036C5358, 0x3BE8, 0x036CA620, 0x036CE808)
        );
    }
}

/// The version in an SDK middleware string such as "SDK MW+Nintendo+NintendoSDK_NVN-7_3_2-Release", by its `lib` prefix.
pub fn middleware_version(ro: &[u8], lib: &[u8]) -> Option<(u32, u32, u32)> {
    let tag = [b"SDK MW+Nintendo+".as_slice(), lib].concat();
    let at = ro.windows(tag.len()).position(|w| w == tag)? + tag.len();
    let end = ro[at..].iter().position(|&c| c == b'-' || c == 0)?;
    let mut parts = std::str::from_utf8(&ro[at..at + end]).ok()?.split('_').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next()??))
}
