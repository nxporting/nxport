// NVN devices: device info, the bootstrap loader, nvnDeviceGetProcAddress, stubs for functions without an
// implementation, and the guest entries handed out.

use super::*;

// Last-resort fallback: only used if the labeled-stub pool below is
// exhausted or a null name slips through. Prefer the labeled pool -- this
// one genuinely can't say which NVN function it's standing in for.
#[no_mangle]
pub unsafe extern "C" fn hle_nvn_generic_stub() -> usize {
    record_hle_call("hle_nvn_generic_stub");
    0
}

// Labeled, nameable unimplemented-NVN-function stubs.
//
// The *resolution* of an unmapped NVN name was already logged
// ("NVN ProcAddress: requested 'X'"), but that only fires once, when the
// game fetches the function pointer -- not each time it's actually called
// afterward, and every unmapped name used to resolve to the exact same
// anonymous hle_nvn_generic_stub, which has no way to know which function
// it's pretending to be. That made tracking an unimplemented-function bug
// down require manual disassembly every time (see the 0xCB2500 investigation
// this session). Fixed properly: each unmapped name gets its own slot from
// this pool of distinct, genuinely-nameable functions, so every actual call
// -- not just the resolution -- prints its real NVN name, a running call
// count, its first two arguments, and the guest return address (so the call
// site is directly addressable without hunting for it).
pub(crate) const NVN_LABELED_STUB_POOL_SIZE: usize = 320;

pub(crate) static NVN_STUB_NAMES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn nvn_stub_index_for_name(name: &str) -> Option<usize> {
    let mut names = NVN_STUB_NAMES.lock().unwrap();
    if let Some(pos) = names.iter().position(|n| n == name) {
        return Some(pos);
    }
    if names.len() >= NVN_LABELED_STUB_POOL_SIZE {
        eprintln!("[!] NVN labeled-stub pool exhausted ({} slots); '{}' falls back to the anonymous stub -- raise NVN_LABELED_STUB_POOL_SIZE", NVN_LABELED_STUB_POOL_SIZE, name);
        return None;
    }
    names.push(name.to_string());
    Some(names.len() - 1)
}

pub(crate) fn nvn_stub_name_for_index(idx: usize) -> String {
    NVN_STUB_NAMES
        .lock()
        .unwrap()
        .get(idx)
        .cloned()
        .unwrap_or_else(|| format!("<unregistered slot {}>", idx))
}

pub(crate) unsafe fn nvn_labeled_stub_called(idx: usize, a0: u64, a1: u64) -> u64 {
    static CALL_COUNTS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());
    let n = {
        let mut counts = CALL_COUNTS.lock().unwrap();
        while counts.len() <= idx {
            counts.push(0);
        }
        counts[idx] += 1;
        counts[idx]
    };
    let lr = crate::guest_reg!(x30);
    let name = nvn_stub_name_for_index(idx);
    // Throttled: the same few unimplemented functions can be called hundreds of thousands of times.
    if n <= 3 || n % 5000 == 0 {
        record_hle_call(format!("{} (UNIMPLEMENTED)", name));
        println!(
            "[!] unimplemented NVN function called: '{}' (slot {}, call #{}, arg0=0x{:x}, arg1=0x{:x}, guest_lr=0x{:x})",
            name, idx, n, a0, a1, lr
        );
    }
    0
}

pub(crate) type NvnStubFn = unsafe extern "C" fn(u64, u64) -> u64;

