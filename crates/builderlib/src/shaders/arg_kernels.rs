//! Argument-buffer variants of translated stages: `ab_<entry>` is the entry point with every resource parameter
//! (constant buffers, storage buffers, textures, samplers) moved into one struct read from buffer(AB_BUFFER), so a draw
//! binds one buffer per stage instead of one slot per resource. Each member is 8 bytes in declaration order (Metal 3
//! argument buffers: a buffer is its GPU address, a texture or sampler its resource ID), which the engine writes
//! directly. The stage's code is unchanged: the parameters become local aliases of the struct's members.
//!
//! Index line `ab <members>`: per member its class and original binding, c<b> constant buffer, b<b> device buffer,
//! t<t> texture, s<s> sampler.

use crate::tiny_kernels::{entry, parse_params, Param};

/// Start of the appended variant text (analyses of the stage's own code stop here).
pub const MARKER: &str = "\n// ---- ab: argument buffer variant ----";

/// The struct's buffer index (free in the variant: every constant / storage buffer binding moved into it).
pub const AB_BUFFER: u32 = 0;

fn binding(attr: &str, kind: &str) -> Option<u32> {
    attr.strip_prefix(&format!("[[{kind}("))?.strip_suffix(")]]")?.parse().ok()
}

/// (member declaration, alias statement, index entry) of a resource parameter, None for anything else.
fn member(p: &Param) -> Option<(String, String, String)> {
    let name = &p.name;
    if let Some(b) = binding(&p.attr, "buffer") {
        let ty = p.decl[..p.decl.len() - name.len()].trim();
        if let Some(t) = ty.strip_suffix('&') {
            // T& -> T* member, reference alias; a device reference is a storage buffer
            let t = t.trim();
            let class = if t.starts_with("device") { 'b' } else { 'c' };
            return Some((format!("{t}* {name};"), format!("{t}& {name} = *ab.{name};"), format!("{class}{b}")));
        }
        if ty.ends_with('*') {
            return Some((format!("{ty} {name};"), format!("{ty} {name} = ab.{name};"), format!("b{b}")));
        }
        return None;
    }
    if let Some(t) = binding(&p.attr, "texture") {
        let ty = p.decl[..p.decl.len() - name.len()].trim();
        return Some((format!("{ty} {name};"), format!("{ty} {name} = ab.{name};"), format!("t{t}")));
    }
    if let Some(s) = binding(&p.attr, "sampler") {
        return Some((format!("sampler {name};"), format!("sampler {name} = ab.{name};"), format!("s{s}")));
    }
    None
}

/// The variant of `msl`'s entry point (`kind`: "vertex" or "fragment") and its index entries, or None when the stage
/// has no resource parameter or a parameter this rewrite does not understand.
pub fn arg_variant(msl: &str, kind: &str) -> Option<(String, Vec<String>)> {
    let (ret, name, params, b0, b1) = entry(msl, kind)?;
    let body = &msl[b0..b1];
    let ps = parse_params(&params);
    let mut fields = String::new();
    let mut aliases = String::new();
    let mut entries = Vec::new();
    let mut kept = Vec::new();
    for p in &ps {
        match member(p) {
            Some((f, a, e)) => {
                fields.push_str(&format!("    {f}\n"));
                aliases.push_str(&format!("    {a}\n"));
                entries.push(e);
            }
            None => {
                // Anything else bound to a resource slot cannot stay: the struct takes buffer(AB_BUFFER).
                if p.attr.starts_with("[[buffer(") || p.attr.starts_with("[[texture(") || p.attr.starts_with("[[sampler(") {
                    return None;
                }
                kept.push(format!("{} {}", p.decl, p.attr).trim().to_string());
            }
        }
    }
    if entries.is_empty() {
        return None;
    }
    let st = format!("ab_{name}_args");
    kept.push(format!("constant {st}& ab [[buffer({AB_BUFFER})]]"));
    let mut body = body.to_string();
    let open = body.find('{')?;
    body.insert_str(open + 1, &format!("\n{}", aliases.trim_end_matches('\n')));
    let text = format!("{MARKER}\nstruct {st}\n{{\n{fields}}};\n\n{kind} {ret} ab_{name}({})\n{body}\n", kept.join(", "));
    Some((text, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_variant() {
        let msl = "\nstruct s_x_out\n{\n    float4 c [[color(0)]];\n};\n\nfragment s_x_out s_x(s_x_in in [[stage_in]], constant _16& fp_c1 [[buffer(1)]], const device uint* fp_s0 [[buffer(18)]], texture2d<float> fp_t [[texture(0)]], sampler fp_tSmplr [[sampler(0)]])\n{\n    s_x_out out = {};\n    out.c = fp_t.sample(fp_tSmplr, float2(fp_c1._m0[0].x));\n    return out;\n}\n";
        let (v, e) = arg_variant(msl, "fragment").unwrap();
        assert_eq!(e, ["c1", "b18", "t0", "s0"]);
        assert!(v.contains("constant _16* fp_c1;"));
        assert!(v.contains("constant _16& fp_c1 = *ab.fp_c1;"));
        assert!(v.contains("const device uint* fp_s0 = ab.fp_s0;"));
        assert!(v.contains("fragment s_x_out ab_s_x(s_x_in in [[stage_in]], constant ab_s_x_args& ab [[buffer(0)]])"));
    }
}
