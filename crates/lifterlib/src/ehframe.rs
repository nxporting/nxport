// Function starts from a module's .eh_frame_hdr binary search table (located through MOD0).

/// Reads a DWARF exception-header encoded value at `off`: (value, size). `pc` is the value's own address (pcrel),
/// `data` the start of .eh_frame_hdr (datarel).
fn read_encoded(img: &[u8], off: usize, enc: u8, pc: u64, data: u64) -> Option<(u64, usize)> {
    let le = |n: usize| -> Option<u64> {
        let b = img.get(off..off + n)?;
        let mut v = 0u64;
        for (i, &x) in b.iter().enumerate() {
            v |= (x as u64) << (8 * i);
        }
        Some(v)
    };
    let (raw, size) = match enc & 0x0f {
        0x00 | 0x04 | 0x0c => (le(8)?, 8),
        0x02 => (le(2)?, 2),
        0x03 => (le(4)?, 4),
        0x0a => (le(2)? as i16 as i64 as u64, 2),
        0x0b => (le(4)? as i32 as i64 as u64, 4),
        _ => return None, // uleb/sleb never appear in the search table
    };
    let base = match enc & 0x70 {
        0x00 => 0,
        0x10 => pc,
        0x30 => data,
        _ => return None,
    };
    Some((base.wrapping_add(raw), size))
}

/// The function start addresses in the .eh_frame_hdr at image offset `hdr` of an image loaded at `base`.
pub fn hdr_functions(img: &[u8], base: u64, hdr: usize) -> Vec<u64> {
    let mut out = Vec::new();
    let Some(&[version, ptr_enc, count_enc, table_enc]) = img.get(hdr..hdr + 4).map(|s| <&[u8; 4]>::try_from(s).unwrap())
    else {
        return out;
    };
    if version != 1 || table_enc == 0xff {
        return out;
    }
    let data = base + hdr as u64;
    let mut off = hdr + 4;
    let Some((_, n)) = read_encoded(img, off, ptr_enc, base + off as u64, data) else { return out };
    off += n;
    let Some((count, n)) = read_encoded(img, off, count_enc, base + off as u64, data) else { return out };
    off += n;
    for _ in 0..count {
        let Some((loc, n)) = read_encoded(img, off, table_enc, base + off as u64, data) else { break };
        off += n;
        let Some((_, n)) = read_encoded(img, off, table_enc, base + off as u64, data) else { break };
        off += n;
        out.push(loc);
    }
    out
}

/// A function's C++ exception landing pads (from its FDE's LSDA call-site table): where the unwinder resumes it.
#[derive(Debug, PartialEq, Eq)]
pub struct LandingPads {
    pub start: u64,
    pub end: u64,
    pub pads: Vec<u64>,
}

fn uleb(img: &[u8], off: &mut usize) -> Option<u64> {
    let (mut v, mut shift) = (0u64, 0);
    loop {
        let b = *img.get(*off)?;
        *off += 1;
        if shift < 64 {
            v |= ((b & 0x7f) as u64) << shift;
        }
        shift += 7;
        if b & 0x80 == 0 {
            return Some(v);
        }
    }
}

/// read_encoded for .eh_frame and LSDA fields: LEB128 too, pc-relative to the field itself, `func` for funcrel.
fn read_field(img: &[u8], off: &mut usize, enc: u8, base: u64, func: u64) -> Option<u64> {
    if enc == 0xff {
        return Some(0);
    }
    let at = base + *off as u64;
    let raw = match enc & 0x0f {
        0x01 => uleb(img, off)?,
        0x09 => {
            let (mut v, mut shift) = (0i64, 0);
            loop {
                let b = *img.get(*off)?;
                *off += 1;
                v |= ((b & 0x7f) as i64) << shift;
                shift += 7;
                if b & 0x80 == 0 {
                    if shift < 64 && b & 0x40 != 0 {
                        v |= -1i64 << shift;
                    }
                    break v as u64;
                }
            }
        }
        _ => {
            let (v, n) = read_encoded(img, *off, enc & 0x0f, 0, 0)?;
            *off += n;
            v
        }
    };
    let base = match enc & 0x70 {
        0x00 => 0,
        0x10 => at,
        0x40 => func,
        _ => return None,
    };
    Some(base.wrapping_add(raw))
}

/// The landing pads of every function in the .eh_frame_hdr at image offset `hdr` that has any (an image loaded at
/// `base`): each FDE's CIE gives its augmentation ("z" with an "L" LSDA encoding), the FDE its LSDA, and the LSDA's
/// call-site table the pads (offsets from the function's start; 0: none).
pub fn landing_pads(img: &[u8], base: u64, hdr: usize) -> Vec<LandingPads> {
    let mut out = Vec::new();
    let Some(&[1, ptr_enc, count_enc, table_enc]) = img.get(hdr..hdr + 4).map(|s| <&[u8; 4]>::try_from(s).unwrap()) else {
        return out;
    };
    let data = base + hdr as u64;
    let mut off = hdr + 4;
    let Some((_, n)) = read_encoded(img, off, ptr_enc, base + off as u64, data) else { return out };
    off += n;
    let Some((count, n)) = read_encoded(img, off, count_enc, base + off as u64, data) else { return out };
    off += n;
    for _ in 0..count {
        let Some((_, n)) = read_encoded(img, off, table_enc, base + off as u64, data) else { break };
        off += n;
        let Some((fde, n)) = read_encoded(img, off, table_enc, base + off as u64, data) else { break };
        off += n;
        if let Some(lp) = fde_landing_pads(img, base, fde.wrapping_sub(base) as usize) {
            out.push(lp);
        }
    }
    out
}

