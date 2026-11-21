// Completing one faulting guest load without the page it touches being accessible: the plain load forms (register with
// unsigned / unscaled / pre / post-indexed immediate, register offset, pair; general and SIMD registers) and
// load-acquire. The caller supplies where the bytes can be read instead; anything else (stores, exclusives, atomics,
// prefetches) is left alone. Stores are left to wait on purpose: one wait opens the page for every store after it,
// while completing each store here faults on every one of them (Breath of the Wild walking: 3% slower).

/// Byte offset of the NEON registers in Darwin's arm64 mcontext: __es (16 bytes) then __ss (272 bytes).
const NEON_OFFSET: usize = 16 + 272;

struct Load {
    /// Registers written, in order, each `bytes` wide: (register, SIMD).
    regs: [(usize, bool); 2],
    count: usize,
    bytes: usize,
    /// Sign-extend general registers to 64 bits (Some(false): to 32 bits, upper half zero).
    sext: Option<bool>,
    rn: usize,
    /// Address offset from Rn for the access, and Rn's write-back (pre / post indexing).
    offset: i64,
    writeback: Option<i64>,
}

fn sext(v: u64, bits: u32) -> i64 {
    ((v << (64 - bits)) as i64) >> (64 - bits)
}

/// Size and signedness of a general-register load from its size / opc fields; None for stores and prefetches.
fn gpr_load(size: u32, opc: u32) -> Option<(usize, Option<bool>)> {
    match (opc, size) {
        (1, _) => Some((1 << size, None)),
        (2, 0..=2) => Some((1 << size, Some(true))),
        (3, 0..=1) => Some((1 << size, Some(false))),
        _ => None,
    }
}

/// Bytes of a SIMD load from its size / opc fields (opc bit 1 with size 0 is the 128-bit form); None for stores.
fn simd_load(size: u32, opc: u32) -> Option<usize> {
    if opc & 1 == 0 {
        return None;
    }
    match (opc & 2 != 0, size) {
        (false, s) => Some(1 << s),
        (true, 0) => Some(16),
        _ => None,
    }
}

fn decode(insn: u32, reg: impl Fn(usize) -> u64) -> Option<Load> {
    let rt = (insn & 31) as usize;
    let rn = ((insn >> 5) & 31) as usize;
    let size = insn >> 30;
    let simd = (insn >> 26) & 1 == 1;
    let opc = (insn >> 22) & 3;
    let single = |bytes: usize, sx: Option<bool>, offset: i64, writeback: Option<i64>| Load {
        regs: [(rt, simd), (0, false)],
        count: 1,
        bytes,
        sext: sx,
        rn,
        offset,
        writeback,
    };
    let sized = || -> Option<(usize, Option<bool>)> {
        if simd {
            simd_load(size, opc).map(|b| (b, None))
        } else {
            gpr_load(size, opc)
        }
    };
    // LDAR / LDAPR (load-acquire, not exclusive): no offset. The caller orders the read with an acquire fence.
    if insn & 0x3fff_fc00 == 0x08df_fc00 || insn & 0x3fff_fc00 == 0x38bf_c000 {
        return Some(Load { regs: [(rt, false), (0, false)], count: 1, bytes: 1 << size, sext: None, rn, offset: 0, writeback: None });
    }
    if insn & 0x3b00_0000 == 0x3900_0000 {
        let (bytes, sx) = sized()?;
        let imm = ((insn >> 10) & 0xfff) as i64 * bytes as i64;
        return Some(single(bytes, sx, imm, None));
    }
    if insn & 0x3b20_0000 == 0x3800_0000 {
        let (bytes, sx) = sized()?;
        let imm = sext(((insn >> 12) & 0x1ff) as u64, 9);
        return Some(match (insn >> 10) & 3 {
            1 => single(bytes, sx, 0, Some(imm)),
            3 => single(bytes, sx, imm, Some(imm)),
            _ => single(bytes, sx, imm, None),
        });
    }
    if insn & 0x3b20_0c00 == 0x3820_0800 {
        let (bytes, sx) = sized()?;
        let m = reg((insn >> 16 & 31) as usize);
        let ext = match (insn >> 13) & 7 {
            2 => m as u32 as i64,
            3 | 7 => m as i64,
            6 => m as u32 as i32 as i64,
            _ => return None,
        };
        let shift = if (insn >> 12) & 1 == 1 { bytes.trailing_zeros() } else { 0 };
        return Some(single(bytes, sx, ext << shift, None));
    }
    if insn & 0x3a00_0000 == 0x2800_0000 && (insn >> 22) & 1 == 1 {
        let (bytes, sx) = match (simd, size) {
            (true, 0..=2) => (4usize << size, None),
            (false, 0) => (4, None),
            (false, 1) => (4, Some(true)),
            (false, 2) => (8, None),
            _ => return None,
        };
        let imm = sext(((insn >> 15) & 0x7f) as u64, 7) * bytes as i64;
        let rt2 = ((insn >> 10) & 31) as usize;
        let (offset, writeback) = match (insn >> 23) & 3 {
            1 => (0, Some(imm)),
            3 => (imm, Some(imm)),
            _ => (imm, None),
        };
        return Some(Load { regs: [(rt, simd), (rt2, simd)], count: 2, bytes, sext: sx, rn, offset, writeback });
    }
    None
}

