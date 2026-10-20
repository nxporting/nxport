// Image loads, stores, atomics and reductions (InstEmitSurface).

use super::helper::*;
use super::texture::{next_reg, SAMPLE_1D_AS_2D};
use crate::decoders::inst_decoders::*;
use crate::decoders::register::REGISTER_ZERO_INDEX as RZ;
use crate::ir::{Instruction as I, OperandId, TextureFlags, DEFAULT_CBUF_SLOT};
use crate::translation::emitter::{Emit, EmitterContext as Ctx};
use crate::translation::properties::{texture_format, texture_format_atomic};
use crate::types::{SamplerType, SetBindingPair, TextureFormat};

pub fn suatom_b(c: &mut Ctx) {
    let op = InstSuatomB(c.raw_op());
    emit_suatom(c, op.dim(), op.op(), op.size(), 0, op.src_a(), op.src_b(), op.src_c(), op.dest(), op.ba(), true, false);
}

pub fn suatom(c: &mut Ctx) {
    let op = InstSuatom(c.raw_op());
    emit_suatom(c, op.dim(), op.op(), op.size(), op.tid_b(), op.src_a(), op.src_b(), 0, op.dest(), op.ba(), false, false);
}

pub fn suatom_b2(c: &mut Ctx) {
    let op = InstSuatomB2(c.raw_op());
    emit_suatom(c, op.dim(), op.op(), op.size(), 0, op.src_a(), op.src_b(), op.src_c(), op.dest(), op.ba(), true, false);
}

pub fn suatom_cas_b(c: &mut Ctx) {
    let op = InstSuatomCasB(c.raw_op());
    emit_suatom(c, op.dim(), SuatomOp(0), op.size(), 0, op.src_a(), op.src_b(), op.src_c(), op.dest(), op.ba(), true, true);
}

pub fn suatom_cas(c: &mut Ctx) {
    let op = InstSuatomCas(c.raw_op());
    emit_suatom(c, op.dim(), SuatomOp(0), op.size(), op.tid_b(), op.src_a(), op.src_b(), 0, op.dest(), op.ba(), false, true);
}

pub fn suld_db(c: &mut Ctx) {
    let op = InstSuldDB(c.raw_op());
    emit_suld(c, op.cache_op(), op.dim(), op.size(), 0, SuRgba(0), op.src_a(), op.dest(), op.src_c(), false, op.ba(), true);
}

pub fn suld_d(c: &mut Ctx) {
    let op = InstSuldD(c.raw_op());
    emit_suld(c, op.cache_op(), op.dim(), op.size(), op.tid_b(), SuRgba(0), op.src_a(), op.dest(), 0, false, op.ba(), false);
}

pub fn suld_b(c: &mut Ctx) {
    let op = InstSuldB(c.raw_op());
    emit_suld(c, op.cache_op(), op.dim(), SuSize(0), 0, op.rgba(), op.src_a(), op.dest(), op.src_c(), true, false, true);
}

pub fn suld(c: &mut Ctx) {
    let op = InstSuld(c.raw_op());
    emit_suld(c, op.cache_op(), op.dim(), SuSize(0), op.tid_b(), op.rgba(), op.src_a(), op.dest(), 0, true, false, false);
}

pub fn sured_b(c: &mut Ctx) {
    let op = InstSuredB(c.raw_op());
    emit_sured(c, op.dim(), op.op(), op.size(), 0, op.src_a(), op.dest(), op.src_c(), op.ba(), true);
}

pub fn sured(c: &mut Ctx) {
    let op = InstSured(c.raw_op());
    emit_sured(c, op.dim(), op.op(), op.size(), op.tid_b(), op.src_a(), op.dest(), 0, op.ba(), false);
}

pub fn sust_db(c: &mut Ctx) {
    let op = InstSustDB(c.raw_op());
    emit_sust(c, op.cache_op(), op.dim(), op.size(), 0, SuRgba(0), op.src_a(), op.dest(), op.src_c(), false, op.ba(), true);
}

pub fn sust_d(c: &mut Ctx) {
    let op = InstSustD(c.raw_op());
    emit_sust(c, op.cache_op(), op.dim(), op.size(), op.tid_b(), SuRgba(0), op.src_a(), op.dest(), 0, false, op.ba(), false);
}

pub fn sust_b(c: &mut Ctx) {
    let op = InstSustB(c.raw_op());
    emit_sust(c, op.cache_op(), op.dim(), SuSize(0), 0, op.rgba(), op.src_a(), op.dest(), op.src_c(), true, false, true);
}

