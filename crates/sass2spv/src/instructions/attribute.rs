// Attribute loads and stores, interpolation and geometry output (InstEmitAttribute, AttributeMap).

use std::collections::HashMap;
use std::sync::OnceLock;

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::decoders::register::{Register, RegisterType, REGISTER_ZERO_INDEX};
use crate::ir::{Instruction as I, IoVariable, OperandId, StorageKind};
use crate::translation::definitions::{attribute_consts as ac, AggregateType, PixelImap, MAX_ATTRIBUTES};
use crate::translation::emitter::{Emit, EmitterContext as Ctx};
use crate::types::{AttributeType, ShaderStage};

mod stages {
    use crate::types::ShaderStage as S;
    pub const NONE: u8 = 0;
    pub const VERTEX: u8 = 1 << S::Vertex as u8;
    pub const TESS_CONTROL: u8 = 1 << S::TessellationControl as u8;
    pub const TESS_EVAL: u8 = 1 << S::TessellationEvaluation as u8;
    pub const GEOMETRY: u8 = 1 << S::Geometry as u8;
    pub const FRAGMENT: u8 = 1 << S::Fragment as u8;
    pub const TESSELLATION: u8 = TESS_CONTROL | TESS_EVAL;
    pub const VTG: u8 = VERTEX | TESSELLATION | GEOMETRY;
    pub const TGF: u8 = TESSELLATION | GEOMETRY | FRAGMENT;
    pub const ALL_GRAPHICS: u8 = VERTEX | TESSELLATION | GEOMETRY | FRAGMENT;
}

#[derive(Clone, Copy)]
struct AttributeEntry {
    base_offset: i32,
    ty: AggregateType,
    io: IoVariable,
    input_mask: u8,
    output_mask: u8,
}

type Map = HashMap<i32, AttributeEntry>;

fn element_count(t: AggregateType) -> i32 {
    match t.count() {
        AggregateType::VECTOR2 => 2,
        AggregateType::VECTOR3 => 3,
        AggregateType::VECTOR4 => 4,
        _ => 1,
    }
}

#[allow(clippy::too_many_arguments)]
fn add(map: &mut Map, mut offset: i32, ty: AggregateType, io: IoVariable, input_mask: u8, output_mask: u8, count: i32, upper_bound: i32) {
    let base_offset = offset;
    for _ in 0..count {
        for _ in 0..element_count(ty) {
            map.insert(offset, AttributeEntry { base_offset, ty, io, input_mask, output_mask });
            offset += 4;
            if offset >= upper_bound {
                return;
            }
        }
    }
}

fn attributes() -> &'static Map {
    static MAP: OnceLock<Map> = OnceLock::new();
    MAP.get_or_init(|| {
        use stages::*;
        use AggregateType as A;
        use IoVariable as V;
        let mut m = Map::new();
        let v4 = A::VECTOR4 | A::FP32;
        add(&mut m, 0x060, A::S32, V::PrimitiveId, TGF, GEOMETRY, 1, 0x400);
        add(&mut m, 0x064, A::S32, V::Layer, FRAGMENT, VTG, 1, 0x400);
        add(&mut m, 0x068, A::S32, V::ViewportIndex, FRAGMENT, VTG, 1, 0x400);
        add(&mut m, 0x06c, A::FP32, V::PointSize, NONE, VTG, 1, 0x400);
        add(&mut m, 0x070, v4, V::Position, TGF, VTG, 1, 0x400);
        add(&mut m, 0x080, v4, V::UserDefined, ALL_GRAPHICS, VTG, 32, 0x400);
        add(&mut m, 0x280, v4, V::FrontColorDiffuse, TGF, VTG, 1, 0x400);
        add(&mut m, 0x290, v4, V::FrontColorSpecular, TGF, VTG, 1, 0x400);
        add(&mut m, 0x2a0, v4, V::BackColorDiffuse, TGF, VTG, 1, 0x400);
        add(&mut m, 0x2b0, v4, V::BackColorSpecular, TGF, VTG, 1, 0x400);
        add(&mut m, 0x2c0, A::ARRAY | A::FP32, V::ClipDistance, TGF, VTG, 8, 0x400);
        add(&mut m, 0x2e0, A::VECTOR2 | A::FP32, V::PointCoord, FRAGMENT, NONE, 1, 0x400);
        add(&mut m, 0x2e8, A::FP32, V::FogCoord, TGF, VTG, 1, 0x400);
        add(&mut m, 0x2f0, A::VECTOR2 | A::FP32, V::TessellationCoord, TESS_EVAL, NONE, 1, 0x400);
        add(&mut m, 0x2f8, A::S32, V::InstanceId, VERTEX, NONE, 1, 0x400);
        add(&mut m, 0x2fc, A::S32, V::VertexId, VERTEX, NONE, 1, 0x400);
        add(&mut m, 0x300, v4, V::TextureCoord, TGF, VTG, 1, 0x400);
        add(&mut m, 0x3a0, A::ARRAY | A::S32, V::ViewportMask, FRAGMENT, VTG, 1, 0x400);
        add(&mut m, 0x3fc, A::BOOL, V::FrontFacing, FRAGMENT, NONE, 1, 0x400);
        m
    })
}

