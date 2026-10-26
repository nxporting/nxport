// Texture sampling, fetches, gathers and queries (InstEmitTexture).

use crate::decoders::inst_decoders::*;
use crate::decoders::register::REGISTER_ZERO_INDEX as RZ;
use crate::ir::{Instruction as I, OperandId, TextureFlags, DEFAULT_CBUF_SLOT};
use crate::translation::emitter::{Emit, EmitterContext as Ctx};
use crate::types::{SamplerType, SetBindingPair, TextureFormat};

const MASK_LUT: [[i32; 8]; 2] = [
    [0b0001, 0b0010, 0b0100, 0b1000, 0b0011, 0b1001, 0b1010, 0b1100],
    [0b0111, 0b1011, 0b1101, 0b1110, 0b1111, 0b0000, 0b0000, 0b0000],
];

pub const SAMPLE_1D_AS_2D: bool = true;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TexsType {
    Texs,
    Tlds,
    Tld4s,
}

/// The next source register (Ra() / Rb()): a copy of it, or zero once past RZ.
pub(super) fn next_reg(c: &mut Ctx, index: &mut i32) -> OperandId {
    if *index > RZ {
        return c.const_i(0);
    }
    let r = c.gpr(*index);
    *index += 1;
    c.copy(r)
}

pub fn tex(c: &mut Ctx) {
    let op = InstTex(c.raw_op());
    emit_tex(c, TextureFlags::NONE, op.dim(), op.lod(), op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), false, op.dc(), op.aoffi());
}

pub fn tex_b(c: &mut Ctx) {
    let op = InstTexB(c.raw_op());
    emit_tex(c, TextureFlags::BINDLESS, op.dim(), op.lodb(), 0, op.w_mask(), op.src_a(), op.src_b(), op.dest(), false, op.dc(), op.aoffib());
}

pub fn texs(c: &mut Ctx) {
    let op = InstTexs(c.raw_op());
    emit_texs(c, TexsType::Texs, op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.dest2(), false);
}

pub fn texs_f16(c: &mut Ctx) {
    let op = InstTexs(c.raw_op());
    emit_texs(c, TexsType::Texs, op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.dest2(), true);
}

pub fn tld(c: &mut Ctx) {
    let op = InstTld(c.raw_op());
    let lod = if op.lod() { Lod::LL } else { Lod::LZ };
    emit_tex(c, TextureFlags::INT_COORDS, op.dim(), lod, op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.ms(), false, op.toff());
}

pub fn tld_b(c: &mut Ctx) {
    let op = InstTldB(c.raw_op());
    let flags = TextureFlags::INT_COORDS | TextureFlags::BINDLESS;
    let lod = if op.lod() { Lod::LL } else { Lod::LZ };
    emit_tex(c, flags, op.dim(), lod, 0, op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.ms(), false, op.toff());
}

pub fn tlds(c: &mut Ctx) {
    let op = InstTlds(c.raw_op());
    emit_texs(c, TexsType::Tlds, op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.dest2(), false);
}

pub fn tlds_f16(c: &mut Ctx) {
    let op = InstTlds(c.raw_op());
    emit_texs(c, TexsType::Tlds, op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.dest2(), true);
}

pub fn tld4(c: &mut Ctx) {
    let op = InstTld4(c.raw_op());
    emit_tld4(c, op.dim(), op.tex_comp(), op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.toff(), op.dc(), false);
}

pub fn tld4_b(c: &mut Ctx) {
    let op = InstTld4B(c.raw_op());
    emit_tld4(c, op.dim(), op.tex_comp(), 0, op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.toff(), op.dc(), true);
}

pub fn tld4s(c: &mut Ctx) {
    let op = InstTld4s(c.raw_op());
    emit_texs(c, TexsType::Tld4s, op.tid_b(), 4, op.src_a(), op.src_b(), op.dest(), op.dest2(), false);
}

pub fn tld4s_f16(c: &mut Ctx) {
    let op = InstTld4s(c.raw_op());
    emit_texs(c, TexsType::Tld4s, op.tid_b(), 4, op.src_a(), op.src_b(), op.dest(), op.dest2(), true);
}

pub fn tmml(c: &mut Ctx) {
    let op = InstTmml(c.raw_op());
    emit_tmml(c, op.dim(), op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), false);
}

pub fn tmml_b(c: &mut Ctx) {
    let op = InstTmmlB(c.raw_op());
    emit_tmml(c, op.dim(), 0, op.w_mask(), op.src_a(), op.src_b(), op.dest(), true);
}

