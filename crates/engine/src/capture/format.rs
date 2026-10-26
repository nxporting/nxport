// What a frame capture holds, and its files.
//
// A capture directory:
//   frame.bin      the frame: NVN frontend state, the command lists its submits reach, the submits and the present
//   memory.bin     guest memory the frame reaches, at its addresses, as it stood when the frame had been submitted
//   textures.bin   texture contents only the GPU held then (rendered in earlier frames), in the guest's texel encoding;
//                  replays write them into guest memory before the frame runs, where the Switch's GPU would have left them
//   presented.bin  the image the game presented for this frame (Texels), as the capturing run rendered it
//   ref/           references a replay recorded (see replay.rs)
// All of them are zstd-compressed bincode, except memory.bin's page stream.

use std::io::{Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::gpu::Texels;
use crate::nvn::cmd::Cmd;

pub const VERSION: u32 = 1;
/// Granularity of memory.bin's pages and of the ranges it holds.
pub const PAGE: usize = 0x4000;
/// Ranges start and end on this boundary, so a replay can reserve them where the host allocates in 64 KiB units.
pub const RANGE_ALIGN: usize = 0x10000;

#[derive(Serialize, Deserialize)]
pub struct Frame {
    pub version: u32,
    pub title: String,
    /// The presented frame number in the capturing run.
    pub frame: u64,
    pub frontend: Frontend,
    /// Command list handles of each nvnQueueSubmitCommands, in order.
    pub submits: Vec<Vec<u64>>,
    /// Every list the submits reach (through CallCommands too), by handle.
    pub lists: Vec<(u64, Vec<Op>)>,
    /// The window texture presented at the end of the frame (NVNtexture address) and its crop.
    pub present: Option<(u64, [i32; 4])>,
}

/// One recorded command.
#[derive(Clone, Serialize, Deserialize)]
pub struct Op {
    pub name: String,
    pub typed: Option<Cmd>,
    pub args: Option<[u64; 5]>,
}

/// The NVN frontend's state that the backend consults while it runs recorded commands (nvn::api).
#[derive(Default, Serialize, Deserialize)]
pub struct Frontend {
    /// Memory pools: (base, size, flags).
    pub pools: Vec<(u64, u64, i32)>,
    /// Virtual pool mappings: (virtual start, size, physical start).
    pub virtual_maps: Vec<(u64, u64, u64)>,
    /// Texture snapshots by object address: (address, generation, NVNtexture bytes).
    pub textures: Vec<(u64, u64, Vec<u8>)>,
    /// Objects in the storage index (initialized and not finalized).
    pub indexed: Vec<u64>,
    /// Replaced snapshots by generation (low 32 bits).
    pub history: Vec<(u32, Vec<u8>)>,
    pub next_gen: u64,
    pub max_texture_span: u64,
    pub pool_textures: Vec<PoolTexture>,
    pub pool_samplers: Vec<(i32, crate::nvn::api::SamplerDesc)>,
    /// Programs: (NVNprogram address, [(code, control section)]).
    pub programs: Vec<(u64, Vec<(u64, u64)>)>,
    pub window_origin_mode: i32,
    pub depth_mode: i32,
}

/// A texture pool registration.
#[derive(Clone, Serialize, Deserialize)]
pub struct PoolTexture {
    pub id: i32,
    pub obj: u64,
    pub gen: u64,
    pub state: Vec<u8>,
    pub view: Option<Vec<u8>>,
}

/// A texture level / layer only the GPU held, to be written into guest memory at `storage` with `params`' layout.
#[derive(Serialize, Deserialize)]
pub struct GpuTexture {
    pub storage: u64,
    pub params: crate::nvn::layout::TextureParams,
    pub level: u32,
    pub layer: u32,
    pub last_written: u64,
    pub texels: Texels,
}

pub fn pod_bytes<T: Copy>(v: &T) -> Vec<u8> {
    unsafe { std::slice::from_raw_parts(v as *const T as *const u8, std::mem::size_of::<T>()) }.to_vec()
}

/// A `T` from bytes pod_bytes wrote (shorter input is zero-extended).
pub fn from_pod<T: Copy>(b: &[u8]) -> T {
    let mut v = std::mem::MaybeUninit::<T>::zeroed();
    let n = b.len().min(std::mem::size_of::<T>());
    unsafe {
        std::ptr::copy_nonoverlapping(b.as_ptr(), v.as_mut_ptr() as *mut u8, n);
        v.assume_init()
    }
}

pub fn write_bin<T: Serialize>(path: &Path, v: &T) -> Result<(), String> {
    let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zstd::Encoder::new(std::io::BufWriter::new(f), 3).map_err(|e| e.to_string())?;
    bincode::serialize_into(&mut z, v).map_err(|e| format!("{}: {e}", path.display()))?;
    z.finish().and_then(|mut w| w.flush()).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn read_bin<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let z = zstd::Decoder::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
    bincode::deserialize_from(z).map_err(|e| format!("{}: {e}", path.display()))
}

/// Writes a sequence of values to one zstd stream (textures.bin): each value prefixed by a 1, then a 0.
pub struct SeqWriter(zstd::Encoder<'static, std::io::BufWriter<std::fs::File>>);

impl SeqWriter {
    pub fn create(path: &Path) -> Result<Self, String> {
        let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(SeqWriter(zstd::Encoder::new(std::io::BufWriter::new(f), 3).map_err(|e| e.to_string())?))
    }
    pub fn push<T: Serialize>(&mut self, v: &T) -> Result<(), String> {
        self.0.write_all(&[1]).map_err(|e| e.to_string())?;
        bincode::serialize_into(&mut self.0, v).map_err(|e| e.to_string())
    }
    pub fn finish(mut self) -> Result<(), String> {
        self.0.write_all(&[0]).map_err(|e| e.to_string())?;
        self.0.finish().and_then(|mut w| w.flush()).map_err(|e| e.to_string())
    }
}

/// Reads what SeqWriter wrote, one value at a time.
pub struct SeqReader(zstd::Decoder<'static, std::io::BufReader<std::fs::File>>, std::path::PathBuf);

impl SeqReader {
    pub fn open(path: &Path) -> Result<Self, String> {
        let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(SeqReader(zstd::Decoder::new(file).map_err(|e| e.to_string())?, path.to_path_buf()))
    }
    /// The next value; None after the last.
    pub fn next<T: for<'a> Deserialize<'a>>(&mut self) -> Result<Option<T>, String> {
        let mut tag = [0u8];
        self.0.read_exact(&mut tag).map_err(|e| format!("{}: {e}", self.1.display()))?;
        if tag[0] == 0 {
            return Ok(None);
        }
        bincode::deserialize_from(&mut self.0).map(Some).map_err(|e| format!("{}: {e}", self.1.display()))
    }
}

const MEMORY_MAGIC: &[u8; 8] = b"NXMEM1\0\0";

/// memory.bin: the magic, the range count, then per range its start and length and per PAGE a byte (0: zeros, 1: the
/// page follows), all in one zstd stream.
pub struct MemoryWriter {
    z: zstd::Encoder<'static, std::io::BufWriter<std::fs::File>>,
    pub bytes: u64,
    pub zero_pages: u64,
}

impl MemoryWriter {
    pub fn create(path: &Path, ranges: usize) -> Result<Self, String> {
        let f = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut z = zstd::Encoder::new(std::io::BufWriter::new(f), 1).map_err(|e| e.to_string())?;
        z.write_all(MEMORY_MAGIC).map_err(|e| e.to_string())?;
        z.write_all(&(ranges as u64).to_le_bytes()).map_err(|e| e.to_string())?;
        Ok(MemoryWriter { z, bytes: 0, zero_pages: 0 })
    }
    pub fn begin_range(&mut self, start: u64, len: u64) -> Result<(), String> {
        self.z.write_all(&start.to_le_bytes()).map_err(|e| e.to_string())?;
        self.z.write_all(&len.to_le_bytes()).map_err(|e| e.to_string())
    }
    /// One page of the current range; None: unreadable, stored as zeros.
    pub fn page(&mut self, data: Option<&[u8]>) -> Result<(), String> {
        match data {
            Some(d) if d.iter().any(|&b| b != 0) => {
                self.z.write_all(&[1]).map_err(|e| e.to_string())?;
                self.z.write_all(d).map_err(|e| e.to_string())?;
                self.bytes += d.len() as u64;
            }
            _ => {
                self.z.write_all(&[0]).map_err(|e| e.to_string())?;
                self.zero_pages += 1;
            }
        }
        Ok(())
    }
    pub fn finish(self) -> Result<(), String> {
        self.z.finish().and_then(|mut w| w.flush()).map_err(|e| e.to_string())
    }
}

/// Reads memory.bin: `range(start, len)` for each range before its pages, then `page(addr, bytes)` for each page that
/// is not all zeros.
pub fn read_memory(
    path: &Path,
    mut range: impl FnMut(u64, u64) -> Result<(), String>,
    mut page: impl FnMut(u64, &[u8]) -> Result<(), String>,
) -> Result<(), String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut z = zstd::Decoder::new(std::io::BufReader::new(f)).map_err(|e| e.to_string())?;
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut magic = [0u8; 8];
    z.read_exact(&mut magic).map_err(err)?;
    if &magic != MEMORY_MAGIC {
        return Err(format!("{}: not a memory capture", path.display()));
    }
    let mut w = [0u8; 8];
    z.read_exact(&mut w).map_err(err)?;
    let n = u64::from_le_bytes(w);
    let mut buf = vec![0u8; PAGE];
    for _ in 0..n {
        z.read_exact(&mut w).map_err(err)?;
        let start = u64::from_le_bytes(w);
        z.read_exact(&mut w).map_err(err)?;
        let len = u64::from_le_bytes(w);
        range(start, len)?;
        for p in 0..len / PAGE as u64 {
            let mut k = [0u8];
            z.read_exact(&mut k).map_err(err)?;
            if k[0] == 1 {
                z.read_exact(&mut buf).map_err(err)?;
                page(start + p * PAGE as u64, &buf)?;
            }
        }
    }
    Ok(())
}

/// Merges [start, end) ranges into sorted, disjoint ones aligned to RANGE_ALIGN.
pub fn merge_ranges(mut v: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    let a = RANGE_ALIGN as u64;
    for r in v.iter_mut() {
        *r = (r.0 / a * a, r.1.div_ceil(a) * a);
    }
    v.retain(|r| r.1 > r.0);
    v.sort_unstable();
    let mut out: Vec<(u64, u64)> = Vec::new();
    for (s, e) in v {
        match out.last_mut() {
            Some(l) if s <= l.1 => l.1 = l.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}
