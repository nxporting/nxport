// crates/cli/src/msl_specialize.rs
// Makes a translated stage's vertex inputs / fragment outputs format-agnostic, and the other MSL passes the Metal
// backend needs (port of the former tools/msl_specialize.py; its output is byte-identical, checked over ~4,800 stages).
//
// The Switch GPU's registers are untyped: vertex fetch and the ROP convert to/from raw 32-bit words according to the
// bound format, and the shader code decides what the bits mean. MSL inputs and outputs are typed, and Metal rejects a
// vertex format whose class differs from the declared type. sass2spv therefore translates with raw `uint4` inputs and
// outputs (see Program.cs, QueryGraphicsState); this pass rewrites each into three declarations at the same location, of
// which exactly one exists in a given pipeline, selected by a function constant that the engine sets from the real vertex
// / render target format:
//
//     kind 0  float-class  (float, half, unorm, snorm, and integer formats that the NVN format scales to float)
//     kind 1  unsigned integer
//     kind 2  signed integer
//
// Function constant ids: vertex attribute N -> N (0..31), fragment color output N -> 32 + N (0..7), vertex clip-space
// mode -> 40 (`depth_gl`: true when NVN's depth mode is NEAR_IS_MINUS_W, i.e. clip z in [-w, w]), vertex y negation -> 41
// (`y_negate`), default point size -> 42 (`point_size_fc`), fragment depth output present -> 43 (`has_depth_target_fc`),
// fragment input (varying) location N present -> 48 + N (`inN_fc`).
//
// Metal refuses pipelines the Switch GPU happily runs: a fragment shader that writes depth with no depth target (the
// write is dropped on the GPU), and a fragment shader reading a varying its vertex shader never writes (undefined on the
// GPU; TotK pairs material vertex shaders with fragment shaders that read more). The depth output and every
// `user(locnN)` input are therefore function-constant conditional; the engine clears the constant when there is no depth
// target / the vertex stage does not write location N (the input then reads as zero). Unset constants mean "present".
//
// Vertex stages also get the NVN -> Metal clip-space fix-up before each `return out;`. With NVN's UPPER_LEFT window
// origin (TotK; the D3D convention) clip y = +1 is framebuffer row 0, as in Metal, so y is kept; with LOWER_LEFT (the GL
// default) row 0 is clip y = -1 and y is negated (`y_negate`, which also reverses triangle winding: the engine flips the
// front-face setting to match). A [-w, w] clip z is remapped to Metal's [0, w]. Ryujinx's own remap is left disabled
// (DepthMode=false in Program.cs) because the mode is runtime device state. The shader body keeps seeing the raw uint4
// it had before: inputs are bit-cast at function entry and outputs are written to a raw local that is converted just
// before each `return out;`.

use regex::{Captures, Regex};
use std::sync::{OnceLock, RwLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Contract {
    /// No fused math.
    None,
    /// Fused math in vertex stages.
    Vertex,
    /// Fused math everywhere.
    All,
}

#[derive(Clone, Copy, Debug)]
pub struct TranslationOptions {
    pub contract_fast: Contract,
    pub keep_optnone: bool,
    pub fuse_guards: bool,
    pub no_invariant: bool,
    pub no_cbuf_limits: bool,
    pub no_discard_depth: bool,
    pub early_tests: bool,
}

impl Default for TranslationOptions {
    fn default() -> Self {
        Self {
            contract_fast: Contract::Vertex,
            keep_optnone: false,
            fuse_guards: false,
            no_invariant: false,
            no_cbuf_limits: false,
            no_discard_depth: false,
            early_tests: false,
        }
    }
}

static CURRENT_OPTIONS: RwLock<Option<TranslationOptions>> = RwLock::new(None);

pub fn set_options(options: TranslationOptions) {
    *CURRENT_OPTIONS.write().unwrap() = Some(options);
}

pub fn get_options() -> TranslationOptions {
    CURRENT_OPTIONS.read().unwrap().unwrap_or_default()
}

macro_rules! re {
    ($pat:expr) => {{
        static R: OnceLock<Regex> = OnceLock::new();
        R.get_or_init(|| Regex::new($pat).unwrap())
    }};
}

fn attr_re() -> &'static Regex {
    re!(r"(?m)^(\s+)uint4 (\w+) \[\[attribute\((\d+)\)\]\];")
}
fn color_re() -> &'static Regex {
    re!(r"(?m)^(\s+)uint4 (\w+) \[\[color\((\d+)\)\]\];")
}
fn using_re() -> &'static Regex {
    re!(r"(?m)^using namespace metal;\s*$")
}
fn depth_re() -> &'static Regex {
    re!(r"(?m)^(\s+float gl_FragDepth \[\[depth\(\w+\))\]\];")
}
fn varying_re() -> &'static Regex {
    re!(r"(?m)^(\s+)(\w+) (\w+) \[\[user\(locn(\d+)\)((?:, \w+)*)\]\];")
}

