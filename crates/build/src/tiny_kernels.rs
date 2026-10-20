// crates/cli/src/tiny_kernels.rs
//! Compute versions of translated stages: a render pass costs ~40 us of fixed GPU time on Apple
//! GPUs, a compute dispatch ~1 us, and TotK runs ~76 tiny render passes per frame. For a draw the engine runs as
//! compute, the vertex stage's `tkv_<entry>` evaluates the draw's few vertices into a buffer and the fragment stage's
//! `tkf_<entry>` shades every pixel of the viewport rectangle: coverage and perspective-correct interpolation from those
//! vertices, then the original fragment body. Both kernels reuse the stage's own code: the entry point is turned into
//! an inline body function (resource parameters keep their types, lose their attributes) that the kernel calls.
//!
//! A fragment stage gets a kernel only if its result does not depend on rasterization details the kernel does not
//! reproduce: no discard, no derivatives (dfdx / dfdy / fwidth, implicit-LOD queries), no depth or sample-mask output,
//! no simdgroup / quad operations, no device-memory writes or atomics, no inputs beyond stage_in and the position.
//! Implicit-LOD samples become level(0): the engine uses the kernel only when every sampled texture has one level.
//!
//! Kernel inputs (beyond the stage's own resources):
//!   fragment: buffer(27) tk_FParams, buffer(28) the vertex records the vertex kernel wrote, texture(24 + N) color
//!             output N (read_write: partial write masks are read-modify-write);
//!   vertex:   buffer(27) tk_VParams, buffer(28) the vertex records (output), buffer(25 + s) vertex stream s (s < 2).
//! Vertex record: TK_VSTRIDE float4s per vertex: [0] gl_Position, [1 + L] the varying at user location L.

macro_rules! tk_try {
    ($e:expr) => {
        match $e {
            Some(v) => v,
            None => {
                if crate::translation::get().debug_kernels {
                    eprintln!("[tiny-kernels] declined at tiny_kernels.rs:{}", line!());
                }
                return None;
            }
        }
    };
}

/// float4 slots per vertex record: gl_Position plus 32 user locations.
pub const TK_VSTRIDE: usize = 33;

/// Start of the appended kernel text (the builder cuts here to compile a stage without its kernels).
pub const MARKER: &str = "\n#ifndef TK_COMMON";

