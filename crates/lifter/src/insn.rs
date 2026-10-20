// One A64 instruction -> C (runtime/cpu.h). Integer, memory, branch and system instructions; FP/SIMD is not lifted
// yet (it traps: docs/windows-port.md). Semantics follow LAS Recomp's tools/lifter.py (see LICENSE-LASRecomp), with
// returns that keep x30's value (Ret), exclusive pairs, and stricter rejection of reserved encodings.

use crate::bits::{bits, decode_bit_masks, hx, ones, sext};

/// The lifted form of one instruction.
#[derive(Clone, Debug, PartialEq)]
pub enum Insn {
    /// Straight-line C statements; execution continues with the next instruction.
    Op(String),
    /// Unconditional branch to an address.
    B(u64),
    /// Conditional branch: the C condition, the target.
    BCond(String, u64),
    /// Call (bl) to an address.
    Bl(u64),
    /// Call through a register (blr): the C expression of the target.
    Blr(String),
    /// Jump through a register (br).
    Br(String),
    /// Return to the address in a register (ret, usually x30).
    Ret(String),
    /// Does not continue (udf, brk): a C statement that does not return.
    Trap(String),
}

impl Insn {
    /// Whether execution never falls through to the next instruction.
    pub fn ends_block(&self) -> bool {
        matches!(self, Insn::B(_) | Insn::Br(_) | Insn::Ret(_) | Insn::Trap(_))
    }
}

/// An encoding the lifter does not handle (reserved, or not implemented yet).
#[derive(Clone, Debug, PartialEq)]
pub struct Unsupported(pub String);

type R = Result<Insn, Unsupported>;

fn unsupported<T>(what: impl Into<String>) -> Result<T, Unsupported> {
    Err(Unsupported(what.into()))
}

const COND_NAMES: [&str; 16] = ["EQ", "NE", "CS", "CC", "MI", "PL", "VS", "VC", "HI", "LS", "GE", "LT", "GT", "LE", "AL", "NV"];

/// The C condition for an A64 condition code (AL and NV are always true).
pub fn cond_expr(cond: u32) -> String {
    if cond >= 14 {
        "1".into()
    } else {
        format!("COND_{}(c)", COND_NAMES[cond as usize])
    }
}

// ---- register operands: 31 is the zero register or sp depending on the encoding

fn x(r: u32) -> String {
    if r == 31 {
        "0ull".into()
    } else {
        format!("c->x[{r}]")
    }
}

fn xs(r: u32) -> String {
    if r == 31 {
        "c->sp".into()
    } else {
        format!("c->x[{r}]")
    }
}

fn w(r: u32) -> String {
    if r == 31 {
        "0u".into()
    } else {
        format!("(uint32_t)c->x[{r}]")
    }
}

fn ws(r: u32) -> String {
    if r == 31 {
        "(uint32_t)c->sp".into()
    } else {
        format!("(uint32_t)c->x[{r}]")
    }
}

/// Rn/Rm of the operation's width (sf: 64-bit); `sp`: register 31 is sp.
fn reg(sf: bool, r: u32, sp: bool) -> String {
    match (sf, sp) {
        (true, true) => xs(r),
        (true, false) => x(r),
        (false, true) => ws(r),
        (false, false) => w(r),
    }
}

/// A statement writing `expr` to Rd; 32-bit results are zero-extended. Register 31 discards the value unless `sp`.
fn set_reg(sf: bool, r: u32, expr: &str, sp: bool) -> String {
    if r == 31 && !sp {
        return format!("(void)({expr});");
    }
    let dst = if r == 31 { "c->sp".to_string() } else { format!("c->x[{r}]") };
    if sf {
        format!("{dst} = (uint64_t)({expr});")
    } else {
        format!("{dst} = (uint32_t)({expr});")
    }
}

fn op(code: String) -> R {
    Ok(Insn::Op(code))
}

fn size_of(sf: bool) -> u32 {
    if sf {
        64
    } else {
        32
    }
}

fn shift_expr(sf: bool, val: &str, typ: u32, amt: u32) -> String {
    if amt == 0 {
        return val.to_string();
    }
    match typ {
        0 => format!("((uint{}_t){val} << {amt})", size_of(sf)),
        1 => format!("({val} >> {amt})"),
        2 if sf => format!("(uint64_t)((int64_t){val} >> {amt})"),
        2 => format!("(uint32_t)((int32_t){val} >> {amt})"),
        _ => format!("ROR{}({val}, {amt})", size_of(sf)),
    }
}

