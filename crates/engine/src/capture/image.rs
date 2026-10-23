// Comparing textures read back from two backends (or a backend and its references), and images for people to look at.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::gpu::Texels;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Unorm,
    Snorm,
    Uint,
    Sint,
    Float,
}

/// Channels, bytes per channel and kind of an NVN color or depth format whose texels are plain channels in order; BGRA
/// formats are reported as RGBA with `bgra` set.
fn layout(format: i32) -> Option<(usize, usize, Kind, bool)> {
    use Kind::*;
    Some(match format {
        1 => (1, 1, Unorm, false),
        2 => (1, 1, Snorm, false),
        3 | 49 => (1, 1, Uint, false),
        4 => (1, 1, Sint, false),
        5 => (1, 2, Float, false),
        6 | 50 => (1, 2, Unorm, false),
        7 => (1, 2, Snorm, false),
        8 => (1, 2, Uint, false),
        9 => (1, 2, Sint, false),
        10 | 52 => (1, 4, Float, false),
        11 => (1, 4, Uint, false),
        12 => (1, 4, Sint, false),
        13 => (2, 1, Unorm, false),
        14 => (2, 1, Snorm, false),
        15 => (2, 1, Uint, false),
        16 => (2, 1, Sint, false),
        17 => (2, 2, Float, false),
        18 => (2, 2, Unorm, false),
        19 => (2, 2, Snorm, false),
        20 => (2, 2, Uint, false),
        21 => (2, 2, Sint, false),
        22 => (2, 4, Float, false),
        23 => (2, 4, Uint, false),
        24 => (2, 4, Sint, false),
        37 | 55 | 56 | 109 => (4, 1, Unorm, false),
        38 => (4, 1, Snorm, false),
        39 => (4, 1, Uint, false),
        40 => (4, 1, Sint, false),
        41 => (4, 2, Float, false),
        42 => (4, 2, Unorm, false),
        43 => (4, 2, Snorm, false),
        44 => (4, 2, Uint, false),
        45 => (4, 2, Sint, false),
        46 => (4, 4, Float, false),
        47 => (4, 4, Uint, false),
        48 => (4, 4, Sint, false),
        153..=156 => (4, 1, Unorm, true),
        _ => return None,
    })
}

