// Texture and image operations (codegen::instructions texture_sample, lod, texture_query_*, image_*), with the argument
// forms of SPIRV-Cross's CompilerMSL::to_function_name / to_function_args.

use super::helpers::Helper;
use super::{component, enclose, type_name, unsupported, Ctx, R};
use crate::ir::{Instruction as I, OperandType, TextureFlags};
use crate::structured::{AstId, AstOperation, AstTexture};
use crate::translation::definitions::AggregateType as A;
use crate::types::{SamplerType, ShaderStage};

type Res = R<(A, Option<String>)>;

struct Srcs<'o> {
    sources: &'o [Option<AstId>],
    index: usize,
}

impl Srcs<'_> {
    fn next_node(&mut self) -> R<Option<AstId>> {
        let n = self.sources.get(self.index).copied().ok_or("msl: missing texture source")?;
        self.index += 1;
        Ok(n)
    }
    fn next(&mut self, c: &mut Ctx, ty: A) -> R<String> {
        let n = self.next_node()?;
        c.expr(ty, n)
    }
}

fn vector(ty: &str, elems: Vec<String>) -> String {
    if elems.len() == 1 {
        elems.into_iter().next().unwrap()
    } else {
        format!("{ty}{}({})", elems.len(), elems.join(", "))
    }
}

/// Selects the components of `index`'s mask (codegen swizzled_result).
fn swizzle(e: String, mask: i32, vector_result: bool) -> R<String> {
    let comps: Vec<&str> = (0..4).filter(|i| mask & (1 << i) != 0).map(|i| component(i).unwrap()).collect();
    if vector_result {
        Ok(format!("{}.{}", enclose(e), comps.concat()))
    } else {
        let k = (mask as u32).trailing_zeros() as i32;
        Ok(format!("{}.{}", enclose(e), component(k)?))
    }
}

pub fn operation(c: &mut Ctx, o: &AstOperation) -> Res {
    let tex = o.tex.ok_or("msl: texture operation without texture")?;
    match o.inst.base() {
        I::TEXTURE_SAMPLE => sample(c, o, tex),
        I::LOD => lod(c, o, tex),
        I::TEXTURE_QUERY_SIZE => query_size(c, o, tex),
        I::TEXTURE_QUERY_SAMPLES => {
            let (img, _) = sampled_image(c, tex, &mut Srcs { sources: &o.sources, index: 0 })?;
            Ok((A::S32, Some(format!("int({img}.get_num_samples())"))))
        }
        I::IMAGE_LOAD => image_load(c, o, tex),
        I::IMAGE_STORE => image_store(c, o, tex),
        I::IMAGE_ATOMIC => unsupported!("image atomics"),
        _ => unsupported!("texture operation {}", o.inst.name()),
    }
}

/// The texture and sampler expressions (codegen sampled_image_load); reads the index sources of indexed ones.
fn sampled_image(c: &mut Ctx, tex: AstTexture, _srcs: &mut Srcs) -> R<(String, String)> {
    let key = tex.texture_set_and_binding();
    let t = c.texture(key)?;
    if t.array_length != 1 {
        unsupported!("texture arrays");
    }
    let name = t.name.clone();
    if tex.is_separate() {
        let sk = tex.sampler_set_and_binding();
        let s = c.texture(sk)?;
        if s.array_length != 1 {
            unsupported!("sampler arrays");
        }
        let sname = s.name.clone();
        c.note_texture(key, Some(sk));
        Ok((name, sname))
    } else {
        c.note_texture(key, None);
        Ok((name.clone(), format!("{name}Smplr")))
    }
}

fn is_zero_constant(c: &Ctx, n: Option<AstId>) -> bool {
    n.and_then(|n| c.info.ast.as_operand(n)).is_some_and(|o| o.ty == OperandType::Constant && o.value == 0)
}

