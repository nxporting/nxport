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
