// NVN pipeline state objects (NVNblendState, NVNchannelMaskState, NVNcolorState,
// NVNdepthStencilState, NVNmultisampleState, NVNpolygonState, NVNvertexAttribState,
// NVNvertexStreamState) and texture handles.
//
// All of these are small opaque POD structs whose sizes are fixed by NVN:
// blend 8, channel mask 4, color 4, depth/stencil 8, multisample 24, polygon 4, vertex attrib 4,
// vertex stream 8 bytes. The game fills thousands of them while building pipelines, so each
// setter stores into a packed bitfield inside that size (the layout is ours; only the size is
// fixed) and each getter returns what was stored, with the defaults documented in NVN.

use std::os::raw::c_int;

#[inline]
fn get(bits: u64, shift: u32, width: u32) -> u64 {
    (bits >> shift) & ((1u64 << width) - 1)
}
// The game places these objects at 4-byte-aligned addresses inside its own structs even where
// NVN declares 8-byte alignment, so every state struct here is `packed` (alignment 1) and the
// bitfield containers are read and written by value, never by reference.
macro_rules! setbits {
    ($s:expr, $shift:expr, $width:expr, $v:expr) => {{
        let mut v = ($s).bits;
        put(&mut v, $shift, $width, $v);
        ($s).bits = v;
    }};
}

#[inline]
fn put(bits: &mut u64, shift: u32, width: u32, v: u64) {
    let m = ((1u64 << width) - 1) << shift;
    *bits = (*bits & !m) | ((v << shift) & m);
}

/// NVNblendFunc values are 1..=0x13 plus the constant-color factors 0x61..=0x64 (CONSTANT_COLOR,
/// ONE_MINUS_CONSTANT_COLOR, CONSTANT_ALPHA, ONE_MINUS_CONSTANT_ALPHA). The blend state keeps each factor in
/// 6 bits, so those four are stored as 0x21..=0x24 (no other value uses that range). Storing them
/// unconverted truncated them to 33..36, which decoded as ONE.
pub fn blend_func_encode(v: u64) -> u64 {
    if (0x61..=0x64).contains(&v) {
        v - 0x40
    } else {
        v
    }
}
pub fn blend_func_decode(code: u64) -> u32 {
    (if (0x21..=0x24).contains(&code) {
        code + 0x40
    } else {
        code
    }) as u32
}

// Enumerant values used for defaults.
const BLEND_FUNC_ONE: u64 = 2;
const BLEND_FUNC_ZERO: u64 = 1;
const BLEND_EQUATION_ADD: u64 = 1;
const BLEND_ADVANCED_OVERLAP_UNCORRELATED: u64 = 1;
const DEPTH_FUNC_LESS: u64 = 2;
const STENCIL_FUNC_ALWAYS: u64 = 8;
const STENCIL_OP_KEEP: u64 = 1;
const FACE_FRONT: i32 = 1;
const FACE_BACK: i32 = 2;
const FRONT_FACE_CCW: u64 = 1;
const POLYGON_MODE_FILL: u64 = 2;
const LOGIC_OP_COPY: u64 = 3;
const ALPHA_FUNC_ALWAYS: u64 = 8;

