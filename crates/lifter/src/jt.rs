// Jump tables: the targets of a `br` that indexes a table of offsets, from the instructions before it.
//
// Compilers emit (clang; gcc is the same up to register choice and extend kinds):
//     cmp   w8, #N            ; bounds check: b.hi / b.cs to the default
//     b.hi  default
//     adrp  x9, table ; add x9, x9, :lo12:table
//     ldrb  w10, [x9, x8]     ; ldrh .. lsl #1, ldrsw .. lsl #2
//     adr   x11, anchor       ; ldrsw tables use the table address as the anchor
//     add   x11, x11, x10, lsl #2
//     br    x11
// A wrong resolution cannot make lifted code misbehave: the emitted switch compares the run-time target with each
// case, and anything else goes through the dispatcher. It only costs labels, or a dispatch where a goto would do.

use crate::bits::{bits, sext};

/// Code and data the matcher reads: instruction words and image bytes at guest addresses.
pub trait Memory {
    fn word(&self, addr: u64) -> Option<u32>;
    fn bytes(&self, addr: u64, len: usize) -> Option<&[u8]>;
}

/// A load of a table entry: `table` indexed by register `idx` (scaled by `scale` bits), `size` bytes, sign-extended
/// or not, by the instruction at `at`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Load {
    table: u64,
    idx: u32,
    size: u32,
    signed: bool,
    scale: u32,
    at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Sym {
    Const(u64),
    Load(Load),
    /// `base + (entry << shift)`.
    Target { base: u64, load: Load, shift: u32 },
}

const WINDOW: u64 = 24;
const MAX_ENTRIES: u64 = 4096;

