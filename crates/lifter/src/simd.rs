// FP / AdvSIMD / crypto / CRC32 -> portable C over the Cpu struct, calling runtime/simd.h's helpers with constant
// fields (sizes, lanes, ops) so clang folds their switches. ARMv8.0-A + Crypto + CRC32 (the Switch's Cortex-A57);
// later-architecture encodings (FP16 arithmetic, RDM, dot product, ...) are Unsupported. A port of LAS Recomp's
// tools/simd_c.py (see LICENSE-LASRecomp).

use crate::bits::{bits, hx, sext};
use crate::insn::{cond_expr, extend_expr, set_reg, w as wreg, x as xreg, xs, Unsupported};

type R = Result<String, Unsupported>;

fn un<T>(what: &str) -> Result<T, Unsupported> {
    Err(Unsupported(what.to_string()))
}

fn lop(name: &str, u: u32) -> String {
    if u != 0 {
        format!("LOP_{name} | LOP_U")
    } else {
        format!("LOP_{name}")
    }
}

fn vu(sz: u32) -> &'static str {
    ["vu8", "vu16", "vu32", "vu64"][sz as usize]
}

fn vs(sz: u32) -> &'static str {
    ["vs8", "vs16", "vs32", "vs64"][sz as usize]
}

fn ut(sz: u32) -> &'static str {
    ["uint8_t", "uint16_t", "uint32_t", "uint64_t"][sz as usize]
}

fn vr(t: &str, n: u32) -> String {
    format!("VREG({t}, {n})")
}

fn vq(t: &str, q: u32, n: u32) -> String {
    if q != 0 {
        format!("VREG({t}, {n})")
    } else {
        format!("VQ({t}, 0, {n})")
    }
}

fn vw(t: &str, q: u32, d: u32, e: &str) -> String {
    format!("VW({t}, {q}, {d}, {e});")
}

#[allow(clippy::too_many_arguments)]
fn gen(
    op: &str,
    sz: u32,
    ne: u32,
    d: u32,
    szd: u32,
    offd: u32,
    keep: u32,
    n: u32,
    szn: u32,
    offn: u32,
    m: i32,
    szm: u32,
    offm: u32,
    bidx: i32,
    bimm: u64,
) -> String {
    format!(
        "sl_gen(c, {op}, {sz}, {ne}, {d}, {szd}, {offd}, {keep}, {n}, {szn}, {offn}, {m}, {szm}, {offm}, {bidx}, {});",
        hx(bimm)
    )
}

/// A lane-wise op with all operands the same size.
fn same(op: &str, sz: u32, ne: u32, d: u32, n: u32, m: i32, bimm: u64, bidx: i32) -> String {
    gen(op, sz, ne, d, sz, 0, 0, n, sz, 0, m, sz, 0, bidx, bimm)
}

fn lanes(q: u32, sz: u32) -> u32 {
    (8 << q) >> sz
}

fn shuf(a: &str, b: &str, idx: &[u32]) -> String {
    let i: Vec<String> = idx.iter().map(|v| v.to_string()).collect();
    format!("SHUF({a}, {b}, {})", i.join(", "))
}

fn u128_lit(v: u128) -> String {
    let (lo, hi) = (v as u64, (v >> 64) as u64);
    if hi != 0 {
        format!("((u128){} << 64 | {})", hx(hi), hx(lo))
    } else {
        hx(lo)
    }
}

/// Names per FP precision (0 single, 1 double).
struct Ft {
    f: &'static str,
    vt: &'static str,
    vu: &'static str,
    sf: &'static str,
    su: &'static str,
    set: &'static str,
}

const FT: [Ft; 2] = [
    Ft { f: "s", vt: "vf32", vu: "vu32", sf: "VSF32", su: "VS32", set: "FP_SETS" },
    Ft { f: "d", vt: "vf64", vu: "vu64", sf: "VSF64", su: "VS64", set: "FP_SETD" },
];

/// Vector three-same FP: (U, a, opcode) -> FOP.
fn fop3(key: (u32, u32, u32)) -> Option<&'static str> {
    Some(match key {
        (0, 0, 0b11000) => "FOP_MAXNM",
        (0, 0, 0b11010) => "FOP_ADD",
        (0, 0, 0b11011) => "FOP_MULX",
        (0, 0, 0b11110) => "FOP_MAX",
        (0, 0, 0b11111) => "FOP_RECPS",
        (0, 1, 0b11000) => "FOP_MINNM",
        (0, 1, 0b11010) => "FOP_SUB",
        (0, 1, 0b11110) => "FOP_MIN",
        (0, 1, 0b11111) => "FOP_RSQRTS",
        (1, 0, 0b11011) => "FOP_MUL",
        (1, 0, 0b11111) => "FOP_DIV",
        (1, 1, 0b11010) => "FOP_ABD",
        _ => return None,
    })
}

fn fpairv(key: (u32, u32, u32)) -> Option<&'static str> {
    Some(match key {
        (1, 0, 0b11000) => "FOP_MAXNM",
        (1, 1, 0b11000) => "FOP_MINNM",
        (1, 0, 0b11010) => "FOP_ADD",
        (1, 0, 0b11110) => "FOP_MAX",
        (1, 1, 0b11110) => "FOP_MIN",
        _ => return None,
    })
}

fn fcmp3(key: (u32, u32, u32)) -> Option<u32> {
    Some(match key {
        (0, 0, 0b11100) => 0,
        (1, 0, 0b11100) => 1,
        (1, 1, 0b11100) => 2,
        (1, 0, 0b11101) => 3,
        (1, 1, 0b11101) => 4,
        _ => return None,
    })
}

fn vfp_expand_imm(imm8: u64, n: u32) -> u64 {
    let e = if n == 32 { 8 } else { 11 };
    let f = n - e - 1;
    let sign = imm8 >> 7;
    let b6 = (imm8 >> 6) & 1;
    let exp = ((b6 ^ 1) << (e - 1)) | ((if b6 != 0 { (1u64 << (e - 3)) - 1 } else { 0 }) << 2) | ((imm8 >> 4) & 3);
    let frac = (imm8 & 15) << (f - 4);
    (sign << (n - 1)) | (exp << f) | frac
}

fn simd_expand_imm(op: u32, cmode: u32, imm8: u64) -> u64 {
    let rep16 = |v: u64| (0..4).fold(0u64, |a, i| a | v << (16 * i));
    match cmode >> 1 {
        0 => (imm8 << 32) | imm8,
        1 => (imm8 << 40) | (imm8 << 8),
        2 => (imm8 << 48) | (imm8 << 16),
        3 => (imm8 << 56) | (imm8 << 24),
        4 => rep16(imm8),
        5 => rep16(imm8 << 8),
        6 => {
            let v = if cmode & 1 == 0 { (imm8 << 8) | 0xff } else { (imm8 << 16) | 0xffff };
            (v << 32) | v
        }
        _ => match (cmode & 1, op) {
            (0, 0) => (0..8).fold(0u64, |a, i| a | imm8 << (8 * i)),
            (0, _) => (0..8).fold(0u64, |a, i| if (imm8 >> i) & 1 != 0 { a | 0xff << (8 * i) } else { a }),
            (_, 0) => {
                let v = vfp_expand_imm(imm8, 32);
                (v << 32) | v
            }
            _ => vfp_expand_imm(imm8, 64),
        },
    }
}

// ---- top level: data processing

