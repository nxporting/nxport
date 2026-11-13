// Same-pass feedback read from the tile (MSL side).
//
// TotK's G-buffer decals sample targets of the pass they draw into, at their own pixel, after a barrier that makes the
// pass's earlier rendering visible to texture reads (translated.rs feedback_snapshots). Metal keeps a pass's writes in
// tile memory, so the engine ends the pass before such a draw (13 times a frame in Hateno, far more in shrines; each
// end stores every attachment and the next pass loads them again, which costs most on the phone's GPU). Apple GPUs can
// read the pixel's own attachment value in the fragment shader instead ([[color(k)]], programmable blending): it holds
// every earlier fragment of the pass in order, which is what NVN guarantees such a draw after a barrier.
//
// That is only the same value when the sample is at the pixel's own position. This module proves it on the MSL:
// - the vertex stage writes a varying as the clip -> screen mapping of its position: loc.x = (w + x) / 2,
//   loc.y = (w - y) / 2, loc.w = w (before the y flip the engine adds for y_negate), its last assignments at the top
//   level of the entry function (`screen_varyings`);
// - the fragment stage samples the texture only at that varying's xy / w (`rewrite`), which then reads the tile.
// The engine checks the rest per draw (viewport over the whole target, no y flip, the sampled view is the attachment's
// own level, layer, format and swizzle).
//
// The same idea in nxnative (dm/gfx/tools/dmsh_fetch.py, its rendering.md "Same-pass feedback read from the tile").

use std::collections::HashMap;

/// The body of the entry function `entry` of kind `kind` ("vertex" / "fragment"): (signature start, body start, body end)
/// as byte offsets: the signature runs from `kind` to the `{`, the body from after `{` to its closing `}` at line start.
fn entry_span(msl: &str, kind: &str, entry: &str) -> Option<(usize, usize, usize)> {
    let mut at = 0;
    while let Some(i) = msl[at..].find(kind) {
        let start = at + i;
        at = start + kind.len();
        if start > 0 && msl.as_bytes()[start - 1] != b'\n' {
            continue;
        }
        let line_end = msl[start..].find('\n').map_or(msl.len(), |e| start + e);
        let line = &msl[start..line_end];
        if !line.contains(&format!(" {entry}(")) {
            continue;
        }
        let open = start + msl[start..].find("\n{")? + 2;
        let close = open + msl[open..].find("\n}")?;
        return Some((start, open, close));
    }
    None
}

/// `(inN_present ? in.locN : float4(0))` -> `in.locN` (msl_specialize's optional fragment inputs).
fn plain_inputs(e: &str) -> String {
    let re = regex::Regex::new(r"\(in(\d+)_present \? in\.loc(\d+) : float4\(0\)\)").unwrap();
    re.replace_all(e, |c: &regex::Captures| {
        if c[1] == c[2] {
            format!("in.loc{}", &c[2])
        } else {
            c[0].to_string()
        }
    })
    .into_owned()
}

/// Top-level `float tN = EXPR;` definitions of a body whose variable is assigned nowhere else (so it is a value).
fn single_defs(body: &str) -> HashMap<String, String> {
    let def = regex::Regex::new(r"^    (?:float|float2|float4|int|uint) (t\d+) = (.+);$").unwrap();
    let mut defs = HashMap::new();
    for line in body.lines() {
        if let Some(c) = def.captures(line) {
            defs.insert(c[1].to_string(), c[2].to_string());
        }
    }
    defs.retain(|v, _| {
        let assign = regex::Regex::new(&format!(r"\b{v}(\.[xyzw]+)? *(=[^=]|\+=|-=|\*=|/=)")).unwrap();
        assign.find_iter(body).count() == 1
    });
    defs
}

/// `e` with single-assignment variables replaced by their definitions (repeatedly), inputs made plain, spaces removed.
fn resolve(e: &str, defs: &HashMap<String, String>) -> String {
    let var = regex::Regex::new(r"\bt\d+\b").unwrap();
    let mut e = plain_inputs(e);
    for _ in 0..8 {
        let next = var
            .replace_all(&e, |c: &regex::Captures| match defs.get(&c[0]) {
                Some(d) => format!("({})", plain_inputs(d)),
                None => c[0].to_string(),
            })
            .into_owned();
        if next == e {
            break;
        }
        e = next;
    }
    e.replace(' ', "")
}