fn fde_landing_pads(img: &[u8], base: u64, fde: usize) -> Option<LandingPads> {
    let u32_at = |o: usize| img.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let len = u32_at(fde)? as usize;
    if len == 0 || len == 0xffff_ffff {
        return None;
    }
    let cie_ptr = u32_at(fde + 4)? as usize;
    let cie = (fde + 4).checked_sub(cie_ptr)?;
    // CIE: length, id 0, version, augmentation string, code / data alignment, return register, then "z" data.
    let mut o = cie + 9;
    let aug_start = o;
    while *img.get(o)? != 0 {
        o += 1;
    }
    let aug = img.get(aug_start..o)?.to_vec();
    o += 1;
    if aug.first() != Some(&b'z') {
        return None;
    }
    uleb(img, &mut o)?; // code alignment
    uleb(img, &mut o)?; // data alignment (sleb; skipped as bytes)
    if *img.get(cie + 8)? == 1 {
        o += 1;
    } else {
        uleb(img, &mut o)?;
    }
    uleb(img, &mut o)?; // augmentation data length
    let (mut lsda_enc, mut fde_enc) = (0xffu8, 0u8);
    for &c in &aug[1..] {
        match c {
            b'P' => {
                let e = *img.get(o)?;
                o += 1;
                read_field(img, &mut o, e & 0x7f, base, 0)?;
            }
            b'L' => {
                lsda_enc = *img.get(o)?;
                o += 1;
            }
            b'R' => {
                fde_enc = *img.get(o)?;
                o += 1;
            }
            b'S' | b'B' => {}
            _ => return None,
        }
    }
    if lsda_enc == 0xff {
        return None;
    }
    let mut o = fde + 8;
    let start = read_field(img, &mut o, fde_enc, base, 0)?;
    let range = read_field(img, &mut o, fde_enc & 0x0f, base, 0)?;
    uleb(img, &mut o)?; // augmentation data length
    let lsda = read_field(img, &mut o, lsda_enc, base, 0)?;
    if lsda == 0 {
        return None;
    }
    // LSDA: landing pad base, type table, call-site table (start, length, landing pad, action).
    let mut o = lsda.checked_sub(base)? as usize;
    let lp_enc = *img.get(o)?;
    o += 1;
    let lp_start = if lp_enc == 0xff { start } else { read_field(img, &mut o, lp_enc, base, start)? };
    let tt_enc = *img.get(o)?;
    o += 1;
    if tt_enc != 0xff {
        uleb(img, &mut o)?;
    }
    let cs_enc = *img.get(o)?;
    o += 1;
    let cs_len = uleb(img, &mut o)? as usize;
    let cs_end = o + cs_len;
    let mut pads = Vec::new();
    while o < cs_end {
        read_field(img, &mut o, cs_enc, base, 0)?;
        read_field(img, &mut o, cs_enc, base, 0)?;
        let pad = read_field(img, &mut o, cs_enc, base, 0)?;
        uleb(img, &mut o)?;
        if pad != 0 && !pads.contains(&(lp_start + pad)) {
            pads.push(lp_start + pad);
        }
    }
    pads.sort_unstable();
    (!pads.is_empty()).then_some(LandingPads { start, end: start + range, pads })
}

/// The image offset of .eh_frame_hdr from the module's MOD0 header (the word at image offset 4 points to it).
pub fn mod0_eh_frame_hdr(img: &[u8]) -> Option<usize> {
    let u32_at = |o: usize| img.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let mod0 = u32_at(4)? as usize;
    if img.get(mod0..mod0 + 4)? != b"MOD0" {
        return None;
    }
    // MOD0: magic, dynamic, bss start, bss end, eh_frame_hdr start, eh_frame_hdr end, module object (each relative to
    // MOD0)
    let rel = u32_at(mod0 + 4 + 3 * 4)? as i32;
    let hdr = (mod0 as i64 + rel as i64) as usize;
    (hdr != mod0 && hdr < img.len()).then_some(hdr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_table() {
        // version 1, eh_frame_ptr pcrel|sdata4, count udata4, table datarel|sdata4; two entries
        let mut img = vec![0u8; 0x100];
        let hdr = 0x40;
        img[hdr..hdr + 4].copy_from_slice(&[1, 0x1b, 0x03, 0x3b]);
        img[hdr + 4..hdr + 8].copy_from_slice(&0i32.to_le_bytes());
        img[hdr + 8..hdr + 12].copy_from_slice(&2u32.to_le_bytes());
        img[hdr + 12..hdr + 16].copy_from_slice(&(-0x40i32).to_le_bytes()); // function at image offset 0
        img[hdr + 20..hdr + 24].copy_from_slice(&(-0x30i32).to_le_bytes()); // and at 0x10
        assert_eq!(hdr_functions(&img, 0x7100_0000, hdr), vec![0x7100_0000, 0x7100_0010]);
    }
}