pub fn txd(c: &mut Ctx) {
    let op = InstTxd(c.raw_op());
    emit_txd(c, op.dim(), op.tid_b(), op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.toff(), false);
}

pub fn txd_b(c: &mut Ctx) {
    let op = InstTxdB(c.raw_op());
    emit_txd(c, op.dim(), 0, op.w_mask(), op.src_a(), op.src_b(), op.dest(), op.toff(), true);
}

pub fn txq(c: &mut Ctx) {
    let op = InstTxq(c.raw_op());
    emit_txq(c, op.tex_query(), op.tid_b(), op.w_mask(), op.src_a(), op.dest(), false);
}

pub fn txq_b(c: &mut Ctx) {
    let op = InstTxqB(c.raw_op());
    emit_txq(c, op.tex_query(), 0, op.w_mask(), op.src_a(), op.dest(), true);
}

/// Destination registers rd, rd + 1, ... for `count` components, stopping at RZ.
fn gpr_dests(c: &mut Ctx, rd: i32, count: u32) -> Vec<Option<OperandId>> {
    let mut dests = Vec::new();
    for i in 0..count as i32 {
        if rd + i >= RZ {
            break;
        }
        dests.push(Some(c.gpr(rd + i)));
    }
    dests
}

#[allow(clippy::too_many_arguments)]
fn emit_tex(
    c: &mut Ctx,
    mut flags: TextureFlags,
    dimensions: TexDim,
    lod_mode: Lod,
    imm: i32,
    component_mask: i32,
    mut ra_index: i32,
    mut rb_index: i32,
    rd_index: i32,
    is_multisample: bool,
    has_depth_compare: bool,
    has_offset: bool,
) {
    if rd_index == RZ {
        return;
    }
    let mut ty = convert_sampler_type(dimensions);
    let is_array = ty.has(SamplerType::ARRAY);
    let is_bindless = flags.has(TextureFlags::BINDLESS);
    let array_index = if is_array { Some(next_reg(c, &mut ra_index)) } else { None };
    let mut sources = Vec::new();
    if is_bindless {
        sources.push(next_reg(c, &mut rb_index));
    }
    let has_lod = lod_mode.0 > Lod::LZ.0;
    let mut wrap_1d = false;
    if ty == SamplerType::TEXTURE_1D
        && (flags & !TextureFlags::BINDLESS) == TextureFlags::INT_COORDS
        && !(has_lod || has_depth_compare || has_offset || is_array || is_multisample)
    {
        // Bindless textures have no known type: 1D is assumed to be a buffer, the more common case.
        let is_type_buffer = is_bindless || c.tc.gpu.sampler_type(imm, DEFAULT_CBUF_SLOT) == SamplerType::TEXTURE_BUFFER;
        if is_type_buffer {
            ty = SamplerType::TEXTURE_BUFFER;
        } else {
            wrap_1d = SAMPLE_1D_AS_2D;
        }
    }
    let coords_count = ty.dimensions();
    for _ in 0..coords_count {
        sources.push(next_reg(c, &mut ra_index));
    }
    let mut is_1d_to_2d = false;
    if SAMPLE_1D_AS_2D && ty.base() == SamplerType::TEXTURE_1D {
        if wrap_1d {
            let last_idx = sources.len() - 1;
            let x = sources[last_idx];
            let mask = c.const_i(0x3FFF);
            sources[last_idx] = c.bitwise_and(x, mask);
            let shift = c.const_i(14);
            sources.push(c.shift_right_u32(x, shift));
        } else {
            sources.push(c.const_f(0.0));
        }
        ty = SamplerType::TEXTURE_2D | (ty & SamplerType::ARRAY);
        is_1d_to_2d = true;
    }
    if let Some(a) = array_index {
        sources.push(a);
    }
    let lod_value = if has_lod { next_reg(c, &mut rb_index) } else { c.const_f(0.0) };
    let packed_offs = if has_offset { Some(next_reg(c, &mut rb_index)) } else { None };
    if has_depth_compare {
        sources.push(next_reg(c, &mut rb_index));
        ty = ty | SamplerType::SHADOW;
    }
    if (lod_mode == Lod::LZ || lod_mode == Lod::LL || lod_mode == Lod::LLA) && !is_multisample && ty != SamplerType::TEXTURE_BUFFER {
        sources.push(lod_value);
        flags |= TextureFlags::LOD_LEVEL;
    }
    if let Some(packed) = packed_offs {
        for index in 0..coords_count {
            let (o, s) = (c.const_i(index * 4), c.const_i(4));
            sources.push(c.bitfield_extract_s32(packed, o, s));
        }
        if is_1d_to_2d {
            sources.push(c.const_i(0));
        }
        flags |= TextureFlags::OFFSET;
    }
    if lod_mode == Lod::LB || lod_mode == Lod::LBA {
        sources.push(lod_value);
        flags |= TextureFlags::LOD_BIAS;
    }
    if is_multisample {
        sources.push(next_reg(c, &mut rb_index));
        ty = ty | SamplerType::MULTISAMPLE;
    }
    let dests = gpr_dests(c, rd_index, (component_mask as u32).count_ones());
    let handle = if !is_bindless { imm } else { 0 };
    emit_texture_sample(c, ty, flags, handle, component_mask, &dests, &sources);
}

