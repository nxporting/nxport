// NVN samplers and sampler builders.

use super::*;

// ---------------------------------------------------------------------------
// NVN Sampler family. NVNsampler 0x30 (48) bytes, NVNsamplerBuilder 0x60 (96)
// bytes. Sampler state (filtering, wrap mode, anisotropy, LOD) only matters
// once real Metal translation samples a texture -- for now this just needs
// to not crash and hold the state faithfully for that later pass.
#[repr(C)]
pub struct NvnSamplerBuilder {
    pub device: usize,
    pub min_filter: i32,
    pub mag_filter: i32,
    pub wrap_s: i32,
    pub wrap_t: i32,
    pub wrap_r: i32,
    pub compare_mode: i8,
    pub compare_func: i8,
    pub reduction_filter: i16,
    pub max_anisotropy: f32,
    pub lod_min: f32,
    pub lod_max: f32,
    pub lod_bias: f32,
    pub border_color: [f32; 4],
}

#[repr(C)]
pub struct NvnSampler {
    pub min_filter: i32,
    pub mag_filter: i32,
    pub wrap_s: i32,
    pub wrap_t: i32,
    pub wrap_r: i32,
    pub compare_mode: i8,
    pub compare_func: i8,
    pub reduction_filter: i16,
    pub max_anisotropy: f32,
    pub lod_min: f32,
    pub lod_max: f32,
    pub lod_bias: f32,
    pub border_color: [f32; 4],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetDefaults(b: *mut NvnSamplerBuilder) {
    record_hle_call("hle_nvnSamplerBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
        (*b).max_anisotropy = 1.0;
        (*b).lod_max = 1000.0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetDevice(b: *mut NvnSamplerBuilder, device: usize) {
    record_hle_call("hle_nvnSamplerBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetMinMagFilter(
    b: *mut NvnSamplerBuilder,
    min_f: i32,
    mag_f: i32,
) {
    record_hle_call("hle_nvnSamplerBuilderSetMinMagFilter");
    if !b.is_null() {
        (*b).min_filter = min_f;
        (*b).mag_filter = mag_f;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetWrapMode(
    b: *mut NvnSamplerBuilder,
    s: i32,
    t: i32,
    r: i32,
) {
    record_hle_call("hle_nvnSamplerBuilderSetWrapMode");
    if !b.is_null() {
        (*b).wrap_s = s;
        (*b).wrap_t = t;
        (*b).wrap_r = r;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetBorderColor(
    b: *mut NvnSamplerBuilder,
    color: *const f32,
) {
    record_hle_call("hle_nvnSamplerBuilderSetBorderColor");
    if !b.is_null() && !color.is_null() {
        (*b).border_color = std::slice::from_raw_parts(color, 4).try_into().unwrap();
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetMaxAnisotropy(
    b: *mut NvnSamplerBuilder,
    max_aniso: f32,
) {
    record_hle_call("hle_nvnSamplerBuilderSetMaxAnisotropy");
    if !b.is_null() {
        (*b).max_anisotropy = max_aniso;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetLodClamp(
    b: *mut NvnSamplerBuilder,
    lod_min: f32,
    lod_max: f32,
) {
    record_hle_call("hle_nvnSamplerBuilderSetLodClamp");
    if !b.is_null() {
        (*b).lod_min = lod_min;
        (*b).lod_max = lod_max;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetLodBias(b: *mut NvnSamplerBuilder, bias: f32) {
    record_hle_call("hle_nvnSamplerBuilderSetLodBias");
    if !b.is_null() {
        (*b).lod_bias = bias;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetCompare(
    b: *mut NvnSamplerBuilder,
    mode: i32,
    func: i32,
) {
    record_hle_call("hle_nvnSamplerBuilderSetCompare");
    if !b.is_null() {
        (*b).compare_mode = mode as i8;
        (*b).compare_func = func as i8;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerInitialize(
    s: *mut NvnSampler,
    builder: *const NvnSamplerBuilder,
) -> u8 {
    record_hle_call("hle_nvnSamplerInitialize");
    if s.is_null() || builder.is_null() {
        return 0;
    }
    let b = &*builder;
    std::ptr::write(
        s,
        NvnSampler {
            min_filter: b.min_filter,
            mag_filter: b.mag_filter,
            wrap_s: b.wrap_s,
            wrap_t: b.wrap_t,
            wrap_r: b.wrap_r,
            compare_mode: b.compare_mode,
            compare_func: b.compare_func,
            reduction_filter: b.reduction_filter,
            max_anisotropy: b.max_anisotropy,
            lod_min: b.lod_min,
            lod_max: b.lod_max,
            lod_bias: b.lod_bias,
            border_color: b.border_color,
        },
    );
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderSetReductionFilter(
    b: *mut NvnSamplerBuilder,
    filter: i32,
) {
    if !b.is_null() {
        (*b).reduction_filter = filter as i16;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetReductionFilter(s: *const NvnSampler) -> i32 {
    if s.is_null() {
        0
    } else {
        (*s).reduction_filter as i32
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetMaxAnisotropy(s: *const NvnSampler) -> f32 {
    if s.is_null() {
        1.0
    } else {
        (*s).max_anisotropy
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetLodClamp(
    s: *const NvnSampler,
    lod_min: *mut f32,
    lod_max: *mut f32,
) {
    if s.is_null() {
        return;
    }
    if !lod_min.is_null() {
        *lod_min = (*s).lod_min;
    }
    if !lod_max.is_null() {
        *lod_max = (*s).lod_max;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetLodBias(s: *const NvnSampler) -> f32 {
    if s.is_null() {
        0.0
    } else {
        (*s).lod_bias
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetCompare(
    s: *const NvnSampler,
    mode: *mut i32,
    func: *mut i32,
) {
    if s.is_null() {
        return;
    }
    if !mode.is_null() {
        *mode = (*s).compare_mode as i32;
    }
    if !func.is_null() {
        *func = (*s).compare_func as i32;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetBorderColor(s: *const NvnSampler, color: *mut f32) {
    if s.is_null() || color.is_null() {
        return;
    }
    for i in 0..4 {
        *color.add(i) = (*s).border_color[i];
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerFinalize(s: *mut NvnSampler) {
    record_hle_call("hle_nvnSamplerFinalize");
    if !s.is_null() {
        std::ptr::write_bytes(s, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetDevice(b: *const NvnSamplerBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetMinMagFilter(
    b: *const NvnSamplerBuilder,
    min: *mut i32,
    mag: *mut i32,
) {
    if b.is_null() {
        return;
    }
    if !min.is_null() {
        *min = (*b).min_filter;
    }
    if !mag.is_null() {
        *mag = (*b).mag_filter;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetWrapMode(
    b: *const NvnSamplerBuilder,
    s: *mut i32,
    t: *mut i32,
    r: *mut i32,
) {
    if b.is_null() {
        return;
    }
    if !s.is_null() {
        *s = (*b).wrap_s;
    }
    if !t.is_null() {
        *t = (*b).wrap_t;
    }
    if !r.is_null() {
        *r = (*b).wrap_r;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetLodClamp(
    b: *const NvnSamplerBuilder,
    min: *mut f32,
    max: *mut f32,
) {
    if b.is_null() {
        return;
    }
    if !min.is_null() {
        *min = (*b).lod_min;
    }
    if !max.is_null() {
        *max = (*b).lod_max;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetLodBias(b: *const NvnSamplerBuilder) -> f32 {
    if b.is_null() {
        0.0
    } else {
        (*b).lod_bias
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetCompare(
    b: *const NvnSamplerBuilder,
    mode: *mut i32,
    func: *mut i32,
) {
    if b.is_null() {
        return;
    }
    if !mode.is_null() {
        *mode = (*b).compare_mode as i32;
    }
    if !func.is_null() {
        *func = (*b).compare_func as i32;
    }
}

/// Float, int and uint variants share the 16 bytes: the integer setters store the raw words, which the matching getter
/// returns unchanged (the executor only distinguishes Metal's three fixed border colors).
#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetBorderColor(
    b: *const NvnSamplerBuilder,
    color: *mut f32,
) {
    if !b.is_null() && !color.is_null() {
        std::ptr::copy_nonoverlapping((*b).border_color.as_ptr(), color, 4);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetMaxAnisotropy(b: *const NvnSamplerBuilder) -> f32 {
    if b.is_null() {
        1.0
    } else {
        (*b).max_anisotropy
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerBuilderGetReductionFilter(
    b: *const NvnSamplerBuilder,
) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).reduction_filter as i32
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerCompare(a: *const NvnSampler, b: *const NvnSampler) -> u8 {
    if a.is_null() || b.is_null() {
        return (a == b) as u8;
    }
    let n = std::mem::size_of::<NvnSampler>();
    (std::slice::from_raw_parts(a as *const u8, n) == std::slice::from_raw_parts(b as *const u8, n))
        as u8
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetMinMagFilter(
    s: *const NvnSampler,
    min: *mut i32,
    mag: *mut i32,
) {
    record_hle_call("hle_nvnSamplerGetMinMagFilter");
    if !s.is_null() {
        if !min.is_null() {
            *min = (*s).min_filter;
        }
        if !mag.is_null() {
            *mag = (*s).mag_filter;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerGetWrapMode(
    s: *const NvnSampler,
    wrap_s: *mut i32,
    wrap_t: *mut i32,
    wrap_r: *mut i32,
) {
    record_hle_call("hle_nvnSamplerGetWrapMode");
    if !s.is_null() {
        if !wrap_s.is_null() {
            *wrap_s = (*s).wrap_s;
        }
        if !wrap_t.is_null() {
            *wrap_t = (*s).wrap_t;
        }
        if !wrap_r.is_null() {
            *wrap_r = (*s).wrap_r;
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerSetDebugLabel(
    _s: *mut NvnSampler,
    _label: *const libc::c_char,
) {
    record_hle_call("hle_nvnSamplerSetDebugLabel");
}
