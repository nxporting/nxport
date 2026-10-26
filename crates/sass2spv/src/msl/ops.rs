// Operations (codegen::instructions, op for op), written as SPIRV-Cross writes the SPIR-V instruction the code
// generator makes for each.

use super::helpers::Helper;
use super::{component, enclose, is_main, type_name, unsupported, Ctx, Global, R};
use crate::ir::{Instruction as I, IoVariable, OperandType, StorageKind};
use crate::structured::{AstBlockType, AstId, AstOperation};
use crate::translation::definitions::AggregateType as A;
use crate::types::ShaderStage;

type Res = R<(A, Option<String>)>;

const NONE: (A, Option<String>) = (A::INVALID, None);

struct Op {
    inst: I,
    storage_kind: StorageKind,
    force_precise: bool,
    index: i32,
    sources: Vec<Option<AstId>>,
}

impl Op {
    fn src(&self, i: usize) -> R<Option<AstId>> {
        self.sources.get(i).copied().ok_or_else(|| "msl: missing source".into())
    }
    fn has(&self, f: I) -> bool {
        self.inst.0 & f.0 == f.0
    }
    fn fp(&self) -> A {
        if self.has(I::FP64) {
            A::FP64
        } else if self.has(I::FP32) {
            A::FP32
        } else {
            A::S32
        }
    }
}

/// A value from one SPIR-V result computed from `operands` (Ctx::inst).
fn vi(c: &mut Ctx, t: A, s: String, operands: &[&String]) -> Res {
    let ops: Vec<&str> = operands.iter().map(|o| o.as_str()).collect();
    Ok((t, Some(c.inst(t, s, &ops)?)))
}

fn vcall(c: &mut Ctx, t: A, f: &str, args: Vec<String>) -> Res {
    let text = call(f, &args);
    let ops: Vec<&str> = args.iter().map(|o| o.as_str()).collect();
    Ok((t, Some(c.inst(t, text, &ops)?)))
}

fn vbin(c: &mut Ctx, t: A, a: String, sym: &str, b: String) -> Res {
    let text = bin(a.clone(), sym, b.clone());
    Ok((t, Some(c.inst(t, text, &[&a, &b])?)))
}

fn val(t: A, s: String) -> Res {
    Ok((t, Some(s)))
}

fn stmt(s: String) -> Res {
    Ok((A::INVALID, Some(s)))
}

fn bin(a: String, op: &str, b: String) -> String {
    format!("{} {op} {}", enclose(a), enclose(b))
}

fn call(f: &str, args: &[String]) -> String {
    format!("{f}({})", args.join(", "))
}