pub(crate) static NVN_LABELED_STUB_TABLE: [NvnStubFn; NVN_LABELED_STUB_POOL_SIZE] = [
    hle_nvn_labeled_stub_0,
    hle_nvn_labeled_stub_1,
    hle_nvn_labeled_stub_2,
    hle_nvn_labeled_stub_3,
    hle_nvn_labeled_stub_4,
    hle_nvn_labeled_stub_5,
    hle_nvn_labeled_stub_6,
    hle_nvn_labeled_stub_7,
    hle_nvn_labeled_stub_8,
    hle_nvn_labeled_stub_9,
    hle_nvn_labeled_stub_10,
    hle_nvn_labeled_stub_11,
    hle_nvn_labeled_stub_12,
    hle_nvn_labeled_stub_13,
    hle_nvn_labeled_stub_14,
    hle_nvn_labeled_stub_15,
    hle_nvn_labeled_stub_16,
    hle_nvn_labeled_stub_17,
    hle_nvn_labeled_stub_18,
    hle_nvn_labeled_stub_19,
    hle_nvn_labeled_stub_20,
    hle_nvn_labeled_stub_21,
    hle_nvn_labeled_stub_22,
    hle_nvn_labeled_stub_23,
    hle_nvn_labeled_stub_24,
    hle_nvn_labeled_stub_25,
    hle_nvn_labeled_stub_26,
    hle_nvn_labeled_stub_27,
    hle_nvn_labeled_stub_28,
    hle_nvn_labeled_stub_29,
    hle_nvn_labeled_stub_30,
    hle_nvn_labeled_stub_31,
    hle_nvn_labeled_stub_32,
    hle_nvn_labeled_stub_33,
    hle_nvn_labeled_stub_34,
    hle_nvn_labeled_stub_35,
    hle_nvn_labeled_stub_36,
    hle_nvn_labeled_stub_37,
    hle_nvn_labeled_stub_38,
    hle_nvn_labeled_stub_39,
    hle_nvn_labeled_stub_40,
    hle_nvn_labeled_stub_41,
    hle_nvn_labeled_stub_42,
    hle_nvn_labeled_stub_43,
    hle_nvn_labeled_stub_44,
    hle_nvn_labeled_stub_45,
    hle_nvn_labeled_stub_46,
    hle_nvn_labeled_stub_47,
    hle_nvn_labeled_stub_48,
    hle_nvn_labeled_stub_49,
    hle_nvn_labeled_stub_50,
    hle_nvn_labeled_stub_51,
    hle_nvn_labeled_stub_52,
    hle_nvn_labeled_stub_53,
    hle_nvn_labeled_stub_54,
    hle_nvn_labeled_stub_55,
    hle_nvn_labeled_stub_56,
    hle_nvn_labeled_stub_57,
    hle_nvn_labeled_stub_58,
    hle_nvn_labeled_stub_59,
    hle_nvn_labeled_stub_60,
    hle_nvn_labeled_stub_61,
    hle_nvn_labeled_stub_62,
    hle_nvn_labeled_stub_63,
    hle_nvn_labeled_stub_64,
    hle_nvn_labeled_stub_65,
    hle_nvn_labeled_stub_66,
    hle_nvn_labeled_stub_67,
    hle_nvn_labeled_stub_68,
    hle_nvn_labeled_stub_69,
    hle_nvn_labeled_stub_70,
    hle_nvn_labeled_stub_71,
    hle_nvn_labeled_stub_72,
    hle_nvn_labeled_stub_73,
    hle_nvn_labeled_stub_74,
    hle_nvn_labeled_stub_75,
    hle_nvn_labeled_stub_76,
    hle_nvn_labeled_stub_77,
    hle_nvn_labeled_stub_78,
    hle_nvn_labeled_stub_79,
    hle_nvn_labeled_stub_80,
    hle_nvn_labeled_stub_81,
    hle_nvn_labeled_stub_82,
    hle_nvn_labeled_stub_83,
    hle_nvn_labeled_stub_84,
    hle_nvn_labeled_stub_85,
    hle_nvn_labeled_stub_86,
    hle_nvn_labeled_stub_87,
    hle_nvn_labeled_stub_88,
    hle_nvn_labeled_stub_89,
    hle_nvn_labeled_stub_90,
    hle_nvn_labeled_stub_91,
    hle_nvn_labeled_stub_92,
    hle_nvn_labeled_stub_93,
    hle_nvn_labeled_stub_94,
    hle_nvn_labeled_stub_95,
    hle_nvn_labeled_stub_96,
    hle_nvn_labeled_stub_97,
    hle_nvn_labeled_stub_98,
    hle_nvn_labeled_stub_99,
    hle_nvn_labeled_stub_100,
    hle_nvn_labeled_stub_101,
    hle_nvn_labeled_stub_102,
    hle_nvn_labeled_stub_103,
    hle_nvn_labeled_stub_104,
    hle_nvn_labeled_stub_105,
    hle_nvn_labeled_stub_106,
    hle_nvn_labeled_stub_107,
    hle_nvn_labeled_stub_108,
    hle_nvn_labeled_stub_109,
    hle_nvn_labeled_stub_110,
    hle_nvn_labeled_stub_111,
    hle_nvn_labeled_stub_112,
    hle_nvn_labeled_stub_113,
    hle_nvn_labeled_stub_114,
    hle_nvn_labeled_stub_115,
    hle_nvn_labeled_stub_116,
    hle_nvn_labeled_stub_117,
    hle_nvn_labeled_stub_118,
    hle_nvn_labeled_stub_119,
    hle_nvn_labeled_stub_120,
    hle_nvn_labeled_stub_121,
    hle_nvn_labeled_stub_122,
    hle_nvn_labeled_stub_123,
    hle_nvn_labeled_stub_124,
    hle_nvn_labeled_stub_125,
    hle_nvn_labeled_stub_126,
    hle_nvn_labeled_stub_127,
    hle_nvn_labeled_stub_128,
    hle_nvn_labeled_stub_129,
    hle_nvn_labeled_stub_130,
    hle_nvn_labeled_stub_131,
    hle_nvn_labeled_stub_132,
    hle_nvn_labeled_stub_133,
    hle_nvn_labeled_stub_134,
    hle_nvn_labeled_stub_135,
    hle_nvn_labeled_stub_136,
    hle_nvn_labeled_stub_137,
    hle_nvn_labeled_stub_138,
    hle_nvn_labeled_stub_139,
    hle_nvn_labeled_stub_140,
    hle_nvn_labeled_stub_141,
    hle_nvn_labeled_stub_142,
    hle_nvn_labeled_stub_143,
    hle_nvn_labeled_stub_144,
    hle_nvn_labeled_stub_145,
    hle_nvn_labeled_stub_146,
    hle_nvn_labeled_stub_147,
    hle_nvn_labeled_stub_148,
    hle_nvn_labeled_stub_149,
    hle_nvn_labeled_stub_150,
    hle_nvn_labeled_stub_151,
    hle_nvn_labeled_stub_152,
    hle_nvn_labeled_stub_153,
    hle_nvn_labeled_stub_154,
    hle_nvn_labeled_stub_155,
    hle_nvn_labeled_stub_156,
    hle_nvn_labeled_stub_157,
    hle_nvn_labeled_stub_158,
    hle_nvn_labeled_stub_159,
    hle_nvn_labeled_stub_160,
    hle_nvn_labeled_stub_161,
    hle_nvn_labeled_stub_162,
    hle_nvn_labeled_stub_163,
    hle_nvn_labeled_stub_164,
    hle_nvn_labeled_stub_165,
    hle_nvn_labeled_stub_166,
    hle_nvn_labeled_stub_167,
    hle_nvn_labeled_stub_168,
    hle_nvn_labeled_stub_169,
    hle_nvn_labeled_stub_170,
    hle_nvn_labeled_stub_171,
    hle_nvn_labeled_stub_172,
    hle_nvn_labeled_stub_173,
    hle_nvn_labeled_stub_174,
    hle_nvn_labeled_stub_175,
    hle_nvn_labeled_stub_176,
    hle_nvn_labeled_stub_177,
    hle_nvn_labeled_stub_178,
    hle_nvn_labeled_stub_179,
    hle_nvn_labeled_stub_180,
    hle_nvn_labeled_stub_181,
    hle_nvn_labeled_stub_182,
    hle_nvn_labeled_stub_183,
    hle_nvn_labeled_stub_184,
    hle_nvn_labeled_stub_185,
    hle_nvn_labeled_stub_186,
    hle_nvn_labeled_stub_187,
    hle_nvn_labeled_stub_188,
    hle_nvn_labeled_stub_189,
    hle_nvn_labeled_stub_190,
    hle_nvn_labeled_stub_191,
    hle_nvn_labeled_stub_192,
    hle_nvn_labeled_stub_193,
    hle_nvn_labeled_stub_194,
    hle_nvn_labeled_stub_195,
    hle_nvn_labeled_stub_196,
    hle_nvn_labeled_stub_197,
    hle_nvn_labeled_stub_198,
    hle_nvn_labeled_stub_199,
    hle_nvn_labeled_stub_200,
    hle_nvn_labeled_stub_201,
    hle_nvn_labeled_stub_202,
    hle_nvn_labeled_stub_203,
    hle_nvn_labeled_stub_204,
    hle_nvn_labeled_stub_205,
    hle_nvn_labeled_stub_206,
    hle_nvn_labeled_stub_207,
    hle_nvn_labeled_stub_208,
    hle_nvn_labeled_stub_209,
    hle_nvn_labeled_stub_210,
    hle_nvn_labeled_stub_211,
    hle_nvn_labeled_stub_212,
    hle_nvn_labeled_stub_213,
    hle_nvn_labeled_stub_214,
    hle_nvn_labeled_stub_215,
    hle_nvn_labeled_stub_216,
    hle_nvn_labeled_stub_217,
    hle_nvn_labeled_stub_218,
    hle_nvn_labeled_stub_219,
    hle_nvn_labeled_stub_220,
    hle_nvn_labeled_stub_221,
    hle_nvn_labeled_stub_222,
    hle_nvn_labeled_stub_223,
    hle_nvn_labeled_stub_224,
    hle_nvn_labeled_stub_225,
    hle_nvn_labeled_stub_226,
    hle_nvn_labeled_stub_227,
    hle_nvn_labeled_stub_228,
    hle_nvn_labeled_stub_229,
    hle_nvn_labeled_stub_230,
    hle_nvn_labeled_stub_231,
    hle_nvn_labeled_stub_232,
    hle_nvn_labeled_stub_233,
    hle_nvn_labeled_stub_234,
    hle_nvn_labeled_stub_235,
    hle_nvn_labeled_stub_236,
    hle_nvn_labeled_stub_237,
    hle_nvn_labeled_stub_238,
    hle_nvn_labeled_stub_239,
    hle_nvn_labeled_stub_240,
    hle_nvn_labeled_stub_241,
    hle_nvn_labeled_stub_242,
    hle_nvn_labeled_stub_243,
    hle_nvn_labeled_stub_244,
    hle_nvn_labeled_stub_245,
    hle_nvn_labeled_stub_246,
    hle_nvn_labeled_stub_247,
    hle_nvn_labeled_stub_248,
    hle_nvn_labeled_stub_249,
    hle_nvn_labeled_stub_250,
    hle_nvn_labeled_stub_251,
    hle_nvn_labeled_stub_252,
    hle_nvn_labeled_stub_253,
    hle_nvn_labeled_stub_254,
    hle_nvn_labeled_stub_255,
    hle_nvn_labeled_stub_256,
    hle_nvn_labeled_stub_257,
    hle_nvn_labeled_stub_258,
    hle_nvn_labeled_stub_259,
    hle_nvn_labeled_stub_260,
    hle_nvn_labeled_stub_261,
    hle_nvn_labeled_stub_262,
    hle_nvn_labeled_stub_263,
    hle_nvn_labeled_stub_264,
    hle_nvn_labeled_stub_265,
    hle_nvn_labeled_stub_266,
    hle_nvn_labeled_stub_267,
    hle_nvn_labeled_stub_268,
    hle_nvn_labeled_stub_269,
    hle_nvn_labeled_stub_270,
    hle_nvn_labeled_stub_271,
    hle_nvn_labeled_stub_272,
    hle_nvn_labeled_stub_273,
    hle_nvn_labeled_stub_274,
    hle_nvn_labeled_stub_275,
    hle_nvn_labeled_stub_276,
    hle_nvn_labeled_stub_277,
    hle_nvn_labeled_stub_278,
    hle_nvn_labeled_stub_279,
    hle_nvn_labeled_stub_280,
    hle_nvn_labeled_stub_281,
    hle_nvn_labeled_stub_282,
    hle_nvn_labeled_stub_283,
    hle_nvn_labeled_stub_284,
    hle_nvn_labeled_stub_285,
    hle_nvn_labeled_stub_286,
    hle_nvn_labeled_stub_287,
    hle_nvn_labeled_stub_288,
    hle_nvn_labeled_stub_289,
    hle_nvn_labeled_stub_290,
    hle_nvn_labeled_stub_291,
    hle_nvn_labeled_stub_292,
    hle_nvn_labeled_stub_293,
    hle_nvn_labeled_stub_294,
    hle_nvn_labeled_stub_295,
    hle_nvn_labeled_stub_296,
    hle_nvn_labeled_stub_297,
    hle_nvn_labeled_stub_298,
    hle_nvn_labeled_stub_299,
    hle_nvn_labeled_stub_300,
    hle_nvn_labeled_stub_301,
    hle_nvn_labeled_stub_302,
    hle_nvn_labeled_stub_303,
    hle_nvn_labeled_stub_304,
    hle_nvn_labeled_stub_305,
    hle_nvn_labeled_stub_306,
    hle_nvn_labeled_stub_307,
    hle_nvn_labeled_stub_308,
    hle_nvn_labeled_stub_309,
    hle_nvn_labeled_stub_310,
    hle_nvn_labeled_stub_311,
    hle_nvn_labeled_stub_312,
    hle_nvn_labeled_stub_313,
    hle_nvn_labeled_stub_314,
    hle_nvn_labeled_stub_315,
    hle_nvn_labeled_stub_316,
    hle_nvn_labeled_stub_317,
    hle_nvn_labeled_stub_318,
    hle_nvn_labeled_stub_319,
];

