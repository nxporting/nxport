// The shader cache's source pack (<cache>/source.pack): translated stages as Metal source for an app built without
// Apple's Metal compiler (an .ipa built on Windows). The app compiles a stage from it the first time the game uses it,
// without translating it. Each stage is compressed on its own with one shared dictionary, so a stage is read without
// the rest; loose files were ~1 GB for TotK's 25,000 stages.
//
// Layout (little-endian): "NXMSRC01", u32 dictionary length, the dictionary, u32 stage count, then per stage sorted by
// name: u16 name length, the name (the entry point, s_<hash>), u64 offset from the start of the file, u32 compressed
// length, u32 source length; then the compressed stages.

use std::collections::HashMap;

const MAGIC: &[u8; 8] = b"NXMSRC01";
/// Dictionary size for zstd's trainer; stages share much of their code around the translated body.
const DICT_SIZE: usize = 256 * 1024;
const LEVEL: i32 = 19;

/// Writes `stages` (entry point, source) as a pack at `path`.
pub fn write(path: &std::path::Path, stages: &mut [(String, Vec<u8>)]) -> Result<(), String> {
    stages.sort_by(|a, b| a.0.cmp(&b.0));
    let samples: Vec<&[u8]> = stages.iter().map(|s| s.1.as_slice()).collect();
    // Training needs enough samples; with few stages they go in without one.
    let dict = if samples.len() >= 64 { zstd::dict::from_samples(&samples, DICT_SIZE).unwrap_or_default() } else { Vec::new() };
    let mut c = zstd::bulk::Compressor::with_dictionary(LEVEL, &dict).map_err(|e| e.to_string())?;
    let blobs: Vec<Vec<u8>> = stages.iter().map(|s| c.compress(&s.1).map_err(|e| e.to_string())).collect::<Result<_, _>>()?;
    let mut head = Vec::new();
    head.extend_from_slice(MAGIC);
    head.extend_from_slice(&(dict.len() as u32).to_le_bytes());
    head.extend_from_slice(&dict);
    head.extend_from_slice(&(stages.len() as u32).to_le_bytes());
    let table_len: usize = stages.iter().map(|s| 2 + s.0.len() + 8 + 4 + 4).sum();
    let mut off = (head.len() + table_len) as u64;
    for ((name, src), blob) in stages.iter().zip(&blobs) {
        head.extend_from_slice(&(name.len() as u16).to_le_bytes());
        head.extend_from_slice(name.as_bytes());
        head.extend_from_slice(&off.to_le_bytes());
        head.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        head.extend_from_slice(&(src.len() as u32).to_le_bytes());
        off += blob.len() as u64;
    }
    let mut out = head;
    for b in &blobs {
        out.extend_from_slice(b);
    }
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}

/// A pack read into memory, its stages decompressed on request.
pub struct Pack {
    data: Vec<u8>,
    dict: std::ops::Range<usize>,
    /// Entry point -> (offset, compressed length, source length).
    stages: HashMap<String, (usize, usize, usize)>,
}

impl Pack {
    pub fn open(path: &std::path::Path) -> Option<Pack> {
        let data = std::fs::read(path).ok()?;
        let u32_at = |o: usize| -> Option<usize> { Some(u32::from_le_bytes(data.get(o..o + 4)?.try_into().ok()?) as usize) };
        if data.get(..8)? != MAGIC {
            return None;
        }
        let dict_len = u32_at(8)?;
        let dict = 12..12 + dict_len;
        let mut p = dict.end;
        let n = u32_at(p)?;
        p += 4;
        let mut stages = HashMap::with_capacity(n);
        for _ in 0..n {
            let len = u16::from_le_bytes(data.get(p..p + 2)?.try_into().ok()?) as usize;
            let name = std::str::from_utf8(data.get(p + 2..p + 2 + len)?).ok()?.to_string();
            p += 2 + len;
            let off = u64::from_le_bytes(data.get(p..p + 8)?.try_into().ok()?) as usize;
            let (clen, slen) = (u32_at(p + 8)?, u32_at(p + 12)?);
            p += 16;
            data.get(off..off + clen)?;
            stages.insert(name, (off, clen, slen));
        }
        Some(Pack { data, dict, stages })
    }

    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    /// The source of stage `entry`.
    pub fn source(&self, entry: &str) -> Option<String> {
        let &(off, clen, slen) = self.stages.get(entry)?;
        let mut d = zstd::bulk::Decompressor::with_dictionary(&self.data[self.dict.clone()]).ok()?;
        let src = d.decompress(&self.data[off..off + clen], slen).ok()?;
        String::from_utf8(src).ok()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join(format!("nxport_source_pack_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("source.pack");
        let mut stages: Vec<(String, Vec<u8>)> = (0..100)
            .map(|i| (format!("s_{i:016x}"), format!("#include <metal_stdlib>\nkernel void s_{i:016x}() {{ /* {i} */ }}\n").into_bytes()))
            .collect();
        let want = stages.clone();
        super::write(&path, &mut stages).unwrap();
        let p = super::Pack::open(&path).unwrap();
        assert_eq!(p.len(), 100);
        for (name, src) in &want {
            assert_eq!(p.source(name).unwrap().as_bytes(), src.as_slice());
        }
        assert!(p.source("s_missing").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
