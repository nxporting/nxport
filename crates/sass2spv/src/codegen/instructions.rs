// SPIR-V for each operation (CodeGen.Spirv.Instructions). Arguments are evaluated in the original's order (left to right,
// innermost first): that order assigns the ids.

use super::declarations::{builtin_of, is_per_vertex_array_builtin, is_per_vertex_builtin, per_vertex_struct_field_index};
use super::{CodeGenContext, R};
use crate::ir::{Instruction as I, IoVariable, OperandType, StorageKind, TextureFlags};
use crate::spv::*;
use crate::structured::{AstBlockType, AstId, AstTexture, IoDefinition};
use crate::translation::definitions::AggregateType as A;
use crate::translation::optimizations::utils::NRE;
use crate::types::{SamplerType, ShaderStage};

type Res = R<(A, Option<Inst>)>;
type Un = fn(&mut Module, &Inst, &Inst) -> Inst;
type Bin = fn(&mut Module, &Inst, &Inst, &Inst) -> Inst;
type Ter = fn(&mut Module, &Inst, &Inst, &Inst, &Inst) -> Inst;
type Quad = fn(&mut Module, &Inst, &Inst, &Inst, &Inst, &Inst) -> Inst;

const INVALID: (A, Option<Inst>) = (A::INVALID, None);

fn default_memory_semantics() -> MemorySemanticsMask {
    MemorySemanticsMask(
        MemorySemanticsMask::IMAGE_MEMORY.0
            | MemorySemanticsMask::ATOMIC_COUNTER_MEMORY.0
            | MemorySemanticsMask::WORKGROUP_MEMORY.0
            | MemorySemanticsMask::UNIFORM_MEMORY.0
            | MemorySemanticsMask::ACQUIRE_RELEASE.0,
    )
}

/// Metal has SIMD-group functions only in fragment and kernel functions: in the other stages every invocation is a
/// subgroup of one (a shader must work for any subgroup size): ballot = own bit, votes = own value, shuffles = own
/// value, lane id 0.
pub fn single_lane_subgroups(stage: ShaderStage) -> bool {
    matches!(stage, ShaderStage::Vertex | ShaderStage::TessellationControl | ShaderStage::TessellationEvaluation | ShaderStage::Geometry)
}

struct Op {
    inst: I,
    storage_kind: StorageKind,
    force_precise: bool,
    index: i32,
    sources: Vec<Option<AstId>>,
    tex: Option<AstTexture>,
}

impl Op {
    fn src(&self, i: usize) -> R<Option<AstId>> {
        self.sources.get(i).copied().ok_or_else(|| "System.IndexOutOfRangeException: Index was outside the bounds of the array.".into())
    }

    fn has(&self, f: I) -> bool {
        self.inst.0 & f.0 == f.0
    }

    fn vector_type(&self, scalar: A) -> A {
        match (self.index as u32).count_ones() {
            2 => scalar | A::VECTOR2,
            3 => scalar | A::VECTOR3,
            4 => scalar | A::VECTOR4,
            _ => scalar,
        }
    }
}