fn attributes_per_patch() -> &'static Map {
    static MAP: OnceLock<Map> = OnceLock::new();
    MAP.get_or_init(|| {
        use stages::*;
        use AggregateType as A;
        let mut m = Map::new();
        add(&mut m, 0x000, A::VECTOR4 | A::FP32, IoVariable::TessellationLevelOuter, TESS_EVAL, TESS_CONTROL, 1, 0x400);
        add(&mut m, 0x010, A::VECTOR2 | A::FP32, IoVariable::TessellationLevelInner, TESS_EVAL, TESS_CONTROL, 1, 0x400);
        add(&mut m, 0x018, A::VECTOR4 | A::FP32, IoVariable::UserDefined, TESS_EVAL, TESS_CONTROL, 31, 0x200);
        m
    })
}

fn stage_bit(stage: ShaderStage) -> u8 {
    1 << stage as u8
}

fn is_supported_by_host(c: &Ctx, stage: ShaderStage, io: IoVariable) -> bool {
    if io == IoVariable::ViewportIndex && stage != ShaderStage::Geometry && stage != ShaderStage::Fragment {
        return c.tc.gpu.host_supports_viewport_index_vertex_tessellation();
    }
    if io == IoVariable::ViewportMask {
        return c.tc.gpu.host_supports_viewport_mask();
    }
    true
}

fn io_variable(stage: ShaderStage, e: &AttributeEntry) -> IoVariable {
    if e.io == IoVariable::Position && stage == ShaderStage::Fragment { IoVariable::FragmentCoord } else { e.io }
}

fn attribute_type(c: &Ctx, is_output: bool, inner_index: i32, e: &AttributeEntry) -> AggregateType {
    if e.io == IoVariable::UserDefined {
        c.tc.definitions.user_defined_type(inner_index / 4, is_output)
    } else if e.io == IoVariable::FragmentOutputColor {
        c.tc.definitions.fragment_output_color_type(inner_index / 4)
    } else {
        e.ty
    }
}

pub fn has_primitive_vertex_stage(stage: ShaderStage, is_output: bool) -> bool {
    !is_output && matches!(stage, ShaderStage::TessellationControl | ShaderStage::TessellationEvaluation | ShaderStage::Geometry)
}

pub fn has_invocation_id(stage: ShaderStage, is_output: bool) -> bool {
    is_output && stage == ShaderStage::TessellationControl
}

