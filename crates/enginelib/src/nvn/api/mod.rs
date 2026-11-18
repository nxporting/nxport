// NVN entry points: the device, memory pools, buffers, textures, samplers, programs, command buffers (recorded into
// crate::nvn::cmd lists), queues and windows. Submits and presents go to the GPU backend (crate::gpu::metal).
#![allow(non_snake_case)]

use crate::horizon::hle::{hle_sleep_ns, log_timestamps};
use crate::horizon::call_log::{guest_chain, record_hle_call};
use crate::horizon::os::memory::release_address_region;
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{Mutex as StdMutex, OnceLock};

mod device;
mod memory_pool;
mod command_buffer;
mod buffer;
mod texture;
mod sampler;
mod window;
mod queue;
mod program;
mod pools;
mod sync;
pub(crate) use device::*;
pub(crate) use memory_pool::*;
pub(crate) use command_buffer::*;
pub(crate) use buffer::*;
pub(crate) use texture::*;
pub(crate) use sampler::*;
pub(crate) use window::*;
pub(crate) use queue::*;
pub(crate) use program::*;
pub(crate) use pools::*;
pub(crate) use sync::*;







macro_rules! nvn_labeled_stubs {
    ($($idx:literal => $fn_name:ident),+ $(,)?) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $fn_name(a0: u64, a1: u64) -> u64 {
                nvn_labeled_stub_called($idx, a0, a1)
            }
        )+
    };
}