/// `e` without redundant parentheses, also inside a leading minus: `-(x)` -> `-x`.
fn signed(e: &str) -> String {
    let e = unparen(e);
    match e.strip_prefix('-') {
        Some(rest) => format!("-{}", unparen(rest)),
        None => e.to_string(),
    }
}

/// Strips redundant outer parentheses.
fn unparen(mut e: &str) -> &str {
    while e.starts_with('(') && e.ends_with(')') && balanced(&e[1..e.len() - 1]) {
        e = &e[1..e.len() - 1];
    }
    e
}

fn balanced(e: &str) -> bool {
    let mut d = 0i32;
    for c in e.chars() {
        d += match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        if d < 0 {
            return false;
        }
    }
    d == 0
}

/// Splits `f(a,b)` (no spaces) into its two arguments at the top-level comma.
fn two_args<'a>(e: &'a str, f: &str) -> Option<(&'a str, &'a str)> {
    let inner = e.strip_prefix(f)?.strip_prefix('(')?.strip_suffix(')')?;
    let mut d = 0i32;
    for (i, c) in inner.char_indices() {
        match c {
            '(' => d += 1,
            ')' => d -= 1,
            ',' if d == 0 => return Some((&inner[..i], &inner[i + 1..])),
            _ => {}
        }
    }
    None
}

/// The location N if `e` is in.locN.<c> / in.locN.w (as a product with a reciprocal, or a division).
fn projective(e: &str, c: char) -> Option<u32> {
    let e = unparen(e);
    let comp = |x: &str| -> Option<u32> {
        let x = unparen(x);
        x.strip_prefix("in.loc")?.strip_suffix(&format!(".{c}"))?.parse().ok()
    };
    let recip = |x: &str, n: u32| unparen(x) == format!("1.0/in.loc{n}.w");
    let pair = two_args(e, "spvFMul").or_else(|| {
        let i = top_level(e, '*')?;
        Some((&e[..i], &e[i + 1..]))
    });
    if let Some((a, b)) = pair {
        if let Some(n) = comp(a).filter(|&n| recip(b, n)) {
            return Some(n);
        }
        if let Some(n) = comp(b).filter(|&n| recip(a, n)) {
            return Some(n);
        }
    }
    let i = top_level(e, '/')?;
    comp(&e[..i]).filter(|&n| unparen(&e[i + 1..]) == format!("in.loc{n}.w"))
}

/// The byte index of the single top-level `op` in `e`.
fn top_level(e: &str, op: char) -> Option<usize> {
    let mut d = 0i32;
    let mut found = None;
    for (i, c) in e.char_indices() {
        match c {
            '(' => d += 1,
            ')' => d -= 1,
            _ if c == op && d == 0 => {
                if found.is_some() {
                    return None;
                }
                found = Some(i);
            }
            _ => {}
        }
    }
    found
}