pub fn operation(c: &mut Ctx, node: AstId) -> Res {
    let info = c.info;
    let o: &AstOperation = info.ast.op(node);
    if o.tex.is_some() {
        return super::tex::operation(c, o);
    }
    let op = Op { inst: o.inst, storage_kind: o.storage_kind, force_precise: o.force_precise, index: o.index, sources: o.sources.clone() };
    let s = |i: usize| op.src(i);
    match op.inst.base() {
        I::ABSOLUTE => unary_func(c, &op, "abs", "abs"),
        I::ADD => arith(c, &op, "+", Some(Helper::FAdd), "spvFAdd"),
        I::SUBTRACT => arith(c, &op, "-", Some(Helper::FSub), "spvFSub"),
        I::MULTIPLY => arith(c, &op, "*", Some(Helper::FMul), "spvFMul"),
        I::DIVIDE => arith(c, &op, "/", None, ""),
        I::ATOMIC_ADD => atomic(c, &op, "atomic_fetch_add_explicit"),
        I::ATOMIC_AND => atomic(c, &op, "atomic_fetch_and_explicit"),
        I::ATOMIC_MIN_S32 => atomic(c, &op, "atomic_fetch_min_explicit"),
        I::ATOMIC_MIN_U32 => atomic(c, &op, "atomic_fetch_min_explicit"),
        I::ATOMIC_MAX_S32 => atomic(c, &op, "atomic_fetch_max_explicit"),
        I::ATOMIC_MAX_U32 => atomic(c, &op, "atomic_fetch_max_explicit"),
        I::ATOMIC_OR => atomic(c, &op, "atomic_fetch_or_explicit"),
        I::ATOMIC_SWAP => atomic(c, &op, "atomic_exchange_explicit"),
        I::ATOMIC_XOR => atomic(c, &op, "atomic_fetch_xor_explicit"),
        I::ATOMIC_COMPARE_AND_SWAP => atomic_compare_and_swap(c, &op),
        I::BALLOT => {
            let cond = c.expr(A::BOOL, s(0)?)?;
            if c.single_lane_subgroups() {
                if op.index == 0 {
                    return vi(c, A::U32, format!("uint({cond})"), &[&cond]);
                }
                return vi(c, A::U32, format!("{} ? 0u : 0u", enclose(cond.clone())), &[&cond]);
            }
            c.helper(Helper::SubgroupBallot);
            {
                let b = c.inst(A::VECTOR4 | A::U32, format!("spvSubgroupBallot({cond})"), &[&cond])?;
                vi(c, A::U32, format!("{}.{}", enclose(b.clone()), component(op.index)?), &[&b])
            }
        }
        I::BARRIER => {
            // SPIRV-Cross writes barriers only in kernels, and not right after a memory barrier.
            if c.stage != ShaderStage::Compute || prev_is_memory_barrier(c, node) {
                return Ok(NONE);
            }
            stmt("threadgroup_barrier(mem_flags::mem_threadgroup)".into())
        }
        I::GROUP_MEMORY_BARRIER | I::MEMORY_BARRIER => {
            if c.stage != ShaderStage::Compute {
                return Ok(NONE);
            }
            stmt("threadgroup_barrier(mem_flags::mem_device | mem_flags::mem_threadgroup | mem_flags::mem_texture)".into())
        }
        I::BIT_COUNT => {
            let v = c.expr(A::S32, s(0)?)?;
            vcall(c, A::S32, "popcount", vec![v])
        }
        I::BITFIELD_REVERSE => {
            let v = c.expr(A::S32, s(0)?)?;
            vcall(c, A::S32, "reverse_bits", vec![v])
        }
        I::BITWISE_NOT => {
            let v = c.expr(A::S32, s(0)?)?;
            vi(c, A::S32, format!("~{}", enclose(v.clone())), &[&v])
        }
        I::BITWISE_AND => int_bin(c, &op, "&"),
        I::BITWISE_OR => int_bin(c, &op, "|"),
        I::BITWISE_EXCLUSIVE_OR => int_bin(c, &op, "^"),
        I::SHIFT_LEFT => int_bin(c, &op, "<<"),
        I::SHIFT_RIGHT_S32 => int_bin(c, &op, ">>"),
        I::SHIFT_RIGHT_U32 => {
            let a = c.expr(A::S32, s(0)?)?;
            let b = c.expr(A::S32, s(1)?)?;
            vi(c, A::S32, format!("int(uint({a}) >> uint({b}))"), &[&a, &b])
        }
        I::BITFIELD_EXTRACT_S32 => {
            let a = c.expr(A::S32, s(0)?)?;
            let b = c.expr(A::U32, s(1)?)?;
            let d = c.expr(A::U32, s(2)?)?;
            vcall(c, A::S32, "extract_bits", vec![a, b, d])
        }
        I::BITFIELD_EXTRACT_U32 => {
            let a = c.expr(A::U32, s(0)?)?;
            let b = c.expr(A::U32, s(1)?)?;
            let d = c.expr(A::U32, s(2)?)?;
            vcall(c, A::U32, "extract_bits", vec![a, b, d])
        }
        I::BITFIELD_INSERT => {
            let a = c.expr(A::U32, s(0)?)?;
            let b = c.expr(A::U32, s(1)?)?;
            let d = c.expr(A::U32, s(2)?)?;
            let e = c.expr(A::U32, s(3)?)?;
            vcall(c, A::U32, "insert_bits", vec![a, b, d, e])
        }
        I::CALL => call_op(c, &op),
        I::CEILING => unary_func(c, &op, "ceil", ""),
        I::FLOOR => unary_func(c, &op, "floor", ""),
        I::TRUNCATE => unary_func(c, &op, "trunc", ""),
        I::ROUND => unary_func(c, &op, "rint", ""),
        I::SQUARE_ROOT => unary_func(c, &op, "sqrt", ""),
        I::RECIPROCAL_SQUARE_ROOT => unary_func(c, &op, "rsqrt", ""),
        I::EXPONENT_B2 => unary_func(c, &op, "exp2", ""),
        I::LOGARITHM_B2 => unary_func(c, &op, "log2", ""),
        I::SINE => unary_func(c, &op, "sin", ""),
        I::COSINE => unary_func(c, &op, "cos", ""),
        I::CLAMP => ternary_func(c, &op, "precise::clamp", "clamp"),
        I::CLAMP_U32 => {
            let a = c.expr(A::U32, s(0)?)?;
            let b = c.expr(A::U32, s(1)?)?;
            let d = c.expr(A::U32, s(2)?)?;
            vcall(c, A::U32, "clamp", vec![a, b, d])
        }
        I::FUSED_MULTIPLY_ADD => ternary_func(c, &op, "fma", ""),
        I::MAXIMUM => binary_func(c, &op, "precise::max", "max"),
        I::MINIMUM => binary_func(c, &op, "precise::min", "min"),
        I::MAXIMUM_U32 => {
            let a = c.expr(A::U32, s(0)?)?;
            let b = c.expr(A::U32, s(1)?)?;
            vcall(c, A::U32, "max", vec![a, b])
        }
        I::MINIMUM_U32 => {
            let a = c.expr(A::U32, s(0)?)?;
            let b = c.expr(A::U32, s(1)?)?;
            vcall(c, A::U32, "min", vec![a, b])
        }
        I::MODULO => {
            if op.fp() == A::S32 {
                unsupported!("integer modulo");
            }
            let t = op.fp();
            let a = c.expr(t, s(0)?)?;
            let b = c.expr(t, s(1)?)?;
            c.helper(Helper::Mod);
            vcall(c, t, "mod", vec![a, b])
        }
        I::NEGATE => {
            let t = op.fp();
            let a = c.expr(t, s(0)?)?;
            vi(c, t, format!("-{}", enclose(a.clone())), &[&a])
        }
        I::COMMENT => Ok(NONE),
        I::COMPARE_EQUAL => compare(c, &op, "=="),
        I::COMPARE_NOT_EQUAL => compare(c, &op, "!="),
        I::COMPARE_GREATER => compare(c, &op, ">"),
        I::COMPARE_GREATER_OR_EQUAL => compare(c, &op, ">="),
        I::COMPARE_LESS => compare(c, &op, "<"),
        I::COMPARE_LESS_OR_EQUAL => compare(c, &op, "<="),
        I::COMPARE_GREATER_U32 => compare_u32(c, &op, ">"),
        I::COMPARE_GREATER_OR_EQUAL_U32 => compare_u32(c, &op, ">="),
        I::COMPARE_LESS_U32 => compare_u32(c, &op, "<"),
        I::COMPARE_LESS_OR_EQUAL_U32 => compare_u32(c, &op, "<="),
        I::CONDITIONAL_SELECT => {
            let cond = c.expr(A::BOOL, s(0)?)?;
            let t = op.fp();
            // A select of the constants 1 and 0 is a conversion (SPIRV-Cross to_trivial_mix_op; 0 compares equal, so
            // -0.0 counts as 0 too).
            let one_zero = match (constant_operand(c, s(1)?), constant_operand(c, s(2)?)) {
                (Some(a), Some(b)) if t == A::S32 => a == 1 && b == 0,
                (Some(a), Some(b)) => f32::from_bits(a as u32) == 1.0 && f32::from_bits(b as u32) == 0.0,
                _ => false,
            };
            if one_zero {
                return vi(c, t, format!("{}({cond})", type_name(t)?), &[&cond]);
            }
            let a = c.expr(t, s(1)?)?;
            let b = c.expr(t, s(2)?)?;
            vi(c, t, format!("{} ? {} : {}", enclose(cond.clone()), enclose(a.clone()), enclose(b.clone())), &[&cond, &a, &b])
        }
        I::CONVERT_FP32_TO_FP64 | I::CONVERT_FP64_TO_FP32 => {
            let src = if op.inst.base() == I::CONVERT_FP32_TO_FP64 { A::FP32 } else { A::FP64 };
            let dst = if src == A::FP32 { A::FP64 } else { A::FP32 };
            let v = c.expr(src, s(0)?)?;
            vcall(c, dst, "float", vec![v])
        }
        I::CONVERT_FP32_TO_S32 | I::CONVERT_FP64_TO_S32 => convert(c, &op, A::S32, "int"),
        I::CONVERT_FP32_TO_U32 | I::CONVERT_FP64_TO_U32 => convert(c, &op, A::U32, "uint"),
        I::CONVERT_S32_TO_FP32 | I::CONVERT_S32_TO_FP64 | I::CONVERT_U32_TO_FP32 | I::CONVERT_U32_TO_FP64 => {
            let src = if matches!(op.inst.base(), I::CONVERT_S32_TO_FP32 | I::CONVERT_S32_TO_FP64) { A::S32 } else { A::U32 };
            let dst = if matches!(op.inst.base(), I::CONVERT_S32_TO_FP64 | I::CONVERT_U32_TO_FP64) { A::FP64 } else { A::FP32 };
            let v = c.expr(src, s(0)?)?;
            vcall(c, dst, "float", vec![v])
        }
        I::DDX => {
            let v = c.expr(A::FP32, s(0)?)?;
            vcall(c, A::FP32, "dfdx", vec![v])
        }
        I::DDY => {
            let v = c.expr(A::FP32, s(0)?)?;
            vcall(c, A::FP32, "dfdy", vec![v])
        }
        I::DISCARD => {
            if c.uses[0].contains(&Global::Builtin(IoVariable::ThreadKill)) {
                stmt("gl_HelperInvocation = true, discard_fragment()".into())
            } else {
                stmt("discard_fragment()".into())
            }
        }
        I::EMIT_VERTEX | I::END_PRIMITIVE => unsupported!("geometry operations"),
        I::FSI_BEGIN | I::FSI_END => unsupported!("fragment shader interlock"),
        I::FIND_LSB => {
            let v = c.expr(A::U32, s(0)?)?;
            c.helper(Helper::FindILsb);
            vcall(c, A::U32, "spvFindLSB", vec![v])
        }
        I::FIND_MSB_S32 => {
            let v = c.expr(A::S32, s(0)?)?;
            c.helper(Helper::FindSMsb);
            vi(c, A::U32, format!("uint(spvFindSMSB({v}))"), &[&v])
        }
        I::FIND_MSB_U32 => {
            let v = c.expr(A::U32, s(0)?)?;
            c.helper(Helper::FindUMsb);
            vcall(c, A::U32, "spvFindUMSB", vec![v])
        }
        I::IS_NAN => {
            let t = if op.has(I::FP64) { A::FP64 } else { A::FP32 };
            let v = c.expr(t, s(0)?)?;
            vcall(c, A::BOOL, "isnan", vec![v])
        }
        I::LOAD => load(c, &op),
        I::STORE => store(c, &op),
        I::LOGICAL_AND => bool_bin(c, &op, "&&"),
        I::LOGICAL_OR => bool_bin(c, &op, "||"),
        I::LOGICAL_EXCLUSIVE_OR => bool_bin(c, &op, "!="),
        I::LOGICAL_NOT => {
            let v = c.expr(A::BOOL, s(0)?)?;
            vi(c, A::BOOL, format!("!{}", enclose(v.clone())), &[&v])
        }
        I::LOOP_BREAK => stmt("break".into()),
        I::LOOP_CONTINUE => stmt("continue".into()),
        I::MULTIPLY_HIGH_S32 => {
            let a = c.expr(A::S32, s(0)?)?;
            let b = c.expr(A::S32, s(1)?)?;
            vcall(c, A::S32, "mulhi", vec![a, b])
        }
        I::MULTIPLY_HIGH_U32 => {
            let a = c.expr(A::U32, s(0)?)?;
            let b = c.expr(A::U32, s(1)?)?;
            vcall(c, A::U32, "mulhi", vec![a, b])
        }
        I::PACK_DOUBLE_2X32 => pack_double(c, &op),
        I::UNPACK_DOUBLE_2X32 => unsupported!("UnpackDouble2x32 outside a pack"),
        I::PACK_HALF_2X16 => {
            let a = c.expr(A::FP32, s(0)?)?;
            let b = c.expr(A::FP32, s(1)?)?;
            {
                let v = c.inst(A::VECTOR2 | A::FP32, format!("float2({a}, {b})"), &[&a, &b])?;
                vi(c, A::U32, format!("as_type<uint>(half2({v}))"), &[&v])
            }
        }
        I::UNPACK_HALF_2X16 => {
            let v = c.expr(A::U32, s(0)?)?;
            {
                let u = c.inst(A::VECTOR2 | A::FP32, format!("float2(as_type<half2>({v}))"), &[&v])?;
                vi(c, A::FP32, format!("{u}.{}", component(op.index)?), &[&u])
            }
        }
        I::RETURN => {
            c.may_have_returned = true;
            // A function's final return without a value is left implicit (SPIRV-Cross emits it only where control
            // flow continues after it).
            let last = c.info.ast.last(c.info.functions[c.func].main_block) == Some(node);
            if last && op.sources.is_empty() && !(is_main(c) && c.has_stage_out) {
                return Ok(NONE);
            }
            if is_main(c) {
                return stmt(if c.has_stage_out { "return out".into() } else { "return".into() });
            }
            if op.sources.is_empty() {
                stmt("return".into())
            } else {
                let rt = c.info.functions[c.func].return_type;
                let v = c.expr(rt, s(0)?)?;
                stmt(format!("return {v}"))
            }
        }
        I::SHUFFLE => shuffle(c, &op, Helper::SubgroupShuffle, "spvSubgroupShuffle"),
        I::SHUFFLE_XOR => shuffle(c, &op, Helper::SubgroupShuffleXor, "spvSubgroupShuffleXor"),
        I::SHUFFLE_UP => shuffle(c, &op, Helper::SubgroupShuffleUp, "spvSubgroupShuffleUp"),
        I::SHUFFLE_DOWN => shuffle(c, &op, Helper::SubgroupShuffleDown, "spvSubgroupShuffleDown"),
        I::SWIZZLE_ADD => swizzle_add(c, &op),
        I::VECTOR_EXTRACT => {
            let (vt, v) = c.expr_typed(s(0)?)?;
            let st = vt & !A::ELEMENT_COUNT_MASK;
            let idx = s(1)?;
            let k = idx.and_then(|n| c.info.ast.as_operand(n)).filter(|o| o.ty == OperandType::Constant).map(|o| o.value);
            match k {
                Some(i) => vi(c, st, format!("{}.{}", enclose(v.clone()), component(i)?), &[&v]),
                None => {
                    let e = c.expr(A::S32, idx)?;
                    vi(c, st, format!("{}[{e}]", enclose(v.clone())), &[&v, &e])
                }
            }
        }
        I::VOTE_ALL => vote(c, &op, "simd_all"),
        I::VOTE_ANY => vote(c, &op, "simd_any"),
        I::VOTE_ALL_EQUAL => {
            if c.single_lane_subgroups() {
                return val(A::BOOL, "true".into());
            }
            let v = c.expr(A::BOOL, s(0)?)?;
            c.helper(Helper::SubgroupAllEqual);
            vcall(c, A::BOOL, "spvSubgroupAllEqual", vec![v])
        }
        _ => unsupported!("operation {}", op.inst.name()),
    }
}