pub fn generate_attribute_load(c: &mut Ctx, mut prim_vertex: Option<OperandId>, offset: i32, is_output: bool, is_per_patch: bool) -> OperandId {
    let map = if is_per_patch { attributes_per_patch() } else { attributes() };
    let stage = c.stage();
    let Some(entry) = map.get(&offset).copied() else {
        c.log(&format!("Attribute offset 0x{offset:X} is not valid."));
        return c.const_i(0);
    };
    let valid = if is_output { entry.output_mask } else { entry.input_mask };
    if stage_bit(stage) & valid == 0 {
        c.log(&format!("Attribute offset 0x{offset:X} ({:?}) is not valid for stage {stage}.", entry.io));
        return c.const_i(0);
    }
    if !is_supported_by_host(c, stage, entry.io) {
        c.log(&format!("Attribute offset 0x{offset:X} ({:?}) is not supported by the host for stage {stage}.", entry.io));
        return c.const_i(0);
    }
    if has_invocation_id(stage, is_output) && !is_per_patch {
        prim_vertex = Some(c.load_io(StorageKind::Input, IoVariable::InvocationId, None, None));
    }
    let inner_index = (offset - entry.base_offset) / 4;
    let sk = match (is_per_patch, is_output) {
        (true, true) => StorageKind::OutputPerPatch,
        (true, false) => StorageKind::InputPerPatch,
        (false, true) => StorageKind::Output,
        (false, false) => StorageKind::Input,
    };
    let io = io_variable(stage, &entry);
    let ty = attribute_type(c, is_output, inner_index, &entry);
    let elements = element_count(ty);
    let is_array = ty.has(AggregateType::ARRAY);
    let has_array_index = is_array || c.tc.definitions.has_per_location_input_or_output(io, is_output);
    let has_element_index = elements > 1;

    if has_array_index && has_element_index {
        let array_index = inner_index / elements;
        let element_index = inner_index - array_index * elements;
        let (ai, ei) = (c.const_i(array_index), c.const_i(element_index));
        match prim_vertex {
            Some(pv) if !is_array => c.load_io3(sk, io, Some(ai), pv, ei),
            _ => c.load_io3(sk, io, prim_vertex, ai, ei),
        }
    } else if has_array_index || has_element_index {
        let ii = c.const_i(inner_index);
        match prim_vertex {
            Some(pv) if !is_array && has_array_index => c.load_io2(sk, io, Some(ii), pv),
            _ => c.load_io2(sk, io, prim_vertex, ii),
        }
    } else {
        c.load_io(sk, io, prim_vertex, None)
    }
}

pub fn generate_attribute_store(c: &mut Ctx, offset: i32, is_per_patch: bool, value: OperandId) {
    let map = if is_per_patch { attributes_per_patch() } else { attributes() };
    let stage = c.stage();
    let Some(entry) = map.get(&offset).copied() else {
        c.log(&format!("Attribute offset 0x{offset:X} is not valid."));
        return;
    };
    if stage_bit(stage) & entry.output_mask == 0 {
        c.log(&format!("Attribute offset 0x{offset:X} ({:?}) is not valid for stage {stage}.", entry.io));
        return;
    }
    if !is_supported_by_host(c, stage, entry.io) {
        c.log(&format!("Attribute offset 0x{offset:X} ({:?}) is not supported by the host for stage {stage}.", entry.io));
        return;
    }
    let invocation_id = if has_invocation_id(stage, true) && !is_per_patch {
        Some(c.load_io(StorageKind::Input, IoVariable::InvocationId, None, None))
    } else {
        None
    };
    let inner_index = (offset - entry.base_offset) / 4;
    let sk = if is_per_patch { StorageKind::OutputPerPatch } else { StorageKind::Output };
    let io = io_variable(stage, &entry);
    let ty = attribute_type(c, true, inner_index, &entry);
    let elements = element_count(ty);
    let is_array = ty.has(AggregateType::ARRAY);
    let has_array_index = is_array || c.tc.definitions.has_per_location_input_or_output(io, true);
    let has_element_index = elements > 1;

    if has_array_index && has_element_index {
        let array_index = inner_index / elements;
        let element_index = inner_index - array_index * elements;
        let (ai, ei) = (c.const_i(array_index), c.const_i(element_index));
        match invocation_id {
            Some(inv) if !is_array => c.store_io(sk, io, Some(ai), &[inv, ei, value]),
            _ => c.store_io(sk, io, invocation_id, &[ai, ei, value]),
        }
    } else if has_array_index || has_element_index {
        let ii = c.const_i(inner_index);
        match invocation_id {
            Some(inv) if !is_array && has_array_index => c.store_io(sk, io, Some(ii), &[inv, value]),
            _ => c.store_io(sk, io, invocation_id, &[ii, value]),
        }
    } else {
        c.store_io(sk, io, invocation_id, &[value]);
    }
}

pub fn al2p(c: &mut Ctx) {
    let op = InstAl2p(c.raw_op());
    let d = get_dest(c, op.dest());
    let a = get_src_reg(c, op.src_a(), false);
    let k = c.const_i(op.imm11());
    let s = c.iadd(a, k);
    c.copy_to(d, s);
}