/// Locations the vertex stage `entry` writes as the screen mapping of its position (bit N = location N).
pub fn screen_varyings(msl: &str, entry: &str) -> u32 {
    let Some((_, open, close)) = entry_span(msl, "vertex", entry) else { return 0 };
    let body = &msl[open..close];
    let defs = single_defs(body);
    // The last top-level assignment of each output component; none may be assigned inside a nested block.
    let top = regex::Regex::new(r"^    out\.(gl_Position|loc\d+)\.([xyzw]) = (.+);$").unwrap();
    let nested = regex::Regex::new(r"^     +out\.(gl_Position|loc\d+)(\.[xyzw])? =").unwrap();
    let whole = regex::Regex::new(r"out\.(gl_Position|loc\d+) =").unwrap();
    let mut last: HashMap<(String, String), String> = HashMap::new();
    let mut nested_set = std::collections::HashSet::new();
    for line in body.lines() {
        if let Some(c) = top.captures(line) {
            last.insert((c[1].to_string(), c[2].to_string()), c[3].to_string());
        } else if let Some(c) = nested.captures(line) {
            nested_set.insert(c[1].to_string());
        }
        if let Some(c) = whole.captures(line) {
            nested_set.insert(c[1].to_string());
        }
    }
    if nested_set.contains("gl_Position") {
        return 0;
    }
    let pos = |c: &str| last.get(&("gl_Position".to_string(), c.to_string())).map(|e| resolve(e, &defs));
    let (Some(x), Some(y), Some(w)) = (pos("x"), pos("y"), pos("w")) else { return 0 };
    let (x, y, w) = (unparen(&x).to_string(), unparen(&y).to_string(), unparen(&w).to_string());
    let half = |e: &str| -> Option<String> {
        let e = unparen(e);
        two_args(e, "spvFMul")
            .and_then(|(a, b)| (unparen(b) == "0.5").then(|| a.to_string()).or_else(|| (unparen(a) == "0.5").then(|| b.to_string())))
            .or_else(|| {
                let i = top_level(e, '*')?;
                (unparen(&e[i + 1..]) == "0.5").then(|| e[..i].to_string())
            })
    };
    let sum = |e: &str, a: &str, b: &str| -> bool {
        let e = unparen(e);
        let terms = two_args(e, "spvFAdd").or_else(|| {
            let i = top_level(e, '+')?;
            Some((&e[..i], &e[i + 1..]))
        });
        terms.is_some_and(|(p, q)| {
            let (p, q, a, b) = (signed(p), signed(q), signed(a), signed(b));
            (p == a && q == b) || (p == b && q == a)
        })
    };
    let diff = |e: &str, a: &str, b: &str| -> bool {
        // a - b, also as a + (-b)
        let e = unparen(e);
        sum(e, a, &format!("-{b}"))
            || two_args(e, "spvFSub").is_some_and(|(p, q)| unparen(p) == a && unparen(q) == b)
            || top_level(e, '-').is_some_and(|i| i > 0 && unparen(&e[..i]) == a && unparen(&e[i + 1..]) == b)
    };
    let mut mask = 0;
    for ((reg, c), _) in last.iter().filter(|((r, c), _)| r.starts_with("loc") && c == "w") {
        let n: u32 = match reg[3..].parse() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let _ = c;
        if nested_set.contains(reg.as_str()) || n >= 32 {
            continue;
        }
        let comp = |c: &str| last.get(&(reg.clone(), c.to_string())).map(|e| resolve(e, &defs));
        let (Some(lx), Some(ly), Some(lw)) = (comp("x"), comp("y"), comp("w")) else { continue };
        let ok_w = unparen(&lw) == w;
        let ok_x = half(&lx).is_some_and(|s| sum(&s, &w, &x));
        let ok_y = half(&ly).is_some_and(|s| diff(&s, &w, &y));
        if ok_w && ok_x && ok_y {
            mask |= 1 << n;
        }
    }
    mask
}