/// FP / AdvSIMD / crypto data processing (op0 = x111).
pub fn dp(w: u32) -> R {
    let (op0, op1, op2, op3) = (bits(w, 28, 4), bits(w, 23, 2), bits(w, 19, 4), bits(w, 10, 9));
    if op0 == 0b0100 && op1 & 2 == 0 && op2 & 7 == 5 && op3 & 0b110000011 == 0b10 {
        return crypto_aes(w);
    }
    if op0 == 0b0101 && op1 & 2 == 0 && op2 & 4 == 0 && op3 & 0b000100011 == 0 {
        return crypto_sha3(w);
    }
    if op0 == 0b0101 && op1 & 2 == 0 && op2 & 7 == 5 && op3 & 0b110000011 == 0b10 {
        return crypto_sha2(w);
    }
    if op0 & 0b1101 == 0b0101 {
        // AdvSIMD scalar
        if op1 == 0 && op2 & 0b1100 == 0 && op3 & 0b000100001 == 1 {
            return scalar_copy(w);
        }
        if op1 & 2 == 0 && op2 & 7 == 4 && op3 & 0b110000011 == 0b10 {
            return misc2(w, true);
        }
        if op1 & 2 == 0 && op2 & 7 == 6 && op3 & 0b110000011 == 0b10 {
            return scalar_pairwise(w);
        }
        if op1 & 2 == 0 && op2 & 4 != 0 && op3 & 3 == 0 {
            return three_diff(w, true);
        }
        if op1 & 2 == 0 && op2 & 4 != 0 && op3 & 1 != 0 {
            return three_same(w, true);
        }
        if op1 == 2 && op3 & 1 != 0 {
            return shift_imm(w, true);
        }
        if op1 & 2 != 0 && op3 & 1 == 0 {
            return indexed(w, true);
        }
        return un("simd scalar");
    }
    if op0 & 0b1011 == 0 && op1 & 2 == 0 && op2 & 4 == 0 && op3 & 0b000100011 == 0 {
        return tbl(w);
    }
    if op0 & 0b1011 == 0 && op1 & 2 == 0 && op2 & 4 == 0 && op3 & 0b000100011 == 2 {
        return permute(w);
    }
    if op0 & 0b1011 == 0b0010 && op1 & 2 == 0 && op2 & 4 == 0 && op3 & 0b000100001 == 0 {
        return ext(w);
    }
    if op0 & 0b1001 == 0 {
        // AdvSIMD vector
        if op1 == 0 && op2 & 0b1100 == 0 && op3 & 0b000100001 == 1 {
            return copy(w);
        }
        if op1 & 2 == 0 && op2 & 4 != 0 && op3 & 1 != 0 {
            return three_same(w, false);
        }
        if op1 & 2 == 0 && op2 & 4 != 0 && op3 & 3 == 0 {
            return three_diff(w, false);
        }
        if op1 & 2 == 0 && op2 & 7 == 4 && op3 & 0b110000011 == 0b10 {
            return misc2(w, false);
        }
        if op1 & 2 == 0 && op2 & 7 == 6 && op3 & 0b110000011 == 0b10 {
            return across(w);
        }
        if op1 == 2 && op2 == 0 && op3 & 1 != 0 {
            return modimm(w);
        }
        if op1 == 2 && op2 != 0 && op3 & 1 != 0 {
            return shift_imm(w, false);
        }
        if op1 & 2 != 0 && op3 & 1 == 0 {
            return indexed(w, false);
        }
        return un("simd vector");
    }
    if op0 & 0b0101 == 0b0001 {
        // scalar FP
        if bits(w, 29, 1) != 0 || (bits(w, 31, 1) != 0 && (op1 & 2 != 0 || (op2 & 4 != 0 && op3 & 0b111111 != 0))) {
            return un("fp M/S");
        }
        if op1 & 2 != 0 {
            return fp_dp3(w);
        }
        if op2 & 4 == 0 {
            return fp_fixed(w);
        }
        if op3 & 0b111111 == 0 {
            return fp_int(w);
        }
        if op3 & 0b11111 == 0b10000 {
            return fp_dp1(w);
        }
        if op3 & 0b1111 == 0b1000 {
            return fp_cmp(w);
        }
        if op3 & 0b111 == 0b100 {
            return fp_imm(w);
        }
        return match op3 & 3 {
            1 => fp_ccmp(w),
            2 => fp_dp2(w),
            3 => fp_csel(w),
            _ => un("fp op3"),
        };
    }
    un("fp/simd")
}

// ---- crypto

fn crypto_aes(w: u32) -> R {
    if bits(w, 22, 2) != 0 {
        return un("aes size");
    }
    let opc = bits(w, 12, 5);
    if !(4..=7).contains(&opc) {
        return un("aes opcode");
    }
    Ok(format!("sv_aes(c, {}, {}, {});", opc - 4, bits(w, 0, 5), bits(w, 5, 5)))
}

fn crypto_sha3(w: u32) -> R {
    if bits(w, 22, 2) != 0 {
        return un("sha size");
    }
    let (opc, d, n, m) = (bits(w, 12, 3), bits(w, 0, 5), bits(w, 5, 5), bits(w, 16, 5));
    Ok(match opc {
        0..=2 => format!("sv_sha1hash(c, {opc}, {d}, {n}, {m});"),
        3 => format!("sv_sha_misc(c, 0, {d}, {n}, {m});"),
        4 | 5 => format!("sv_sha256hash(c, {}, {d}, {n}, {m});", (opc == 4) as u32),
        6 => format!("sv_sha_misc(c, 3, {d}, {n}, {m});"),
        _ => return un("sha3 opcode"),
    })
}

fn crypto_sha2(w: u32) -> R {
    if bits(w, 22, 2) != 0 {
        return un("sha size");
    }
    let (opc, d, n) = (bits(w, 12, 5), bits(w, 0, 5), bits(w, 5, 5));
    let op = match opc {
        0 => 4,
        1 => 1,
        2 => 2,
        _ => return un("sha2 opcode"),
    };
    Ok(format!("sv_sha_misc(c, {op}, {d}, {n}, 0);"))
}

// ---- copy / permute / table

fn imm5(imm5: u32) -> Result<(u32, u32), Unsupported> {
    if imm5 & 15 == 0 {
        return un("copy imm5");
    }
    let sz = imm5.trailing_zeros();
    Ok((sz, imm5 >> (sz + 1)))
}

const LANE_PTR: [&str; 4] = ["uint8_t", "sa16", "sa32", "sa64"];

fn scalar_copy(w: u32) -> R {
    if bits(w, 29, 1) != 0 || bits(w, 11, 4) != 0 {
        return un("scalar copy");
    }
    let (sz, idx) = imm5(bits(w, 16, 5))?;
    let (d, n) = (bits(w, 0, 5), bits(w, 5, 5));
    Ok(format!("c->v[{d}] = (({} *)&c->v[{n}])[{idx}];", LANE_PTR[sz as usize]))
}

fn copy(w: u32) -> R {
    let (q, op, im5, imm4) = (bits(w, 30, 1), bits(w, 29, 1), bits(w, 16, 5), bits(w, 11, 4));
    let (d, n) = (bits(w, 0, 5), bits(w, 5, 5));
    let (sz, idx) = imm5(im5)?;
    let p = LANE_PTR[sz as usize];
    if op != 0 {
        if q == 0 {
            return un("ins elem Q");
        }
        let idx2 = imm4 >> sz;
        return Ok(format!("(({p} *)&c->v[{d}])[{idx}] = (({p} *)&c->v[{n}])[{idx2}];"));
    }
    match imm4 {
        0 => {
            // DUP (element)
            if sz == 3 && q == 0 {
                return un("dup 1d");
            }
            let t = vu(sz);
            Ok(vw(t, q, d, &shuf(&vr(t, n), &vr(t, n), &vec![idx; (16 >> sz) as usize])))
        }
        1 => {
            // DUP (general)
            if sz == 3 && q == 0 {
                return un("dup 1d");
            }
            Ok(vw(vu(sz), q, d, &format!("({}){{}} + ({}){}", vu(sz), ut(sz), xreg(n))))
        }
        3 => {
            // INS (general)
            if q == 0 {
                return un("ins Q");
            }
            Ok(format!("(({p} *)&c->v[{d}])[{idx}] = ({}){};", ut(sz), xreg(n)))
        }
        5 => {
            // SMOV
            if sz >= 2 + q {
                return un("smov size");
            }
            let st = ["int8_t", "int16_t", "int32_t"][sz as usize];
            Ok(set_reg(q == 1, d, &format!("(uint64_t)(int64_t)({st})(({p} *)&c->v[{n}])[{idx}]"), false))
        }
        7 => {
            // UMOV
            if (q == 1 && sz != 3) || (q == 0 && sz == 3) {
                return un("umov size");
            }
            Ok(set_reg(q == 1, d, &format!("(({p} *)&c->v[{n}])[{idx}]"), false))
        }
        _ => un("copy imm4"),
    }
}

fn permute(w: u32) -> R {
    let (q, sz, m, opc, n, d) = (bits(w, 30, 1), bits(w, 22, 2), bits(w, 16, 5), bits(w, 12, 3), bits(w, 5, 5), bits(w, 0, 5));
    if opc == 0 || opc == 4 || (sz == 3 && q == 0) {
        return un("permute");
    }
    let (ne, nl) = (lanes(q, sz), 16 >> sz);
    let (part, kind) = (opc >> 2, opc & 3);
    let mut idx = Vec::new();
    for i in 0..ne {
        idx.push(match kind {
            1 => {
                // UZP
                let j = 2 * i + part;
                if j < ne {
                    j
                } else {
                    nl + j - ne
                }
            }
            2 => {
                // TRN
                let p = i & !1;
                if i & 1 == 0 {
                    p + part
                } else {
                    nl + p + part
                }
            }
            _ => {
                // ZIP
                let base = part * ne / 2;
                if i & 1 == 0 {
                    base + i / 2
                } else {
                    nl + base + i / 2
                }
            }
        });
    }
    idx.extend(std::iter::repeat(0).take((nl - ne) as usize));
    let t = vu(sz);
    Ok(vw(t, q, d, &shuf(&vr(t, n), &vr(t, m), &idx)))
}

fn ext(w: u32) -> R {
    let (q, m, imm4, n, d) = (bits(w, 30, 1), bits(w, 16, 5), bits(w, 11, 4), bits(w, 5, 5), bits(w, 0, 5));
    if bits(w, 22, 2) != 0 || (q == 0 && imm4 >= 8) {
        return un("ext");
    }
    let idx: Vec<u32> = if q != 0 {
        (0..16).map(|i| imm4 + i).collect()
    } else {
        (0..8).map(|i| if imm4 + i < 8 { imm4 + i } else { 16 + imm4 + i - 8 }).chain(std::iter::repeat(0).take(8)).collect()
    };
    Ok(vw("vu8", q, d, &shuf(&vr("vu8", n), &vr("vu8", m), &idx)))
}

fn tbl(w: u32) -> R {
    let (q, m, ln, op, n, d) = (bits(w, 30, 1), bits(w, 16, 5), bits(w, 13, 2), bits(w, 12, 1), bits(w, 5, 5), bits(w, 0, 5));
    if bits(w, 22, 2) != 0 {
        return un("tbl op2");
    }
    Ok(format!("sv_tbl(c, {q}, {ln}, {op}, {d}, {n}, {m});"))
}