/// An extended register operand (UXTB .. SXTX) shifted left by `shift`, as 64 bits.
fn extend_expr(r: u32, option: u32, shift: u32) -> String {
    let base = x(r);
    let e = match option {
        0 => format!("(uint64_t)(uint8_t){base}"),
        1 => format!("(uint64_t)(uint16_t){base}"),
        2 => format!("(uint64_t)(uint32_t){base}"),
        4 => format!("(uint64_t)(int64_t)(int8_t){base}"),
        5 => format!("(uint64_t)(int64_t)(int16_t){base}"),
        6 => format!("(uint64_t)(int64_t)(int32_t){base}"),
        _ => base,
    };
    if shift > 0 {
        format!("({e} << {shift})")
    } else {
        e
    }
}

/// Lifts the instruction `word` at guest address `pc`.
pub fn lift(word: u32, pc: u64) -> R {
    let op0 = bits(word, 25, 4);
    if op0 & 0b1110 == 0b1000 {
        dp_imm(word, pc)
    } else if op0 & 0b1110 == 0b1010 {
        branch_sys(word, pc)
    } else if op0 & 0b0101 == 0b0100 {
        ldst(word, pc)
    } else if op0 & 0b0111 == 0b0101 {
        dp_reg(word)
    } else if op0 & 0b0111 == 0b0111 {
        unsupported("fp/simd data processing")
    } else {
        // udf, padding and data in the text segment: trap if ever executed.
        Ok(Insn::Trap(format!("rt_udf(c, {});", hx(pc))))
    }
}

// ---- data processing (immediate)

fn dp_imm(wd: u32, pc: u64) -> R {
    let sf = bits(wd, 31, 1) == 1;
    let (rd, rn) = (bits(wd, 0, 5), bits(wd, 5, 5));
    match bits(wd, 23, 3) {
        0 | 1 => {
            // ADR / ADRP
            let imm = sext(((bits(wd, 5, 19) as u64) << 2) | bits(wd, 29, 2) as u64, 21);
            let val = if bits(wd, 31, 1) == 1 {
                (pc & !0xfff).wrapping_add((imm << 12) as u64)
            } else {
                pc.wrapping_add(imm as u64)
            };
            op(set_reg(true, rd, &hx(val), false))
        }
        2 => {
            // ADD / SUB (immediate)
            if bits(wd, 23, 1) == 1 {
                return unsupported("add/sub imm with tags");
            }
            let (sub, s, sh) = (bits(wd, 30, 1) == 1, bits(wd, 29, 1) == 1, bits(wd, 22, 1) == 1);
            let imm = (bits(wd, 10, 12) as u64) << if sh { 12 } else { 0 };
            let a = reg(sf, rn, true);
            if s {
                let f = format!("{}{}", if sub { "subs" } else { "adds" }, size_of(sf));
                op(set_reg(sf, rd, &format!("{f}(c, {a}, {})", hx(imm)), false))
            } else {
                op(set_reg(sf, rd, &format!("{a} {} {}", if sub { "-" } else { "+" }, hx(imm)), true))
            }
        }
        4 => {
            // logical (immediate)
            let (opc, n) = (bits(wd, 29, 2), bits(wd, 22, 1));
            if !sf && n == 1 {
                return unsupported("logical imm N=1 in 32-bit");
            }
            let size = size_of(sf);
            let Some((imm, _)) = decode_bit_masks(n, bits(wd, 10, 6), bits(wd, 16, 6), size, true) else {
                return unsupported("reserved bitmask");
            };
            let a = reg(sf, rn, false);
            let i = hx(imm);
            match opc {
                0 => op(set_reg(sf, rd, &format!("{a} & {i}"), true)),
                1 => op(set_reg(sf, rd, &format!("{a} | {i}"), true)),
                2 => op(set_reg(sf, rd, &format!("{a} ^ {i}"), true)),
                _ => op(set_reg(sf, rd, &format!("logic{size}(c, {a} & {i})"), false)),
            }
        }
        5 => {
            // move wide
            let (opc, hw) = (bits(wd, 29, 2), bits(wd, 21, 2));
            if !sf && hw > 1 {
                return unsupported("move wide hw");
            }
            let imm = (bits(wd, 5, 16) as u64) << (hw * 16);
            match opc {
                0 => op(set_reg(sf, rd, &hx(!imm & ones(size_of(sf))), false)),
                2 => op(set_reg(sf, rd, &hx(imm), false)),
                3 => {
                    let mask = !(0xffffu64 << (hw * 16));
                    op(set_reg(sf, rd, &format!("({} & {}) | {}", x(rd), hx(mask), hx(imm)), false))
                }
                _ => unsupported("move wide opc"),
            }
        }
        6 => {
            // bitfield
            let (opc, n) = (bits(wd, 29, 2), bits(wd, 22, 1));
            let (immr, imms) = (bits(wd, 16, 6), bits(wd, 10, 6));
            let size = size_of(sf);
            if n != sf as u32 || (!sf && (immr > 31 || imms > 31)) {
                return unsupported("bitfield N / immediate");
            }
            let Some((wmask, tmask)) = decode_bit_masks(n, imms, immr, size, false) else {
                return unsupported("reserved bitfield");
            };
            let src = reg(sf, rn, false);
            let rot = if immr > 0 { format!("ROR{size}({src}, {immr})") } else { src.clone() };
            let m = |v: u64| hx(v & ones(size));
            match opc {
                2 => op(set_reg(sf, rd, &format!("{rot} & {}", m(wmask & tmask)), false)),
                0 => {
                    let top = format!("(uint{size}_t)(-(int{size}_t)(({src} >> {imms}) & 1))");
                    op(set_reg(sf, rd, &format!("({top} & {}) | ({rot} & {})", m(!tmask), m(wmask & tmask)), false))
                }
                1 => {
                    let dst = reg(sf, rd, false);
                    let bot = format!("(({dst} & {}) | ({rot} & {}))", m(!wmask), m(wmask));
                    op(set_reg(sf, rd, &format!("({dst} & {}) | ({bot} & {})", m(!tmask), m(tmask)), false))
                }
                _ => unsupported("bitfield opc"),
            }
        }
        7 => {
            // EXTR
            let (rm, lsb) = (bits(wd, 16, 5), bits(wd, 10, 6));
            let size = size_of(sf);
            if bits(wd, 29, 2) != 0 || bits(wd, 22, 1) != sf as u32 || bits(wd, 21, 1) != 0 || lsb >= size {
                return unsupported("extr variant");
            }
            if lsb == 0 {
                return op(set_reg(sf, rd, &reg(sf, rm, false), false));
            }
            op(set_reg(
                sf,
                rd,
                &format!("({} >> {lsb}) | ((uint{size}_t){} << {})", reg(sf, rm, false), reg(sf, rn, false), size - lsb),
                false,
            ))
        }
        _ => unsupported("dp imm"),
    }
}