pub fn generate(c: &mut CodeGenContext, node: AstId) -> Res {
    let info = c.info;
    let o = info.ast.op(node);
    let op = Op {
        inst: o.inst,
        storage_kind: o.storage_kind,
        force_precise: o.force_precise,
        index: o.index,
        sources: o.sources.clone(),
        tex: o.tex,
    };
    match op.inst.base() {
        I::ABSOLUTE => unary(c, &op, Module::glsl_f_abs, Some(Module::glsl_s_abs)),
        I::ADD => binary(c, &op, Module::f_add, Some(Module::i_add)),
        I::ATOMIC_ADD => atomic_memory_binary(c, &op, Module::atomic_i_add),
        I::ATOMIC_AND => atomic_memory_binary(c, &op, Module::atomic_and),
        I::ATOMIC_COMPARE_AND_SWAP => atomic_memory_cas(c, &op),
        I::ATOMIC_MIN_S32 => atomic_memory_binary(c, &op, Module::atomic_s_min),
        I::ATOMIC_MIN_U32 => atomic_memory_binary(c, &op, Module::atomic_u_min),
        I::ATOMIC_MAX_S32 => atomic_memory_binary(c, &op, Module::atomic_s_max),
        I::ATOMIC_MAX_U32 => atomic_memory_binary(c, &op, Module::atomic_u_max),
        I::ATOMIC_OR => atomic_memory_binary(c, &op, Module::atomic_or),
        I::ATOMIC_SWAP => atomic_memory_binary(c, &op, Module::atomic_exchange),
        I::ATOMIC_XOR => atomic_memory_binary(c, &op, Module::atomic_xor),
        I::BALLOT => ballot(c, &op),
        I::BARRIER => barrier(c),
        I::BIT_COUNT => unary_s32(c, &op, Module::bit_count),
        I::BITFIELD_EXTRACT_S32 => bitfield_extract_s32(c, &op),
        I::BITFIELD_EXTRACT_U32 => ternary_u32(c, &op, Module::bit_field_u_extract),
        I::BITFIELD_INSERT => bitfield_insert(c, &op),
        I::BITFIELD_REVERSE => unary_s32(c, &op, Module::bit_reverse),
        I::BITWISE_AND => binary_s32(c, &op, Module::bitwise_and),
        I::BITWISE_EXCLUSIVE_OR => binary_s32(c, &op, Module::bitwise_xor),
        I::BITWISE_NOT => unary_s32(c, &op, Module::not),
        I::BITWISE_OR => binary_s32(c, &op, Module::bitwise_or),
        I::CALL => call(c, &op),
        I::CEILING => unary(c, &op, Module::glsl_ceil, None),
        I::CLAMP => ternary(c, &op, Module::glsl_f_clamp, Some(Module::glsl_s_clamp)),
        I::CLAMP_U32 => ternary_u32(c, &op, Module::glsl_u_clamp),
        I::COMMENT => Ok(INVALID),
        I::COMPARE_EQUAL => compare(c, &op, Module::f_ord_equal, Module::i_equal),
        I::COMPARE_GREATER => compare(c, &op, Module::f_ord_greater_than, Module::s_greater_than),
        I::COMPARE_GREATER_OR_EQUAL => compare(c, &op, Module::f_ord_greater_than_equal, Module::s_greater_than_equal),
        I::COMPARE_GREATER_OR_EQUAL_U32 => compare_u32(c, &op, Module::u_greater_than_equal),
        I::COMPARE_GREATER_U32 => compare_u32(c, &op, Module::u_greater_than),
        I::COMPARE_LESS => compare(c, &op, Module::f_ord_less_than, Module::s_less_than),
        I::COMPARE_LESS_OR_EQUAL => compare(c, &op, Module::f_ord_less_than_equal, Module::s_less_than_equal),
        I::COMPARE_LESS_OR_EQUAL_U32 => compare_u32(c, &op, Module::u_less_than_equal),
        I::COMPARE_LESS_U32 => compare_u32(c, &op, Module::u_less_than),
        I::COMPARE_NOT_EQUAL => compare(c, &op, Module::f_ord_not_equal, Module::i_not_equal),
        I::CONDITIONAL_SELECT => conditional_select(c, &op),
        I::CONVERT_FP32_TO_FP64 => convert(c, &op, A::FP64, A::FP32, Module::f_convert),
        I::CONVERT_FP32_TO_S32 => convert(c, &op, A::S32, A::FP32, Module::convert_f_to_s),
        I::CONVERT_FP32_TO_U32 => convert(c, &op, A::U32, A::FP32, Module::convert_f_to_u),
        I::CONVERT_FP64_TO_FP32 => convert(c, &op, A::FP32, A::FP64, Module::f_convert),
        I::CONVERT_FP64_TO_S32 => convert(c, &op, A::S32, A::FP64, Module::convert_f_to_s),
        I::CONVERT_FP64_TO_U32 => convert(c, &op, A::U32, A::FP64, Module::convert_f_to_u),
        I::CONVERT_S32_TO_FP32 => convert(c, &op, A::FP32, A::S32, Module::convert_s_to_f),
        I::CONVERT_S32_TO_FP64 => convert(c, &op, A::FP64, A::S32, Module::convert_s_to_f),
        I::CONVERT_U32_TO_FP32 => convert(c, &op, A::FP32, A::U32, Module::convert_u_to_f),
        I::CONVERT_U32_TO_FP64 => convert(c, &op, A::FP64, A::U32, Module::convert_u_to_f),
        I::COSINE => unary(c, &op, Module::glsl_cos, None),
        I::DDX => unary_fp32(c, &op, Module::d_pdx),
        I::DDY => unary_fp32(c, &op, Module::d_pdy),
        I::DISCARD => {
            c.m.kill();
            Ok(INVALID)
        }
        I::DIVIDE => binary(c, &op, Module::f_div, Some(Module::s_div)),
        I::EMIT_VERTEX => {
            c.m.emit_vertex();
            Ok(INVALID)
        }
        I::END_PRIMITIVE => {
            c.m.end_primitive();
            Ok(INVALID)
        }
        I::EXPONENT_B2 => unary(c, &op, Module::glsl_exp2, None),
        I::FSI_BEGIN => {
            if c.host.supports_fragment_shader_interlock {
                c.m.begin_invocation_interlock_ext();
            }
            Ok(INVALID)
        }
        I::FSI_END => {
            if c.host.supports_fragment_shader_interlock {
                c.m.end_invocation_interlock_ext();
            }
            Ok(INVALID)
        }
        I::FIND_LSB => {
            let s = c.get_u32(op.src(0)?)?;
            let t = c.type_u32();
            Ok((A::U32, Some(c.m.glsl_find_i_lsb(&t, &s))))
        }
        I::FIND_MSB_S32 => {
            let s = c.get_s32(op.src(0)?)?;
            let t = c.type_u32();
            Ok((A::U32, Some(c.m.glsl_find_s_msb(&t, &s))))
        }
        I::FIND_MSB_U32 => {
            let s = c.get_u32(op.src(0)?)?;
            let t = c.type_u32();
            Ok((A::U32, Some(c.m.glsl_find_u_msb(&t, &s))))
        }
        I::FLOOR => unary(c, &op, Module::glsl_floor, None),
        I::FUSED_MULTIPLY_ADD => ternary(c, &op, Module::glsl_fma, None),
        I::GROUP_MEMORY_BARRIER => {
            let scope = constant_u32_en(c, Scope::WORKGROUP.0);
            let sem = constant_u32_en(c, default_memory_semantics().0);
            c.m.memory_barrier(&scope, &sem);
            Ok(INVALID)
        }
        I::IMAGE_ATOMIC => image_atomic(c, &op),
        I::IMAGE_LOAD => image_load(c, &op),
        I::IMAGE_STORE => image_store(c, &op),
        I::IS_NAN => {
            let src = op.src(0)?;
            let b = c.type_bool();
            let v = if op.has(I::FP64) { c.get_fp64(src)? } else { c.get_fp32(src)? };
            Ok((A::BOOL, Some(c.m.is_nan(&b, &v))))
        }
        I::LOAD => load_or_store(c, &op, false),
        I::LOD => lod(c, &op),
        I::LOGARITHM_B2 => unary(c, &op, Module::glsl_log2, None),
        I::LOGICAL_AND => binary_bool(c, &op, Module::logical_and),
        I::LOGICAL_EXCLUSIVE_OR => binary_bool(c, &op, Module::logical_not_equal),
        I::LOGICAL_NOT => {
            let b = c.type_bool();
            let v = c.get(A::BOOL, op.src(0)?)?;
            Ok((A::BOOL, Some(c.m.logical_not(&b, &v))))
        }
        I::LOGICAL_OR => binary_bool(c, &op, Module::logical_or),
        I::LOOP_BREAK => {
            let loop_block = enclosing_loop(c)?;
            let parent = c.info.ast.parent(loop_block).ok_or(NRE)?;
            let l = c.next_label(parent);
            c.m.branch(&l);
            Ok(INVALID)
        }
        I::LOOP_CONTINUE => {
            let loop_block = enclosing_loop(c)?;
            let (_, continue_target) = c.loop_targets.get(&loop_block).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
            c.m.branch(&continue_target);
            Ok(INVALID)
        }
        I::MAXIMUM => binary(c, &op, Module::glsl_f_max, Some(Module::glsl_s_max)),
        I::MAXIMUM_U32 => binary_u32(c, &op, Module::glsl_u_max),
        I::MEMORY_BARRIER => {
            let scope = constant_u32_en(c, Scope::DEVICE.0);
            let sem = constant_u32_en(c, default_memory_semantics().0);
            c.m.memory_barrier(&scope, &sem);
            Ok(INVALID)
        }
        I::MINIMUM => binary(c, &op, Module::glsl_f_min, Some(Module::glsl_s_min)),
        I::MINIMUM_U32 => binary_u32(c, &op, Module::glsl_u_min),
        I::MODULO => binary(c, &op, Module::f_mod, None),
        I::MULTIPLY => binary(c, &op, Module::f_mul, Some(Module::i_mul)),
        I::MULTIPLY_HIGH_S32 => multiply_high(c, &op, true),
        I::MULTIPLY_HIGH_U32 => multiply_high(c, &op, false),
        I::NEGATE => unary(c, &op, Module::f_negate, Some(Module::s_negate)),
        I::PACK_DOUBLE_2X32 => {
            let v0 = c.get_u32(op.src(0)?)?;
            let v1 = c.get_u32(op.src(1)?)?;
            let u = c.type_u32();
            let vt = c.m.type_vector(&u, Lit::i32(2), false);
            let vector = c.m.composite_construct(&vt, &[v0, v1]);
            let f = c.type_fp64();
            Ok((A::FP64, Some(c.m.glsl_pack_double2x32(&f, &vector))))
        }
        I::PACK_HALF_2X16 => {
            let v0 = c.get_fp32(op.src(0)?)?;
            let v1 = c.get_fp32(op.src(1)?)?;
            let f = c.type_fp32();
            let vt = c.m.type_vector(&f, Lit::i32(2), false);
            let vector = c.m.composite_construct(&vt, &[v0, v1]);
            let u = c.type_u32();
            Ok((A::U32, Some(c.m.glsl_pack_half2x16(&u, &vector))))
        }
        I::RECIPROCAL_SQUARE_ROOT => unary(c, &op, Module::glsl_inverse_sqrt, None),
        I::RETURN => {
            c.may_have_returned = true;
            if !op.sources.is_empty() {
                let rt = c.info.functions[c.current_function].return_type;
                let v = c.get(rt, op.src(0)?)?;
                c.m.return_value(&v);
            } else {
                c.m.return_();
            }
            Ok(INVALID)
        }
        I::ROUND => unary(c, &op, Module::glsl_round_even, None),
        I::SHIFT_LEFT => binary_s32(c, &op, Module::shift_left_logical),
        I::SHIFT_RIGHT_S32 => binary_s32(c, &op, Module::shift_right_arithmetic),
        I::SHIFT_RIGHT_U32 => binary_s32(c, &op, Module::shift_right_logical),
        I::SHUFFLE => shuffle(c, &op, Module::group_non_uniform_shuffle),
        I::SHUFFLE_DOWN => shuffle(c, &op, Module::group_non_uniform_shuffle_down),
        I::SHUFFLE_UP => shuffle(c, &op, Module::group_non_uniform_shuffle_up),
        I::SHUFFLE_XOR => shuffle(c, &op, Module::group_non_uniform_shuffle_xor),
        I::SINE => unary(c, &op, Module::glsl_sin, None),
        I::SQUARE_ROOT => unary(c, &op, Module::glsl_sqrt, None),
        I::STORE => load_or_store(c, &op, true),
        I::SUBTRACT => binary(c, &op, Module::f_sub, Some(Module::i_sub)),
        I::SWIZZLE_ADD => swizzle_add(c, &op),
        I::TEXTURE_SAMPLE => texture_sample(c, &op),
        I::TEXTURE_QUERY_SAMPLES => texture_query_samples(c, &op),
        I::TEXTURE_QUERY_SIZE => texture_query_size(c, &op),
        I::TRUNCATE => unary(c, &op, Module::glsl_trunc, None),
        I::UNPACK_DOUBLE_2X32 => {
            let v = c.get_fp64(op.src(0)?)?;
            let u = c.type_u32();
            let vt = c.m.type_vector(&u, Lit::i32(2), false);
            let vector = c.m.glsl_unpack_double2x32(&vt, &v);
            let u = c.type_u32();
            Ok((A::U32, Some(c.m.composite_extract(&u, &vector, &[Lit::i32(op.index).into()]))))
        }
        I::UNPACK_HALF_2X16 => {
            let v = c.get_u32(op.src(0)?)?;
            let f = c.type_fp32();
            let vt = c.m.type_vector(&f, Lit::i32(2), false);
            let vector = c.m.glsl_unpack_half2x16(&vt, &v);
            let f = c.type_fp32();
            Ok((A::FP32, Some(c.m.composite_extract(&f, &vector, &[Lit::i32(op.index).into()]))))
        }
        I::VECTOR_EXTRACT => vector_extract(c, &op),
        I::VOTE_ALL => vote(c, &op, Module::group_non_uniform_all),
        I::VOTE_ALL_EQUAL => {
            if single_lane_subgroups(c.definitions.stage) {
                let b = c.type_bool();
                return Ok((A::BOOL, Some(c.m.constant_true(&b))));
            }
            vote(c, &op, Module::group_non_uniform_all_equal)
        }
        I::VOTE_ANY => vote(c, &op, Module::group_non_uniform_any),
        _ => Err(format!("System.NotImplementedException: {}", op.inst.name())),
    }
}