#[allow(clippy::too_many_arguments)]
fn emit_texs(
    c: &mut Ctx,
    texs_type: TexsType,
    imm: i32,
    write_mask: i32,
    mut src_a: i32,
    mut src_b: i32,
    dest: i32,
    dest2: i32,
    is_f16: bool,
) {
    if dest == RZ && dest2 == RZ {
        return;
    }
    let mut sources: Vec<OperandId> = Vec::new();

    fn add_texture_offset(c: &mut Ctx, sources: &mut Vec<OperandId>, src_b: &mut i32, coords_count: i32, stride: i32, size: i32) {
        let packed = next_reg(c, src_b);
        for index in 0..coords_count {
            let (o, s) = (c.const_i(index * stride), c.const_i(size));
            sources.push(c.bitfield_extract_s32(packed, o, s));
        }
    }

    let mut ty;
    let mut flags;
    match texs_type {
        TexsType::Texs => {
            let op = InstTexs(c.raw_op());
            ty = convert_sampler_type_texs(op.target());
            if ty == SamplerType::NONE {
                c.log("Invalid texture sampler type.");
                return;
            }
            flags = convert_texture_flags_texs(op.target());
            // Only fetches with integer coordinates can use buffer targets: no 1D to buffer conversion here.
            if ty.has(SamplerType::ARRAY) {
                let array_index = next_reg(c, &mut src_a);
                sources.push(next_reg(c, &mut src_a));
                sources.push(next_reg(c, &mut src_b));
                sources.push(array_index);
                if ty.has(SamplerType::SHADOW) {
                    sources.push(next_reg(c, &mut src_b));
                }
                if flags.has(TextureFlags::LOD_LEVEL) {
                    sources.push(c.const_f(0.0));
                }
            } else {
                match op.target() {
                    TexsTarget::TEXTURE1_D_LOD_ZERO => {
                        sources.push(next_reg(c, &mut src_a));
                        if SAMPLE_1D_AS_2D {
                            sources.push(c.const_f(0.0));
                            ty = (ty & !SamplerType::MASK) | SamplerType::TEXTURE_2D;
                        }
                        sources.push(c.const_f(0.0));
                    }
                    TexsTarget::TEXTURE2_D => {
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_b));
                    }
                    TexsTarget::TEXTURE2_D_LOD_ZERO => {
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_b));
                        sources.push(c.const_f(0.0));
                    }
                    TexsTarget::TEXTURE2_D_LOD_LEVEL
                    | TexsTarget::TEXTURE2_D_DEPTH_COMPARE
                    | TexsTarget::TEXTURE3_D
                    | TexsTarget::TEXTURE_CUBE => {
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_b));
                    }
                    TexsTarget::TEXTURE2_D_LOD_ZERO_DEPTH_COMPARE | TexsTarget::TEXTURE3_D_LOD_ZERO => {
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_b));
                        sources.push(c.const_f(0.0));
                    }
                    TexsTarget::TEXTURE2_D_LOD_LEVEL_DEPTH_COMPARE | TexsTarget::TEXTURE_CUBE_LOD_LEVEL => {
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_a));
                        sources.push(next_reg(c, &mut src_b));
                        sources.push(next_reg(c, &mut src_b));
                    }
                    _ => {}
                }
            }
        }
        TexsType::Tlds => {
            let op = InstTlds(c.raw_op());
            ty = convert_sampler_type_tlds(op.target());
            if ty == SamplerType::NONE {
                c.log("Invalid texel fetch sampler type.");
                return;
            }
            flags = convert_texture_flags_tlds(op.target()) | TextureFlags::INT_COORDS;
            if op.target() == TldsTarget::TEXTURE1_D_LOD_ZERO
                && c.tc.gpu.sampler_type(op.tid_b(), DEFAULT_CBUF_SLOT) == SamplerType::TEXTURE_BUFFER
            {
                ty = SamplerType::TEXTURE_BUFFER;
                flags = flags & !TextureFlags::LOD_LEVEL;
            }
            match op.target() {
                TldsTarget::TEXTURE1_D_LOD_ZERO => {
                    let x = next_reg(c, &mut src_a);
                    if ty != SamplerType::TEXTURE_BUFFER {
                        if SAMPLE_1D_AS_2D {
                            let mask = c.const_i(0x3FFF);
                            let x_coord = c.bitwise_and(x, mask);
                            let shift = c.const_i(14);
                            let y_coord = c.shift_right_u32(x, shift);
                            sources.push(x_coord);
                            sources.push(y_coord);
                            ty = (ty & !SamplerType::MASK) | SamplerType::TEXTURE_2D;
                        } else {
                            sources.push(x);
                        }
                        sources.push(c.const_f(0.0));
                    } else {
                        sources.push(x);
                    }
                }
                TldsTarget::TEXTURE1_D_LOD_LEVEL => {
                    let x = next_reg(c, &mut src_a);
                    if SAMPLE_1D_AS_2D {
                        let mask = c.const_i(0x3FFF);
                        let x_coord = c.bitwise_and(x, mask);
                        let shift = c.const_i(14);
                        let y_coord = c.shift_right_u32(x, shift);
                        sources.push(x_coord);
                        sources.push(y_coord);
                        ty = (ty & !SamplerType::MASK) | SamplerType::TEXTURE_2D;
                    } else {
                        sources.push(x);
                    }
                    sources.push(next_reg(c, &mut src_b));
                }
                TldsTarget::TEXTURE2_D_LOD_ZERO => {
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(next_reg(c, &mut src_b));
                    sources.push(c.const_i(0));
                }
                TldsTarget::TEXTURE2_D_LOD_ZERO_OFFSET => {
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(c.const_i(0));
                }
                TldsTarget::TEXTURE2_D_LOD_ZERO_MULTISAMPLE
                | TldsTarget::TEXTURE2_D_LOD_LEVEL
                | TldsTarget::TEXTURE2_D_LOD_LEVEL_OFFSET => {
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(next_reg(c, &mut src_b));
                }
                TldsTarget::TEXTURE3_D_LOD_ZERO => {
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(next_reg(c, &mut src_b));
                    sources.push(c.const_i(0));
                }
                TldsTarget::TEXTURE2_D_ARRAY_LOD_ZERO => {
                    sources.push(next_reg(c, &mut src_b));
                    sources.push(next_reg(c, &mut src_b));
                    sources.push(next_reg(c, &mut src_a));
                    sources.push(c.const_i(0));
                }
                _ => {}
            }
            if flags.has(TextureFlags::OFFSET) {
                add_texture_offset(c, &mut sources, &mut src_b, ty.dimensions(), 4, 4);
            }
        }
        TexsType::Tld4s => {
            let op = InstTld4s(c.raw_op());
            if !(op.dc() || op.aoffi()) {
                sources.push(next_reg(c, &mut src_a));
                sources.push(next_reg(c, &mut src_b));
            } else {
                sources.push(next_reg(c, &mut src_a));
                sources.push(next_reg(c, &mut src_a));
            }
            ty = SamplerType::TEXTURE_2D;
            flags = TextureFlags::GATHER;
            let depth_compare_index = sources.len();
            if op.aoffi() {
                add_texture_offset(c, &mut sources, &mut src_b, ty.dimensions(), 8, 6);
                flags |= TextureFlags::OFFSET;
            }
            if op.dc() {
                let r = next_reg(c, &mut src_b);
                sources.insert(depth_compare_index, r);
                ty = ty | SamplerType::SHADOW;
            } else {
                sources.push(c.const_i(op.tex_comp().0 as i32));
            }
        }
    }

    let mut rd0 = [c.const_f(0.0), c.const_f(0.0)];
    let mut rd1 = [c.const_f(0.0), c.const_f(0.0)];
    let handle = imm;
    let component_mask = MASK_LUT[if dest2 == RZ { 0 } else { 1 }][write_mask as usize];
    let components_count = (component_mask as u32).count_ones() as i32;
    let mut dests = Vec::with_capacity(components_count as usize);
    for i in 0..components_count {
        let high = i >> 1;
        let low = (i & 1) as usize;
        if is_f16 {
            let l = c.local();
            if high != 0 {
                rd1[low] = l;
            } else {
                rd0[low] = l;
            }
            dests.push(Some(l));
        } else {
            let mut rd_index = if high != 0 { dest2 } else { dest };
            if rd_index < RZ {
                rd_index += low as i32;
            }
            dests.push(Some(c.gpr(rd_index)));
        }
    }
    emit_texture_sample(c, ty, flags, handle, component_mask, &dests, &sources);
    if is_f16 {
        let d = c.gpr(dest);
        let v = c.pack_half_2x16(rd0[0], rd0[1]);
        c.copy_to(d, v);
        let d2 = c.gpr(dest2);
        let v = c.pack_half_2x16(rd1[0], rd1[1]);
        c.copy_to(d2, v);
    }
}