fn decl(indent: &str, name: &str, loc_attr: &str, const_name: &str) -> String {
    // `function_constant()` takes a named constant, not an expression: see kind_consts().
    [("float4", "f"), ("uint4", "u"), ("int4", "i")]
        .iter()
        .map(|(ty, suffix)| format!("{indent}{ty} {name}_{suffix} [[{loc_attr}, function_constant({const_name}_{suffix})]];"))
        .collect::<Vec<_>>()
        .join("\n")
}

pub const LOOP_GUARD_LIMIT: usize = 65536;

/// Caps every unbounded loop (`do {} while`, `for (;;)`, `while (true)`) at LOOP_GUARD_LIMIT iterations. A decompiler
/// control-flow bug can turn a forward branch into a loop that never exits (seen in a TotK UI vertex shader:
/// `do { ... if (_57) continue; } while (true || _57);`). On the GPU that hangs the device, which stalls the whole macOS
/// display. Real shader loops run far fewer iterations, so the guard only ever fires on such bugs.
pub fn guard_loops(msl: &str) -> String {
    let head = re!(r"^(\s*)(do|for \(;;\)|while \(true\))\s*$");
    let lines: Vec<&str> = msl.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let (mut n, mut i) = (0, 0);
    while i < lines.len() {
        let line = lines[i];
        if let Some(m) = head.captures(line) {
            if i + 1 < lines.len() && lines[i + 1].trim() == "{" {
                let ind = &m[1];
                out.push(format!("{ind}uint _loop_guard{n} = 0u;"));
                out.push(line.to_string());
                out.push(lines[i + 1].to_string());
                out.push(format!(
                    "{ind}    if (++_loop_guard{n} > {LOOP_GUARD_LIMIT}u) break;"
                ));
                n += 1;
                i += 2;
                continue;
            }
        }
        out.push(line.to_string());
        i += 1;
    }
    out.join("\n")
}

/// Metal allows sampler indices 0..15 per function; Maxwell shaders can use more. Samplers from index 16 up are passed
/// in an argument buffer at this buffer index instead (an array of sampler handles the engine fills with gpuResourceID).
pub const EXTRA_SAMPLER_BASE: usize = 16;
pub const EXTRA_SAMPLER_BUFFER: usize = 29;

/// Metal has no double. Maxwell shaders use a few FP64 ops; the decompiler wraps FP64 values in
/// PackDouble2x32(UnpackDouble2x32(x).x, UnpackDouble2x32(x).y) round trips (identities). Collapse those and compute in
/// float; any other use of the unsupported helpers is left for the Metal compiler to reject.
pub fn demote_doubles(msl: &str) -> String {
    if !msl.contains("double") {
        return msl.to_string();
    }
    // The original pattern back-references the first name; the regex crate has no back-references, so both names are
    // captured and the match is kept only when they are equal (\w+ cannot run into the ')' after either name).
    let ident = re!(
        r"unsupported_GLSLstd450PackDouble2x32\(uint2\(unsupported_GLSLstd450UnpackDouble2x32\((\w+)\)\.x, unsupported_GLSLstd450UnpackDouble2x32\((\w+)\)\.y\)\)"
    );
    let msl = ident.replace_all(msl, |c: &Captures| {
        if c[1] == c[2] {
            c[1].to_string()
        } else {
            c[0].to_string()
        }
    });
    re!(r"\bdouble([234]?)\b")
        .replace_all(&msl, "float${1}")
        .into_owned()
}