/// Constant(TypeU32(), enum value): the literal is a signed 32-bit integer.
fn constant_u32_en(c: &mut CodeGenContext, v: u32) -> Inst {
    let t = c.type_u32();
    c.m.constant(&t, Lit::en(v))
}

fn enclosing_loop(c: &CodeGenContext) -> R<AstId> {
    let ast = &c.info.ast;
    let mut b = c.current_block;
    while ast.block_type(b) != AstBlockType::DoWhile {
        b = ast.parent(b).ok_or(NRE)?;
    }
    Ok(b)
}

fn ballot(c: &mut CodeGenContext, op: &Op) -> Res {
    let source = op.src(0)?;
    if single_lane_subgroups(c.definitions.stage) {
        let bit = c.const_u32_i(if op.index == 0 { 1 } else { 0 });
        let u = c.type_u32();
        let cond = c.get(A::BOOL, source)?;
        let z = c.const_u32_i(0);
        return Ok((A::U32, Some(c.m.select(&u, &cond, &bit, &z))));
    }
    let u = c.type_u32();
    let uvec4 = c.m.type_vector(&u, Lit::i32(4), false);
    let execution = constant_u32_en(c, Scope::SUBGROUP.0);
    let cond = c.get(A::BOOL, source)?;
    let mask_vector = c.m.group_non_uniform_ballot(&uvec4, &execution, &cond);
    let u = c.type_u32();
    Ok((A::U32, Some(c.m.composite_extract(&u, &mask_vector, &[Lit::i32(op.index).into()]))))
}

fn barrier(c: &mut CodeGenContext) -> Res {
    // Barriers in divergent control flow can hang the GPU without host support for them.
    if !c.host.supports_shader_barrier_divergence
        && (c.info.ast.block_type(c.current_block) != AstBlockType::Main || c.may_have_returned || !c.is_main_function)
    {
        return Ok(INVALID);
    }
    let e = constant_u32_en(c, Scope::WORKGROUP.0);
    let m = constant_u32_en(c, Scope::WORKGROUP.0);
    let s = constant_u32_en(c, MemorySemanticsMask::WORKGROUP_MEMORY.0 | MemorySemanticsMask::ACQUIRE_RELEASE.0);
    c.m.control_barrier(&e, &m, &s);
    Ok(INVALID)
}

fn call(c: &mut CodeGenContext, op: &Op) -> Res {
    let ast = &c.info.ast;
    let func_id = ast.opnd(op.src(0)?.ok_or(NRE)?).value;
    let (function, spv_func) = c.function(func_id)?;
    let mut args = Vec::with_capacity(op.sources.len() - 1);
    for i in 1..op.sources.len() {
        let local = op.src(i)?.ok_or(NRE)?;
        if ast.as_operand(local).is_none() {
            return Err("System.InvalidCastException: Unable to cast object of type 'AstOperation' to type 'AstOperand'.".into());
        }
        args.push(c.local_pointer(local)?);
    }
    let ret_type = c.info.functions[function].return_type;
    let t = c.get_type(ret_type, 1)?;
    Ok((ret_type, Some(c.m.function_call(&t, &spv_func, &args))))
}

fn conditional_select(c: &mut CodeGenContext, op: &Op) -> Res {
    let (s1, s2, s3) = (op.src(0)?, op.src(1)?, op.src(2)?);
    let cond = c.get(A::BOOL, s1)?;
    if op.has(I::FP64) {
        let t = c.type_fp64();
        let (a, b) = (c.get_fp64(s2)?, c.get_fp64(s3)?);
        Ok((A::FP64, Some(c.m.select(&t, &cond, &a, &b))))
    } else if op.has(I::FP32) {
        let t = c.type_fp32();
        let (a, b) = (c.get_fp32(s2)?, c.get_fp32(s3)?);
        Ok((A::FP32, Some(c.m.select(&t, &cond, &a, &b))))
    } else {
        let t = c.type_s32();
        let (a, b) = (c.get_s32(s2)?, c.get_s32(s3)?);
        Ok((A::S32, Some(c.m.select(&t, &cond, &a, &b))))
    }
}

fn type_of(c: &mut CodeGenContext, t: A) -> Inst {
    match t {
        A::FP32 => c.type_fp32(),
        A::FP64 => c.type_fp64(),
        A::S32 => c.type_s32(),
        _ => c.type_u32(),
    }
}

fn convert(c: &mut CodeGenContext, op: &Op, dst: A, src: A, f: Un) -> Res {
    let source = op.src(0)?;
    let t = type_of(c, dst);
    let v = c.get(src, source)?;
    Ok((dst, Some(f(&mut c.m, &t, &v))))
}

fn multiply_high(c: &mut CodeGenContext, op: &Op, signed: bool) -> Res {
    let (s1, s2) = (op.src(0)?, op.src(1)?);
    let (ta, tb) = if signed { (c.type_s32(), c.type_s32()) } else { (c.type_u32(), c.type_u32()) };
    let result_type = c.m.type_struct(false, &[ta, tb]);
    let (a, b) = if signed { (c.get_s32(s1)?, c.get_s32(s2)?) } else { (c.get_u32(s1)?, c.get_u32(s2)?) };
    let r = if signed { c.m.s_mul_extended(&result_type, &a, &b) } else { c.m.u_mul_extended(&result_type, &a, &b) };
    let t = if signed { c.type_s32() } else { c.type_u32() };
    let r = c.m.composite_extract(&t, &r, &[Lit::i32(1).into()]);
    Ok((if signed { A::S32 } else { A::U32 }, Some(r)))
}

fn shuffle(c: &mut CodeGenContext, op: &Op, f: Ter) -> Res {
    let value = c.get_fp32(op.src(0)?)?;
    if single_lane_subgroups(c.definitions.stage) {
        return Ok((A::FP32, Some(value)));
    }
    let index = c.get_u32(op.src(1)?)?;
    let t = c.type_fp32();
    let scope = c.const_u32_i(Scope::SUBGROUP.0 as i32);
    Ok((A::FP32, Some(f(&mut c.m, &t, &scope, &value, &index))))
}

fn vote(c: &mut CodeGenContext, op: &Op, f: Bin) -> Res {
    if single_lane_subgroups(c.definitions.stage) {
        return Ok((A::BOOL, Some(c.get(A::BOOL, op.src(0)?)?)));
    }
    let execution = constant_u32_en(c, Scope::SUBGROUP.0);
    let b = c.type_bool();
    let v = c.get(A::BOOL, op.src(0)?)?;
    Ok((A::BOOL, Some(f(&mut c.m, &b, &execution, &v))))
}

fn swizzle_add(c: &mut CodeGenContext, op: &Op) -> Res {
    let x = c.get(A::FP32, op.src(0)?)?;
    let y = c.get(A::FP32, op.src(1)?)?;
    let mask = c.get(A::U32, op.src(2)?)?;
    let f = c.type_fp32();
    let v4float = c.m.type_vector(&f, Lit::i32(4), false);
    let f = c.type_fp32();
    let one = c.m.constant(&f, Lit::f32(1.0));
    let f = c.type_fp32();
    let minus_one = c.m.constant(&f, Lit::f32(-1.0));
    let f = c.type_fp32();
    let zero = c.m.constant(&f, Lit::f32(0.0));
    let x_lut = c.m.constant_composite(&v4float, &[one.clone(), minus_one.clone(), one.clone(), zero]);
    let y_lut = c.m.constant_composite(&v4float, &[one.clone(), one.clone(), minus_one, one]);
    let three = c.const_u32_i(3);
    let thread_id = scalar_input(c, IoVariable::SubgroupLaneId)?;
    let u = c.type_u32();
    let shift = c.m.bitwise_and(&u, &thread_id, &three);
    let u = c.type_u32();
    let k1 = c.const_u32_i(1);
    let shift = c.m.shift_left_logical(&u, &shift, &k1);
    let u = c.type_u32();
    let lut_idx = c.m.shift_right_logical(&u, &mask, &shift);
    let u = c.type_u32();
    let lut_idx = c.m.bitwise_and(&u, &lut_idx, &three);
    let f = c.type_fp32();
    let x_lut_value = c.m.vector_extract_dynamic(&f, &x_lut, &lut_idx);
    let f = c.type_fp32();
    let y_lut_value = c.m.vector_extract_dynamic(&f, &y_lut, &lut_idx);
    let f = c.type_fp32();
    let x_result = c.m.f_mul(&f, &x, &x_lut_value);
    let f = c.type_fp32();
    let y_result = c.m.f_mul(&f, &y, &y_lut_value);
    let f = c.type_fp32();
    Ok((A::FP32, Some(c.m.f_add(&f, &x_result, &y_result))))
}