fn prev_is_memory_barrier(c: &Ctx, node: AstId) -> bool {
    let ast = &c.info.ast;
    match ast.previous(node) {
        Ok(Some(p)) => ast.as_operation(p).is_some_and(|o| matches!(o.inst.base(), I::MEMORY_BARRIER | I::GROUP_MEMORY_BARRIER)),
        _ => false,
    }
}

fn unary_func(c: &mut Ctx, op: &Op, f: &str, fi: &str) -> Res {
    let t = op.fp();
    if t == A::S32 && fi.is_empty() {
        unsupported!("integer {}", op.inst.name());
    }
    let v = c.expr(t, op.src(0)?)?;
    vcall(c, t, if t == A::S32 { fi } else { f }, vec![v])
}

fn binary_func(c: &mut Ctx, op: &Op, f: &str, fi: &str) -> Res {
    let t = op.fp();
    let a = c.expr(t, op.src(0)?)?;
    let b = c.expr(t, op.src(1)?)?;
    vcall(c, t, if t == A::S32 { fi } else { f }, vec![a, b])
}

fn ternary_func(c: &mut Ctx, op: &Op, f: &str, fi: &str) -> Res {
    let t = op.fp();
    if t == A::S32 && fi.is_empty() {
        unsupported!("integer {}", op.inst.name());
    }
    let a = c.expr(t, op.src(0)?)?;
    let b = c.expr(t, op.src(1)?)?;
    let d = c.expr(t, op.src(2)?)?;
    vcall(c, t, if t == A::S32 { fi } else { f }, vec![a, b, d])
}