/// Completes the load at the faulting pc of `mc` (a Darwin arm64 mcontext) if it is one of the plain forms, reading
/// its bytes with `read(address, buffer)` (false: they cannot be read there). Returns whether it did; the context then
/// continues after the instruction.
pub(crate) unsafe fn emulate(mc: *mut libc::__darwin_mcontext64, read: impl Fn(usize, &mut [u8]) -> bool) -> bool {
    let ss = &mut (*mc).__ss;
    let insn = *(ss.__pc as *const u32);
    let x = |n: usize, sp: bool| -> u64 {
        match n {
            0..=28 => ss.__x[n],
            29 => ss.__fp,
            30 => ss.__lr,
            _ if sp => ss.__sp,
            _ => 0,
        }
    };
    let Some(l) = decode(insn, |n| x(n, false)) else {
        return false;
    };
    let base = x(l.rn, true);
    let addr = (base as i64).wrapping_add(l.offset) as usize;
    let mut buf = [0u8; 32];
    let total = l.bytes * l.count;
    if !read(addr, &mut buf[..total]) {
        return false;
    }
    std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
    for k in 0..l.count {
        let (r, simd) = l.regs[k];
        let b = &buf[k * l.bytes..(k + 1) * l.bytes];
        if simd {
            let mut v = [0u8; 16];
            v[..l.bytes].copy_from_slice(b);
            let p = (mc as *mut u8).add(NEON_OFFSET + r * 16) as *mut [u8; 16];
            p.write_unaligned(v);
            continue;
        }
        let mut raw = [0u8; 8];
        raw[..l.bytes].copy_from_slice(b);
        let mut v = u64::from_le_bytes(raw);
        match l.sext {
            Some(true) => v = sext(v, l.bytes as u32 * 8) as u64,
            Some(false) => v = sext(v, l.bytes as u32 * 8) as u64 & 0xffff_ffff,
            None => {}
        }
        let ss = &mut (*mc).__ss;
        match r {
            0..=28 => ss.__x[r] = v,
            29 => ss.__fp = v,
            30 => ss.__lr = v,
            _ => {}
        }
    }
    let ss = &mut (*mc).__ss;
    if let Some(wb) = l.writeback {
        let v = (base as i64).wrapping_add(wb) as u64;
        match l.rn {
            0..=28 => ss.__x[l.rn] = v,
            29 => ss.__fp = v,
            30 => ss.__lr = v,
            _ => ss.__sp = v,
        }
    }
    ss.__pc += 4;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn regs(n: usize) -> u64 {
        n as u64 * 0x10
    }

    #[test]
    fn forms() {
        // ldrh w28, [x8, #0x44]
        let l = decode(0x7940_891c, regs).unwrap();
        assert_eq!((l.regs[0], l.bytes, l.offset, l.rn), ((28, false), 2, 0x44, 8));
        // ldr s2, [x8, #0x2d4]
        let l = decode(0xbd42_d502, regs).unwrap();
        assert_eq!((l.regs[0], l.bytes, l.offset), ((2, true), 4, 0x2d4));
        // ldp s0, s1, [x8]
        let l = decode(0x2d40_0500, regs).unwrap();
        assert_eq!((l.count, l.regs, l.bytes, l.offset), (2, [(0, true), (1, true)], 4, 0));
        // ldr b0, [x11, w16, uxtw]
        let l = decode(0x3c70_4960, regs).unwrap();
        assert_eq!((l.regs[0], l.bytes, l.offset, l.rn), ((0, true), 1, 0x100, 11));
        // ldrsw x0, [x1, #8]
        let l = decode(0xb980_0820, regs).unwrap();
        assert_eq!((l.bytes, l.sext, l.offset), (4, Some(true), 8));
        // ldr x1, [x2], #16 (post-index)
        let l = decode(0xf841_0441, regs).unwrap();
        assert_eq!((l.offset, l.writeback), (0, Some(16)));
        // ldar w8, [x0]
        let l = decode(0x88df_fc08, regs).unwrap();
        assert_eq!((l.regs[0], l.bytes, l.offset, l.rn), ((8, false), 4, 0, 0));
        // ldaxr w8, [x0] (exclusive) is left alone
        assert!(decode(0x885f_fc08, regs).is_none());
        // stp x9, x27, [x8] and str w0, [x1] are not loads
        assert!(decode(0xa900_6d09, regs).is_none());
        assert!(decode(0xb900_0020, regs).is_none());
    }
}
