// NVN programs and the glslc compiler entry points (compiling is not supported: games ship compiled shaders).

use super::*;

// ── NVN Program family ──────────────────────────────────────────────────────
// `NVNprogram` is an opaque `char reserved[128]` in NVN, so the layout inside
// is ours to define. The game's shader-init path (guest+0x2477140, called per
// ShaderVariation while loading a BNSH) does nvnProgramInitialize followed by
// nvnProgramSetShaders[Ext] and treats a false return from SetShaders as
// "this variation is unusable" -- it then falls through to the next
// binary/IR/source probe and, with none left, executes the compiler-inserted
// null dereference at guest+0x51400. Both used to hit the unimplemented-stub
// pool (returns 0), which is exactly that failure.
#[repr(C)]
pub struct NvnProgram {
    device: usize,
    stage_count: u32,
    initialized: u32,
    // First stage's NVNshaderData {data address, control pointer}. NVNshaderData
    // and NVNshaderDataExt both begin with these two words; the Ext stride isn't
    // part of the public header, so only the first entry is recorded.
    first_stage_data: u64,
    first_stage_control: u64,
    sample_shading: u32,
    _reserved: [u8; 0x80 - 40],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramInitialize(p: *mut NvnProgram, device: usize) -> u8 {
    record_hle_call("hle_nvnProgramInitialize");
    if p.is_null() {
        return 0;
    }
    std::ptr::write_bytes(p, 0, 1);
    (*p).device = device;
    (*p).initialized = 1;
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramFinalize(p: *mut NvnProgram) {
    record_hle_call("hle_nvnProgramFinalize");
    if !p.is_null() {
        PROGRAM_STAGES.lock().unwrap().remove(&(p as usize));
        std::ptr::write_bytes(p, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramSetDebugLabel(_p: *mut NvnProgram, _label: *const c_char) {
    record_hle_call("hle_nvnProgramSetDebugLabel");
}

/// The stages each program was last given (frame captures).
/// The stages (code, control section) program `p` was last given.
pub fn program_stages(p: usize) -> Vec<(u64, u64)> {
    PROGRAM_STAGES.lock().unwrap().get(&p).cloned().unwrap_or_default()
}

pub(crate) static PROGRAM_STAGES: std::sync::LazyLock<StdMutex<rustc_hash::FxHashMap<usize, Vec<(u64, u64)>>>> =
    std::sync::LazyLock::new(|| StdMutex::new(rustc_hash::FxHashMap::default()));

pub(crate) unsafe fn program_set_shaders(
    p: *mut NvnProgram,
    count: i32,
    stages: *const u64,
    stride_words: usize,
) -> u8 {
    if p.is_null() || (*p).initialized == 0 || count < 0 || (count > 0 && stages.is_null()) {
        return 0;
    }
    (*p).stage_count = count as u32;
    // NVNshaderData is {data, control} (2 words); NVNshaderDataExt adds a debug-hash pointer (3 words).
    let list: Vec<crate::gpu::ShaderStage> = (0..count as usize)
        .map(|i| crate::gpu::ShaderStage {
            data: std::ptr::read_unaligned(stages.add(i * stride_words)),
            control: std::ptr::read_unaligned(stages.add(i * stride_words + 1)),
        })
        .collect();
    if let Some(first) = list.first() {
        (*p).first_stage_data = first.data;
        (*p).first_stage_control = first.control;
    }
    PROGRAM_STAGES.lock().unwrap().insert(p as usize, list.iter().map(|s| (s.data, s.control)).collect());
    crate::gpu::backend().set_program(p as usize, &list);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramSetShaders(
    p: *mut NvnProgram,
    count: i32,
    stages: *const u64,
) -> u8 {
    record_hle_call("hle_nvnProgramSetShaders");
    program_set_shaders(p, count, stages, 2)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramSetShadersExt(
    p: *mut NvnProgram,
    count: i32,
    stages: *const u64,
) -> u8 {
    record_hle_call("hle_nvnProgramSetShadersExt");
    program_set_shaders(p, count, stages, 3)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramSetSubroutineLinkage(
    p: *mut NvnProgram,
    _count: i32,
    _maps: *const (),
) -> u8 {
    record_hle_call("hle_nvnProgramSetSubroutineLinkage");
    if p.is_null() || (*p).initialized == 0 {
        0
    } else {
        1
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramSetSampleShading(p: *mut NvnProgram, enable: u8) {
    record_hle_call("hle_nvnProgramSetSampleShading");
    if !p.is_null() {
        (*p).sample_shading = enable as u32;
    }
}

// The GLSLC online shader compiler. Shaders are translated ahead of time, so these report failure or do nothing.
#[no_mangle]
pub unsafe extern "C" fn hle_glslcInitialize(_options: *const ()) -> i32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_glslcFinalize() {}

#[no_mangle]
pub unsafe extern "C" fn hle_glslcCompile(_input: *const (), _output: *mut ()) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn hle_glslcCompilePreSpecialized(
    _input: *const (),
    _output: *mut (),
) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn hle_glslcCompileSpecialized(_input: *const (), _output: *mut ()) -> i32 {
    -1
}

#[no_mangle]
pub unsafe extern "C" fn hle_glslcGetVersion() -> u32 {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_glslcSetAllocator(_alloc: *const ()) {}

#[no_mangle]
pub unsafe extern "C" fn hle_glslcGetDefaultOptions(out: *mut u8, len: usize) {
    if !out.is_null() && len > 0 {
        std::ptr::write_bytes(out, 0, len);
    }
}