// ---- three same

fn pair_idx(q: u32, sz: u32) -> (Vec<u32>, Vec<u32>) {
    // lane indices (into SHUF(a, b)) of the left / right operands of a pairwise op; for Q=0 the padding lanes
    // point at a's upper half
    let (ne, nl) = (lanes(q, sz), 16 >> sz);
    let src: Vec<u32> = (0..ne).chain((0..ne).map(|i| nl + i)).collect();
    let pad = ne % nl;
    let ev = (0..ne).map(|k| src[2 * k as usize]).chain(std::iter::repeat(pad).take((nl - ne) as usize)).collect();
    let od = (0..ne).map(|k| src[2 * k as usize + 1]).chain(std::iter::repeat(pad).take((nl - ne) as usize)).collect();
    (ev, od)
}

fn three_same(w: u32, scalar: bool) -> R {
    let (q, u, sz, m, opc, n, d) =
        (bits(w, 30, 1), bits(w, 29, 1), bits(w, 22, 2), bits(w, 16, 5), bits(w, 11, 5), bits(w, 5, 5), bits(w, 0, 5));
    if opc >= 0b11000 {
        return three_same_fp(w, scalar);
    }
    if scalar {
        return three_same_scalar(u, sz, opc, d, n, m);
    }
    if sz == 3 && q == 0 && opc != 3 {
        return un("3same 1d");
    }
    let ne = lanes(q, sz);
    let (t, s) = (vu(sz), vs(sz));
    let (a, b, o) = (vr(t, n), vr(t, m), vr(t, d));
    let (sa, sb) = (vr(s, n), vr(s, m));
    let (x, y) = if u != 0 { (&a, &b) } else { (&sa, &sb) };
    let mi = m as i32;
    if matches!(opc, 0 | 2 | 4 | 14 | 15 | 12 | 13) && sz == 3 {
        return un("3same size");
    }
    Ok(match opc {
        0 => same(&lop("HADD", u), sz, ne, d, n, mi, 0, -1),
        1 => vw(t, q, d, &format!("({t})__builtin_elementwise_add_sat({x}, {y})")),
        2 => same(&lop("RHADD", u), sz, ne, d, n, mi, 0, -1),
        3 => {
            let (a64, b64, o64) = (vr("vu64", n), vr("vu64", m), vr("vu64", d));
            let e = match (u, sz) {
                (0, 0) => format!("{a64} & {b64}"),
                (0, 1) => format!("{a64} & ~{b64}"),
                (0, 2) => format!("{a64} | {b64}"),
                (0, _) => format!("{a64} | ~{b64}"),
                (_, 0) => format!("{a64} ^ {b64}"),
                (_, 1) => format!("({o64} & {a64}) | (~{o64} & {b64})"),
                (_, 2) => format!("({o64} & ~{b64}) | ({a64} & {b64})"),
                _ => format!("({o64} & {b64}) | ({a64} & ~{b64})"),
            };
            vw("vu64", q, d, &e)
        }
        4 => same(&lop("HSUB", u), sz, ne, d, n, mi, 0, -1),
        5 => vw(t, q, d, &format!("({t})__builtin_elementwise_sub_sat({x}, {y})")),
        6 => vw(t, q, d, &format!("({t})({x} > {y})")),
        7 => vw(t, q, d, &format!("({t})({x} >= {y})")),
        8..=11 => same(&lop(["SHL", "QSHL", "RSHL", "QRSHL"][(opc - 8) as usize], u), sz, ne, d, n, mi, 0, -1),
        12 => vw(t, q, d, &format!("({t})__builtin_elementwise_max({x}, {y})")),
        13 => vw(t, q, d, &format!("({t})__builtin_elementwise_min({x}, {y})")),
        14 | 15 => {
            let e = format!("({t})__builtin_elementwise_max({x}, {y}) - ({t})__builtin_elementwise_min({x}, {y})");
            vw(t, q, d, &if opc == 15 { format!("{o} + ({e})") } else { e })
        }
        16 => vw(t, q, d, &format!("{a} {} {b}", if u != 0 { "-" } else { "+" })),
        17 => vw(t, q, d, &if u != 0 { format!("({t})({a} == {b})") } else { format!("({t})(({a} & {b}) != 0)") }),
        _ => {
            if sz == 3 && opc != 23 {
                return un("3same size");
            }
            match opc {
                18 => vw(t, q, d, &format!("{o} {} {a} * {b}", if u != 0 { "-" } else { "+" })),
                19 => {
                    if u != 0 {
                        if sz != 0 {
                            return un("pmul size");
                        }
                        same(&lop("PMUL", 0), 0, ne, d, n, mi, 0, -1)
                    } else {
                        vw(t, q, d, &format!("{a} * {b}"))
                    }
                }
                20 | 21 | 23 => {
                    if opc == 23 && u != 0 {
                        return un("addp U");
                    }
                    let (ev, od) = pair_idx(q, sz);
                    let (aa, bb) = if u != 0 || opc == 23 { (&a, &b) } else { (&sa, &sb) };
                    let (l, r) = (shuf(aa, bb, &ev), shuf(aa, bb, &od));
                    if opc == 23 {
                        vw(t, q, d, &format!("({t})({l} + {r})"))
                    } else {
                        let f = if opc == 20 { "max" } else { "min" };
                        vw(t, q, d, &format!("({t})__builtin_elementwise_{f}({l}, {r})"))
                    }
                }
                22 => {
                    if !matches!(sz, 1 | 2) {
                        return un("sqdmulh size");
                    }
                    same(&lop(if u != 0 { "SQRDMULH" } else { "SQDMULH" }, 0), sz, ne, d, n, mi, 0, -1)
                }
                _ => return un("3same opcode"),
            }
        }
    })
}

fn three_same_scalar(u: u32, sz: u32, opc: u32, d: u32, n: u32, m: u32) -> R {
    let mi = m as i32;
    match opc {
        1 | 5 => {
            let op = match (opc, u) {
                (1, 0) => "SQADD",
                (1, _) => "UQADD",
                (_, 0) => "SQSUB",
                _ => "UQSUB",
            };
            return Ok(same(&lop(op, 0), sz, 1, d, n, mi, 0, -1));
        }
        9 | 11 => return Ok(same(&lop(if opc == 9 { "QSHL" } else { "QRSHL" }, u), sz, 1, d, n, mi, 0, -1)),
        22 => {
            if !matches!(sz, 1 | 2) {
                return un("sqdmulh size");
            }
            return Ok(same(&lop(if u != 0 { "SQRDMULH" } else { "SQDMULH" }, 0), sz, 1, d, n, mi, 0, -1));
        }
        _ => {}
    }
    if sz != 3 {
        return un("scalar 3same size");
    }
    let (a, b) = (format!("VS64({n})"), format!("VS64({m})"));
    let (sa, sb) = (format!("(int64_t)VS64({n})"), format!("(int64_t)VS64({m})"));
    let (x, y) = if u != 0 { (&a, &b) } else { (&sa, &sb) };
    Ok(match opc {
        6 => format!("c->v[{d}] = ({x} > {y}) ? ~0ull : 0;"),
        7 => format!("c->v[{d}] = ({x} >= {y}) ? ~0ull : 0;"),
        8 | 10 => same(&lop(if opc == 8 { "SHL" } else { "RSHL" }, u), 3, 1, d, n, mi, 0, -1),
        16 => format!("c->v[{d}] = (uint64_t)({a} {} {b});", if u != 0 { "-" } else { "+" }),
        17 if u != 0 => format!("c->v[{d}] = ({a} == {b}) ? ~0ull : 0;"),
        17 => format!("c->v[{d}] = ({a} & {b}) ? ~0ull : 0;"),
        _ => return un("scalar 3same opcode"),
    })
}

fn three_same_fp(w: u32, scalar: bool) -> R {
    let (q, u, a_, sz, m, opc, n, d) = (
        bits(w, 30, 1),
        bits(w, 29, 1),
        bits(w, 23, 1),
        bits(w, 22, 1),
        bits(w, 16, 5),
        bits(w, 11, 5),
        bits(w, 5, 5),
        bits(w, 0, 5),
    );
    if !scalar && sz != 0 && q == 0 {
        return un("fp 2d Q");
    }
    let ft = &FT[sz as usize];
    let (f, vt, vuu, sf, set) = (ft.f, ft.vt, ft.vu, ft.sf, ft.set);
    let key = (u, a_, opc);
    if let Some(cop) = fcmp3(key) {
        if scalar {
            return Ok(format!(
                "c->v[{d}] = ({})vcmp_{f}(c, {cop}, ({vt}){{{sf}({n})}}, ({vt}){{{sf}({m})}})[0];",
                ut(2 + sz)
            ));
        }
        return Ok(vw(vuu, q, d, &format!("vcmp_{f}(c, {cop}, {}, {})", vq(vt, q, n), vq(vt, q, m))));
    }
    if scalar {
        if !matches!(key, (0, 0, 0b11011) | (0, 0, 0b11111) | (0, 1, 0b11111) | (1, 1, 0b11010)) {
            return un("scalar fp 3same");
        }
        let fop = fop3(key).unwrap();
        let xx = if matches!(fop, "FOP_RECPS" | "FOP_RSQRTS") { format!("neg_{f}({sf}({n}))") } else { format!("{sf}({n})") };
        return Ok(format!("{set}({d}, op_{f}(c, {fop}, {xx}, {sf}({m}), 0));"));
    }
    let (mut a, b) = (vq(vt, q, n), vq(vt, q, m));
    let fu = f.to_uppercase();
    if let Some(fop) = fpairv(key) {
        let (ev, od) = pair_idx(q, 2 + sz);
        return Ok(format!("SV_FOP_{fu}({fop}, {q}, {d}, {}, {}, ({vt}){{}});", shuf(&a, &b, &ev), shuf(&a, &b, &od)));
    }
    if u == 0 && opc == 0b11001 {
        // FMLA / FMLS: the multiplicand is negated for FMLS
        let x = if a_ != 0 { format!("vneg_{f}({a})") } else { a.clone() };
        return Ok(format!("SV_FOP_{fu}(FOP_MADD, {q}, {d}, {}, {x}, {b});", vq(vt, q, d)));
    }
    let Some(fop) = fop3(key) else { return un("fp 3same opcode") };
    if matches!(fop, "FOP_RECPS" | "FOP_RSQRTS") {
        a = format!("vneg_{f}({a})");
    }
    Ok(format!("SV_FOP_{fu}({fop}, {q}, {d}, {a}, {b}, ({vt}){{}});"))
}