nvn_labeled_stubs! {
    0 => hle_nvn_labeled_stub_0,
    1 => hle_nvn_labeled_stub_1,
    2 => hle_nvn_labeled_stub_2,
    3 => hle_nvn_labeled_stub_3,
    4 => hle_nvn_labeled_stub_4,
    5 => hle_nvn_labeled_stub_5,
    6 => hle_nvn_labeled_stub_6,
    7 => hle_nvn_labeled_stub_7,
    8 => hle_nvn_labeled_stub_8,
    9 => hle_nvn_labeled_stub_9,
    10 => hle_nvn_labeled_stub_10,
    11 => hle_nvn_labeled_stub_11,
    12 => hle_nvn_labeled_stub_12,
    13 => hle_nvn_labeled_stub_13,
    14 => hle_nvn_labeled_stub_14,
    15 => hle_nvn_labeled_stub_15,
    16 => hle_nvn_labeled_stub_16,
    17 => hle_nvn_labeled_stub_17,
    18 => hle_nvn_labeled_stub_18,
    19 => hle_nvn_labeled_stub_19,
    20 => hle_nvn_labeled_stub_20,
    21 => hle_nvn_labeled_stub_21,
    22 => hle_nvn_labeled_stub_22,
    23 => hle_nvn_labeled_stub_23,
    24 => hle_nvn_labeled_stub_24,
    25 => hle_nvn_labeled_stub_25,
    26 => hle_nvn_labeled_stub_26,
    27 => hle_nvn_labeled_stub_27,
    28 => hle_nvn_labeled_stub_28,
    29 => hle_nvn_labeled_stub_29,
    30 => hle_nvn_labeled_stub_30,
    31 => hle_nvn_labeled_stub_31,
    32 => hle_nvn_labeled_stub_32,
    33 => hle_nvn_labeled_stub_33,
    34 => hle_nvn_labeled_stub_34,
    35 => hle_nvn_labeled_stub_35,
    36 => hle_nvn_labeled_stub_36,
    37 => hle_nvn_labeled_stub_37,
    38 => hle_nvn_labeled_stub_38,
    39 => hle_nvn_labeled_stub_39,
    40 => hle_nvn_labeled_stub_40,
    41 => hle_nvn_labeled_stub_41,
    42 => hle_nvn_labeled_stub_42,
    43 => hle_nvn_labeled_stub_43,
    44 => hle_nvn_labeled_stub_44,
    45 => hle_nvn_labeled_stub_45,
    46 => hle_nvn_labeled_stub_46,
    47 => hle_nvn_labeled_stub_47,
    48 => hle_nvn_labeled_stub_48,
    49 => hle_nvn_labeled_stub_49,
    50 => hle_nvn_labeled_stub_50,
    51 => hle_nvn_labeled_stub_51,
    52 => hle_nvn_labeled_stub_52,
    53 => hle_nvn_labeled_stub_53,
    54 => hle_nvn_labeled_stub_54,
    55 => hle_nvn_labeled_stub_55,
    56 => hle_nvn_labeled_stub_56,
    57 => hle_nvn_labeled_stub_57,
    58 => hle_nvn_labeled_stub_58,
    59 => hle_nvn_labeled_stub_59,
    60 => hle_nvn_labeled_stub_60,
    61 => hle_nvn_labeled_stub_61,
    62 => hle_nvn_labeled_stub_62,
    63 => hle_nvn_labeled_stub_63,
    64 => hle_nvn_labeled_stub_64,
    65 => hle_nvn_labeled_stub_65,
    66 => hle_nvn_labeled_stub_66,
    67 => hle_nvn_labeled_stub_67,
    68 => hle_nvn_labeled_stub_68,
    69 => hle_nvn_labeled_stub_69,
    70 => hle_nvn_labeled_stub_70,
    71 => hle_nvn_labeled_stub_71,
    72 => hle_nvn_labeled_stub_72,
    73 => hle_nvn_labeled_stub_73,
    74 => hle_nvn_labeled_stub_74,
    75 => hle_nvn_labeled_stub_75,
    76 => hle_nvn_labeled_stub_76,
    77 => hle_nvn_labeled_stub_77,
    78 => hle_nvn_labeled_stub_78,
    79 => hle_nvn_labeled_stub_79,
    80 => hle_nvn_labeled_stub_80,
    81 => hle_nvn_labeled_stub_81,
    82 => hle_nvn_labeled_stub_82,
    83 => hle_nvn_labeled_stub_83,
    84 => hle_nvn_labeled_stub_84,
    85 => hle_nvn_labeled_stub_85,
    86 => hle_nvn_labeled_stub_86,
    87 => hle_nvn_labeled_stub_87,
    88 => hle_nvn_labeled_stub_88,
    89 => hle_nvn_labeled_stub_89,
    90 => hle_nvn_labeled_stub_90,
    91 => hle_nvn_labeled_stub_91,
    92 => hle_nvn_labeled_stub_92,
    93 => hle_nvn_labeled_stub_93,
    94 => hle_nvn_labeled_stub_94,
    95 => hle_nvn_labeled_stub_95,
    96 => hle_nvn_labeled_stub_96,
    97 => hle_nvn_labeled_stub_97,
    98 => hle_nvn_labeled_stub_98,
    99 => hle_nvn_labeled_stub_99,
    100 => hle_nvn_labeled_stub_100,
    101 => hle_nvn_labeled_stub_101,
    102 => hle_nvn_labeled_stub_102,
    103 => hle_nvn_labeled_stub_103,
    104 => hle_nvn_labeled_stub_104,
    105 => hle_nvn_labeled_stub_105,
    106 => hle_nvn_labeled_stub_106,
    107 => hle_nvn_labeled_stub_107,
    108 => hle_nvn_labeled_stub_108,
    109 => hle_nvn_labeled_stub_109,
    110 => hle_nvn_labeled_stub_110,
    111 => hle_nvn_labeled_stub_111,
    112 => hle_nvn_labeled_stub_112,
    113 => hle_nvn_labeled_stub_113,
    114 => hle_nvn_labeled_stub_114,
    115 => hle_nvn_labeled_stub_115,
    116 => hle_nvn_labeled_stub_116,
    117 => hle_nvn_labeled_stub_117,
    118 => hle_nvn_labeled_stub_118,
    119 => hle_nvn_labeled_stub_119,
    120 => hle_nvn_labeled_stub_120,
    121 => hle_nvn_labeled_stub_121,
    122 => hle_nvn_labeled_stub_122,
    123 => hle_nvn_labeled_stub_123,
    124 => hle_nvn_labeled_stub_124,
    125 => hle_nvn_labeled_stub_125,
    126 => hle_nvn_labeled_stub_126,
    127 => hle_nvn_labeled_stub_127,
    128 => hle_nvn_labeled_stub_128,
    129 => hle_nvn_labeled_stub_129,
    130 => hle_nvn_labeled_stub_130,
    131 => hle_nvn_labeled_stub_131,
    132 => hle_nvn_labeled_stub_132,
    133 => hle_nvn_labeled_stub_133,
    134 => hle_nvn_labeled_stub_134,
    135 => hle_nvn_labeled_stub_135,
    136 => hle_nvn_labeled_stub_136,
    137 => hle_nvn_labeled_stub_137,
    138 => hle_nvn_labeled_stub_138,
    139 => hle_nvn_labeled_stub_139,
    140 => hle_nvn_labeled_stub_140,
    141 => hle_nvn_labeled_stub_141,
    142 => hle_nvn_labeled_stub_142,
    143 => hle_nvn_labeled_stub_143,
    144 => hle_nvn_labeled_stub_144,
    145 => hle_nvn_labeled_stub_145,
    146 => hle_nvn_labeled_stub_146,
    147 => hle_nvn_labeled_stub_147,
    148 => hle_nvn_labeled_stub_148,
    149 => hle_nvn_labeled_stub_149,
    150 => hle_nvn_labeled_stub_150,
    151 => hle_nvn_labeled_stub_151,
    152 => hle_nvn_labeled_stub_152,
    153 => hle_nvn_labeled_stub_153,
    154 => hle_nvn_labeled_stub_154,
    155 => hle_nvn_labeled_stub_155,
    156 => hle_nvn_labeled_stub_156,
    157 => hle_nvn_labeled_stub_157,
    158 => hle_nvn_labeled_stub_158,
    159 => hle_nvn_labeled_stub_159,
    160 => hle_nvn_labeled_stub_160,
    161 => hle_nvn_labeled_stub_161,
    162 => hle_nvn_labeled_stub_162,
    163 => hle_nvn_labeled_stub_163,
    164 => hle_nvn_labeled_stub_164,
    165 => hle_nvn_labeled_stub_165,
    166 => hle_nvn_labeled_stub_166,
    167 => hle_nvn_labeled_stub_167,
    168 => hle_nvn_labeled_stub_168,
    169 => hle_nvn_labeled_stub_169,
    170 => hle_nvn_labeled_stub_170,
    171 => hle_nvn_labeled_stub_171,
    172 => hle_nvn_labeled_stub_172,
    173 => hle_nvn_labeled_stub_173,
    174 => hle_nvn_labeled_stub_174,
    175 => hle_nvn_labeled_stub_175,
    176 => hle_nvn_labeled_stub_176,
    177 => hle_nvn_labeled_stub_177,
    178 => hle_nvn_labeled_stub_178,
    179 => hle_nvn_labeled_stub_179,
    180 => hle_nvn_labeled_stub_180,
    181 => hle_nvn_labeled_stub_181,
    182 => hle_nvn_labeled_stub_182,
    183 => hle_nvn_labeled_stub_183,
    184 => hle_nvn_labeled_stub_184,
    185 => hle_nvn_labeled_stub_185,
    186 => hle_nvn_labeled_stub_186,
    187 => hle_nvn_labeled_stub_187,
    188 => hle_nvn_labeled_stub_188,
    189 => hle_nvn_labeled_stub_189,
    190 => hle_nvn_labeled_stub_190,
    191 => hle_nvn_labeled_stub_191,
    192 => hle_nvn_labeled_stub_192,
    193 => hle_nvn_labeled_stub_193,
    194 => hle_nvn_labeled_stub_194,
    195 => hle_nvn_labeled_stub_195,
    196 => hle_nvn_labeled_stub_196,
    197 => hle_nvn_labeled_stub_197,
    198 => hle_nvn_labeled_stub_198,
    199 => hle_nvn_labeled_stub_199,
    200 => hle_nvn_labeled_stub_200,
    201 => hle_nvn_labeled_stub_201,
    202 => hle_nvn_labeled_stub_202,
    203 => hle_nvn_labeled_stub_203,
    204 => hle_nvn_labeled_stub_204,
    205 => hle_nvn_labeled_stub_205,
    206 => hle_nvn_labeled_stub_206,
    207 => hle_nvn_labeled_stub_207,
    208 => hle_nvn_labeled_stub_208,
    209 => hle_nvn_labeled_stub_209,
    210 => hle_nvn_labeled_stub_210,
    211 => hle_nvn_labeled_stub_211,
    212 => hle_nvn_labeled_stub_212,
    213 => hle_nvn_labeled_stub_213,
    214 => hle_nvn_labeled_stub_214,
    215 => hle_nvn_labeled_stub_215,
    216 => hle_nvn_labeled_stub_216,
    217 => hle_nvn_labeled_stub_217,
    218 => hle_nvn_labeled_stub_218,
    219 => hle_nvn_labeled_stub_219,
    220 => hle_nvn_labeled_stub_220,
    221 => hle_nvn_labeled_stub_221,
    222 => hle_nvn_labeled_stub_222,
    223 => hle_nvn_labeled_stub_223,
    224 => hle_nvn_labeled_stub_224,
    225 => hle_nvn_labeled_stub_225,
    226 => hle_nvn_labeled_stub_226,
    227 => hle_nvn_labeled_stub_227,
    228 => hle_nvn_labeled_stub_228,
    229 => hle_nvn_labeled_stub_229,
    230 => hle_nvn_labeled_stub_230,
    231 => hle_nvn_labeled_stub_231,
    232 => hle_nvn_labeled_stub_232,
    233 => hle_nvn_labeled_stub_233,
    234 => hle_nvn_labeled_stub_234,
    235 => hle_nvn_labeled_stub_235,
    236 => hle_nvn_labeled_stub_236,
    237 => hle_nvn_labeled_stub_237,
    238 => hle_nvn_labeled_stub_238,
    239 => hle_nvn_labeled_stub_239,
    240 => hle_nvn_labeled_stub_240,
    241 => hle_nvn_labeled_stub_241,
    242 => hle_nvn_labeled_stub_242,
    243 => hle_nvn_labeled_stub_243,
    244 => hle_nvn_labeled_stub_244,
    245 => hle_nvn_labeled_stub_245,
    246 => hle_nvn_labeled_stub_246,
    247 => hle_nvn_labeled_stub_247,
    248 => hle_nvn_labeled_stub_248,
    249 => hle_nvn_labeled_stub_249,
    250 => hle_nvn_labeled_stub_250,
    251 => hle_nvn_labeled_stub_251,
    252 => hle_nvn_labeled_stub_252,
    253 => hle_nvn_labeled_stub_253,
    254 => hle_nvn_labeled_stub_254,
    255 => hle_nvn_labeled_stub_255,
    256 => hle_nvn_labeled_stub_256,
    257 => hle_nvn_labeled_stub_257,
    258 => hle_nvn_labeled_stub_258,
    259 => hle_nvn_labeled_stub_259,
    260 => hle_nvn_labeled_stub_260,
    261 => hle_nvn_labeled_stub_261,
    262 => hle_nvn_labeled_stub_262,
    263 => hle_nvn_labeled_stub_263,
    264 => hle_nvn_labeled_stub_264,
    265 => hle_nvn_labeled_stub_265,
    266 => hle_nvn_labeled_stub_266,
    267 => hle_nvn_labeled_stub_267,
    268 => hle_nvn_labeled_stub_268,
    269 => hle_nvn_labeled_stub_269,
    270 => hle_nvn_labeled_stub_270,
    271 => hle_nvn_labeled_stub_271,
    272 => hle_nvn_labeled_stub_272,
    273 => hle_nvn_labeled_stub_273,
    274 => hle_nvn_labeled_stub_274,
    275 => hle_nvn_labeled_stub_275,
    276 => hle_nvn_labeled_stub_276,
    277 => hle_nvn_labeled_stub_277,
    278 => hle_nvn_labeled_stub_278,
    279 => hle_nvn_labeled_stub_279,
    280 => hle_nvn_labeled_stub_280,
    281 => hle_nvn_labeled_stub_281,
    282 => hle_nvn_labeled_stub_282,
    283 => hle_nvn_labeled_stub_283,
    284 => hle_nvn_labeled_stub_284,
    285 => hle_nvn_labeled_stub_285,
    286 => hle_nvn_labeled_stub_286,
    287 => hle_nvn_labeled_stub_287,
    288 => hle_nvn_labeled_stub_288,
    289 => hle_nvn_labeled_stub_289,
    290 => hle_nvn_labeled_stub_290,
    291 => hle_nvn_labeled_stub_291,
    292 => hle_nvn_labeled_stub_292,
    293 => hle_nvn_labeled_stub_293,
    294 => hle_nvn_labeled_stub_294,
    295 => hle_nvn_labeled_stub_295,
    296 => hle_nvn_labeled_stub_296,
    297 => hle_nvn_labeled_stub_297,
    298 => hle_nvn_labeled_stub_298,
    299 => hle_nvn_labeled_stub_299,
    300 => hle_nvn_labeled_stub_300,
    301 => hle_nvn_labeled_stub_301,
    302 => hle_nvn_labeled_stub_302,
    303 => hle_nvn_labeled_stub_303,
    304 => hle_nvn_labeled_stub_304,
    305 => hle_nvn_labeled_stub_305,
    306 => hle_nvn_labeled_stub_306,
    307 => hle_nvn_labeled_stub_307,
    308 => hle_nvn_labeled_stub_308,
    309 => hle_nvn_labeled_stub_309,
    310 => hle_nvn_labeled_stub_310,
    311 => hle_nvn_labeled_stub_311,
    312 => hle_nvn_labeled_stub_312,
    313 => hle_nvn_labeled_stub_313,
    314 => hle_nvn_labeled_stub_314,
    315 => hle_nvn_labeled_stub_315,
    316 => hle_nvn_labeled_stub_316,
    317 => hle_nvn_labeled_stub_317,
    318 => hle_nvn_labeled_stub_318,
    319 => hle_nvn_labeled_stub_319,
}