// ---- branches, exceptions, system

fn branch_sys(wd: u32, pc: u64) -> R {
    if bits(wd, 26, 5) == 0b00101 {
        // B / BL
        let t = pc.wrapping_add((sext(bits(wd, 0, 26) as u64, 26) * 4) as u64);
        return Ok(if bits(wd, 31, 1) == 1 { Insn::Bl(t) } else { Insn::B(t) });
    }
    if bits(wd, 25, 7) == 0b0101010 {
        // B.cond
        if bits(wd, 24, 1) != 0 || bits(wd, 4, 1) != 0 {
            return unsupported("b.cond variant");
        }
        let t = pc.wrapping_add((sext(bits(wd, 5, 19) as u64, 19) * 4) as u64);
        let cond = bits(wd, 0, 4);
        return Ok(if cond >= 14 { Insn::B(t) } else { Insn::BCond(cond_expr(cond), t) });
    }
    if bits(wd, 25, 6) == 0b011010 {
        // CBZ / CBNZ
        let (sf, nz, rt) = (bits(wd, 31, 1) == 1, bits(wd, 24, 1) == 1, bits(wd, 0, 5));
        let t = pc.wrapping_add((sext(bits(wd, 5, 19) as u64, 19) * 4) as u64);
        return Ok(Insn::BCond(format!("{} {} 0", reg(sf, rt, false), if nz { "!=" } else { "==" }), t));
    }
    if bits(wd, 25, 6) == 0b011011 {
        // TBZ / TBNZ
        let b = (bits(wd, 31, 1) << 5) | bits(wd, 19, 5);
        let (nz, rt) = (bits(wd, 24, 1) == 1, bits(wd, 0, 5));
        let t = pc.wrapping_add((sext(bits(wd, 5, 14) as u64, 14) * 4) as u64);
        return Ok(Insn::BCond(format!("(({} >> {b}) & 1) {} 0", x(rt), if nz { "!=" } else { "==" }), t));
    }
    if bits(wd, 25, 7) == 0b1101011 {
        // branch (register)
        let (opc, rn) = (bits(wd, 21, 4), bits(wd, 5, 5));
        if bits(wd, 16, 5) != 31 || bits(wd, 10, 6) != 0 || bits(wd, 0, 5) != 0 {
            return unsupported("branch register variant (pointer authentication)");
        }
        return match opc {
            0 => Ok(Insn::Br(x(rn))),
            1 => Ok(Insn::Blr(x(rn))),
            2 => Ok(Insn::Ret(x(rn))),
            _ => unsupported("branch register opc"),
        };
    }
    if bits(wd, 24, 8) == 0b1101_0100 {
        // exception generation
        let (opc, ll, imm) = (bits(wd, 21, 3), bits(wd, 0, 5), bits(wd, 5, 16));
        if bits(wd, 2, 3) != 0 {
            return unsupported("exception op2");
        }
        return match (opc, ll) {
            (0, 1) => op(format!("rt_svc(c, {imm}, {});", hx(pc))),
            (1, 0) => Ok(Insn::Trap(format!("rt_brk(c, {imm}, {});", hx(pc)))),
            _ => unsupported("exception generation"),
        };
    }
    if bits(wd, 22, 10) == 0b11_0101_0100 {
        return system(wd);
    }
    unsupported("branch/system")
}