fn sample(c: &mut Ctx, o: &AstOperation, tex: AstTexture) -> Res {
    let f = tex.flags;
    let is_gather = f.has(TextureFlags::GATHER);
    let has_derivatives = f.has(TextureFlags::DERIVATIVES);
    let int_coords = f.has(TextureFlags::INT_COORDS);
    let mut has_lod_bias = f.has(TextureFlags::LOD_BIAS);
    let mut has_lod_level = f.has(TextureFlags::LOD_LEVEL);
    let has_offset = f.has(TextureFlags::OFFSET);
    let has_offsets = f.has(TextureFlags::OFFSETS);
    let is_array = tex.ty.has(SamplerType::ARRAY);
    let is_ms = tex.ty.has(SamplerType::MULTISAMPLE);
    let is_shadow = tex.ty.has(SamplerType::SHADOW);
    let base = tex.ty.base();
    if base == SamplerType::TEXTURE_BUFFER {
        unsupported!("texture buffers");
    }

    let mut srcs = Srcs { sources: &o.sources, index: 0 };
    let (img, smp) = sampled_image(c, tex, &mut srcs)?;

    let dims = tex.ty.dimensions();
    let coord_type = if int_coords { A::S32 } else { A::FP32 };
    let mut coords = Vec::new();
    for _ in 0..dims {
        coords.push(srcs.next(c, coord_type)?);
    }
    let layer = if is_array { Some(srcs.next(c, A::S32)?) } else { None };
    // The coordinate vector (OpCompositeConstruct). With a layer SPIRV-Cross reads it twice (coordinates, then the
    // layer component), so it is a temporary.
    let cty = if int_coords { "int" } else { "float" };
    let (coord_operand, coord_expr, layer_expr) = match &layer {
        Some(l) => {
            let l = if int_coords { l.clone() } else { c.inst(A::FP32, format!("float({l})"), &[l])? };
            let mut elems = coords.clone();
            elems.push(l);
            let n = elems.len();
            let t = c.temp();
            c.pre.push(format!("{cty}{n} {t} = {};", vector(cty, elems)));
            // A temporary (SPIRV-Cross reads the vector twice): a new expression tree.
            c.deps.insert(t.clone(), 0);
            let sw = ["x", "xy", "xyz"][dims as usize - 1];
            let lc = ["y", "z", "w"][dims as usize - 1];
            (t.clone(), format!("{t}.{sw}"), Some(if int_coords { format!("uint({t}.{lc})") } else { format!("uint(rint({t}.{lc}))") }))
        }
        None if coords.len() > 1 => {
            let ty = if int_coords { A::S32 } else { A::FP32 } | [A::SCALAR, A::SCALAR, A::VECTOR2, A::VECTOR3][coords.len()];
            let refs: Vec<&str> = coords.iter().map(|s| s.as_str()).collect();
            let v = c.inst(ty, vector(cty, coords.clone()), &refs)?;
            (v.clone(), v, None)
        }
        None => (coords[0].clone(), coords[0].clone(), None),
    };
    let d_ref = if is_shadow { Some(srcs.next(c, A::FP32)?) } else { None };
    let gradient = |c: &mut Ctx, srcs: &mut Srcs| -> R<String> {
        let mut v = Vec::new();
        for _ in 0..dims {
            v.push(srcs.next(c, A::FP32)?);
        }
        if v.len() == 1 {
            return Ok(v.remove(0));
        }
        let ty = A::FP32 | [A::SCALAR, A::SCALAR, A::VECTOR2, A::VECTOR3][v.len()];
        let refs: Vec<&str> = v.iter().map(|s| s.as_str()).collect();
        let text = vector("float", v.clone());
        c.inst(ty, text, &refs)
    };
    let derivatives = if has_derivatives {
        let dx = gradient(c, &mut srcs)?;
        let dy = gradient(c, &mut srcs)?;
        Some((dx, dy, (srcs.index - 2 * dims as usize..srcs.index).map(|i| o.sources[i]).collect::<Vec<_>>()))
    } else {
        None
    };
    let mut sample_index = None;
    let mut lod = None;
    if is_ms {
        sample_index = Some(srcs.next(c, A::S32)?);
    } else if has_lod_level {
        lod = Some(srcs.next(c, coord_type)?);
    }
    let mut offsets: Vec<Vec<String>> = Vec::new();
    let offset_count = if has_offset { 1 } else if has_offsets { 4 } else { 0 };
    for _ in 0..offset_count {
        let mut v = Vec::new();
        for _ in 0..dims {
            v.push(srcs.next(c, A::S32)?);
        }
        offsets.push(v);
    }
    let mut lod_bias = if has_lod_bias { Some(srcs.next(c, A::FP32)?) } else { None };
    let mut bias_is_zero = false;
    if has_lod_bias {
        bias_is_zero = is_zero_constant(c, o.sources.get(srcs.index - 1).copied().flatten());
    }
    if !is_gather && !int_coords && !is_ms && !has_lod_level && !has_derivatives && c.stage != ShaderStage::Fragment {
        // Implicit LOD exists only in fragment shaders: the bias, if any, is the explicit LOD.
        lod = Some(lod_bias.take().unwrap_or_else(|| "0.0".into()));
        has_lod_bias = false;
        has_lod_level = true;
    }
    let comp_idx = if is_gather && !is_shadow {
        let n = srcs.next_node()?;
        let k = n.and_then(|n| c.info.ast.as_operand(n)).filter(|o| o.ty == OperandType::Constant).map(|o| o.value);
        Some(k.ok_or("msl: gather component must be constant")?)
    } else {
        None
    };

    // The function and its arguments.
    let dref = d_ref.is_some();
    let mut img_expr = img.clone();
    if dref {
        c.helper(Helper::DepthCast);
        img_expr = format!("spvDepthCast({img_expr})");
    }
    let mut args: Vec<String> = Vec::new();
    let fname;
    if has_offsets && is_gather {
        if dref {
            c.helper(Helper::GatherCompareReturn);
            c.helper(Helper::GatherCompareConstOffsets);
            fname = "spvGatherCompareConstOffsets".to_string();
        } else {
            c.helper(Helper::GatherReturn);
            c.helper(Helper::GatherConstOffsets);
            fname = "spvGatherConstOffsets".to_string();
        }
        args.push(img_expr.clone());
    } else {
        fname = format!("{img_expr}.{}{}", if int_coords { "read" } else if is_gather { "gather" } else { "sample" }, if dref { "_compare" } else { "" });
    }
    if !int_coords {
        args.push(smp);
    }
    if has_offsets && is_gather {
        let v: Vec<String> = offsets.iter().map(|o| vector("int", o.clone())).collect();
        args.push(format!("spvUnsafeArray<int2, 4>({{ {} }})", v.join(", ")));
        c.helper(Helper::UnsafeArray);
        if let Some(k) = comp_idx {
            args.push(format!("component::{}", component(k)?));
        }
    }

    let mut tc = if int_coords {
        match base {
            SamplerType::TEXTURE_1D => format!("uint({coord_expr})"),
            SamplerType::TEXTURE_2D => format!("uint2({coord_expr})"),
            SamplerType::TEXTURE_3D => format!("uint3({coord_expr})"),
            _ => unsupported!("fetch from {}", base.0),
        }
    } else {
        coord_expr
    };
    if int_coords && has_offset {
        tc = format!("{tc} + {}", match base {
            SamplerType::TEXTURE_1D => format!("uint({})", offsets[0][0]),
            SamplerType::TEXTURE_2D => format!("uint2({})", vector("int", offsets[0].clone())),
            _ => format!("uint3({})", vector("int", offsets[0].clone())),
        });
    }
    args.push(tc);
    if let Some(l) = layer_expr {
        args.push(l);
    }

    let mut lod_out = lod;
    let mut derivs = derivatives;
    // Operands the result depends on (SPIRV-Cross inherited_expressions: coordinates, dref, image operands).
    let mut operand_exprs: Vec<String> = vec![coord_operand];
    if let Some(r) = &d_ref {
        operand_exprs.push(r.clone());
    }
    if let Some((dx, dy, _)) = &derivs {
        operand_exprs.push(dx.clone());
        operand_exprs.push(dy.clone());
    }
    for e in [&lod_out, &lod_bias, &sample_index].into_iter().flatten() {
        operand_exprs.push(e.clone());
    }
    if let Some(r) = d_ref {
        args.push(r);
        // macOS: constant-zero gradients for a comparison become level(0).
        if let Some((_, _, nodes)) = &derivs {
            let zero = nodes.iter().all(|n| is_zero_constant(c, *n));
            if zero {
                lod_out = None;
                derivs = None;
                args.push("level(0)".into());
            }
        }
        if has_lod_bias && bias_is_zero {
            lod_bias = None;
        }
    }
    let one_d = base == SamplerType::TEXTURE_1D;
    if let Some(b) = lod_bias {
        if !one_d {
            args.push(format!("bias({b})"));
        }
    }
    if let Some(l) = lod_out {
        if !one_d {
            if int_coords {
                args.push(l);
            } else {
                args.push(format!("level({l})"));
            }
        }
    } else if int_coords && !one_d && !is_ms {
        args.push("0".into());
    }
    if let Some((dx, dy, _)) = derivs {
        if !one_d {
            let g = match base {
                SamplerType::TEXTURE_2D => "gradient2d",
                SamplerType::TEXTURE_3D => "gradient3d",
                SamplerType::TEXTURE_CUBE => "gradientcube",
                _ => unsupported!("gradients on {}", base.0),
            };
            args.push(format!("{g}({dx}, {dy})"));
        }
    }
    let mut has_offset_arg = false;
    if has_offset && !int_coords {
        match base {
            SamplerType::TEXTURE_2D | SamplerType::TEXTURE_3D => {
                args.push(vector("int", offsets[0].clone()));
                has_offset_arg = true;
            }
            _ => {}
        }
    }
    if let Some(k) = comp_idx {
        if !has_offsets {
            if base == SamplerType::TEXTURE_2D && !has_offset_arg {
                args.push("int2(0)".into());
            }
            args.push(format!("component::{}", component(k)?));
        }
    }
    if let Some(s) = sample_index {
        args.push(s);
    }
    let color_is_vector = is_gather || !is_shadow;
    let refs: Vec<&str> = operand_exprs.iter().map(|s| s.as_str()).collect();
    let e = c.inst(if color_is_vector { A::VECTOR4 | A::FP32 } else { A::FP32 }, format!("{fname}({})", args.join(", ")), &refs)?;
    if color_is_vector {
        let t = o.vector_type(A::FP32);
        let vec = t.count() != A::SCALAR;
        let sw = swizzle(e.clone(), o.index, vec)?;
        Ok((t, Some(c.inst(t, sw, &[&e])?)))
    } else {
        Ok((A::FP32, Some(e)))
    }
}

