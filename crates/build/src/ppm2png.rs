// crates/cli/src/ppm2png.rs
// Fast standalone PPM to PNG converter in pure Rust.
// Replaces tools/ppm2png.py with zero external python dependencies.

use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

pub fn convert_ppm_to_png(ppm_path: &Path, png_path: &Path, downscale: usize) -> Result<(), Box<dyn std::error::Error>> {
    let mut f = File::open(ppm_path)?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;

    if !data.starts_with(b"P6") {
        return Err("Not a P6 PPM file".into());
    }

    // Parse header tokens
    let mut pos = 2;
    let mut tokens = Vec::new();
    while tokens.len() < 3 && pos < data.len() {
        // Skip whitespace and comments
        while pos < data.len() && (data[pos].is_ascii_whitespace() || data[pos] == b'#') {
            if data[pos] == b'#' {
                while pos < data.len() && data[pos] != b'\n' {
                    pos += 1;
                }
            } else {
                pos += 1;
            }
        }
        let start = pos;
        while pos < data.len() && !data[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if start < pos {
            let s = std::str::from_utf8(&data[start..pos])?;
            tokens.push(s.parse::<usize>()?);
        }
    }

    if tokens.len() < 3 {
        return Err("Truncated PPM header".into());
    }

    let orig_w = tokens[0];
    let orig_h = tokens[1];
    let _maxval = tokens[2];
    pos += 1; // single whitespace character after maxval

    let rgb = &data[pos..];
    if rgb.len() < orig_w * orig_h * 3 {
        return Err("PPM pixel data is smaller than width * height * 3".into());
    }

    let scale = downscale.max(1);
    let out_w = orig_w / scale;
    let out_h = orig_h / scale;

    // Downscale and format scanlines for PNG (each scanline begins with filter byte 0)
    let mut raw_scanlines = Vec::with_capacity(out_h * (out_w * 3 + 1));
    for y in 0..out_h {
        raw_scanlines.push(0u8); // filter byte: None
        for x in 0..out_w {
            let src_x = x * scale;
            let src_y = y * scale;
            let idx = (src_y * orig_w + src_x) * 3;
            raw_scanlines.push(rgb[idx]);
            raw_scanlines.push(rgb[idx + 1]);
            raw_scanlines.push(rgb[idx + 2]);
        }
    }

    encode_scanlines_to_png(out_w, out_h, &raw_scanlines, png_path)?;
    println!("[+] Converted {} ({}x{}) -> {} ({}x{})", ppm_path.display(), orig_w, orig_h, png_path.display(), out_w, out_h);
    Ok(())
}

/// Encodes raw BGRA pixel bytes directly to a PNG file on disk without writing any intermediate files.
pub fn write_bgra_to_png(w: usize, h: usize, bgra: &[u8], png_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    if bgra.len() < w * h * 4 {
        return Err("BGRA pixel data length is smaller than width * height * 4".into());
    }
    let mut raw_scanlines = Vec::with_capacity(h * (w * 3 + 1));
    for y in 0..h {
        raw_scanlines.push(0u8); // filter byte: None
        for x in 0..w {
            let idx = (y * w + x) * 4;
            raw_scanlines.push(bgra[idx + 2]); // R
            raw_scanlines.push(bgra[idx + 1]); // G
            raw_scanlines.push(bgra[idx]);     // B
        }
    }
    encode_scanlines_to_png(w, h, &raw_scanlines, png_path)
}

fn encode_scanlines_to_png(out_w: usize, out_h: usize, raw_scanlines: &[u8], png_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    // Compress scanlines (zlib stream, the default level as zlib's compress())
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(raw_scanlines)?;
    let comp_buf = enc.finish()?;

    // Write PNG
    let mut out = File::create(png_path)?;
    // 8-byte PNG header
    out.write_all(b"\x89PNG\r\n\x1a\n")?;

    // IHDR chunk
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&(out_w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(out_h as u32).to_be_bytes());
    ihdr.push(8); // bit depth 8
    ihdr.push(2); // color type 2: RGB
    ihdr.push(0); // compression method 0
    ihdr.push(0); // filter method 0
    ihdr.push(0); // interlace method 0
    write_chunk(&mut out, b"IHDR", &ihdr)?;

    // IDAT chunk
    write_chunk(&mut out, b"IDAT", &comp_buf)?;

    // IEND chunk
    write_chunk(&mut out, b"IEND", &[])?;

    Ok(())
}

fn write_chunk(w: &mut File, chunk_type: &[u8; 4], data: &[u8]) -> std::io::Result<()> {
    w.write_all(&(data.len() as u32).to_be_bytes())?;
    w.write_all(chunk_type)?;
    w.write_all(data)?;
    // The CRC covers the chunk type and data together (one running CRC, finalised once).
    let crc = update_crc32(update_crc32(0xFFFF_FFFF, chunk_type), data) ^ 0xFFFF_FFFF;
    w.write_all(&crc.to_be_bytes())?;
    Ok(())
}

fn update_crc32(mut crc: u32, data: &[u8]) -> u32 {
    static CRC_TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let table = CRC_TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for i in 0..256 {
            let mut c = i as u32;
            for _ in 0..8 {
                if c & 1 != 0 {
                    c = 0xEDB88320 ^ (c >> 1);
                } else {
                    c >>= 1;
                }
            }
            t[i] = c;
        }
        t
    });

    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc
}