/// Add / subtract / multiply / divide: floating point ones that may not contract go through SPIRV-Cross's helpers.
fn arith(c: &mut Ctx, op: &Op, sym: &str, helper: Option<Helper>, f: &str) -> Res {
    let t = op.fp();
    let a = c.expr(t, op.src(0)?)?;
    let b = c.expr(t, op.src(1)?)?;
    if t != A::S32 && c.precise(op.force_precise) {
        if let Some(h) = helper {
            c.helper(h);
            return vcall(c, t, f, vec![a, b]);
        }
    }
    vbin(c, t, a, sym, b)
}

fn int_bin(c: &mut Ctx, op: &Op, sym: &str) -> Res {
    let a = c.expr(A::S32, op.src(0)?)?;
    let b = c.expr(A::S32, op.src(1)?)?;
    vbin(c, A::S32, a, sym, b)
}

fn bool_bin(c: &mut Ctx, op: &Op, sym: &str) -> Res {
    let a = c.expr(A::BOOL, op.src(0)?)?;
    let b = c.expr(A::BOOL, op.src(1)?)?;
    vbin(c, A::BOOL, a, sym, b)
}

fn compare(c: &mut Ctx, op: &Op, sym: &str) -> Res {
    let t = op.fp();
    let a = c.expr(t, op.src(0)?)?;
    let b = c.expr(t, op.src(1)?)?;
    vbin(c, A::BOOL, a, sym, b)
}