const COMMON: &str = r#"
#ifndef TK_COMMON
#define TK_COMMON
struct tk_FParams {
    float4 viewport;     // x, y, width, height
    float2 depth_range;  // near, far
    uint2 origin;        // first pixel of the dispatch
    uint2 size;          // pixels in the dispatch
    uint ntri;           // triangles (at most 2)
    uint masks;          // write mask per color output, 4 bits each (r, g, b, a)
    uint4 tri[2];        // vertex record indices of each triangle
    // Per color output: x = blending on, y = MTLBlendFactor src rgb | dst rgb << 8 | src a << 16 | dst a << 24,
    // z = MTLBlendOperation rgb | a << 8, w = 1 if the target is unorm / snorm (the source is clamped first).
    uint4 blend[8];
    float4 blend_color;
    // x = cull mode (0 none, 1 front, 2 back), y = 1 if front faces are clockwise as seen in window coordinates
    // (Metal's winding; y points down), zw unused.
    uint4 raster;
    // Pending clears folded into the draw (the rectangle is the whole target level): bit N = output N starts as
    // clear_color[N] (uncovered pixels get it, blending and write masks see it as the destination). x only; yzw unused.
    uint4 clear_mask;
    float4 clear_color[8];
};
static inline float4 tk_factor(uint f, float4 s, float4 d, float4 k) {
    switch (f) {
    case 0: return float4(0);
    case 1: return float4(1);
    case 2: return s;
    case 3: return 1.0 - s;
    case 4: return float4(s.a);
    case 5: return float4(1.0 - s.a);
    case 6: return d;
    case 7: return 1.0 - d;
    case 8: return float4(d.a);
    case 9: return float4(1.0 - d.a);
    case 10: return float4(float3(min(s.a, 1.0 - d.a)), 1.0);
    case 11: return k;
    case 12: return 1.0 - k;
    case 13: return float4(k.a);
    case 14: return float4(1.0 - k.a);
    default: return float4(1);
    }
}
static inline float4 tk_blend_op(uint op, float4 s, float4 d, float4 sf, float4 df) {
    switch (op) {
    case 1: return s * sf - d * df;
    case 2: return d * df - s * sf;
    case 3: return min(s, d);
    case 4: return max(s, d);
    default: return s * sf + d * df;
    }
}
// Fixed-function blending of one output: rgb and alpha each with their own factors and operation.
static inline float4 tk_blend(uint4 b, float4 s, float4 d, float4 k) {
    if (b.w != 0u) s = clamp(s, 0.0, 1.0);
    float4 rgb = tk_blend_op(b.z & 0xFFu, s, d, tk_factor(b.y & 0xFFu, s, d, k), tk_factor((b.y >> 8) & 0xFFu, s, d, k));
    float4 a = tk_blend_op((b.z >> 8) & 0xFFu, s, d, tk_factor((b.y >> 16) & 0xFFu, s, d, k), tk_factor((b.y >> 24) & 0xFFu, s, d, k));
    return float4(rgb.rgb, a.a);
}
struct tk_VAttr {
    uint stream;         // 0 or 1 (buffer 25 + stream)
    uint format;         // MTLVertexFormat
    uint offset;
    uint stride;         // 0: per-draw constant
};
struct tk_VParams {
    uint count;          // vertices to evaluate
    uint first;          // first vertex (arrays) / index position (elements)
    uint index_type;     // 0: no index buffer, 1: u8, 2: u16, 4: u32 (indices in stream slot 1 of the index buffer)
    int base_vertex;
    uint base_instance;
    uint pad[3];
    tk_VAttr attr[16];
};
static inline float tk_unorm(uint v, uint bits) { return float(v) / float((1u << bits) - 1u); }
static inline float tk_snorm(int v, uint bits) { return max(float(v) / float((1 << (bits - 1)) - 1), -1.0f); }
// Metal's vertex fetch: missing components read as 0, w as 1.
static inline float4 tk_fetch_f(const device uchar* p, uint fmt) {
    float4 r = float4(0, 0, 0, 1);
    switch (fmt) {
    case 7: case 8: case 9: case 47: { uint n = fmt == 47 ? 1 : fmt - 5; for (uint i = 0; i < n; i++) r[i] = tk_unorm(p[i], 8); break; }
    case 42: { r = float4(tk_unorm(p[2], 8), tk_unorm(p[1], 8), tk_unorm(p[0], 8), tk_unorm(p[3], 8)); break; }
    case 10: case 11: case 12: case 48: { uint n = fmt == 48 ? 1 : fmt - 8; for (uint i = 0; i < n; i++) r[i] = tk_snorm(int(as_type<char>(p[i])), 8); break; }
    case 19: case 20: case 21: case 51: { uint n = fmt == 51 ? 1 : fmt - 17; const device ushort* q = (const device ushort*)p; for (uint i = 0; i < n; i++) r[i] = tk_unorm(q[i], 16); break; }
    case 22: case 23: case 24: case 52: { uint n = fmt == 52 ? 1 : fmt - 20; const device short* q = (const device short*)p; for (uint i = 0; i < n; i++) r[i] = tk_snorm(int(q[i]), 16); break; }
    case 25: case 26: case 27: case 53: { uint n = fmt == 53 ? 1 : fmt - 23; const device half* q = (const device half*)p; for (uint i = 0; i < n; i++) r[i] = float(q[i]); break; }
    case 28: case 29: case 30: case 31: { uint n = fmt - 27; const device float* q = (const device float*)p; for (uint i = 0; i < n; i++) r[i] = q[i]; break; }
    case 41: { uint v = *(const device uint*)p; r = float4(tk_unorm(v & 1023u, 10), tk_unorm((v >> 10) & 1023u, 10), tk_unorm((v >> 20) & 1023u, 10), tk_unorm(v >> 30, 2)); break; }
    case 40: { int v = *(const device int*)p; r = float4(tk_snorm((v << 22) >> 22, 10), tk_snorm((v << 12) >> 22, 10), tk_snorm((v << 2) >> 22, 10), max(float(v >> 30), -1.0f)); break; }
    default: break;
    }
    return r;
}
static inline uint4 tk_fetch_u(const device uchar* p, uint fmt) {
    uint4 r = uint4(0, 0, 0, 1);
    switch (fmt) {
    case 1: case 2: case 3: case 45: { uint n = fmt == 45 ? 1 : fmt + 1; for (uint i = 0; i < n; i++) r[i] = p[i]; break; }
    case 13: case 14: case 15: case 49: { uint n = fmt == 49 ? 1 : fmt - 11; const device ushort* q = (const device ushort*)p; for (uint i = 0; i < n; i++) r[i] = q[i]; break; }
    case 36: case 37: case 38: case 39: { uint n = fmt - 35; const device uint* q = (const device uint*)p; for (uint i = 0; i < n; i++) r[i] = q[i]; break; }
    default: break;
    }
    return r;
}
static inline int4 tk_fetch_i(const device uchar* p, uint fmt) {
    int4 r = int4(0, 0, 0, 1);
    switch (fmt) {
    case 4: case 5: case 6: case 46: { uint n = fmt == 46 ? 1 : fmt - 2; for (uint i = 0; i < n; i++) r[i] = as_type<char>(p[i]); break; }
    case 16: case 17: case 18: case 50: { uint n = fmt == 50 ? 1 : fmt - 14; const device short* q = (const device short*)p; for (uint i = 0; i < n; i++) r[i] = q[i]; break; }
    case 32: case 33: case 34: case 35: { uint n = fmt - 31; const device int* q = (const device int*)p; for (uint i = 0; i < n; i++) r[i] = q[i]; break; }
    default: break;
    }
    return r;
}
#endif
"#;

pub(crate) struct Param {
    pub(crate) decl: String, // type and name, without attributes
    pub(crate) name: String,
    pub(crate) attr: String, // the [[...]] text, or empty
}