fn user_attribute_indexes(c: &mut Ctx, src_a: i32) -> (OperandId, OperandId) {
    let a = get_src_reg(c, src_a, false);
    let base = c.const_i(ac::USER_ATTRIBUTE_BASE);
    let offset = c.isubtract(a, base);
    let k4 = c.const_i(4);
    let vec_index = c.shift_right_u32(offset, k4);
    let k2 = c.const_i(2);
    let sh = c.shift_right_u32(offset, k2);
    let k3 = c.const_i(3);
    let elem_index = c.bitwise_and(sh, k3);
    (vec_index, elem_index)
}

pub fn ald(c: &mut Ctx) {
    let op = InstAld(c.raw_op());
    let stage = c.stage();
    // Some attributes are per invocation: no primitive vertex indexing for those.
    let mut has_prim_vertex = has_primitive_vertex_stage(stage, op.o()) && !op.p();
    if !op.phys() {
        has_prim_vertex &= has_primitive_vertex(op.imm11());
    }
    let prim_vertex = if has_prim_vertex {
        let b = get_src_reg(c, op.src_b(), false);
        Some(c.copy(b))
    } else {
        None
    };

    for index in 0..op.al_size().0 as i32 + 1 {
        let rd = Register::new(op.dest() + index, RegisterType::Gpr);
        if rd.is_rz() {
            break;
        }
        if op.phys() {
            let (vec_index, elem_index) = user_attribute_indexes(c, op.src_a());
            let sk = if op.o() { StorageKind::Output } else { StorageKind::Input };
            let d = c.ir.register(rd);
            let v = c.load_io3(sk, IoVariable::UserDefined, prim_vertex, vec_index, elem_index);
            c.copy_to(d, v);
        } else if op.src_b() == REGISTER_ZERO_INDEX || op.p() {
            let offset = fixed_func_to_user_attribute(c, op.imm11() + index * 4, op.o());
            let is_output = op.o() && can_load_output(offset);
            let converted = if !op.p() && !is_output { try_convert_id_to_index_for_vulkan(c, offset) } else { None };
            if let Some(value) = converted {
                let d = c.ir.register(rd);
                c.copy_to(d, value);
            } else {
                let mut value = generate_attribute_load(c, prim_vertex, offset, is_output, op.p());
                if !c.tc.definitions.supports_scaled_vertex_formats && stage == ShaderStage::Vertex && !op.o() && (0x80..0x280).contains(&offset) {
                    // Without host scaled vertex formats the fetch is integer: convert to float here.
                    let t = c.tc.definitions.attribute_type((offset - 0x80) >> 4);
                    if t == AttributeType::Sscaled {
                        value = c.iconvert_s32_to_fp32(value);
                    } else if t == AttributeType::Uscaled {
                        value = c.iconvert_u32_to_fp32(value);
                    }
                } else if offset == ac::PRIMITIVE_ID && c.tc.definitions.halve_primitive_id() {
                    let one = c.const_i(1);
                    value = c.shift_right_s32(value, one);
                }
                let d = c.ir.register(rd);
                c.copy_to(d, value);
            }
        } else {
            let offset = fixed_func_to_user_attribute(c, op.imm11() + index * 4, op.o());
            let is_output = op.o() && can_load_output(offset);
            let d = c.ir.register(rd);
            let v = generate_attribute_load(c, prim_vertex, offset, is_output, false);
            c.copy_to(d, v);
        }
    }
}

pub fn ast(c: &mut Ctx) {
    let op = InstAst(c.raw_op());
    for index in 0..op.al_size().0 as i32 + 1 {
        if op.src_b() + index > REGISTER_ZERO_INDEX {
            break;
        }
        let rd = Register::new(op.src_b() + index, RegisterType::Gpr);
        if op.phys() {
            let (vec_index, elem_index) = user_attribute_indexes(c, op.src_a());
            let invocation_id = if has_invocation_id(c.stage(), true) {
                Some(c.load_io(StorageKind::Input, IoVariable::InvocationId, None, None))
            } else {
                None
            };
            let v = c.ir.register(rd);
            c.store_io(StorageKind::Output, IoVariable::UserDefined, invocation_id, &[vec_index, elem_index, v]);
        } else {
            // TODO in the original: indirect stores through Ra.
            let offset = op.imm11() + index * 4;
            if !c.program.attribute_usage.is_used_output_attribute(offset) {
                return;
            }
            let offset = fixed_func_to_user_attribute(c, offset, true);
            let v = c.ir.register(rd);
            generate_attribute_store(c, offset, op.p(), v);
        }
    }
}