pub fn sust(c: &mut Ctx) {
    let op = InstSust(c.raw_op());
    emit_sust(c, op.cache_op(), op.dim(), SuSize(0), op.tid_b(), op.rgba(), op.src_a(), op.dest(), 0, true, false, false);
}

/// The coordinates (after a bindless handle), with 1D read as 2D and the array index last.
fn coords(c: &mut Ctx, sources: &mut Vec<OperandId>, ty: &mut SamplerType, src_a: &mut i32) {
    for _ in 0..ty.dimensions() {
        sources.push(next_reg(c, src_a));
    }
    if SAMPLE_1D_AS_2D && ty.base() == SamplerType::TEXTURE_1D {
        sources.push(c.const_i(0));
        *ty = (*ty & !SamplerType::MASK) | SamplerType::TEXTURE_2D;
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_suatom(
    c: &mut Ctx,
    dimensions: SuDim,
    atomic_op: SuatomOp,
    size: SuatomSize,
    imm: i32,
    mut src_a: i32,
    mut src_b: i32,
    src_c: i32,
    dest: i32,
    byte_address: bool,
    is_bindless: bool,
    compare_and_swap: bool,
) {
    let mut ty = convert_sampler_type(dimensions);
    if ty == SamplerType::NONE {
        c.log("Invalid image atomic sampler type.");
        return;
    }
    let d = c.gpr(dest);
    let mut sources = Vec::new();
    if is_bindless {
        let r = get_src_reg(c, src_c, false);
        sources.push(c.copy(r));
    }
    coords(c, &mut sources, &mut ty, &mut src_a);
    if ty.has(SamplerType::ARRAY) {
        sources.push(next_reg(c, &mut src_a));
        ty = ty | SamplerType::ARRAY;
    }
    if byte_address {
        let x = if is_bindless { 1 } else { 0 };
        let k = c.const_i(component_size_in_bytes_log2_atom(size));
        sources[x] = c.shift_right_s32(sources[x], k);
    }
    // Floating point and 64-bit formats are not handled.
    let format = if size == SuatomSize::SD32 || size == SuatomSize::SD64 {
        if is_bindless { TextureFormat::Unknown } else { texture_format_atomic(&c.tc.gpu, imm, DEFAULT_CBUF_SLOT) }
    } else {
        texture_format_atom(size)
    };
    if compare_and_swap {
        sources.push(next_reg(c, &mut src_b));
    }
    sources.push(next_reg(c, &mut src_b));
    let mut flags = if compare_and_swap { TextureFlags::CAS } else { atomic_op_flags(atomic_op) };
    if is_bindless {
        flags |= TextureFlags::BINDLESS;
    }
    let sb = if is_bindless {
        SetBindingPair::new(0, 0)
    } else {
        c.rm.texture_or_image_binding(&c.tc.gpu, I::IMAGE_ATOMIC, ty, format, flags, DEFAULT_CBUF_SLOT, imm, 1, false)
    };
    let res = c.image_atomic(ty, format, flags, sb, &sources);
    c.copy_to(d, res);
}

#[allow(clippy::too_many_arguments)]
fn emit_suld(
    c: &mut Ctx,
    cache_op: CacheOpLd,
    dimensions: SuDim,
    size: SuSize,
    imm: i32,
    component_mask: SuRgba,
    mut src_a: i32,
    src_b: i32,
    src_c: i32,
    use_components: bool,
    byte_address: bool,
    is_bindless: bool,
) {
    if src_b == RZ {
        return;
    }
    let mut ty = convert_sampler_type(dimensions);
    if ty == SamplerType::NONE {
        c.log("Invalid image store sampler type.");
        return;
    }
    let mut sources = Vec::new();
    if is_bindless {
        let r = c.gpr(src_c);
        sources.push(c.copy(r));
    }
    coords(c, &mut sources, &mut ty, &mut src_a);
    if ty.has(SamplerType::ARRAY) {
        sources.push(next_reg(c, &mut src_a));
    }
    let handle = imm;
    let mut flags = if is_bindless { TextureFlags::BINDLESS } else { TextureFlags::NONE };
    if cache_op == CacheOpLd::CG {
        flags |= TextureFlags::COHERENT;
    }
    let dests_for = |c: &mut Ctx, n: i32| -> Vec<Option<OperandId>> {
        let mut dests = Vec::new();
        for i in 0..n {
            if src_b + i >= RZ {
                break;
            }
            dests.push(Some(c.gpr(src_b + i)));
        }
        dests
    };
    if use_components {
        let dests = dests_for(c, component_mask.0.count_ones() as i32);
        let format = if is_bindless { TextureFormat::Unknown } else { texture_format(&c.tc.gpu, handle, DEFAULT_CBUF_SLOT) };
        let sb = if is_bindless {
            SetBindingPair::new(0, 0)
        } else {
            c.rm.texture_or_image_binding(&c.tc.gpu, I::IMAGE_LOAD, ty, format, flags, DEFAULT_CBUF_SLOT, handle, 1, false)
        };
        c.image_load(ty, format, flags, sb, component_mask.0 as i32, &dests, &sources);
    } else {
        if byte_address {
            let x = if is_bindless { 1 } else { 0 };
            let k = c.const_i(component_size_in_bytes_log2_su(size));
            sources[x] = c.shift_right_s32(sources[x], k);
        }
        let components = components_of(size);
        let comp_mask = (1 << components) - 1;
        let dests = dests_for(c, components);
        let format = texture_format_su(size);
        let sb = if is_bindless {
            SetBindingPair::new(0, 0)
        } else {
            c.rm.texture_or_image_binding(&c.tc.gpu, I::IMAGE_LOAD, ty, format, flags, DEFAULT_CBUF_SLOT, handle, 1, false)
        };
        c.image_load(ty, format, flags, sb, comp_mask, &dests, &sources);
        let d0 = dests[0].unwrap();
        let v = match size {
            SuSize::U8 => Some(zero_extend_to_32(c, d0, 8)),
            SuSize::U16 => Some(zero_extend_to_32(c, d0, 16)),
            SuSize::S8 => Some(sign_extend_to_32(c, d0, 8)),
            SuSize::S16 => Some(sign_extend_to_32(c, d0, 16)),
            _ => None,
        };
        if let Some(v) = v {
            c.copy_to(d0, v);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_sured(
    c: &mut Ctx,
    dimensions: SuDim,
    atomic_op: RedOp,
    size: SuatomSize,
    imm: i32,
    mut src_a: i32,
    mut src_b: i32,
    src_c: i32,
    byte_address: bool,
    is_bindless: bool,
) {
    let mut ty = convert_sampler_type(dimensions);
    if ty == SamplerType::NONE {
        c.log("Invalid image reduction sampler type.");
        return;
    }
    let mut sources = Vec::new();
    if is_bindless {
        let r = get_src_reg(c, src_c, false);
        sources.push(c.copy(r));
    }
    coords(c, &mut sources, &mut ty, &mut src_a);
    if ty.has(SamplerType::ARRAY) {
        sources.push(next_reg(c, &mut src_a));
        ty = ty | SamplerType::ARRAY;
    }
    if byte_address {
        let x = if is_bindless { 1 } else { 0 };
        let k = c.const_i(component_size_in_bytes_log2_atom(size));
        sources[x] = c.shift_right_s32(sources[x], k);
    }
    // Floating point and 64-bit formats are not handled.
    let format = if size == SuatomSize::SD32 || size == SuatomSize::SD64 {
        if is_bindless { TextureFormat::Unknown } else { texture_format_atomic(&c.tc.gpu, imm, DEFAULT_CBUF_SLOT) }
    } else {
        texture_format_atom(size)
    };
    sources.push(next_reg(c, &mut src_b));
    let mut flags = atomic_op_flags(SuatomOp(atomic_op.0));
    if is_bindless {
        flags |= TextureFlags::BINDLESS;
    }
    let sb = if is_bindless {
        SetBindingPair::new(0, 0)
    } else {
        c.rm.texture_or_image_binding(&c.tc.gpu, I::IMAGE_ATOMIC, ty, format, flags, DEFAULT_CBUF_SLOT, imm, 1, false)
    };
    c.image_atomic(ty, format, flags, sb, &sources);
}

#[allow(clippy::too_many_arguments)]
fn emit_sust(
    c: &mut Ctx,
    cache_op: CacheOpSt,
    dimensions: SuDim,
    size: SuSize,
    imm: i32,
    component_mask: SuRgba,
    mut src_a: i32,
    mut src_b: i32,
    src_c: i32,
    use_components: bool,
    byte_address: bool,
    is_bindless: bool,
) {
    let mut ty = convert_sampler_type(dimensions);
    if ty == SamplerType::NONE {
        c.log("Invalid image store sampler type.");
        return;
    }
    let mut sources = Vec::new();
    if is_bindless {
        let r = c.gpr(src_c);
        sources.push(c.copy(r));
    }
    coords(c, &mut sources, &mut ty, &mut src_a);
    if ty.has(SamplerType::ARRAY) {
        sources.push(next_reg(c, &mut src_a));
    }
    let mut format = TextureFormat::Unknown;
    if use_components {
        let mut comp_mask = component_mask.0 as i32;
        while comp_mask != 0 {
            if comp_mask & 1 != 0 {
                sources.push(next_reg(c, &mut src_b));
            }
            comp_mask >>= 1;
        }
        if !is_bindless {
            format = texture_format(&c.tc.gpu, imm, DEFAULT_CBUF_SLOT);
        }
    } else {
        if byte_address {
            let x = if is_bindless { 1 } else { 0 };
            let k = c.const_i(component_size_in_bytes_log2_su(size));
            sources[x] = c.shift_right_s32(sources[x], k);
        }
        for _ in 0..components_of(size) {
            sources.push(next_reg(c, &mut src_b));
        }
        format = texture_format_su(size);
    }
    let handle = imm;
    let mut flags = if is_bindless { TextureFlags::BINDLESS } else { TextureFlags::NONE };
    if cache_op == CacheOpSt::CG {
        flags |= TextureFlags::COHERENT;
    }
    let sb = if is_bindless {
        SetBindingPair::new(0, 0)
    } else {
        c.rm.texture_or_image_binding(&c.tc.gpu, I::IMAGE_STORE, ty, format, flags, DEFAULT_CBUF_SLOT, handle, 1, false)
    };
    c.image_store(ty, format, flags, sb, &sources);
}

fn component_size_in_bytes_log2_atom(size: SuatomSize) -> i32 {
    match size {
        SuatomSize::U64 | SuatomSize::S64 | SuatomSize::SD64 => 3,
        _ => 2,
    }
}

fn texture_format_atom(size: SuatomSize) -> TextureFormat {
    match size {
        SuatomSize::S32 => TextureFormat::R32Sint,
        SuatomSize::U64 | SuatomSize::S64 | SuatomSize::SD64 => TextureFormat::R32G32Uint,
        SuatomSize::F32_FTZ_RN => TextureFormat::R32Float,
        SuatomSize::F16X2_FTZ_RN => TextureFormat::R16G16Float,
        _ => TextureFormat::R32Uint,
    }
}

fn atomic_op_flags(op: SuatomOp) -> TextureFlags {
    match op {
        SuatomOp::MIN => TextureFlags::MINIMUM,
        SuatomOp::MAX => TextureFlags::MAXIMUM,
        SuatomOp::INC => TextureFlags::INCREMENT,
        SuatomOp::DEC => TextureFlags::DECREMENT,
        SuatomOp::AND => TextureFlags::BITWISE_AND,
        SuatomOp::OR => TextureFlags::BITWISE_OR,
        SuatomOp::XOR => TextureFlags::BITWISE_XOR,
        SuatomOp::EXCH => TextureFlags::SWAP,
        _ => TextureFlags::ADD,
    }
}

fn components_of(size: SuSize) -> i32 {
    match size {
        SuSize::B64 => 2,
        SuSize::B128 | SuSize::UB128 => 4,
        _ => 1,
    }
}

fn component_size_in_bytes_log2_su(size: SuSize) -> i32 {
    match size {
        SuSize::U8 | SuSize::S8 => 0,
        SuSize::U16 | SuSize::S16 => 1,
        SuSize::B64 => 3,
        SuSize::B128 | SuSize::UB128 => 4,
        _ => 2,
    }
}

fn texture_format_su(size: SuSize) -> TextureFormat {
    match size {
        SuSize::U8 => TextureFormat::R8Uint,
        SuSize::S8 => TextureFormat::R8Sint,
        SuSize::U16 => TextureFormat::R16Uint,
        SuSize::S16 => TextureFormat::R16Sint,
        SuSize::B64 => TextureFormat::R32G32Uint,
        SuSize::B128 | SuSize::UB128 => TextureFormat::R32G32B32A32Uint,
        _ => TextureFormat::R32Uint,
    }
}

fn convert_sampler_type(target: SuDim) -> SamplerType {
    match target {
        SuDim::D1D => SamplerType::TEXTURE_1D,
        SuDim::D1D_BUFFER => SamplerType::TEXTURE_BUFFER,
        SuDim::D1D_ARRAY => SamplerType::TEXTURE_1D | SamplerType::ARRAY,
        SuDim::D2D => SamplerType::TEXTURE_2D,
        SuDim::D2D_ARRAY => SamplerType::TEXTURE_2D | SamplerType::ARRAY,
        SuDim::D3D => SamplerType::TEXTURE_3D,
        _ => SamplerType::NONE,
    }
}