fn compare_u32(c: &mut Ctx, op: &Op, sym: &str) -> Res {
    let a = c.expr(A::U32, op.src(0)?)?;
    let b = c.expr(A::U32, op.src(1)?)?;
    vbin(c, A::BOOL, a, sym, b)
}

fn convert(c: &mut Ctx, op: &Op, dst: A, f: &str) -> Res {
    let src = if matches!(op.inst.base(), I::CONVERT_FP64_TO_S32 | I::CONVERT_FP64_TO_U32) { A::FP64 } else { A::FP32 };
    let v = c.expr(src, op.src(0)?)?;
    vcall(c, dst, f, vec![v])
}

fn vote(c: &mut Ctx, op: &Op, f: &str) -> Res {
    let v = c.expr(A::BOOL, op.src(0)?)?;
    if c.single_lane_subgroups() {
        return val(A::BOOL, v);
    }
    vcall(c, A::BOOL, f, vec![v])
}

fn shuffle(c: &mut Ctx, op: &Op, h: Helper, f: &str) -> Res {
    let v = c.expr(A::FP32, op.src(0)?)?;
    if c.single_lane_subgroups() {
        return val(A::FP32, v);
    }
    let i = c.expr(A::U32, op.src(1)?)?;
    c.helper(h);
    vcall(c, A::FP32, f, vec![v, i])
}

fn swizzle_add(c: &mut Ctx, op: &Op) -> Res {
    let x = c.expr(A::FP32, op.src(0)?)?;
    let y = c.expr(A::FP32, op.src(1)?)?;
    let mask = c.expr(A::U32, op.src(2)?)?;
    let (lane, _) = super::builtin_input(c, IoVariable::SubgroupLaneId)?;
    let idx = format!("({} >> ((({lane} & 3u) << 1u))) & 3u", enclose(mask));
    val(
        A::FP32,
        format!("({} * float4(1.0, -1.0, 1.0, 0.0)[{idx}]) + ({} * float4(1.0, 1.0, -1.0, 1.0)[{idx}])", enclose(x), enclose(y)),
    )
}