// ---- three different

fn three_diff(w: u32, scalar: bool) -> R {
    let (q, u, sz, m, opc, n, d) =
        (bits(w, 30, 1), bits(w, 29, 1), bits(w, 22, 2), bits(w, 16, 5), bits(w, 12, 4), bits(w, 5, 5), bits(w, 0, 5));
    let mi = m as i32;
    if scalar {
        if u != 0 || !matches!(opc, 9 | 11 | 13) || !matches!(sz, 1 | 2) {
            return un("scalar 3diff");
        }
        let op = match opc {
            9 => "SQDMLAL",
            11 => "SQDMLSL",
            _ => "SQDMULL",
        };
        return Ok(gen(&lop(op, 0), sz, 1, d, sz + 1, 0, 0, n, sz, 0, mi, sz, 0, -1, 0));
    }
    if opc == 14 {
        // PMULL
        if u != 0 {
            return un("pmull U");
        }
        if sz == 3 {
            return Ok(format!("sl_pmull64(c, {q}, {d}, {n}, {m});"));
        }
        if sz != 0 {
            return un("pmull size");
        }
    }
    if sz == 3 || opc == 15 {
        return un("3diff size");
    }
    let ne = 8 >> sz;
    let off = q * ne;
    if matches!(opc, 4 | 6) {
        // ADDHN / RADDHN / SUBHN / RSUBHN: narrow into the low or high half
        let op = ["ADDHN", "RADDHN", "SUBHN", "RSUBHN"][((opc == 6) as usize) * 2 + u as usize];
        return Ok(gen(&lop(op, 0), sz, ne, d, sz, off, q, n, sz + 1, 0, mi, sz + 1, 0, -1, 0));
    }
    if matches!(opc, 1 | 3) {
        let op = if opc == 1 { "ADDW" } else { "SUBW" };
        return Ok(gen(&lop(op, u), sz, ne, d, sz + 1, 0, 0, n, sz + 1, 0, mi, sz, off, -1, 0));
    }
    let op = match opc {
        0 => "ADDL",
        2 => "SUBL",
        5 => "ABAL",
        7 => "ABDL",
        8 => "MLAL",
        10 => "MLSL",
        12 => "MULL",
        9 => "SQDMLAL",
        11 => "SQDMLSL",
        13 => "SQDMULL",
        _ => "PMULL",
    };
    if op.starts_with("SQD") && (u != 0 || !matches!(sz, 1 | 2)) {
        return un("sqdmull");
    }
    let uu = if op.starts_with("SQD") || op.starts_with("PM") { 0 } else { u };
    Ok(gen(&lop(op, uu), sz, ne, d, sz + 1, 0, 0, n, sz, off, mi, sz, off, -1, 0))
}

// ---- two-register misc

fn misc2(w: u32, scalar: bool) -> R {
    let (q, u, sz, opc, n, d) = (bits(w, 30, 1), bits(w, 29, 1), bits(w, 22, 2), bits(w, 12, 5), bits(w, 5, 5), bits(w, 0, 5));
    if opc >= 0b11000 || matches!(opc, 0b10110 | 0b10111) || (0b01100..=0b01111).contains(&opc) {
        return misc2_fp(w, scalar);
    }
    if scalar {
        return misc2_scalar(u, sz, opc, d, n);
    }
    let ne = lanes(q, sz);
    let (t, s) = (vu(sz), vs(sz));
    if opc == 0 || (opc == 1 && u == 0) {
        // REV64 / REV32 / REV16
        let gsz = match (u, opc) {
            (0, 0) => 3,
            (1, 0) => 2,
            (0, 1) => 1,
            _ => return un("rev"),
        };
        if sz >= gsz {
            return un("rev");
        }
        let (es, gb) = (1u32 << sz, 1u32 << gsz);
        let idx: Vec<u32> = (0..16)
            .map(|i| {
                let (g, o) = (i / gb * gb, i % gb);
                let (e, b) = (o / es, o % es);
                g + (gb / es - 1 - e) * es + b
            })
            .collect();
        return Ok(vw("vu8", q, d, &shuf(&vr("vu8", n), &vr("vu8", n), &idx)));
    }
    if sz == 3 && q == 0 && !matches!(opc, 0b10010 | 0b10100 | 0b10011) {
        return un("misc 1d");
    }
    match opc {
        2 | 6 => {
            if sz == 3 {
                return un("addlp size");
            }
            return Ok(format!("sl_addlp(c, {u}, {}, {q}, {sz}, {d}, {n});", (opc == 6) as u32));
        }
        3 => return Ok(same(&lop(if u != 0 { "USQADD" } else { "SUQADD" }, 0), sz, ne, d, n, -1, 0, -1)),
        4 => {
            if sz == 3 {
                return un("clz size");
            }
            return Ok(same(&lop(if u != 0 { "CLZ" } else { "CLS" }, 0), sz, ne, d, n, -1, 0, -1));
        }
        5 => {
            if u == 0 {
                if sz != 0 {
                    return un("cnt size");
                }
                return Ok(same(&lop("CNT", 0), 0, ne, d, n, -1, 0, -1));
            }
            return match sz {
                0 => Ok(vw("vu64", q, d, &format!("~{}", vr("vu64", n)))),
                1 => Ok(same(&lop("RBIT", 0), 0, lanes(q, 0), d, n, -1, 0, -1)),
                _ => un("not/rbit size"),
            };
        }
        7 => return Ok(same(&lop(if u != 0 { "SQNEG" } else { "SQABS" }, 0), sz, ne, d, n, -1, 0, -1)),
        _ => {}
    }
    let (a, sa) = (vr(t, n), vr(s, n));
    let sh = (8 << sz) - 1;
    Ok(match (opc, u) {
        (8, _) => vw(t, q, d, &format!("({t})({sa} {} 0)", if u != 0 { ">=" } else { ">" })),
        (9, _) => vw(t, q, d, &format!("({t})({sa} {} 0)", if u != 0 { "<=" } else { "==" })),
        (10, 0) => vw(t, q, d, &format!("({t})({sa} < 0)")),
        (11, 1) => vw(t, q, d, &format!("-{a}")),
        (11, _) => vw(t, q, d, &format!("(({t}){sa} ^ ({t})({sa} >> {sh})) - ({t})({sa} >> {sh})")),
        (0b10010, _) | (0b10100, _) => {
            // XTN / SQXTUN / SQXTN / UQXTN
            if sz == 3 {
                return un("xtn size");
            }
            let op = match (opc, u) {
                (0b10010, 0) => "XTN",
                (0b10010, _) => "SQXTUN",
                (_, 0) => "SQXTN",
                _ => "UQXTN",
            };
            let ne = 8 >> sz;
            gen(&lop(op, 0), sz, ne, d, sz, q * ne, q, n, sz + 1, 0, -1, 0, 0, -1, 0)
        }
        (0b10011, 1) => {
            // SHLL
            if sz == 3 {
                return un("shll size");
            }
            let ne = 8 >> sz;
            gen(&lop("SHLL", 0), sz, ne, d, sz + 1, 0, 0, n, sz, q * ne, -1, 0, 0, -1, 0)
        }
        _ => return un("misc2 opcode"),
    })
}