fn system(wd: u32) -> R {
    let l = bits(wd, 21, 1) == 1;
    let (op0, op1, crn, crm, op2, rt) =
        (bits(wd, 19, 2), bits(wd, 16, 3), bits(wd, 12, 4), bits(wd, 8, 4), bits(wd, 5, 3), bits(wd, 0, 5));
    if op0 == 0 && !l {
        if crn == 2 && op1 == 3 && rt == 31 {
            return op("/* hint */".into()); // nop, yield, wfe, ..., also the PAC hints
        }
        if crn == 3 && op1 == 3 && rt == 31 {
            return match op2 {
                2 => op("c->excl_addr = ~0ull;".into()), // clrex
                4..=6 => op("__atomic_thread_fence(__ATOMIC_SEQ_CST);".into()), // dsb, dmb, isb
                _ => unsupported("barrier"),
            };
        }
        return unsupported("pstate");
    }
    if op0 == 1 {
        // SYS: cache maintenance
        if !l && op1 == 3 && crn == 7 && crm == 4 && op2 == 1 {
            return op(format!("memset((void *)(uintptr_t)({} & ~63ull), 0, 64);", x(rt))); // dc zva
        }
        if !l && crn == 7 {
            return op("/* cache maintenance */".into());
        }
        return unsupported("sys");
    }
    let key = (op0, op1, crn, crm, op2);
    if l {
        let src = match key {
            (3, 3, 13, 0, 3) => "c->tpidrro",
            (3, 3, 13, 0, 2) => "c->tpidr",
            (3, 3, 14, 0, 0) => "19200000ull", // cntfrq_el0: the Switch's 19.2 MHz counter
            (3, 3, 14, 0, 1) | (3, 3, 14, 0, 2) => "rt_ticks()", // cntpct_el0, cntvct_el0
            (3, 3, 4, 2, 0) => "c->nzcv",
            (3, 3, 4, 4, 0) => "c->fpcr",
            (3, 3, 4, 4, 1) => "c->fpsr",
            (3, 3, 0, 0, 7) => "4ull",          // dczid_el0: dc zva clears 64 bytes
            (3, 3, 0, 0, 1) => "0x8444c004ull", // ctr_el0: the Cortex-A57's
            _ => return unsupported(format!("mrs {key:?}")),
        };
        return op(set_reg(true, rt, src, false));
    }
    let v = x(rt);
    match key {
        (3, 3, 13, 0, 2) => op(format!("c->tpidr = {v};")),
        (3, 3, 4, 2, 0) => op(format!("c->nzcv = {v} & 0xf0000000ull;")),
        (3, 3, 4, 4, 0) => op(format!("c->fpcr = {v} & 0x7c00000ull;")),
        (3, 3, 4, 4, 1) => op(format!("c->fpsr = {v} & 0xf800009full;")),
        _ => unsupported(format!("msr {key:?}")),
    }
}

// ---- loads and stores