pub fn ipa(c: &mut Ctx) {
    let op = InstIpa(c.raw_op());
    let mut res;
    let mut is_fixed_func = false;
    if op.idx() {
        let (vec_index, elem_index) = user_attribute_indexes(c, op.src_a());
        res = c.load_io3(StorageKind::Input, IoVariable::UserDefined, None, vec_index, elem_index);
        let k3 = c.const_i(3);
        let w = c.load_io2(StorageKind::Input, IoVariable::FragmentCoord, None, k3);
        res = c.fp_multiply(res, w, I::FP32);
    } else {
        let (fixed, r) = try_fixed_func_to_user_attribute_ipa(c, op.imm10());
        is_fixed_func = fixed;
        res = r;
        let imm10 = op.imm10();
        if (ac::USER_ATTRIBUTE_BASE..ac::USER_ATTRIBUTE_END).contains(&imm10) {
            let index = (imm10 - ac::USER_ATTRIBUTE_BASE) >> 4;
            let imap = c.tc.definitions.imap_types.as_ref().expect("fragment input maps")[index as usize];
            if imap.first_used_type() == PixelImap::Perspective {
                let k3 = c.const_i(3);
                let w = c.load_io2(StorageKind::Input, IoVariable::FragmentCoord, None, k3);
                res = c.fp_multiply(res, w, I::FP32);
            }
        } else if imm10 == ac::POSITION_X || imm10 == ac::POSITION_Y {
            // With resolution scaling, FragCoord X / Y are divided by the render target scale: the code expects
            // unscaled values.
            let scale = {
                let (field, zero) = (c.const_i(crate::translation::properties::SupportBufferField::RenderScale as i32), c.const_i(0));
                c.load_b(StorageKind::ConstantBuffer, crate::translation::properties::SUPPORT_BUFFER_BINDING, &[field, zero])
            };
            res = c.fp_divide(res, scale, I::FP32);
            // Vulkan cannot change the origin: with YNegate the coordinates are flipped vertically.
            if imm10 == ac::POSITION_Y && c.tc.definitions.y_negate_enabled() {
                let (field, one) = (c.const_i(crate::translation::properties::SupportBufferField::ViewportSize as i32), c.const_i(1));
                let height = c.load_b(StorageKind::ConstantBuffer, 0, &[field, one]);
                res = c.fp_subtract(height, res, I::FP32);
            }
        } else if imm10 == ac::PRIMITIVE_ID && c.tc.definitions.halve_primitive_id() {
            // Quads drawn as two triangles each: halve the primitive ID.
            let one = c.const_i(1);
            res = c.shift_right_s32(res, one);
        }
        // The front facing workaround is for a host bug the engine's host does not have.
    }
    if op.ipa_op() == IpaOp::MULTIPLY && !is_fixed_func {
        let b = get_src_reg(c, op.src_b(), false);
        res = c.fp_multiply(res, b, I::FP32);
    }
    let res = c.fp_saturate_if(res, op.sat(), I::FP32);
    let d = get_dest(c, op.dest());
    c.copy_to(d, res);
}

pub fn isberd(c: &mut Ctx) {
    let op = InstIsberd(c.raw_op());
    // A load from ISBE memory: the offset is passed through (with the primitive index fixed at zero it is the
    // vertex index ALD then uses).
    let d = get_dest(c, op.dest());
    let a = get_src_reg(c, op.src_a(), false);
    c.copy_to(d, a);
}

fn out_flags(t: OutType) -> (bool, bool) {
    (t.0 & OutType::EMIT.0 == OutType::EMIT.0, t.0 & OutType::CUT.0 == OutType::CUT.0)
}

pub fn out_r(c: &mut Ctx) {
    let (emit, cut) = out_flags(InstOutR(c.raw_op()).out_type());
    emit_out(c, emit, cut);
}

pub fn out_i(c: &mut Ctx) {
    let (emit, cut) = out_flags(InstOutI(c.raw_op()).out_type());
    emit_out(c, emit, cut);
}