// ── NVNblendState (8 bytes) ───────────────────────────────────────────────────────────────────
// target[0..4) srcRGB[4..10) dstRGB[10..16) srcA[16..22) dstA[22..28) eqRGB[28..32) eqA[32..36)
// advMode[36..42) overlap[42..45) premultSrc[45] normDst[46]
#[repr(C, packed)]
pub struct BlendState {
    bits: u64,
}
const _: () = assert!(std::mem::size_of::<BlendState>() == 8);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetDefaults(s: *mut BlendState) {
    let mut b = 0u64;
    put(&mut b, 4, 6, BLEND_FUNC_ONE);
    put(&mut b, 10, 6, BLEND_FUNC_ZERO);
    put(&mut b, 16, 6, BLEND_FUNC_ONE);
    put(&mut b, 22, 6, BLEND_FUNC_ZERO);
    put(&mut b, 28, 4, BLEND_EQUATION_ADD);
    put(&mut b, 32, 4, BLEND_EQUATION_ADD);
    put(&mut b, 42, 3, BLEND_ADVANCED_OVERLAP_UNCORRELATED);
    put(&mut b, 45, 1, 1);
    put(&mut b, 46, 1, 1);
    (*s).bits = b;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetBlendTarget(s: *mut BlendState, target: c_int) {
    setbits!(*s, 0, 4, target as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetBlendFunc(
    s: *mut BlendState,
    src: c_int,
    dst: c_int,
    src_a: c_int,
    dst_a: c_int,
) {
    setbits!(*s, 4, 6, blend_func_encode(src as u64));
    setbits!(*s, 10, 6, blend_func_encode(dst as u64));
    setbits!(*s, 16, 6, blend_func_encode(src_a as u64));
    setbits!(*s, 22, 6, blend_func_encode(dst_a as u64));
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetBlendEquation(
    s: *mut BlendState,
    rgb: c_int,
    alpha: c_int,
) {
    setbits!(*s, 28, 4, rgb as u64);
    setbits!(*s, 32, 4, alpha as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetAdvancedMode(s: *mut BlendState, mode: c_int) {
    setbits!(*s, 36, 6, mode as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetAdvancedOverlap(s: *mut BlendState, overlap: c_int) {
    setbits!(*s, 42, 3, overlap as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetAdvancedPremultipliedSrc(s: *mut BlendState, v: u8) {
    setbits!(*s, 45, 1, (v != 0) as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateSetAdvancedNormalizedDst(s: *mut BlendState, v: u8) {
    setbits!(*s, 46, 1, (v != 0) as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateGetBlendTarget(s: *const BlendState) -> c_int {
    get((*s).bits, 0, 4) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateGetBlendFunc(
    s: *const BlendState,
    src: *mut c_int,
    dst: *mut c_int,
    src_a: *mut c_int,
    dst_a: *mut c_int,
) {
    let b = (*s).bits;
    if !src.is_null() {
        *src = blend_func_decode(get(b, 4, 6)) as c_int;
    }
    if !dst.is_null() {
        *dst = blend_func_decode(get(b, 10, 6)) as c_int;
    }
    if !src_a.is_null() {
        *src_a = blend_func_decode(get(b, 16, 6)) as c_int;
    }
    if !dst_a.is_null() {
        *dst_a = blend_func_decode(get(b, 22, 6)) as c_int;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateGetBlendEquation(
    s: *const BlendState,
    rgb: *mut c_int,
    alpha: *mut c_int,
) {
    let b = (*s).bits;
    if !rgb.is_null() {
        *rgb = get(b, 28, 4) as c_int;
    }
    if !alpha.is_null() {
        *alpha = get(b, 32, 4) as c_int;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateGetAdvancedMode(s: *const BlendState) -> c_int {
    get((*s).bits, 36, 6) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateGetAdvancedOverlap(s: *const BlendState) -> c_int {
    get((*s).bits, 42, 3) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateGetAdvancedPremultipliedSrc(s: *const BlendState) -> u8 {
    get((*s).bits, 45, 1) as u8
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBlendStateGetAdvancedNormalizedDst(s: *const BlendState) -> u8 {
    get((*s).bits, 46, 1) as u8
}

// ── NVNchannelMaskState (4 bytes): 8 targets x RGBA bits ──────────────────────────────────────
#[repr(C, packed)]
pub struct ChannelMaskState {
    bits: u32,
}
const _: () = assert!(std::mem::size_of::<ChannelMaskState>() == 4);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnChannelMaskStateSetDefaults(s: *mut ChannelMaskState) {
    (*s).bits = 0xFFFF_FFFF;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnChannelMaskStateSetChannelMask(
    s: *mut ChannelMaskState,
    index: c_int,
    r: u8,
    g: u8,
    b: u8,
    a: u8,
) {
    if !(0..8).contains(&index) {
        return;
    }
    let v =
        (r != 0) as u32 | ((g != 0) as u32) << 1 | ((b != 0) as u32) << 2 | ((a != 0) as u32) << 3;
    let sh = index as u32 * 4;
    (*s).bits = ((*s).bits & !(0xF << sh)) | (v << sh);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnChannelMaskStateGetChannelMask(
    s: *const ChannelMaskState,
    index: c_int,
    r: *mut u8,
    g: *mut u8,
    b: *mut u8,
    a: *mut u8,
) {
    if !(0..8).contains(&index) {
        return;
    }
    let v = ((*s).bits >> (index as u32 * 4)) & 0xF;
    if !r.is_null() {
        *r = (v & 1) as u8;
    }
    if !g.is_null() {
        *g = ((v >> 1) & 1) as u8;
    }
    if !b.is_null() {
        *b = ((v >> 2) & 1) as u8;
    }
    if !a.is_null() {
        *a = ((v >> 3) & 1) as u8;
    }
}

// ── NVNcolorState (4 bytes): blendEnable[0..8) logicOp[8..13) alphaTest[13..17) ───────────────
#[repr(C, packed)]
pub struct ColorState {
    bits: u32,
}
const _: () = assert!(std::mem::size_of::<ColorState>() == 4);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnColorStateSetDefaults(s: *mut ColorState) {
    let mut b = 0u64;
    put(&mut b, 8, 5, LOGIC_OP_COPY);
    put(&mut b, 13, 4, ALPHA_FUNC_ALWAYS);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnColorStateSetBlendEnable(
    s: *mut ColorState,
    index: c_int,
    enable: u8,
) {
    if !(0..8).contains(&index) {
        return;
    }
    let m = 1u32 << index;
    (*s).bits = if enable != 0 {
        (*s).bits | m
    } else {
        (*s).bits & !m
    };
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnColorStateSetLogicOp(s: *mut ColorState, op: c_int) {
    let mut b = (*s).bits as u64;
    put(&mut b, 8, 5, op as u64);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnColorStateSetAlphaTest(s: *mut ColorState, func: c_int) {
    let mut b = (*s).bits as u64;
    put(&mut b, 13, 4, func as u64);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnColorStateGetBlendEnable(s: *const ColorState, index: c_int) -> u8 {
    if !(0..8).contains(&index) {
        return 0;
    }
    (((*s).bits >> index) & 1) as u8
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnColorStateGetLogicOp(s: *const ColorState) -> c_int {
    get((*s).bits as u64, 8, 5) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnColorStateGetAlphaTest(s: *const ColorState) -> c_int {
    get((*s).bits as u64, 13, 4) as c_int
}

// ── NVNdepthStencilState (8 bytes) ────────────────────────────────────────────────────────────
// depthTest[0] depthWrite[1] depthFunc[2..6) stencilTest[6]
// front: func[7..11) fail[11..15) depthFail[15..19) pass[19..23); back: func[23..27) fail[27..31)
// depthFail[31..35) pass[35..39)
#[repr(C, packed)]
pub struct DepthStencilState {
    bits: u64,
}
const _: () = assert!(std::mem::size_of::<DepthStencilState>() == 8);

const DS_FRONT: u32 = 7;
const DS_BACK: u32 = 23;

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateSetDefaults(s: *mut DepthStencilState) {
    let mut b = 0u64;
    put(&mut b, 1, 1, 1); // depth write
    put(&mut b, 2, 4, DEPTH_FUNC_LESS);
    for base in [DS_FRONT, DS_BACK] {
        put(&mut b, base, 4, STENCIL_FUNC_ALWAYS);
        put(&mut b, base + 4, 4, STENCIL_OP_KEEP);
        put(&mut b, base + 8, 4, STENCIL_OP_KEEP);
        put(&mut b, base + 12, 4, STENCIL_OP_KEEP);
    }
    (*s).bits = b;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateSetDepthTestEnable(
    s: *mut DepthStencilState,
    e: u8,
) {
    setbits!(*s, 0, 1, (e != 0) as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateSetDepthWriteEnable(
    s: *mut DepthStencilState,
    e: u8,
) {
    setbits!(*s, 1, 1, (e != 0) as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateSetDepthFunc(s: *mut DepthStencilState, f: c_int) {
    setbits!(*s, 2, 4, f as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateSetStencilTestEnable(
    s: *mut DepthStencilState,
    e: u8,
) {
    setbits!(*s, 6, 1, (e != 0) as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateSetStencilFunc(
    s: *mut DepthStencilState,
    face: c_int,
    f: c_int,
) {
    if face & FACE_FRONT != 0 {
        setbits!(*s, DS_FRONT, 4, f as u64);
    }
    if face & FACE_BACK != 0 {
        setbits!(*s, DS_BACK, 4, f as u64);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateSetStencilOp(
    s: *mut DepthStencilState,
    face: c_int,
    fail: c_int,
    depth_fail: c_int,
    pass: c_int,
) {
    for (mask, base) in [(FACE_FRONT, DS_FRONT), (FACE_BACK, DS_BACK)] {
        if face & mask != 0 {
            setbits!(*s, base + 4, 4, fail as u64);
            setbits!(*s, base + 8, 4, depth_fail as u64);
            setbits!(*s, base + 12, 4, pass as u64);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateGetDepthTestEnable(
    s: *const DepthStencilState,
) -> u8 {
    get((*s).bits, 0, 1) as u8
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateGetDepthWriteEnable(
    s: *const DepthStencilState,
) -> u8 {
    get((*s).bits, 1, 1) as u8
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateGetDepthFunc(
    s: *const DepthStencilState,
) -> c_int {
    get((*s).bits, 2, 4) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateGetStencilTestEnable(
    s: *const DepthStencilState,
) -> u8 {
    get((*s).bits, 6, 1) as u8
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateGetStencilFunc(
    s: *const DepthStencilState,
    face: c_int,
) -> c_int {
    let base = if face == FACE_BACK { DS_BACK } else { DS_FRONT };
    get((*s).bits, base, 4) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDepthStencilStateGetStencilOp(
    s: *const DepthStencilState,
    face: c_int,
    fail: *mut c_int,
    depth_fail: *mut c_int,
    pass: *mut c_int,
) {
    let base = if face == FACE_BACK { DS_BACK } else { DS_FRONT };
    let b = (*s).bits;
    if !fail.is_null() {
        *fail = get(b, base + 4, 4) as c_int;
    }
    if !depth_fail.is_null() {
        *depth_fail = get(b, base + 8, 4) as c_int;
    }
    if !pass.is_null() {
        *pass = get(b, base + 12, 4) as c_int;
    }
}

// ── NVNpolygonState (4 bytes): cull[0..2) front[2] mode[3..5) offsetEnables[5..8) ─────────────
#[repr(C, packed)]
pub struct PolygonState {
    bits: u32,
}
const _: () = assert!(std::mem::size_of::<PolygonState>() == 4);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateSetDefaults(s: *mut PolygonState) {
    let mut b = 0u64;
    put(&mut b, 2, 1, FRONT_FACE_CCW);
    put(&mut b, 3, 2, POLYGON_MODE_FILL);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateSetCullFace(s: *mut PolygonState, face: c_int) {
    let mut b = (*s).bits as u64;
    put(&mut b, 0, 2, face as u64);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateSetFrontFace(s: *mut PolygonState, face: c_int) {
    let mut b = (*s).bits as u64;
    put(&mut b, 2, 1, face as u64);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateSetPolygonMode(s: *mut PolygonState, mode: c_int) {
    let mut b = (*s).bits as u64;
    put(&mut b, 3, 2, mode as u64);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateSetPolygonOffsetEnables(
    s: *mut PolygonState,
    enables: c_int,
) {
    let mut b = (*s).bits as u64;
    put(&mut b, 5, 3, enables as u64);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateGetCullFace(s: *const PolygonState) -> c_int {
    get((*s).bits as u64, 0, 2) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateGetFrontFace(s: *const PolygonState) -> c_int {
    get((*s).bits as u64, 2, 1) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateGetPolygonMode(s: *const PolygonState) -> c_int {
    get((*s).bits as u64, 3, 2) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnPolygonStateGetPolygonOffsetEnables(
    s: *const PolygonState,
) -> c_int {
    get((*s).bits as u64, 5, 3) as c_int
}

// ── NVNvertexAttribState (4 bytes): stream[0..5) format[5..14) offset[14..32) ──────
#[repr(C, packed)]
pub struct VertexAttribState {
    bits: u32,
}
const _: () = assert!(std::mem::size_of::<VertexAttribState>() == 4);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexAttribStateSetDefaults(s: *mut VertexAttribState) {
    (*s).bits = 0;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexAttribStateSetFormat(
    s: *mut VertexAttribState,
    format: c_int,
    rel_offset: isize,
) {
    let mut b = (*s).bits as u64;
    put(&mut b, 5, 9, format as u64);
    put(&mut b, 14, 18, rel_offset as u64);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexAttribStateSetStreamIndex(
    s: *mut VertexAttribState,
    index: c_int,
) {
    let mut b = (*s).bits as u64;
    put(&mut b, 0, 5, (index as u64) & 31);
    (*s).bits = b as u32;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexAttribStateGetFormat(
    s: *const VertexAttribState,
    format: *mut c_int,
    rel_offset: *mut isize,
) {
    let b = (*s).bits as u64;
    if !format.is_null() {
        *format = get(b, 5, 9) as c_int;
    }
    if !rel_offset.is_null() {
        *rel_offset = get(b, 14, 18) as isize;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexAttribStateGetStreamIndex(
    s: *const VertexAttribState,
) -> c_int {
    (get((*s).bits as u64, 0, 5) & 31) as c_int
}

// ── NVNvertexStreamState (8 bytes): stride[0..32) divisor[32..64) ─────────────────────────────
#[repr(C, packed)]
pub struct VertexStreamState {
    bits: u64,
}
const _: () = assert!(std::mem::size_of::<VertexStreamState>() == 8);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexStreamStateSetDefaults(s: *mut VertexStreamState) {
    (*s).bits = 0;
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexStreamStateSetStride(
    s: *mut VertexStreamState,
    stride: isize,
) {
    setbits!(*s, 0, 32, stride as u32 as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexStreamStateSetDivisor(
    s: *mut VertexStreamState,
    divisor: c_int,
) {
    setbits!(*s, 32, 32, divisor as u32 as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexStreamStateGetStride(s: *const VertexStreamState) -> isize {
    get((*s).bits, 0, 32) as u32 as isize
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnVertexStreamStateGetDivisor(s: *const VertexStreamState) -> c_int {
    get((*s).bits, 32, 32) as u32 as c_int
}

// ── NVNmultisampleState (24 bytes) ────────────────────────────────────────────────────────────
// bits: enable[0] a2c[1] a2cDither[2] samples[3..8) raster[8..13) covMod[13..16) cov2color[16]
// cov2colorOut[17..20) locEnable[20] gridEnable[21]; then 16 bytes of quantized sample locations
// (each float in [0,1] stored as u8 = value*255).
#[repr(C, packed)]
pub struct MultisampleState {
    bits: u64,
    locations: [u8; 16],
}
const _: () = assert!(std::mem::size_of::<MultisampleState>() == 24);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateSetDefaults(s: *mut MultisampleState) {
    let mut b = 0u64;
    put(&mut b, 0, 1, 1); // multisample enable
    put(&mut b, 2, 1, 1); // alpha-to-coverage dither
    (*s).bits = b;
    (*s).locations = [128; 16]; // pixel centre
}
macro_rules! ms_flag {
    ($set:ident, $get:ident, $shift:expr) => {
        #[no_mangle]
        pub unsafe extern "C" fn $set(s: *mut MultisampleState, v: u8) {
            setbits!(*s, $shift, 1, (v != 0) as u64);
        }
        #[no_mangle]
        pub unsafe extern "C" fn $get(s: *const MultisampleState) -> u8 {
            get((*s).bits, $shift, 1) as u8
        }
    };
}
ms_flag!(
    hle_nvnMultisampleStateSetMultisampleEnable,
    hle_nvnMultisampleStateGetMultisampleEnable,
    0
);
ms_flag!(
    hle_nvnMultisampleStateSetAlphaToCoverageEnable,
    hle_nvnMultisampleStateGetAlphaToCoverageEnable,
    1
);
ms_flag!(
    hle_nvnMultisampleStateSetAlphaToCoverageDither,
    hle_nvnMultisampleStateGetAlphaToCoverageDither,
    2
);
ms_flag!(
    hle_nvnMultisampleStateSetCoverageToColorEnable,
    hle_nvnMultisampleStateGetCoverageToColorEnable,
    16
);
ms_flag!(
    hle_nvnMultisampleStateSetSampleLocationsEnable,
    hle_nvnMultisampleStateGetSampleLocationsEnable,
    20
);
ms_flag!(
    hle_nvnMultisampleStateSetSampleLocationsGridEnable,
    hle_nvnMultisampleStateGetSampleLocationsGridEnable,
    21
);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateSetSamples(s: *mut MultisampleState, n: c_int) {
    setbits!(*s, 3, 5, n as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateGetSamples(s: *const MultisampleState) -> c_int {
    get((*s).bits, 3, 5) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateSetRasterSamples(
    s: *mut MultisampleState,
    n: c_int,
) {
    setbits!(*s, 8, 5, n as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateGetRasterSamples(
    s: *const MultisampleState,
) -> c_int {
    get((*s).bits, 8, 5) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateSetCoverageModulationMode(
    s: *mut MultisampleState,
    m: c_int,
) {
    setbits!(*s, 13, 3, m as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateGetCoverageModulationMode(
    s: *const MultisampleState,
) -> c_int {
    get((*s).bits, 13, 3) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateSetCoverageToColorOutput(
    s: *mut MultisampleState,
    c: c_int,
) {
    setbits!(*s, 17, 3, c as u64);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateGetCoverageToColorOutput(
    s: *const MultisampleState,
) -> c_int {
    get((*s).bits, 17, 3) as c_int
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateGetSampleLocationsGrid(
    _s: *mut MultisampleState,
    w: *mut c_int,
    h: *mut c_int,
) {
    // The standard sample-location grid is 1x1 pixels.
    if !w.is_null() {
        *w = 1;
    }
    if !h.is_null() {
        *h = 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMultisampleStateSetSampleLocations(
    s: *mut MultisampleState,
    start: c_int,
    count: c_int,
    values: *const f32,
) {
    if values.is_null() {
        return;
    }
    for i in 0..count.max(0) as usize {
        let idx = start.max(0) as usize + i;
        if idx >= 16 {
            break;
        }
        (*s).locations[idx] = ((*values.add(i)).clamp(0.0, 1.0) * 255.0) as u8;
    }
}

// ── texture handles ───────────────────────────────────────────────────────────────────────────
// Maxwell texture handle: texture (TIC) index in the low 20 bits, sampler (TSC) index above.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetTextureHandle(
    _device: *const u8,
    texture_id: c_int,
    sampler_id: c_int,
) -> u64 {
    (texture_id as u32 as u64 & 0xFFFFF) | ((sampler_id as u32 as u64 & 0xFFF) << 20)
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetTexelFetchHandle(
    _device: *const u8,
    texture_id: c_int,
) -> u64 {
    texture_id as u32 as u64 & 0xFFFFF
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetImageHandle(_device: *const u8, image_id: c_int) -> u64 {
    image_id as u32 as u64 & 0xFFFFF
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetSeparateTextureHandle(
    _device: *const u8,
    texture_id: c_int,
) -> u64 {
    texture_id as u32 as u64 & 0xFFFFF
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetSeparateSamplerHandle(
    _device: *const u8,
    sampler_id: c_int,
) -> u64 {
    (sampler_id as u32 as u64 & 0xFFF) << 20
}

/// NVN function name -> HLE address for everything in this module.
pub fn lookup(name: &str) -> Option<usize> {
    macro_rules! f {
        ($f:expr) => {
            Some($f as *const () as usize)
        };
    }
    match name {
        "nvnBlendStateSetDefaults" => f!(hle_nvnBlendStateSetDefaults),
        "nvnBlendStateSetBlendTarget" => f!(hle_nvnBlendStateSetBlendTarget),
        "nvnBlendStateSetBlendFunc" => f!(hle_nvnBlendStateSetBlendFunc),
        "nvnBlendStateSetBlendEquation" => f!(hle_nvnBlendStateSetBlendEquation),
        "nvnBlendStateSetAdvancedMode" => f!(hle_nvnBlendStateSetAdvancedMode),
        "nvnBlendStateSetAdvancedOverlap" => f!(hle_nvnBlendStateSetAdvancedOverlap),
        "nvnBlendStateSetAdvancedPremultipliedSrc" => {
            f!(hle_nvnBlendStateSetAdvancedPremultipliedSrc)
        }
        "nvnBlendStateSetAdvancedNormalizedDst" => f!(hle_nvnBlendStateSetAdvancedNormalizedDst),
        "nvnBlendStateGetBlendTarget" => f!(hle_nvnBlendStateGetBlendTarget),
        "nvnBlendStateGetBlendFunc" => f!(hle_nvnBlendStateGetBlendFunc),
        "nvnBlendStateGetBlendEquation" => f!(hle_nvnBlendStateGetBlendEquation),
        "nvnBlendStateGetAdvancedMode" => f!(hle_nvnBlendStateGetAdvancedMode),
        "nvnBlendStateGetAdvancedOverlap" => f!(hle_nvnBlendStateGetAdvancedOverlap),
        "nvnBlendStateGetAdvancedPremultipliedSrc" => {
            f!(hle_nvnBlendStateGetAdvancedPremultipliedSrc)
        }
        "nvnBlendStateGetAdvancedNormalizedDst" => f!(hle_nvnBlendStateGetAdvancedNormalizedDst),
        "nvnChannelMaskStateSetDefaults" => f!(hle_nvnChannelMaskStateSetDefaults),
        "nvnChannelMaskStateSetChannelMask" => f!(hle_nvnChannelMaskStateSetChannelMask),
        "nvnChannelMaskStateGetChannelMask" => f!(hle_nvnChannelMaskStateGetChannelMask),
        "nvnColorStateSetDefaults" => f!(hle_nvnColorStateSetDefaults),
        "nvnColorStateSetBlendEnable" => f!(hle_nvnColorStateSetBlendEnable),
        "nvnColorStateSetLogicOp" => f!(hle_nvnColorStateSetLogicOp),
        "nvnColorStateSetAlphaTest" => f!(hle_nvnColorStateSetAlphaTest),
        "nvnColorStateGetBlendEnable" => f!(hle_nvnColorStateGetBlendEnable),
        "nvnColorStateGetLogicOp" => f!(hle_nvnColorStateGetLogicOp),
        "nvnColorStateGetAlphaTest" => f!(hle_nvnColorStateGetAlphaTest),
        "nvnDepthStencilStateSetDefaults" => f!(hle_nvnDepthStencilStateSetDefaults),
        "nvnDepthStencilStateSetDepthTestEnable" => f!(hle_nvnDepthStencilStateSetDepthTestEnable),
        "nvnDepthStencilStateSetDepthWriteEnable" => {
            f!(hle_nvnDepthStencilStateSetDepthWriteEnable)
        }
        "nvnDepthStencilStateSetDepthFunc" => f!(hle_nvnDepthStencilStateSetDepthFunc),
        "nvnDepthStencilStateSetStencilTestEnable" => {
            f!(hle_nvnDepthStencilStateSetStencilTestEnable)
        }
        "nvnDepthStencilStateSetStencilFunc" => f!(hle_nvnDepthStencilStateSetStencilFunc),
        "nvnDepthStencilStateSetStencilOp" => f!(hle_nvnDepthStencilStateSetStencilOp),
        "nvnDepthStencilStateGetDepthTestEnable" => f!(hle_nvnDepthStencilStateGetDepthTestEnable),
        "nvnDepthStencilStateGetDepthWriteEnable" => {
            f!(hle_nvnDepthStencilStateGetDepthWriteEnable)
        }
        "nvnDepthStencilStateGetDepthFunc" => f!(hle_nvnDepthStencilStateGetDepthFunc),
        "nvnDepthStencilStateGetStencilTestEnable" => {
            f!(hle_nvnDepthStencilStateGetStencilTestEnable)
        }
        "nvnDepthStencilStateGetStencilFunc" => f!(hle_nvnDepthStencilStateGetStencilFunc),
        "nvnDepthStencilStateGetStencilOp" => f!(hle_nvnDepthStencilStateGetStencilOp),
        "nvnPolygonStateSetDefaults" => f!(hle_nvnPolygonStateSetDefaults),
        "nvnPolygonStateSetCullFace" => f!(hle_nvnPolygonStateSetCullFace),
        "nvnPolygonStateSetFrontFace" => f!(hle_nvnPolygonStateSetFrontFace),
        "nvnPolygonStateSetPolygonMode" => f!(hle_nvnPolygonStateSetPolygonMode),
        "nvnPolygonStateSetPolygonOffsetEnables" => f!(hle_nvnPolygonStateSetPolygonOffsetEnables),
        "nvnPolygonStateGetCullFace" => f!(hle_nvnPolygonStateGetCullFace),
        "nvnPolygonStateGetFrontFace" => f!(hle_nvnPolygonStateGetFrontFace),
        "nvnPolygonStateGetPolygonMode" => f!(hle_nvnPolygonStateGetPolygonMode),
        "nvnPolygonStateGetPolygonOffsetEnables" => f!(hle_nvnPolygonStateGetPolygonOffsetEnables),
        "nvnVertexAttribStateSetDefaults" => f!(hle_nvnVertexAttribStateSetDefaults),
        "nvnVertexAttribStateSetFormat" => f!(hle_nvnVertexAttribStateSetFormat),
        "nvnVertexAttribStateSetStreamIndex" => f!(hle_nvnVertexAttribStateSetStreamIndex),
        "nvnVertexAttribStateGetFormat" => f!(hle_nvnVertexAttribStateGetFormat),
        "nvnVertexAttribStateGetStreamIndex" => f!(hle_nvnVertexAttribStateGetStreamIndex),
        "nvnVertexStreamStateSetDefaults" => f!(hle_nvnVertexStreamStateSetDefaults),
        "nvnVertexStreamStateSetStride" => f!(hle_nvnVertexStreamStateSetStride),
        "nvnVertexStreamStateSetDivisor" => f!(hle_nvnVertexStreamStateSetDivisor),
        "nvnVertexStreamStateGetStride" => f!(hle_nvnVertexStreamStateGetStride),
        "nvnVertexStreamStateGetDivisor" => f!(hle_nvnVertexStreamStateGetDivisor),
        "nvnMultisampleStateSetDefaults" => f!(hle_nvnMultisampleStateSetDefaults),
        "nvnMultisampleStateSetMultisampleEnable" => {
            f!(hle_nvnMultisampleStateSetMultisampleEnable)
        }
        "nvnMultisampleStateGetMultisampleEnable" => {
            f!(hle_nvnMultisampleStateGetMultisampleEnable)
        }
        "nvnMultisampleStateSetAlphaToCoverageEnable" => {
            f!(hle_nvnMultisampleStateSetAlphaToCoverageEnable)
        }
        "nvnMultisampleStateGetAlphaToCoverageEnable" => {
            f!(hle_nvnMultisampleStateGetAlphaToCoverageEnable)
        }
        "nvnMultisampleStateSetAlphaToCoverageDither" => {
            f!(hle_nvnMultisampleStateSetAlphaToCoverageDither)
        }
        "nvnMultisampleStateGetAlphaToCoverageDither" => {
            f!(hle_nvnMultisampleStateGetAlphaToCoverageDither)
        }
        "nvnMultisampleStateSetCoverageToColorEnable" => {
            f!(hle_nvnMultisampleStateSetCoverageToColorEnable)
        }
        "nvnMultisampleStateGetCoverageToColorEnable" => {
            f!(hle_nvnMultisampleStateGetCoverageToColorEnable)
        }
        "nvnMultisampleStateSetSampleLocationsEnable" => {
            f!(hle_nvnMultisampleStateSetSampleLocationsEnable)
        }
        "nvnMultisampleStateGetSampleLocationsEnable" => {
            f!(hle_nvnMultisampleStateGetSampleLocationsEnable)
        }
        "nvnMultisampleStateSetSampleLocationsGridEnable" => {
            f!(hle_nvnMultisampleStateSetSampleLocationsGridEnable)
        }
        "nvnMultisampleStateGetSampleLocationsGridEnable" => {
            f!(hle_nvnMultisampleStateGetSampleLocationsGridEnable)
        }
        "nvnMultisampleStateSetSamples" => f!(hle_nvnMultisampleStateSetSamples),
        "nvnMultisampleStateGetSamples" => f!(hle_nvnMultisampleStateGetSamples),
        "nvnMultisampleStateSetRasterSamples" => f!(hle_nvnMultisampleStateSetRasterSamples),
        "nvnMultisampleStateGetRasterSamples" => f!(hle_nvnMultisampleStateGetRasterSamples),
        "nvnMultisampleStateSetCoverageModulationMode" => {
            f!(hle_nvnMultisampleStateSetCoverageModulationMode)
        }
        "nvnMultisampleStateGetCoverageModulationMode" => {
            f!(hle_nvnMultisampleStateGetCoverageModulationMode)
        }
        "nvnMultisampleStateSetCoverageToColorOutput" => {
            f!(hle_nvnMultisampleStateSetCoverageToColorOutput)
        }
        "nvnMultisampleStateGetCoverageToColorOutput" => {
            f!(hle_nvnMultisampleStateGetCoverageToColorOutput)
        }
        "nvnMultisampleStateGetSampleLocationsGrid" => {
            f!(hle_nvnMultisampleStateGetSampleLocationsGrid)
        }
        "nvnMultisampleStateSetSampleLocations" => f!(hle_nvnMultisampleStateSetSampleLocations),
        "nvnDeviceGetTextureHandle" => f!(hle_nvnDeviceGetTextureHandle),
        "nvnDeviceGetTexelFetchHandle" => f!(hle_nvnDeviceGetTexelFetchHandle),
        "nvnDeviceGetImageHandle" => f!(hle_nvnDeviceGetImageHandle),
        "nvnDeviceGetSeparateTextureHandle" => f!(hle_nvnDeviceGetSeparateTextureHandle),
        "nvnDeviceGetSeparateSamplerHandle" => f!(hle_nvnDeviceGetSeparateSamplerHandle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_stencil_defaults_and_face_masks() {
        let mut s = DepthStencilState { bits: 0 };
        unsafe {
            hle_nvnDepthStencilStateSetDefaults(&mut s);
            assert_eq!(hle_nvnDepthStencilStateGetDepthFunc(&s), 2);
            assert_eq!(hle_nvnDepthStencilStateGetDepthWriteEnable(&s), 1);
            assert_eq!(hle_nvnDepthStencilStateGetStencilFunc(&s, FACE_BACK), 8);
            hle_nvnDepthStencilStateSetStencilFunc(&mut s, FACE_BACK, 3);
            assert_eq!(hle_nvnDepthStencilStateGetStencilFunc(&s, FACE_BACK), 3);
            assert_eq!(
                hle_nvnDepthStencilStateGetStencilFunc(&s, FACE_FRONT),
                8,
                "front untouched"
            );
            hle_nvnDepthStencilStateSetStencilOp(&mut s, 3, 2, 3, 4);
            let (mut f, mut d, mut p) = (0, 0, 0);
            hle_nvnDepthStencilStateGetStencilOp(&s, FACE_FRONT, &mut f, &mut d, &mut p);
            assert_eq!((f, d, p), (2, 3, 4));
        }
    }

    #[test]
    fn blend_and_vertex_state_round_trip() {
        unsafe {
            let mut b = BlendState { bits: 0 };
            hle_nvnBlendStateSetDefaults(&mut b);
            hle_nvnBlendStateSetBlendFunc(&mut b, 5, 6, 7, 8);
            let (mut a, mut c, mut d, mut e) = (0, 0, 0, 0);
            hle_nvnBlendStateGetBlendFunc(&b, &mut a, &mut c, &mut d, &mut e);
            assert_eq!((a, c, d, e), (5, 6, 7, 8));
            hle_nvnBlendStateSetBlendFunc(&mut b, 0x63, 0x64, 0x61, 0x62);
            hle_nvnBlendStateGetBlendFunc(&b, &mut a, &mut c, &mut d, &mut e);
            assert_eq!((a, c, d, e), (0x63, 0x64, 0x61, 0x62));
            let mut v = VertexAttribState { bits: 0 };
            hle_nvnVertexAttribStateSetFormat(&mut v, 41, 24);
            hle_nvnVertexAttribStateSetStreamIndex(&mut v, 3);
            let (mut f, mut o) = (0, 0);
            hle_nvnVertexAttribStateGetFormat(&v, &mut f, &mut o);
            assert_eq!(
                (f, o, hle_nvnVertexAttribStateGetStreamIndex(&v)),
                (41, 24, 3)
            );
            let mut st = VertexStreamState { bits: 0 };
            hle_nvnVertexStreamStateSetStride(&mut st, 48);
            hle_nvnVertexStreamStateSetDivisor(&mut st, 1);
            assert_eq!(
                (
                    hle_nvnVertexStreamStateGetStride(&st),
                    hle_nvnVertexStreamStateGetDivisor(&st)
                ),
                (48, 1)
            );
        }
    }

    #[test]
    fn texture_handle_encoding() {
        unsafe {
            let h = hle_nvnDeviceGetTextureHandle(std::ptr::null(), 0x2eb, 0x120);
            assert_eq!(h & 0xFFFFF, 0x2eb);
            assert_eq!(h >> 20, 0x120);
        }
    }
}