fn vector_extract(c: &mut CodeGenContext, op: &Op) -> Res {
    let (vector_type, vector) = c.get_with_type(op.src(0)?)?;
    let scalar_type = vector_type & !A::ELEMENT_COUNT_MASK;
    let result_type = c.get_type(scalar_type, 1)?;
    let s1 = op.src(1)?;
    let constant_index = s1.and_then(|s| c.info.ast.as_operand(s)).filter(|o| o.ty == OperandType::Constant).map(|o| o.value);
    let result = match constant_index {
        Some(i) => c.m.composite_extract(&result_type, &vector, &[Lit::i32(i).into()]),
        None => {
            let index = c.get(A::S32, s1)?;
            c.m.vector_extract_dynamic(&result_type, &vector, &index)
        }
    };
    Ok((scalar_type, Some(result)))
}

fn compare(c: &mut CodeGenContext, op: &Op, emit_f: Bin, emit_i: Bin) -> Res {
    let (s1, s2) = (op.src(0)?, op.src(1)?);
    let b = c.type_bool();
    let r = if op.has(I::FP64) {
        let (x, y) = (c.get_fp64(s1)?, c.get_fp64(s2)?);
        emit_f(&mut c.m, &b, &x, &y)
    } else if op.has(I::FP32) {
        let (x, y) = (c.get_fp32(s1)?, c.get_fp32(s2)?);
        emit_f(&mut c.m, &b, &x, &y)
    } else {
        let (x, y) = (c.get_s32(s1)?, c.get_s32(s2)?);
        emit_i(&mut c.m, &b, &x, &y)
    };
    Ok((A::BOOL, Some(r)))
}

fn compare_u32(c: &mut CodeGenContext, op: &Op, emit: Bin) -> Res {
    let (s1, s2) = (op.src(0)?, op.src(1)?);
    let b = c.type_bool();
    let (x, y) = (c.get_u32(s1)?, c.get_u32(s2)?);
    Ok((A::BOOL, Some(emit(&mut c.m, &b, &x, &y))))
}

fn atomic_memory_binary(c: &mut CodeGenContext, op: &Op, emit: Quad) -> Res {
    let (elem_pointer, var_type) = storage_pointer(c, op)?;
    let value = c.get(var_type, op.src(op.sources.len() - 1)?)?;
    let one = c.const_u32_i(1);
    let zero = c.const_u32_i(0);
    let t = c.get_type(var_type, 1)?;
    Ok((var_type, Some(emit(&mut c.m, &t, &elem_pointer, &one, &zero, &value))))
}

fn atomic_memory_cas(c: &mut CodeGenContext, op: &Op) -> Res {
    let (elem_pointer, var_type) = storage_pointer(c, op)?;
    let n = op.sources.len();
    let value0 = c.get(var_type, op.src(n - 2)?)?;
    let value1 = c.get(var_type, op.src(n - 1)?)?;
    let one = c.const_u32_i(1);
    let zero = c.const_u32_i(0);
    let t = c.get_type(var_type, 1)?;
    Ok((var_type, Some(c.m.atomic_compare_exchange(&t, &elem_pointer, &one, &zero, &zero, &value1, &value0))))
}

fn load_or_store(c: &mut CodeGenContext, op: &Op, is_store: bool) -> Res {
    if !is_store && op.storage_kind == StorageKind::Input && single_lane_subgroups(c.definitions.stage) {
        let s0 = op.src(0)?.and_then(|s| c.info.ast.as_operand(s)).filter(|o| o.ty == OperandType::Constant).map(|o| o.value);
        if let Some(v) = s0 {
            let own = match IoVariable::from_value(v) {
                IoVariable::SubgroupLaneId => Some(0),
                IoVariable::SubgroupEqMask | IoVariable::SubgroupGeMask | IoVariable::SubgroupLeMask => Some(1),
                IoVariable::SubgroupGtMask | IoVariable::SubgroupLtMask => Some(0),
                _ => None,
            };
            if let Some(own) = own {
                // Masks are read a component at a time; only bit 0 of component 0 exists.
                let component0 = op.sources.len() < 2
                    || op.sources[1].and_then(|s| c.info.ast.as_operand(s)).is_some_and(|o| o.ty == OperandType::Constant && o.value == 0);
                return Ok((A::U32, Some(c.const_u32_i(if component0 { own } else { 0 }))));
            }
        }
    }
    let (pointer, var_type) = storage_pointer(c, op)?;
    if is_store {
        let v = c.get(var_type, op.src(op.sources.len() - 1)?)?;
        c.m.store(&pointer, &v, None);
        Ok(INVALID)
    } else {
        let t = c.get_type(var_type, 1)?;
        let v = c.m.load(&t, &pointer, None);
        Ok((var_type, Some(bounded_cbuf_read(c, op, var_type, v)?)))
    }
}

/// HostCapabilities::cbuf_limits: a constant buffer read whose vec4 index is not below the binding's bound size (in the
/// support buffer) reads zero.
fn bounded_cbuf_read(c: &mut CodeGenContext, op: &Op, var_type: A, value: Inst) -> R<Inst> {
    use crate::translation::properties::{SUPPORT_BUFFER_BINDING, SUPPORT_CBUF_LIMITS_COUNT, SUPPORT_CBUF_LIMITS_FIELD};
    if !c.host.cbuf_limits || op.storage_kind != StorageKind::ConstantBuffer || op.sources.len() < 3 {
        return Ok(value);
    }
    let Some(binding) = constant_operand(c, op.src(0)?) else { return Ok(value) };
    if binding == SUPPORT_BUFFER_BINDING || !(0..SUPPORT_CBUF_LIMITS_COUNT).contains(&binding) {
        return Ok(value);
    }
    let Some(support) = c.constant_buffers.get(&SUPPORT_BUFFER_BINDING).cloned() else { return Ok(value) };
    let index = c.get(A::S32, op.src(2)?)?;
    let u = c.type_u32();
    let s = c.type_s32();
    let field = c.m.constant(&s, Lit::i32(SUPPORT_CBUF_LIMITS_FIELD));
    let element = c.m.constant(&s, Lit::i32(binding));
    let pt = c.m.type_pointer(StorageClass::UNIFORM, &u, false);
    let limit_ptr = c.m.access_chain(&pt, &support, &[field, element]);
    let limit = c.m.load(&u, &limit_ptr, None);
    let index_u = c.m.bitcast(&u, &index);
    let b = c.type_bool();
    let in_range = c.m.u_less_than(&b, &index_u, &limit);
    let t = c.get_type(var_type, 1)?;
    let zero = match var_type {
        A::FP32 => {
            let f = c.type_fp32();
            c.m.constant(&f, Lit::f32(0.0))
        }
        _ => c.m.constant(&t, Lit::i32(0)),
    };
    Ok(c.m.select(&t, &in_range, &value, &zero))
}

fn constant_operand(c: &CodeGenContext, node: Option<AstId>) -> Option<i32> {
    node.and_then(|n| c.info.ast.as_operand(n)).filter(|o| o.ty == OperandType::Constant).map(|o| o.value)
}