/// The fragment stage `entry` with its samples of the textures at `map`'s Metal texture bindings read from the colour
/// attachments `map` gives them instead (`float4 nx_fb<k> [[color(k)]]`), if every sample of each is at the position
/// varying.xy / varying.w; and the varying locations those samples use (each must be a screen varying of the paired
/// vertex stage, see screen_varyings). None if any use of such a texture is anything else.
pub fn rewrite(msl: &str, entry: &str, map: &[(u32, u32)]) -> Option<(String, u32)> {
    let (sig, open, close) = entry_span(msl, "fragment", entry)?;
    let signature = &msl[sig..open];
    let body = &msl[open..close];
    let defs = single_defs(body);
    let mut new_body = body.to_string();
    let mut locs = 0u32;
    let mut params = String::new();
    for &(binding, k) in map {
        let decl = regex::Regex::new(&format!(r"texture2d<float> (\w+) \[\[texture\({binding}\)\]\]")).unwrap();
        let name = decl.captures(signature)?[1].to_string();
        let sample =
            regex::Regex::new(&format!(r"{name}\.sample\({name}Smplr, float2\(")).unwrap();
        let mut out = String::new();
        let mut rest = new_body.as_str();
        let mut uses = 0;
        while let Some(m) = sample.find(rest) {
            // float2( ... ) and what follows up to the call's closing parenthesis.
            let args_start = m.end();
            let after = &rest[args_start..];
            let mut d = 1i32;
            let mut end = None;
            for (i, c) in after.char_indices() {
                match c {
                    '(' => d += 1,
                    ')' => {
                        d -= 1;
                        if d == 0 {
                            end = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let end = end?;
            let coord = &after[..end];
            let tail = &after[end + 1..];
            // The call closes right after float2(...), or after a level(0) argument.
            let close_len = if tail.starts_with(')') {
                1
            } else if let Some(t) = tail.strip_prefix(", level(0)") {
                if !t.starts_with(')') {
                    return None;
                }
                ", level(0))".len()
            } else {
                return None;
            };
            let (cx, cy) = {
                let mut d = 0i32;
                let mut split = None;
                for (i, c) in coord.char_indices() {
                    match c {
                        '(' => d += 1,
                        ')' => d -= 1,
                        ',' if d == 0 => {
                            split = Some(i);
                            break;
                        }
                        _ => {}
                    }
                }
                let i = split?;
                (&coord[..i], &coord[i + 1..])
            };
            let (nx, ny) = (projective(&resolve(cx, &defs), 'x')?, projective(&resolve(cy, &defs), 'y')?);
            if nx != ny {
                return None;
            }
            locs |= 1 << nx;
            out.push_str(&rest[..m.start()]);
            out.push_str(&format!("nx_fb{k}"));
            rest = &after[end + 1 + close_len..];
            uses += 1;
        }
        out.push_str(rest);
        // Any other use of the texture (a read, another sample form) keeps the original: no rewrite.
        if uses == 0 || regex::Regex::new(&format!(r"\b{name}\b")).unwrap().is_match(&out) {
            return None;
        }
        new_body = out;
        params.push_str(&format!(", float4 nx_fb{k} [[color({k})]]"));
    }
    let sig_close = signature.rfind(')')?;
    let new_sig = format!("{}{}{}", &signature[..sig_close], params, &signature[sig_close..]);
    Some((format!("{}{}{}{}", &msl[..sig], new_sig, new_body, &msl[close..]), locs))
}

// ---- engine side ----

use super::shader_cache::CachedStage;
use super::Tex;
use objc2_metal::{MTLPixelFormat, MTLTexture, MTLTextureSwizzle, MTLTextureType};
use std::sync::{Mutex, OnceLock};

pub(super) fn enabled() -> bool {
    static E: OnceLock<bool> = OnceLock::new();
    *E.get_or_init(|| crate::settings::get().graphics.tile_feedback)
}

/// Whether this draw may read the tile: --tile-feedback, or replay side B with --replay-ab-tile.
pub(super) fn active() -> bool {
    static AB: OnceLock<bool> = OnceLock::new();
    enabled() || (*AB.get_or_init(|| crate::settings::get().replay.replay_ab_tile) && super::replay_b())
}

/// A variant's state: compiling on a thread, ready, or not possible for this pair and map.
enum Slot {
    Pending,
    Ready(&'static CachedStage),
    Failed,
}

/// Variants by (vertex stage, fragment stage, binding -> attachment map).
type Key = (usize, usize, Vec<(u32, u32)>);

fn variants() -> &'static Mutex<HashMap<Key, Slot>> {
    static V: OnceLock<Mutex<HashMap<Key, Slot>>> = OnceLock::new();
    V.get_or_init(Default::default)
}

/// Variants made, draws that read the tile, draws whose variant was not ready or not possible (--log-timeline).
pub(super) static STATS: [std::sync::atomic::AtomicU64; 3] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 3];

/// The fragment stage that reads `map`'s textures from the tile for this vertex / fragment pair, once made; the first
/// call translates both stages (on this thread: their guest code may be released later) and compiles the variant on
/// another. None until it is ready, or when the proof fails.
pub(super) unsafe fn variant(
    vs: &CachedStage,
    vs_code: (u64, u64),
    fs: &'static CachedStage,
    fs_code: (u64, u64),
    map: &[(u32, u32)],
) -> Option<&'static CachedStage> {
    let key = (vs as *const _ as usize, fs as *const _ as usize, map.to_vec());
    let mut v = variants().lock().unwrap();
    match v.get(&key) {
        Some(Slot::Ready(s)) => return Some(s),
        Some(_) => return None,
        None => {}
    }
    v.insert(key.clone(), Slot::Pending);
    drop(v);
    let made = (|| {
        let vt = super::shader_cache::translate_runtime(vs_code.0, vs_code.1, 0)?;
        let ft = super::shader_cache::translate_runtime(fs_code.0, fs_code.1, 1)?;
        // The runtime translation must be the stage the cache holds.
        if vt.entry != vs.entry || ft.entry != fs.entry {
            return None;
        }
        let screen = screen_varyings(&vt.specialized, &vt.entry);
        let (msl, locs) = rewrite(&ft.specialized, &ft.entry, map)?;
        if locs & !screen != 0 {
            return None;
        }
        let tag = map.iter().fold(0u64, |h, &(b, k)| h.wrapping_mul(31).wrapping_add((b as u64) << 8 | k as u64));
        let entry = format!("{}_fb{tag:x}", fs.entry);
        let msl = msl.replacen(&format!(" {}(", fs.entry), &format!(" {entry}("), 1);
        Some((msl, entry))
    })();
    let Some((msl, entry)) = made else {
        variants().lock().unwrap().insert(key, Slot::Failed);
        STATS[2].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        return None;
    };
    std::thread::Builder::new()
        .name("tile-variant".into())
        .spawn(move || {
            let slot = match super::shader_cache::compile_variant(&msl, &entry) {
                Some(lib) => {
                    super::translated::register_dynamic_library(&entry, lib);
                    let mut st = fs.clone();
                    st.entry = entry;
                    // The compute and argument-buffer versions belong to the original entry.
                    st.tiny_kernel = false;
                    st.tiny_clears = false;
                    st.batch_cbufs = None;
                    st.args = None;
                    STATS[0].fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    Slot::Ready(Box::leak(Box::new(st)))
                }
                None => Slot::Failed,
            };
            variants().lock().unwrap().insert(key, slot);
        })
        .ok();
    None
}

/// Channels a colour format has (the others read 0, 0, 1 from both a sample and the tile).
fn channels(pf: MTLPixelFormat) -> Option<usize> {
    use MTLPixelFormat as F;
    Some(match pf {
        F::R8Unorm | F::R8Snorm | F::R16Unorm | F::R16Snorm | F::R16Float | F::R32Float => 1,
        F::RG8Unorm | F::RG8Snorm | F::RG16Unorm | F::RG16Snorm | F::RG16Float | F::RG32Float => 2,
        F::RG11B10Float | F::RGB9E5Float => 3,
        F::RGBA8Unorm | F::RGBA8Unorm_sRGB | F::RGBA8Snorm | F::BGRA8Unorm | F::BGRA8Unorm_sRGB | F::RGB10A2Unorm
        | F::RGBA16Unorm | F::RGBA16Snorm | F::RGBA16Float | F::RGBA32Float => 4,
        _ => return None,
    })
}

/// Whether `t`'s swizzle reads what the tile gives: each channel itself, or the default for one the format lacks.
fn plain_swizzle(t: &Tex, n: usize) -> bool {
    let s = t.swizzle();
    let want = |i: usize, c: MTLTextureSwizzle| {
        let own = [MTLTextureSwizzle::Red, MTLTextureSwizzle::Green, MTLTextureSwizzle::Blue, MTLTextureSwizzle::Alpha][i];
        c == own || (i >= n && c == if i == 3 { MTLTextureSwizzle::One } else { MTLTextureSwizzle::Zero })
    };
    want(0, s.red) && want(1, s.green) && want(2, s.blue) && want(3, s.alpha)
}

/// The (subresource of its root) a texture names: its level and slice relative to its parent, for a 2D single-level,
/// single-sample view.
fn single_2d(t: &Tex) -> Option<(usize, usize)> {
    (t.textureType() == MTLTextureType::Type2D && t.mipmapLevelCount() == 1 && t.sampleCount() == 1)
        .then(|| unsafe { (t.parentRelativeLevel(), t.parentRelativeSlice()) })
}

/// Whether the bound texture `t` is exactly colour attachment `a` (at `level` / `layer` of it), read the same way.
pub(super) fn same_pixels(t: &Tex, a: &Tex, level: usize, layer: usize) -> bool {
    if t.pixelFormat() != a.pixelFormat() || a.sampleCount() != 1 {
        return false;
    }
    let Some(n) = channels(t.pixelFormat()) else { return false };
    if !plain_swizzle(t, n) {
        return false;
    }
    let Some(sub) = single_2d(t) else { return false };
    let att = unsafe { (a.parentRelativeLevel() + level, a.parentRelativeSlice() + layer) };
    // Same root (the caller checked) and the same level and slice of it.
    sub == att && t.width() == a.width() >> level.min(31) && t.height() == a.height() >> level.min(31)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Cut down from TotK 1.1.2's decal pair s_289733a05368a463 / s_89183acd4f64b6c3.
    const VS: &str = "vertex s_v_out s_v(s_v_in in [[stage_in]])
{
    s_v_out out = {};
    out.gl_Position.x = 0.0;
    out.gl_Position.y = 0.0;
    out.gl_Position.z = 0.0;
    out.gl_Position.w = 1.0;
    float t25 = in.a.w;
    float t26 = in.a.x;
    float t27 = in.a.y;
    out.gl_Position.z = 0.5;
    out.gl_Position.x = t26;
    out.gl_Position.y = t27;
    out.gl_Position.w = t25;
    out.loc2.w = t25;
    out.loc2.x = spvFMul(spvFAdd(t25, t26), 0.5);
    out.loc2.y = spvFMul(spvFAdd(t25, -t27), 0.5);
    out.loc3.w = t25;
    out.loc3.x = spvFMul(spvFAdd(t25, t26), 0.5);
    out.loc3.y = spvFMul(spvFAdd(t25, t27), 0.5);
    if (y_negate) out.gl_Position.y = -out.gl_Position.y;
    return out;
}
";

    const FS: &str = "fragment s_f_out s_f(s_f_in in [[stage_in]], texture2d<float> fp_t_tcb_2A [[texture(0)]], texture2d<float> fp_t_tcb_10 [[texture(3)]], sampler fp_t_tcb_2ASmplr [[sampler(0)]], sampler fp_t_tcb_10Smplr [[sampler(3)]], float4 gl_FragCoord [[position]])
{
    s_f_out out = {};
    float2 t2 = fp_t_tcb_2A.sample(fp_t_tcb_2ASmplr, float2((in3_present ? in.loc3 : float4(0)).x, (in3_present ? in.loc3 : float4(0)).y)).xy;
    float t9 = 1.0 / (in2_present ? in.loc2 : float4(0)).w;
    if (0.0 <= fp_t_tcb_10.sample(fp_t_tcb_10Smplr, float2(spvFMul((in2_present ? in.loc2 : float4(0)).x, t9), spvFMul((in2_present ? in.loc2 : float4(0)).y, t9))).x)
    {
        out.color7_f = float4(t2, 0.0, 1.0);
    }
    return out;
}
static inline s_f_out s_f_tkbody(texture2d<float> fp_t_tcb_10, sampler fp_t_tcb_10Smplr)
{
    return fp_t_tcb_10.sample(fp_t_tcb_10Smplr, float2(0.5, 0.5));
}
";

    #[test]
    fn finds_the_screen_varying() {
        // loc2 is the screen mapping; loc3 has y the wrong way round.
        assert_eq!(screen_varyings(VS, "s_v"), 1 << 2);
        // A later assignment inside a block: not proven.
        let nested = VS.replace("    return out;", "    if (t25 > 0.0)\n    {\n        out.loc2.x = 0.0;\n    }\n    return out;");
        assert_eq!(screen_varyings(&nested, "s_v"), 0);
    }

    #[test]
    fn rewrites_a_sample_at_the_screen_varying() {
        let (out, locs) = rewrite(FS, "s_f", &[(3, 7)]).unwrap();
        assert_eq!(locs, 1 << 2);
        assert!(out.contains("float4 gl_FragCoord [[position]], float4 nx_fb7 [[color(7)]])"));
        assert!(out.contains("if (0.0 <= nx_fb7.x)"));
        // Other functions keep their own parameters and samples.
        assert!(out.contains("return fp_t_tcb_10.sample(fp_t_tcb_10Smplr, float2(0.5, 0.5));"));
        // The texture at binding 0 is sampled at loc3 without the division: not the pixel's position.
        assert!(rewrite(FS, "s_f", &[(0, 1)]).is_none());
    }

    #[test]
    fn refuses_other_uses() {
        let read = FS.replace("    return out;\n}\nstatic", "    out.color7_f.y = fp_t_tcb_10.read(uint2(0)).x;\n    return out;\n}\nstatic");
        assert!(rewrite(&read, "s_f", &[(3, 7)]).is_none());
        let reassigned = FS.replace("    if (0.0 <=", "    t9 = 2.0;\n    if (0.0 <=");
        assert!(rewrite(&reassigned, "s_f", &[(3, 7)]).is_none());
        let bias = FS.replace("t9))).x)", "t9)), bias(1.0)).x)");
        assert!(rewrite(&bias, "s_f", &[(3, 7)]).is_none());
    }
}
