// Transformations after optimisation (Translation.Transforms). With the engine's accessor answers only these run:
// DrawParametersReplace (vertex), TexturePass, SharedStoreSmallIntCas (compute with shared memory) and ShufflePass.
// ForcePreciseEnable (reduced precision), VectorComponentSelect (vector indexing bug), SharedAtomicSignedCas (not
// SPIR-V) and the vertex / geometry as compute passes are never enabled.

use super::definitions::{FeatureFlags, NVN_BASE_INSTANCE_BYTE_OFFSET, NVN_BASE_VERTEX_BYTE_OFFSET, NVN_DRAW_INDEX_BYTE_OFFSET};
use super::helper_functions::HelperFunctionName;
use super::optimizations::utils::delete_node;
use super::TransformContext;
use crate::ir::{Instruction as I, Ir, NodeId, OperandType, StorageKind, TextureFlags};
use crate::types::{SamplerType, ShaderStage};

type Pass = fn(&mut TransformContext, NodeId) -> Result<NodeId, String>;

pub fn run_pass(ctx: &mut TransformContext) -> Result<(), String> {
    let stage = ctx.stage;
    let used = *ctx.used_features;
    if stage == ShaderStage::Vertex {
        run(ctx, draw_parameters_replace)?;
    }
    run(ctx, texture_pass)?;
    if stage == ShaderStage::Compute && used.has(FeatureFlags::SHARED_MEMORY) {
        run(ctx, shared_store_small_int_cas)?;
    }
    if ctx.used_features.has(FeatureFlags::SHUFFLE) {
        run(ctx, shuffle_pass)?;
    }
    Ok(())
}

fn run(ctx: &mut TransformContext, pass: Pass) -> Result<(), String> {
    for &block in ctx.blocks {
        let mut node = ctx.ir.first_node(block);
        while let Some(n) = node {
            let n = if ctx.ir.is_op(n) { pass(ctx, n)? } else { n };
            node = ctx.ir.next_node(n);
        }
    }
    Ok(())
}

fn draw_parameters_replace(ctx: &mut TransformContext, n: NodeId) -> Result<NodeId, String> {
    // The accessor reports no constant buffer draw parameters: they are only noted as used.
    let ir = &*ctx.ir;
    for i in 0..ir.sources_count(n) {
        let src = ir.src(n, i);
        if ir.ty(src) == OperandType::ConstantBuffer && ir.opnd(src).cbuf_slot() == 0 {
            let off = ir.opnd(src).cbuf_offset();
            if off == NVN_BASE_VERTEX_BYTE_OFFSET / 4 || off == NVN_BASE_INSTANCE_BYTE_OFFSET / 4 || off == NVN_DRAW_INDEX_BYTE_OFFSET / 4 {
                *ctx.used_features |= FeatureFlags::DRAW_PARAMETERS;
                break;
            }
        }
    }
    Ok(n)
}

fn texture_pass(ctx: &mut TransformContext, n: NodeId) -> Result<NodeId, String> {
    let Some(tex) = ctx.ir.tex(n).copied() else { return Ok(n) };
    insert_texel_fetch_scale(ctx, n)?;
    insert_texture_size_unscale(ctx, n)?;
    if ctx.ir.inst(n) == I::TEXTURE_SAMPLE {
        // Coordinates are always normalised and the host needs no gather bias, supports non-constant and gather offsets
        // and SNORM buffer textures, so the remaining rewrites have nothing to do.
        let flags = tex.flags;
        let is_bindless = flags.has(TextureFlags::BINDLESS);
        let is_indexed = ctx.rm.is_array_of_textures_or_images(tex.binding, false);
        if !is_bindless && !is_indexed && ctx.rm.cbuf_slot_and_handle_for_texture(tex.binding).is_some() && !flags.has(TextureFlags::INT_COORDS) {
            debug_assert!(ctx.gpu.texture_coord_normalized(0, 0));
        }
        debug_assert!(ctx.gpu.host_gather_bias_precision() == 0);
        debug_assert!(ctx.gpu.host_supports_texture_gather_offsets() && ctx.gpu.host_supports_non_constant_texture_offset());
    }
    Ok(n)
}

fn type_supports_scale(ty: SamplerType) -> bool {
    ty.base() == SamplerType::TEXTURE_2D
}

