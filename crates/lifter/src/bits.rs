// Bit-field helpers for decoding A64 instruction words.

/// `n` bits of `w` starting at bit `lo`.
pub fn bits(w: u32, lo: u32, n: u32) -> u32 {
    (w >> lo) & ((1u32 << n) - 1)
}

/// Sign-extends the low `n` bits of `v`.
pub fn sext(v: u64, n: u32) -> i64 {
    let shift = 64 - n;
    ((v << shift) as i64) >> shift
}

/// The low `n` bits set (n <= 64).
pub fn ones(n: u32) -> u64 {
    if n >= 64 {
        u64::MAX
    } else {
        (1u64 << n) - 1
    }
}

/// Rotate right within a `size`-bit value.
pub fn ror(v: u64, r: u32, size: u32) -> u64 {
    let r = r % size;
    if r == 0 {
        v
    } else {
        ((v >> r) | (v << (size - r))) & ones(size)
    }
}

fn replicate(v: u64, esize: u32, size: u32) -> u64 {
    let mut out = 0;
    let mut i = 0;
    while i < size {
        out |= v << i;
        i += esize;
    }
    out
}

/// DecodeBitMasks (Arm ARM): the (wmask, tmask) of a logical immediate or bitfield instruction, or None for a
/// reserved encoding.
pub fn decode_bit_masks(n: u32, imms: u32, immr: u32, size: u32, immediate: bool) -> Option<(u64, u64)> {
    let x = (n << 6) | (!imms & 0x3f);
    if x == 0 {
        return None;
    }
    let len = 31 - x.leading_zeros(); // highest set bit
    if len < 1 {
        return None;
    }
    let levels = ones(len) as u32;
    if immediate && (imms & levels) == levels {
        return None;
    }
    let s = imms & levels;
    let r = immr & levels;
    let d = (s.wrapping_sub(r)) & levels;
    let esize = 1u32 << len;
    let welem = ones(s + 1);
    let telem = ones(d + 1);
    Some((replicate(ror(welem, r, esize), esize, size), replicate(telem, esize, size)))
}

/// A C unsigned 64-bit literal.
pub fn hx(v: u64) -> String {
    format!("0x{v:x}ull")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bit_masks() {
        // and x0, x0, #0xff: N=1 immr=0 imms=7
        assert_eq!(decode_bit_masks(1, 7, 0, 64, true).unwrap().0, 0xff);
        // and w0, w0, #0x80000000: N=0 immr=1 imms=0
        assert_eq!(decode_bit_masks(0, 0, 1, 32, true).unwrap().0, 0x8000_0000);
        // orr x0, xzr, #0x5555555555555555: N=0 immr=0 imms=0b111100
        assert_eq!(decode_bit_masks(0, 0b111100, 0, 64, true).unwrap().0, 0x5555_5555_5555_5555);
        // all ones is reserved for immediates
        assert!(decode_bit_masks(1, 0b111111, 0, 64, true).is_none());
        // lsr x0, x1, #4 is ubfm x0, x1, #4, #63
        let (w, t) = decode_bit_masks(1, 63, 4, 64, false).unwrap();
        assert_eq!(ror(u64::MAX, 4, 64) & w & t, u64::MAX >> 4);
    }

    #[test]
    fn sign_extend() {
        assert_eq!(sext(0x3ff_ffff, 26), -1);
        assert_eq!(sext(0x1ff_ffff, 26), 0x1ff_ffff);
    }
}