fn lod(c: &mut Ctx, o: &AstOperation, tex: AstTexture) -> Res {
    let mut srcs = Srcs { sources: &o.sources, index: 0 };
    let (img, smp) = sampled_image(c, tex, &mut srcs)?;
    let dims = tex.ty.dimensions();
    let mut coords = Vec::new();
    for _ in 0..dims {
        coords.push(srcs.next(c, A::FP32)?);
    }
    if tex.ty.base() == SamplerType::TEXTURE_1D {
        unsupported!("ImageQueryLod on 1D textures");
    }
    // SPIRV-Cross: a float2 temporary with both values, then the component.
    let t = c.temp();
    let cs = vector("float", coords);
    c.pre.push(format!("float2 {t};"));
    c.pre.push(format!("{t}.x = {img}.calculate_clamped_lod({smp}, {cs});"));
    c.pre.push(format!("{t}.y = {img}.calculate_unclamped_lod({smp}, {cs});"));
    let t = c.leaf(t);
    let r = c.inst(A::FP32, format!("{t}.{}", component(o.index)?), &[&t])?;
    Ok((A::FP32, Some(r)))
}

fn query_size(c: &mut Ctx, o: &AstOperation, tex: AstTexture) -> Res {
    let mut srcs = Srcs { sources: &o.sources, index: 0 };
    let (img, _) = sampled_image(c, tex, &mut srcs)?;
    if o.index == 3 {
        return Ok((A::S32, Some(format!("int({img}.get_num_mip_levels())"))));
    }
    let ty = c.texture(tex.texture_set_and_binding())?.ty;
    let has_lod = !ty.has(SamplerType::MULTISAMPLE) && ty != SamplerType::TEXTURE_BUFFER;
    let base = ty.base();
    let mut dimensions = if base == SamplerType::TEXTURE_CUBE { 2 } else { ty.dimensions() };
    if ty.has(SamplerType::ARRAY) {
        dimensions += 1;
    }
    let lod = if has_lod {
        let l = srcs.next(c, A::S32)?;
        if l == "0" { String::new() } else { l }
    } else {
        String::new()
    };
    let mut parts = vec![format!("{img}.get_width({lod})")];
    if matches!(base, SamplerType::TEXTURE_2D | SamplerType::TEXTURE_CUBE | SamplerType::TEXTURE_3D) {
        parts.push(format!("{img}.get_height({lod})"));
    }
    if base == SamplerType::TEXTURE_3D {
        parts.push(format!("{img}.get_depth({lod})"));
    }
    if ty.has(SamplerType::ARRAY) {
        parts.push(format!("{img}.get_array_size()"));
    }
    if dimensions == 1 {
        return Ok((A::S32, Some(format!("int({})", parts[0]))));
    }
    let v = format!("int{dimensions}({})", parts.join(", "));
    Ok((A::S32, Some(format!("{v}.{}", component(o.index)?))))
}