thread_local! {
    static POOL_CACHE: std::cell::RefCell<PoolCache> = const { std::cell::RefCell::new(PoolCache { gen: 0, entries: [(0, 0, false); 8], next: 0 }) };
}

















































































thread_local! {
    static CAPTURE: std::cell::RefCell<Option<Capture>> = const { std::cell::RefCell::new(None) };
}



thread_local! {
    /// capture_note writes notes (only while a port is compared with the game).
    static NOTES: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}














macro_rules! nvn_command_recorders {
    ($($fn_name:ident => $nvn_name:literal),+ $(,)?) => {
        $(
            #[no_mangle]
            pub unsafe extern "C" fn $fn_name(cmd: *mut NvnCommandBuffer, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64) {
                record_generic($nvn_name, cmd, a1, a2, a3, a4, a5);
            }
        )+
    };
}

nvn_command_recorders! {
    hle_nvnCommandBufferBarrier => "nvnCommandBufferBarrier",
    hle_nvnCommandBufferBeginTransformFeedback => "nvnCommandBufferBeginTransformFeedback",
    hle_nvnCommandBufferBindCoverageModulationTable => "nvnCommandBufferBindCoverageModulationTable",
    hle_nvnCommandBufferBindImage => "nvnCommandBufferBindImage",
    hle_nvnCommandBufferBindImages => "nvnCommandBufferBindImages",
    hle_nvnCommandBufferBindImagesDeferred => "nvnCommandBufferBindImagesDeferred",
    hle_nvnCommandBufferBindSeparateSampler => "nvnCommandBufferBindSeparateSampler",
    hle_nvnCommandBufferBindSeparateSamplers => "nvnCommandBufferBindSeparateSamplers",
    hle_nvnCommandBufferBindSeparateSamplersDeferred => "nvnCommandBufferBindSeparateSamplersDeferred",
    hle_nvnCommandBufferBindSeparateTexture => "nvnCommandBufferBindSeparateTexture",
    hle_nvnCommandBufferBindSeparateTextures => "nvnCommandBufferBindSeparateTextures",
    hle_nvnCommandBufferBindSeparateTexturesDeferred => "nvnCommandBufferBindSeparateTexturesDeferred",
    hle_nvnCommandBufferBindStorageBuffer => "nvnCommandBufferBindStorageBuffer",
    hle_nvnCommandBufferBindStorageBuffers => "nvnCommandBufferBindStorageBuffers",
    hle_nvnCommandBufferBindStorageBuffersDeferred => "nvnCommandBufferBindStorageBuffersDeferred",
    hle_nvnCommandBufferBindTextures => "nvnCommandBufferBindTextures",
    hle_nvnCommandBufferBindTexturesDeferred => "nvnCommandBufferBindTexturesDeferred",
    hle_nvnCommandBufferBindTransformFeedbackBuffer => "nvnCommandBufferBindTransformFeedbackBuffer",
    hle_nvnCommandBufferBindTransformFeedbackBuffers => "nvnCommandBufferBindTransformFeedbackBuffers",
    hle_nvnCommandBufferBindUniformBuffers => "nvnCommandBufferBindUniformBuffers",
    hle_nvnCommandBufferBindUniformBuffersDeferred => "nvnCommandBufferBindUniformBuffersDeferred",
    hle_nvnCommandBufferBindVertexBuffers => "nvnCommandBufferBindVertexBuffers",
    hle_nvnCommandBufferClearBuffer => "nvnCommandBufferClearBuffer",
    hle_nvnCommandBufferClearColori => "nvnCommandBufferClearColori",
    hle_nvnCommandBufferClearColorui => "nvnCommandBufferClearColorui",
    hle_nvnCommandBufferClearTexture => "nvnCommandBufferClearTexture",
    hle_nvnCommandBufferClearTexturei => "nvnCommandBufferClearTexturei",
    hle_nvnCommandBufferClearTextureui => "nvnCommandBufferClearTextureui",
    hle_nvnCommandBufferCopyBufferToBuffer => "nvnCommandBufferCopyBufferToBuffer",
    hle_nvnCommandBufferCopyBufferToTexture => "nvnCommandBufferCopyBufferToTexture",
    hle_nvnCommandBufferCopyCommands => "nvnCommandBufferCopyCommands",
    hle_nvnCommandBufferCopyTextureToBuffer => "nvnCommandBufferCopyTextureToBuffer",
    hle_nvnCommandBufferCopyTextureToTexture => "nvnCommandBufferCopyTextureToTexture",
    hle_nvnCommandBufferDiscardColor => "nvnCommandBufferDiscardColor",
    hle_nvnCommandBufferDiscardDepthStencil => "nvnCommandBufferDiscardDepthStencil",
    hle_nvnCommandBufferDispatchCompute => "nvnCommandBufferDispatchCompute",
    hle_nvnCommandBufferDispatchComputeIndirect => "nvnCommandBufferDispatchComputeIndirect",
    hle_nvnCommandBufferDownsample => "nvnCommandBufferDownsample",
    hle_nvnCommandBufferDownsampleTextureView => "nvnCommandBufferDownsampleTextureView",
    hle_nvnCommandBufferDrawArraysIndirect => "nvnCommandBufferDrawArraysIndirect",
    hle_nvnCommandBufferDrawArraysInstanced => "nvnCommandBufferDrawArraysInstanced",
    hle_nvnCommandBufferDrawElementsIndirect => "nvnCommandBufferDrawElementsIndirect",
    hle_nvnCommandBufferDrawElementsInstanced => "nvnCommandBufferDrawElementsInstanced",
    hle_nvnCommandBufferDrawTexture => "nvnCommandBufferDrawTexture",
    hle_nvnCommandBufferDrawTransformFeedback => "nvnCommandBufferDrawTransformFeedback",
    hle_nvnCommandBufferEndTransformFeedback => "nvnCommandBufferEndTransformFeedback",
    hle_nvnCommandBufferFenceSync => "nvnCommandBufferFenceSync",
    hle_nvnCommandBufferInsertCheckpointDynamic => "nvnCommandBufferInsertCheckpointDynamic",
    hle_nvnCommandBufferInsertCheckpointStatic => "nvnCommandBufferInsertCheckpointStatic",
    hle_nvnCommandBufferInsertDebugMarker => "nvnCommandBufferInsertDebugMarker",
    hle_nvnCommandBufferInsertDebugMarkerDynamic => "nvnCommandBufferInsertDebugMarkerDynamic",
    hle_nvnCommandBufferInsertDebugMarkerStatic => "nvnCommandBufferInsertDebugMarkerStatic",
    hle_nvnCommandBufferMultiDrawArraysIndirectCount => "nvnCommandBufferMultiDrawArraysIndirectCount",
    hle_nvnCommandBufferMultiDrawElementsIndirectCount => "nvnCommandBufferMultiDrawElementsIndirectCount",
    hle_nvnCommandBufferOverrideSubtileSize => "nvnCommandBufferOverrideSubtileSize",
    hle_nvnCommandBufferPauseTransformFeedback => "nvnCommandBufferPauseTransformFeedback",
    hle_nvnCommandBufferPopDebugGroup => "nvnCommandBufferPopDebugGroup",
    hle_nvnCommandBufferPopDebugGroupId => "nvnCommandBufferPopDebugGroupId",
    hle_nvnCommandBufferPushDebugGroup => "nvnCommandBufferPushDebugGroup",
    hle_nvnCommandBufferPushDebugGroupDynamic => "nvnCommandBufferPushDebugGroupDynamic",
    hle_nvnCommandBufferPushDebugGroupStatic => "nvnCommandBufferPushDebugGroupStatic",
    hle_nvnCommandBufferReportCounter => "nvnCommandBufferReportCounter",
    hle_nvnCommandBufferReportValue => "nvnCommandBufferReportValue",
    hle_nvnCommandBufferResetCounter => "nvnCommandBufferResetCounter",
    hle_nvnCommandBufferResolveDepthBuffer => "nvnCommandBufferResolveDepthBuffer",
    hle_nvnCommandBufferRestoreZCullData => "nvnCommandBufferRestoreZCullData",
    hle_nvnCommandBufferResumeTransformFeedback => "nvnCommandBufferResumeTransformFeedback",
    hle_nvnCommandBufferSaveZCullData => "nvnCommandBufferSaveZCullData",
    hle_nvnCommandBufferSetColorReductionEnable => "nvnCommandBufferSetColorReductionEnable",
    hle_nvnCommandBufferSetColorReductionThresholds => "nvnCommandBufferSetColorReductionThresholds",
    hle_nvnCommandBufferSetCommandMemoryCallbackEnabled => "nvnCommandBufferSetCommandMemoryCallbackEnabled",
    hle_nvnCommandBufferSetConservativeRasterDilate => "nvnCommandBufferSetConservativeRasterDilate",
    hle_nvnCommandBufferSetConservativeRasterEnable => "nvnCommandBufferSetConservativeRasterEnable",
    hle_nvnCommandBufferSetControlMemoryCallbackEnabled => "nvnCommandBufferSetControlMemoryCallbackEnabled",
    hle_nvnCommandBufferSetCopyImageStride => "nvnCommandBufferSetCopyImageStride",
    hle_nvnCommandBufferSetCopyRowStride => "nvnCommandBufferSetCopyRowStride",
    hle_nvnCommandBufferSetDebugLabel => "nvnCommandBufferSetDebugLabel",
    hle_nvnCommandBufferSetDepthBounds => "nvnCommandBufferSetDepthBounds",
    hle_nvnCommandBufferSetInnerTessellationLevels => "nvnCommandBufferSetInnerTessellationLevels",
    hle_nvnCommandBufferSetMemoryCallback => "nvnCommandBufferSetMemoryCallback",
    hle_nvnCommandBufferSetMemoryCallbackData => "nvnCommandBufferSetMemoryCallbackData",
    hle_nvnCommandBufferSetOuterTessellationLevels => "nvnCommandBufferSetOuterTessellationLevels",
    hle_nvnCommandBufferSetPatchSize => "nvnCommandBufferSetPatchSize",
    hle_nvnCommandBufferSetPointSize => "nvnCommandBufferSetPointSize",
    hle_nvnCommandBufferSetPolygonOffsetClamp => "nvnCommandBufferSetPolygonOffsetClamp",
    hle_nvnCommandBufferSetProgramSubroutines => "nvnCommandBufferSetProgramSubroutines",
    hle_nvnCommandBufferSetProvokingVertexMode => "nvnCommandBufferSetProvokingVertexMode",
    hle_nvnCommandBufferSetRenderEnable => "nvnCommandBufferSetRenderEnable",
    hle_nvnCommandBufferSetRenderEnableConditional => "nvnCommandBufferSetRenderEnableConditional",
    hle_nvnCommandBufferSetSampleMask => "nvnCommandBufferSetSampleMask",
    hle_nvnCommandBufferSetShaderScratchMemory => "nvnCommandBufferSetShaderScratchMemory",
    hle_nvnCommandBufferSetStencilCullCriteria => "nvnCommandBufferSetStencilCullCriteria",
    hle_nvnCommandBufferSetStencilMask => "nvnCommandBufferSetStencilMask",
    hle_nvnCommandBufferSetStencilRef => "nvnCommandBufferSetStencilRef",
    hle_nvnCommandBufferSetStencilValueMask => "nvnCommandBufferSetStencilValueMask",
    hle_nvnCommandBufferSetSubpixelPrecisionBias => "nvnCommandBufferSetSubpixelPrecisionBias",
    hle_nvnCommandBufferSetTiledCacheAction => "nvnCommandBufferSetTiledCacheAction",
    hle_nvnCommandBufferSetTiledCacheTileSize => "nvnCommandBufferSetTiledCacheTileSize",
    hle_nvnCommandBufferSetViewportSwizzles => "nvnCommandBufferSetViewportSwizzles",
    hle_nvnCommandBufferSetZCullZF32CompressionEnable => "nvnCommandBufferSetZCullZF32CompressionEnable",
    hle_nvnCommandBufferSignalEvent => "nvnCommandBufferSignalEvent",
    hle_nvnCommandBufferTiledDownsample => "nvnCommandBufferTiledDownsample",
    hle_nvnCommandBufferTiledDownsampleTextureView => "nvnCommandBufferTiledDownsampleTextureView",
    hle_nvnCommandBufferWaitEvent => "nvnCommandBufferWaitEvent",
    hle_nvnCommandBufferWaitSync => "nvnCommandBufferWaitSync",
}