fn misc2_scalar(u: u32, sz: u32, opc: u32, d: u32, n: u32) -> R {
    match opc {
        3 => return Ok(same(&lop(if u != 0 { "USQADD" } else { "SUQADD" }, 0), sz, 1, d, n, -1, 0, -1)),
        7 => return Ok(same(&lop(if u != 0 { "SQNEG" } else { "SQABS" }, 0), sz, 1, d, n, -1, 0, -1)),
        0b10010 | 0b10100 => {
            if sz == 3 || (opc == 0b10010 && u == 0) {
                return un("scalar xtn");
            }
            let op = match (opc, u) {
                (0b10010, _) => "SQXTUN",
                (_, 0) => "SQXTN",
                _ => "UQXTN",
            };
            return Ok(gen(&lop(op, 0), sz, 1, d, sz, 0, 0, n, sz + 1, 0, -1, 0, 0, -1, 0));
        }
        _ => {}
    }
    if sz != 3 {
        return un("scalar misc size");
    }
    let (a, sa) = (format!("VS64({n})"), format!("(int64_t)VS64({n})"));
    Ok(match (opc, u) {
        (8, _) => format!("c->v[{d}] = ({sa} {} 0) ? ~0ull : 0;", if u != 0 { ">=" } else { ">" }),
        (9, _) => format!("c->v[{d}] = ({sa} {} 0) ? ~0ull : 0;", if u != 0 { "<=" } else { "==" }),
        (10, 0) => format!("c->v[{d}] = ({sa} < 0) ? ~0ull : 0;"),
        (11, 1) => format!("c->v[{d}] = (uint64_t)-{a};"),
        (11, _) => format!("c->v[{d}] = ({sa} < 0) ? (uint64_t)-{a} : {a};"),
        _ => return un("scalar misc opcode"),
    })
}

fn misc2_fp(w: u32, scalar: bool) -> R {
    let (q, u, a_, sz, opc, n, d) =
        (bits(w, 30, 1), bits(w, 29, 1), bits(w, 23, 1), bits(w, 22, 1), bits(w, 12, 5), bits(w, 5, 5), bits(w, 0, 5));
    let ft = &FT[sz as usize];
    let (f, vt, vuu, sf, su, set) = (ft.f, ft.vt, ft.vu, ft.sf, ft.su, ft.set);
    if matches!(opc, 0b10110 | 0b10111) {
        // FCVTN / FCVTL / FCVTXN
        if a_ != 0 {
            return un("fcvtn a");
        }
        if opc == 0b10110 && u != 0 {
            if sz == 0 {
                return un("fcvtxn size");
            }
            if scalar {
                return Ok(format!("FP_SETS({d}, simd_d2s_odd(c, VSF64({n})));"));
            }
            return Ok(format!("sv_fcvtn(c, 1, {q}, 1, {d}, {n});"));
        }
        if scalar || u != 0 {
            return un("fcvtn/l");
        }
        if opc == 0b10110 {
            return Ok(format!("sv_fcvtn(c, {sz}, {q}, 0, {d}, {n});"));
        }
        return Ok(format!("sv_fcvtl(c, {sz}, {q}, {d}, {n});"));
    }
    if !scalar && sz != 0 && q == 0 {
        return un("fp misc 2d Q");
    }
    let xv = vq(vt, q, n);
    let fu = f.to_uppercase();
    if (0b01100..=0b01111).contains(&opc) {
        if a_ == 0 {
            return un("fp16 misc");
        }
        if opc == 0b01111 {
            // FABS / FNEG
            if scalar {
                return un("scalar fabs");
            }
            let mask = if sz == 0 { "0x80000000u" } else { "0x8000000000000000ull" };
            let e = if u != 0 { format!("{} ^ {mask}", vr(vuu, n)) } else { format!("{} & ~{mask}", vr(vuu, n)) };
            return Ok(vw(vuu, q, d, &e));
        }
        let (op, swap) = match (opc, u) {
            (0b01100, 0) => (2, false),
            (0b01100, 1) => (1, false),
            (0b01101, 0) => (0, false),
            (0b01101, 1) => (1, true),
            (0b01110, 0) => (2, true),
            _ => return un("fcm zero"),
        };
        if scalar {
            let (mut a, mut b) = (format!("({vt}){{{sf}({n})}}"), format!("({vt}){{}}"));
            if swap {
                std::mem::swap(&mut a, &mut b);
            }
            return Ok(format!("c->v[{d}] = ({})vcmp_{f}(c, {op}, {a}, {b})[0];", ut(2 + sz)));
        }
        let (mut a, mut b) = (xv.clone(), format!("({vt}){{}}"));
        if swap {
            std::mem::swap(&mut a, &mut b);
        }
        return Ok(vw(vuu, q, d, &format!("vcmp_{f}(c, {op}, {a}, {b})")));
    }
    match opc {
        0b11111 => {
            if a_ == 0 {
                return un("fp misc 11111");
            }
            if u != 0 {
                if scalar {
                    return un("scalar fsqrt");
                }
                return Ok(format!("SV_FOP_{fu}(FOP_SQRT, {q}, {d}, {xv}, {xv}, ({vt}){{}});"));
            }
            if !scalar {
                return un("vector frecpx");
            }
            Ok(format!("{set}({d}, recpx_{f}(c, {sf}({n})));"))
        }
        0b11000 | 0b11001 => {
            // FRINT*
            if scalar {
                return un("scalar frint");
            }
            let rm = match (u, a_, opc & 1) {
                (0, 0, 0) => 0,
                (0, 0, 1) => 2,
                (0, 1, 0) => 1,
                (0, 1, 1) => 3,
                (1, 0, 0) => 4,
                (1, 0, 1) => 5,
                (1, 1, 1) => 6,
                _ => return un("frint"),
            };
            Ok(vw(vt, q, d, &format!("vfrint_{f}(c, {xv}, {rm})")))
        }
        0b11010 | 0b11011 | 0b11100 if !(opc == 0b11100 && a_ != 0) => {
            // FCVT[NPMZA][SU] (0b11100 with a = 1 is URECPE / URSQRTE, below)
            let rm = match (opc, a_) {
                (0b11010, 0) => 0,
                (0b11010, _) => 1,
                (0b11011, 0) => 2,
                (0b11011, _) => 3,
                _ => 4,
            };
            if scalar {
                return Ok(format!("c->v[{d}] = ({})toint_{f}(c, {sf}({n}), {rm}, 0, {}, {u});", ut(2 + sz), 32 << sz));
            }
            Ok(vw(vuu, q, d, &format!("vtoint_{f}(c, {xv}, {rm}, 0, {u})")))
        }
        0b11100 => {
            // URECPE / URSQRTE
            if scalar || sz != 0 {
                return un("urecpe");
            }
            Ok(same(&lop(if u != 0 { "URSQRTE" } else { "URECPE" }, 0), 2, lanes(q, 2), d, n, -1, 0, -1))
        }
        0b11101 => {
            if a_ == 0 {
                // SCVTF / UCVTF
                if scalar {
                    return Ok(format!("{set}({d}, fromint_{f}(c, {su}({n}), {}, {u}, 0));", 32 << sz));
                }
                return Ok(vw(vt, q, d, &format!("vfromint_{f}(c, {}, 0, {u})", vr(vuu, n))));
            }
            let est = if u != 0 { "rsqrte" } else { "recpe" };
            if scalar {
                // FRECPE / FRSQRTE
                return Ok(format!("{set}({d}, {est}_{f}(c, {sf}({n})));"));
            }
            Ok(vw(vt, q, d, &format!("v{est}_{f}(c, {xv})")))
        }
        _ => un("fp misc opcode"),
    }
}

// ---- across lanes / pairwise

fn across(w: u32) -> R {
    let (q, u, sz, opc, n, d) = (bits(w, 30, 1), bits(w, 29, 1), bits(w, 22, 2), bits(w, 12, 5), bits(w, 5, 5), bits(w, 0, 5));
    if matches!(opc, 0b01100 | 0b01111) {
        if u == 0 {
            return un("fp16 across");
        }
        if q == 0 || sz & 1 != 0 {
            return un("fp across size");
        }
        let fop = match (opc, sz >> 1) {
            (0b01100, 0) => "FOP_MAXNM",
            (0b01100, _) => "FOP_MINNM",
            (_, 0) => "FOP_MAX",
            _ => "FOP_MIN",
        };
        return Ok(format!("sv_fred_s(c, {fop}, {d}, {n});"));
    }
    let op = match opc {
        0b00011 => 1,
        0b01010 => 2,
        0b11010 => 3,
        0b11011 => 0,
        _ => return un("across"),
    };
    if (op == 0 && u != 0) || sz == 3 || (sz == 2 && q == 0) {
        return un("across");
    }
    Ok(format!("sl_across(c, {op}, {u}, {q}, {sz}, {d}, {n});"))
}

fn scalar_pairwise(w: u32) -> R {
    let (u, sz, opc, n, d) = (bits(w, 29, 1), bits(w, 22, 2), bits(w, 12, 5), bits(w, 5, 5), bits(w, 0, 5));
    if u == 0 {
        if opc == 0b11011 && sz == 3 {
            return Ok(format!("c->v[{d}] = (uint64_t)(VS64({n}) + VHI64({n}));"));
        }
        return un("scalar pairwise");
    }
    let fop = match (opc, sz >> 1) {
        (0b01100, 0) => "FOP_MAXNM",
        (0b01100, 1) => "FOP_MINNM",
        (0b01101, 0) => "FOP_ADD",
        (0b01111, 0) => "FOP_MAX",
        (0b01111, 1) => "FOP_MIN",
        _ => return un("scalar fp pairwise"),
    };
    let ft = &FT[(sz & 1) as usize];
    let v = vr(ft.vt, n);
    Ok(format!("{}({d}, pair_{}(c, {fop}, {v}[0], {v}[1]));", ft.set, ft.f))
}

// ---- modified immediate