/// Doubles are computed in float (Metal has none): a pack of the two halves of one unpacked value is that value.
fn pack_double(c: &mut Ctx, op: &Op) -> Res {
    let ast = &c.info.ast;
    let half = |n: Option<AstId>| -> Option<(AstId, i32)> {
        let o = ast.as_operation(n?)?;
        (o.inst.base() == I::UNPACK_DOUBLE_2X32).then_some((o.sources.first().copied()??, o.index))
    };
    match (half(op.src(0)?), half(op.src(1)?)) {
        (Some((a, 0)), Some((b, 1))) if a == b || same_operand(c, a, b) => {
            let v = c.expr(A::FP64, Some(a))?;
            val(A::FP64, v)
        }
        _ => unsupported!("PackDouble2x32 of other values"),
    }
}

fn same_operand(c: &Ctx, a: AstId, b: AstId) -> bool {
    let ast = &c.info.ast;
    match (ast.as_operand(a), ast.as_operand(b)) {
        (Some(x), Some(y)) => x.ty == OperandType::LocalVariable && y.ty == OperandType::LocalVariable && a == b,
        _ => false,
    }
}

fn call_op(c: &mut Ctx, op: &Op) -> Res {
    let ast = &c.info.ast;
    let func_id = ast.opnd(op.src(0)?.ok_or("msl: call without target")?).value as usize;
    let f = &c.info.functions[func_id];
    let mut args = Vec::new();
    for i in 1..op.sources.len() {
        let local = op.src(i)?.ok_or("msl: call argument")?;
        let n = c.locals.get(&local).cloned().ok_or("msl: call arguments must be locals")?;
        args.push(n);
    }
    for g in c.uses[func_id].clone() {
        c.note(g);
        args.push(super::global_arg(c, g)?);
    }
    let call = call(&super::function_name(func_id), &args);
    if f.return_type == A::VOID {
        stmt(call)
    } else if f.out_arguments.is_empty() && function_is_pure(c, func_id) {
        vi(c, f.return_type, call, &[])
    } else {
        // SPIRV-Cross forwards a call only to a pure function without out parameters.
        let t = c.temp();
        c.pre.push(format!("{} {t} = {call};", super::type_name(f.return_type)?));
        val(f.return_type, c.leaf(t))
    }
}

/// SPIRV-Cross's function_is_pure: no memory writes outside the function's own variables, atomics, barriers,
/// discards or vertex emission, also in the functions it calls.
fn function_is_pure(c: &Ctx, func_id: usize) -> bool {
    let ast = &c.info.ast;
    super::nodes_of(c.info, c.info.functions[func_id].main_block).into_iter().filter_map(|n| ast.as_operation(n)).all(|o| {
        let i = o.inst.base();
        if i == I::CALL {
            let callee = o.sources.first().copied().flatten().and_then(|s| ast.as_operand(s)).map(|x| x.value as usize);
            return callee.is_some_and(|f| f != func_id && function_is_pure(c, f));
        }
        !(o.inst.is_atomic()
            || matches!(
                i,
                I::STORE | I::IMAGE_STORE | I::IMAGE_ATOMIC | I::BARRIER | I::MEMORY_BARRIER | I::GROUP_MEMORY_BARRIER | I::DISCARD | I::EMIT_VERTEX | I::END_PRIMITIVE
            ))
    })
}

fn constant_operand(c: &Ctx, node: Option<AstId>) -> Option<i32> {
    node.and_then(|n| c.info.ast.as_operand(n)).filter(|o| o.ty == OperandType::Constant).map(|o| o.value)
}