const _: () = assert!(std::mem::size_of::<NvnTextureBuilder>() <= 0x80);


const _: () = assert!(std::mem::size_of::<NvnTexture>() == 0x80);






























const _: () = assert!(std::mem::size_of::<NvnTextureView>() == 0x10);



const _: () = assert!(std::mem::size_of::<NvnSamplerBuilder>() == 0x40);

const _: () = assert!(std::mem::size_of::<NvnSampler>() <= 0x40);











#[no_mangle]
pub unsafe extern "C" fn hle_nvn_noop_setter(_obj: usize, _a: usize, _b: usize) {}

/// The Get*MemorySize / GetCommandFlushThreshold getters return whether the value was set (nonzero here).
unsafe fn queue_size_out(v: usize, out: *mut i32) -> u8 {
    if !out.is_null() {
        *out = v as i32;
    }
    (v != 0) as u8
}

const _: () = assert!(std::mem::size_of::<NvnProgram>() == 0x80);

















const fn pool_slot_key(pool: u32, id: i32) -> i32 {
    ((pool << 20) as i32) | id
}














































const _: () = assert!(std::mem::size_of::<NvnQueueBuilder>() <= 0x40);













// Every HLE struct is written over a guest-allocated NVN object; it must fit inside the real object (NVN object sizes),
// or writing it would overwrite whatever the game placed next to it.
const _: () = {
    use std::mem::size_of;
    assert!(size_of::<NvnMemoryPoolBuilder>() <= 64);
    assert!(size_of::<NvnMemoryPool>() <= 256);
    assert!(size_of::<NvnCommandBuffer>() <= 160);
    assert!(size_of::<NvnBufferBuilder>() <= 64);
    assert!(size_of::<NvnBuffer>() <= 48);
    assert!(size_of::<NvnTextureBuilder>() <= 128);
    assert!(size_of::<NvnTexture>() <= 128);
    assert!(size_of::<NvnTextureView>() <= 16);
    assert!(size_of::<NvnSamplerBuilder>() <= 64);
    assert!(size_of::<NvnSampler>() <= 64);
    assert!(size_of::<NvnProgram>() <= 128);
    assert!(size_of::<NvnTexturePool>() <= 32);
    assert!(size_of::<NvnSamplerPool>() <= 32);
    assert!(size_of::<NvnSync>() <= 64);
    assert!(size_of::<NvnEvent>() <= 64);
    assert!(size_of::<NvnWindowBuilder>() <= 64);
    assert!(size_of::<NvnWindow>() <= 384);
    assert!(size_of::<NvnQueueBuilder>() <= 64);
    assert!(size_of::<NvnQueue>() <= 8192);
};

