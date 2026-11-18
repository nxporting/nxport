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

/// A load of a table entry: `table` (None: not known from the instructions before) indexed by register `idx` (scaled by
/// `scale` bits), `size` bytes, sign-extended or not, by the instruction at `at`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Load {
    table: Option<u64>,
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

/// The instructions that may have set register `r` last before `pc`, following the function's branches back (None: one
/// is out of sight). Lets a base register set before a loop be read at a jump inside it, past the loop's back branch
/// (BotW 1.0.0's SDK printf: `adrp x28; add x28, #0xf80` before the loop over the format, `add x12, x8, x28; br x12` in it).
pub type Defs<'a> = &'a dyn Fn(u64, u32) -> Option<Vec<u64>>;
const MAX_ENTRIES: u64 = 4096;

/// The targets of a jump through register `r` holding its value just before `pc` (a `br` there, or the instruction
/// after one that sets it), in the function starting at `entry`; None if it is not a recognised table.
pub fn resolve(mem: &dyn Memory, defs: Defs, entry: u64, pc: u64, r: u32) -> Option<Vec<u64>> {
    let Sym::Target { base, load: Load { table, idx, size, signed, scale, at }, shift } = value(mem, defs, entry, pc, r, 0)?
    else {
        return None;
    };
    let count = bound(mem, entry, at, idx)?;
    if count == 0 || count > MAX_ENTRIES || (1u32 << scale) != size {
        return None;
    }
    let raw = mem.bytes(table?, (count as usize) * size as usize)?;
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

/// For a jump (as `resolve`) with the shape of a table (`base + (entry << shift)`) that `resolve` cannot read (the
/// table address or its bounds check is out of sight), the range every target lies in: any entry the load's size can
/// hold.
pub fn reach(mem: &dyn Memory, defs: Defs, entry: u64, pc: u64, r: u32) -> Option<std::ops::RangeInclusive<u64>> {
    let Sym::Target { base, load, shift } = value(mem, defs, entry, pc, r, 0)? else { return None };
    let bits = load.size as u64 * 8;
    let (lo, hi) = if load.signed { (-(1i64 << (bits - 1)), (1i64 << (bits - 1)) - 1) } else { (0, (1i64 << bits) - 1) };
    Some(base.wrapping_add((lo << shift) as u64)..=base.wrapping_add((hi << shift) as u64))
}

/// The symbolic value of register `r` just before `pc`, from the closest instruction before it that writes it.
fn value(mem: &dyn Memory, defs: Defs, entry: u64, pc: u64, r: u32, depth: u32) -> Option<Sym> {
    if depth > 4 || r == 31 {
        return None;
    }
    // Out of sight of the instructions just before: the same constant from every definition that reaches `pc`.
    let reached = || -> Option<Sym> {
        let ds = defs(pc, r)?;
        let mut out = None;
        for d in ds {
            let v = match value(mem, defs, entry, d + 4, r, depth + 1)? {
                Sym::Const(v) => v,
                _ => return None,
            };
            if out.is_some_and(|o| o != v) {
                return None;
            }
            out = Some(v);
        }
        out.map(Sym::Const)
    };
    let mut a = pc;
    while a > entry && pc - a < WINDOW * 4 {
        a -= 4;
        let w = mem.word(a)?;
        if is_branch(w) && !is_bcond(w) && !(is_call(w) && callee_saved(r)) {
            return reached();
        }
        if !writes(w, r) {
            continue;
        }
        if let Some(src) = mov_reg(w) {
            return value(mem, defs, entry, a, src, depth + 1);
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
            let Sym::Const(b) = value(mem, defs, entry, a, bits(w, 5, 5), depth + 1)? else { return None };
            return Some(Sym::Const(b.wrapping_add(imm)));
        }
        if w & 0x3f20_0000 == 0x3820_0000 && bits(w, 10, 2) == 2 && bits(w, 26, 1) == 0 {
            // ldr{b,h,sb,sh,sw} / ldr w (register offset)
            let (size, opc) = (bits(w, 30, 2), bits(w, 22, 2));
            let (option, s) = (bits(w, 13, 3), bits(w, 12, 1));
            if opc == 0 || size == 3 || (size == 2 && opc == 3) || !matches!(option, 2 | 3 | 6 | 7) {
                return None;
            }
            let table = match value(mem, defs, entry, a, bits(w, 5, 5), depth + 1) {
                Some(Sym::Const(t)) => Some(t),
                _ => None,
            };
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
            let (n, m) = (value(mem, defs, entry, a, bits(w, 5, 5), depth + 1), value(mem, defs, entry, a, bits(w, 16, 5), depth + 1));
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
    reached()
}

/// The number of table entries, from the bounds check of register `idx` before the table load at `load_pc` (following
/// register copies and zero extensions: `cmp w0, #5; b.hi ..; mov w8, w0; .. ldrsw x8, [x9, x8, lsl #2]`).
///
/// The check is the first compare before the bounds branch (the one whose flags it reads). It may compare a register the
/// index is copied from further back, with no write to that register in between: BotW 1.0.0 main+0x109b90c
/// `mov w12, w11; cmp w11, #3; b.hi ..; ldrsw x11, [x26, x12, lsl #2]`, where an earlier `cmp w11, #1` (for a ccmp)
/// was taken for the bound and two of the four cases were left unlifted.
fn bound(mem: &dyn Memory, entry: u64, load_pc: u64, mut idx: u32) -> Option<u64> {
    let mut a = load_pc;
    let mut limit_cond = None;
    // The check found on another register than the index (so far): that register and the entry count.
    let mut check: Option<(u32, u64)> = None;
    while a > entry && load_pc - a < WINDOW * 4 {
        a -= 4;
        let w = mem.word(a)?;
        if is_bcond(w) {
            if check.is_none() {
                limit_cond.get_or_insert(bits(w, 0, 4));
            }
            continue;
        }
        if is_branch(w) && !(is_call(w) && callee_saved(idx)) {
            return None;
        }
        // cmp wN / xN, #imm (subs zr, rn, #imm): the first one is the check.
        if check.is_none() && w & 0x7f80_001f == 0x7100_001f {
            let imm = (bits(w, 10, 12) as u64) << if bits(w, 22, 1) == 1 { 12 } else { 0 };
            let entries = match limit_cond? {
                8 | 9 => imm + 1, // b.hi default / b.ls cases: 0..=imm
                2 | 3 => imm,     // b.cs default / b.cc cases: 0..imm
                _ => return None,
            };
            let reg = bits(w, 5, 5);
            if reg == idx {
                return Some(entries);
            }
            check = Some((reg, entries));
            continue;
        }
        if writes(w, idx) {
            match mov_reg(w).or_else(|| zero_extend(w)) {
                Some(src) => idx = src,
                None => return None,
            }
            if let Some((reg, entries)) = check {
                if reg == idx {
                    return Some(entries);
                }
            }
            continue;
        }
        // The compared register changed between the copy and the check: they hold different values.
        if let Some((reg, _)) = check {
            if writes(w, reg) {
                return None;
            }
        }
    }
    None
}

/// The source register of `mov wd/xd, ws/xs` (orr with the zero register, unshifted).
fn mov_reg(w: u32) -> Option<u32> {
    (w & 0x7f20_ffe0 == 0x2a00_03e0).then(|| bits(w, 16, 5))
}

/// The source register of a zero extension of its low 32 bits: `and xd, xs, #0xffffffff` / `ubfx xd, xs, #0, #32`
/// (uxtw), which keep a 32-bit bound.
fn zero_extend(w: u32) -> Option<u32> {
    (w & 0xffff_fc00 == 0x9240_7c00 || w & 0xffff_fc00 == 0xd340_7c00).then(|| bits(w, 5, 5))
}

pub(crate) fn is_call(w: u32) -> bool {
    w & 0xfc00_0000 == 0x9400_0000 || w & 0xffff_fc1f == 0xd63f_0000
}

/// Registers a call preserves (AAPCS64 x19-x28): a table base kept in one survives the call.
pub(crate) fn callee_saved(r: u32) -> bool {
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
pub(crate) fn writes(w: u32, r: u32) -> bool {
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
        let pair_rt2 = bits(w, 27, 3) == 0b101 && load && bits(w, 26, 1) == 0 && bits(w, 10, 5) == r;
        let excl_status = bits(w, 24, 6) == 0b001000 && !load && bits(w, 16, 5) == r;
        return (load && bits(w, 26, 1) == 0 && bits(w, 0, 5) == r) || (rn_wb && bits(w, 5, 5) == r) || pair_rt2
            || excl_status;
    }
    if is_branch(w) {
        return bits(w, 26, 5) == 0b00101 && bits(w, 31, 1) == 1 && r == 30;
    }
    if op0 & 0b0111 == 0b0111 {
        // floating point and SIMD: only conversions to integers (fmov w, s; fcvtzs ..) and smov / umov (imm4 0101 /
        // 0111) have a general register as Rd. dup and ins, in the same copy group, write a vector register (BotW 1.9.0
        // main+0x1440a4, dup v25: taken for a write of x25, it hid a jump table's base).
        let to_general =
            w & 0x7f20_fc00 == 0x1e20_0000 || (w & 0xbfe0_8400 == 0x0e00_0400 && matches!(bits(w, 11, 4), 0b0101 | 0b0111));
        return to_general && bits(w, 0, 5) == r;
    }
    bits(w, 0, 5) == r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// dup v25.4h, v25.h[1] writes a vector register; umov w0, v1.s[0] a general one.
    #[test]
    fn copy_group_writes_general_only_for_smov_umov() {
        assert!(!writes(0x0e0c_0739, 25));
        assert!(writes(0x0e04_3c20, 0));
    }

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
        assert_eq!(resolve(&img, &|_, _| None, 0x1000, 0x101c, 11), Some(vec![0x1020, 0x1024, 0x1028, 0x1030]));
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
        assert_eq!(resolve(&img, &|_, _| None, 0x2000, 0x201c, 8), Some(vec![0x2020, 0x2028, 0x2030, 0x2038, 0x2040, 0x2048]));
    }

    #[test]
    fn general_register_writes() {
        assert!(!writes(0x1e262890, 16)); // fadd s16, s4, s6
        assert!(writes(0x1e260010, 16)); // fmov w16, s0
        assert!(writes(0x0e043c10, 16)); // umov w16, v0.s[0]
        assert!(!writes(0xad404410, 17)); // ldp q16, q17, [x0]
        assert!(writes(0xa9404410, 17)); // ldp x16, x17, [x0]
    }

    #[test]
    fn zero_extended_index() {
        // TotK: 0x3000: cmp w27, #2; b.hi ..; ubfx x8, x27, #0, #32; adrp x11, 0x3000; add x11, x11, #0x100;
        //       adr x9, 0x3028; ldrh w10, [x11, x8, lsl #1]; add x9, x9, x10, lsl #2; mov w23, wzr; br x9
        let code: [u32; 10] = [
            0x71000b7f, 0x54000208, 0xd3407f68, 0x9000000b, 0x9104016b, 0x100000a9, 0x7868796a, 0x8b0a0929, 0x2a1f03f7,
            0xd61f0120,
        ];
        let mut data = vec![0u8; 0x200];
        for (i, w) in code.iter().enumerate() {
            data[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        for (i, e) in [0u16, 1, 3].iter().enumerate() {
            data[0x100 + 2 * i..0x102 + 2 * i].copy_from_slice(&e.to_le_bytes());
        }
        let img = Img { base: 0x3000, data };
        assert_eq!(resolve(&img, &|_, _| None, 0x3000, 0x3024, 9), Some(vec![0x3028, 0x302c, 0x3034]));
    }

    #[test]
    fn check_on_the_index_source() {
        // BotW 1.0.0 main+0x109b900 (moved to 0x4000): cmp w11, #1 (another branch's compare); mov w12, w11;
        // cmp w11, #3; b.hi ..; adrp x26, 0x4000; add x26, x26, #0x100; ldrsw x11, [x26, x12, lsl #2];
        // add x11, x11, x26; br x11. Four entries, not the two of the first compare.
        let code: [u32; 9] = [
            0x7100057f, 0x2a0b03ec, 0x71000d7f, 0x54000208, 0x9000001a, 0x9104035a, 0xb8ac7b4b, 0x8b1a016b,
            0xd61f0160,
        ];
        let mut data = vec![0u8; 0x200];
        for (i, w) in code.iter().enumerate() {
            data[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
        }
        // Targets 0x4024 + 4k, as offsets from the table at 0x4100.
        for k in 0..4i32 {
            let off = (0x4024 + 4 * k) - 0x4100;
            data[0x100 + 4 * k as usize..0x104 + 4 * k as usize].copy_from_slice(&off.to_le_bytes());
        }
        let img = Img { base: 0x4000, data };
        assert_eq!(resolve(&img, &|_, _| None, 0x4000, 0x4020, 11), Some(vec![0x4024, 0x4028, 0x402c, 0x4030]));
    }
}