fn image_coords(c: &mut Ctx, srcs: &mut Srcs, tex: AstTexture) -> R<Vec<String>> {
    let base = tex.ty.base();
    let dims = tex.ty.dimensions();
    let mut coords = Vec::new();
    for _ in 0..dims {
        coords.push(srcs.next(c, A::S32)?);
    }
    let layer = if tex.ty.has(SamplerType::ARRAY) { Some(srcs.next(c, A::S32)?) } else { None };
    let (tc, layer) = match layer {
        Some(l) => {
            let t = c.temp();
            let n = coords.len() + 1;
            let mut elems = coords;
            elems.push(l);
            c.pre.push(format!("int{n} {t} = {};", vector("int", elems)));
            let sw = ["x", "xy", "xyz"][dims as usize - 1];
            let lc = ["y", "z", "w"][dims as usize - 1];
            (format!("{t}.{sw}"), Some(format!("{t}.{lc}")))
        }
        None => (vector("int", coords), None),
    };
    let mut out = vec![match base {
        SamplerType::TEXTURE_1D => format!("uint({tc})"),
        SamplerType::TEXTURE_2D => format!("uint2({tc})"),
        SamplerType::TEXTURE_3D => format!("uint3({tc})"),
        _ => unsupported!("image of type {}", base.0),
    }];
    if let Some(l) = layer {
        out.push(format!("uint({l})"));
    }
    Ok(out)
}