pub fn move_extra_samplers(msl: &str) -> String {
    let extra: Vec<(String, usize)> = re!(r",\s*sampler (\w+) \[\[sampler\((\d+)\)\]\]")
        .captures_iter(msl)
        .map(|c| (c[1].to_string(), c[2].parse::<usize>().unwrap()))
        .filter(|(_, k)| *k >= EXTRA_SAMPLER_BASE)
        .collect();
    if extra.is_empty() {
        return msl.to_string();
    }
    let mut msl = msl.to_string();
    for (name, k) in &extra {
        let r = Regex::new(&format!(
            r",\s*sampler {} \[\[sampler\({}\)\]\]",
            regex::escape(name),
            k
        ))
        .unwrap();
        msl = r.replace_all(&msl, "").into_owned();
    }
    let count = extra.iter().map(|(_, k)| *k).max().unwrap() - EXTRA_SAMPLER_BASE + 1;
    let Some(entry) = re!(r"(?ms)^(fragment|vertex|kernel) .*?(\)\s*\n\{\n)").captures(&msl) else {
        return msl;
    };
    let (e_start, e_end, g2_start) = (
        entry.get(0).unwrap().start(),
        entry.get(0).unwrap().end(),
        entry.get(2).unwrap().start(),
    );
    let locals: String = extra
        .iter()
        .map(|(name, k)| {
            format!(
                "    sampler {name} = spvExtraSmpl.s[{}];\n",
                k - EXTRA_SAMPLER_BASE
            )
        })
        .collect();
    let msl = format!(
        "{}, constant spvExtraSamplers& spvExtraSmpl [[buffer({EXTRA_SAMPLER_BUFFER})]]{}{}{}",
        &msl[..g2_start],
        &msl[g2_start..e_end],
        locals,
        &msl[e_end..]
    );
    let structure = format!("struct spvExtraSamplers\n{{\n    array<sampler, {count}> s;\n}};\n\n");
    format!("{}{}{}", &msl[..e_start], structure, &msl[e_start..])
}

/// Ryujinx marks Maxwell's separate FMUL/FADD as NoContraction (the hardware never fuses them into an FMA); spirv-cross
/// implements that with [[clang::optnone]] helper functions (spvFMul, spvFAdd, ...), one non-inlined call per operation,
/// which made fragment shaders several times slower. Instead, turn contraction off for the whole file: separate
/// multiplies and adds then stay separate everywhere (as on Maxwell), explicit fma() calls are unaffected, and the
/// helpers inline. --keep-optnone keeps spirv-cross's helpers (for A/B; needs a cache rebuild).
pub fn no_contraction(msl: &str) -> String {
    let opts = get_options();
    if !msl.contains("[[clang::optnone]]") || opts.keep_optnone {
        return msl.to_string();
    }
    // The helpers were also never optimized under fast math's no-NaN/no-infinity assumptions; inlined into fast-math code
    // they are, and TotK's shaders rely on IEEE inf/NaN there (fast math rendered every frame black). Relaxed mode keeps
    // the other fast-math freedoms but honours inf/NaN.
    let mut msl = msl.replace("[[clang::optnone]] ", "");
    // --contract-fast: let the compiler fuse multiplies and adds. The helpers' fma(l, r, 0) / fma(1, l, r) keep every
    // Maxwell FMUL/FADD a separate instruction; as plain operators under contract(fast) they can become one FMA. Default
    // "vertex" (2026-10-05: ~12% less GPU time in Hateno, frames match); =off restores contract(off) everywhere, =1 fuses
    // fragment shaders too (white specks on characters).
    let fuse = match opts.contract_fast {
        Contract::None => false,
        Contract::Vertex => re!(r"(?m)^vertex ").is_match(&msl),
        Contract::All => true,
    };
    if fuse {
        msl = msl
            .replace("return fma(l, r, T(0));", "return l * r;")
            .replace("return fma(T(1), l, r);", "return l + r;");
        msl = msl.replace("return fma(T(-1), r, l);", "return l - r;");
        // --fuse-guards (experiment): fused rounding can push a value that is exactly 0 on Maxwell slightly below it,
        // and sqrt / rsqrt / pow of it is NaN (white specks). Clamp their arguments at 0 (only negative inputs change).
        let guards = if opts.fuse_guards {
            "#include <metal_stdlib>\n\
             #define sqrt(x) metal::sqrt(metal::max((x), (x) * 0))\n\
             #define rsqrt(x) metal::rsqrt(metal::max((x), (x) * 0))\n\
             #define pow(x, y) metal::pow(metal::max((x), (x) * 0), (y))\n"
        } else {
            ""
        };
        // Fused math must not change the position: TotK draws foliage (and other alpha-tested geometry) in a depth-only
        // pre-pass and then a colour pass with an EQUAL depth test, through different vertex shaders. Rounding the same
        // position computation differently in the two left whole 16x16 tiles of tree canopy without colour (black
        // squares, 2026-10-10). [[invariant]] plus -fpreserve-invariance (the cache build) keeps the position
        // bit-identical across shaders while everything else stays fused. --no-invariant disables (A/B).
        if !opts.no_invariant {
            msl = msl.replace(
                "float4 gl_Position [[position]];",
                "float4 gl_Position [[position, invariant]];",
            );
        }
        return format!(
            "#pragma METAL fp math_mode(relaxed)\n#pragma METAL fp contract(fast)\n{guards}{msl}"
        );
    }
    format!("#pragma METAL fp math_mode(relaxed)\n#pragma METAL fp contract(off)\n{msl}")
}