fn modimm(w: u32) -> R {
    let (q, op, cmode, o2, d) = (bits(w, 30, 1), bits(w, 29, 1), bits(w, 12, 4), bits(w, 11, 1), bits(w, 0, 5));
    let imm8 = ((bits(w, 16, 3) << 5) | bits(w, 5, 5)) as u64;
    if o2 != 0 {
        return un("fmov half imm");
    }
    if cmode == 15 && op != 0 && q == 0 {
        return un("fmov 1d");
    }
    let mut imm = simd_expand_imm(op, cmode, imm8);
    if cmode < 12 && cmode & 1 != 0 {
        // ORR / BIC
        if op != 0 {
            return Ok(vw("vu64", q, d, &format!("{} & {}", vr("vu64", d), hx(!imm))));
        }
        return Ok(vw("vu64", q, d, &format!("{} | {}", vr("vu64", d), hx(imm))));
    }
    if op != 0 && !matches!(cmode, 14 | 15) {
        imm = !imm; // MVNI
    }
    let v = if q != 0 { imm as u128 | ((imm as u128) << 64) } else { imm as u128 };
    Ok(format!("c->v[{d}] = {};", u128_lit(v)))
}

// ---- shift by immediate

fn shift_imm(w: u32, scalar: bool) -> R {
    let (mut q, u, immh, immb, opc, n, d) =
        (bits(w, 30, 1), bits(w, 29, 1), bits(w, 19, 4), bits(w, 16, 3), bits(w, 11, 5), bits(w, 5, 5), bits(w, 0, 5));
    if immh == 0 {
        return un("shift immh");
    }
    let sz = 31 - immh.leading_zeros();
    let es = 8u32 << sz;
    let ih = (immh << 3) | immb;
    let (rsh, lsh) = (2 * es - ih, ih.wrapping_sub(es));
    if scalar {
        q = 1;
    }
    let ne = if scalar { 1 } else { lanes(q, sz) };
    if matches!(opc, 0b11100 | 0b11111) {
        // fixed-point conversions
        if sz < 2 {
            return un("fp16 fixed");
        }
        if sz == 3 && q == 0 {
            return un("fixed 1d");
        }
        let ft = &FT[(sz - 2) as usize];
        let (f, vt, vuu, sf, su, set) = (ft.f, ft.vt, ft.vu, ft.sf, ft.su, ft.set);
        let fb = rsh;
        if opc == 0b11100 {
            if scalar {
                return Ok(format!("{set}({d}, fromint_{f}(c, {su}({n}), {es}, {u}, {fb}));"));
            }
            return Ok(vw(vt, q, d, &format!("vfromint_{f}(c, {}, {fb}, {u})", vr(vuu, n))));
        }
        if scalar {
            return Ok(format!("c->v[{d}] = ({})toint_{f}(c, {sf}({n}), 3, {fb}, {es}, {u});", ut(sz)));
        }
        return Ok(vw(vuu, q, d, &format!("vtoint_{f}(c, {}, 3, {fb}, {u})", vq(vt, q, n))));
    }
    if (0b10000..=0b10011).contains(&opc) {
        // narrowing
        if sz == 3 {
            return un("narrow shift size");
        }
        let ne = if scalar { 1 } else { 8 >> sz };
        let (name, uu) = match (opc, u) {
            (0b10000, 0) => ("SHRN", 0),
            (0b10001, 0) => ("RSHRN", 0),
            (0b10000, _) => ("QSHRUN", 0),
            (0b10001, _) => ("QRSHRUN", 0),
            (0b10010, uu) => ("QSHRN", uu),
            (_, uu) => ("QRSHRN", uu),
        };
        if scalar && matches!(name, "SHRN" | "RSHRN") {
            return un("scalar shrn");
        }
        let part = if scalar { 0 } else { q };
        return Ok(gen(&lop(name, uu), sz, ne, d, sz, part * ne, part, n, sz + 1, 0, -1, 0, 0, -1, rsh as u64));
    }
    if opc == 0b10100 {
        // SSHLL / USHLL
        if scalar || sz == 3 {
            return un("sshll");
        }
        let ne = 8 >> sz;
        return Ok(gen(&lop("SHLLI", u), sz, ne, d, sz + 1, 0, 0, n, sz, q * ne, -1, 0, 0, -1, lsh as u64));
    }
    if sz == 3 && q == 0 {
        return un("shift 1d");
    }
    if scalar && sz != 3 && !matches!(opc, 0b01100 | 0b01110) {
        return un("scalar shift size");
    }
    let (t, s) = (vu(sz), vs(sz));
    let out = |e: String| if scalar { format!("c->v[{d}] = (uint64_t)(({e})[0]);") } else { vw(t, q, d, &e) };
    Ok(match opc {
        0 | 2 => {
            // SSHR / USHR, SSRA / USRA
            let mut e = if u != 0 {
                if rsh >= es {
                    format!("({t}){{}}")
                } else {
                    format!("({} >> {rsh})", vr(t, n))
                }
            } else {
                format!("({t})({} >> {})", vr(s, n), rsh.min(es - 1))
            };
            if opc == 2 {
                e = format!("{} + {e}", vr(t, d));
            }
            out(e)
        }
        4 | 6 => same(&lop(if opc == 4 { "RSHRI" } else { "RSRAI" }, u), sz, ne, d, n, -1, rsh as u64, -1),
        8 => {
            if u == 0 {
                return un("sri U");
            }
            same(&lop("SRI", 0), sz, ne, d, n, -1, rsh as u64, -1)
        }
        10 => {
            if u != 0 {
                same(&lop("SLI", 0), sz, ne, d, n, -1, lsh as u64, -1)
            } else {
                out(format!("{} << {lsh}", vr(t, n)))
            }
        }
        12 => {
            if u == 0 {
                return un("sqshlu U");
            }
            same(&lop("QSHLUI", 0), sz, ne, d, n, -1, lsh as u64, -1)
        }
        14 => same(&lop("QSHLI", u), sz, ne, d, n, -1, lsh as u64, -1),
        _ => return un("shift imm opcode"),
    })
}

// ---- indexed element

fn indexed(w: u32, scalar: bool) -> R {
    let (q, u, sz, l, mb, rm, opc, h, n, d) = (
        bits(w, 30, 1),
        bits(w, 29, 1),
        bits(w, 22, 2),
        bits(w, 21, 1),
        bits(w, 20, 1),
        bits(w, 16, 4),
        bits(w, 12, 4),
        bits(w, 11, 1),
        bits(w, 5, 5),
        bits(w, 0, 5),
    );
    let fp = matches!(opc, 1 | 5 | 9) && !(u != 0 && opc != 9);
    if fp {
        if sz < 2 {
            return un("fp16 indexed");
        }
        let fsz = sz & 1;
        let idx = if fsz != 0 {
            if l != 0 {
                return un("indexed L");
            }
            h
        } else {
            (h << 1) | l
        };
        let m = (mb << 4) | rm;
        if !scalar && fsz != 0 && q == 0 {
            return un("indexed 2d Q");
        }
        let ft = &FT[fsz as usize];
        let (f, vt, sf, set) = (ft.f, ft.vt, ft.sf, ft.set);
        let fu = f.to_uppercase();
        let mul = if u != 0 { "FOP_MULX" } else { "FOP_MUL" };
        if scalar {
            let el = format!("{}[{idx}]", vr(vt, m));
            if opc == 9 {
                return Ok(format!("{set}({d}, op_{f}(c, {mul}, {sf}({n}), {el}, 0));"));
            }
            let xx = if opc == 1 { format!("{sf}({n})") } else { format!("neg_{f}({sf}({n}))") };
            return Ok(format!("{set}({d}, op_{f}(c, FOP_MADD, {sf}({d}), {xx}, {el}));"));
        }
        let el = format!("vsplat_{f}({}, {idx}, {q})", vr(vt, m));
        if opc == 9 {
            return Ok(format!("SV_FOP_{fu}({mul}, {q}, {d}, {}, {el}, ({vt}){{}});", vq(vt, q, n)));
        }
        let xx = if opc == 1 { vq(vt, q, n) } else { format!("vneg_{f}({})", vq(vt, q, n)) };
        return Ok(format!("SV_FOP_{fu}(FOP_MADD, {q}, {d}, {}, {xx}, {el});", vq(vt, q, d)));
    }
    let (idx, m) = match sz {
        1 => ((h << 2) | (l << 1) | mb, rm),
        2 => ((h << 1) | l, (mb << 4) | rm),
        _ => return un("indexed size"),
    };
    let mi = m as i32;
    let t = vu(sz);
    if scalar {
        let op = match (u, opc) {
            (0, 3) => "SQDMLAL",
            (0, 7) => "SQDMLSL",
            (0, 11) => "SQDMULL",
            (0, 12) => "SQDMULH",
            (0, 13) => "SQRDMULH",
            _ => return un("scalar indexed"),
        };
        let szd = if matches!(op, "SQDMULH" | "SQRDMULH") { sz } else { sz + 1 };
        return Ok(gen(&lop(op, 0), sz, 1, d, szd, 0, 0, n, sz, 0, mi, sz, 0, idx as i32, 0));
    }
    let ne = lanes(q, sz);
    let el = shuf(&vr(t, m), &vr(t, m), &vec![idx; (16 >> sz) as usize]);
    match (u, opc) {
        (0, 8) => return Ok(vw(t, q, d, &format!("{} * {el}", vr(t, n)))),
        (1, 0) | (1, 4) => {
            return Ok(vw(t, q, d, &format!("{} {} {} * {el}", vr(t, d), if opc == 0 { "+" } else { "-" }, vr(t, n))))
        }
        (0, 12) | (0, 13) => {
            let op = if opc == 12 { "SQDMULH" } else { "SQRDMULH" };
            return Ok(same(&lop(op, 0), sz, ne, d, n, mi, 0, idx as i32));
        }
        _ => {}
    }
    let lng = match opc {
        2 => "MLAL",
        6 => "MLSL",
        10 => "MULL",
        3 => "SQDMLAL",
        7 => "SQDMLSL",
        11 => "SQDMULL",
        _ => return un("indexed opcode"),
    };
    if u != 0 && lng.starts_with("SQ") {
        return un("indexed opcode");
    }
    let ne = 8 >> sz;
    let uu = if lng.starts_with("SQ") { 0 } else { u };
    Ok(gen(&lop(lng, uu), sz, ne, d, sz + 1, 0, 0, n, sz, q * ne, mi, sz, 0, idx as i32, 0))
}