/// Splits at top-level commas. `angle`: '<' / '>' nest (declarations: texture2d<float, access::read>); in expressions
/// they are comparisons and must not.
fn split_top_mode(s: &str, angle: bool) -> Vec<String> {
    let (mut depth, mut cur, mut out) = (0i32, String::new(), Vec::new());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            '<' if angle => depth += 1,
            '>' if angle => depth -= 1,
            '[' if chars.peek() == Some(&'[') => depth += 1,
            ']' if chars.peek() == Some(&']') => depth -= 1,
            ',' if depth == 0 => {
                out.push(cur.trim().to_string());
                cur.clear();
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn split_top(s: &str) -> Vec<String> {
    split_top_mode(s, true)
}

pub(crate) fn parse_params(s: &str) -> Vec<Param> {
    split_top(s)
        .into_iter()
        .map(|p| {
            let (decl, attr) = match p.find("[[") {
                Some(i) => (p[..i].trim().to_string(), p[i..].trim().to_string()),
                None => (p.trim().to_string(), String::new()),
            };
            let name = decl.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("").to_string();
            Param { decl, name, attr }
        })
        .collect()
}

/// The entry point's (kind, return type, name, parameter text, body including braces, start, end) in `msl`.
pub(crate) fn entry(msl: &str, kind: &str) -> Option<(String, String, String, usize, usize)> {
    let pat = format!("\n{} ", kind);
    let start = msl.find(&pat)? + 1;
    let line_end = start + msl[start..].find('\n')?;
    let line = &msl[start..line_end];
    let open = line.find('(')?;
    let close = line.rfind(')')?;
    let head: Vec<&str> = line[kind.len() + 1..open].split_whitespace().collect();
    let (ret, name) = (head.first()?.to_string(), head.get(1)?.to_string());
    let params = line[open + 1..close].to_string();
    // Body: from the first '{' after the signature to its matching brace.
    let b0 = line_end + msl[line_end..].find('{')?;
    let mut depth = 0i32;
    let mut b1 = None;
    for (i, c) in msl[b0..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    b1 = Some(b0 + i + 1);
                    break;
                }
            }
            _ => {}
        }
    }
    let _ = (ret.clone(), name.clone());
    Some((ret, name, params, b0, b1?))
}

/// Struct body text of `struct <name>` in `msl`.
pub(crate) fn struct_body<'a>(msl: &'a str, name: &str) -> Option<&'a str> {
    let i = msl.find(&format!("struct {}\n{{", name))?;
    let b0 = i + msl[i..].find('{')? + 1;
    let b1 = b0 + msl[b0..].find("};")?;
    Some(&msl[b0..b1])
}