/// Marks the fragment entry point [[early_fragment_tests]]. Used for stages whose GLSLC control section sets the
/// early-fragment-tests flag (+0x718 bit 0): the NVN driver then sets Maxwell's EarlyZForce (confirmed against the
/// register value Ryujinx sees for all 210 fragment stages of the opening). Without it Metal runs the depth test after
/// any fragment shader that writes memory, which disables early-Z and hidden surface removal for those draws.
pub fn early_fragment_tests(msl: &str) -> String {
    let frag = re!(r"(?m)^fragment ");
    if !frag.is_match(msl) || msl.contains("[[early_fragment_tests]]") || msl.contains("[[depth(") {
        return msl.to_string();
    }
    frag.replacen(msl, 1, "[[early_fragment_tests]]\nfragment ")
        .into_owned()
}

/// Inserts `text` after the `using namespace metal;` line (the end of its match, as the original did).
fn after_using(msl: &str, text: &str) -> String {
    let end = using_re()
        .find(msl)
        .expect("MSL without `using namespace metal;`")
        .end();
    format!("{}{}{}", &msl[..end], text, &msl[end..])
}

fn replace_word(body: &str, pattern: &str, with: &str) -> String {
    Regex::new(pattern)
        .unwrap()
        .replace_all(body, regex::NoExpand(with))
        .into_owned()
}