/// Integer coordinates of 2D textures and images are scaled by the render scale (a helper function call each).
fn insert_texel_fetch_scale(ctx: &mut TransformContext, n: NodeId) -> Result<(), String> {
    let tex = *ctx.ir.tex(n).unwrap();
    let inst = ctx.ir.inst(n);
    let is_bindless = tex.flags.has(TextureFlags::BINDLESS);
    let int_coords = tex.flags.has(TextureFlags::INT_COORDS);
    let is_image = inst == I::IMAGE_LOAD;
    let is_indexed = ctx.rm.is_array_of_textures_or_images(tex.binding, is_image);
    if !((inst == I::TEXTURE_SAMPLE || is_image) && (int_coords || is_image) && !is_bindless && !is_indexed && ctx.stage.supports_render_scale() && type_supports_scale(tex.ty)) {
        return Ok(());
    }
    let function_id = ctx.hfm.get_or_create_function_id(ctx.ir, HelperFunctionName::TexelFetchScale)?;
    let sampler_index = if is_image {
        ctx.rm.texture_descriptors_without_arrays_count() + ctx.rm.find_image_descriptor_index(tex.binding)
    } else {
        ctx.rm.find_texture_descriptor_index(tex.binding)
    };
    let coords_count = tex.ty.dimensions();
    let coords_index = if is_bindless { 1 } else { 0 };
    let ir = &mut *ctx.ir;
    for index in 0..coords_count {
        let scaled_coord = ir.local();
        let fid = ir.const_i(function_id);
        let coord = ir.src(n, (coords_index + index) as usize);
        let si = ir.const_i(sampler_index);
        let call_args = if ctx.stage == ShaderStage::Fragment {
            let idx = ir.const_i(index);
            vec![fid, coord, si, idx]
        } else {
            vec![fid, coord, si]
        };
        let call = ir.operation(I::CALL, Some(scaled_coord), &call_args);
        ir.add_before(n, call);
        ir.set_source(n, (coords_index + index) as usize, Some(scaled_coord));
    }
    Ok(())
}

/// Sizes of 2D textures are unscaled by the render scale after the query.
fn insert_texture_size_unscale(ctx: &mut TransformContext, n: NodeId) -> Result<(), String> {
    let tex = *ctx.ir.tex(n).unwrap();
    let is_bindless = tex.flags.has(TextureFlags::BINDLESS);
    let is_indexed = ctx.rm.is_array_of_textures_or_images(tex.binding, false);
    if !(ctx.ir.inst(n) == I::TEXTURE_QUERY_SIZE
        && ctx.ir.op(n).index < 2
        && !is_bindless
        && !is_indexed
        && ctx.stage.supports_render_scale()
        && type_supports_scale(tex.ty))
    {
        return Ok(());
    }
    let function_id = ctx.hfm.get_or_create_function_id(ctx.ir, HelperFunctionName::TextureSizeUnscale)?;
    let sampler_index = ctx.rm.find_texture_descriptor_index(tex.binding);
    let ir: &mut Ir = ctx.ir;
    for index in (0..ir.dests_count(n)).rev() {
        let dest = ir.get_dest(n, index).unwrap();
        let unscaled_size = ir.local();
        // Every use takes the unscaled size, before the call (itself a use of the original) exists.
        for use_op in ir.use_ops(dest).to_vec() {
            for s in 0..ir.sources_count(use_op) {
                if ir.get_source_opt(use_op, s) == Some(dest) {
                    ir.set_source(use_op, s, Some(unscaled_size));
                }
            }
        }
        let (fid, si) = (ir.const_i(function_id), ir.const_i(sampler_index));
        let call = ir.operation(I::CALL, Some(unscaled_size), &[fid, dest, si]);
        ir.add_after(n, call);
    }
    Ok(())
}

fn shared_store_small_int_cas(ctx: &mut TransformContext, n: NodeId) -> Result<NodeId, String> {
    let sk = ctx.ir.op(n).storage_kind;
    let name = match sk {
        StorageKind::SharedMemory8 => HelperFunctionName::SharedStore8,
        StorageKind::SharedMemory16 => HelperFunctionName::SharedStore16,
        _ => return Ok(n),
    };
    if ctx.ir.inst(n) != I::STORE {
        return Ok(n);
    }
    let (memory_id, byte_offset, value) = (ctx.ir.src(n, 0), ctx.ir.src(n, 1), ctx.ir.src(n, 2));
    let function_id = ctx.hfm.get_or_create_function_id_with(ctx.ir, name, ctx.ir.value(memory_id))?;
    let fid = ctx.ir.const_i(function_id);
    let call = ctx.ir.operation(I::CALL, None, &[fid, byte_offset, value]);
    let new_node = ctx.ir.add_before(n, call);
    delete_node(ctx.ir, n);
    Ok(new_node)
}

/// Shuffles with a validity result become calls to helper functions that handle the segment mask and clamp.
fn shuffle_pass(ctx: &mut TransformContext, n: NodeId) -> Result<NodeId, String> {
    let name = match ctx.ir.inst(n) {
        I::SHUFFLE => HelperFunctionName::Shuffle,
        I::SHUFFLE_DOWN => HelperFunctionName::ShuffleDown,
        I::SHUFFLE_UP => HelperFunctionName::ShuffleUp,
        I::SHUFFLE_XOR => HelperFunctionName::ShuffleXor,
        _ => return Ok(n),
    };
    if ctx.ir.sources_count(n) != 3 || ctx.ir.dests_count(n) != 2 {
        return Ok(n);
    }
    let function_id = ctx.hfm.get_or_create_shuffle_function_id(ctx.ir, name, ctx.gpu.host_subgroup_size())?;
    let ir = &mut *ctx.ir;
    let (result, valid) = (ir.get_dest(n, 0), ir.get_dest(n, 1).unwrap());
    let (value, index, mask) = (ir.src(n, 0), ir.src(n, 1), ir.src(n, 2));
    ir.set_dest_single(n, None);
    let fid = ir.const_i(function_id);
    let call = ir.operation(I::CALL, result, &[fid, value, index, mask, valid]);
    let new_node = ir.add_before(n, call);
    delete_node(ir, n);
    Ok(new_node)
}