pub fn out_c(c: &mut Ctx) {
    let (emit, cut) = out_flags(InstOutC(c.raw_op()).out_type());
    emit_out(c, emit, cut);
}

fn emit_out(c: &mut Ctx, emit: bool, cut: bool) {
    if !(emit || cut) {
        c.log("Invalid OUT encoding.");
    }
    // With the viewport transform and depth mode fixups off there are no position values to save and restore
    // around the vertex.
    if emit {
        c.emit_vertex();
    }
    if cut {
        c.end_primitive();
    }
}

fn has_primitive_vertex(attr: i32) -> bool {
    attr != ac::PRIMITIVE_ID && attr != ac::TESS_COORD_X && attr != ac::TESS_COORD_Y
}

fn can_load_output(attr: i32) -> bool {
    attr != ac::TESS_COORD_X && attr != ac::TESS_COORD_Y
}

fn try_fixed_func_to_user_attribute_ipa(c: &mut Ctx, attr: i32) -> (bool, OperandId) {
    if (ac::FRONT_COLOR_DIFFUSE_R..ac::BACK_COLOR_DIFFUSE_R).contains(&attr) {
        // TODO in the original: back colours for back faces with two-sided rendering.
        let o = fixed_func_to_user_attribute(c, attr, false);
        return (true, generate_ipa_load(c, o));
    }
    if attr == ac::FOG_COORD {
        let o = fixed_func_to_user_attribute(c, attr, false);
        return (true, generate_ipa_load(c, o));
    }
    if (ac::BACK_COLOR_DIFFUSE_R..ac::CLIP_DISTANCE0).contains(&attr) {
        return (true, c.const_f(if (attr >> 2) & 3 == 3 { 1.0 } else { 0.0 }));
    }
    if (ac::TEX_COORD_BASE..ac::TEX_COORD_END).contains(&attr) {
        let o = fixed_func_to_user_attribute(c, attr, false);
        return (true, generate_ipa_load(c, o));
    }
    (false, generate_ipa_load(c, attr))
}

fn generate_ipa_load(c: &mut Ctx, offset: i32) -> OperandId {
    generate_attribute_load(c, None, offset, false, false)
}

/// The host supports layer output from vertex and tessellation stages, so only fog, colours and texture
/// coordinates move to user attributes.
fn fixed_func_to_user_attribute(c: &mut Ctx, attr: i32, is_output: bool) -> i32 {
    let fixed_start_attr = 0;
    if attr == ac::FOG_COORD {
        fixed_func_to_user_attribute_at(c, attr, ac::FOG_COORD, fixed_start_attr, is_output)
    } else if (ac::FRONT_COLOR_DIFFUSE_R..ac::CLIP_DISTANCE0).contains(&attr) {
        fixed_func_to_user_attribute_at(c, attr, ac::FRONT_COLOR_DIFFUSE_R, fixed_start_attr + 1, is_output)
    } else if (ac::TEX_COORD_BASE..ac::TEX_COORD_END).contains(&attr) {
        fixed_func_to_user_attribute_at(c, attr, ac::TEX_COORD_BASE, fixed_start_attr + 5, is_output)
    } else {
        attr
    }
}

fn fixed_func_to_user_attribute_at(c: &mut Ctx, attr: i32, base_attr: i32, base_index: i32, is_output: bool) -> i32 {
    let index = (attr - base_attr) >> 4;
    let user_attr_index = c.program.attribute_usage.free_user_attribute(is_output, base_index + index);
    if (user_attr_index as u32) < MAX_ATTRIBUTES as u32 {
        ac::USER_ATTRIBUTE_BASE + user_attr_index * 16 + (attr & 0xf)
    } else {
        c.log(&format!("No enough user attributes for fixed attribute offset 0x{attr:X}."));
        attr
    }
}

fn try_convert_id_to_index_for_vulkan(c: &mut Ctx, attr: i32) -> Option<OperandId> {
    if attr == ac::INSTANCE_ID {
        let index = c.load_io(StorageKind::Input, IoVariable::InstanceIndex, None, None);
        let base = c.load_io(StorageKind::Input, IoVariable::BaseInstance, None, None);
        return Some(c.isubtract(index, base));
    }
    if attr == ac::VERTEX_ID {
        return Some(c.load_io(StorageKind::Input, IoVariable::VertexIndex, None, None));
    }
    None
}