fn half(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    match e {
        0 => s * m * 2f32.powi(-24),
        31 => {
            if m == 0.0 {
                s * f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => s * (1.0 + m / 1024.0) * 2f32.powi(e - 15),
    }
}

/// An unsigned float of `mbits` mantissa and 5 exponent bits (R11G11B10F).
fn ufloat(v: u32, mbits: u32) -> f32 {
    let e = (v >> mbits) as i32;
    let m = (v & ((1 << mbits) - 1)) as f32 / (1u32 << mbits) as f32;
    match e {
        0 => m * 2f32.powi(-14),
        31 => {
            if m == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        _ => (1.0 + m) * 2f32.powi(e - 15),
    }
}

/// Texels as RGBA floats (missing channels 0, alpha 1), and whether the values are integers (compared exactly). None for
/// formats it cannot decode (compressed, packed): those are compared byte for byte.
pub fn decode(t: &Texels) -> Option<(Vec<[f32; 4]>, bool)> {
    let n = (t.width * t.height) as usize;
    if t.format == 61 || t.format == 63 {
        if t.data.len() < n * 4 {
            return None;
        }
        let px = t.data.chunks_exact(4).take(n).map(|c| {
            let v = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            if t.format == 61 {
                let f = |s: u32, b: u32| ((v >> s) & ((1 << b) - 1)) as f32 / ((1u32 << b) - 1) as f32;
                [f(0, 10), f(10, 10), f(20, 10), f(30, 2)]
            } else {
                [ufloat(v & 0x7ff, 6), ufloat((v >> 11) & 0x7ff, 6), ufloat(v >> 22, 5), 1.0]
            }
        });
        return Some((px.collect(), false));
    }
    let (ch, bytes, kind, bgra) = layout(t.format)?;
    if t.data.len() < n * ch * bytes {
        return None;
    }
    let comp = |c: &[u8]| -> f32 {
        match (kind, bytes) {
            (Kind::Unorm, 1) => c[0] as f32 / 255.0,
            (Kind::Snorm, 1) => (c[0] as i8 as f32 / 127.0).max(-1.0),
            (Kind::Uint, 1) => c[0] as f32,
            (Kind::Sint, 1) => c[0] as i8 as f32,
            (Kind::Unorm, 2) => u16::from_le_bytes([c[0], c[1]]) as f32 / 65535.0,
            (Kind::Snorm, 2) => (i16::from_le_bytes([c[0], c[1]]) as f32 / 32767.0).max(-1.0),
            (Kind::Uint, 2) => u16::from_le_bytes([c[0], c[1]]) as f32,
            (Kind::Sint, 2) => i16::from_le_bytes([c[0], c[1]]) as f32,
            (Kind::Float, 2) => half(u16::from_le_bytes([c[0], c[1]])),
            (Kind::Float, 4) => f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
            (Kind::Uint, 4) => u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32,
            (Kind::Sint, 4) => i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32,
            _ => 0.0,
        }
    };
    let px = t.data.chunks_exact(ch * bytes).take(n).map(|p| {
        let mut v = [0.0, 0.0, 0.0, 1.0];
        for (i, c) in p.chunks_exact(bytes).enumerate() {
            v[i] = comp(c);
        }
        if bgra {
            v.swap(0, 2);
        }
        v
    });
    Some((px.collect(), matches!(kind, Kind::Uint | Kind::Sint)))
}

/// How far a result may stray from its reference.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Tolerance {
    /// Largest channel difference that still matches: absolute for normalized formats, relative to max(1, |reference|)
    /// for float formats. Integer formats must match exactly.
    pub max_diff: f32,
    /// Fraction of pixels allowed to exceed max_diff (GPU particles and other nondeterministic work).
    pub max_fraction: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance {
            max_diff: 2.0 / 255.0,
            max_fraction: 0.0,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Diff {
    pub pixels: u64,
    pub differing: u64,
    pub max: f32,
    /// Bounding box of the differing pixels (x0, y0, x1, y1).
    pub bbox: Option<(u32, u32, u32, u32)>,
}

impl Diff {
    pub fn passes(&self, tol: &Tolerance) -> bool {
        self.differing as f64 <= tol.max_fraction * self.pixels as f64
    }
}

/// Compares `out` with `reference`; a size or format mismatch differs everywhere.
pub fn compare(reference: &Texels, out: &Texels, tol: &Tolerance) -> Diff {
    let pixels = (reference.width * reference.height) as u64;
    if (reference.width, reference.height, reference.format) != (out.width, out.height, out.format)
        || reference.data.len() != out.data.len()
    {
        return Diff {
            pixels,
            differing: pixels,
            max: f32::INFINITY,
            bbox: Some((0, 0, reference.width.saturating_sub(1), reference.height.saturating_sub(1))),
        };
    }
    let mut d = Diff {
        pixels,
        ..Default::default()
    };
    let w = reference.width.max(1);
    let mark = |i: usize, d: &mut Diff| {
        let (x, y) = (i as u32 % w, i as u32 / w);
        d.differing += 1;
        d.bbox = Some(match d.bbox {
            None => (x, y, x, y),
            Some((a, b, c, e)) => (a.min(x), b.min(y), c.max(x), e.max(y)),
        });
    };
    match (decode(reference), decode(out)) {
        (Some((a, int)), Some((b, _))) => {
            let float = layout(reference.format).is_some_and(|l| l.2 == Kind::Float) || reference.format == 63;
            for (i, (p, q)) in a.iter().zip(&b).enumerate() {
                let mut worst = 0f32;
                for c in 0..4 {
                    let (x, y) = (p[c], q[c]);
                    let diff = if x.is_nan() || y.is_nan() {
                        if x.is_nan() == y.is_nan() { 0.0 } else { f32::INFINITY }
                    } else if x == y {
                        0.0
                    } else if float {
                        (x - y).abs() / x.abs().max(1.0)
                    } else {
                        (x - y).abs()
                    };
                    worst = worst.max(diff);
                }
                d.max = d.max.max(worst);
                if (int && worst > 0.0) || (!int && worst > tol.max_diff) {
                    mark(i, &mut d);
                }
            }
        }
        _ => {
            let bpp = (reference.data.len() / pixels.max(1) as usize).max(1);
            for (i, (p, q)) in reference.data.chunks(bpp).zip(out.data.chunks(bpp)).enumerate() {
                if p != q {
                    d.max = f32::INFINITY;
                    mark(i, &mut d);
                }
            }
        }
    }
    d
}

/// Texels as 8-bit RGBA for viewing (values clamped to 0..1; integers scaled by their largest value).
pub fn to_rgba8(t: &Texels) -> Vec<u8> {
    let n = (t.width * t.height) as usize;
    let Some((px, int)) = decode(t) else {
        return vec![0; n * 4];
    };
    let scale = if int {
        px.iter().flat_map(|p| p[..3].iter()).fold(1f32, |m, v| m.max(v.abs()))
    } else {
        1.0
    };
    px.iter()
        .flat_map(|p| {
            let c = |v: f32| ((v / scale).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            [c(p[0]), c(p[1]), c(p[2]), 255]
        })
        .collect()
}

/// Reference, result and their difference side by side: differing pixels red over a dimmed reference.
pub fn diff_image(reference: &Texels, out: &Texels, tol: &Tolerance) -> (u32, u32, Vec<u8>) {
    let (w, h) = (reference.width, reference.height);
    let a = to_rgba8(reference);
    let b = if (out.width, out.height) == (w, h) { to_rgba8(out) } else { vec![0; a.len()] };
    let da = decode(reference);
    let db = if (out.width, out.height, out.format) == (w, h, reference.format) { decode(out) } else { None };
    let mut img = vec![0u8; (w * 3 * h * 4) as usize];
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * w as usize + x;
            let row = y * (w as usize * 3);
            img[(row + x) * 4..(row + x) * 4 + 4].copy_from_slice(&a[i * 4..i * 4 + 4]);
            img[(row + w as usize + x) * 4..(row + w as usize + x) * 4 + 4].copy_from_slice(&b[i * 4..i * 4 + 4]);
            let bad = match (&da, &db) {
                (Some((p, int)), Some((q, _))) => {
                    let worst = (0..4).map(|c| (p[i][c] - q[i][c]).abs()).fold(0f32, f32::max);
                    if *int { worst > 0.0 } else { worst > tol.max_diff }
                }
                _ => a[i * 4..i * 4 + 4] != b[i * 4..i * 4 + 4],
            };
            let o = (row + 2 * w as usize + x) * 4;
            if bad {
                img[o..o + 4].copy_from_slice(&[255, 0, 0, 255]);
            } else {
                let g = |v: u8| v / 3;
                img[o..o + 4].copy_from_slice(&[g(a[i * 4]), g(a[i * 4 + 1]), g(a[i * 4 + 2]), 255]);
            }
        }
    }
    (w * 3, h, img)
}

fn crc32(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    crc
}

/// Writes 8-bit RGBA rows as a PNG.
pub fn write_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> Result<(), String> {
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for row in rgba.chunks_exact(w as usize * 4).take(h as usize) {
        raw.push(0);
        raw.extend_from_slice(row);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw).map_err(|e| e.to_string())?;
    let idat = z.finish().map_err(|e| e.to_string())?;
    let mut out = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let crc = crc32(crc32(0xFFFF_FFFF, kind), data) ^ 0xFFFF_FFFF;
        out.extend_from_slice(&crc.to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &idat);
    chunk(b"IEND", &[]);
    std::fs::write(path, out).map_err(|e| format!("{}: {e}", path.display()))
}