fn ldst(wd: u32, pc: u64) -> R {
    if bits(wd, 26, 1) == 1 {
        return unsupported("fp/simd load/store");
    }
    if bits(wd, 24, 6) == 0b001000 {
        return ldst_excl(wd);
    }
    if bits(wd, 27, 3) == 0b011 && bits(wd, 24, 2) == 0 {
        // load (literal)
        let (opc, rt) = (bits(wd, 30, 2), bits(wd, 0, 5));
        let addr = hx(pc.wrapping_add((sext(bits(wd, 5, 19) as u64, 19) * 4) as u64));
        return match opc {
            0 => op(set_reg(false, rt, &format!("LD32({addr})"), false)),
            1 => op(set_reg(true, rt, &format!("LD64({addr})"), false)),
            2 => op(set_reg(true, rt, &format!("(int64_t)(int32_t)LD32({addr})"), false)),
            _ => op("/* prfm */".into()),
        };
    }
    if bits(wd, 27, 3) == 0b101 {
        return ldst_pair(wd);
    }
    if bits(wd, 27, 3) == 0b111 {
        return ldst_reg(wd);
    }
    unsupported("load/store class")
}

fn ldst_excl(wd: u32) -> R {
    let (size, o2, l, o1, rs, rt2, rn, rt) = (
        bits(wd, 30, 2),
        bits(wd, 23, 1),
        bits(wd, 22, 1) == 1,
        bits(wd, 21, 1),
        bits(wd, 16, 5),
        bits(wd, 10, 5),
        bits(wd, 5, 5),
        bits(wd, 0, 5),
    );
    let nbits = 8u32 << size;
    let addr = xs(rn);
    if o1 == 1 {
        // LDXP / STXP (and the acquire/release forms): 32- or 64-bit register pairs
        if o2 == 1 || size < 2 {
            return unsupported("compare and swap pair");
        }
        if l {
            let half = nbits;
            let lo = if size == 3 { "(uint64_t)v".to_string() } else { "(uint32_t)v".to_string() };
            let hi = format!("(uint64_t)(v >> {half})");
            let v = if size == 3 {
                format!("unsigned __int128 v = ldxp128(c, {addr});")
            } else {
                format!("uint64_t v = ldx64(c, {addr});")
            };
            return op(format!("{{ {v} {} {} }}", set_reg(true, rt, &lo, false), set_reg(true, rt2, &hi, false)));
        }
        let st = if size == 3 {
            format!("stxp128(c, {addr}, {}, {})", x(rt), x(rt2))
        } else {
            format!("stx64(c, {addr}, (uint64_t){} | ((uint64_t){} << 32))", w(rt), w(rt2))
        };
        return op(format!("{{ uint32_t st = {st}; {} }}", set_reg(false, rs, "st", false)));
    }
    if o2 == 0 {
        // LDXR / LDAXR / STXR / STLXR
        if l {
            return op(set_reg(true, rt, &format!("ldx{nbits}(c, {addr})"), false));
        }
        return op(format!(
            "{{ uint32_t st = stx{nbits}(c, {addr}, {}); {} }}",
            x(rt),
            set_reg(false, rs, "st", false)
        ));
    }
    if bits(wd, 15, 1) == 0 {
        return unsupported("load/store LO (v8.1)");
    }
    // LDAR / STLR
    if l {
        return op(set_reg(true, rt, &format!("lda{nbits}({addr})"), false));
    }
    op(format!("stl{nbits}({addr}, {});", x(rt)))
}