fn image_load(c: &mut Ctx, o: &AstOperation, tex: AstTexture) -> Res {
    let key = tex.texture_set_and_binding();
    let def = c.image(key)?;
    if def.array_length != 1 {
        unsupported!("image arrays");
    }
    let name = def.name.clone();
    c.note_image(key);
    let mut srcs = Srcs { sources: &o.sources, index: 0 };
    let coords = image_coords(c, &mut srcs, tex)?;
    let ct = tex.format.component_type();
    let e = format!("{name}.read({})", coords.join(", "));
    let t = o.vector_type(ct);
    Ok((ct, Some(swizzle(e, o.index, t.count() != A::SCALAR)?)))
}

fn image_store(c: &mut Ctx, o: &AstOperation, tex: AstTexture) -> Res {
    let key = tex.texture_set_and_binding();
    let def = c.image(key)?;
    if def.array_length != 1 {
        unsupported!("image arrays");
    }
    let name = def.name.clone();
    c.note_image(key);
    let mut srcs = Srcs { sources: &o.sources, index: 0 };
    let coords = image_coords(c, &mut srcs, tex)?;
    let ct = tex.format.component_type();
    let mut elems = Vec::new();
    for _ in 0..4 {
        if srcs.index < o.sources.len() {
            elems.push(srcs.next(c, ct)?);
        } else {
            elems.push(match ct {
                A::S32 => "0".into(),
                A::U32 => "0u".into(),
                _ => "0.0".into(),
            });
        }
    }
    let tn = type_name(ct)?;
    Ok((A::INVALID, Some(format!("{name}.write({tn}4({}), {})", elems.join(", "), coords.join(", ")))))
}