/// Rewrites implicit-LOD sample calls on the given textures to level(0) (bias options too). None if a call cannot be
/// rewritten safely.
fn explicit_lod(body: &str, textures: &[(String, String)], implicit: &mut Vec<String>) -> Option<String> {
    let mut out = String::with_capacity(body.len() + 256);
    let mut rest = body;
    loop {
        let next = [".sample(", ".sample_compare("].iter().filter_map(|p| rest.find(p).map(|i| (i, *p))).min();
        let Some((i, pat)) = next else {
            out.push_str(rest);
            break;
        };
        // The texture: the identifier right before the dot.
        let before = &rest[..i];
        let mut tex = before.rsplit(|c: char| !(c.is_alphanumeric() || c == '_')).next().unwrap_or("");
        // spirv-cross samples depth textures declared as color ones through spvDepthCast(<texture>).
        if tex.is_empty() && before.ends_with(')') {
            if let Some(o) = before.rfind("spvDepthCast(") {
                tex = &before[o + "spvDepthCast(".len()..before.len() - 1];
            }
        }
        if crate::translation::get().debug_kernels && !textures.iter().any(|(n, _)| n == tex) {
            eprintln!("[tiny-kernels] texture {:?} not among {:?}", tex, textures);
        }
        let ty = tk_try!(textures.iter().find(|(n, _)| n == tex).map(|(_, t)| t.as_str()));
        let a0 = i + pat.len();
        let mut depth = 1i32;
        let mut a1 = None;
        for (k, c) in rest[a0..].char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        a1 = Some(a0 + k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let a1 = tk_try!(a1);
        let mut args = split_top_mode(&rest[a0..a1], false);
        let arrayed = ty.contains("_array");
        let base = 2 + arrayed as usize + (pat == ".sample_compare(") as usize;
        if args.len() < base {
            if crate::translation::get().debug_kernels { eprintln!("[tiny-kernels] sample args {:?}", args); }
            return None;
        }
        let lod = args[base..].iter().position(|a| a.starts_with("level(") || a.starts_with("gradient") || a.starts_with("bias(") || a.starts_with("min_lod_clamp("));
        // Implicit LOD (none given, bias, min_lod_clamp) becomes level 0, exact only for one-level textures: the
        // engine checks those (index line `tk <bindings>`). Explicit levels and gradients stay as they are.
        match lod {
            Some(k) if args[base + k].starts_with("bias(") || args[base + k].starts_with("min_lod_clamp(") => {
                args[base + k] = "level(0)".into();
                implicit.push(tex.to_string());
            }
            Some(_) => {}
            None => {
                args.insert(base, "level(0)".into());
                implicit.push(tex.to_string());
            }
        }
        out.push_str(&rest[..a0]);
        out.push_str(&args.join(", "));
        rest = &rest[a1..];
    }
    Some(out)
}

/// Comment line in a fragment kernel listing the Metal texture bindings it samples with implicit LOD (comma list, maybe
/// empty); the cache builder copies it into the index (`tk <list>`).
pub const IMPLICIT_TAG: &str = "// tk-implicit-lod: ";

/// Fragment kernels that apply folded clears (tk_FParams.clear_mask; index line `tkc`).
pub fn applies_clears(kernel: &str) -> bool {
    kernel.contains("tk.clear_mask.x")
}

/// The bindings an appended fragment kernel samples with implicit LOD (see IMPLICIT_TAG).
pub fn implicit_tag(kernel: &str) -> Option<String> {
    let i = kernel.find(IMPLICIT_TAG)? + IMPLICIT_TAG.len();
    Some(kernel[i..].lines().next().unwrap_or("").trim().to_string())
}

// Atomics are fine (a kernel thread runs once per covered pixel, as the fragment does; TotK's are order-independent
// add / min / max reductions); simd_is_helper_thread() is false in a kernel (see fragment_kernel).
/// None, logging the generator line with --debug-kernels.
fn tk_none<T>(line: u32) -> Option<T> {
    if crate::translation::get().debug_kernels {
        eprintln!("[tiny-kernels] declined at tiny_kernels.rs:{}", line);
    }
    None
}

const INELIGIBLE: &[&str] = &["dfdx", "dfdy", "fwidth", "simd_", "quad_", "calculate_clamped_lod",
    "calculate_unclamped_lod", "sample_mask", "access::write", "access::read_write", "threadgroup"];

/// Appends `tkf_<entry>` to a translated fragment stage when eligible (see module docs). None: no kernel.
pub fn fragment_kernel(msl: &str) -> Option<String> {
    let (ret, name, params, b0, b1) = entry(msl, "fragment")?;
    // Kernels have no helper threads.
    let body_owned = msl[b0..b1].replace("simd_is_helper_thread()", "false");
    let body = body_owned.as_str();
    let sig_and_body = format!("{}{}", &msl[msl.find("\nfragment ")?..b0], body);
    if INELIGIBLE.iter().any(|w| sig_and_body.contains(w)) {
        return tk_none(line!());
    }
    // Atomic results that feed back into the shading (not just reductions) would depend on the order threads run in.
    // (spirv-cross assigns every result to a temporary: those must not be read anywhere else.)
    if body.contains("atomic_") {
        if ["atomic_load", "atomic_store", "atomic_exchange", "atomic_compare_exchange", "atomic_thread_fence"].iter().any(|w| body.contains(w)) {
            return tk_none(line!());
        }
        // Results and plain copies of them (`uint _68 = _176;`) must not be read anywhere else.
        let mut tainted: Vec<String> = regex::Regex::new(r"(\w+) = atomic_fetch_").unwrap().captures_iter(body).map(|c| c[1].to_string()).collect();
        let copy = regex::Regex::new(r"\w+ (\w+) = (\w+);").unwrap();
        loop {
            let more: Vec<String> = copy.captures_iter(body).filter(|c| tainted.contains(&c[2].to_string()) && !tainted.contains(&c[1].to_string()))
                .map(|c| c[1].to_string()).collect();
            if more.is_empty() {
                break;
            }
            tainted.extend(more);
        }
        for t in &tainted {
            let uses = regex::Regex::new(&format!(r"\b{}\b", regex::escape(t))).unwrap().find_iter(body).count();
            let copies_from = copy.captures_iter(body).filter(|c| &c[2] == t.as_str()).count();
            if uses > 1 + copies_from {
                return tk_none(line!());
            }
        }
    }
    // discard_fragment(): the pixel keeps its target contents (the kernel skips its writes). The discarding shaders'
    // depth output (msl_specialize discard_depth) only exists with a depth target, which tiny passes never have.
    let discards = body.contains("discard_fragment()");
    let ps = parse_params(&params);
    let mut in_ty = None;
    let mut pass: Vec<&Param> = Vec::new();
    for p in &ps {
        if p.attr.contains("stage_in") {
            in_ty = Some(p.decl.split_whitespace().next()?.to_string());
        } else if p.attr.contains("[[position]]") {
            // gl_FragCoord: the kernel passes its own pixel position.
        } else if p.attr.contains("buffer(") || p.attr.contains("texture(") || p.attr.contains("sampler(") {
            if p.decl.starts_with("device ") && discards {
                // Storage writes after a discard do not happen on the GPU; a returning kernel body would not match
                // writes made before it either way, so these stay render draws.
                return tk_none(line!());
            }
            pass.push(p);
        } else {
            return tk_none(line!()); // front_facing, sample_id, color (framebuffer fetch), ...
        }
    }
    if pass.iter().any(|p| p.attr.contains("buffer(27)") || p.attr.contains("buffer(28)") || (24..40).any(|i| p.attr.contains(&format!("texture({})", i)))) {
        return tk_none(line!());
    }
    let textures: Vec<(String, String)> = pass.iter().filter(|p| p.decl.starts_with("texture") || p.decl.starts_with("depth"))
        .map(|p| (p.name.clone(), p.decl.split('<').next().unwrap_or("").to_string())).collect();
    let mut implicit = Vec::new();
    let body = tk_try!(explicit_lod(body, &textures, &mut implicit));
    // Metal texture bindings sampled with implicit LOD (see explicit_lod).
    let mut implicit_bindings: Vec<u32> = pass.iter()
        .filter(|p| implicit.contains(&p.name))
        .filter_map(|p| regex::Regex::new(r"texture\((\d+)\)").unwrap().captures(&p.attr).and_then(|c| c[1].parse().ok()))
        .collect();
    implicit_bindings.sort_unstable();
    implicit_bindings.dedup();
    let body = if discards {
        let out_var = format!("{}_out out = {{}};", name);
        if !body.contains(&out_var) || body.matches("discard_fragment()").count() != body.matches("discard_fragment();").count() {
            tk_try!(None::<()>);
        }
        body.replace("discard_fragment();", "{ tk_disc = true; return out; }")
    } else {
        body
    };
    // Stage-in members: (type, name, location, interpolation, function constant).
    let mut members = Vec::new();
    if let Some(ty) = &in_ty {
        for line in struct_body(msl, ty)?.lines() {
            let line = line.trim().trim_end_matches(';');
            if line.is_empty() {
                continue;
            }
            let caps = tk_try!(regex::Regex::new(r"^(\w+) (\w+) \[\[user\(locn(\d+)\)([^\]]*)\]\]$").unwrap().captures(line));
            let rest = caps.get(4).map_or("", |m| m.as_str());
            let fc = regex::Regex::new(r"function_constant\((\w+)\)").unwrap().captures(rest).map(|c| c[1].to_string());
            let interp = if rest.contains("flat") { "flat" } else if rest.contains("no_perspective") { "linear" } else { "persp" };
            members.push((caps[1].to_string(), caps[2].to_string(), caps[3].parse::<usize>().ok()?, interp, fc));
        }
    }
    // A tiny pass draw is at most 2 triangles (3-6 vertices) for fullscreen quads/blurs/downsamples.
    // Fragment shaders taking more than 4 varyings are complex 3D surface shaders (tangents, normals, shadow coords, etc.)
    // that never qualify for tiny passes.
    if members.len() > 4 {
        return tk_none(line!());
    }
    // Color outputs: (type, member, color index, function constant).
    let mut outs = Vec::new();
    for line in struct_body(msl, &ret)?.lines() {
        let line = line.trim().trim_end_matches(';');
        if line.is_empty() || line.contains("[[depth(") {
            continue;
        }
        let caps = tk_try!(regex::Regex::new(r"^(\w+) (\w+) \[\[color\((\d+)\)(?:, function_constant\((\w+)\))?\]\]$").unwrap().captures(line));
        outs.push((caps[1].to_string(), caps[2].to_string(), caps[3].parse::<usize>().ok()?, caps.get(4).map(|m| m.as_str().to_string())));
    }
    // Tiny passes have at most 6 targets (cubemap MRTs) or 1-2 targets (blurs/downsamples).
    if outs.is_empty() || outs.len() > 6 || outs.iter().any(|o| o.2 > 7) {
        return tk_none(line!());
    }
    let decls: Vec<String> = pass.iter().map(|p| p.decl.clone()).collect();
    let names: Vec<String> = pass.iter().map(|p| p.name.clone()).collect();
    let in_param = in_ty.as_ref().map(|t| format!("{} in, ", t)).unwrap_or_default();
    let in_arg = if in_ty.is_some() { "tk_in, " } else { "" };
    let mut k = String::new();
    k.push_str(COMMON);
    k.push_str(&format!("\n{}{}\n", IMPLICIT_TAG, implicit_bindings.iter().map(|b| b.to_string()).collect::<Vec<_>>().join(",")));
    k.push_str(&format!("\nstatic inline {} {}_tkbody({}float4 gl_FragCoord{}{}{})\n{}\n", ret, name, in_param,
        if decls.is_empty() { "" } else { ", " }, decls.join(", "), if discards { ", thread bool& tk_disc" } else { "" }, body));
    let mut kparams: Vec<String> = pass.iter().map(|p| format!("{} {}", p.decl, p.attr)).collect();
    kparams.push("constant tk_FParams& tk [[buffer(27)]]".into());
    kparams.push("const device float4* tk_v [[buffer(28)]]".into());
    for (i, (ty, m, c, fc)) in outs.iter().enumerate() {
        let elem = match ty.as_str() { t if t.starts_with("uint") => "uint", t if t.starts_with("int") => "int", _ => "float" };
        let fcs = fc.as_ref().map(|f| format!(", function_constant({})", f)).unwrap_or_default();
        let _ = (i, m);
        // Written through 24 + N; read (blending, partial write masks) through 32 + N, the same texture bound twice:
        // read_write access is not available for every color format (R11G11B10F).
        kparams.push(format!("texture2d<{}, access::write> tk_o_{} [[texture({}){}]]", elem, m, 24 + c, fcs));
        kparams.push(format!("texture2d<{}, access::read> tk_r_{} [[texture({}){}]]", elem, m, 32 + c, fcs));
    }
    kparams.push("uint2 tk_gid [[thread_position_in_grid]]".into());
    k.push_str(&format!("\nkernel void tkf_{}({})\n{{\n", name, kparams.join(", ")));
    k.push_str(r#"    if (tk_gid.x >= tk.size.x || tk_gid.y >= tk.size.y) return;
    uint2 tk_pix = tk.origin + tk_gid;
    float2 tk_pc = float2(tk_pix) + 0.5;
    // Coverage and barycentrics (window space, Metal's viewport transform).
    float3 tk_b = float3(0);
    uint3 tk_t = uint3(0);
    float3 tk_w = float3(1), tk_z = float3(0);
    bool tk_hit = false;
    for (uint t = 0; t < tk.ntri && !tk_hit; t++) {
        uint3 ix = tk.tri[t].xyz;
        float2 s[3];
        float ws[3], zs[3];
        for (uint j = 0; j < 3; j++) {
            float4 c = tk_v[ix[j] * 33];
            float iw = 1.0 / c.w;
            float3 ndc = c.xyz * iw;
            s[j] = float2(tk.viewport.x + (ndc.x * 0.5 + 0.5) * tk.viewport.z, tk.viewport.y + (0.5 - ndc.y * 0.5) * tk.viewport.w);
            ws[j] = c.w;
            zs[j] = tk.depth_range.x + ndc.z * (tk.depth_range.y - tk.depth_range.x);
        }
        float2 e0 = s[1] - s[0], e1 = s[2] - s[0], d = tk_pc - s[0];
        float area = e0.x * e1.y - e0.y * e1.x;
        if (area == 0.0) continue;
        // Facing: with y pointing down, a positive area is clockwise on screen.
        if (tk.raster.x != 0u) {
            bool front = (area > 0.0) == (tk.raster.y != 0u);
            if ((tk.raster.x == 1u && front) || (tk.raster.x == 2u && !front)) continue;
        }
        float b1 = (d.x * e1.y - d.y * e1.x) / area;
        float b2 = (e0.x * d.y - e0.y * d.x) / area;
        float b0 = 1.0 - b1 - b2;
        if (b0 >= 0.0 && b1 >= 0.0 && b2 >= 0.0) {
            tk_hit = true; tk_b = float3(b0, b1, b2); tk_t = ix;
            tk_w = float3(ws[0], ws[1], ws[2]); tk_z = float3(zs[0], zs[1], zs[2]);
        }
    }
    if (!tk_hit) {
        if (tk.clear_mask.x != 0u) {
TK_CLEAR_UNCOVERED        }
        return;
    }
    float3 tk_pb = tk_b / tk_w;
    float tk_invw = tk_pb.x + tk_pb.y + tk_pb.z;
    tk_pb /= tk_invw;
    float4 gl_FragCoord = float4(tk_pc, dot(tk_b, tk_z), tk_invw);
"#);
    if let Some(ty) = &in_ty {
        k.push_str(&format!("    {} tk_in;\n", ty));
        for (mty, mname, loc, interp, fc) in &members {
            let l = loc + 1;
            let val = if mty.starts_with("float") && *interp != "flat" {
                let w = if *interp == "linear" { "tk_b" } else { "tk_pb" };
                let sum = format!("(tk_v[tk_t.x * {s} + {l}] * {w}.x + tk_v[tk_t.y * {s} + {l}] * {w}.y + tk_v[tk_t.z * {s} + {l}] * {w}.z)", s = TK_VSTRIDE, l = l, w = w);
                format!("{}{}", sum, swizzle(mty)?)
            } else {
                // Flat: the provoking (first) vertex's value, bit for bit (integer varyings travel as raw bits).
                format!("as_type<{}4>(tk_v[tk_t.x * {} + {}]){}", scalar(mty)?, TK_VSTRIDE, l, swizzle(mty)?)
            };
            match fc {
                Some(f) => k.push_str(&format!("    if ({}) tk_in.{} = {};\n", f, mname, val)),
                None => k.push_str(&format!("    tk_in.{} = {};\n", mname, val)),
            }
        }
    }
    if discards {
        k.push_str("    bool tk_disc = false;\n");
    }
    k.push_str(&format!("    {} tk_o = {}_tkbody({}gl_FragCoord{}{}{});\n", ret, name, in_arg, if names.is_empty() { "" } else { ", " }, names.join(", "),
        if discards { ", tk_disc" } else { "" }));
    if discards {
        k.push_str("    if (tk_disc) return;\n");
    }
    // Pending clears folded into the draw (tk.clear_mask): float outputs only (the engine folds no others).
    let mut clear_writes = String::new();
    for (ty, m, c, fc) in &outs {
        if !(ty.starts_with("uint") || ty.starts_with("int")) {
            let guard = fc.as_ref().map(|f| f.to_string()).unwrap_or_else(|| "true".into());
            clear_writes.push_str(&format!("            if ({g} && ((tk.clear_mask.x >> {c}) & 1u) != 0u) tk_o_{m}.write(tk.clear_color[{c}], tk_pix);\n", g = guard, c = c, m = m));
        }
    }
    k = k.replace("TK_CLEAR_UNCOVERED", &clear_writes);
    if discards {
        k = k.replace("    if (tk_disc) return;\n", &format!("    if (tk_disc) {{\n        if (tk.clear_mask.x != 0u) {{\n{}        }}\n        return;\n    }}\n", clear_writes));
    }
    for (ty, m, c, fc) in &outs {
        let guard = fc.as_ref().map(|f| f.to_string()).unwrap_or_else(|| "true".into());
        let vty = match ty.as_str() { t if t.starts_with("uint") => "uint4", t if t.starts_with("int") => "int4", _ => "float4" };
        if vty == "float4" {
            k.push_str(&format!(
                "    if ({g}) {{\n        uint m = (tk.masks >> {sh}) & 0xFu;\n        bool clr = ((tk.clear_mask.x >> {c}) & 1u) != 0u;\n        if (m != 0u || clr) {{\n            float4 dst = clr ? tk.clear_color[{c}] : ((tk.blend[{c}].x != 0u || m != 0xFu) ? float4(tk_r_{m}.read(tk_pix)) : float4(0));\n            float4 v = float4(tk_o.{m});\n            if (m != 0u && tk.blend[{c}].x != 0u) v = tk_blend(tk.blend[{c}], v, dst, tk.blend_color);\n            if (m != 0xFu) v = select(dst, v, bool4(m & 1u, m & 2u, m & 4u, m & 8u));\n            tk_o_{m}.write(v, tk_pix);\n        }}\n    }}\n",
                g = guard, sh = 4 * c, c = c, m = m));
        } else {
            k.push_str(&format!(
                "    if ({g}) {{\n        uint m = (tk.masks >> {sh}) & 0xFu;\n        if (m != 0u) {{\n            {vt} v = {vt}(tk_o.{m});\n            if (m != 0xFu) {{ {vt} old = tk_r_{m}.read(tk_pix); v = select(old, v, bool4(m & 1u, m & 2u, m & 4u, m & 8u)); }}\n            tk_o_{m}.write(v, tk_pix);\n        }}\n    }}\n",
                g = guard, sh = 4 * c, vt = vty, m = m));
        }
    }
    k.push_str("}\n");
    Some(k)
}

/// Appends `tkv_<entry>` to a translated vertex stage (see module docs). None: no kernel.
pub fn vertex_kernel(msl: &str) -> Option<String> {
    let (ret, name, params, b0, b1) = entry(msl, "vertex")?;
    let body = &msl[b0..b1];
    let sig_and_body = &msl[msl.find("\nvertex ")?..b1];
    if ["atomic", "simd_", "threadgroup", "access::write", "access::read_write"].iter().any(|w| sig_and_body.contains(w)) {
        return None;
    }
    let ps = parse_params(&params);
    let mut in_ty = None;
    let mut pass: Vec<&Param> = Vec::new();
    let mut vid_name = None;
    let mut iid_name = None;
    let mut bv_name = None;
    let mut bi_name = None;
    for p in &ps {
        if p.attr.contains("stage_in") {
            in_ty = Some(p.decl.split_whitespace().next()?.to_string());
        } else if p.attr.contains("[[vertex_id]]") {
            vid_name = Some(p.name.clone());
        } else if p.attr.contains("[[instance_id]]") {
            iid_name = Some(p.name.clone());
        } else if p.attr.contains("[[base_vertex]]") {
            bv_name = Some(p.name.clone());
        } else if p.attr.contains("[[base_instance]]") {
            bi_name = Some(p.name.clone());
        } else if p.attr.contains("buffer(") || p.attr.contains("texture(") || p.attr.contains("sampler(") {
            if p.decl.starts_with("device ") {
                return None;
            }
            pass.push(p);
        } else {
            return None;
        }
    }
    if pass.iter().any(|p| (25..=28).any(|i| p.attr.contains(&format!("buffer({})", i)))) {
        return None;
    }
    let textures: Vec<(String, String)> = pass.iter().filter(|p| p.decl.starts_with("texture") || p.decl.starts_with("depth"))
        .map(|p| (p.name.clone(), p.decl.split('<').next().unwrap_or("").to_string())).collect();
    let body = explicit_lod(body, &textures, &mut Vec::new())?; // vertex samples are explicit already; same rewrite for safety
    // Attributes: (type, member, location, function constant).
    let mut attrs = Vec::new();
    if let Some(ty) = &in_ty {
        for line in struct_body(msl, ty)?.lines() {
            let line = line.trim().trim_end_matches(';');
            if line.is_empty() {
                continue;
            }
            let caps = regex::Regex::new(r"^(\w+) (\w+) \[\[attribute\((\d+)\)(?:, function_constant\((\w+)\))?\]\]$").unwrap().captures(line)?;
            attrs.push((caps[1].to_string(), caps[2].to_string(), caps[3].parse::<usize>().ok()?, caps.get(4).map(|m| m.as_str().to_string())));
        }
    }
    // Fullscreen quads/blurs only have at most 4 attributes (position, uv, color) from streams 0/1.
    if attrs.len() > 4 || attrs.iter().any(|a| a.2 > 15) {
        return None;
    }
    // Outputs: gl_Position and user locations.
    let mut outs = Vec::new();
    let mut pos = None;
    for line in struct_body(msl, &ret)?.lines() {
        let line = line.trim().trim_end_matches(';');
        if line.is_empty() {
            continue;
        }
        if let Some(c) = regex::Regex::new(r"^\w+ (\w+) \[\[position").unwrap().captures(line) {
            pos = Some(c[1].to_string());
        } else if let Some(c) = regex::Regex::new(r"^(\w+) (\w+) \[\[user\(locn(\d+)\)").unwrap().captures(line) {
            outs.push((c[1].to_string(), c[2].to_string(), c[3].parse::<usize>().ok()?));
        } else if line.contains("point_size") || line.contains("clip_distance") {
            // not needed by the fragment kernel
        } else {
            return None;
        }
    }
    let pos = pos?;
    // Fullscreen quads/blurs pass at most 4 varyings (uv, color).
    if outs.len() > 4 || outs.iter().any(|o| o.2 > 31) {
        return None;
    }
    let mut decls: Vec<String> = pass.iter().map(|p| p.decl.clone()).collect();
    let mut args: Vec<String> = pass.iter().map(|p| p.name.clone()).collect();
    if let Some(v) = &vid_name {
        decls.push(format!("uint {}", v));
        args.push("tk_vid".into());
    }
    if let Some(i) = &iid_name {
        decls.push(format!("uint {}", i));
        args.push("tk_iid".into());
    }
    // As Metal passes them: vertex_id includes the base vertex, instance_id the base instance (tk_vid / tk_iid do too).
    if let Some(b) = &bv_name {
        decls.push(format!("uint {}", b));
        args.push("uint(tv.base_vertex)".into());
    }
    if let Some(b) = &bi_name {
        decls.push(format!("uint {}", b));
        args.push("tv.base_instance".into());
    }
    let in_param = in_ty.as_ref().map(|t| format!("{} in{}", t, if decls.is_empty() { "" } else { ", " })).unwrap_or_default();
    let in_arg = if in_ty.is_some() { format!("tk_in{}", if args.is_empty() { "" } else { ", " }) } else { String::new() };
    let mut k = String::new();
    k.push_str(COMMON);
    k.push_str(&format!("\nstatic inline {} {}_tkvbody({}{})\n{}\n", ret, name, in_param, decls.join(", "), body));
    let mut kparams: Vec<String> = pass.iter().map(|p| format!("{} {}", p.decl, p.attr)).collect();
    kparams.push("constant tk_VParams& tv [[buffer(27)]]".into());
    kparams.push("device float4* tk_v [[buffer(28)]]".into());
    kparams.push("const device uchar* tk_s0 [[buffer(25)]]".into());
    kparams.push("const device uchar* tk_s1 [[buffer(26)]]".into());
    kparams.push("uint tk_k [[thread_position_in_grid]]".into());
    k.push_str(&format!("\nkernel void tkv_{}({})\n{{\n", name, kparams.join(", ")));
    k.push_str(r#"    if (tk_k >= tv.count) return;
    // Vertex number: from the index buffer (stream slot 1 holds it when the draw is indexed) or consecutive.
    uint tk_vid;
    if (tv.index_type == 0) tk_vid = tv.first + tk_k;
    else if (tv.index_type == 1) tk_vid = uint(tk_s1[tv.first + tk_k]);
    else if (tv.index_type == 2) tk_vid = uint(((const device ushort*)tk_s1)[tv.first + tk_k]);
    else tk_vid = ((const device uint*)tk_s1)[tv.first + tk_k];
    tk_vid = uint(int(tk_vid) + tv.base_vertex);
    uint tk_iid = tv.base_instance;
"#);
    if let Some(ty) = &in_ty {
        k.push_str(&format!("    {} tk_in;\n", ty));
        for (aty, m, loc, fc) in &attrs {
            let f = match aty.as_str() { t if t.starts_with("uint") => "tk_fetch_u", t if t.starts_with("int") => "tk_fetch_i", _ => "tk_fetch_f" };
            let fetch = format!("{f}((tv.attr[{l}].stream == 0 ? tk_s0 : tk_s1) + tv.attr[{l}].offset + tv.attr[{l}].stride * tk_vid, tv.attr[{l}].format)", f = f, l = loc);
            match fc {
                Some(c) => k.push_str(&format!("    if ({}) tk_in.{} = {};\n", c, m, fetch)),
                None => k.push_str(&format!("    tk_in.{} = {};\n", m, fetch)),
            }
        }
    }
    k.push_str(&format!("    {} tk_o = {}_tkvbody({}{});\n", ret, name, in_arg, args.join(", ")));
    k.push_str(&format!("    tk_v[tk_k * {}] = tk_o.{};\n", TK_VSTRIDE, pos));
    for (oty, m, loc) in &outs {
        // Padded to four components; integer varyings as raw bits (the fragment kernel reads them back with as_type).
        let n = width(oty)?;
        let pad = match n { 1 => ", 0, 0, 0", 2 => ", 0, 0", 3 => ", 0", _ => "" };
        let sc = scalar(oty)?;
        let conv = if sc == "float" { format!("float4(tk_o.{}{})", m, pad) } else { format!("as_type<float4>({}4(tk_o.{}{}))", sc, m, pad) };
        k.push_str(&format!("    tk_v[tk_k * {} + {}] = {};\n", TK_VSTRIDE, loc + 1, conv));
    }
    k.push_str("}\n");
    Some(k)
}

/// Component type of a scalar / vector type name (float, int, uint; half is not expected in translated stages).
fn scalar(ty: &str) -> Option<&'static str> {
    let base = ty.trim_end_matches(|c: char| c.is_ascii_digit());
    match base { "float" => Some("float"), "int" => Some("int"), "uint" => Some("uint"), _ => None }
}

/// Number of components of a scalar / vector type name.
fn width(ty: &str) -> Option<usize> {
    let d = ty.trim_start_matches(|c: char| c.is_ascii_alphabetic());
    if d.is_empty() { Some(1) } else { d.parse().ok().filter(|n| (1..=4).contains(n)) }
}

/// Swizzle taking a value of this type from a four-component one.
fn swizzle(ty: &str) -> Option<&'static str> {
    Some(match width(ty)? { 1 => ".x", 2 => ".xy", 3 => ".xyz", _ => "" })
}