fn ldst_pair(wd: u32) -> R {
    let (opc, mode, l) = (bits(wd, 30, 2), bits(wd, 23, 2), bits(wd, 22, 1) == 1);
    let (imm7, rt2, rn, rt) = (sext(bits(wd, 15, 7) as u64, 7), bits(wd, 10, 5), bits(wd, 5, 5), bits(wd, 0, 5));
    if opc == 3 || (opc == 1 && (!l || mode == 0)) {
        return unsupported("load/store pair opc"); // (stgp, and no non-temporal ldpsw)
    }
    let scale = if opc == 2 { 3 } else { 2 };
    let off = imm7 << scale;
    let base = xs(rn);
    let post = mode == 1;
    let wb = mode == 1 || mode == 3;
    let ea = if post { base.clone() } else { offset_expr(&base, off) };
    let mut lines = vec![format!("uint64_t ea = {ea};")];
    if l {
        lines.push(match opc {
            2 => "uint64_t a = LD64(ea), b = LD64(ea + 8);".into(),
            1 => "uint64_t a = (int64_t)(int32_t)LD32(ea), b = (int64_t)(int32_t)LD32(ea + 4);".into(),
            _ => "uint64_t a = LD32(ea), b = LD32(ea + 4);".into(),
        });
    } else if opc == 2 {
        lines.push(format!("ST64(ea, {}); ST64(ea + 8, {});", x(rt), x(rt2)));
    } else {
        lines.push(format!("ST32(ea, {}); ST32(ea + 4, {});", w(rt), w(rt2)));
    }
    if wb {
        let dst = xs(rn);
        lines.push(if post { format!("{dst} = {};", offset_expr("ea", off)) } else { format!("{dst} = ea;") });
    }
    if l {
        lines.push(set_reg(true, rt, "a", false));
        lines.push(set_reg(true, rt2, "b", false));
    }
    op(format!("{{ {} }}", lines.join(" ")))
}

fn offset_expr(base: &str, off: i64) -> String {
    match off {
        0 => base.to_string(),
        o if o > 0 => format!("{base} + {o}"),
        o => format!("{base} - {}", -o),
    }
}

fn ldst_reg(wd: u32) -> R {
    let (size, opc, rn, rt) = (bits(wd, 30, 2), bits(wd, 22, 2), bits(wd, 5, 5), bits(wd, 0, 5));
    let base = xs(rn);
    let mut wb = None;
    let ea = if bits(wd, 24, 2) == 1 {
        format!("{base} + {}", hx((bits(wd, 10, 12) as u64) << size))
    } else {
        let kind = bits(wd, 10, 2);
        if bits(wd, 21, 1) == 1 {
            if kind != 2 {
                return unsupported("atomic memory operation (v8.1) / pointer authentication");
            }
            let (rm, option, s) = (bits(wd, 16, 5), bits(wd, 13, 3), bits(wd, 12, 1));
            if !matches!(option, 2 | 3 | 6 | 7) {
                return unsupported("load/store register offset option");
            }
            format!("{base} + {}", extend_expr(rm, option, if s == 1 { size } else { 0 }))
        } else {
            let imm9 = sext(bits(wd, 12, 9) as u64, 9);
            match kind {
                0 | 2 => offset_expr(&base, imm9), // unscaled, unprivileged
                1 => {
                    wb = Some(offset_expr(&base, imm9)); // post-index
                    base.clone()
                }
                _ => {
                    wb = Some("ea".to_string()); // pre-index
                    offset_expr(&base, imm9)
                }
            }
        }
    };
    let nbits = 8u32 << size;
    let (access, load) = match opc {
        0 => (format!("ST{nbits}(ea, {});", if size == 3 { x(rt) } else { w(rt) }), None),
        1 => (format!("uint64_t v = LD{nbits}(ea);"), Some(set_reg(true, rt, "v", false))),
        _ if size == 3 => {
            // prfm (unsigned offset, register offset) and prfum; there is no pre/post-indexed or unprivileged form
            let prefetch = bits(wd, 24, 2) == 1 || bits(wd, 21, 1) == 1 || bits(wd, 10, 2) == 0;
            if opc == 2 && prefetch {
                return op("/* prfm */".into());
            }
            return unsupported("load/store size 3 opc 3 / prefetch form");
        }
        3 if size == 2 => return unsupported("load/store size 2 opc 3"),
        _ => (
            format!("uint64_t v = (uint64_t)(int64_t)(int{nbits}_t)LD{nbits}(ea);"),
            Some(set_reg(opc == 2, rt, "v", false)),
        ),
    };
    let mut parts = vec![format!("uint64_t ea = {ea};"), access];
    if let Some(wb) = wb {
        // write back before the register load, so a load into rn wins
        parts.push(format!("{} = {wb};", xs(rn)));
    }
    parts.extend(load);
    op(format!("{{ {} }}", parts.join(" ")))
}

// ---- data processing (register)