/// The specialized MSL with the stage's (attribute locations, color output locations, varying locations). Varyings:
/// the user(locnN) locations a vertex stage writes or a fragment stage reads.
pub fn specialize(msl: &str) -> (String, Vec<u32>, Vec<u32>, Vec<u32>) {
    let mut msl = no_contraction(&guard_loops(msl));
    let attrs: Vec<(String, u32)> = attr_re()
        .captures_iter(&msl)
        .map(|c| (c[2].to_string(), c[3].parse().unwrap()))
        .collect();
    let colors: Vec<(String, u32)> = color_re()
        .captures_iter(&msl)
        .map(|c| (c[2].to_string(), c[3].parse().unwrap()))
        .collect();
    let is_vertex = re!(r"(?m)^vertex ").is_match(&msl);
    let is_fragment = re!(r"(?m)^fragment ").is_match(&msl);
    let varyings: Vec<u32> = if is_vertex || is_fragment {
        let mut v: Vec<u32> = varying_re()
            .captures_iter(&msl)
            .map(|c| c[4].parse().unwrap())
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    } else {
        Vec::new()
    };
    if is_fragment {
        msl = fragment_optional_inputs(&discard_depth(&msl));
    }
    if is_vertex || is_fragment {
        let compute_attrs = [
            ("thread_position_in_threadgroup", "gl_LocalInvocationID", "uint3(0)", "uint3"),
            ("threadgroup_position_in_grid", "gl_WorkGroupID", "uint3(0)", "uint3"),
            ("thread_index_in_threadgroup", "gl_LocalInvocationIndex", "0u", "uint"),
            ("thread_execution_width", "gl_SubgroupSize", "32u", "uint"),
        ];
        let mut global_decls = Vec::new();
        for (attr, var, default_val, ty) in compute_attrs {
            if msl.contains(attr) {
                let pat1 = format!(r",\s*[a-zA-Z0-9_]+\s+{}\s+\[\[{}\]\]", var, attr);
                let pat2 = format!(r"[a-zA-Z0-9_]+\s+{}\s+\[\[{}\]\],\s*", var, attr);
                let pat3 = format!(r"[a-zA-Z0-9_]+\s+{}\s+\[\[{}\]\]", var, attr);
                msl = Regex::new(&pat1).unwrap().replace_all(&msl, "").into_owned();
                msl = Regex::new(&pat2).unwrap().replace_all(&msl, "").into_owned();
                msl = Regex::new(&pat3).unwrap().replace_all(&msl, "").into_owned();
                global_decls.push(format!("constant {ty} {var} = {default_val};"));
            }
        }
        if !global_decls.is_empty() {
            msl = after_using(&msl, &format!("\n{}\n", global_decls.join("\n")));
        }
    }
    if attrs.is_empty() && colors.is_empty() && !is_vertex {
        return (finish(&msl), Vec::new(), Vec::new(), varyings);
    }

    let kind_consts = |prefix: &str, loc: u32, fc_id: u32| {
        vec![
            format!("constant int {prefix}{loc}_kind [[function_constant({fc_id})]];"),
            format!("constant bool {prefix}{loc}_kind_f = {prefix}{loc}_kind == 0;"),
            format!("constant bool {prefix}{loc}_kind_u = {prefix}{loc}_kind == 1;"),
            format!("constant bool {prefix}{loc}_kind_i = {prefix}{loc}_kind == 2;"),
        ]
    };
    let mut consts: Vec<String> = Vec::new();
    if is_vertex {
        consts.push("constant bool depth_gl [[function_constant(40)]];".into());
        consts.push("constant bool y_negate [[function_constant(41)]];".into());
        // nvnCommandBufferSetPointSize: the rasterized point size when the shader does not write gl_PointSize.
        consts.push("constant float point_size_fc [[function_constant(42)]];".into());
        consts.push("constant float point_size = is_function_constant_defined(point_size_fc) ? point_size_fc : 1.0;".into());
    }
    for (_, loc) in &attrs {
        consts.extend(kind_consts("attr", *loc, *loc));
    }
    for (_, loc) in &colors {
        consts.extend(kind_consts("out", *loc, 32 + *loc));
    }

    msl = attr_re()
        .replace_all(&msl, |c: &Captures| {
            decl(
                &c[1],
                &c[2],
                &format!("attribute({})", &c[3]),
                &format!("attr{}_kind", &c[3]),
            )
        })
        .into_owned();
    msl = color_re()
        .replace_all(&msl, |c: &Captures| {
            decl(
                &c[1],
                &c[2],
                &format!("color({})", &c[3]),
                &format!("out{}_kind", &c[3]),
            )
        })
        .into_owned();
    msl = after_using(&msl, &format!("\n\n{}\n", consts.join("\n")));

    // Entry point body: the opening brace of the (single) entry function.
    let entry_re = re!(r"(?ms)^(vertex|fragment) .*?\)\s*\n\{\n");
    let mut entry_end = entry_re.find(&msl).map(|m| m.end());
    if !attrs.is_empty() {
        let mut prologue = String::new();
        for (name, loc) in &attrs {
            prologue += &format!(
                "    uint4 {name} = attr{loc}_kind == 0 ? as_type<uint4>(in.{name}_f) : (attr{loc}_kind == 1 ? in.{name}_u : as_type<uint4>(in.{name}_i));\n"
            );
        }
        let body_start = entry_end.expect("vertex inputs without an entry point");
        let (head, mut body) = (msl[..body_start].to_string(), msl[body_start..].to_string());
        for (name, _) in &attrs {
            body = replace_word(&body, &format!(r"\bin\.{}\b", regex::escape(name)), name);
        }
        msl = head + &prologue + &body;
        entry_end = entry_re.find(&msl).map(|m| m.end());
    }

    if !colors.is_empty() {
        let raw_decl: String = colors
            .iter()
            .map(|(name, _)| format!("    uint4 {name}_raw = uint4(0u);\n"))
            .collect();
        let mut epilogue = String::new();
        for (name, loc) in &colors {
            epilogue += &format!(
                "    if (out{loc}_kind == 0) out.{name}_f = as_type<float4>({name}_raw);\n    else if (out{loc}_kind == 1) out.{name}_u = {name}_raw;\n    else out.{name}_i = as_type<int4>({name}_raw);\n"
            );
        }
        let body_start = entry_end.expect("color outputs without an entry point");
        let (head, mut body) = (msl[..body_start].to_string(), msl[body_start..].to_string());
        for (name, _) in &colors {
            body = replace_word(
                &body,
                &format!(r"\bout\.{}\b", regex::escape(name)),
                &format!("{name}_raw"),
            );
        }
        body = body.replace("return out;", &format!("{epilogue}    return out;"));
        msl = head + &raw_decl + &body;
    }

    if is_vertex {
        let fixup = "    if (y_negate) out.gl_Position.y = -out.gl_Position.y;\n    if (depth_gl) out.gl_Position.z = (out.gl_Position.z + out.gl_Position.w) * 0.5;\n";
        msl = msl.replace("    return out;", &format!("{fixup}    return out;"));
        // spirv-cross initializes gl_PointSize to 1.0; a shader that writes it overrides this later in the body.
        msl = msl.replace("out.gl_PointSize = 1.0;", "out.gl_PointSize = point_size;");
    }

    (
        finish(&msl),
        attrs.iter().map(|(_, l)| *l).collect(),
        colors.iter().map(|(_, l)| *l).collect(),
        varyings,
    )
}