fn storage_pointer(c: &mut CodeGenContext, op: &Op) -> R<(Inst, A)> {
    let sk = op.storage_kind;
    let mut src_index = 0usize;
    let mut per_vertex_builtin = None;
    let (storage_class, var_type, base_obj) = match sk {
        StorageKind::ConstantBuffer | StorageKind::StorageBuffer => {
            let binding = constant_operand(c, op.src(src_index)?)
                .ok_or_else(|| format!("System.InvalidOperationException: First input of {} with {sk:?} storage must be a constant operand.", op.inst.name()))?;
            src_index += 1;
            let field_index = constant_operand(c, op.src(src_index)?)
                .ok_or_else(|| format!("System.InvalidOperationException: Second input of {} with {sk:?} storage must be a constant operand.", op.inst.name()))?;
            let props = c.properties;
            let buffer = if sk == StorageKind::ConstantBuffer { props.constant_buffers.get(&binding) } else { props.storage_buffers.get(&binding) }
                .ok_or("System.Collections.Generic.KeyNotFoundException")?;
            let field = buffer.ty.fields.get(field_index as usize).ok_or("System.IndexOutOfRangeException")?;
            let base = if sk == StorageKind::ConstantBuffer { c.constant_buffers.get(&binding) } else { c.storage_buffers.get(&binding) }
                .cloned()
                .ok_or("System.Collections.Generic.KeyNotFoundException")?;
            (StorageClass::UNIFORM, field.ty.element(), base)
        }
        StorageKind::LocalMemory | StorageKind::SharedMemory => {
            let id = constant_operand(c, op.src(src_index)?)
                .ok_or_else(|| format!("System.InvalidOperationException: First input of {} with {sk:?} storage must be a constant operand.", op.inst.name()))?;
            src_index += 1;
            let props = c.properties;
            if sk == StorageKind::LocalMemory {
                let t = props.local_memories.get(&id).ok_or("System.Collections.Generic.KeyNotFoundException")?.ty.element();
                (StorageClass::PRIVATE, t, c.local_memories[&id].clone())
            } else {
                let t = props.shared_memories.get(&id).ok_or("System.Collections.Generic.KeyNotFoundException")?.ty.element();
                (StorageClass::WORKGROUP, t, c.shared_memories[&id].clone())
            }
        }
        StorageKind::Input | StorageKind::InputPerPatch | StorageKind::Output | StorageKind::OutputPerPatch => {
            let var_id = constant_operand(c, op.src(src_index)?)
                .ok_or_else(|| format!("System.InvalidOperationException: First input of {} with {sk:?} storage must be a constant operand.", op.inst.name()))?;
            src_index += 1;
            let mut io_variable = IoVariable::from_value(var_id);
            let is_output = sk.is_output();
            let is_per_patch = sk.is_per_patch();
            let (mut location, mut component) = (0, 0);
            let d = c.definitions;
            if d.has_per_location_input_or_output(io_variable, is_output) {
                location = constant_operand(c, op.src(src_index)?)
                    .ok_or_else(|| format!("System.InvalidOperationException: Second input of {} with {sk:?} storage must be a constant operand.", op.inst.name()))?;
                src_index += 1;
                if op.sources.len() > src_index {
                    if let Some(e) = constant_operand(c, op.sources[src_index]) {
                        if d.has_per_location_input_or_output_component(io_variable, location, e, is_output) {
                            component = e;
                            src_index += 1;
                        }
                    }
                }
            }
            let mut var_type = if io_variable == IoVariable::UserDefined {
                d.user_defined_type(location, is_output)
            } else if io_variable == IoVariable::FragmentOutputColor {
                d.fragment_output_color_type(location)
            } else {
                let (_, t) = builtin_of(io_variable);
                if is_per_vertex_builtin(io_variable) {
                    per_vertex_builtin = Some(io_variable);
                    io_variable = IoVariable::Position;
                }
                t
            };
            var_type = var_type.element();
            let storage_class = if is_output { StorageClass::OUTPUT } else { StorageClass::INPUT };
            let io = IoDefinition { storage_kind: sk, io_variable, location, component };
            let dict = match (is_per_patch, is_output) {
                (true, true) => &c.outputs_per_patch,
                (true, false) => &c.inputs_per_patch,
                (false, true) => &c.outputs,
                (false, false) => &c.inputs,
            };
            let base = dict.get(&io).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
            (storage_class, var_type, base)
        }
        _ => return Err(format!("System.InvalidOperationException: Invalid storage kind {sk:?}.")),
    };

    let is_store_or_atomic = op.inst == I::STORE || op.inst.is_atomic();
    let mut inputs_count = (if is_store_or_atomic { op.sources.len() - 1 } else { op.sources.len() }) as i32 - src_index as i32;

    if let Some(pvb) = per_vertex_builtin {
        let field_index = per_vertex_struct_field_index(pvb)?;
        let mut indexes = Vec::new();
        if is_per_vertex_array_builtin(sk, c.definitions.stage) {
            indexes.push(c.get(A::S32, op.src(src_index)?)?);
            src_index += 1;
            let s = c.type_s32();
            indexes.push(c.m.constant(&s, Lit::i32(field_index)));
        } else {
            let s = c.type_s32();
            indexes.push(c.m.constant(&s, Lit::i32(field_index)));
        }
        while (indexes.len() as i32) < inputs_count + 1 {
            indexes.push(c.get(A::S32, op.src(src_index)?)?);
            src_index += 1;
        }
        let t = c.get_type(var_type, 1)?;
        let pt = c.m.type_pointer(storage_class, &t, false);
        return Ok((c.m.access_chain(&pt, &base_obj, &indexes), var_type));
    }

    if op.inst == I::ATOMIC_COMPARE_AND_SWAP {
        inputs_count -= 1;
    }
    let pointer = if inputs_count == 0 {
        base_obj
    } else {
        let mut indexes = Vec::with_capacity(inputs_count.max(0) as usize);
        for _ in 0..inputs_count {
            indexes.push(c.get(A::S32, op.src(src_index)?)?);
            src_index += 1;
        }
        let t = c.get_type(var_type, 1)?;
        let pt = c.m.type_pointer(storage_class, &t, false);
        c.m.access_chain(&pt, &base_obj, &indexes)
    };
    Ok((pointer, var_type))
}

fn scalar_input(c: &mut CodeGenContext, io_variable: IoVariable) -> R<Inst> {
    let (_, var_type) = builtin_of(io_variable);
    let var_type = var_type.element();
    let io = IoDefinition { storage_kind: StorageKind::Input, io_variable, location: 0, component: 0 };
    let t = c.get_type(var_type, 1)?;
    let v = c.inputs.get(&io).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
    Ok(c.m.load(&t, &v, None))
}

fn swizzled_result(c: &mut CodeGenContext, vector: &Inst, swizzled_result_type: A, mask: i32) -> R<Inst> {
    if swizzled_result_type.0 & A::ELEMENT_COUNT_MASK.0 != 0 {
        let components: Vec<Operand> = (0..4).filter(|i| mask & (1 << i) != 0).map(|i| Lit::i32(i).into()).collect();
        let t = c.get_type(swizzled_result_type, 1)?;
        Ok(c.m.vector_shuffle(&t, vector, vector, &components))
    } else {
        let component_index = (mask as u32).trailing_zeros() as i32;
        let t = c.get_type(swizzled_result_type, 1)?;
        Ok(c.m.composite_extract(&t, vector, &[Lit::i32(component_index).into()]))
    }
}

/// Sources of a texture operation, read in order (Src()).
struct Srcs<'o> {
    op: &'o Op,
    index: usize,
}

impl Srcs<'_> {
    fn next(&mut self, c: &mut CodeGenContext, ty: A) -> R<Inst> {
        let s = self.op.src(self.index)?;
        self.index += 1;
        c.get(ty, s)
    }
}

fn coords(c: &mut CodeGenContext, srcs: &mut Srcs, p_count: i32, ty: A) -> R<Inst> {
    if p_count > 1 {
        let mut elems = Vec::new();
        for _ in 0..p_count {
            elems.push(srcs.next(c, ty)?);
        }
        let st = if ty == A::S32 { c.type_s32() } else { c.type_fp32() };
        let vt = c.m.type_vector(&st, Lit::i32(p_count), false);
        Ok(c.m.composite_construct(&vt, &elems))
    } else {
        srcs.next(c, ty)
    }
}