/// The lvalue an operation's storage operands name, with its element type (codegen storage_pointer).
/// Also the dependency count of the access chain (the index expressions it reads).
fn place(c: &mut Ctx, op: &Op, value_sources: usize) -> R<(String, A, Option<&'static str>, Option<usize>)> {
    let sk = op.storage_kind;
    let mut si = 0usize;
    let mut chain = 0usize;
    let mut indexed = false;
    let first = constant_operand(c, op.src(si)?).ok_or("msl: storage operand must be constant")?;
    si += 1;
    let inputs_end = op.sources.len() - value_sources;
    match sk {
        StorageKind::ConstantBuffer | StorageKind::StorageBuffer => {
            let field_index = constant_operand(c, op.src(si)?).ok_or("msl: field must be constant")?;
            si += 1;
            let b = c.buffer(sk, first)?;
            c.note_buffer(sk, first);
            let field = b.ty.fields.get(field_index as usize).ok_or("msl: field index")?;
            let mut lv = format!("{}._m{field_index}", b.name);
            let mut t = field.ty;
            let vec4_scalars = super::scalar_array_in_vec4(b, field_index as usize);
            while si < inputs_end {
                let n = op.src(si)?;
                si += 1;
                indexed = true;
                if t.has(A::ARRAY) {
                    let e = index(c, n)?;
                    chain += c.deps.get(&e).map_or(0, |d| 1 + d);
                    lv = format!("{lv}[{e}]");
                    t = t & !A::ARRAY;
                    if vec4_scalars {
                        lv.push_str(".x");
                    }
                } else if t.count() != A::SCALAR {
                    let (l, d) = component_access(c, lv, n)?;
                    lv = l;
                    chain += d;
                    t = t & !A::ELEMENT_COUNT_MASK;
                } else {
                    unsupported!("buffer index depth");
                }
            }
            let space = if sk == StorageKind::ConstantBuffer { "constant" } else { "device" };
            Ok((lv, field.ty.element(), Some(space), indexed.then_some(chain)))
        }
        StorageKind::LocalMemory | StorageKind::SharedMemory => {
            let (name, m, space) = if sk == StorageKind::LocalMemory {
                c.note(Global::LocalMemory(first));
                (format!("lmem{first}"), c.props.local_memories.get(&first).ok_or("msl: local memory")?, "thread")
            } else {
                c.note(Global::SharedMemory(first));
                (format!("smem{first}"), c.props.shared_memories.get(&first).ok_or("msl: shared memory")?, "threadgroup")
            };
            let mut lv = name;
            while si < inputs_end {
                indexed = true;
                let e = index(c, op.src(si)?)?;
                chain += c.deps.get(&e).map_or(0, |d| 1 + d);
                si += 1;
                lv = format!("{lv}[{e}]");
            }
            Ok((lv, m.ty.element(), Some(space), indexed.then_some(chain)))
        }
        StorageKind::Input | StorageKind::Output => {
            let v = IoVariable::from_value(first);
            let is_output = sk == StorageKind::Output;
            let d = c.d;
            let (mut lv, t) = if d.has_per_location_input_or_output(v, is_output) {
                let location = constant_operand(c, op.src(si)?).ok_or("msl: location must be constant")?;
                si += 1;
                if v == IoVariable::FragmentOutputColor {
                    c.note(Global::StageOut);
                    (format!("out.color{location}"), d.fragment_output_color_type(location))
                } else if is_output {
                    c.note(Global::StageOut);
                    (format!("out.loc{location}"), d.user_defined_type(location, true))
                } else if c.stage == ShaderStage::Vertex {
                    c.note(Global::StageIn);
                    (format!("in.in_attr{location}"), d.user_defined_type(location, false))
                } else {
                    c.note(Global::StageIn);
                    (format!("in.loc{location}"), d.user_defined_type(location, false))
                }
            } else if is_output {
                c.note(Global::StageOut);
                match v {
                    IoVariable::Position => ("out.gl_Position".to_string(), A::VECTOR4 | A::FP32),
                    IoVariable::PointSize => ("out.gl_PointSize".to_string(), A::FP32),
                    IoVariable::FragmentOutputDepth => ("out.gl_FragDepth".to_string(), A::FP32),
                    IoVariable::Layer => ("out.gl_Layer".to_string(), A::U32),
                    IoVariable::ViewportIndex => ("out.gl_ViewportIndex".to_string(), A::U32),
                    v => unsupported!("output {v:?}"),
                }
            } else {
                super::builtin_input(c, v)?
            };
            let mut ty = t;
            while si < inputs_end {
                let n = op.src(si)?;
                si += 1;
                indexed = true;
                if ty.count() != A::SCALAR {
                    let (l, d) = component_access(c, lv, n)?;
                    lv = l;
                    chain += d;
                    ty = ty & !A::ELEMENT_COUNT_MASK;
                } else {
                    unsupported!("I/O index depth");
                }
            }
            Ok((lv, ty.element(), None, indexed.then_some(chain)))
        }
        sk => unsupported!("storage {sk:?}"),
    }
}

fn index(c: &mut Ctx, n: Option<AstId>) -> R<String> {
    match constant_operand(c, n) {
        Some(k) if k >= 0 => Ok(k.to_string()),
        _ => c.expr(A::S32, n),
    }
}

fn component_access(c: &mut Ctx, lv: String, n: Option<AstId>) -> R<(String, usize)> {
    match constant_operand(c, n) {
        Some(k) => Ok((format!("{lv}.{}", component(k)?), 0)),
        None => {
            let e = c.expr(A::S32, n)?;
            let d = c.deps.get(&e).map_or(0, |d| 1 + d);
            Ok((format!("{lv}[{e}]"), d))
        }
    }
}

/// The IR type of an I/O element (codegen builtin_of: what the operation reads and writes).
fn io_ir_type(v: IoVariable, msl: A) -> A {
    use IoVariable as V;
    match v {
        V::VertexIndex | V::InstanceIndex | V::BaseInstance | V::BaseVertex | V::Layer | V::ViewportIndex => A::S32,
        _ => msl,
    }
}