fn is_array_dim(d: TexDim) -> bool {
    matches!(d, TexDim::ARRAY1D | TexDim::ARRAY2D | TexDim::ARRAY3D | TexDim::ARRAY_CUBE)
}

#[allow(clippy::too_many_arguments)]
fn emit_tld4(
    c: &mut Ctx,
    dimensions: TexDim,
    component: TexComp,
    imm: i32,
    component_mask: i32,
    mut src_a: i32,
    mut src_b: i32,
    dest: i32,
    offset: TexOffset,
    has_depth_compare: bool,
    is_bindless: bool,
) {
    if dest == RZ {
        return;
    }
    let is_array = is_array_dim(dimensions);
    let array_index = if is_array { Some(next_reg(c, &mut src_a)) } else { None };
    let mut sources = Vec::new();
    let mut ty = convert_sampler_type(dimensions);
    let mut flags = TextureFlags::GATHER;
    if is_bindless {
        sources.push(next_reg(c, &mut src_b));
        flags |= TextureFlags::BINDLESS;
    }
    let coords_count = ty.dimensions();
    for _ in 0..coords_count {
        sources.push(next_reg(c, &mut src_a));
    }
    let is_1d_to_2d = SAMPLE_1D_AS_2D && ty.base() == SamplerType::TEXTURE_1D;
    if is_1d_to_2d {
        sources.push(c.const_f(0.0));
        ty = SamplerType::TEXTURE_2D | (ty & SamplerType::ARRAY);
    }
    if let Some(a) = array_index {
        sources.push(a);
    }
    let has_any_offset = offset == TexOffset::AOFFI || offset == TexOffset::PTP;
    let packed0 = if has_any_offset { Some(next_reg(c, &mut src_b)) } else { None };
    let packed1 = if offset == TexOffset::PTP { Some(next_reg(c, &mut src_b)) } else { None };
    if has_depth_compare {
        sources.push(next_reg(c, &mut src_b));
        ty = ty | SamplerType::SHADOW;
    }
    if has_any_offset {
        let offset_texels_count = if offset == TexOffset::PTP { 4 } else { 1 };
        for index in 0..coords_count * offset_texels_count {
            // packedOffs[1] is null without PTP; the original passes it on as a null source then.
            let packed = if (index >> 2) & 1 == 0 { packed0 } else { packed1 };
            let (o, s) = (c.const_i((index & 3) * 8), c.const_i(6));
            sources.push(c.bitfield_extract_s32(packed.expect("PTP offsets"), o, s));
        }
        if is_1d_to_2d {
            for _ in 0..offset_texels_count {
                sources.push(c.const_i(0));
            }
        }
        flags |= if offset == TexOffset::PTP { TextureFlags::OFFSETS } else { TextureFlags::OFFSET };
    }
    if !has_depth_compare {
        sources.push(c.const_i(component.0 as i32));
    }
    let dests = gpr_dests(c, dest, (component_mask as u32).count_ones());
    emit_texture_sample(c, ty, flags, imm, component_mask, &dests, &sources);
}

