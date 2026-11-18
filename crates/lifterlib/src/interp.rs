// An interpreter for guest code that changed after it was lifted: what plugin mods patch while the game runs, until the
// lifted version of the same code is ready. It runs on the same Cpu as lifted code and hands over to it at branches, so
// only patched code and the trampolines mods generate pass through here.
//
// It covers what hooks and the instructions they displace use: integer operations, loads and stores, branches. Anything
// else (floating point arithmetic, SIMD, exclusives, atomics, svc) stops with `Stop::Unsupported`, leaving the Cpu at that
// instruction, for the caller to wait for the lifted version. A partial answer here would be a wrong result later.

use crate::abi::Cpu;

/// Why `run` returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// A branch: execution goes on at `c.pc`, which `interpreted` says is not this interpreter's.
    Branch,
    /// A call to `c.pc`, which returns to the address given. Always reported, like Return: the caller nests calls as lifted
    /// code does, so each return goes back to the frame that made its call, wherever that frame runs.
    Call(u64),
    /// A return, to `c.pc`.
    Return,
    /// The instruction at `c.pc` is not one this interpreter runs; nothing of it has happened.
    Unsupported(u32),
}

const N: u64 = 1 << 31;
const Z: u64 = 1 << 30;
const C: u64 = 1 << 29;
const V: u64 = 1 << 28;

#[inline]
fn bits(w: u32, hi: u32, lo: u32) -> u32 {
    (w >> lo) & ((1u32 << (hi - lo + 1)) - 1)
}

#[inline]
fn sext(v: u64, width: u32) -> i64 {
    ((v << (64 - width)) as i64) >> (64 - width)
}

/// Register n as a value, register 31 reading zero.
#[inline]
fn rz(c: &Cpu, n: u32) -> u64 {
    if n == 31 {
        0
    } else {
        c.x[n as usize]
    }
}

/// Register n as a base or a stack-capable operand, register 31 being sp.
#[inline]
fn rsp(c: &Cpu, n: u32) -> u64 {
    if n == 31 {
        c.sp
    } else {
        c.x[n as usize]
    }
}

/// Writes register n, register 31 discarding. A 32-bit result clears the upper half.
#[inline]
fn wz(c: &mut Cpu, n: u32, v: u64, sf: bool) {
    if n != 31 {
        c.x[n as usize] = if sf { v } else { v as u32 as u64 };
    }
}

#[inline]
fn wsp(c: &mut Cpu, n: u32, v: u64, sf: bool) {
    let v = if sf { v } else { v as u32 as u64 };
    if n == 31 {
        c.sp = v;
    } else {
        c.x[n as usize] = v;
    }
}

fn add_flags(a: u64, b: u64, carry: bool, sf: bool) -> (u64, u64) {
    if sf {
        let (r1, c1) = a.overflowing_add(b);
        let (r, c2) = r1.overflowing_add(carry as u64);
        let v = ((a ^ r) & (b ^ r)) >> 63 != 0;
        (r, nzcv(r >> 63 != 0, r == 0, c1 || c2, v))
    } else {
        let (a, b) = (a as u32, b as u32);
        let (r1, c1) = a.overflowing_add(b);
        let (r, c2) = r1.overflowing_add(carry as u32);
        let v = ((a ^ r) & (b ^ r)) >> 31 != 0;
        (r as u64, nzcv(r >> 31 != 0, r == 0, c1 || c2, v))
    }
}

#[inline]
fn nzcv(n: bool, z: bool, c: bool, v: bool) -> u64 {
    (n as u64 * N) | (z as u64 * Z) | (c as u64 * C) | (v as u64 * V)
}

fn cond(c: &Cpu, code: u32) -> bool {
    let f = c.nzcv;
    let (n, z, cy, v) = (f & N != 0, f & Z != 0, f & C != 0, f & V != 0);
    let r = match code >> 1 {
        0 => z,
        1 => cy,
        2 => n,
        3 => v,
        4 => cy && !z,
        5 => n == v,
        6 => n == v && !z,
        _ => true,
    };
    if code & 1 == 1 && code != 15 {
        !r
    } else {
        r
    }
}

fn shift(v: u64, kind: u32, amount: u32, sf: bool) -> u64 {
    if sf {
        match kind {
            0 => v.wrapping_shl(amount),
            1 => v.wrapping_shr(amount),
            2 => ((v as i64).wrapping_shr(amount)) as u64,
            _ => v.rotate_right(amount),
        }
    } else {
        let w = v as u32;
        (match kind {
            0 => w.wrapping_shl(amount),
            1 => w.wrapping_shr(amount),
            2 => ((w as i32).wrapping_shr(amount)) as u32,
            _ => w.rotate_right(amount),
        }) as u64
    }
}

fn extend(v: u64, option: u32, amount: u32) -> u64 {
    let e = match option {
        0 => v as u8 as u64,
        1 => v as u16 as u64,
        2 => v as u32 as u64,
        3 => v,
        4 => v as u8 as i8 as i64 as u64,
        5 => v as u16 as i16 as i64 as u64,
        6 => v as u32 as i32 as i64 as u64,
        _ => v,
    };
    e << amount
}