pub fn finish(msl: &str) -> String {
    bound_cbuf_reads(&demote_doubles(&move_extra_samplers(msl)))
}

/// Driver cbuf (guest cbuf 0, Metal buffer 17): the engine writes each stage's constant buffer sizes there, in float4
/// units, one u32 per Metal binding 0..17 from this vec4 index on (must match CBUF_LIMITS_OFFSET in gfx/translated.rs).
pub const CBUF_LIMITS_VEC4: usize = 0xF0;

fn entry_params_re() -> &'static Regex {
    re!(r"(?m)^((?:fragment|vertex|kernel) [^\n]*?\()(.*)\)\s*$")
}

/// Constant buffer reads with a computed index return zero beyond the buffer's bound size, as the GPU does. Skinning
/// shaders compute palette offsets such as bone * 48 - 16, which go negative (a huge unsigned index) for bone 0; Super
/// Mario Bros. Wonder's tiled lighting reads a light count one tile past its 45056-byte buffer at the screen edge: zero on
/// the GPU, on Metal whatever lies next in memory (the arena's next copy), which made 65536-iteration light loops and hung
/// the GPU. In the entry function the limit is the bound size the engine writes into the driver cbuf (binding 17; added
/// as a parameter when the stage has none, index line `cl`); reads in helper functions keep the 64 KiB bank limit.
/// Literal indices are left alone. --no-cbuf-limits: the bank limit everywhere.
pub fn bound_cbuf_reads(msl: &str) -> String {
    let em = entry_params_re().captures(msl);
    let use_limits = em.is_some() && !get_options().no_cbuf_limits;
    let entry_start = em.as_ref().map_or(msl.len(), |c| c.get(0).unwrap().start());
    // Constant buffer variable -> Metal binding, from the entry signature (the driver cbuf is named after guest slot 0
    // but bound at 17); in first-seen order, later duplicates updating the binding (as a dict did).
    let mut slots: Vec<(String, i64)> = Vec::new();
    let mut c17 = String::from("nx_c17");
    if use_limits {
        let params = em.as_ref().unwrap().get(2).unwrap().as_str();
        for c in re!(r"(\w+_c\d+) \[\[buffer\((\d+)\)\]\]").captures_iter(params) {
            let b: i64 = c[2].parse().unwrap();
            match slots.iter_mut().find(|(n, _)| n == &c[1]) {
                Some(e) => e.1 = b,
                None => slots.push((c[1].to_string(), b)),
            }
        }
        if let Some((n, _)) = slots.iter().find(|(_, b)| *b == 17) {
            c17 = format!("{n}._m0");
        }
    }
    let read = re!(r"\b(\w+_c\d+)\._m0\[");
    let bytes = msl.as_bytes();
    let mut used = false;
    let mut out = String::with_capacity(msl.len() + msl.len() / 8);
    let mut i = 0;
    while let Some(m) = read.captures_at(msl, i) {
        let whole = m.get(0).unwrap();
        let name = &m[1];
        let (start, mut j, mut depth) = (whole.end(), whole.end(), 1i32);
        while j < bytes.len() && depth != 0 {
            match bytes[j] {
                b'[' => depth += 1,
                b']' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        let expr = &msl[start..j - 1];
        out.push_str(&msl[i..whole.start()]);
        let binding = slots
            .iter()
            .find(|(n, _)| n == name)
            .map_or(-1, |(_, b)| *b);
        let lit = expr.trim();
        if !lit.is_empty() && lit.bytes().all(|b| b.is_ascii_digit()) {
            out.push_str(&msl[whole.start()..j]);
        } else if use_limits && whole.start() > entry_start && (0..17).contains(&binding) {
            let limit = format!(
                "as_type<uint>({c17}[{}][{}])",
                CBUF_LIMITS_VEC4 + binding as usize / 4,
                binding % 4
            );
            out.push_str(&format!(
                "(uint({expr}) < {limit} ? {name}._m0[{expr}] : float4(0.0))"
            ));
            used = true;
        } else {
            out.push_str(&format!(
                "(uint({expr}) < 4096u ? {name}._m0[{expr}] : float4(0.0))"
            ));
        }
        i = j;
    }
    out.push_str(&msl[i..]);
    let mut msl = out;
    if used {
        msl = format!("// nx: cbuf limits\n{msl}");
    }
    if used && c17 == "nx_c17" {
        let em = entry_params_re().captures(&msl).unwrap();
        let g = em.get(2).unwrap();
        let params = g.as_str();
        let sep = if params.trim().is_empty() { "" } else { ", " };
        msl = format!(
            "{}{}{}constant float4* nx_c17 [[buffer(17)]]{}",
            &msl[..g.start()],
            params,
            sep,
            &msl[g.end()..]
        );
    }
    msl
}

/// The stage reads constant buffer sizes from the driver cbuf (the engine binds it even if the stage declares none).
pub fn uses_cbuf_limits(msl: &str) -> bool {
    msl.contains("// nx: cbuf limits")
}

/// A fragment shader that can discard writes its depth explicitly (gl_FragCoord.z, [[depth(greater)]]). On Apple GPUs
/// the depth a discarding shader leaves in the depth buffer is not always bit-identical to the depth a later draw of the
/// same triangle is tested with; on Maxwell both are the rasterized depth. TotK draws foliage in an alpha-tested depth
/// pre-pass and then a colour pass with depth EQUAL, which failed at scattered pixels: rows of dark dots in every tree
/// canopy (2026-10-10). depth(greater) is a true promise (the value is equal) and keeps early rejection for LESS tests.
/// Off without a depth target (constant 43). --no-discard-depth disables.
pub fn discard_depth(msl: &str) -> String {
    if get_options().no_discard_depth
        || !msl.contains("discard_fragment()")
        || msl.contains("[[depth(")
    {
        return msl.to_string();
    }
    let sig = re!(r"(?m)^fragment (\S+)_out (\S+)\(");
    let Some(em) = sig.captures(msl) else {
        return msl.to_string();
    };
    let entry = em[2].to_string();
    let Some(st) = msl.find(&format!("struct {entry}_out\n{{")) else {
        return msl.to_string();
    };
    let end = st + msl[st..].find("};").expect("unterminated output struct");
    let mut msl = format!(
        "{}    float discard_depth_out [[depth(greater), function_constant(discard_depth_on)]];\n{}",
        &msl[..end], &msl[end..]
    );
    let em_start = sig.find(&msl).unwrap().start();
    let mut sig_end = em_start + msl[em_start..].find(")\n").expect("entry signature");
    if !msl[em_start..sig_end].contains("[[position]]") {
        msl = format!(
            "{}, float4 gl_FragCoord [[position]]{}",
            &msl[..sig_end],
            &msl[sig_end..]
        );
        sig_end += ", float4 gl_FragCoord [[position]]".len();
    }
    let body_start = sig_end + msl[sig_end..].find('{').expect("entry body");
    let body = msl[body_start..].replace(
        "return out;",
        "if (discard_depth_on) out.discard_depth_out = gl_FragCoord.z;\n    return out;",
    );
    let msl = format!("{}{}", &msl[..body_start], body);
    after_using(
        &msl,
        "\n\nconstant bool discard_depth_fc [[function_constant(43)]];\n\
         constant bool discard_depth_on = is_function_constant_defined(discard_depth_fc) ? discard_depth_fc : true;\n",
    )
}

/// Fragment stage: depth output and varying inputs conditional on function constants (see the module doc).
pub fn fragment_optional_inputs(msl: &str) -> String {
    let mut msl = msl.to_string();
    let mut consts: Vec<String> = Vec::new();
    if depth_re().is_match(&msl) {
        consts.push("constant bool has_depth_target_fc [[function_constant(43)]];".into());
        consts.push("constant bool has_depth_target = is_function_constant_defined(has_depth_target_fc) ? has_depth_target_fc : true;".into());
        msl = depth_re()
            .replace_all(&msl, "${1}, function_constant(has_depth_target)]];")
            .into_owned();
        msl = re!(r"(?m)^(\s+)(out\.gl_FragDepth\b[^;]*;)")
            .replace_all(&msl, "${1}if (has_depth_target) ${2}")
            .into_owned();
    }
    let inputs: Vec<(String, String, u32)> = varying_re()
        .captures_iter(&msl)
        .map(|c| (c[2].to_string(), c[3].to_string(), c[4].parse().unwrap()))
        .collect();
    for (_, _, loc) in &inputs {
        consts.push(format!(
            "constant bool in{loc}_fc [[function_constant({})]];",
            48 + loc
        ));
        consts.push(format!("constant bool in{loc}_present = is_function_constant_defined(in{loc}_fc) ? in{loc}_fc : true;"));
    }
    if !inputs.is_empty() {
        msl = varying_re()
            .replace_all(&msl, |c: &Captures| {
                format!(
                    "{}{} {} [[user(locn{}){}, function_constant(in{}_present)]];",
                    &c[1], &c[2], &c[3], &c[4], &c[5], &c[4]
                )
            })
            .into_owned();
        let entry_end = re!(r"(?ms)^fragment .*?\)\s*\n\{\n")
            .find(&msl)
            .expect("fragment entry point")
            .end();
        let (head, mut body) = (msl[..entry_end].to_string(), msl[entry_end..].to_string());
        for (ty, name, loc) in &inputs {
            body = replace_word(
                &body,
                &format!(r"\bin\.{}\b", regex::escape(name)),
                &format!("(in{loc}_present ? in.{name} : {ty}(0))"),
            );
        }
        msl = head + &body;
    }
    if !consts.is_empty() {
        msl = after_using(&msl, &format!("\n\n{}\n", consts.join("\n")));
    }
    msl
}

fn entry_line_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| Regex::new(r"(?m)^(?:fragment|vertex|kernel) .*$").unwrap())
}