#[allow(clippy::too_many_arguments)]
fn emit_tmml(c: &mut Ctx, dimensions: TexDim, imm: i32, component_mask: i32, mut src_a: i32, mut src_b: i32, mut dest: i32, is_bindless: bool) {
    if dest == RZ {
        return;
    }
    let mut flags = TextureFlags::NONE;
    let mut sources = Vec::new();
    if is_bindless {
        sources.push(next_reg(c, &mut src_b));
        flags |= TextureFlags::BINDLESS;
    }
    let mut ty = convert_sampler_type(dimensions);
    let coords_count = ty.dimensions();
    let is_array = is_array_dim(dimensions);
    let array_index = if is_array { Some(next_reg(c, &mut src_a)) } else { None };
    for _ in 0..coords_count {
        sources.push(next_reg(c, &mut src_a));
    }
    if SAMPLE_1D_AS_2D && ty.base() == SamplerType::TEXTURE_1D {
        sources.push(c.const_f(0.0));
        ty = SamplerType::TEXTURE_2D | (ty & SamplerType::ARRAY);
    }
    if let Some(a) = array_index {
        sources.push(a);
    }
    let sb = if is_bindless {
        SetBindingPair::new(0, 0)
    } else {
        c.rm.texture_or_image_binding(&c.tc.gpu, I::LOD, ty, TextureFormat::Unknown, flags, DEFAULT_CBUF_SLOT, imm, 1, false)
    };
    let mut comp_mask = component_mask;
    let mut comp_index = 0;
    while comp_mask != 0 {
        if comp_mask & 1 != 0 {
            if dest >= RZ {
                break;
            }
            let d = c.gpr(dest);
            dest += 1;
            // Components z and w are not standard: zero, with a comment.
            if comp_index >= 2 {
                let n = c.ir.comment("Unsupported component z or w found".into());
                c.add_node(n);
                let z = c.const_i(0);
                c.copy_to(d, z);
            } else {
                // The instruction's component order is the inverse of GLSL's.
                let res = c.lod(ty, flags, sb, comp_index ^ 1, &sources);
                let k = c.const_f(256.0);
                let res = c.fp_multiply(res, k, I::FP32);
                let fixed_point_value = c.fp32_convert_to_s32(res);
                c.copy_to(d, fixed_point_value);
            }
        }
        comp_mask >>= 1;
        comp_index += 1;
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_txd(c: &mut Ctx, dimensions: TexDim, imm: i32, component_mask: i32, mut src_a: i32, mut src_b: i32, dest: i32, has_offset: bool, is_bindless: bool) {
    if dest == RZ {
        return;
    }
    let mut flags = TextureFlags::DERIVATIVES;
    let mut sources = Vec::new();
    if is_bindless {
        sources.push(next_reg(c, &mut src_a));
        flags |= TextureFlags::BINDLESS;
    }
    let mut ty = convert_sampler_type(dimensions);
    let coords_count = ty.dimensions();
    for _ in 0..coords_count {
        sources.push(next_reg(c, &mut src_a));
    }
    let is_1d_to_2d = SAMPLE_1D_AS_2D && ty.base() == SamplerType::TEXTURE_1D;
    if is_1d_to_2d {
        sources.push(c.const_f(0.0));
        ty = SamplerType::TEXTURE_2D | (ty & SamplerType::ARRAY);
    }
    let packed_params = next_reg(c, &mut src_a);
    if is_array_dim(dimensions) {
        let m = c.const_i(0xffff);
        sources.push(c.bitwise_and(packed_params, m));
    }
    // Derivatives (X and Y).
    for _ in 0..2 * coords_count {
        sources.push(next_reg(c, &mut src_b));
        if is_1d_to_2d {
            sources.push(c.const_f(0.0));
        }
    }
    if has_offset {
        for index in 0..coords_count {
            let (o, s) = (c.const_i(16 + index * 4), c.const_i(4));
            sources.push(c.bitfield_extract_s32(packed_params, o, s));
        }
        if is_1d_to_2d {
            sources.push(c.const_i(0));
        }
        flags |= TextureFlags::OFFSET;
    }
    let dests = gpr_dests(c, dest, (component_mask as u32).count_ones());
    emit_texture_sample(c, ty, flags, imm, component_mask, &dests, &sources);
}

fn emit_txq(c: &mut Ctx, query: TexQuery, imm: i32, component_mask: i32, mut src_a: i32, mut dest: i32, is_bindless: bool) {
    if dest == RZ {
        return;
    }
    let mut sources = Vec::new();
    if is_bindless {
        sources.push(next_reg(c, &mut src_a));
    }
    sources.push(next_reg(c, &mut src_a));

    fn get_dest(c: &mut Ctx, dest: &mut i32) -> Option<OperandId> {
        if *dest >= RZ {
            return None;
        }
        let d = c.gpr(*dest);
        *dest += 1;
        Some(d)
    }

    let ty = if is_bindless {
        if query == TexQuery::TEX_HEADER_TEXTURE_TYPE {
            SamplerType::TEXTURE_2D | SamplerType::MULTISAMPLE
        } else if component_mask & 4 != 0 {
            SamplerType::TEXTURE_3D
        } else {
            SamplerType::TEXTURE_2D
        }
    } else {
        c.tc.gpu.sampler_type(imm, DEFAULT_CBUF_SLOT)
    };
    let flags = if is_bindless { TextureFlags::BINDLESS } else { TextureFlags::NONE };
    match query {
        TexQuery::TEX_HEADER_DIMENSION => {
            let sb = if is_bindless {
                SetBindingPair::new(0, 0)
            } else {
                c.rm.texture_or_image_binding(&c.tc.gpu, I::TEXTURE_QUERY_SIZE, ty, TextureFormat::Unknown, flags, DEFAULT_CBUF_SLOT, imm, 1, false)
            };
            let mut comp_mask = component_mask;
            let mut comp_index = 0;
            while comp_mask != 0 {
                if comp_mask & 1 != 0 {
                    let Some(d) = get_dest(c, &mut dest) else { break };
                    let v = c.texture_query_size(ty, flags, sb, comp_index, &sources);
                    c.copy_to(d, v);
                }
                comp_mask >>= 1;
                comp_index += 1;
            }
        }
        TexQuery::TEX_HEADER_TEXTURE_TYPE => {
            let sb = if is_bindless {
                SetBindingPair::new(0, 0)
            } else {
                c.rm.texture_or_image_binding(&c.tc.gpu, I::TEXTURE_QUERY_SAMPLES, ty, TextureFormat::Unknown, flags, DEFAULT_CBUF_SLOT, imm, 1, false)
            };
            if component_mask & 4 != 0 {
                // Skip the first two components if they are written.
                if component_mask & 1 != 0 {
                    get_dest(c, &mut dest);
                }
                if component_mask & 2 != 0 {
                    get_dest(c, &mut dest);
                }
                if let Some(d) = get_dest(c, &mut dest) {
                    let v = c.texture_query_samples(ty, flags, sb, &sources);
                    c.copy_to(d, v);
                }
            }
        }
        _ => c.log(&format!("Invalid or unsupported query type \"{}\".", tex_query_name(query))),
    }
}

/// TexQuery.ToString().
fn tex_query_name(q: TexQuery) -> String {
    match q {
        TexQuery::TEX_HEADER_DIMENSION => "TexHeaderDimension".into(),
        TexQuery::TEX_HEADER_TEXTURE_TYPE => "TexHeaderTextureType".into(),
        TexQuery::TEX_HEADER_SAMPLER_POS => "TexHeaderSamplerPos".into(),
        TexQuery::TEX_SAMPLER_FILTER => "TexSamplerFilter".into(),
        TexQuery::TEX_SAMPLER_LOD => "TexSamplerLod".into(),
        TexQuery::TEX_SAMPLER_WRAP => "TexSamplerWrap".into(),
        TexQuery::TEX_SAMPLER_BORDER_COLOR => "TexSamplerBorderColor".into(),
        q => q.0.to_string(),
    }
}

fn emit_texture_sample(c: &mut Ctx, ty: SamplerType, flags: TextureFlags, handle: i32, component_mask: i32, dests: &[Option<OperandId>], sources: &[OperandId]) {
    let sb = if flags.has(TextureFlags::BINDLESS) {
        SetBindingPair::new(0, 0)
    } else {
        c.rm.texture_or_image_binding(&c.tc.gpu, I::TEXTURE_SAMPLE, ty, TextureFormat::Unknown, flags, DEFAULT_CBUF_SLOT, handle, 1, false)
    };
    c.texture_sample(ty, flags, sb, component_mask, dests, sources);
}

fn convert_sampler_type(dimensions: TexDim) -> SamplerType {
    match dimensions {
        TexDim::D1D => SamplerType::TEXTURE_1D,
        TexDim::ARRAY1D => SamplerType::TEXTURE_1D | SamplerType::ARRAY,
        TexDim::D2D => SamplerType::TEXTURE_2D,
        TexDim::ARRAY2D => SamplerType::TEXTURE_2D | SamplerType::ARRAY,
        TexDim::D3D => SamplerType::TEXTURE_3D,
        TexDim::ARRAY3D => SamplerType::TEXTURE_3D | SamplerType::ARRAY,
        TexDim::CUBE => SamplerType::TEXTURE_CUBE,
        _ => SamplerType::TEXTURE_CUBE | SamplerType::ARRAY,
    }
}

fn convert_sampler_type_texs(t: TexsTarget) -> SamplerType {
    match t {
        TexsTarget::TEXTURE1_D_LOD_ZERO => SamplerType::TEXTURE_1D,
        TexsTarget::TEXTURE2_D | TexsTarget::TEXTURE2_D_LOD_ZERO | TexsTarget::TEXTURE2_D_LOD_LEVEL => SamplerType::TEXTURE_2D,
        TexsTarget::TEXTURE2_D_DEPTH_COMPARE
        | TexsTarget::TEXTURE2_D_LOD_LEVEL_DEPTH_COMPARE
        | TexsTarget::TEXTURE2_D_LOD_ZERO_DEPTH_COMPARE => SamplerType::TEXTURE_2D | SamplerType::SHADOW,
        TexsTarget::TEXTURE2_D_ARRAY | TexsTarget::TEXTURE2_D_ARRAY_LOD_ZERO => SamplerType::TEXTURE_2D | SamplerType::ARRAY,
        TexsTarget::TEXTURE2_D_ARRAY_LOD_ZERO_DEPTH_COMPARE => SamplerType::TEXTURE_2D | SamplerType::ARRAY | SamplerType::SHADOW,
        TexsTarget::TEXTURE3_D | TexsTarget::TEXTURE3_D_LOD_ZERO => SamplerType::TEXTURE_3D,
        TexsTarget::TEXTURE_CUBE | TexsTarget::TEXTURE_CUBE_LOD_LEVEL => SamplerType::TEXTURE_CUBE,
        _ => SamplerType::NONE,
    }
}

fn convert_sampler_type_tlds(t: TldsTarget) -> SamplerType {
    match t {
        TldsTarget::TEXTURE1_D_LOD_ZERO | TldsTarget::TEXTURE1_D_LOD_LEVEL => SamplerType::TEXTURE_1D,
        TldsTarget::TEXTURE2_D_LOD_ZERO
        | TldsTarget::TEXTURE2_D_LOD_ZERO_OFFSET
        | TldsTarget::TEXTURE2_D_LOD_LEVEL
        | TldsTarget::TEXTURE2_D_LOD_LEVEL_OFFSET => SamplerType::TEXTURE_2D,
        TldsTarget::TEXTURE2_D_LOD_ZERO_MULTISAMPLE => SamplerType::TEXTURE_2D | SamplerType::MULTISAMPLE,
        TldsTarget::TEXTURE3_D_LOD_ZERO => SamplerType::TEXTURE_3D,
        TldsTarget::TEXTURE2_D_ARRAY_LOD_ZERO => SamplerType::TEXTURE_2D | SamplerType::ARRAY,
        _ => SamplerType::NONE,
    }
}

fn convert_texture_flags_texs(t: TexsTarget) -> TextureFlags {
    match t {
        TexsTarget::TEXTURE1_D_LOD_ZERO
        | TexsTarget::TEXTURE2_D_LOD_ZERO
        | TexsTarget::TEXTURE2_D_LOD_LEVEL
        | TexsTarget::TEXTURE2_D_LOD_LEVEL_DEPTH_COMPARE
        | TexsTarget::TEXTURE2_D_LOD_ZERO_DEPTH_COMPARE
        | TexsTarget::TEXTURE2_D_ARRAY_LOD_ZERO
        | TexsTarget::TEXTURE2_D_ARRAY_LOD_ZERO_DEPTH_COMPARE
        | TexsTarget::TEXTURE3_D_LOD_ZERO
        | TexsTarget::TEXTURE_CUBE_LOD_LEVEL => TextureFlags::LOD_LEVEL,
        _ => TextureFlags::NONE,
    }
}

fn convert_texture_flags_tlds(t: TldsTarget) -> TextureFlags {
    match t {
        TldsTarget::TEXTURE1_D_LOD_ZERO
        | TldsTarget::TEXTURE1_D_LOD_LEVEL
        | TldsTarget::TEXTURE2_D_LOD_ZERO
        | TldsTarget::TEXTURE2_D_LOD_LEVEL
        | TldsTarget::TEXTURE2_D_LOD_ZERO_MULTISAMPLE
        | TldsTarget::TEXTURE3_D_LOD_ZERO
        | TldsTarget::TEXTURE2_D_ARRAY_LOD_ZERO => TextureFlags::LOD_LEVEL,
        TldsTarget::TEXTURE2_D_LOD_ZERO_OFFSET | TldsTarget::TEXTURE2_D_LOD_LEVEL_OFFSET => TextureFlags::LOD_LEVEL | TextureFlags::OFFSET,
        _ => TextureFlags::NONE,
    }
}