/// A64's DecodeBitMasks; None for a reserved pattern. All ones in an element is reserved only for logical immediates
/// (`logical`): bitfield instructions use it, as `asr x0, x1, #4` does.
fn bitmask(n: u32, immr: u32, imms: u32, sf: bool, logical: bool) -> Option<(u64, u64)> {
    let combined = (n << 6) | (!imms & 0x3f);
    if combined == 0 {
        return None;
    }
    let len = 31 - combined.leading_zeros();
    if len < 1 || (!sf && len > 5) {
        return None;
    }
    let esize = 1u32 << len;
    let levels = esize - 1;
    let (s, r) = (imms & levels, immr & levels);
    if logical && s == levels {
        return None;
    }
    let d = s.wrapping_sub(r) & levels;
    let ones = |k: u32| -> u64 {
        if k >= 64 {
            u64::MAX
        } else {
            (1u64 << k) - 1
        }
    };
    let ror = |v: u64, by: u32| -> u64 {
        if by == 0 {
            v
        } else {
            let mask = ones(esize);
            ((v >> by) | (v << (esize - by))) & mask
        }
    };
    let (mut wmask, mut tmask) = (ror(ones(s + 1), r), ones(d + 1));
    let mut size = esize;
    while size < 64 {
        wmask |= wmask << size;
        tmask |= tmask << size;
        size *= 2;
    }
    Some((wmask, tmask))
}

#[inline]
unsafe fn load(a: u64, size: u32) -> u64 {
    match size {
        0 => std::ptr::read_volatile(a as *const u8) as u64,
        1 => std::ptr::read_unaligned(a as *const u16) as u64,
        2 => std::ptr::read_unaligned(a as *const u32) as u64,
        _ => std::ptr::read_unaligned(a as *const u64),
    }
}

#[inline]
unsafe fn store(a: u64, size: u32, v: u64) {
    match size {
        0 => std::ptr::write_volatile(a as *mut u8, v as u8),
        1 => std::ptr::write_unaligned(a as *mut u16, v as u16),
        2 => std::ptr::write_unaligned(a as *mut u32, v as u32),
        _ => std::ptr::write_unaligned(a as *mut u64, v),
    }
}

/// An integer or vector register load or store of one register at `a`. `opc` as encoded. False: not supported.
unsafe fn load_store(c: &mut Cpu, rt: u32, a: u64, size: u32, opc: u32, vector: bool) -> bool {
    if vector {
        // size + opc<1> give the width: b h s d q.
        let log = size | ((opc >> 1) << 2);
        if log > 4 {
            return false;
        }
        if opc & 1 == 0 {
            let v = c.v[rt as usize];
            if log == 4 {
                std::ptr::write_unaligned(a as *mut u128, v);
            } else {
                store(a, log, v as u64);
            }
        } else if log == 4 {
            c.v[rt as usize] = std::ptr::read_unaligned(a as *const u128);
        } else {
            c.v[rt as usize] = load(a, log) as u128;
        }
        return true;
    }
    match opc {
        0 => store(a, size, rz(c, rt)),
        1 => {
            let v = load(a, size);
            wz(c, rt, v, true);
        }
        // Sign extending to 64 bits (opc 2) or 32 (opc 3).
        2 | 3 => {
            if size == 3 {
                // prfm with an immediate or register offset (opc 2); opc 3 with size 3 is unallocated.
                return opc == 2;
            }
            if opc == 3 && size == 2 {
                return false;
            }
            let v = sext(load(a, size), 8 << size) as u64;
            wz(c, rt, v, opc == 2);
        }
        _ => return false,
    }
    true
}

