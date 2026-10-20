// crates/cli/src/batch_kernels.rs
//! Batched variants of translated stages: TotK draws many runs of
//! consecutive draws that differ only in their uniform buffers (413 occlusion-style boxes per Hateno frame, each with its
//! own 256-byte block; terrain tiles; shadow casters). The engine draws such a run as one instanced draw of the stage's
//! `tb_<entry>`, which is the entry point with every constant-buffer reference read from a per-instance table of GPU
//! addresses instead of a buffer binding:
//!
//!   buffer(B) `constant ulong* tk_cbt`, B = the first constant buffer's own binding (free in the variant);
//!   tk_cbt[0] = the original draws' base instance (the variant's own instance numbers count the run);
//!   tk_cbt[1 + i * N + k] = GPU address of constant buffer k (index line `tb`: the bindings in table order) for the
//!   run's i-th draw.
//!
//! The vertex variant writes the run index as a flat varying (`user(tkiid)`) that the fragment variant reads. The
//! stage's code is otherwise unchanged, so each instance computes what its original draw computed (positions stay
//! [[invariant]]). Metal keeps primitive order across the instances of a draw, so blending and depth see the same
//! sequence as the separate draws.

use crate::tiny_kernels::{entry, parse_params, struct_body, Param};

/// Start of the appended variant text (cbuf extents and other analyses of the stage's own code stop here).
pub const MARKER: &str = "\n// ---- tb: batched variant ----";

fn cbuf_param(p: &Param) -> Option<(String, String, u32)> {
    let decl = p.decl.strip_prefix("constant ")?;
    let (ty, name) = decl.split_once("& ")?;
    if ty.contains(' ') || name.contains(' ') {
        return None;
    }
    let b = p.attr.strip_prefix("[[buffer(")?.strip_suffix(")]]")?.parse().ok()?;
    Some((ty.to_string(), name.to_string(), b))
}

/// The variant of `msl`'s entry point (`kind`: "vertex" or "fragment") and its table's bindings, or None when the stage
/// reads no constant buffer (nothing can differ per instance) or has a shape this rewrite does not handle.
pub fn batch_variant(msl: &str, kind: &str) -> Option<(String, Vec<u32>)> {
    let (ret, name, params, b0, b1) = entry(msl, kind)?;
    let body = &msl[b0..b1];
    let ps = parse_params(&params);
    let cbs: Vec<(String, String, u32)> = ps.iter().filter_map(cbuf_param).collect();
    if cbs.is_empty() {
        return None;
    }
    let n = cbs.len();
    let table_binding = cbs[0].2;
    let mut new_params: Vec<String> = Vec::new();
    let mut prefix = String::new();
    let mut structs = String::new();
    let iid_expr;
    if kind == "vertex" {
        if ps.iter().any(|p| p.attr.contains("[[stage_in]]") && !p.decl.ends_with(" in")) {
            return None;
        }
        for p in &ps {
            if cbuf_param(p).is_some() {
                continue;
            }
            if p.attr == "[[instance_id]]" || p.attr == "[[base_instance]]" {
                // The original draws each had one instance: both read their base instance.
                prefix.push_str(&format!("    uint {} = uint(tk_cbt[0]);\n", p.name));
                continue;
            }
            new_params.push(format!("{} {}", p.decl, p.attr).trim().to_string());
        }
        new_params.push(format!("constant ulong* tk_cbt [[buffer({})]]", table_binding));
        new_params.push("uint tk_iid [[instance_id]]".into());
        iid_expr = "tk_iid".to_string();
        // Output struct with the run index.
        let sb = struct_body(msl, &ret)?;
        structs.push_str(&format!("struct {}_tb\n{{{}    uint tk_iid [[user(tkiid)]];\n}};\n\n", ret, sb));
    } else if kind == "fragment" {
        let mut in_name = None;
        for p in &ps {
            if cbuf_param(p).is_some() {
                continue;
            }
            if p.attr == "[[stage_in]]" {
                let ty = p.decl.split_whitespace().next()?;
                let sb = struct_body(msl, ty)?;
                structs.push_str(&format!("struct {}_tb\n{{{}    uint tk_iid [[user(tkiid), flat]];\n}};\n\n", ty, sb));
                new_params.push(format!("{}_tb {} [[stage_in]]", ty, p.name));
                in_name = Some(p.name.clone());
                continue;
            }
            new_params.push(format!("{} {}", p.decl, p.attr).trim().to_string());
        }
        if in_name.is_none() {
            structs.push_str(&format!("struct tb_{}_in\n{{\n    uint tk_iid [[user(tkiid), flat]];\n}};\n\n", name));
            new_params.insert(0, format!("tb_{}_in tk_tbin [[stage_in]]", name));
            in_name = Some("tk_tbin".into());
        }
        new_params.push(format!("constant ulong* tk_cbt [[buffer({})]]", table_binding));
        iid_expr = format!("{}.tk_iid", in_name?);
    } else {
        return None;
    }
    for (k, (ty, nm, _)) in cbs.iter().enumerate() {
        prefix.push_str(&format!("    constant {ty}& {nm} = *(constant {ty}*)(tk_cbt[1 + {iid_expr} * {n} + {k}]);\n"));
    }
    // Body: the run index goes into the output struct right after it is declared.
    let mut body = body.to_string();
    if kind == "vertex" {
        let decl = format!("    {} out = {{}};\n", ret);
        if body.matches(&decl).count() != 1 || body.matches(ret.as_str()).count() != 1 {
            return None;
        }
        body = body.replacen(&decl, &format!("    {}_tb out = {{}};\n    out.tk_iid = tk_iid;\n", ret), 1);
    }
    let open = body.find('{')?;
    body.insert_str(open + 1, &format!("\n{}", prefix.trim_end_matches('\n')));
    let ret_tb = if kind == "vertex" { format!("{}_tb", ret) } else { ret.clone() };
    let text = format!("{}\n{}{} {} tb_{}({})\n{}\n", MARKER, structs, kind, ret_tb, name, new_params.join(", "), body);
    Some((text, cbs.iter().map(|c| c.2).collect()))
}