pub fn cbuf_extents(msl: &str) -> std::collections::BTreeMap<u32, usize> {
    static PARAM: OnceLock<Regex> = OnceLock::new();
    let param = PARAM.get_or_init(|| Regex::new(r"constant _\w+& (\w+) \[\[buffer\((\d+)\)\]\]").unwrap());
    let mut out = std::collections::BTreeMap::new();
    let Some(m) = entry_line_re().find(msl) else { return out };
    let body = &msl[m.end()..];
    for c in param.captures_iter(m.as_str()) {
        let use_re = Regex::new(&format!(r"\b{}\b", regex::escape(&c[1]))).unwrap();
        let mut top = Some(0usize);
        for u in use_re.find_iter(body) {
            let rest = &body[u.end()..];
            let lit = rest.strip_prefix("._m0[").and_then(|r| {
                let n = r.bytes().take_while(|b| b.is_ascii_digit()).count();
                let after = r[n..].strip_prefix(']')?;
                let next_ok = after.bytes().next().is_none_or(|b| !(b.is_ascii_alphanumeric() || b == b'_' || b == b'['));
                (n > 0 && next_ok).then(|| r[..n].parse::<usize>().ok()).flatten()
            });
            match lit {
                Some(i) => top = top.map(|t| t.max(i + 1)),
                None => {
                    top = None;
                    break;
                }
            }
        }
        if let Some(t) = top {
            out.insert(c[2].parse().unwrap(), t * 16);
        }
    }
    out
}