/// One instruction at `pc`. Some(next pc), or None: not supported, nothing done. A branch that changes nothing else
/// returns its target; the caller decides whether to stay.
unsafe fn step(c: &mut Cpu, w: u32, pc: u64) -> Option<u64> {
    let next = pc.wrapping_add(4);
    let (rd, rn, rm) = (bits(w, 4, 0), bits(w, 9, 5), bits(w, 20, 16));
    let sf = w >> 31 == 1;

    // Branches.
    if w & 0x7C00_0000 == 0x1400_0000 {
        let t = pc.wrapping_add((sext(bits(w, 25, 0) as u64, 26) << 2) as u64);
        if w >> 31 == 1 {
            c.x[30] = next;
        }
        return Some(t);
    }
    if w & 0xFF00_0010 == 0x5400_0000 {
        let t = pc.wrapping_add((sext(bits(w, 23, 5) as u64, 19) << 2) as u64);
        return Some(if cond(c, bits(w, 3, 0)) { t } else { next });
    }
    if w & 0x7E00_0000 == 0x3400_0000 {
        let t = pc.wrapping_add((sext(bits(w, 23, 5) as u64, 19) << 2) as u64);
        let v = rz(c, rd);
        let v = if sf { v } else { v as u32 as u64 };
        let nonzero = bits(w, 24, 24) == 1;
        return Some(if (v != 0) == nonzero { t } else { next });
    }
    if w & 0x7E00_0000 == 0x3600_0000 {
        let t = pc.wrapping_add((sext(bits(w, 18, 5) as u64, 14) << 2) as u64);
        let bit = (bits(w, 31, 31) << 5) | bits(w, 23, 19);
        let set = (rz(c, rd) >> bit) & 1 == 1;
        return Some(if set == (bits(w, 24, 24) == 1) { t } else { next });
    }
    match w & 0xFFFF_FC1F {
        0xD61F_0000 => return Some(rz(c, rn)),
        0xD63F_0000 => {
            let t = rz(c, rn);
            c.x[30] = next;
            return Some(t);
        }
        0xD65F_0000 => return Some(rz(c, rn)),
        _ => {}
    }

    // Hints (nop and the like), barriers.
    if w & 0xFFFF_F01F == 0xD503_201F {
        return Some(next);
    }
    if w & 0xFFFF_F01F == 0xD503_301F {
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        return Some(next);
    }
    // System registers the lifted code keeps in the Cpu too.
    match w & 0xFFFF_FFE0 {
        0xD53B_D040 => {
            wz(c, rd, c.tpidr, true);
            return Some(next);
        }
        0xD53B_D060 => {
            wz(c, rd, c.tpidrro, true);
            return Some(next);
        }
        0xD51B_D040 => {
            c.tpidr = rz(c, rd);
            return Some(next);
        }
        0xD53B_4200 => {
            wz(c, rd, c.nzcv, true);
            return Some(next);
        }
        0xD51B_4200 => {
            c.nzcv = rz(c, rd) & (N | Z | C | V);
            return Some(next);
        }
        _ => {}
    }

    // PC-relative addresses.
    if w & 0x1F00_0000 == 0x1000_0000 {
        let imm = (bits(w, 23, 5) << 2 | bits(w, 30, 29)) as u64;
        let v = if w >> 31 == 1 {
            (pc & !0xfff).wrapping_add((sext(imm, 21) << 12) as u64)
        } else {
            pc.wrapping_add(sext(imm, 21) as u64)
        };
        wz(c, rd, v, true);
        return Some(next);
    }
    // Add / subtract, immediate.
    if w & 0x1F00_0000 == 0x1100_0000 {
        if bits(w, 23, 23) == 1 {
            return None;
        }
        let imm = (bits(w, 21, 10) as u64) << (bits(w, 22, 22) * 12);
        let (sub, flags) = (bits(w, 30, 30) == 1, bits(w, 29, 29) == 1);
        let a = rsp(c, rn);
        if flags {
            let (r, f) = if sub { add_flags(a, !imm, true, sf) } else { add_flags(a, imm, false, sf) };
            c.nzcv = f;
            wz(c, rd, r, sf);
        } else {
            let r = if sub { a.wrapping_sub(imm) } else { a.wrapping_add(imm) };
            wsp(c, rd, r, sf);
        }
        return Some(next);
    }
    // Logical, immediate.
    if w & 0x1F80_0000 == 0x1200_0000 {
        let (imm, _) = bitmask(bits(w, 22, 22), bits(w, 21, 16), bits(w, 15, 10), sf, true)?;
        let a = rz(c, rn);
        let opc = bits(w, 30, 29);
        let r = match opc {
            0 | 3 => a & imm,
            1 => a | imm,
            _ => a ^ imm,
        };
        let r = if sf { r } else { r as u32 as u64 };
        if opc == 3 {
            let neg = if sf { r >> 63 != 0 } else { r >> 31 != 0 };
            c.nzcv = nzcv(neg, r == 0, false, false);
            wz(c, rd, r, sf);
        } else {
            wsp(c, rd, r, sf);
        }
        return Some(next);
    }
    // Move wide.
    if w & 0x1F80_0000 == 0x1280_0000 {
        let hw = bits(w, 22, 21) * 16;
        if !sf && hw > 16 {
            return None;
        }
        let imm = (bits(w, 20, 5) as u64) << hw;
        match bits(w, 30, 29) {
            0 => wz(c, rd, !imm, sf),
            2 => wz(c, rd, imm, sf),
            3 => {
                let v = (rz(c, rd) & !(0xffffu64 << hw)) | imm;
                wz(c, rd, v, sf);
            }
            _ => return None,
        }
        return Some(next);
    }
    // Bitfield.
    if w & 0x1F80_0000 == 0x1300_0000 {
        let (immr, imms, opc) = (bits(w, 21, 16), bits(w, 15, 10), bits(w, 30, 29));
        let width = if sf { 64 } else { 32 };
        if opc == 3 || bits(w, 22, 22) != sf as u32 || immr >= width || imms >= width {
            return None;
        }
        let (wmask, tmask) = bitmask(bits(w, 22, 22), immr, imms, sf, false)?;
        let src = rz(c, rn);
        let src = if sf { src } else { src as u32 as u64 };
        let rotated = if sf { src.rotate_right(immr) } else { (src as u32).rotate_right(immr) as u64 };
        let bot = rotated & wmask;
        let r = match opc {
            0 => {
                let top = if (src >> imms) & 1 == 1 { u64::MAX } else { 0 };
                (top & !tmask) | (bot & tmask)
            }
            1 => {
                let dst = rz(c, rd);
                let b = (dst & !wmask) | bot;
                (dst & !tmask) | (b & tmask)
            }
            _ => bot & tmask,
        };
        wz(c, rd, r, sf);
        return Some(next);
    }
    // Extract.
    if w & 0x7FA0_0000 == 0x1380_0000 {
        let lsb = bits(w, 15, 10);
        if !sf && lsb > 31 {
            return None;
        }
        let (hi, lo) = (rz(c, rn), rz(c, rm));
        let r = if sf {
            if lsb == 0 {
                lo
            } else {
                (lo >> lsb) | (hi << (64 - lsb))
            }
        } else if lsb == 0 {
            lo as u32 as u64
        } else {
            ((lo as u32 >> lsb) | ((hi as u32) << (32 - lsb))) as u64
        };
        wz(c, rd, r, sf);
        return Some(next);
    }
    // Logical, shifted register.
    if w & 0x1F00_0000 == 0x0A00_0000 {
        let amount = bits(w, 15, 10);
        if !sf && amount > 31 {
            return None;
        }
        let mut b = shift(rz(c, rm), bits(w, 23, 22), amount, sf);
        if bits(w, 21, 21) == 1 {
            b = !b;
        }
        let a = rz(c, rn);
        let opc = bits(w, 30, 29);
        let r = match opc {
            0 | 3 => a & b,
            1 => a | b,
            _ => a ^ b,
        };
        let r = if sf { r } else { r as u32 as u64 };
        if opc == 3 {
            let neg = if sf { r >> 63 != 0 } else { r >> 31 != 0 };
            c.nzcv = nzcv(neg, r == 0, false, false);
        }
        wz(c, rd, r, sf);
        return Some(next);
    }
    // Add / subtract, shifted and extended register.
    if w & 0x1F20_0000 == 0x0B00_0000 || w & 0x1F20_0000 == 0x0B20_0000 {
        let extended = bits(w, 21, 21) == 1;
        let (a, b) = if extended {
            if bits(w, 23, 22) != 0 || bits(w, 12, 10) > 4 {
                return None;
            }
            (rsp(c, rn), extend(rz(c, rm), bits(w, 15, 13), bits(w, 12, 10)))
        } else {
            let (kind, amount) = (bits(w, 23, 22), bits(w, 15, 10));
            if kind == 3 || (!sf && amount > 31) {
                return None;
            }
            (rz(c, rn), shift(rz(c, rm), kind, amount, sf))
        };
        let (sub, flags) = (bits(w, 30, 30) == 1, bits(w, 29, 29) == 1);
        if flags {
            let (r, f) = if sub { add_flags(a, !b, true, sf) } else { add_flags(a, b, false, sf) };
            c.nzcv = f;
            wz(c, rd, r, sf);
        } else {
            let r = if sub { a.wrapping_sub(b) } else { a.wrapping_add(b) };
            if extended {
                wsp(c, rd, r, sf);
            } else {
                wz(c, rd, r, sf);
            }
        }
        return Some(next);
    }
    // Add / subtract with carry.
    if w & 0x1FE0_FC00 == 0x1A00_0000 {
        let carry = c.nzcv & C != 0;
        let (a, b) = (rz(c, rn), rz(c, rm));
        let (r, f) = if bits(w, 30, 30) == 1 { add_flags(a, !b, carry, sf) } else { add_flags(a, b, carry, sf) };
        if bits(w, 29, 29) == 1 {
            c.nzcv = f;
        }
        wz(c, rd, r, sf);
        return Some(next);
    }
    // Conditional select.
    if w & 0x1FE0_0000 == 0x1A80_0000 {
        let op = (bits(w, 30, 30) << 1) | bits(w, 10, 10);
        if bits(w, 29, 29) == 1 || bits(w, 11, 11) == 1 {
            return None;
        }
        let r = if cond(c, bits(w, 15, 12)) {
            rz(c, rn)
        } else {
            let b = rz(c, rm);
            match op {
                0 => b,
                1 => b.wrapping_add(1),
                2 => !b,
                _ => b.wrapping_neg(),
            }
        };
        wz(c, rd, r, sf);
        return Some(next);
    }
    // Conditional compare.
    if w & 0x1FE0_0000 == 0x1A40_0000 && bits(w, 29, 29) == 1 {
        if bits(w, 10, 10) == 1 || bits(w, 4, 4) == 1 {
            return None;
        }
        if cond(c, bits(w, 15, 12)) {
            let a = rz(c, rn);
            let b = if bits(w, 11, 11) == 1 { rm as u64 } else { rz(c, rm) };
            let (_, f) = if bits(w, 30, 30) == 1 { add_flags(a, !b, true, sf) } else { add_flags(a, b, false, sf) };
            c.nzcv = f;
        } else {
            c.nzcv = (bits(w, 3, 0) as u64) << 28;
        }
        return Some(next);
    }
    // Two sources.
    if w & 0x5FE0_0000 == 0x1AC0_0000 {
        let (a, b) = (rz(c, rn), rz(c, rm));
        let (a32, b32) = (a as u32, b as u32);
        let op = bits(w, 15, 10);
        let r = match op {
            2 if sf => a.checked_div(b).unwrap_or(0),
            2 => a32.checked_div(b32).unwrap_or(0) as u64,
            3 if sf => {
                if b == 0 {
                    0
                } else {
                    (a as i64).wrapping_div(b as i64) as u64
                }
            }
            3 => {
                if b32 == 0 {
                    0
                } else {
                    (a32 as i32).wrapping_div(b32 as i32) as u32 as u64
                }
            }
            8..=11 => {
                let width = if sf { 64 } else { 32 };
                shift(a, op - 8, (b % width) as u32, sf)
            }
            _ => return None,
        };
        wz(c, rd, r, sf);
        return Some(next);
    }
    // One source.
    if w & 0x5FFF_0000 == 0x5AC0_0000 {
        let a = rz(c, rn);
        let a32 = a as u32;
        let r = match (bits(w, 15, 10), sf) {
            (0, true) => a.reverse_bits(),
            (0, false) => a32.reverse_bits() as u64,
            (1, true) => ((a & 0x00ff00ff00ff00ff) << 8) | ((a >> 8) & 0x00ff00ff00ff00ff),
            (1, false) => (((a32 & 0x00ff00ff) << 8) | ((a32 >> 8) & 0x00ff00ff)) as u64,
            (2, true) => (((a as u32).swap_bytes() as u64) << 0) | (((a >> 32) as u32).swap_bytes() as u64) << 32,
            (2, false) => a32.swap_bytes() as u64,
            (3, true) => a.swap_bytes(),
            (4, true) => a.leading_zeros() as u64,
            (4, false) => a32.leading_zeros() as u64,
            (5, true) => ((a ^ ((a as i64 >> 1) as u64)) & (u64::MAX >> 1)).leading_zeros() as u64 - 1,
            (5, false) => ((a32 ^ ((a32 as i32 >> 1) as u32)) & (u32::MAX >> 1)).leading_zeros() as u64 - 1,
            _ => return None,
        };
        wz(c, rd, r, sf);
        return Some(next);
    }
    // Three sources.
    if w & 0x1F00_0000 == 0x1B00_0000 {
        let ra = bits(w, 14, 10);
        let (a, b, x) = (rz(c, rn), rz(c, rm), rz(c, ra));
        let o0 = bits(w, 15, 15) == 1;
        let r = match bits(w, 23, 21) {
            0 => {
                let m = a.wrapping_mul(b);
                if o0 {
                    x.wrapping_sub(m)
                } else {
                    x.wrapping_add(m)
                }
            }
            1 if sf => {
                let m = (a as u32 as i32 as i64).wrapping_mul(b as u32 as i32 as i64) as u64;
                if o0 {
                    x.wrapping_sub(m)
                } else {
                    x.wrapping_add(m)
                }
            }
            5 if sf => {
                let m = (a as u32 as u64).wrapping_mul(b as u32 as u64);
                if o0 {
                    x.wrapping_sub(m)
                } else {
                    x.wrapping_add(m)
                }
            }
            2 if sf && !o0 => (((a as i64 as i128) * (b as i64 as i128)) >> 64) as u64,
            6 if sf && !o0 => (((a as u128) * (b as u128)) >> 64) as u64,
            _ => return None,
        };
        wz(c, rd, r, sf);
        return Some(next);
    }
    // Load register, literal.
    if w & 0x3B00_0000 == 0x1800_0000 {
        let a = pc.wrapping_add((sext(bits(w, 23, 5) as u64, 19) << 2) as u64);
        let (opc, vector) = (bits(w, 31, 30), bits(w, 26, 26) == 1);
        if vector {
            match opc {
                0 => c.v[rd as usize] = load(a, 2) as u128,
                1 => c.v[rd as usize] = load(a, 3) as u128,
                2 => c.v[rd as usize] = std::ptr::read_unaligned(a as *const u128),
                _ => return None,
            }
        } else {
            match opc {
                0 => wz(c, rd, load(a, 2), true),
                1 => wz(c, rd, load(a, 3), true),
                2 => wz(c, rd, sext(load(a, 2), 32) as u64, true),
                _ => {} // prfm
            }
        }
        return Some(next);
    }
    // Load-acquire, store-release: stronger than the host needs for its own loads, so a fence each.
    if w & 0x3FFF_FC00 == 0x08DF_FC00 {
        let v = load(rsp(c, rn), bits(w, 31, 30));
        std::sync::atomic::fence(std::sync::atomic::Ordering::Acquire);
        wz(c, rd, v, true);
        return Some(next);
    }
    if w & 0x3FFF_FC00 == 0x089F_FC00 {
        std::sync::atomic::fence(std::sync::atomic::Ordering::Release);
        store(rsp(c, rn), bits(w, 31, 30), rz(c, rd));
        return Some(next);
    }
    // Load / store pair.
    if w & 0x3A00_0000 == 0x2800_0000 {
        let mode = bits(w, 24, 23);
        let (opc, vector, is_load) = (bits(w, 31, 30), bits(w, 26, 26) == 1, bits(w, 22, 22) == 1);
        let log = if vector {
            if opc == 3 {
                return None;
            }
            2 + opc
        } else {
            match opc {
                0 | 1 => 2,
                2 => 3,
                _ => return None,
            }
        };
        if !vector && opc == 1 && !is_load {
            return None; // stgp
        }
        let rt2 = bits(w, 14, 10);
        let off = sext(bits(w, 21, 15) as u64, 7) << log;
        let base = rsp(c, rn);
        let a = if mode == 1 { base } else { base.wrapping_add(off as u64) };
        let b = a.wrapping_add(1u64 << log);
        if vector {
            if is_load {
                let (x, y) = if log == 4 {
                    (std::ptr::read_unaligned(a as *const u128), std::ptr::read_unaligned(b as *const u128))
                } else {
                    (load(a, log) as u128, load(b, log) as u128)
                };
                c.v[rd as usize] = x;
                c.v[rt2 as usize] = y;
            } else {
                let (x, y) = (c.v[rd as usize], c.v[rt2 as usize]);
                if log == 4 {
                    std::ptr::write_unaligned(a as *mut u128, x);
                    std::ptr::write_unaligned(b as *mut u128, y);
                } else {
                    store(a, log, x as u64);
                    store(b, log, y as u64);
                }
            }
        } else if is_load {
            let signed = opc == 1;
            let (x, y) = (load(a, log), load(b, log));
            let (x, y) = if signed { (sext(x, 32) as u64, sext(y, 32) as u64) } else { (x, y) };
            wz(c, rd, x, true);
            wz(c, rt2, y, true);
        } else {
            store(a, log, rz(c, rd));
            store(b, log, rz(c, rt2));
        }
        if mode == 1 || mode == 3 {
            wsp(c, rn, base.wrapping_add(off as u64), true);
        }
        return Some(next);
    }
    // Load / store register.
    if w & 0x3A00_0000 == 0x3800_0000 {
        let (size, vector, opc) = (bits(w, 31, 30), bits(w, 26, 26) == 1, bits(w, 23, 22));
        let base = rsp(c, rn);
        if bits(w, 24, 24) == 1 {
            let scale = if vector { size | ((opc >> 1) << 2) } else { size };
            let a = base.wrapping_add((bits(w, 21, 10) as u64) << scale);
            return load_store(c, rd, a, size, opc, vector).then_some(next);
        }
        if bits(w, 21, 21) == 0 {
            let imm = sext(bits(w, 20, 12) as u64, 9) as u64;
            let mode = bits(w, 11, 10);
            if mode == 2 {
                return None; // unprivileged
            }
            let a = if mode == 1 { base } else { base.wrapping_add(imm) };
            if !load_store(c, rd, a, size, opc, vector) {
                return None;
            }
            if mode != 0 {
                wsp(c, rn, base.wrapping_add(imm), true);
            }
            return Some(next);
        }
        if bits(w, 11, 10) == 2 {
            let option = bits(w, 15, 13);
            if option & 2 == 0 {
                return None;
            }
            let scale = if vector { size | ((opc >> 1) << 2) } else { size };
            let amount = if bits(w, 12, 12) == 1 { scale } else { 0 };
            let a = base.wrapping_add(extend(rz(c, rm), option, amount));
            return load_store(c, rd, a, size, opc, vector).then_some(next);
        }
        return None; // atomics
    }
    None
}