fn dp_reg(wd: u32) -> R {
    let sf = bits(wd, 31, 1) == 1;
    let (rd, rn, rm) = (bits(wd, 0, 5), bits(wd, 5, 5), bits(wd, 16, 5));
    let size = size_of(sf);
    if bits(wd, 28, 1) == 0 {
        if bits(wd, 24, 4) == 0b1010 {
            // logical (shifted register)
            let (opc, sh, n, amt) = (bits(wd, 29, 2), bits(wd, 22, 2), bits(wd, 21, 1), bits(wd, 10, 6));
            if !sf && amt >= 32 {
                return unsupported("logical shift amount");
            }
            let mut b = shift_expr(sf, &reg(sf, rm, false), sh, amt);
            if n == 1 {
                b = format!("(uint{size}_t)~{b}");
            }
            let a = reg(sf, rn, false);
            let e = match opc {
                1 => format!("{a} | {b}"),
                2 => format!("{a} ^ {b}"),
                _ => format!("{a} & {b}"),
            };
            if opc == 3 {
                return op(set_reg(sf, rd, &format!("logic{size}(c, (uint{size}_t)({e}))"), false));
            }
            return op(set_reg(sf, rd, &e, false));
        }
        if bits(wd, 24, 4) == 0b1011 {
            // add/sub (shifted or extended register)
            let (sub, s) = (bits(wd, 30, 1) == 1, bits(wd, 29, 1) == 1);
            let (a, b, sp_ok);
            if bits(wd, 21, 1) == 0 {
                let (sh, amt) = (bits(wd, 22, 2), bits(wd, 10, 6));
                if sh == 3 || (!sf && amt >= 32) {
                    return unsupported("add/sub shift");
                }
                a = reg(sf, rn, false);
                b = shift_expr(sf, &reg(sf, rm, false), sh, amt);
                sp_ok = false;
            } else {
                let (option, amt) = (bits(wd, 13, 3), bits(wd, 10, 3));
                if bits(wd, 22, 2) != 0 || amt > 4 {
                    return unsupported("add/sub extend");
                }
                a = reg(sf, rn, true);
                let e = extend_expr(rm, option, amt);
                b = if sf { e } else { format!("(uint32_t)({e})") };
                sp_ok = !s;
            }
            if s {
                let f = format!("{}{size}", if sub { "subs" } else { "adds" });
                return op(set_reg(sf, rd, &format!("{f}(c, {a}, (uint{size}_t)({b}))"), false));
            }
            return op(set_reg(sf, rd, &format!("{a} {} {b}", if sub { "-" } else { "+" }), sp_ok));
        }
        return unsupported("dp reg");
    }
    if bits(wd, 24, 4) == 0b1011 {
        // data processing (3 source)
        let (op54, op31, o0, ra) = (bits(wd, 29, 2), bits(wd, 21, 3), bits(wd, 15, 1) == 1, bits(wd, 10, 5));
        if op54 != 0 {
            return unsupported("dp3 op54");
        }
        let sgn = if o0 { "-" } else { "+" };
        return match (op31, sf) {
            (0, _) => op(set_reg(
                sf,
                rd,
                &format!("{} {sgn} {} * {}", reg(sf, ra, false), reg(sf, rn, false), reg(sf, rm, false)),
                false,
            )),
            (1, true) => op(set_reg(
                true,
                rd,
                &format!("{} {sgn} (uint64_t)((int64_t)(int32_t){} * (int64_t)(int32_t){})", x(ra), w(rn), w(rm)),
                false,
            )),
            (5, true) => op(set_reg(true, rd, &format!("{} {sgn} (uint64_t){} * (uint64_t){}", x(ra), w(rn), w(rm)), false)),
            (2, true) if !o0 => op(set_reg(
                true,
                rd,
                &format!("(uint64_t)(((__int128)(int64_t){} * (__int128)(int64_t){}) >> 64)", x(rn), x(rm)),
                false,
            )),
            (6, true) if !o0 => op(set_reg(
                true,
                rd,
                &format!("(uint64_t)(((unsigned __int128){} * {}) >> 64)", x(rn), x(rm)),
                false,
            )),
            _ => unsupported("dp3"),
        };
    }
    let s = bits(wd, 29, 1) == 1;
    match bits(wd, 21, 4) {
        0 => {
            // add/sub with carry
            if bits(wd, 10, 6) != 0 {
                return unsupported("rmif / setf");
            }
            let f = format!("{}{}{size}", if bits(wd, 30, 1) == 1 { "sbc" } else { "adc" }, if s { "s" } else { "" });
            op(set_reg(sf, rd, &format!("{f}(c, {}, {})", reg(sf, rn, false), reg(sf, rm, false)), false))
        }
        2 => {
            // conditional compare (register / immediate)
            if !s || bits(wd, 10, 1) != 0 || bits(wd, 4, 1) != 0 {
                return unsupported("ccmp variant");
            }
            let (sub, imm_form, cond, nzcv) =
                (bits(wd, 30, 1) == 1, bits(wd, 11, 1) == 1, bits(wd, 12, 4), bits(wd, 0, 4));
            let b = if imm_form { hx(rm as u64) } else { reg(sf, rm, false) };
            let f = format!("{}{size}", if sub { "subs" } else { "adds" });
            op(format!(
                "if ({}) (void){f}(c, {}, {b}); else c->nzcv = {};",
                cond_expr(cond),
                reg(sf, rn, false),
                hx((nzcv as u64) << 28)
            ))
        }
        4 => {
            // conditional select
            let (inv, o2, cond) = (bits(wd, 30, 1), bits(wd, 10, 2), bits(wd, 12, 4));
            if s || o2 > 1 {
                return unsupported("csel variant");
            }
            let b = reg(sf, rm, false);
            let alt = match (inv, o2) {
                (0, 0) => b,
                (0, _) => format!("{b} + 1"),
                (_, 0) => format!("~{b}"),
                _ => format!("-{b}"),
            };
            op(set_reg(sf, rd, &format!("({}) ? {} : (uint{size}_t)({alt})", cond_expr(cond), reg(sf, rn, false)), false))
        }
        6 => {
            if s {
                return unsupported("dp1/dp2 S");
            }
            let opcode = bits(wd, 10, 6);
            let a = reg(sf, rn, false);
            if bits(wd, 30, 1) == 1 {
                // data processing (1 source)
                if rm != 0 {
                    return unsupported("dp1 opcode2 (pointer authentication)");
                }
                let e = match (opcode, sf) {
                    (0, _) => format!("rbit{size}({a})"),
                    (1, _) => format!("rev16_{size}({a})"),
                    (2, false) => format!("__builtin_bswap32({a})"),
                    (2, true) => format!("rev32_64({a})"),
                    (3, true) => format!("__builtin_bswap64({a})"),
                    (4, _) => format!("clz{size}({a})"),
                    (5, _) => format!("cls{size}({a})"),
                    _ => return unsupported("dp1"),
                };
                return op(set_reg(sf, rd, &e, false));
            }
            // data processing (2 source)
            if (16..=23).contains(&opcode) {
                return unsupported("crc32");
            }
            let b = reg(sf, rm, false);
            let e = match opcode {
                2 => format!("udiv{size}({a}, {b})"),
                3 => format!("sdiv{size}({a}, {b})"),
                8 => format!("{a} << ({b} & {})", size - 1),
                9 => format!("{a} >> ({b} & {})", size - 1),
                10 => format!("(uint{size}_t)((int{size}_t){a} >> ({b} & {}))", size - 1),
                11 => format!("ROR{size}({a}, {b} & {})", size - 1),
                _ => return unsupported("dp2"),
            };
            op(set_reg(sf, rd, &e, false))
        }
        _ => unsupported("dp reg"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(w: u32) -> String {
        match lift(w, 0x7100_0000).unwrap() {
            Insn::Op(c) => c,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn basics() {
        assert_eq!(code(0x91000420), "c->x[0] = (uint64_t)(c->x[1] + 0x1ull);"); // add x0, x1, #1
        assert_eq!(code(0x910003fd), "c->x[29] = (uint64_t)(c->sp + 0x0ull);"); // mov x29, sp
        assert_eq!(code(0xd503201f), "/* hint */"); // nop
        assert_eq!(lift(0xd65f03c0, 0).unwrap(), Insn::Ret("c->x[30]".into()));
        assert_eq!(lift(0x94000002, 0x1000).unwrap(), Insn::Bl(0x1008));
        assert_eq!(lift(0x17ffffff, 0x1000).unwrap(), Insn::B(0xffc));
        assert!(matches!(lift(0x00000000, 0).unwrap(), Insn::Trap(_))); // udf #0
    }

    #[test]
    fn mov_wide_and_logic() {
        assert_eq!(code(0xd2800540), "c->x[0] = (uint64_t)(0x2aull);"); // mov x0, #42
        assert_eq!(code(0x12800000), "c->x[0] = (uint32_t)(0xffffffffull);"); // mov w0, #-1
        // ands wzr, w1, #1 (tst): the result is discarded, the flags kept
        assert_eq!(code(0x7200003f), "(void)(logic32(c, (uint32_t)c->x[1] & 0x1ull));");
    }
}