fn load(c: &mut Ctx, op: &Op) -> Res {
    if op.storage_kind == StorageKind::Input && c.single_lane_subgroups() {
        if let Some(v) = constant_operand(c, op.src(0)?) {
            let own = match IoVariable::from_value(v) {
                IoVariable::SubgroupLaneId => Some(0),
                IoVariable::SubgroupEqMask | IoVariable::SubgroupGeMask | IoVariable::SubgroupLeMask => Some(1),
                IoVariable::SubgroupGtMask | IoVariable::SubgroupLtMask => Some(0),
                _ => None,
            };
            if let Some(own) = own {
                let component0 = op.sources.len() < 2 || constant_operand(c, op.sources[1]) == Some(0);
                return val(A::U32, format!("{}u", if component0 { own } else { 0 }));
            }
        }
    }
    let (lv, t, _, chain) = place(c, op, 0)?;
    // An access chain (an expression with the index expressions as dependencies), then the load of it; a
    // variable read directly (a builtin) is a load without a chain.
    let ds: Vec<usize> = chain.into_iter().collect();
    if matches!(op.storage_kind, StorageKind::Input | StorageKind::Output) {
        let v = IoVariable::from_value(constant_operand(c, op.src(0)?).unwrap_or(0));
        let ir = io_ir_type(v, t);
        if ir != t {
            let e = c.inst_d(ir, format!("{}({lv})", type_name(ir)?), &ds)?;
            return Ok((ir, Some(e)));
        }
    }
    let e = c.inst_d(t, lv, &ds)?;
    Ok((t, Some(e)))
}

fn store(c: &mut Ctx, op: &Op) -> Res {
    let (lv, t, _, _) = place(c, op, 1)?;
    let mut ty = t;
    if matches!(op.storage_kind, StorageKind::Input | StorageKind::Output) {
        let v = IoVariable::from_value(constant_operand(c, op.src(0)?).unwrap_or(0));
        ty = io_ir_type(v, t);
    }
    let v = c.expr(ty, op.src(op.sources.len() - 1)?)?;
    let v = if ty != t { format!("{}({v})", type_name(t)?) } else { v };
    stmt(format!("{lv} = {v}"))
}

fn atomic(c: &mut Ctx, op: &Op, f: &str) -> Res {
    let (lv, t, space, _) = place(c, op, 1)?;
    let space = space.ok_or("msl: atomic on I/O")?;
    if !matches!(t, A::U32 | A::S32) {
        unsupported!("atomic of type {}", t.0);
    }
    // Signed / unsigned min and max reinterpret the element (SPIRV-Cross emit_atomic_func_op: expected_type).
    let expected = match op.inst.base() {
        I::ATOMIC_MIN_S32 | I::ATOMIC_MAX_S32 => A::S32,
        I::ATOMIC_MIN_U32 | I::ATOMIC_MAX_U32 => A::U32,
        _ => t,
    };
    let at = if expected == A::U32 { "atomic_uint" } else { "atomic_int" };
    let v = c.expr(expected, op.src(op.sources.len() - 1)?)?;
    let space = if space == "constant" { "device" } else { space };
    let e = format!("{f}(({space} {at}*)&{lv}, {v}, memory_order_relaxed)");
    let e = if expected != t { format!("as_type<{}>({e})", type_name(t)?) } else { e };
    val(t, e)
}

/// Compare-and-swap with the old value as result, as SPIRV-Cross writes OpAtomicCompareExchange: a weak exchange
/// retried while it fails spuriously (the loaded value still equals the comparator).
fn atomic_compare_and_swap(c: &mut Ctx, op: &Op) -> Res {
    let (lv, t, space, _) = place(c, op, 2)?;
    let space = space.ok_or("msl: atomic on I/O")?;
    if !matches!(t, A::U32 | A::S32) {
        unsupported!("atomic of type {}", t.0);
    }
    let at = if t == A::U32 { "atomic_uint" } else { "atomic_int" };
    let n = op.sources.len();
    let cmp = c.expr(t, op.src(n - 2)?)?;
    let v = c.expr(t, op.src(n - 1)?)?;
    let space = if space == "constant" { "device" } else { space };
    let r = c.temp();
    c.pre.push(format!("{} {r};", type_name(t)?));
    c.pre.push(format!(
        "do {{ {r} = {cmp}; }} while (!atomic_compare_exchange_weak_explicit(({space} {at}*)&{lv}, &{r}, {v}, \
         memory_order_relaxed, memory_order_relaxed) && {r} == {cmp});"
    ));
    c.deps.insert(r.clone(), 0);
    val(t, r)
}

/// The ballot of lanes below this one, as SPIRV-Cross computes gl_SubgroupLtMask at the start of the entry point.
pub fn subgroup_lt_mask_prologue(c: &mut Ctx) -> R<()> {
    c.note(Global::Builtin(IoVariable::SubgroupLaneId));
    c.helper(Helper::SubgroupBallot);
    c.line("uint4 gl_SubgroupLtMask = uint4(extract_bits(0xFFFFFFFF, 0, min(gl_SubgroupInvocationID, 32u)), extract_bits(0xFFFFFFFF, 0, (uint)max((int)gl_SubgroupInvocationID - 32, 0)), uint2(0));");
    Ok(())
}

pub fn block_is_main(c: &Ctx) -> bool {
    c.info.ast.block_type(c.current_block) == AstBlockType::Main
}