pub(crate) fn resolve_unimplemented_nvn(name: &str) -> *const () {
    match nvn_stub_index_for_name(name) {
        Some(idx) => crate::guest::exec::hle_abi::hle(NVN_LABELED_STUB_TABLE[idx]),
        None => crate::hle!(hle_nvn_generic_stub()),
    }
}

/// nvnDeviceGetInteger values of the Tegra X1 driver.
pub(crate) fn known_device_info(pname: i32) -> Option<i32> {
    Some(match pname {
        0 => crate::horizon::sdk::nvn_api_version().0, // NVN_API_MAJOR_VERSION
        1 => crate::horizon::sdk::nvn_api_version().1, // NVN_API_MINOR_VERSION
        2 => 14,      // UNIFORM_BUFFER_BINDINGS_PER_STAGE
        3 => 0x10000, // MAX_UNIFORM_BUFFER_SIZE
        5 => 8,       // COLOR_BUFFER_BINDINGS
        6 => 16,      // VERTEX_BUFFER_BINDINGS
        7 => 4,       // TRANSFORM_FEEDBACK_BUFFER_BINDINGS
        8 => 16,      // SHADER_STORAGE_BUFFER_BINDINGS_PER_STAGE
        9 => 32,      // TEXTURE_BINDINGS_PER_STAGE
        14 => 16,     // VERTEX_ATTRIBUTES
        // Descriptor pools. Maxwell texture (TIC) and sampler (TSC) descriptors are 32 bytes each (as in libnx's deko3d).
        // Reserved entries: 256 each (TotK's own handles start at 258 in both pools). Maximum entries: the handle has
        // 20 bits of texture id and 12 of sampler id (nvnDeviceGetTextureHandle). The old flat 256 for all six made
        // Kirby and the Forgotten Land size its pools to 256 entries, all reserved: every texture id it allocated was -1.
        15 | 16 => 32,   // TEXTURE_DESCRIPTOR_SIZE / SAMPLER_DESCRIPTOR_SIZE
        17 | 18 => 256,  // RESERVED_TEXTURE_DESCRIPTORS / RESERVED_SAMPLER_DESCRIPTORS
        36 => 1 << 20,   // MAX_TEXTURE_POOL_SIZE
        37 => 1 << 12,   // MAX_SAMPLER_POOL_SIZE
        27 => 16,        // MAX_TEXTURE_ANISOTROPY
        28 | 31 => 1024, // MAX_COMPUTE_WORK_GROUP_SIZE_X / _THREADS
        29 => 1024,
        30 => 64,
        35 => 8,           // IMAGE_BINDINGS_PER_STAGE
        38 => 16,          // MAX_VIEWPORTS
        40 | 41 | 42 => 1, // SUPPORTS_MIN_MAX_FILTERING / STENCIL8 / ASTC (Tegra X1 has all three)
        43 => 256 * 1024,  // L2_SIZE
        44 => 15,          // MAX_TEXTURE_LEVELS
        45 => 2048,        // MAX_TEXTURE_LAYERS
        // GLSLC GPU-code binary version window (1.16 max; 0.0 min).
        46 => 1,               // MAX_SUPPORTED_GPU_CODE_MAJOR_VERSION
        47 => 0,               // MIN_SUPPORTED_GPU_CODE_MAJOR_VERSION
        48 => 16,              // MAX_SUPPORTED_GPU_CODE_MINOR_VERSION
        49 => 0,               // MIN_SUPPORTED_GPU_CODE_MINOR_VERSION
        61 | 64 | 65 => 16384, // MAX_TEXTURE_SIZE / MAX_CUBE_MAP / MAX_RECTANGLE
        // on NX, virtual pools return zero from unpopulated pages (59), packaged textures may use the sparse tiled
        // form (68) and programmable sample locations exist (76). The other feature flags are second-generation Maxwell
        // features the header lists as unsupported only on older GPUs (passthrough GS 66, viewport swizzle 67, advanced
        // blend 69, draw texture 71, target-independent rasterization 72, coverage to color 73, post-depth coverage 74,
        // images through texture handles 75). The old default (256) was truthy as well.
        59 | 66 | 67 | 68 | 69 | 71 | 72 | 73 | 74 | 75 | 76 => 1,
        87 => 1, // SUPPORTS_FRAGMENT_SHADER_INTERLOCK (Maxwell 2 / GM20B has it; the old 256 default was also truthy)
        90 => 0, // SUPPORTS_DEBUG_LAYER: retail consoles have no debug layer
        _ => return None,
    })
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetInteger(_device: *const (), pname: i32, v: *mut i32) {
    record_hle_call("hle_nvnDeviceGetInteger");
    // Real NVNdeviceInfo values, not guesses. Returning a flat 256
    // for every property regardless of what's being asked (the previous
    // implementation) is exactly the kind of HLE gap that produces silent,
    // hard-to-trace downstream corruption: a wrong MEMORY_POOL_PAGE_SIZE in
    // particular feeds directly into the game's own GPU memory-pool size/
    // alignment math.
    const UNIFORM_BUFFER_ALIGNMENT: i32 = 0x4;
    const COUNTER_ALIGNMENT: i32 = 0xA;
    const TRANSFORM_FEEDBACK_BUFFER_ALIGNMENT: i32 = 0xB;
    const TRANSFORM_FEEDBACK_CONTROL_ALIGNMENT: i32 = 0xC;
    const INDIRECT_DRAW_ALIGNMENT: i32 = 0xD;
    const COMMAND_BUFFER_COMMAND_ALIGNMENT: i32 = 0x13;
    const COMMAND_BUFFER_CONTROL_ALIGNMENT: i32 = 0x14;
    const SHADER_SCRATCH_MEMORY_ALIGNMENT: i32 = 0x19;
    const SHADER_SCRATCH_MEMORY_GRANULARITY: i32 = 0x1A;
    const MEMPOOL_TEXTURE_OBJECT_PAGE_ALIGNMENT: i32 = 0x27;
    const INDIRECT_DISPATCH_ALIGNMENT: i32 = 0x35;
    const ZCULL_SAVE_RESTORE_ALIGNMENT: i32 = 0x36;
    const LINEAR_TEXTURE_STRIDE_ALIGNMENT: i32 = 0x38;
    const LINEAR_RENDER_TARGET_STRIDE_ALIGNMENT: i32 = 0x39;
    const MEMORY_POOL_PAGE_SIZE: i32 = 0x3A;
    const UNIFORM_BUFFER_UPDATE_ALIGNMENT: i32 = 0x3C;

    if !v.is_null() {
        // The game's own driver release first, where its answer is known to differ (horizon::sdk layers).
        if let Some(known) = crate::horizon::sdk::nvn_device_integer(pname) {
            *v = known;
            return;
        }
        *v = match pname {
            MEMORY_POOL_PAGE_SIZE => 0x10000, // 64KB -- Tegra X1 GPU memory pool page size
            MEMPOOL_TEXTURE_OBJECT_PAGE_ALIGNMENT => 0x1000, // 4KB on NX (textures and buffers may not share a 4KB-aligned region)
            UNIFORM_BUFFER_ALIGNMENT | UNIFORM_BUFFER_UPDATE_ALIGNMENT => 0x100,
            COUNTER_ALIGNMENT => 0x10,
            TRANSFORM_FEEDBACK_BUFFER_ALIGNMENT | TRANSFORM_FEEDBACK_CONTROL_ALIGNMENT => 0x100,
            INDIRECT_DRAW_ALIGNMENT | INDIRECT_DISPATCH_ALIGNMENT => 0x4,
            COMMAND_BUFFER_COMMAND_ALIGNMENT | COMMAND_BUFFER_CONTROL_ALIGNMENT => 0x4,
            SHADER_SCRATCH_MEMORY_ALIGNMENT => 0x20,
            SHADER_SCRATCH_MEMORY_GRANULARITY => 0x20000,
            p if known_device_info(p).is_some() => {
                known_device_info(p).unwrap()
            }
            // NVN_DEVICE_INFO_SUPPORTS_CONSERVATIVE_RASTER (second-generation Maxwell or newer; the X1 is one).
            50 => 1,
            ZCULL_SAVE_RESTORE_ALIGNMENT => 0x20,
            LINEAR_TEXTURE_STRIDE_ALIGNMENT => crate::nvn::layout::LINEAR_TEXTURE_STRIDE_ALIGNMENT as i32,
            LINEAR_RENDER_TARGET_STRIDE_ALIGNMENT => crate::nvn::layout::LINEAR_RENDER_TARGET_STRIDE_ALIGNMENT as i32,
            other => match NVN_55_DEVICE_TABLE.get(other as usize) {
                Some(&v) => v as i32,
                None => {
                    println!("[!] nvnDeviceGetInteger: unmapped pname {} (0x{:x}), returning 256 as a generic default", other, other);
                    256
                }
            },
        };
    }
}

/// What the NVN 55.15 driver (BotW 1.8.2's) answers for each device info, as Savage Wind (MIT) read it from the
/// driver's table: the answer for queries nxport has no reasoned value of its own for (above). Indices 23 / 24 are the
/// scratch memory scale factors (minimum, recommended), 85 the default per-queue compute memory size.
pub(crate) const NVN_55_DEVICE_TABLE: [u32; 105] = [
    55, 15, 14, 0x10000, 0x100, 8, 0x10, 4, 0x10, 0x20, // 0-9
    0x10, 4, 4, 4, 0x10, 0x20, 0x20, 0x100, 0x100, 4, // 10-19
    8, 0x200, 0x100, 256, 256, 0x1000, 0x20000, 0x10, 0x600, 0x400, // 20-29
    0x40, 0x600, 0xffff, 0xffff, 0xffff, 8, 0x100000, 0x1000, 0x10, 0x1000, // 30-39
    1, 1, 1, 0x40000, 0xf, 0x800, 1, 1, 0x10, 5, // 40-49
    1, 8, 8, 4, 0x20, 256, 0x20, 0x80, 0x1000, 1, // 50-59
    4, 16384, 0x8000000, 2048, 16384, 16384, 1, 1, 1, 1, // 60-69
    5, 1, 1, 1, 1, 1, 1, 0x10, 0x400, 0x20, // 70-79
    0x1000, 0x10000, 0x10000, 0x100, 0x4000, 0x40000, 0x1000, 1, 4, 2, // 80-89
    0, 0x1000, 0x4000, 0x1000, 0x80, 0x20, 0xffff, 1, 0x20, 0x100, // 90-99
    0x10, 0xffff, 0xffff, 0x100, 0x3fff, // 100-104
];

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetProcAddress(
    _device: *mut (),
    name: *const c_char,
) -> *const () {
    if name.is_null() {
        return guest_entry(crate::hle!(hle_nvn_generic_stub()));
    }
    let func_name = CStr::from_ptr(name).to_string_lossy();
    record_hle_call(format!("nvnDeviceGetProcAddress({})", func_name));
    println!("[+] NVN ProcAddress: requested '{}'", func_name);
    guest_entry(resolve_proc(&func_name))
}

/// The engine function nvnDeviceGetProcAddress hands out for `func_name` (horizon::sdk's NVN tables).
pub fn resolve_proc(func_name: &str) -> *const () {
    crate::horizon::sdk::nvn_proc(func_name).map_or_else(|| resolve_unimplemented_nvn(func_name), |a| a as *const ())
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceBuilderSetDefaults(b: *mut [u8; 0x40]) {
    record_hle_call("hle_nvnDeviceBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceBuilderSetFlags(_b: *mut [u8; 0x40], _flags: i32) {
    record_hle_call("hle_nvnDeviceBuilderSetFlags");
    if !_b.is_null() {
        std::ptr::write_unaligned(_b as *mut i32, _flags);
    } // for nvnDeviceBuilderGetFlags
      // Debug-layer/validation flags only; no functional effect for us.
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceRegisterFastClearColor(
    _device: usize,
    _color: *const f32,
    _format: i32,
) -> u8 {
    record_hle_call("hle_nvnDeviceRegisterFastClearColor");
    // Pure performance hint (lets the driver recognize a fast-clear-eligible
    // color); correctness doesn't depend on it.
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceSetWindowOriginMode(_device: usize, mode: i32) {
    record_hle_call("hle_nvnDeviceSetWindowOriginMode");
    println!(
        "[+] NVN: window origin mode = {} ({})",
        mode,
        if mode == 0 {
            "lower-left"
        } else {
            "upper-left"
        }
    );
    WINDOW_ORIGIN_MODE.store(mode, std::sync::atomic::Ordering::Relaxed);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceSetDepthMode(_device: usize, mode: i32) {
    record_hle_call("hle_nvnDeviceSetDepthMode");
    println!(
        "[+] NVN: depth mode = {} ({})",
        mode,
        if mode == 0 {
            "near is -w"
        } else {
            "near is zero"
        }
    );
    DEPTH_MODE.store(mode, std::sync::atomic::Ordering::Relaxed);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetDepthMode(_device: usize) -> i32 {
    depth_mode()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetWindowOriginMode(_device: usize) -> i32 {
    window_origin_mode()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetCurrentTimestampInNanoseconds(_device: usize) -> u64 {
    // Same clock the executor writes into timestamp counters (metal::write_counter).
    crate::platform::clock::now_ns()
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceBuilderGetFlags(b: *const [u8; 0x40]) -> i32 {
    if b.is_null() {
        0
    } else {
        std::ptr::read_unaligned(b as *const i32)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetTimestampInNanoseconds(
    _device: usize,
    counter: *const u8,
) -> u64 {
    record_hle_call("hle_nvnDeviceGetTimestampInNanoseconds");
    // Converts the timestamp *in the given NVNcounterData* {u64 counter, u64 timestamp}; the executor
    // (metal::write_counter) already writes nanoseconds there.
    if counter.is_null() {
        return 0;
    }
    std::ptr::read_unaligned(counter.add(8) as *const u64)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBootstrapLoader(name: *const c_char) -> *const () {
    record_hle_call("hle_nvnBootstrapLoader");
    if name.is_null() {
        return std::ptr::null();
    }
    let func_name = CStr::from_ptr(name).to_string_lossy();
    println!("[+] NVN Bootstrap: Game requested API '{}'", func_name);
    guest_entry(resolve_bootstrap(&func_name))
}

/// The engine function nvnBootstrapLoader hands out for `func_name`.
pub fn resolve_bootstrap(func_name: &str) -> *const () {
    bootstrap_fn(func_name).unwrap_or_else(|| resolve_unimplemented_nvn(func_name))
}

/// The functions the bootstrap loader serves itself.
pub(crate) fn bootstrap_fn(func_name: &str) -> Option<*const ()> {
    Some(match func_name {
        "nvnDeviceInitialize" => crate::hle!(hle_nvnDeviceInitialize(_, _)),
        "nvnDeviceGetProcAddress" => crate::hle!(hle_nvnDeviceGetProcAddress(_, _)),
        "nvnDeviceGetInteger" => crate::hle!(hle_nvnDeviceGetInteger(_, _, _)),
        "nvnQueueSubmitCommands" => crate::hle!(hle_nvnQueueSubmitCommands(_, _, _)),
        _ => return None,
    })
}

/// HLE trampolines for the functions the two lookups hand out, by function address (`set_guest_entries`).
pub(crate) static GUEST_ENTRIES: std::sync::OnceLock<std::collections::HashMap<usize, usize>> = std::sync::OnceLock::new();

/// Makes a trampoline (`make`) for every NVN function the game can ask for: the names are the `nvn...` strings in its
/// modules (`images`), which is where its lookups take them from. Trampolines cannot be made later (they are part of
/// the AOT image), and without one an NVN call would run on the game's stack buffer (horizon/thread_stack.rs).
pub fn set_guest_entries(images: &[(usize, usize)], make: &mut dyn FnMut(*const ()) -> usize) {
    let mut map = std::collections::HashMap::new();
    // A trampoline per name, not per function address: linkers fold identical functions on some hosts and not on others,
    // and the trampolines must come out the same on every host (the iOS guest image). Only the names no function
    // implements share one, the generic stub's.
    let stub = crate::hle!(hle_nvn_generic_stub());
    let mut seen = std::collections::HashSet::new();
    for &(start, len) in images {
        let bytes = unsafe { std::slice::from_raw_parts(start as *const u8, len) };
        let mut i = 0;
        while let Some(k) = bytes[i..].windows(3).position(|w| w == b"nvn") {
            let at = i + k;
            let end = bytes[at..].iter().position(|b| !b.is_ascii_alphanumeric()).map_or(bytes.len(), |e| at + e);
            i = end.max(at + 3);
            if end >= bytes.len() || bytes[end] != 0 || (at > 0 && bytes[at - 1] != 0) {
                continue;
            }
            let name = std::str::from_utf8(&bytes[at..end]).unwrap_or("");
            if !seen.insert(name.to_string()) {
                continue;
            }
            let (proc, boot) = (resolve_proc(name), bootstrap_fn(name));
            for f in [Some(proc), boot.filter(|&b| b != proc)].into_iter().flatten() {
                if f == stub {
                    map.entry(f as usize).or_insert_with(|| make(f));
                } else {
                    map.insert(f as usize, make(f));
                }
            }
        }
    }
    map.entry(stub as usize).or_insert_with(|| make(stub));
    println!("[+] NVN: {} functions behind HLE trampolines", map.len());
    let _ = GUEST_ENTRIES.set(map);
}

pub(crate) fn guest_entry(f: *const ()) -> *const () {
    match GUEST_ENTRIES.get().and_then(|m| m.get(&(f as usize))) {
        Some(&t) => t as *const (),
        None => {
            eprintln!("[!] NVN: {:p} has no HLE trampoline; it runs on the game's stack", f);
            f
        }
    }
}

pub(crate) unsafe extern "C" fn hle_nvnDeviceInitialize(_device: *mut (), _params: *const ()) -> bool {
    println!("[+] NVN: Initialized virtual Tegra X1 device on Apple Silicon Metal");
    true
}