// ---- scalar FP

fn ftype(w: u32) -> Result<u32, Unsupported> {
    let t = bits(w, 22, 2);
    if t > 1 {
        return un("fp type");
    }
    Ok(t)
}

fn fp_dp3(w: u32) -> R {
    let t = ftype(w)?;
    let (o1, o0) = (bits(w, 21, 1), bits(w, 15, 1));
    let (d, n, m, a) = (bits(w, 0, 5), bits(w, 5, 5), bits(w, 16, 5), bits(w, 10, 5));
    // FMADD 00, FMSUB 01 (-n), FNMADD 10 (-a, -n), FNMSUB 11 (-a)
    Ok(format!("fp_fma_{}(c, {o1}, {}, {d}, {n}, {m}, {a});", FT[t as usize].f, o1 ^ o0))
}

fn fp_dp2(w: u32) -> R {
    let t = ftype(w)?;
    let (opc, d, n, m) = (bits(w, 12, 4), bits(w, 0, 5), bits(w, 5, 5), bits(w, 16, 5));
    const OPS: [&str; 9] =
        ["FOP_MUL", "FOP_DIV", "FOP_ADD", "FOP_SUB", "FOP_MAX", "FOP_MIN", "FOP_MAXNM", "FOP_MINNM", "FOP_NMUL"];
    let Some(fop) = OPS.get(opc as usize) else { return un("fp dp2") };
    Ok(format!("fp_op_{}(c, {fop}, {d}, {n}, {m});", FT[t as usize].f))
}

fn fp_dp1(w: u32) -> R {
    let t = bits(w, 22, 2);
    let (opc, d, n) = (bits(w, 15, 6), bits(w, 0, 5), bits(w, 5, 5));
    if t == 3 {
        // half source: only FCVT
        return match opc {
            4 => Ok(format!("FP_SETS({d}, simd_h2s(c, VS16({n})));")),
            5 => Ok(format!("FP_SETD({d}, simd_h2d(c, VS16({n})));")),
            _ => un("fp16 dp1"),
        };
    }
    if t == 2 {
        return un("fp type");
    }
    let ft = &FT[t as usize];
    let (f, sf, su, set) = (ft.f, ft.sf, ft.su, ft.set);
    let sign = if t == 0 { "0x80000000u" } else { "0x8000000000000000ull" };
    Ok(match opc {
        0 => format!("c->v[{d}] = {su}({n});"),
        1 => format!("c->v[{d}] = {su}({n}) & ~{sign};"),
        2 => format!("c->v[{d}] = {su}({n}) ^ {sign};"),
        3 => format!("fp_op_{f}(c, FOP_SQRT, {d}, {n}, {n});"),
        4 | 5 | 7 => {
            let to = match opc {
                4 => 0,
                5 => 1,
                _ => 3,
            };
            if to == t {
                return un("fcvt same");
            }
            if to == 3 {
                let cv = if t == 0 { "simd_s2h" } else { "simd_dd2h" };
                return Ok(format!("c->v[{d}] = {cv}(c, {sf}({n}));"));
            }
            if to == 1 {
                format!("FP_SETD({d}, simd_s2d(c, VSF32({n})));")
            } else {
                format!("FP_SETS({d}, simd_d2s(c, VSF64({n})));")
            }
        }
        _ => {
            let rm = match opc {
                8 => 0,
                9 => 1,
                10 => 2,
                11 => 3,
                12 => 4,
                14 => 5,
                15 => 6,
                _ => return un("fp dp1 opcode"),
            };
            format!("{set}({d}, frint_{f}(c, {sf}({n}), {rm}));")
        }
    })
}

fn fp_cmp(w: u32) -> R {
    let t = ftype(w)?;
    if bits(w, 14, 2) != 0 || bits(w, 0, 3) != 0 {
        return un("fcmp op");
    }
    let ft = &FT[t as usize];
    let (n, m, zero) = (bits(w, 5, 5), bits(w, 16, 5), bits(w, 3, 1));
    let b = if zero != 0 { "0".to_string() } else { format!("{}({m})", ft.sf) };
    Ok(format!("c->nzcv = cmp_{}(c, {}({n}), {b});", ft.f, ft.sf))
}

fn fp_ccmp(w: u32) -> R {
    let t = ftype(w)?;
    let ft = &FT[t as usize];
    let (n, m, cnd, nzcv) = (bits(w, 5, 5), bits(w, 16, 5), bits(w, 12, 4), bits(w, 0, 4));
    Ok(format!(
        "c->nzcv = {} ? cmp_{}(c, {sf}({n}), {sf}({m})) : {};",
        cond_expr(cnd),
        ft.f,
        hx((nzcv as u64) << 28),
        sf = ft.sf
    ))
}

fn fp_csel(w: u32) -> R {
    let t = ftype(w)?;
    let su = FT[t as usize].su;
    let (d, n, m, cnd) = (bits(w, 0, 5), bits(w, 5, 5), bits(w, 16, 5), bits(w, 12, 4));
    Ok(format!("c->v[{d}] = {} ? {su}({n}) : {su}({m});", cond_expr(cnd)))
}

fn fp_imm(w: u32) -> R {
    let t = ftype(w)?;
    if bits(w, 5, 5) != 0 {
        return un("fmov imm5");
    }
    let (imm8, d) = (bits(w, 13, 8) as u64, bits(w, 0, 5));
    Ok(format!("c->v[{d}] = {};", hx(vfp_expand_imm(imm8, 32 << t))))
}

fn fp_fixed(w: u32) -> R {
    let (sf, t, rmode, opc, scale, n, d) =
        (bits(w, 31, 1), bits(w, 22, 2), bits(w, 19, 2), bits(w, 16, 3), bits(w, 10, 6), bits(w, 5, 5), bits(w, 0, 5));
    if t > 1 {
        return un("fixed type");
    }
    if sf == 0 && scale < 32 {
        return un("fixed scale");
    }
    let ft = &FT[t as usize];
    let (fb, ib) = (64 - scale, if sf != 0 { 64 } else { 32 });
    if rmode == 0 && matches!(opc, 2 | 3) {
        let src = if sf != 0 { xreg(n) } else { wreg(n) };
        return Ok(format!("{}({d}, fromint_{}(c, {src}, {ib}, {}, {fb}));", ft.set, ft.f, opc & 1));
    }
    if rmode == 3 && matches!(opc, 0 | 1) {
        return Ok(set_reg(sf == 1, d, &format!("toint_{}(c, {}({n}), 3, {fb}, {ib}, {opc})", ft.f, ft.sf), false));
    }
    un("fixed opcode")
}

fn fp_int(w: u32) -> R {
    let (sf, t, rmode, opc, n, d) = (bits(w, 31, 1), bits(w, 22, 2), bits(w, 19, 2), bits(w, 16, 3), bits(w, 5, 5), bits(w, 0, 5));
    if matches!(opc, 6 | 7) {
        // FMOV
        if t == 2 && sf == 1 && rmode == 1 {
            return Ok(if opc == 6 {
                set_reg(true, d, &format!("VHI64({n})"), false)
            } else {
                format!("VHI64({d}) = {};", xreg(n))
            });
        }
        if rmode != 0 || t != sf || t > 1 {
            return un("fmov general");
        }
        if opc == 6 {
            return Ok(set_reg(sf == 1, d, &if sf != 0 { format!("VS64({n})") } else { format!("VS32({n})") }, false));
        }
        return Ok(format!("c->v[{d}] = {};", if sf != 0 { xreg(n) } else { wreg(n) }));
    }
    if t > 1 {
        return un("fp int type");
    }
    let ft = &FT[t as usize];
    let ib = if sf != 0 { 64 } else { 32 };
    if matches!(opc, 2 | 3) {
        if rmode != 0 {
            return un("cvtf rmode");
        }
        let src = if sf != 0 { xreg(n) } else { wreg(n) };
        return Ok(format!("{}({d}, fromint_{}(c, {src}, {ib}, {}, 0));", ft.set, ft.f, opc & 1));
    }
    let rm = if matches!(opc, 4 | 5) {
        if rmode != 0 {
            return un("fcvta rmode");
        }
        4
    } else {
        rmode
    };
    Ok(set_reg(sf == 1, d, &format!("toint_{}(c, {}({n}), {rm}, 0, {ib}, {})", ft.f, ft.sf, opc & 1), false))
}