/// Sizes of HLE tables that could grow over a session (--log-sizes).
pub fn debug_table_sizes() -> String {
    format!("snapshots {} by_storage {} pool_textures {} command_lists {} location_handles {} cmd_memory_lists {} dirty_textures {} evicted_class {}",
        TEX_SNAPSHOTS.lock().unwrap().as_ref().map_or(0, |m| m.len()),
        TEX_BY_STORAGE.lock().unwrap().as_ref().map_or(0, |m| m.len()),
        POOL_TEXTURES.lock().unwrap().as_ref().map_or(0, |m| m.len()),
        COMMAND_LISTS.lock().unwrap().len(),
        LOCATION_HANDLES.lock().unwrap().len(),
        COMMAND_MEMORY.lock().unwrap().lists.len(),
        DIRTY_TEXTURES.lock().unwrap().len(),
        EVICTED_CLASS.lock().unwrap().len())
}

/// The frontend state the backend consults while it runs recorded commands (frame captures, capture/format.rs).
pub(crate) fn export_frontend() -> crate::capture::format::Frontend {
    use crate::capture::format::{pod_bytes, PoolTexture};
    use std::sync::atomic::Ordering::Relaxed;
    let flags = MEMORY_POOL_FLAGS.lock().unwrap().clone();
    let pools = MEMORY_POOLS
        .lock()
        .unwrap()
        .iter()
        .map(|(&b, &s)| (b as u64, s as u64, flags.get(&b).copied().unwrap_or(0)))
        .collect();
    let virtual_maps = VIRTUAL_MAPS.lock().unwrap().iter().map(|&(v, s, p)| (v as u64, s as u64, p as u64)).collect();
    // Stand-in copies (resolve_texture_ref, lookup_pool_texture) are host objects of this process: a replay makes its own.
    let mut copies: std::collections::HashSet<usize> = Default::default();
    let mut history = Vec::new();
    if let Some(h) = TEX_HISTORY.lock().unwrap().as_ref() {
        copies.extend(h.copies.values().map(|b| &**b as *const NvnTexture as usize));
        history = h.order.iter().filter_map(|g| h.by_gen.get(g).map(|t| (*g, pod_bytes(t)))).collect();
    }
    let pool_textures: Vec<PoolTexture> = POOL_TEXTURES
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| {
            m.iter()
                .map(|(&id, s)| {
                    if let Some(c) = &s.copy {
                        copies.insert(&**c as *const NvnTexture as usize);
                    }
                    PoolTexture {
                        id,
                        obj: s.obj as u64,
                        gen: s.gen,
                        state: pod_bytes(&s.state),
                        view: s.view.as_ref().map(pod_bytes),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    let textures = TEX_SNAPSHOTS
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| {
            m.iter()
                .filter(|(a, _)| !copies.contains(a))
                .map(|(&a, (g, t))| (a as u64, *g, pod_bytes(t)))
                .collect()
        })
        .unwrap_or_default();
    let indexed = TEX_BY_STORAGE
        .lock()
        .unwrap()
        .as_ref()
        .map(|m| m.values().flatten().map(|&a| a as u64).collect())
        .unwrap_or_default();
    let pool_samplers = POOL_SAMPLERS
        .get()
        .map(|m| m.lock().unwrap().iter().map(|(&id, d)| (id, *d)).collect())
        .unwrap_or_default();
    let programs = PROGRAM_STAGES.lock().unwrap().iter().map(|(&p, v)| (p as u64, v.clone())).collect();
    crate::capture::format::Frontend {
        pools,
        virtual_maps,
        textures,
        indexed,
        history,
        next_gen: TEX_GENERATION.load(Relaxed),
        max_texture_span: MAX_TEXTURE_SPAN.load(Relaxed) as u64,
        pool_textures,
        pool_samplers,
        programs,
        window_origin_mode: window_origin_mode(),
        depth_mode: depth_mode(),
    }
}

/// Installs exported frontend state (a capture replay, before anything is submitted); hands the programs to the backend.
pub(crate) fn import_frontend(f: &crate::capture::format::Frontend) {
    use crate::capture::format::from_pod;
    use std::sync::atomic::Ordering::Relaxed;
    {
        let mut pools = MEMORY_POOLS.lock().unwrap();
        let mut flags = MEMORY_POOL_FLAGS.lock().unwrap();
        for &(b, s, fl) in &f.pools {
            pools.insert(b as usize, s as usize);
            flags.insert(b as usize, fl);
        }
    }
    *VIRTUAL_MAPS.lock().unwrap() = f.virtual_maps.iter().map(|&(v, s, p)| (v as usize, s as usize, p as usize)).collect();
    VIRTUAL_MAPS_EMPTY.store(f.virtual_maps.is_empty(), std::sync::atomic::Ordering::Release);
    bump_pools_gen();
    *TEX_SNAPSHOTS.lock().unwrap() = Some(
        f.textures.iter().map(|(a, g, t)| (*a as usize, (*g, from_pod::<NvnTexture>(t)))).collect(),
    );
    {
        let mut idx: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
        let mut large: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
        let snaps = TEX_SNAPSHOTS.lock().unwrap();
        for &a in &f.indexed {
            if let Some((_, t)) = snaps.as_ref().and_then(|m| m.get(&(a as usize))) {
                idx.entry(t.storage).or_default().push(a as usize);
                if texture_span(t) > SMALL_TEXTURE_SPAN {
                    large.entry(t.storage).or_default().push(a as usize);
                }
            }
        }
        *TEX_BY_STORAGE.lock().unwrap() = Some(idx);
        *TEX_LARGE_BY_STORAGE.lock().unwrap() = Some(large);
    }
    *TEX_HISTORY.lock().unwrap() = Some(SnapshotHistory {
        by_gen: f.history.iter().map(|(g, t)| (*g, from_pod::<NvnTexture>(t))).collect(),
        order: f.history.iter().map(|(g, _)| *g).collect(),
        copies: Default::default(),
    });
    TEX_GENERATION.fetch_max(f.next_gen, Relaxed);
    MAX_TEXTURE_SPAN.fetch_max(f.max_texture_span as usize, Relaxed);
    *POOL_TEXTURES.lock().unwrap() = Some(
        f.pool_textures
            .iter()
            .map(|p| {
                (
                    p.id,
                    PoolSlot {
                        obj: p.obj as usize,
                        gen: p.gen,
                        state: from_pod(&p.state),
                        view: p.view.as_ref().map(|v| from_pod(v)),
                        copy: None,
                    },
                )
            })
            .collect(),
    );
    log_pool_change(-1);
    bump_tex_table_epoch();
    for &(id, d) in &f.pool_samplers {
        register_pool_sampler(id, d);
    }
    WINDOW_ORIGIN_MODE.store(f.window_origin_mode, Relaxed);
    DEPTH_MODE.store(f.depth_mode, Relaxed);
    for (p, stages) in &f.programs {
        let list: Vec<crate::gpu::ShaderStage> =
            stages.iter().map(|&(data, control)| crate::gpu::ShaderStage { data, control }).collect();
        PROGRAM_STAGES.lock().unwrap().insert(*p as usize, stages.clone());
        crate::gpu::backend().set_program(*p as usize, &list);
    }
}

/// Every command list reachable from `roots` (through CallCommands), for a capture.
pub(crate) fn export_lists(roots: &[u64]) -> Vec<(u64, Vec<crate::capture::format::Op>)> {
    let map = COMMAND_LISTS.lock().unwrap();
    let mut seen: std::collections::HashSet<u64> = Default::default();
    let mut stack: Vec<u64> = roots.to_vec();
    let mut out = Vec::new();
    while let Some(h) = stack.pop() {
        if !seen.insert(h) {
            continue;
        }
        let Some(e) = map.get(&h) else { continue };
        stack.extend_from_slice(&e.0.summary().1);
        out.push((
            h,
            e.0.ops
                .iter()
                .map(|op| crate::capture::format::Op {
                    name: op.name.to_string(),
                    typed: op.typed.clone(),
                    args: op.args.as_deref().copied(),
                })
                .collect(),
        ));
    }
    out
}

/// Installs a command list under `handle` (capture replays); it is never evicted as dead or one-shot.
pub(crate) fn insert_command_list(handle: u64, ops: Vec<crate::capture::format::Op>) {
    static NAMES: std::sync::LazyLock<StdMutex<std::collections::HashMap<String, &'static str>>> =
        std::sync::LazyLock::new(Default::default);
    let ops: Vec<RecordedNvnCommand> = ops
        .into_iter()
        .map(|op| {
            let name = *NAMES.lock().unwrap().entry(op.name.clone()).or_insert_with(|| Box::leak(op.name.into_boxed_str()));
            RecordedNvnCommand { name, typed: op.typed, args: op.args.map(Box::new) }
        })
        .collect();
    let list = std::sync::Arc::new(RecordedCommandList {
        ops,
        summary: std::sync::OnceLock::new(),
        pass_events: std::sync::OnceLock::new(),
        recorder: None,
        recorded_frame: 0,
    });
    let bytes = recorded_list_bytes(&list);
    let calls = list.summary().1.clone().into_boxed_slice();
    COMMAND_LISTS.lock().unwrap().insert(handle, (list, 0, LIST_CALLED, bytes, calls));
    COMMAND_LIST_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
    NEXT_COMMAND_HANDLE.fetch_max(handle + 1, std::sync::atomic::Ordering::Relaxed);
}