fn image_atomic(c: &mut CodeGenContext, op: &Op) -> Res {
    let tex = op.tex.ok_or("System.InvalidCastException")?;
    let component_type = tex.format.component_type();
    let is_array = tex.ty.has(SamplerType::ARRAY);
    let mut srcs = Srcs { op, index: 0 };
    let declaration = c.images.get(&tex.texture_set_and_binding()).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
    let mut image = declaration.image.clone();
    let result_type = c.get_type(component_type, 1)?;
    let image_pointer_type = c.m.type_pointer(StorageClass::IMAGE, &result_type, false);
    if declaration.is_indexed {
        let texture_index = srcs.next(c, A::S32)?;
        image = c.m.access_chain(&image_pointer_type, &image, &[texture_index]);
        mark_non_uniform(c, &image);
    }
    let p_count = tex.ty.dimensions() + is_array as i32;
    let p_coords = coords(c, &mut srcs, p_count, A::S32)?;
    let value = srcs.next(c, component_type)?;
    let z = c.const_u32_i(0);
    let pointer = c.m.image_texel_pointer(&image_pointer_type, &image, &p_coords, &z);
    let one = c.const_u32_i(1);
    let zero = c.const_u32_i(0);
    let s32 = component_type == A::S32;
    let atomic_op = TextureFlags(tex.flags.0 & TextureFlags::ATOMIC_MASK.0);
    // The compare value is read right before the instruction, as the original's argument list does.
    let compare = if atomic_op == TextureFlags::CAS { Some(srcs.next(c, component_type)?) } else { None };
    let m = &mut c.m;
    let result = match atomic_op {
        TextureFlags::MINIMUM if s32 => m.atomic_s_min(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::MINIMUM => m.atomic_u_min(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::MAXIMUM if s32 => m.atomic_s_max(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::MAXIMUM => m.atomic_u_max(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::INCREMENT => m.atomic_i_increment(&result_type, &pointer, &one, &zero),
        TextureFlags::DECREMENT => m.atomic_i_decrement(&result_type, &pointer, &one, &zero),
        TextureFlags::BITWISE_AND => m.atomic_and(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::BITWISE_OR => m.atomic_or(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::BITWISE_XOR => m.atomic_xor(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::SWAP => m.atomic_exchange(&result_type, &pointer, &one, &zero, &value),
        TextureFlags::CAS => m.atomic_compare_exchange(&result_type, &pointer, &one, &zero, &zero, compare.as_ref().unwrap(), &value),
        _ => m.atomic_i_add(&result_type, &pointer, &one, &zero, &value),
    };
    Ok((component_type, Some(result)))
}

fn mark_non_uniform(c: &mut CodeGenContext, inst: &Inst) {
    if c.host.supports_shader_non_uniform_indexing {
        if !c.was_non_uniform_access_declared {
            c.m.add_extension("SPV_EXT_descriptor_indexing");
            c.m.add_capability(Capability::SHADER_NON_UNIFORM);
            c.m.add_capability(Capability::SAMPLED_IMAGE_ARRAY_NON_UNIFORM_INDEXING);
            c.m.add_capability(Capability::STORAGE_IMAGE_ARRAY_NON_UNIFORM_INDEXING);
        }
        c.m.decorate(inst, Decoration::NON_UNIFORM, &[]);
        c.was_non_uniform_access_declared = true;
    }
}

fn load_image(c: &mut CodeGenContext, srcs: &mut Srcs, tex: &AstTexture) -> R<Inst> {
    let declaration = c.images.get(&tex.texture_set_and_binding()).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
    let mut image = declaration.image.clone();
    if declaration.is_indexed {
        let texture_index = srcs.next(c, A::S32)?;
        image = c.m.access_chain(&declaration.image_pointer_type, &image, &[texture_index]);
        mark_non_uniform(c, &image);
    }
    let image = c.m.load(&declaration.image_type, &image, None);
    if declaration.is_indexed {
        mark_non_uniform(c, &image);
    }
    Ok(image)
}

fn image_load(c: &mut CodeGenContext, op: &Op) -> Res {
    let tex = op.tex.ok_or("System.InvalidCastException")?;
    let component_type = tex.format.component_type();
    let is_array = tex.ty.has(SamplerType::ARRAY);
    let mut srcs = Srcs { op, index: 0 };
    let image = load_image(c, &mut srcs, &tex)?;
    let p_count = tex.ty.dimensions() + is_array as i32;
    let p_coords = coords(c, &mut srcs, p_count, A::S32)?;
    let image_component_type = c.get_type(component_type, 1)?;
    let swizzled_result_type = op.vector_type(component_type);
    let vt = c.m.type_vector(&image_component_type, Lit::i32(4), false);
    let texel = c.m.image_read(&vt, &image, &p_coords, ImageOperandsMask::MASK_NONE, &[]);
    let result = swizzled_result(c, &texel, swizzled_result_type, op.index)?;
    Ok((component_type, Some(result)))
}

fn image_store(c: &mut CodeGenContext, op: &Op) -> Res {
    let tex = op.tex.ok_or("System.InvalidCastException")?;
    let is_array = tex.ty.has(SamplerType::ARRAY);
    let mut srcs = Srcs { op, index: 0 };
    let image = load_image(c, &mut srcs, &tex)?;
    let p_count = tex.ty.dimensions() + is_array as i32;
    let p_coords = coords(c, &mut srcs, p_count, A::S32)?;
    let component_type = tex.format.component_type();
    let mut c_elems = Vec::new();
    for _ in 0..4 {
        if srcs.index < op.sources.len() {
            c_elems.push(srcs.next(c, component_type)?);
        } else {
            c_elems.push(match component_type {
                A::S32 => {
                    let t = c.type_s32();
                    c.m.constant(&t, Lit::i32(0))
                }
                A::U32 => {
                    let t = c.type_u32();
                    c.m.constant(&t, Lit::u32(0))
                }
                _ => {
                    let t = c.type_fp32();
                    c.m.constant(&t, Lit::f32(0.0))
                }
            });
        }
    }
    let ct = c.get_type(component_type, 1)?;
    let vt = c.m.type_vector(&ct, Lit::i32(4), false);
    let texel = c.m.composite_construct(&vt, &c_elems);
    c.m.image_write(&image, &p_coords, &texel, ImageOperandsMask::MASK_NONE, &[]);
    Ok(INVALID)
}

fn lod(c: &mut CodeGenContext, op: &Op) -> Res {
    let tex = op.tex.ok_or("System.InvalidCastException")?;
    let mut srcs = Srcs { op, index: 0 };
    let declaration = c.samplers.get(&tex.texture_set_and_binding()).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
    let image = sampled_image_load(c, &tex, &declaration, &mut srcs)?;
    let p_coords = coords(c, &mut srcs, tex.ty.dimensions(), A::FP32)?;
    let f = c.type_fp32();
    let result_type = c.m.type_vector(&f, Lit::i32(2), false);
    let packed = c.m.image_query_lod(&result_type, &image, &p_coords);
    let f = c.type_fp32();
    Ok((A::FP32, Some(c.m.composite_extract(&f, &packed, &[Lit::i32(op.index).into()]))))
}

fn sampled_image_load(c: &mut CodeGenContext, tex: &AstTexture, declaration: &super::SamplerDeclaration, srcs: &mut Srcs) -> R<Inst> {
    let mut image = declaration.image.clone();
    let image_indexed = declaration.is_indexed;
    let mut element = None;
    if image_indexed {
        let texture_index = srcs.next(c, A::S32)?;
        element = Some(texture_index.clone());
        image = c.m.access_chain(&declaration.sampled_image_pointer_type, &image, &[texture_index]);
        mark_non_uniform(c, &image);
    }
    if let Some(first_slot) = declaration.table_slot {
        // Separate image, sampler from the table at the index the host wrote for this texture slot.
        image = c.m.load(&declaration.image_type, &image, None);
        if image_indexed {
            mark_non_uniform(c, &image);
        }
        use crate::translation::properties::{SUPPORT_BUFFER_BINDING, SUPPORT_SAMPLER_SLOTS_FIELD};
        let (table, sampler_type, sampler_pointer) = c.sampler_table.clone().ok_or("sampler table")?;
        let support = c.constant_buffers.get(&SUPPORT_BUFFER_BINDING).cloned().ok_or("support buffer")?;
        let s = c.type_s32();
        let u = c.type_u32();
        let mut slot = c.m.constant(&s, Lit::i32(first_slot));
        if let Some(element) = &element {
            slot = c.m.i_add(&s, &slot, element);
        }
        let field = c.m.constant(&s, Lit::i32(SUPPORT_SAMPLER_SLOTS_FIELD));
        let pt = c.m.type_pointer(StorageClass::UNIFORM, &u, false);
        let p = c.m.access_chain(&pt, &support, &[field, slot]);
        let index = c.m.load(&u, &p, None);
        let sp = c.m.access_chain(&sampler_pointer, &table, &[index]);
        let sampler = c.m.load(&sampler_type, &sp, None);
        image = c.m.sampled_image(&declaration.sampled_image_type, &image, &sampler);
        if image_indexed {
            mark_non_uniform(c, &image);
        }
    } else if tex.is_separate() {
        image = c.m.load(&declaration.image_type, &image, None);
        if image_indexed {
            mark_non_uniform(c, &image);
        }
        let sampler_declaration =
            c.samplers.get(&tex.sampler_set_and_binding()).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
        let mut sampler = sampler_declaration.image.clone();
        let sampler_indexed = sampler_declaration.is_indexed;
        if sampler_indexed {
            let sampler_index = srcs.next(c, A::S32)?;
            sampler = c.m.access_chain(&sampler_declaration.sampled_image_pointer_type, &sampler, &[sampler_index]);
            mark_non_uniform(c, &sampler);
        }
        sampler = c.m.load(&sampler_declaration.image_type, &sampler, None);
        if sampler_indexed {
            mark_non_uniform(c, &sampler);
        }
        image = c.m.sampled_image(&declaration.sampled_image_type, &image, &sampler);
        if image_indexed || sampler_indexed {
            mark_non_uniform(c, &image);
        }
    } else {
        image = c.m.load(&declaration.sampled_image_type, &image, None);
        if image_indexed {
            mark_non_uniform(c, &image);
        }
    }
    Ok(image)
}

fn texture_sample(c: &mut CodeGenContext, op: &Op) -> Res {
    let tex = op.tex.ok_or("System.InvalidCastException")?;
    let f = tex.flags;
    let is_gather = f.has(TextureFlags::GATHER);
    let has_derivatives = f.has(TextureFlags::DERIVATIVES);
    let int_coords = f.has(TextureFlags::INT_COORDS);
    let mut has_lod_bias = f.has(TextureFlags::LOD_BIAS);
    let mut has_lod_level = f.has(TextureFlags::LOD_LEVEL);
    let has_offset = f.has(TextureFlags::OFFSET);
    let has_offsets = f.has(TextureFlags::OFFSETS);
    let is_array = tex.ty.has(SamplerType::ARRAY);
    let is_multisample = tex.ty.has(SamplerType::MULTISAMPLE);
    let is_shadow = tex.ty.has(SamplerType::SHADOW);

    let mut srcs = Srcs { op, index: 0 };
    let declaration = c.samplers.get(&tex.texture_set_and_binding()).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
    let mut image = sampled_image_load(c, &tex, &declaration, &mut srcs)?;

    let coords_count = tex.ty.dimensions();
    let mut p_count = coords_count;
    let mut array_index_elem = -1;
    if is_array {
        array_index_elem = p_count;
        p_count += 1;
    }
    let coord_type = if int_coords { A::S32 } else { A::FP32 };

    let p_coords = if p_count > 1 {
        let mut elems = Vec::new();
        for index in 0..p_count {
            if array_index_elem == index {
                let mut e = srcs.next(c, A::S32)?;
                if !int_coords {
                    let t = c.type_fp32();
                    e = c.m.convert_s_to_f(&t, &e);
                }
                elems.push(e);
            } else {
                elems.push(srcs.next(c, coord_type)?);
            }
        }
        let st = if int_coords { c.type_s32() } else { c.type_fp32() };
        let vt = c.m.type_vector(&st, Lit::i32(p_count), false);
        c.m.composite_construct(&vt, &elems)
    } else {
        srcs.next(c, coord_type)?
    };

    let d_ref = if is_shadow { Some(srcs.next(c, A::FP32)?) } else { None };
    let derivatives = if has_derivatives {
        let dpdx = coords(c, &mut srcs, coords_count, A::FP32)?;
        let dpdy = coords(c, &mut srcs, coords_count, A::FP32)?;
        Some((dpdx, dpdy))
    } else {
        None
    };
    let mut sample = None;
    let mut lod = None;
    if is_multisample {
        sample = Some(srcs.next(c, A::S32)?);
    } else if has_lod_level {
        lod = Some(srcs.next(c, coord_type)?);
    }

    fn offset_vector(c: &mut CodeGenContext, srcs: &mut Srcs, count: i32) -> R<Inst> {
        if count > 1 {
            let mut elems = Vec::new();
            for _ in 0..count {
                elems.push(srcs.next(c, A::S32)?);
            }
            let s = c.type_s32();
            let vt = c.m.type_vector(&s, Lit::i32(count), false);
            Ok(c.m.constant_composite(&vt, &elems))
        } else {
            srcs.next(c, A::S32)
        }
    }

    let mut offsets = Vec::new();
    if has_offset {
        offsets.push(offset_vector(c, &mut srcs, coords_count)?);
    } else if has_offsets {
        for _ in 0..4 {
            offsets.push(offset_vector(c, &mut srcs, coords_count)?);
        }
    }
    let mut lod_bias = if has_lod_bias { Some(srcs.next(c, A::FP32)?) } else { None };

    if !is_gather && !int_coords && !is_multisample && !has_lod_level && !has_derivatives && c.definitions.stage != ShaderStage::Fragment {
        // Implicit LOD exists only in fragment shaders: the bias, if any, is the explicit LOD.
        lod = Some(match lod_bias {
            Some(b) => b,
            None => {
                let t = c.type_fp32();
                c.m.constant(&t, Lit::f32(0.0))
            }
        });
        lod_bias = None;
        has_lod_bias = false;
        has_lod_level = true;
    }

    // textureGather's component index (not for shadow samplers).
    let comp_idx = if is_gather && !is_shadow { Some(srcs.next(c, A::S32)?) } else { None };

    let mut operands: Vec<Inst> = Vec::new();
    let mut mask = 0u32;
    if has_lod_bias {
        mask |= ImageOperandsMask::BIAS.0;
        operands.push(lod_bias.clone().ok_or(NRE)?);
    }
    if !is_multisample && has_lod_level {
        mask |= ImageOperandsMask::LOD.0;
        operands.push(lod.clone().ok_or(NRE)?);
    }
    if let Some((dx, dy)) = &derivatives {
        mask |= ImageOperandsMask::GRAD.0;
        operands.push(dx.clone());
        operands.push(dy.clone());
    }
    if has_offset {
        mask |= ImageOperandsMask::CONST_OFFSET.0;
        operands.push(offsets[0].clone());
    } else if has_offsets {
        mask |= ImageOperandsMask::CONST_OFFSETS.0;
        let s = c.type_s32();
        let v2 = c.m.type_vector(&s, Lit::i32(2), false);
        let len = c.const_u32_i(4);
        let arrayv2 = c.m.type_array(&v2, &len, false);
        operands.push(c.m.constant_composite(&arrayv2, &offsets));
    }
    if is_multisample {
        mask |= ImageOperandsMask::SAMPLE.0;
        operands.push(sample.clone().ok_or(NRE)?);
    }
    let operands_mask = ImageOperandsMask(mask);

    let color_is_vector = is_gather || !is_shadow;
    let result_type = if color_is_vector {
        let f = c.type_fp32();
        c.m.type_vector(&f, Lit::i32(4), false)
    } else {
        c.type_fp32()
    };
    if int_coords {
        image = c.m.image(&declaration.image_type, &image);
    }
    let m = &mut c.m;
    let mut result = if int_coords {
        m.image_fetch(&result_type, &image, &p_coords, operands_mask, &operands)
    } else if is_gather {
        if is_shadow {
            m.image_dref_gather(&result_type, &image, &p_coords, d_ref.as_ref().ok_or(NRE)?, operands_mask, &operands)
        } else {
            m.image_gather(&result_type, &image, &p_coords, comp_idx.as_ref().ok_or(NRE)?, operands_mask, &operands)
        }
    } else if is_shadow {
        if has_lod_level {
            m.image_sample_dref_explicit_lod(&result_type, &image, &p_coords, d_ref.as_ref().ok_or(NRE)?, operands_mask, &operands)
        } else {
            m.image_sample_dref_implicit_lod(&result_type, &image, &p_coords, d_ref.as_ref().ok_or(NRE)?, operands_mask, &operands)
        }
    } else if has_derivatives || has_lod_level {
        m.image_sample_explicit_lod(&result_type, &image, &p_coords, operands_mask, &operands)
    } else {
        m.image_sample_implicit_lod(&result_type, &image, &p_coords, operands_mask, &operands)
    };
    let mut swizzled_result_type = A::FP32;
    if color_is_vector {
        swizzled_result_type = op.vector_type(swizzled_result_type);
        result = swizzled_result(c, &result, swizzled_result_type, op.index)?;
    }
    Ok((swizzled_result_type, Some(result)))
}

fn texture_query_samples(c: &mut CodeGenContext, op: &Op) -> Res {
    let tex = op.tex.ok_or("System.InvalidCastException")?;
    let mut srcs = Srcs { op, index: 0 };
    let declaration = c.samplers.get(&tex.texture_set_and_binding()).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
    let image = sampled_image_load(c, &tex, &declaration, &mut srcs)?;
    let image = c.m.image(&declaration.image_type, &image);
    let s = c.type_s32();
    Ok((A::S32, Some(c.m.image_query_samples(&s, &image))))
}

fn texture_query_size(c: &mut CodeGenContext, op: &Op) -> Res {
    let tex = op.tex.ok_or("System.InvalidCastException")?;
    let mut srcs = Srcs { op, index: 0 };
    let declaration = c.samplers.get(&tex.texture_set_and_binding()).cloned().ok_or("System.Collections.Generic.KeyNotFoundException")?;
    let image = sampled_image_load(c, &tex, &declaration, &mut srcs)?;
    let image = c.m.image(&declaration.image_type, &image);
    if op.index == 3 {
        let s = c.type_s32();
        return Ok((A::S32, Some(c.m.image_query_levels(&s, &image))));
    }
    let ty = *c.samplers_types.get(&tex.texture_set_and_binding()).ok_or("System.Collections.Generic.KeyNotFoundException")?;
    let has_lod = !ty.has(SamplerType::MULTISAMPLE) && ty != SamplerType::TEXTURE_BUFFER;
    let mut dimensions = if ty.base() == SamplerType::TEXTURE_CUBE { 2 } else { ty.dimensions() };
    if ty.has(SamplerType::ARRAY) {
        dimensions += 1;
    }
    let result_type = if dimensions == 1 {
        c.type_s32()
    } else {
        let s = c.type_s32();
        c.m.type_vector(&s, Lit::i32(dimensions), false)
    };
    let mut result = if has_lod {
        let lod = c.get_s32(op.src(srcs.index)?)?;
        c.m.image_query_size_lod(&result_type, &image, &lod)
    } else {
        c.m.image_query_size(&result_type, &image)
    };
    if dimensions != 1 {
        let s = c.type_s32();
        result = c.m.composite_extract(&s, &result, &[Lit::i32(op.index).into()]);
    }
    Ok((A::S32, Some(result)))
}

// Generic forms.

fn no_contraction(c: &mut CodeGenContext, op: &Op, r: &Inst) {
    if !c.host.reduced_precision || op.force_precise {
        c.m.decorate(r, Decoration::NO_CONTRACTION, &[]);
    }
}

fn unary(c: &mut CodeGenContext, op: &Op, emit_f: Un, emit_i: Option<Un>) -> Res {
    let source = op.src(0)?;
    if op.has(I::FP64) {
        let t = c.type_fp64();
        let v = c.get_fp64(source)?;
        Ok((A::FP64, Some(emit_f(&mut c.m, &t, &v))))
    } else if op.has(I::FP32) {
        let t = c.type_fp32();
        let v = c.get_fp32(source)?;
        Ok((A::FP32, Some(emit_f(&mut c.m, &t, &v))))
    } else {
        let t = c.type_s32();
        let v = c.get_s32(source)?;
        let emit = emit_i.ok_or(NRE)?;
        Ok((A::S32, Some(emit(&mut c.m, &t, &v))))
    }
}

fn unary_fp32(c: &mut CodeGenContext, op: &Op, emit: Un) -> Res {
    let t = c.type_fp32();
    let v = c.get_fp32(op.src(0)?)?;
    Ok((A::FP32, Some(emit(&mut c.m, &t, &v))))
}

fn unary_s32(c: &mut CodeGenContext, op: &Op, emit: Un) -> Res {
    let t = c.type_s32();
    let v = c.get_s32(op.src(0)?)?;
    Ok((A::S32, Some(emit(&mut c.m, &t, &v))))
}

fn binary(c: &mut CodeGenContext, op: &Op, emit_f: Bin, emit_i: Option<Bin>) -> Res {
    let (s1, s2) = (op.src(0)?, op.src(1)?);
    if op.has(I::FP64) {
        let t = c.type_fp64();
        let (a, b) = (c.get_fp64(s1)?, c.get_fp64(s2)?);
        let r = emit_f(&mut c.m, &t, &a, &b);
        no_contraction(c, op, &r);
        Ok((A::FP64, Some(r)))
    } else if op.has(I::FP32) {
        let t = c.type_fp32();
        let (a, b) = (c.get_fp32(s1)?, c.get_fp32(s2)?);
        let r = emit_f(&mut c.m, &t, &a, &b);
        no_contraction(c, op, &r);
        Ok((A::FP32, Some(r)))
    } else {
        let t = c.type_s32();
        let (a, b) = (c.get_s32(s1)?, c.get_s32(s2)?);
        let emit = emit_i.ok_or(NRE)?;
        Ok((A::S32, Some(emit(&mut c.m, &t, &a, &b))))
    }
}

fn binary_bool(c: &mut CodeGenContext, op: &Op, emit: Bin) -> Res {
    let (s1, s2) = (op.src(0)?, op.src(1)?);
    let t = c.type_bool();
    let (a, b) = (c.get(A::BOOL, s1)?, c.get(A::BOOL, s2)?);
    Ok((A::BOOL, Some(emit(&mut c.m, &t, &a, &b))))
}

fn binary_s32(c: &mut CodeGenContext, op: &Op, emit: Bin) -> Res {
    let (s1, s2) = (op.src(0)?, op.src(1)?);
    let t = c.type_s32();
    let (a, b) = (c.get_s32(s1)?, c.get_s32(s2)?);
    Ok((A::S32, Some(emit(&mut c.m, &t, &a, &b))))
}

fn binary_u32(c: &mut CodeGenContext, op: &Op, emit: Bin) -> Res {
    let (s1, s2) = (op.src(0)?, op.src(1)?);
    let t = c.type_u32();
    let (a, b) = (c.get_u32(s1)?, c.get_u32(s2)?);
    Ok((A::U32, Some(emit(&mut c.m, &t, &a, &b))))
}

fn ternary(c: &mut CodeGenContext, op: &Op, emit_f: Ter, emit_i: Option<Ter>) -> Res {
    let (s1, s2, s3) = (op.src(0)?, op.src(1)?, op.src(2)?);
    if op.has(I::FP64) {
        let t = c.type_fp64();
        let (a, b, d) = (c.get_fp64(s1)?, c.get_fp64(s2)?, c.get_fp64(s3)?);
        let r = emit_f(&mut c.m, &t, &a, &b, &d);
        no_contraction(c, op, &r);
        Ok((A::FP64, Some(r)))
    } else if op.has(I::FP32) {
        let t = c.type_fp32();
        let (a, b, d) = (c.get_fp32(s1)?, c.get_fp32(s2)?, c.get_fp32(s3)?);
        let r = emit_f(&mut c.m, &t, &a, &b, &d);
        no_contraction(c, op, &r);
        Ok((A::FP32, Some(r)))
    } else {
        let t = c.type_s32();
        let (a, b, d) = (c.get_s32(s1)?, c.get_s32(s2)?, c.get_s32(s3)?);
        let emit = emit_i.ok_or(NRE)?;
        Ok((A::S32, Some(emit(&mut c.m, &t, &a, &b, &d))))
    }
}

fn ternary_u32(c: &mut CodeGenContext, op: &Op, emit: Ter) -> Res {
    let (s1, s2, s3) = (op.src(0)?, op.src(1)?, op.src(2)?);
    let t = c.type_u32();
    let (a, b, d) = (c.get_u32(s1)?, c.get_u32(s2)?, c.get_u32(s3)?);
    Ok((A::U32, Some(emit(&mut c.m, &t, &a, &b, &d))))
}

fn bitfield_extract_s32(c: &mut CodeGenContext, op: &Op) -> Res {
    let (s1, s2, s3) = (op.src(0)?, op.src(1)?, op.src(2)?);
    let t = c.type_s32();
    let (a, b, d) = (c.get_s32(s1)?, c.get_u32(s2)?, c.get_u32(s3)?);
    Ok((A::S32, Some(c.m.bit_field_s_extract(&t, &a, &b, &d))))
}

fn bitfield_insert(c: &mut CodeGenContext, op: &Op) -> Res {
    let (s1, s2, s3, s4) = (op.src(0)?, op.src(1)?, op.src(2)?, op.src(3)?);
    let t = c.type_u32();
    let (a, b, d, e) = (c.get_u32(s1)?, c.get_u32(s2)?, c.get_u32(s3)?, c.get_u32(s4)?);
    Ok((A::U32, Some(c.m.bit_field_insert(&t, &a, &b, &d, &e))))
}