// ---- loads / stores

/// FP / SIMD loads and stores (V = 1).
pub fn ldst(w: u32, pc: u64) -> R {
    let rt = bits(w, 0, 5);
    if bits(w, 27, 3) == 0b011 && bits(w, 24, 2) == 0 {
        // literal
        let opc = bits(w, 30, 2);
        if opc == 3 {
            return un("ldr literal opc");
        }
        let addr = pc.wrapping_add((sext(bits(w, 5, 19) as u64, 19) * 4) as u64);
        return Ok(format!("c->v[{rt}] = {}({});", ["LD32", "LD64", "LD128"][opc as usize], hx(addr)));
    }
    if bits(w, 31, 1) == 0 && matches!(bits(w, 24, 6), 0b001100 | 0b001101) {
        return ldst_struct(w);
    }
    if bits(w, 27, 3) == 0b101 {
        return ldst_pair(w);
    }
    if bits(w, 27, 3) == 0b111 {
        return ldst_reg(w);
    }
    un("ldst simd")
}

fn offset(base: &str, off: i64) -> String {
    if off >= 0 {
        format!("{base} + {off}")
    } else {
        format!("{base} - {}", -off)
    }
}

fn ldst_pair(w: u32) -> R {
    let (opc, mode, l) = (bits(w, 30, 2), bits(w, 23, 2), bits(w, 22, 1));
    let (imm7, rt2, rn, rt) = (sext(bits(w, 15, 7) as u64, 7), bits(w, 10, 5), bits(w, 5, 5), bits(w, 0, 5));
    if opc == 3 {
        return un("pair opc 3");
    }
    let scale = 2 + opc;
    let off = imm7 << scale;
    let base = xs(rn);
    let (post, wb) = (mode == 1, matches!(mode, 1 | 3));
    let ea = if post { base.clone() } else { offset(&base, off) };
    let (ld, st) = match scale {
        2 => ("LD32", "ST32"),
        3 => ("LD64", "ST64"),
        _ => ("LD128", "ST128"),
    };
    let sz = 1 << scale;
    let mut lines = vec![format!("uint64_t ea = {ea};")];
    if l != 0 {
        lines.push(format!("u128 a = {ld}(ea), b = {ld}(ea + {sz});"));
        lines.push(format!("c->v[{rt}] = a; c->v[{rt2}] = b;"));
    } else {
        let (v1, v2) = match scale {
            2 => (format!("VS32({rt})"), format!("VS32({rt2})")),
            3 => (format!("VS64({rt})"), format!("VS64({rt2})")),
            _ => (format!("c->v[{rt}]"), format!("c->v[{rt2}]")),
        };
        lines.push(format!("{st}(ea, {v1}); {st}(ea + {sz}, {v2});"));
    }
    if wb {
        lines.push(if post { format!("{base} = {};", offset("ea", off)) } else { format!("{base} = ea;") });
    }
    Ok(format!("{{ {} }}", lines.join(" ")))
}

fn ldst_reg(w: u32) -> R {
    let (size, opc, rn, rt) = (bits(w, 30, 2), bits(w, 22, 2), bits(w, 5, 5), bits(w, 0, 5));
    let scale = if opc & 2 != 0 {
        if size != 0 {
            return un("ldst simd size");
        }
        4
    } else {
        size
    };
    let l = opc & 1;
    let base = xs(rn);
    let mut wb = None;
    let ea = match bits(w, 24, 2) {
        1 => format!("{base} + {}", hx((bits(w, 10, 12) as u64) << scale)),
        0 => {
            let kind = bits(w, 10, 2);
            if bits(w, 21, 1) != 0 {
                if kind != 2 {
                    return un("ldst simd reg kind");
                }
                let (rm, option, s) = (bits(w, 16, 5), bits(w, 13, 3), bits(w, 12, 1));
                if !matches!(option, 2 | 3 | 6 | 7) {
                    return un("ldst simd reg option");
                }
                format!("{base} + {}", extend_expr(rm, option, if s != 0 { scale } else { 0 }))
            } else {
                let imm9 = sext(bits(w, 12, 9) as u64, 9);
                match kind {
                    0 => offset(&base, imm9),
                    1 => {
                        wb = Some(offset(&base, imm9));
                        base.clone()
                    }
                    3 => {
                        wb = Some("ea".to_string());
                        offset(&base, imm9)
                    }
                    _ => return un("ldst simd unprivileged"),
                }
            }
        }
        _ => return un("ldst simd"),
    };
    let mut parts = vec![format!("uint64_t ea = {ea};")];
    if l != 0 {
        parts.push(format!("c->v[{rt}] = {}(ea);", ["LD8", "LD16", "LD32", "LD64", "LD128"][scale as usize]));
    } else {
        let src = match scale {
            0 => format!("VS8({rt})"),
            1 => format!("VS16({rt})"),
            2 => format!("VS32({rt})"),
            3 => format!("VS64({rt})"),
            _ => format!("c->v[{rt}]"),
        };
        parts.push(format!("{}(ea, {src});", ["ST8", "ST16", "ST32", "ST64", "ST128"][scale as usize]));
    }
    if let Some(wb) = wb {
        parts.push(format!("{base} = {wb};"));
    }
    Ok(format!("{{ {} }}", parts.join(" ")))
}

fn ldst_struct(w: u32) -> R {
    let (q, l, post, rm, rn, rt, sz) =
        (bits(w, 30, 1), bits(w, 22, 1), bits(w, 23, 1), bits(w, 16, 5), bits(w, 5, 5), bits(w, 0, 5), bits(w, 10, 2));
    if post == 0 && rm != 0 {
        return un("ldst struct rm");
    }
    let base = xs(rn);
    let (acc, total);
    if bits(w, 24, 1) == 0 {
        // multiple structures
        if bits(w, 21, 1) != 0 {
            return un("ldst multiple bit21");
        }
        let (selem, rpt) = match bits(w, 12, 4) {
            0 => (4, 1),
            2 => (1, 4),
            4 => (3, 1),
            6 => (1, 3),
            7 => (1, 1),
            8 => (2, 1),
            10 => (1, 2),
            _ => return un("ldst multiple opcode"),
        };
        if sz == 3 && q == 0 && selem > 1 {
            return un("ldst multiple 1d");
        }
        total = (8u32 << q) * selem * rpt;
        if selem == 1 {
            // LD1 / ST1: whole registers
            let step = if q != 0 { 16 } else { 8 };
            let mut a = Vec::new();
            for i in 0..rpt {
                let r = (rt + i) % 32;
                a.push(match (l != 0, q != 0) {
                    (true, true) => format!("c->v[{r}] = LD128(ea + {});", i * step),
                    (true, false) => format!("c->v[{r}] = LD64(ea + {});", i * step),
                    (false, true) => format!("ST128(ea + {}, c->v[{r}]);", i * step),
                    (false, false) => format!("ST64(ea + {}, VS64({r}));", i * step),
                });
            }
            acc = a.join(" ");
        } else {
            acc = format!("sl_{}(c, {q}, {sz}, {selem}, {rpt}, {rt}, ea);", if l != 0 { "ldm" } else { "stm" });
        }
    } else {
        // single structure
        let (opc, s, r) = (bits(w, 13, 3), bits(w, 12, 1), bits(w, 21, 1));
        let mut scale = opc >> 1;
        let selem = (((opc & 1) << 1) | r) + 1;
        if scale == 3 {
            if l == 0 || s != 0 {
                return un("ldnr");
            }
            total = selem << sz;
            acc = format!("sl_ldr(c, {q}, {sz}, {selem}, {rt}, ea);");
        } else {
            let idx = match scale {
                0 => (q << 3) | (s << 2) | sz,
                1 => {
                    if sz & 1 != 0 {
                        return un("ld1 h size");
                    }
                    (q << 2) | (s << 1) | (sz >> 1)
                }
                _ => {
                    if sz & 2 != 0 {
                        return un("ld1 s size");
                    }
                    if sz & 1 != 0 {
                        if s != 0 {
                            return un("ld1 d S");
                        }
                        scale = 3;
                        q
                    } else {
                        (q << 1) | s
                    }
                }
            };
            total = selem << scale;
            acc = format!("sl_{}(c, {scale}, {selem}, {idx}, {rt}, ea);", if l != 0 { "lds" } else { "sts" });
        }
    }
    let mut parts = vec![format!("uint64_t ea = {base};"), acc];
    if post != 0 {
        parts.push(if rm == 31 { format!("{base} = ea + {total};") } else { format!("{base} = ea + {};", xreg(rm)) });
    }
    Ok(format!("{{ {} }}", parts.join(" ")))
}

// ---- CRC32

/// CRC32{B,H,W,X} / CRC32C*.
pub fn crc(w: u32) -> R {
    let (sf, rm, c, sz, rn, rd) = (bits(w, 31, 1), bits(w, 16, 5), bits(w, 12, 1), bits(w, 10, 2), bits(w, 5, 5), bits(w, 0, 5));
    if (sz == 3) != (sf == 1) {
        return un("crc size");
    }
    let val = if sz == 3 { xreg(rm) } else { wreg(rm) };
    Ok(set_reg(false, rd, &format!("{}({}, {val}, {})", if c != 0 { "CRC32C" } else { "CRC32" }, wreg(rn), 1 << sz), false))
}