/// The targets of the `br` at `br_pc` in the function starting at `entry`, or None if it is not a recognised table.
pub fn resolve(mem: &dyn Memory, entry: u64, br_pc: u64) -> Option<Vec<u64>> {
    let w = mem.word(br_pc)?;
    let t = bits(w, 5, 5);
    let Sym::Target { base, load: Load { table, idx, size, signed, scale, at }, shift } = value(mem, entry, br_pc, t, 0)?
    else {
        return None;
    };
    let count = bound(mem, entry, at, idx)?;
    if count == 0 || count > MAX_ENTRIES || (1u32 << scale) != size {
        return None;
    }
    let raw = mem.bytes(table, (count as usize) * size as usize)?;
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count as usize {
        let e = &raw[i * size as usize..(i + 1) * size as usize];
        let v = match size {
            1 => e[0] as u64,
            2 => u16::from_le_bytes([e[0], e[1]]) as u64,
            _ => u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as u64,
        };
        let v = if signed { sext(v, size * 8) as u64 } else { v };
        let target = base.wrapping_add(v << shift);
        if target % 4 != 0 || mem.word(target).is_none() {
            return None;
        }
        out.push(target);
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
}

/// The symbolic value of register `r` just before `pc`, from the closest instruction before it that writes it.
fn value(mem: &dyn Memory, entry: u64, pc: u64, r: u32, depth: u32) -> Option<Sym> {
    if depth > 4 || r == 31 {
        return None;
    }
    let mut a = pc;
    while a > entry && pc - a < WINDOW * 4 {
        a -= 4;
        let w = mem.word(a)?;
        if is_branch(w) && !is_bcond(w) && !(is_call(w) && callee_saved(r)) {
            return None;
        }
        if !writes(w, r) {
            continue;
        }
        if let Some(src) = mov_reg(w) {
            return value(mem, entry, a, src, depth + 1);
        }
        if w & 0x9f00_0000 == 0x9000_0000 {
            // adrp
            let imm = sext((((bits(w, 5, 19)) << 2) | bits(w, 29, 2)) as u64, 21);
            return Some(Sym::Const((a & !0xfff).wrapping_add((imm << 12) as u64)));
        }
        if w & 0x9f00_0000 == 0x1000_0000 {
            // adr
            let imm = sext((((bits(w, 5, 19)) << 2) | bits(w, 29, 2)) as u64, 21);
            return Some(Sym::Const(a.wrapping_add(imm as u64)));
        }
        if w & 0xff80_0000 == 0x9100_0000 {
            // add xd, xn, #imm (64-bit, no flags)
            let imm = (bits(w, 10, 12) as u64) << if bits(w, 22, 1) == 1 { 12 } else { 0 };
            let Sym::Const(b) = value(mem, entry, a, bits(w, 5, 5), depth + 1)? else { return None };
            return Some(Sym::Const(b.wrapping_add(imm)));
        }
        if w & 0x3f20_0000 == 0x3820_0000 && bits(w, 10, 2) == 2 && bits(w, 26, 1) == 0 {
            // ldr{b,h,sb,sh,sw} / ldr w (register offset)
            let (size, opc) = (bits(w, 30, 2), bits(w, 22, 2));
            let (option, s) = (bits(w, 13, 3), bits(w, 12, 1));
            if opc == 0 || size == 3 || (size == 2 && opc == 3) || !matches!(option, 2 | 3 | 6 | 7) {
                return None;
            }
            let Sym::Const(table) = value(mem, entry, a, bits(w, 5, 5), depth + 1)? else { return None };
            let scale = if s == 1 { size } else { 0 };
            return Some(Sym::Load(Load { table, idx: bits(w, 16, 5), size: 1 << size, signed: opc >= 2, scale, at: a }));
        }
        if w & 0xff20_0000 == 0x8b00_0000 || w & 0xffe0_0000 == 0x8b20_0000 {
            // add xd, xn, xm{, lsl #s} / add xd, xn, wm, (s|u)xt* #s
            let ext = bits(w, 21, 1) == 1;
            if !ext && bits(w, 22, 2) != 0 {
                return None;
            }
            let shift = if ext { bits(w, 10, 3) } else { bits(w, 10, 6) };
            let (n, m) = (value(mem, entry, a, bits(w, 5, 5), depth + 1), value(mem, entry, a, bits(w, 16, 5), depth + 1));
            return match (n, m) {
                (Some(Sym::Const(base)), Some(Sym::Load(load))) => {
                    // An extend narrower than the entry would truncate it; the usual sxtb/sxth/sxtw match the load.
                    let signed = if ext { load.signed || bits(w, 15, 1) == 1 } else { load.signed };
                    Some(Sym::Target { base, load: Load { signed, ..load }, shift })
                }
                // add xd, <entry>, <base> (Nintendo's SDK and games: ldrsw tables relative to themselves): the shift /
                // extend would apply to the base, so only the plain form
                (Some(Sym::Load(load)), Some(Sym::Const(base))) if !ext && shift == 0 => {
                    Some(Sym::Target { base, load, shift: 0 })
                }
                _ => None,
            };
        }
        return None;
    }
    None
}

/// The number of table entries, from the bounds check of register `idx` before the table load at `load_pc` (following
/// register copies: `cmp w0, #5; b.hi ..; mov w8, w0; .. ldrsw x8, [x9, x8, lsl #2]`).
fn bound(mem: &dyn Memory, entry: u64, load_pc: u64, mut idx: u32) -> Option<u64> {
    let mut a = load_pc;
    let mut limit_cond = None;
    while a > entry && load_pc - a < WINDOW * 4 {
        a -= 4;
        let w = mem.word(a)?;
        if is_bcond(w) {
            limit_cond.get_or_insert(bits(w, 0, 4));
            continue;
        }
        if is_branch(w) && !(is_call(w) && callee_saved(idx)) {
            return None;
        }
        // cmp wN / xN, #imm (subs zr, rn, #imm)
        if w & 0x7f80_001f == 0x7100_001f && bits(w, 5, 5) == idx {
            let imm = (bits(w, 10, 12) as u64) << if bits(w, 22, 1) == 1 { 12 } else { 0 };
            return match limit_cond? {
                8 | 9 => Some(imm + 1), // b.hi default / b.ls cases: 0..=imm
                2 | 3 => Some(imm),     // b.cs default / b.cc cases: 0..imm
                _ => None,
            };
        }
        if writes(w, idx) {
            match mov_reg(w) {
                Some(src) => idx = src,
                None => return None,
            }
        }
    }
    None
}

/// The source register of `mov wd/xd, ws/xs` (orr with the zero register, unshifted).
fn mov_reg(w: u32) -> Option<u32> {
    (w & 0x7f20_ffe0 == 0x2a00_03e0).then(|| bits(w, 16, 5))
}

fn is_call(w: u32) -> bool {
    w & 0xfc00_0000 == 0x9400_0000 || w & 0xffff_fc1f == 0xd63f_0000
}

/// Registers a call preserves (AAPCS64 x19-x28): a table base kept in one survives the call.
fn callee_saved(r: u32) -> bool {
    (19..=28).contains(&r)
}

fn is_branch(w: u32) -> bool {
    bits(w, 26, 5) == 0b00101 || bits(w, 25, 7) == 0b0101010 || bits(w, 25, 6) & 0b111110 == 0b011010
        || bits(w, 25, 7) == 0b1101011
}

fn is_bcond(w: u32) -> bool {
    bits(w, 24, 8) == 0b0101_0100 && bits(w, 4, 1) == 0
}

/// Whether `w` may write general register `r` (conservative: anything with Rd = r that is not a store or a compare).
fn writes(w: u32, r: u32) -> bool {
    let op0 = bits(w, 25, 4);
    let is_ldst = op0 & 0b0101 == 0b0100;
    if is_ldst {
        let load = match bits(w, 27, 3) {
            0b111 => bits(w, 22, 2) != 0, // ldr, ldrs*, prfm (no register write, harmless)
            0b011 => true,               // literal
            _ => bits(w, 22, 1) == 1,     // pairs and exclusives: L
        };
        let rn_wb = (bits(w, 27, 3) == 0b111 && bits(w, 24, 2) == 0 && bits(w, 21, 1) == 0 && bits(w, 10, 1) == 1)
            || (bits(w, 27, 3) == 0b101 && matches!(bits(w, 23, 2), 1 | 3));
        let pair_rt2 = bits(w, 27, 3) == 0b101 && load && bits(w, 10, 5) == r;
        let excl_status = bits(w, 24, 6) == 0b001000 && !load && bits(w, 16, 5) == r;
        return (load && bits(w, 26, 1) == 0 && bits(w, 0, 5) == r) || (rn_wb && bits(w, 5, 5) == r) || pair_rt2
            || excl_status;
    }
    if is_branch(w) {
        return bits(w, 26, 5) == 0b00101 && bits(w, 31, 1) == 1 && r == 30;
    }
    bits(w, 0, 5) == r
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Img {
        base: u64,
        data: Vec<u8>,
    }
    impl Memory for Img {
        fn word(&self, a: u64) -> Option<u32> {
            let o = a.checked_sub(self.base)? as usize;
            Some(u32::from_le_bytes(self.data.get(o..o + 4)?.try_into().ok()?))
        }
        fn bytes(&self, a: u64, len: usize) -> Option<&[u8]> {
            let o = a.checked_sub(self.base)? as usize;
            self.data.get(o..o + len)
        }
    }

    #[test]
    fn clang_byte_table() {
        // 0x1000: cmp w8, #3; b.hi 0x1040; adrp x9, 0x1000; add x9, x9, #0x100; ldrb w10, [x9, x8];
        //         adr x11, 0x1020; add x11, x11, x10, lsl #2; br x11
        let code: [u32; 8] =
            [0x71000d1f, 0x540001e8, 0x90000009, 0x91040129, 0x3868692a, 0x1000006b, 0x8b0a096b, 0xd61f0160];
        let mut data = vec![0u8; 0x200];
        for (i, w) in code.iter().enumerate() {
            data[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        data[0x100..0x104].copy_from_slice(&[0, 1, 2, 4]); // targets 0x1020, 0x1024, 0x1028, 0x1030
        let img = Img { base: 0x1000, data };
        assert_eq!(resolve(&img, 0x1000, 0x101c), Some(vec![0x1020, 0x1024, 0x1028, 0x1030]));
    }

    #[test]
    fn self_relative_word_table() {
        // The shape in Nintendo's games and SDK: 0x2000: cmp w0, #5; b.hi ..; mov w8, w0; adrp x9, 0x2000;
        // add x9, x9, #0x100; ldrsw x8, [x9, x8, lsl #2]; add x8, x8, x9; br x8
        let code: [u32; 8] =
            [0x7100141f, 0x54000108, 0x2a0003e8, 0x90000009, 0x91040129, 0xb8a87928, 0x8b090108, 0xd61f0100];
        let mut data = vec![0u8; 0x200];
        for (i, w) in code.iter().enumerate() {
            data[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        for i in 0..6 {
            let off = 0x20 + 8 * i as i32 - 0x100; // targets 0x2020, 0x2028, .. relative to the table at 0x2100
            data[0x100 + 4 * i..0x104 + 4 * i].copy_from_slice(&off.to_le_bytes());
        }
        let img = Img { base: 0x2000, data };
        assert_eq!(resolve(&img, 0x2000, 0x201c), Some(vec![0x2020, 0x2028, 0x2030, 0x2038, 0x2040, 0x2048]));
    }
}