/// Runs guest code from `c.pc` while `interpreted(pc)` holds, reading instructions from guest memory at their addresses.
///
/// # Safety
/// As running the guest code itself: the addresses it computes must be mapped as the guest expects.
pub unsafe fn run(c: &mut Cpu, interpreted: &dyn Fn(u64) -> bool) -> Stop {
    loop {
        let pc = c.pc;
        let w = std::ptr::read_unaligned(pc as *const u32);
        match step(c, w, pc) {
            Some(next) => {
                c.pc = next;
                if w & 0xFC00_0000 == 0x9400_0000 || w & 0xFFFF_FC1F == 0xD63F_0000 {
                    return Stop::Call(pc.wrapping_add(4));
                }
                if w & 0xFFFF_FC1F == 0xD65F_0000 {
                    return Stop::Return;
                }
                if !interpreted(next) {
                    return Stop::Branch;
                }
            }
            None => return Stop::Unsupported(w),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu() -> Box<Cpu> {
        // Plain data: all zeros is a valid state.
        unsafe { Box::new(std::mem::zeroed()) }
    }

    /// Runs words as code, ending by leaving at the address after them.
    fn exec(c: &mut Cpu, code: &[u32]) -> Stop {
        let (start, end) = (code.as_ptr() as u64, code.as_ptr() as u64 + code.len() as u64 * 4);
        c.pc = start;
        unsafe { run(c, &move |a| a >= start && a < end) }
    }

    fn exec_from_x30_ret(c: &mut Cpu, code: &[u32]) -> Stop {
        let (start, end) = (code.as_ptr() as u64, code.as_ptr() as u64 + code.len() as u64 * 4);
        c.pc = start;
        unsafe { run(c, &move |a| a >= start && a < end) }
    }

    #[test]
    fn immediates_and_moves() {
        let mut c = cpu();
        c.x[1] = 100;
        // add x0, x1, #16; movz x2, #0x1234; movk x2, #0x5678, lsl #16; and x3, x2, #0xff
        exec(&mut c, &[0x9100_4020, 0xD282_4682, 0xF2AA_CF02, 0x9240_1C43]);
        assert_eq!(c.x[0], 116);
        assert_eq!(c.x[2], 0x5678_1234);
        assert_eq!(c.x[3], 0x34);
    }

    #[test]
    fn flags_and_conditions() {
        let mut c = cpu();
        c.x[1] = 5;
        c.x[2] = 5;
        // subs w0, w1, w2; b.eq +8; movz x3, #1; movz x4, #2
        exec(&mut c, &[0x6B02_0020, 0x5400_0040, 0xD280_0023, 0xD280_0044]);
        assert_eq!(c.nzcv & Z, Z);
        assert_eq!(c.nzcv & C, C, "no borrow sets carry");
        assert_eq!(c.x[3], 0, "the branch was taken");
        assert_eq!(c.x[4], 2);
    }

    #[test]
    fn a_32_bit_write_clears_the_upper_half() {
        let mut c = cpu();
        c.x[1] = 0xffff_ffff_ffff_ffff;
        // add w0, w1, #1
        exec(&mut c, &[0x1100_0420]);
        assert_eq!(c.x[0], 0);
    }

    #[test]
    fn register_31_is_sp_or_zero_by_instruction() {
        let mut c = cpu();
        c.sp = 0x1000;
        c.x[1] = 7;
        // add x0, sp, #8; orr x2, xzr, x1
        exec(&mut c, &[0x9100_23E0, 0xAA01_03E2]);
        assert_eq!(c.x[0], 0x1008);
        assert_eq!(c.x[2], 7);
    }

    #[test]
    fn shifts_and_bitfields() {
        let mut c = cpu();
        c.x[1] = 0xF0;
        c.x[5] = 0xffff_ffff_8000_0000;
        // lsl x0, x1, #4; asr x2, x5, #4; sxtw x3, w5; ubfx x4, x1, #4, #4
        exec(&mut c, &[0xD37C_EC20, 0x9344_FCA2, 0x9340_7CA3, 0xD344_1C24]);
        assert_eq!(c.x[0], 0xF00);
        assert_eq!(c.x[2], 0xffff_ffff_f800_0000);
        assert_eq!(c.x[3], 0xffff_ffff_8000_0000);
        assert_eq!(c.x[4], 0xF);
    }

    #[test]
    fn select_multiply_divide_reverse() {
        let mut c = cpu();
        c.x[1] = 6;
        c.x[2] = 3;
        c.nzcv = Z;
        // csel x0, x1, x2, eq; mul x3, x1, x2; udiv x4, x1, x2; clz x5, x2; rev x6, x2
        exec(&mut c, &[0x9A82_0020, 0x9B02_7C23, 0x9AC2_0824, 0xDAC0_1045, 0xDAC0_0C46]);
        assert_eq!(c.x[0], 6);
        assert_eq!(c.x[3], 18);
        assert_eq!(c.x[4], 2);
        assert_eq!(c.x[5], 62);
        assert_eq!(c.x[6], 3u64.swap_bytes());
    }

    #[test]
    fn dividing_by_zero_gives_zero() {
        let mut c = cpu();
        c.x[1] = 6;
        // udiv x4, x1, x2 with x2 = 0
        exec(&mut c, &[0x9AC2_0824]);
        assert_eq!(c.x[4], 0);
    }

    #[test]
    fn conditional_compare() {
        let mut c = cpu();
        c.x[0] = 1;
        c.x[1] = 1;
        c.nzcv = 0; // ne holds, so it compares
        // ccmp x0, x1, #0, ne
        exec(&mut c, &[0xFA41_1000]);
        assert_eq!(c.nzcv & Z, Z);
        c.nzcv = Z; // ne fails: the flags become the immediate
        exec(&mut c, &[0xFA41_1000]);
        assert_eq!(c.nzcv, 0);
        c.nzcv = Z;
        // ccmp x0, x1, #2, ne: the immediate is C
        exec(&mut c, &[0xFA41_1002]);
        assert_eq!(c.nzcv, C);
    }

    #[test]
    fn loads_and_stores() {
        let mut c = cpu();
        let mut mem = [0u64; 8];
        let base = mem.as_mut_ptr() as u64;
        mem[1] = 0x1122_3344_5566_7788;
        mem[2] = 0xFFFF_FFFF_FFFF_FF80;
        c.x[1] = base;
        c.x[2] = 2;
        // ldr x0, [x1, #8]; ldr x3, [x1, x2, lsl #3]; ldrsb x4, [x1, #16]
        exec(&mut c, &[0xF940_0420, 0xF862_7823, 0x3980_4024]);
        assert_eq!(c.x[0], mem[1]);
        assert_eq!(c.x[3], mem[2]);
        assert_eq!(c.x[4] as i64, -128);
        // str w0, [x1], #4 writes, then moves the base
        c.x[0] = 0xAABB_CCDD;
        exec(&mut c, &[0xB800_4420]);
        assert_eq!(mem[0] as u32, 0xAABB_CCDD);
        assert_eq!(c.x[1], base + 4);
    }

    #[test]
    fn acquire_and_release() {
        let mut c = cpu();
        let mut v = [0x1122_3344_5566_7788u64];
        c.x[8] = v.as_mut_ptr() as u64;
        // ldarb w8, [x8]
        exec(&mut c, &[0x08DF_FD08]);
        assert_eq!(c.x[8], 0x88);
        c.x[1] = v.as_mut_ptr() as u64;
        c.x[0] = 0xAB;
        // stlr x0, [x1]
        exec(&mut c, &[0xC89F_FC20]);
        assert_eq!(v[0], 0xAB);
    }

    #[test]
    fn pairs_as_a_hook_saves_and_restores_them() {
        let mut c = cpu();
        let mut stack = [0u64; 8];
        c.sp = stack.as_mut_ptr() as u64 + 64;
        c.x[29] = 0x29;
        c.x[30] = 0x30;
        // stp x29, x30, [sp, #-16]!
        exec(&mut c, &[0xA9BF_7BFD]);
        assert_eq!(c.sp, stack.as_ptr() as u64 + 48);
        assert_eq!((stack[6], stack[7]), (0x29, 0x30));
        c.x[29] = 0;
        c.x[30] = 0;
        // ldp x29, x30, [sp], #16
        exec(&mut c, &[0xA8C1_7BFD]);
        assert_eq!((c.x[29], c.x[30]), (0x29, 0x30));
        assert_eq!(c.sp, stack.as_ptr() as u64 + 64);
    }

    #[test]
    fn vector_registers_as_saved_around_a_hook() {
        let mut c = cpu();
        let mut stack = [0u128; 4];
        c.sp = stack.as_mut_ptr() as u64 + 64;
        c.v[0] = 0xAAAA;
        c.v[1] = 0xBBBB;
        // stp q0, q1, [sp, #-32]!
        exec(&mut c, &[0xADBF_07E0]);
        assert_eq!((stack[2], stack[3]), (0xAAAA, 0xBBBB));
        c.v[2] = 0;
        c.x[1] = stack.as_ptr() as u64 + 32;
        // ldr q2, [x1]
        exec(&mut c, &[0x3DC0_0022]);
        assert_eq!(c.v[2], 0xAAAA);
    }

    #[test]
    fn calls_set_the_link_register_and_leave() {
        let mut c = cpu();
        let code = [0x9400_0100u32]; // bl +0x400: outside the code
        let start = code.as_ptr() as u64;
        assert_eq!(exec(&mut c, &code), Stop::Call(start + 4));
        assert_eq!(c.pc, start + 0x400);
        assert_eq!(c.x[30], start + 4);
    }

    #[test]
    fn calls_and_returns_stop_even_inside_interpreted_code() {
        let mut c = cpu();
        // bl +4 (the next word, still interpreted)
        let code = [0x9400_0001u32, 0xD280_0020];
        let start = code.as_ptr() as u64;
        assert_eq!(exec(&mut c, &code), Stop::Call(start + 4));
        assert_eq!(c.pc, start + 4);
        // ret to an address inside the code
        let mut c = cpu();
        let code = [0xD65F_03C0u32, 0xD280_0020];
        c.x[30] = code.as_ptr() as u64 + 4;
        assert_eq!(exec_from_x30_ret(&mut c, &code), Stop::Return);
        assert_eq!(c.x[0], 0, "it did not run on");
    }

    #[test]
    fn indirect_branches_and_tests() {
        let mut c = cpu();
        c.x[16] = 0x1234_0000;
        // br x16
        assert_eq!(exec(&mut c, &[0xD61F_0200]), Stop::Branch);
        assert_eq!(c.pc, 0x1234_0000);
        c.x[30] = 0x5678_0000;
        // ret
        assert_eq!(exec(&mut c, &[0xD65F_03C0]), Stop::Return);
        assert_eq!(c.pc, 0x5678_0000);
        // cbz x0, +8; movz x1, #1; tbnz w2, #3, +8; movz x3, #1; movz x4, #1
        c.x[2] = 8;
        exec(&mut c, &[0xB400_0040, 0xD280_0021, 0x3718_0042, 0xD280_0023, 0xD280_0024]);
        assert_eq!(c.x[1], 0, "cbz taken");
        assert_eq!(c.x[3], 0, "tbnz taken");
        assert_eq!(c.x[4], 1);
    }

    #[test]
    fn adrp_is_relative_to_the_page() {
        let mut c = cpu();
        let code = [0xB000_0000u32]; // adrp x0, +0x1000
        let pc = code.as_ptr() as u64;
        exec(&mut c, &code);
        assert_eq!(c.x[0], (pc & !0xfff) + 0x1000);
    }

    #[test]
    fn what_it_does_not_run_stops_there_with_nothing_done() {
        let mut c = cpu();
        // movz x0, #1; fmov d0, d1; movz x2, #1
        let code = [0xD280_0020u32, 0x1E60_4020, 0xD280_0022];
        assert_eq!(exec(&mut c, &code), Stop::Unsupported(0x1E60_4020));
        assert_eq!(c.x[0], 1);
        assert_eq!(c.pc, code.as_ptr() as u64 + 4);
        assert_eq!(c.x[2], 0);
        for word in [0xC85F_7C20u32 /* ldxr */, 0xD400_0001 /* svc */, 0x4E20_8400 /* add v0.16b */] {
            let mut c = cpu();
            let w = [word];
            assert_eq!(exec(&mut c, &w), Stop::Unsupported(word), "{word:#x}");
        }
    }
}
