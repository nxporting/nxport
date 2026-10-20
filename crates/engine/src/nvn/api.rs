// NVN entry points: the device, memory pools, buffers, textures, samplers, programs, command buffers (recorded into
// crate::nvn::cmd lists), queues and windows. Submits and presents go to the GPU backend (crate::metal).
#![allow(non_snake_case)]

use crate::horizon::hle::{
    guest_chain, hle_sleep_ns, log_timestamps, record_hle_call, release_address_region,
};
use std::ffi::CStr;
use std::os::raw::c_char;
use std::sync::{Mutex as StdMutex, OnceLock};

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
const NVN_LABELED_STUB_POOL_SIZE: usize = 320;

static NVN_STUB_NAMES: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn nvn_stub_index_for_name(name: &str) -> Option<usize> {
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

fn nvn_stub_name_for_index(idx: usize) -> String {
    NVN_STUB_NAMES
        .lock()
        .unwrap()
        .get(idx)
        .cloned()
        .unwrap_or_else(|| format!("<unregistered slot {}>", idx))
}

unsafe fn nvn_labeled_stub_called(idx: usize, a0: u64, a1: u64) -> u64 {
    static CALL_COUNTS: std::sync::Mutex<Vec<u64>> = std::sync::Mutex::new(Vec::new());
    let n = {
        let mut counts = CALL_COUNTS.lock().unwrap();
        while counts.len() <= idx {
            counts.push(0);
        }
        counts[idx] += 1;
        counts[idx]
    };
    let lr: usize;
    std::arch::asm!("mov {}, x30", out(reg) lr);
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

type NvnStubFn = unsafe extern "C" fn(u64, u64) -> u64;

static NVN_LABELED_STUB_TABLE: [NvnStubFn; NVN_LABELED_STUB_POOL_SIZE] = [
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

fn resolve_unimplemented_nvn(name: &str) -> *const () {
    match nvn_stub_index_for_name(name) {
        Some(idx) => NVN_LABELED_STUB_TABLE[idx] as *const (),
        None => hle_nvn_generic_stub as *const (),
    }
}

/// nvnDeviceGetInteger values of the Tegra X1 driver.
fn known_device_info(pname: i32) -> Option<i32> {
    Some(match pname {
        0 => if crate::profile::is_smo() { 53 } else { 55 },      // NVN_API_MAJOR_VERSION
        1 => if crate::profile::is_smo() { 312 } else { 15 },     // NVN_API_MINOR_VERSION
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
            LINEAR_TEXTURE_STRIDE_ALIGNMENT | LINEAR_RENDER_TARGET_STRIDE_ALIGNMENT => 0x20,
            other => {
                println!("[!] nvnDeviceGetInteger: unmapped pname {} (0x{:x}), returning 256 as a generic default", other, other);
                256
            }
        };
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnDeviceGetProcAddress(
    _device: *mut (),
    name: *const c_char,
) -> *const () {
    if name.is_null() {
        return guest_entry(hle_nvn_generic_stub as *const ());
    }
    let func_name = CStr::from_ptr(name).to_string_lossy();
    record_hle_call(format!("nvnDeviceGetProcAddress({})", func_name));
    println!("[+] NVN ProcAddress: requested '{}'", func_name);
    guest_entry(resolve_proc(&func_name))
}

/// The engine function nvnDeviceGetProcAddress hands out for `func_name`.
pub fn resolve_proc(func_name: &str) -> *const () {
    if let Some(addr) =
        crate::nvn::state::lookup(func_name).or_else(|| crate::nvn::cmd::lookup(func_name))
    {
        return addr as *const ();
    }

    match func_name {
        "nvnDeviceInitialize" => hle_nvnDeviceInitialize as *const (),
        "nvnDeviceGetInteger" => hle_nvnDeviceGetInteger as *const (),
        "nvnQueueSubmitCommands" => hle_nvnQueueSubmitCommands as *const (),
        "nvnMemoryPoolBuilderSetDefaults" => hle_nvnMemoryPoolBuilderSetDefaults as *const (),
        "nvnMemoryPoolBuilderSetDevice" => hle_nvnMemoryPoolBuilderSetDevice as *const (),
        "nvnMemoryPoolBuilderSetStorage" => hle_nvnMemoryPoolBuilderSetStorage as *const (),
        "nvnMemoryPoolBuilderSetFlags" => hle_nvnMemoryPoolBuilderSetFlags as *const (),
        "nvnMemoryPoolBuilderGetDevice" => hle_nvnMemoryPoolBuilderGetDevice as *const (),
        "nvnMemoryPoolBuilderGetMemory" => hle_nvnMemoryPoolBuilderGetMemory as *const (),
        "nvnMemoryPoolBuilderGetSize" => hle_nvnMemoryPoolBuilderGetSize as *const (),
        "nvnMemoryPoolBuilderGetFlags" => hle_nvnMemoryPoolBuilderGetFlags as *const (),
        "nvnMemoryPoolInitialize" => hle_nvnMemoryPoolInitialize as *const (),
        "nvnMemoryPoolFinalize" => hle_nvnMemoryPoolFinalize as *const (),
        "nvnMemoryPoolMap" => hle_nvnMemoryPoolMap as *const (),
        "nvnMemoryPoolGetBufferAddress" => hle_nvnMemoryPoolGetBufferAddress as *const (),
        "nvnMemoryPoolSetDebugLabel" => hle_nvnMemoryPoolSetDebugLabel as *const (),
        "nvnMemoryPoolFlushMappedRange" => hle_nvnMemoryPoolFlushMappedRange as *const (),
        "nvnMemoryPoolInvalidateMappedRange" => hle_nvnMemoryPoolInvalidateMappedRange as *const (),
        "nvnCommandBufferInitialize" => hle_nvnCommandBufferInitialize as *const (),
        "nvnCommandBufferFinalize" => hle_nvnCommandBufferFinalize as *const (),
        "nvnCommandBufferAddCommandMemory" => hle_nvnCommandBufferAddCommandMemory as *const (),
        "nvnCommandBufferAddControlMemory" => hle_nvnCommandBufferAddControlMemory as *const (),
        "nvnCommandBufferBeginRecording" => hle_nvnCommandBufferBeginRecording as *const (),
        "nvnCommandBufferEndRecording" => hle_nvnCommandBufferEndRecording as *const (),
        "nvnCommandBufferIsRecording" => hle_nvnCommandBufferIsRecording as *const (),
        "nvnCommandBufferGetCommandMemoryUsed" => {
            hle_nvnCommandBufferGetCommandMemoryUsed as *const ()
        }
        "nvnCommandBufferGetCommandMemoryFree" => {
            hle_nvnCommandBufferGetCommandMemoryFree as *const ()
        }
        "nvnCommandBufferGetCommandMemorySize" => {
            hle_nvnCommandBufferGetCommandMemorySize as *const ()
        }
        "nvnCommandBufferGetControlMemoryUsed" => {
            hle_nvnCommandBufferGetControlMemoryUsed as *const ()
        }
        "nvnCommandBufferGetControlMemoryFree" => {
            hle_nvnCommandBufferGetControlMemoryFree as *const ()
        }
        "nvnCommandBufferGetControlMemorySize" => {
            hle_nvnCommandBufferGetControlMemorySize as *const ()
        }
        "nvnCommandBufferGetMemoryCallback" => hle_nvnCommandBufferGetMemoryCallback as *const (),
        "nvnCommandBufferGetMemoryCallbackData" => {
            hle_nvnCommandBufferGetMemoryCallbackData as *const ()
        }
        "nvnCommandBufferGetCopyRowStride" => hle_nvnCommandBufferGetCopyRowStride as *const (),
        "nvnCommandBufferGetCopyImageStride" => hle_nvnCommandBufferGetCopyImageStride as *const (),
        "nvnCommandBufferBarrier" => hle_nvnCommandBufferBarrier as *const (),
        "nvnCommandBufferBeginTransformFeedback" => {
            hle_nvnCommandBufferBeginTransformFeedback as *const ()
        }
        "nvnCommandBufferBindCoverageModulationTable" => {
            hle_nvnCommandBufferBindCoverageModulationTable as *const ()
        }
        "nvnCommandBufferBindImage" => hle_nvnCommandBufferBindImage as *const (),
        "nvnCommandBufferBindImages" => hle_nvnCommandBufferBindImages as *const (),
        "nvnCommandBufferBindImagesDeferred" => hle_nvnCommandBufferBindImagesDeferred as *const (),
        "nvnCommandBufferBindSeparateSampler" => {
            hle_nvnCommandBufferBindSeparateSampler as *const ()
        }
        "nvnCommandBufferBindSeparateSamplers" => {
            hle_nvnCommandBufferBindSeparateSamplers as *const ()
        }
        "nvnCommandBufferBindSeparateSamplersDeferred" => {
            hle_nvnCommandBufferBindSeparateSamplersDeferred as *const ()
        }
        "nvnCommandBufferBindSeparateTexture" => {
            hle_nvnCommandBufferBindSeparateTexture as *const ()
        }
        "nvnCommandBufferBindSeparateTextures" => {
            hle_nvnCommandBufferBindSeparateTextures as *const ()
        }
        "nvnCommandBufferBindSeparateTexturesDeferred" => {
            hle_nvnCommandBufferBindSeparateTexturesDeferred as *const ()
        }
        "nvnCommandBufferBindStorageBuffer" => hle_nvnCommandBufferBindStorageBuffer as *const (),
        "nvnCommandBufferBindStorageBuffers" => hle_nvnCommandBufferBindStorageBuffers as *const (),
        "nvnCommandBufferBindStorageBuffersDeferred" => {
            hle_nvnCommandBufferBindStorageBuffersDeferred as *const ()
        }
        "nvnCommandBufferBindTextures" => hle_nvnCommandBufferBindTextures as *const (),
        "nvnCommandBufferBindTexturesDeferred" => {
            hle_nvnCommandBufferBindTexturesDeferred as *const ()
        }
        "nvnCommandBufferBindTransformFeedbackBuffer" => {
            hle_nvnCommandBufferBindTransformFeedbackBuffer as *const ()
        }
        "nvnCommandBufferBindTransformFeedbackBuffers" => {
            hle_nvnCommandBufferBindTransformFeedbackBuffers as *const ()
        }
        "nvnCommandBufferBindUniformBuffers" => hle_nvnCommandBufferBindUniformBuffers as *const (),
        "nvnCommandBufferBindUniformBuffersDeferred" => {
            hle_nvnCommandBufferBindUniformBuffersDeferred as *const ()
        }
        "nvnCommandBufferBindVertexBuffers" => hle_nvnCommandBufferBindVertexBuffers as *const (),
        "nvnCommandBufferClearBuffer" => hle_nvnCommandBufferClearBuffer as *const (),
        "nvnCommandBufferClearColori" => hle_nvnCommandBufferClearColori as *const (),
        "nvnCommandBufferClearColorui" => hle_nvnCommandBufferClearColorui as *const (),
        "nvnCommandBufferClearTexture" => hle_nvnCommandBufferClearTexture as *const (),
        "nvnCommandBufferClearTexturei" => hle_nvnCommandBufferClearTexturei as *const (),
        "nvnCommandBufferClearTextureui" => hle_nvnCommandBufferClearTextureui as *const (),
        "nvnCommandBufferCopyBufferToBuffer" => hle_nvnCommandBufferCopyBufferToBuffer as *const (),
        "nvnCommandBufferCopyBufferToTexture" => {
            hle_nvnCommandBufferCopyBufferToTexture as *const ()
        }
        "nvnCommandBufferCopyCommands" => hle_nvnCommandBufferCopyCommands as *const (),
        "nvnCommandBufferCopyTextureToBuffer" => {
            hle_nvnCommandBufferCopyTextureToBuffer as *const ()
        }
        "nvnCommandBufferCopyTextureToTexture" => {
            hle_nvnCommandBufferCopyTextureToTexture as *const ()
        }
        "nvnCommandBufferDiscardColor" => hle_nvnCommandBufferDiscardColor as *const (),
        "nvnCommandBufferDiscardDepthStencil" => {
            hle_nvnCommandBufferDiscardDepthStencil as *const ()
        }
        "nvnCommandBufferDispatchCompute" => hle_nvnCommandBufferDispatchCompute as *const (),
        "nvnCommandBufferDispatchComputeIndirect" => {
            hle_nvnCommandBufferDispatchComputeIndirect as *const ()
        }
        "nvnCommandBufferDownsample" => hle_nvnCommandBufferDownsample as *const (),
        "nvnCommandBufferDownsampleTextureView" => {
            hle_nvnCommandBufferDownsampleTextureView as *const ()
        }
        "nvnCommandBufferDrawArraysIndirect" => hle_nvnCommandBufferDrawArraysIndirect as *const (),
        "nvnCommandBufferDrawArraysInstanced" => {
            hle_nvnCommandBufferDrawArraysInstanced as *const ()
        }
        "nvnCommandBufferDrawElementsIndirect" => {
            hle_nvnCommandBufferDrawElementsIndirect as *const ()
        }
        "nvnCommandBufferDrawElementsInstanced" => {
            hle_nvnCommandBufferDrawElementsInstanced as *const ()
        }
        "nvnCommandBufferDrawTexture" => hle_nvnCommandBufferDrawTexture as *const (),
        "nvnCommandBufferDrawTransformFeedback" => {
            hle_nvnCommandBufferDrawTransformFeedback as *const ()
        }
        "nvnCommandBufferEndTransformFeedback" => {
            hle_nvnCommandBufferEndTransformFeedback as *const ()
        }
        "nvnCommandBufferFenceSync" => hle_nvnCommandBufferFenceSync as *const (),
        "nvnCommandBufferInsertCheckpointDynamic" => {
            hle_nvnCommandBufferInsertCheckpointDynamic as *const ()
        }
        "nvnCommandBufferInsertCheckpointStatic" => {
            hle_nvnCommandBufferInsertCheckpointStatic as *const ()
        }
        "nvnCommandBufferInsertDebugMarker" => hle_nvnCommandBufferInsertDebugMarker as *const (),
        "nvnCommandBufferInsertDebugMarkerDynamic" => {
            hle_nvnCommandBufferInsertDebugMarkerDynamic as *const ()
        }
        "nvnCommandBufferInsertDebugMarkerStatic" => {
            hle_nvnCommandBufferInsertDebugMarkerStatic as *const ()
        }
        "nvnCommandBufferMultiDrawArraysIndirectCount" => {
            hle_nvnCommandBufferMultiDrawArraysIndirectCount as *const ()
        }
        "nvnCommandBufferMultiDrawElementsIndirectCount" => {
            hle_nvnCommandBufferMultiDrawElementsIndirectCount as *const ()
        }
        "nvnCommandBufferOverrideSubtileSize" => {
            hle_nvnCommandBufferOverrideSubtileSize as *const ()
        }
        "nvnCommandBufferPauseTransformFeedback" => {
            hle_nvnCommandBufferPauseTransformFeedback as *const ()
        }
        "nvnCommandBufferPopDebugGroup" => hle_nvnCommandBufferPopDebugGroup as *const (),
        "nvnCommandBufferPopDebugGroupId" => hle_nvnCommandBufferPopDebugGroupId as *const (),
        "nvnCommandBufferPushDebugGroup" => hle_nvnCommandBufferPushDebugGroup as *const (),
        "nvnCommandBufferPushDebugGroupDynamic" => {
            hle_nvnCommandBufferPushDebugGroupDynamic as *const ()
        }
        "nvnCommandBufferPushDebugGroupStatic" => {
            hle_nvnCommandBufferPushDebugGroupStatic as *const ()
        }
        "nvnCommandBufferReportCounter" => hle_nvnCommandBufferReportCounter as *const (),
        "nvnCommandBufferReportValue" => hle_nvnCommandBufferReportValue as *const (),
        "nvnCommandBufferResetCounter" => hle_nvnCommandBufferResetCounter as *const (),
        "nvnCommandBufferResolveDepthBuffer" => hle_nvnCommandBufferResolveDepthBuffer as *const (),
        "nvnCommandBufferRestoreZCullData" => hle_nvnCommandBufferRestoreZCullData as *const (),
        "nvnCommandBufferResumeTransformFeedback" => {
            hle_nvnCommandBufferResumeTransformFeedback as *const ()
        }
        "nvnCommandBufferSaveZCullData" => hle_nvnCommandBufferSaveZCullData as *const (),
        "nvnCommandBufferSetAlphaRef" => hle_nvnCommandBufferSetAlphaRef as *const (),
        "nvnCommandBufferSetColorReductionEnable" => {
            hle_nvnCommandBufferSetColorReductionEnable as *const ()
        }
        "nvnCommandBufferSetColorReductionThresholds" => {
            hle_nvnCommandBufferSetColorReductionThresholds as *const ()
        }
        "nvnCommandBufferSetCommandMemoryCallbackEnabled" => {
            hle_nvnCommandBufferSetCommandMemoryCallbackEnabled as *const ()
        }
        "nvnCommandBufferSetConservativeRasterDilate" => {
            hle_nvnCommandBufferSetConservativeRasterDilate as *const ()
        }
        "nvnCommandBufferSetConservativeRasterEnable" => {
            hle_nvnCommandBufferSetConservativeRasterEnable as *const ()
        }
        "nvnCommandBufferSetControlMemoryCallbackEnabled" => {
            hle_nvnCommandBufferSetControlMemoryCallbackEnabled as *const ()
        }
        "nvnCommandBufferSetCopyImageStride" => hle_nvnCommandBufferSetCopyImageStride as *const (),
        "nvnCommandBufferSetCopyRowStride" => hle_nvnCommandBufferSetCopyRowStride as *const (),
        "nvnCommandBufferSetDebugLabel" => hle_nvnCommandBufferSetDebugLabel as *const (),
        "nvnCommandBufferSetDepthBounds" => hle_nvnCommandBufferSetDepthBounds as *const (),
        "nvnCommandBufferSetDepthRanges" => {
            crate::nvn::cmd::hle_nvnCommandBufferSetDepthRanges as *const ()
        }
        "nvnCommandBufferSetInnerTessellationLevels" => {
            hle_nvnCommandBufferSetInnerTessellationLevels as *const ()
        }
        "nvnCommandBufferSetLineWidth" => hle_nvnCommandBufferSetLineWidth as *const (),
        "nvnCommandBufferSetMemoryCallback" => hle_nvnCommandBufferSetMemoryCallback as *const (),
        "nvnCommandBufferSetMemoryCallbackData" => {
            hle_nvnCommandBufferSetMemoryCallbackData as *const ()
        }
        "nvnCommandBufferSetOuterTessellationLevels" => {
            hle_nvnCommandBufferSetOuterTessellationLevels as *const ()
        }
        "nvnCommandBufferSetPatchSize" => hle_nvnCommandBufferSetPatchSize as *const (),
        "nvnCommandBufferSetPointSize" => hle_nvnCommandBufferSetPointSize as *const (),
        "nvnCommandBufferSetPolygonOffsetClamp" => {
            hle_nvnCommandBufferSetPolygonOffsetClamp as *const ()
        }
        "nvnCommandBufferSetProgramSubroutines" => {
            hle_nvnCommandBufferSetProgramSubroutines as *const ()
        }
        "nvnCommandBufferSetProvokingVertexMode" => {
            hle_nvnCommandBufferSetProvokingVertexMode as *const ()
        }
        "nvnCommandBufferSetRasterizerDiscard" => {
            hle_nvnCommandBufferSetRasterizerDiscard as *const ()
        }
        "nvnCommandBufferSetRenderEnable" => hle_nvnCommandBufferSetRenderEnable as *const (),
        "nvnCommandBufferSetRenderEnableConditional" => {
            hle_nvnCommandBufferSetRenderEnableConditional as *const ()
        }
        "nvnCommandBufferSetSampleMask" => hle_nvnCommandBufferSetSampleMask as *const (),
        "nvnCommandBufferSetSamplerPool" => hle_nvnCommandBufferSetSamplerPool as *const (),
        "nvnCommandBufferSetScissors" => {
            crate::nvn::cmd::hle_nvnCommandBufferSetScissors as *const ()
        }
        "nvnCommandBufferSetShaderScratchMemory" => {
            hle_nvnCommandBufferSetShaderScratchMemory as *const ()
        }
        "nvnCommandBufferSetStencilCullCriteria" => {
            hle_nvnCommandBufferSetStencilCullCriteria as *const ()
        }
        "nvnCommandBufferSetStencilMask" => hle_nvnCommandBufferSetStencilMask as *const (),
        "nvnCommandBufferSetStencilRef" => hle_nvnCommandBufferSetStencilRef as *const (),
        "nvnCommandBufferSetStencilValueMask" => {
            hle_nvnCommandBufferSetStencilValueMask as *const ()
        }
        "nvnCommandBufferSetSubpixelPrecisionBias" => {
            hle_nvnCommandBufferSetSubpixelPrecisionBias as *const ()
        }
        "nvnCommandBufferSetTexturePool" => hle_nvnCommandBufferSetTexturePool as *const (),
        "nvnCommandBufferSetTiledCacheAction" => {
            hle_nvnCommandBufferSetTiledCacheAction as *const ()
        }
        "nvnCommandBufferSetTiledCacheTileSize" => {
            hle_nvnCommandBufferSetTiledCacheTileSize as *const ()
        }
        "nvnCommandBufferSetViewports" => {
            crate::nvn::cmd::hle_nvnCommandBufferSetViewports as *const ()
        }
        "nvnCommandBufferSetViewportSwizzles" => {
            hle_nvnCommandBufferSetViewportSwizzles as *const ()
        }
        "nvnCommandBufferSetZCullZF32CompressionEnable" => {
            hle_nvnCommandBufferSetZCullZF32CompressionEnable as *const ()
        }
        "nvnCommandBufferSignalEvent" => hle_nvnCommandBufferSignalEvent as *const (),
        "nvnCommandBufferTiledDownsample" => hle_nvnCommandBufferTiledDownsample as *const (),
        "nvnCommandBufferTiledDownsampleTextureView" => {
            hle_nvnCommandBufferTiledDownsampleTextureView as *const ()
        }
        "nvnCommandBufferWaitEvent" => hle_nvnCommandBufferWaitEvent as *const (),
        "nvnCommandBufferWaitSync" => hle_nvnCommandBufferWaitSync as *const (),
        "nvnBufferBuilderSetDefaults" => hle_nvnBufferBuilderSetDefaults as *const (),
        "nvnBufferBuilderSetDevice" => hle_nvnBufferBuilderSetDevice as *const (),
        "nvnBufferBuilderSetStorage" => hle_nvnBufferBuilderSetStorage as *const (),
        "nvnBufferBuilderGetSize" => hle_nvnBufferBuilderGetSize as *const (),
        "nvnBufferBuilderGetMemoryOffset" => hle_nvnBufferBuilderGetMemoryOffset as *const (),
        "nvnBufferBuilderGetMemoryPool" => hle_nvnBufferBuilderGetMemoryPool as *const (),
        "nvnBufferInitialize" => hle_nvnBufferInitialize as *const (),
        "nvnBufferFinalize" => hle_nvnBufferFinalize as *const (),
        "nvnBufferGetAddress" => hle_nvnBufferGetAddress as *const (),
        "nvnBufferMap" => hle_nvnBufferMap as *const (),
        "nvnBufferGetSize" => hle_nvnBufferGetSize as *const (),
        "nvnBufferFlushMappedRange" => hle_nvnBufferFlushMappedRange as *const (),
        "nvnBufferInvalidateMappedRange" => hle_nvnBufferInvalidateMappedRange as *const (),
        "nvnTextureBuilderSetDefaults" => hle_nvnTextureBuilderSetDefaults as *const (),
        "nvnTextureBuilderSetDevice" => hle_nvnTextureBuilderSetDevice as *const (),
        "nvnTextureBuilderSetTarget" => hle_nvnTextureBuilderSetTarget as *const (),
        "nvnTextureBuilderSetWidth" => hle_nvnTextureBuilderSetWidth as *const (),
        "nvnTextureBuilderSetHeight" => hle_nvnTextureBuilderSetHeight as *const (),
        "nvnTextureBuilderSetDepth" => hle_nvnTextureBuilderSetDepth as *const (),
        "nvnTextureBuilderSetLevels" => hle_nvnTextureBuilderSetLevels as *const (),
        "nvnTextureBuilderSetFormat" => hle_nvnTextureBuilderSetFormat as *const (),
        "nvnTextureBuilderSetFlags" => hle_nvnTextureBuilderSetFlags as *const (),
        "nvnTextureBuilderSetSamples" => hle_nvnTextureBuilderSetSamples as *const (),
        "nvnTextureBuilderSetSwizzle" => hle_nvnTextureBuilderSetSwizzle as *const (),
        "nvnTextureBuilderSetStride" => hle_nvnTextureBuilderSetStride as *const (),
        "nvnTextureBuilderSetStorage" => hle_nvnTextureBuilderSetStorage as *const (),
        "nvnTextureBuilderGetStorageSize" => hle_nvnTextureBuilderGetStorageSize as *const (),
        "nvnTextureBuilderGetStorageAlignment" => {
            hle_nvnTextureBuilderGetStorageAlignment as *const ()
        }
        "nvnTextureBuilderGetStorageClass" => hle_nvnTextureBuilderGetStorageClass as *const (),
        "nvnTextureBuilderGetZCullStorageSize" => hle_nvnZCullStorageSize as *const (),
        "nvnTextureInitialize" => hle_nvnTextureInitialize as *const (),
        "nvnTextureFinalize" => hle_nvnTextureFinalize as *const (),
        "nvnTextureGetWidth" => hle_nvnTextureGetWidth as *const (),
        "nvnTextureGetHeight" => hle_nvnTextureGetHeight as *const (),
        "nvnTextureGetZCullStorageSize" => hle_nvnZCullStorageSize as *const (),
        "nvnMemoryPoolGetFlags" => hle_nvnMemoryPoolGetFlags as *const (),
        "nvnMemoryPoolMapVirtual" => hle_nvnMemoryPoolMapVirtual as *const (),
        "nvnTextureBuilderSetDepthStencilMode" => {
            hle_nvnTextureBuilderSetDepthStencilMode as *const ()
        }
        "nvnDeviceBuilderSetDefaults" => hle_nvnDeviceBuilderSetDefaults as *const (),
        "nvnDeviceBuilderSetFlags" => hle_nvnDeviceBuilderSetFlags as *const (),
        "nvnDeviceRegisterFastClearColor" => hle_nvnDeviceRegisterFastClearColor as *const (),
        "nvnSamplerBuilderSetDefaults" => hle_nvnSamplerBuilderSetDefaults as *const (),
        "nvnSamplerBuilderSetDevice" => hle_nvnSamplerBuilderSetDevice as *const (),
        "nvnSamplerBuilderSetMinMagFilter" => hle_nvnSamplerBuilderSetMinMagFilter as *const (),
        "nvnSamplerBuilderSetWrapMode" => hle_nvnSamplerBuilderSetWrapMode as *const (),
        "nvnSamplerBuilderSetBorderColor" => hle_nvnSamplerBuilderSetBorderColor as *const (),
        "nvnSamplerBuilderSetMaxAnisotropy" => hle_nvnSamplerBuilderSetMaxAnisotropy as *const (),
        "nvnSamplerBuilderSetLodClamp" => hle_nvnSamplerBuilderSetLodClamp as *const (),
        "nvnSamplerBuilderSetLodBias" => hle_nvnSamplerBuilderSetLodBias as *const (),
        "nvnSamplerBuilderSetCompare" => hle_nvnSamplerBuilderSetCompare as *const (),
        "nvnSamplerBuilderSetReductionFilter" => {
            hle_nvnSamplerBuilderSetReductionFilter as *const ()
        }
        "nvnSamplerGetReductionFilter" => hle_nvnSamplerGetReductionFilter as *const (),
        "nvnSamplerGetMaxAnisotropy" => hle_nvnSamplerGetMaxAnisotropy as *const (),
        "nvnSamplerGetLodClamp" => hle_nvnSamplerGetLodClamp as *const (),
        "nvnSamplerGetLodBias" => hle_nvnSamplerGetLodBias as *const (),
        "nvnSamplerGetCompare" => hle_nvnSamplerGetCompare as *const (),
        "nvnSamplerGetBorderColor" => hle_nvnSamplerGetBorderColor as *const (),
        "nvnSamplerInitialize" => hle_nvnSamplerInitialize as *const (),
        "nvnSamplerFinalize" => hle_nvnSamplerFinalize as *const (),
        "nvnTextureReadTexels" => hle_nvnTextureReadTexels as *const (),
        "nvnTextureReadTexelsStrided" => hle_nvnTextureReadTexelsStrided as *const (),
        "nvnTextureWriteTexels" => hle_nvnTextureWriteTexels as *const (),
        "nvnTextureWriteTexelsStrided" => hle_nvnTextureWriteTexelsStrided as *const (),
        "nvnTextureFlushTexels" => hle_nvnTextureFlushTexels as *const (),
        "nvnTextureInvalidateTexels" => hle_nvnTextureInvalidateTexels as *const (),
        "nvnTextureBuilderSetPackagedTextureData" => {
            hle_nvnTextureBuilderSetPackagedTextureData as *const ()
        }
        "nvnDeviceGetTimestampInNanoseconds" => hle_nvnDeviceGetTimestampInNanoseconds as *const (),
        "nvnDeviceSetWindowOriginMode" => hle_nvnDeviceSetWindowOriginMode as *const (),
        "nvnDeviceSetDepthMode" => hle_nvnDeviceSetDepthMode as *const (),
        "nvnTextureBuilderSetPackagedTextureLayout" => {
            hle_nvnTextureBuilderSetPackagedTextureLayout as *const ()
        }
        "nvnWindowSetPresentInterval" | "nvnWindowBuilderSetPresentInterval" => {
            hle_nvnWindowSetPresentInterval as *const ()
        }
        "nvnWindowGetPresentInterval" | "nvnWindowBuilderGetPresentInterval" => {
            hle_nvnWindowGetPresentInterval as *const ()
        }
        "nvnWindowBuilderSetNativeWindow"
        | "nvnBufferSetDebugLabel"
        | "nvnWindowSetPresentInterval2"
        | "nvnQueueSetDebugLabel"
        | "nvnWindowSetDebugLabel"
        | "nvnDeviceSetDebugLabel" => hle_nvn_noop_setter as *const (),
        "nvnWindowBuilderSetNumActiveTextures" => {
            hle_nvnWindowBuilderSetNumActiveTextures as *const ()
        }
        "nvnQueueBuilderSetQueueMemory" => hle_nvnQueueBuilderSetQueueMemory as *const (),
        "nvnQueueBuilderSetCommandFlushThreshold" => {
            hle_nvnQueueBuilderSetCommandFlushThreshold as *const ()
        }
        "nvnQueueBuilderSetQueuePriority" => hle_nvnQueueBuilderSetQueuePriority as *const (),
        "nvnQueueBuilderGetDevice" => hle_nvnQueueBuilderGetDevice as *const (),
        "nvnQueueBuilderGetFlags" => hle_nvnQueueBuilderGetFlags as *const (),
        "nvnQueueBuilderGetCommandMemorySize" => {
            hle_nvnQueueBuilderGetCommandMemorySize as *const ()
        }
        "nvnQueueBuilderGetComputeMemorySize" => {
            hle_nvnQueueBuilderGetComputeMemorySize as *const ()
        }
        "nvnQueueBuilderGetControlMemorySize" => {
            hle_nvnQueueBuilderGetControlMemorySize as *const ()
        }
        "nvnQueueBuilderGetCommandFlushThreshold" => {
            hle_nvnQueueBuilderGetCommandFlushThreshold as *const ()
        }
        "nvnQueueBuilderGetQueuePriority" => hle_nvnQueueBuilderGetQueuePriority as *const (),
        "nvnQueueBuilderGetMemory" => hle_nvnQueueBuilderGetMemory as *const (),
        "nvnQueueBuilderGetMemorySize" => hle_nvnQueueBuilderGetMemorySize as *const (),
        "nvnWindowBuilderGetDevice" => hle_nvnWindowBuilderGetDevice as *const (),
        "nvnWindowBuilderGetNumTextures" => hle_nvnWindowBuilderGetNumTextures as *const (),
        "nvnWindowBuilderGetTexture" => hle_nvnWindowBuilderGetTexture as *const (),
        "nvnWindowBuilderGetNumActiveTextures" => {
            hle_nvnWindowBuilderGetNumActiveTextures as *const ()
        }
        "nvnWindowGetNumTextures" => hle_nvnWindowGetNumTextures as *const (),
        "nvnWindowGetNumActiveTextures" => hle_nvnWindowGetNumActiveTextures as *const (),
        "nvnWindowGetCrop" => hle_nvnWindowGetCrop as *const (),
        "nvnBufferBuilderGetDevice" => hle_nvnBufferBuilderGetDevice as *const (),
        "nvnBufferGetMemoryPool" => hle_nvnBufferGetMemoryPool as *const (),
        "nvnBufferGetMemoryOffset" => hle_nvnBufferGetMemoryOffset as *const (),
        "nvnMemoryPoolGetSize" => hle_nvnMemoryPoolGetSize as *const (),
        "nvnDeviceGetDepthMode" => hle_nvnDeviceGetDepthMode as *const (),
        "nvnDeviceGetWindowOriginMode" => hle_nvnDeviceGetWindowOriginMode as *const (),
        "nvnDeviceGetCurrentTimestampInNanoseconds" => {
            hle_nvnDeviceGetCurrentTimestampInNanoseconds as *const ()
        }
        "nvnDeviceBuilderGetFlags" => hle_nvnDeviceBuilderGetFlags as *const (),
        "nvnTextureBuilderGetSwizzle" => hle_nvnTextureBuilderGetSwizzle as *const (),
        "nvnTextureBuilderGetDepthStencilMode" => {
            hle_nvnTextureBuilderGetDepthStencilMode as *const ()
        }
        "nvnSamplerBuilderGetDevice" => hle_nvnSamplerBuilderGetDevice as *const (),
        "nvnSamplerBuilderGetMinMagFilter" => hle_nvnSamplerBuilderGetMinMagFilter as *const (),
        "nvnSamplerBuilderGetWrapMode" => hle_nvnSamplerBuilderGetWrapMode as *const (),
        "nvnSamplerBuilderGetLodClamp" => hle_nvnSamplerBuilderGetLodClamp as *const (),
        "nvnSamplerBuilderGetLodBias" => hle_nvnSamplerBuilderGetLodBias as *const (),
        "nvnSamplerBuilderGetCompare" => hle_nvnSamplerBuilderGetCompare as *const (),
        "nvnSamplerBuilderGetBorderColor"
        | "nvnSamplerBuilderGetBorderColori"
        | "nvnSamplerBuilderGetBorderColorui" => hle_nvnSamplerBuilderGetBorderColor as *const (),
        "nvnSamplerBuilderSetBorderColori" | "nvnSamplerBuilderSetBorderColorui" => {
            hle_nvnSamplerBuilderSetBorderColor as *const ()
        }
        "nvnSamplerGetBorderColori" | "nvnSamplerGetBorderColorui" => {
            hle_nvnSamplerGetBorderColor as *const ()
        }
        "nvnSamplerBuilderGetMaxAnisotropy" => hle_nvnSamplerBuilderGetMaxAnisotropy as *const (),
        "nvnSamplerBuilderGetReductionFilter" => {
            hle_nvnSamplerBuilderGetReductionFilter as *const ()
        }
        "nvnSamplerCompare" => hle_nvnSamplerCompare as *const (),
        "nvnProgramInitialize" => hle_nvnProgramInitialize as *const (),
        "nvnProgramFinalize" => hle_nvnProgramFinalize as *const (),
        "nvnProgramSetDebugLabel" => hle_nvnProgramSetDebugLabel as *const (),
        "nvnProgramSetShaders" => hle_nvnProgramSetShaders as *const (),
        "nvnProgramSetShadersExt" => hle_nvnProgramSetShadersExt as *const (),
        "nvnProgramSetSubroutineLinkage" => hle_nvnProgramSetSubroutineLinkage as *const (),
        "nvnProgramSetSampleShading" => hle_nvnProgramSetSampleShading as *const (),
        "nvnSamplerGetMinMagFilter" => hle_nvnSamplerGetMinMagFilter as *const (),
        "nvnSamplerGetWrapMode" => hle_nvnSamplerGetWrapMode as *const (),
        "nvnSamplerSetDebugLabel" => hle_nvnSamplerSetDebugLabel as *const (),
        "nvnWindowBuilderSetDefaults" => hle_nvnWindowBuilderSetDefaults as *const (),
        "nvnWindowBuilderSetDevice" => hle_nvnWindowBuilderSetDevice as *const (),
        "nvnWindowBuilderSetTextures" => hle_nvnWindowBuilderSetTextures as *const (),
        "nvnWindowInitialize" => hle_nvnWindowInitialize as *const (),
        "nvnWindowFinalize" => hle_nvnWindowFinalize as *const (),
        "nvnWindowAcquireTexture" => hle_nvnWindowAcquireTexture as *const (),
        "nvnWindowSetCrop" => hle_nvnWindowSetCrop as *const (),
        "nvnWindowSetNumActiveTextures" => hle_nvnWindowSetNumActiveTextures as *const (),
        "nvnQueueBuilderSetDevice" => hle_nvnQueueBuilderSetDevice as *const (),
        "nvnQueueBuilderSetDefaults" => hle_nvnQueueBuilderSetDefaults as *const (),
        "nvnQueueBuilderSetFlags" => hle_nvnQueueBuilderSetFlags as *const (),
        "nvnQueueBuilderSetCommandMemorySize" => {
            hle_nvnQueueBuilderSetCommandMemorySize as *const ()
        }
        "nvnQueueBuilderSetComputeMemorySize" => {
            hle_nvnQueueBuilderSetComputeMemorySize as *const ()
        }
        "nvnQueueBuilderSetControlMemorySize" => {
            hle_nvnQueueBuilderSetControlMemorySize as *const ()
        }
        "nvnQueueBuilderGetQueueMemorySize" => hle_nvnQueueBuilderGetQueueMemorySize as *const (),
        "nvnQueueInitialize" => hle_nvnQueueInitialize as *const (),
        "nvnQueueFinalize" => hle_nvnQueueFinalize as *const (),
        "nvnQueueFlush" => hle_nvnQueueFlush as *const (),
        "nvnQueueFinish" => hle_nvnQueueFinish as *const (),
        "nvnQueueGetError" => hle_nvnQueueGetError as *const (),
        "nvnQueueGetTotalCommandMemoryUsed" => hle_nvnQueueGetTotalCommandMemoryUsed as *const (),
        "nvnQueueGetTotalControlMemoryUsed" => hle_nvnQueueGetTotalControlMemoryUsed as *const (),
        "nvnQueueGetTotalComputeMemoryUsed" => hle_nvnQueueGetTotalComputeMemoryUsed as *const (),
        "nvnQueueResetMemoryUsageCounts" => hle_nvnQueueResetMemoryUsageCounts as *const (),
        "nvnQueuePresentTexture" => hle_nvnQueuePresentTexture as *const (),
        "nvnQueueWaitSync" => hle_nvnQueueWaitSync as *const (),
        "nvnQueueFenceSync" => hle_nvnQueueFenceSync as *const (),

        // TexturePool family
        "nvnTexturePoolInitialize" => hle_nvnTexturePoolInitialize as *const (),
        "nvnTexturePoolSetDebugLabel" => hle_nvnTexturePoolSetDebugLabel as *const (),
        "nvnTexturePoolFinalize" => hle_nvnTexturePoolFinalize as *const (),
        "nvnTexturePoolGetMemoryPool" => hle_nvnTexturePoolGetMemoryPool as *const (),
        "nvnTexturePoolGetMemoryOffset" => hle_nvnTexturePoolGetMemoryOffset as *const (),
        "nvnTexturePoolGetSize" => hle_nvnTexturePoolGetSize as *const (),
        "nvnTexturePoolRegisterTexture" => hle_nvnTexturePoolRegisterTexture as *const (),
        "nvnTexturePoolRegisterImage" => hle_nvnTexturePoolRegisterImage as *const (),

        // SamplerPool family
        "nvnSamplerPoolInitialize" => hle_nvnSamplerPoolInitialize as *const (),
        "nvnSamplerPoolSetDebugLabel" => hle_nvnSamplerPoolSetDebugLabel as *const (),
        "nvnSamplerPoolFinalize" => hle_nvnSamplerPoolFinalize as *const (),
        "nvnSamplerPoolGetMemoryPool" => hle_nvnSamplerPoolGetMemoryPool as *const (),
        "nvnSamplerPoolGetMemoryOffset" => hle_nvnSamplerPoolGetMemoryOffset as *const (),
        "nvnSamplerPoolGetSize" => hle_nvnSamplerPoolGetSize as *const (),
        "nvnSamplerPoolRegisterSampler" => hle_nvnSamplerPoolRegisterSampler as *const (),
        "nvnSamplerPoolRegisterSamplerBuilder" => {
            hle_nvnSamplerPoolRegisterSamplerBuilder as *const ()
        }

        // Texture getters and helpers
        "nvnTextureGetTextureAddress" => hle_nvnTextureGetTextureAddress as *const (),
        "nvnTextureGetDepth" => hle_nvnTextureGetDepth as *const (),
        "nvnTextureGetLevels" => hle_nvnTextureGetLevels as *const (),
        "nvnTextureGetFormat" => hle_nvnTextureGetFormat as *const (),
        "nvnTextureGetFlags" => hle_nvnTextureGetFlags as *const (),
        "nvnTextureGetSamples" => hle_nvnTextureGetSamples as *const (),
        "nvnTextureGetTarget" => hle_nvnTextureGetTarget as *const (),
        "nvnTextureGetStride" => hle_nvnTextureGetStride as *const (),
        "nvnTextureGetStorageSize" => hle_nvnTextureGetStorageSize as *const (),
        "nvnTextureGetStorageClass" => hle_nvnTextureGetStorageClass as *const (),
        "nvnTextureGetMemoryPool" => hle_nvnTextureGetMemoryPool as *const (),
        "nvnTextureGetMemoryOffset" => hle_nvnTextureGetMemoryOffset as *const (),
        "nvnTextureGetViewOffset" => hle_nvnTextureGetViewOffset as *const (),
        "nvnTextureGetDebugID" => hle_nvnTextureGetDebugID as *const (),
        "nvnTextureGetSwizzle" => hle_nvnTextureGetSwizzle as *const (),
        "nvnTextureGetDepthStencilMode" => hle_nvnTextureGetDepthStencilMode as *const (),
        "nvnTextureGetRawStorageClass" => hle_nvnTextureGetRawStorageClass as *const (),
        "nvnTextureGetSparseTileLayout" => hle_nvnTextureGetSparseTileLayout as *const (),
        "nvnTextureSetDebugLabel" => hle_nvnTextureSetDebugLabel as *const (),
        "nvnTextureCompare" => hle_nvnTextureCompare as *const (),

        // TextureBuilder additional setters & getters
        "nvnTextureBuilderSetSize1D" => hle_nvnTextureBuilderSetSize1D as *const (),
        "nvnTextureBuilderSetSize2D" => hle_nvnTextureBuilderSetSize2D as *const (),
        "nvnTextureBuilderSetSize3D" => hle_nvnTextureBuilderSetSize3D as *const (),
        "nvnTextureBuilderGetDevice" => hle_nvnTextureBuilderGetDevice as *const (),
        "nvnTextureBuilderGetTarget" => hle_nvnTextureBuilderGetTarget as *const (),
        "nvnTextureBuilderGetWidth" => hle_nvnTextureBuilderGetWidth as *const (),
        "nvnTextureBuilderGetHeight" => hle_nvnTextureBuilderGetHeight as *const (),
        "nvnTextureBuilderGetDepth" => hle_nvnTextureBuilderGetDepth as *const (),
        "nvnTextureBuilderGetLevels" => hle_nvnTextureBuilderGetLevels as *const (),
        "nvnTextureBuilderGetFormat" => hle_nvnTextureBuilderGetFormat as *const (),
        "nvnTextureBuilderGetFlags" => hle_nvnTextureBuilderGetFlags as *const (),
        "nvnTextureBuilderGetSamples" => hle_nvnTextureBuilderGetSamples as *const (),
        "nvnTextureBuilderGetStride" => hle_nvnTextureBuilderGetStride as *const (),
        "nvnTextureBuilderGetMemoryPool" => hle_nvnTextureBuilderGetMemoryPool as *const (),
        "nvnTextureBuilderGetMemoryOffset" => hle_nvnTextureBuilderGetMemoryOffset as *const (),
        "nvnTextureBuilderGetViewOffset" => hle_nvnTextureBuilderGetViewOffset as *const (),

        // TextureView family
        "nvnTextureViewSetDefaults" => hle_nvnTextureViewSetDefaults as *const (),
        "nvnTextureViewSetLevels" => hle_nvnTextureViewSetLevels as *const (),
        "nvnTextureViewSetLayers" => hle_nvnTextureViewSetLayers as *const (),
        "nvnTextureViewSetFormat" => hle_nvnTextureViewSetFormat as *const (),
        "nvnTextureViewSetSwizzle" => hle_nvnTextureViewSetSwizzle as *const (),
        "nvnTextureViewSetDepthStencilMode" => hle_nvnTextureViewSetDepthStencilMode as *const (),
        "nvnTextureViewSetTarget" => hle_nvnTextureViewSetTarget as *const (),
        "nvnTextureViewGetLevels" => hle_nvnTextureViewGetLevels as *const (),
        "nvnTextureViewGetLayers" => hle_nvnTextureViewGetLayers as *const (),
        "nvnTextureViewGetFormat" => hle_nvnTextureViewGetFormat as *const (),
        "nvnTextureViewGetSwizzle" => hle_nvnTextureViewGetSwizzle as *const (),
        "nvnTextureViewGetDepthStencilMode" => hle_nvnTextureViewGetDepthStencilMode as *const (),
        "nvnTextureViewGetTarget" => hle_nvnTextureViewGetTarget as *const (),
        "nvnTextureViewCompare" => hle_nvnTextureViewCompare as *const (),

        // Sync & Event family
        "nvnSyncInitialize" => hle_nvnSyncInitialize as *const (),
        "nvnSyncFinalize" => hle_nvnSyncFinalize as *const (),
        "nvnSyncSetDebugLabel" => hle_nvnSyncSetDebugLabel as *const (),
        "nvnSyncWait" => hle_nvnSyncWait as *const (),
        "nvnEventInitialize" => hle_nvnEventInitialize as *const (),
        "nvnEventFinalize" => hle_nvnEventFinalize as *const (),
        "nvnEventGetValue" => hle_nvnEventGetValue as *const (),
        "nvnEventSignal" => hle_nvnEventSignal as *const (),
        _ => resolve_unimplemented_nvn(func_name),
    }
}

// ---------------------------------------------------------------------------
// NVN Memory Pool family -- real implementation, not a stub.
//
// Root cause this session: the game's own GPU memory-pool wrapper object
// (sead::gfx-style, dispatched through a C++ vtable at guest+0xCB2500/+0xCB2B88)
// never got constructed because `nvnMemoryPoolInitialize` -- which it calls
// internally via a real function pointer fetched through
// nvnDeviceGetProcAddress -- was silently falling through to
// hle_nvn_generic_stub (always returns 0 = NVN_FALSE). The game read that
// as "pool init failed" and never finished its own construction, leaving the
// wrapper's vtable pointer permanently null. Implementing this family for
// real (not just making it return success) fixes that at the source.
//
// NVNmemoryPool (0x100 bytes) and NVNmemoryPoolBuilder (0x40 bytes) are
// opaque, driver-defined storage (NVN declares them as
// `char reserved[...]`) -- the game never reads their fields directly, only
// ever hands the pointer back into other nvn* calls. That means we're free
// to define our own internal layout, as long as our own Set*/Get*/Initialize
// implementations agree with each other -- same freedom nn::vi's opaque
// Display/Layer handles already gave us.
#[repr(C)]
pub(crate) struct NvnMemoryPoolBuilder {
    device: usize,
    memory: usize,
    size: usize,
    flags: i32,
    _pad: u32,
}

#[repr(C)]
pub struct NvnMemoryPool {
    pub memory: usize,
    pub size: usize,
    pub flags: i32,
    pub _pad: u32,
    pub device: usize,
    pub owns_memory: u8, // 1 if we host-allocated `memory` ourselves (VIRTUAL pool) and must free it on Finalize
}

// Live memory pools as (base, size) host ranges. GPU addresses are host pointers, so the Metal executor
// resolves a buffer address to its pool and wraps the pool's pages as one zero-copy MTLBuffer.
/// Registered pools by base -> size. Registration removes overlapping entries, so at most one contains an address.
static MEMORY_POOLS: std::sync::Mutex<std::collections::BTreeMap<usize, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
/// NVNmemoryPool object -> base of its registration in MEMORY_POOLS.
static POOL_OBJECTS: std::sync::Mutex<std::collections::BTreeMap<usize, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());
/// NVN memory pool flags by pool base (NVN_MEMORY_POOL_FLAGS: CPU_NO_ACCESS 0x1, CPU_UNCACHED 0x2, CPU_CACHED 0x4, ...).
static MEMORY_POOL_FLAGS: std::sync::Mutex<std::collections::BTreeMap<usize, i32>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

/// Bumped whenever MEMORY_POOLS, MEMORY_POOL_FLAGS or VIRTUAL_MAPS change; invalidates the per-thread lookup caches
/// below (the executor resolves pools for every buffer and texture binding, and a linear scan per lookup was ~9% of the
/// presentation thread).
static POOLS_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
fn bump_pools_gen() {
    POOLS_GEN.fetch_add(1, std::sync::atomic::Ordering::Release);
}

/// Changes whenever a pool is registered or finalized, a virtual mapping changes, or guest memory is released or
/// unmapped: lookups derived from pools (gfx zero-copy buffer wrappers) are valid while it is unchanged.
pub fn pools_gen() -> u64 {
    POOLS_GEN.load(std::sync::atomic::Ordering::Acquire)
}

/// Per-thread cache of recently found pools: (base, size, cpu_writable); valid while the generation matches.
struct PoolCache {
    gen: u64,
    entries: [(usize, usize, bool); 8],
    next: usize,
}
thread_local! {
    static POOL_CACHE: std::cell::RefCell<PoolCache> = const { std::cell::RefCell::new(PoolCache { gen: 0, entries: [(0, 0, false); 8], next: 0 }) };
}

/// (base, size, cpu_writable) of the pool containing `addr` (no virtual translation).
fn pool_info(addr: usize) -> Option<(usize, usize, bool)> {
    let gen = POOLS_GEN.load(std::sync::atomic::Ordering::Acquire);
    let hit = POOL_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.gen != gen {
            c.gen = gen;
            c.entries = [(0, 0, false); 8];
            return None;
        }
        c.entries
            .iter()
            .copied()
            .find(|&(b, s, _)| s != 0 && addr >= b && addr < b + s)
    });
    if hit.is_some() {
        return hit;
    }
    // Ordered by base: the closest pool starting at or below `addr` that still covers it. Pools can nest (Kirby makes
    // small shader pools inside the storage of a 1.4 GB pool that also holds vertex data), so the nearest one below
    // may end before `addr`.
    let (base, size) = containing_pool(addr, 1)?;
    let writable = MEMORY_POOL_FLAGS
        .lock()
        .unwrap()
        .get(&base)
        .map_or(false, |f| f & 0x6 != 0);
    POOL_CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.gen == gen {
            let i = c.next;
            c.entries[i] = (base, size, writable);
            c.next = (i + 1) % 8;
        }
    });
    Some((base, size, writable))
}

/// Whether the CPU can write the memory pool containing `addr` (so data the GPU reads from it may change under it).
pub fn memory_pool_cpu_writable(addr: usize) -> bool {
    pool_info(translate_virtual(addr)).map_or(false, |p| p.2)
}

/// The registered pool range containing `addr`, if any.
/// Guest memory ranges released since the executor last looked (pool finalized / address region freed): zero-copy
/// Metal buffers wrapping them must not be reused.
static RELEASED_RANGES: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());
/// Address regions whose unmap is deferred until the GPU can no longer be using them: (addr, size, frame freed).
pub(crate) static PENDING_UNMAPS: StdMutex<Vec<(usize, usize, u64)>> = StdMutex::new(Vec::new());

pub(crate) fn guest_memory_released(addr: usize, size: usize) {
    unsafe { crate::guest::dirty_track::release(addr, size) };
    RELEASED_RANGES.lock().unwrap().push((addr, size));
    TEX_RELEASED.lock().unwrap().push((addr, size));
    bump_pools_gen();
}

/// Released ranges for the texture cache (metal::sweep_shared_storage), separate from the buffer cache's list.
static TEX_RELEASED: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());

/// Ranges released since the texture cache last looked.
pub fn take_tex_released() -> Vec<(usize, usize)> {
    std::mem::take(&mut *TEX_RELEASED.lock().unwrap())
}

/// Ranges whose pages were replaced (deferred unmap) since the executor last looked.
static UNMAPPED_RANGES: StdMutex<Vec<(usize, usize)>> = StdMutex::new(Vec::new());

/// Ranges released since the last call (for the executor's buffer cache): (addr, size, pages replaced now).
pub fn take_released_ranges() -> Vec<(usize, usize, bool)> {
    let mut v: Vec<(usize, usize, bool)> = std::mem::take(&mut *RELEASED_RANGES.lock().unwrap())
        .into_iter()
        .map(|(a, s)| (a, s, false))
        .collect();
    v.extend(
        std::mem::take(&mut *UNMAPPED_RANGES.lock().unwrap())
            .into_iter()
            .map(|(a, s)| (a, s, true)),
    );
    v
}

/// Unmaps address regions freed at least `frames` presents ago (called by the executor at present).
pub fn flush_pending_unmaps(now: u64, frames: u64) {
    let mut pending = PENDING_UNMAPS.lock().unwrap();
    pending.retain(|&(addr, size, at)| {
        if now >= at + frames {
            unsafe { release_address_region(addr, size) };
            // The pages are replaced only now, and a zero-copy Metal buffer keeps the pages it was made over: the GPU
            // would read the old contents through it forever (checked with a standalone test). Draws encoded since the
            // free (recorded before it) can have wrapped this range again, and such a buffer would serve the region's
            // next allocation stale data (geometry streaks that accumulate as the game streams). Report it again;
            // this runs on the executor thread, before any later wrap.
            UNMAPPED_RANGES.lock().unwrap().push((addr, size));
            bump_pools_gen();
            false
        } else {
            true
        }
    });
}

pub fn find_memory_pool(addr: usize) -> Option<(usize, usize)> {
    pool_info(addr).map(|(b, s, _)| (b, s))
}

/// The innermost pool (by start) that covers all of `[addr, addr + len)`.
pub fn containing_pool(addr: usize, len: usize) -> Option<(usize, usize)> {
    MEMORY_POOLS
        .lock()
        .unwrap()
        .range(..=addr)
        .rev()
        .take(4096)
        .map(|(&b, &s)| (b, s))
        .find(|&(b, s)| addr + len.max(1) <= b + s)
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetDefaults(builder: *mut NvnMemoryPoolBuilder) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetDefaults");
    if !builder.is_null() {
        std::ptr::write_bytes(builder, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetDevice(
    builder: *mut NvnMemoryPoolBuilder,
    device: usize,
) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetDevice");
    if !builder.is_null() {
        (*builder).device = device;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetStorage(
    builder: *mut NvnMemoryPoolBuilder,
    memory: usize,
    size: usize,
) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetStorage");
    if !builder.is_null() {
        (*builder).memory = memory;
        (*builder).size = size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderSetFlags(
    builder: *mut NvnMemoryPoolBuilder,
    flags: i32,
) {
    record_hle_call("hle_nvnMemoryPoolBuilderSetFlags");
    if !builder.is_null() {
        (*builder).flags = flags;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetDevice(
    builder: *const NvnMemoryPoolBuilder,
) -> usize {
    record_hle_call("hle_nvnMemoryPoolBuilderGetDevice");
    if builder.is_null() {
        0
    } else {
        (*builder).device
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetMemory(
    builder: *const NvnMemoryPoolBuilder,
) -> usize {
    record_hle_call("hle_nvnMemoryPoolBuilderGetMemory");
    if builder.is_null() {
        0
    } else {
        (*builder).memory
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetSize(
    builder: *const NvnMemoryPoolBuilder,
) -> usize {
    record_hle_call("hle_nvnMemoryPoolBuilderGetSize");
    if builder.is_null() {
        0
    } else {
        (*builder).size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolBuilderGetFlags(
    builder: *const NvnMemoryPoolBuilder,
) -> i32 {
    record_hle_call("hle_nvnMemoryPoolBuilderGetFlags");
    if builder.is_null() {
        0
    } else {
        (*builder).flags
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolInitialize(
    pool: *mut NvnMemoryPool,
    builder: *const NvnMemoryPoolBuilder,
) -> u8 {
    let lr: usize;
    std::arch::asm!("mov {}, x30", out(reg) lr, options(nomem, nostack));
    let mut fp: usize;
    std::arch::asm!("mov {}, x29", out(reg) fp, options(nomem, nostack));
    record_hle_call("hle_nvnMemoryPoolInitialize");
    if pool.is_null() || builder.is_null() {
        return 0; // NVN_FALSE
    }
    let size = (*builder).size;
    if size == 0 {
        return 0;
    }
    let mut memory = (*builder).memory;
    let mut owns_memory = 0u8;
    if memory == 0 {
        // VIRTUAL pools (and any caller that didn't supply backing storage)
        // get real host memory of their own -- matches real NVN semantics
        // for a pool with no caller-supplied storage.
        let align = 0x10000usize; // NVN_DEVICE_INFO_MEMORY_POOL_PAGE_SIZE
        let mut out: *mut libc::c_void = std::ptr::null_mut();
        if libc::posix_memalign(&mut out, align, size) != 0 || out.is_null() {
            return 0;
        }
        libc::memset(out, 0, size);
        memory = out as usize;
        owns_memory = 1;
    }
    std::ptr::write(
        pool,
        NvnMemoryPool {
            memory,
            size,
            flags: (*builder).flags,
            _pad: 0,
            device: (*builder).device,
            owns_memory,
        },
    );
    {
        // Pools live until nvnMemoryPoolFinalize and may overlap: Kirby creates small shader pools inside the storage of
        // a live 1.4 GB pool that also holds vertex data (lookups take the innermost covering pool). Only a pool object
        // initialized again without Finalize replaces its own earlier registration.
        let mut pools = MEMORY_POOLS.lock().unwrap();
        if let Some(old) = POOL_OBJECTS.lock().unwrap().insert(pool as usize, memory) {
            if old != memory {
                pools.remove(&old);
                MEMORY_POOL_FLAGS.lock().unwrap().remove(&old);
            }
        }
        pools.insert(memory, size);
        let mut flags = MEMORY_POOL_FLAGS.lock().unwrap();
        flags.insert(memory, (*builder).flags as i32);
        bump_pools_gen();
    }
    println!(
        "[+] NVN: MemoryPool initialized: memory={:#x} size={} flags={:#x} (host-allocated={})",
        memory,
        size,
        (*builder).flags,
        owns_memory != 0
    );
    if size >= 64 << 20 {
        // Big pools (render targets, streaming): who sizes them (mods such as UltraCam enlarge some).
        let base = crate::cpu::crash_handler::get_guest_base();
        let mut chain = vec![format!("{:#x}", lr.wrapping_sub(base))];
        for _ in 0..10 {
            if fp < 0x1000 || fp & 7 != 0 {
                break;
            }
            let next = *(fp as *const usize);
            chain.push(format!(
                "{:#x}",
                (*((fp + 8) as *const usize)).wrapping_sub(base)
            ));
            if next <= fp {
                break;
            }
            fp = next;
        }
        println!(
            "[+] NVN:   pool of {} MB created from base+ {}",
            size >> 20,
            chain.join(" <- ")
        );
    }
    1 // NVN_TRUE
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolFinalize(pool: *mut NvnMemoryPool) {
    record_hle_call("hle_nvnMemoryPoolFinalize");
    if pool.is_null() {
        return;
    }
    let base = (*pool).memory;
    guest_memory_released(base, (*pool).size);
    POOL_OBJECTS.lock().unwrap().remove(&(pool as usize));
    MEMORY_POOLS.lock().unwrap().remove(&base);
    bump_pools_gen();
    if (*pool).owns_memory != 0 && (*pool).memory != 0 {
        libc::free((*pool).memory as *mut libc::c_void);
    }
    std::ptr::write_bytes(pool, 0, 1);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolMap(pool: *const NvnMemoryPool) -> usize {
    record_hle_call("hle_nvnMemoryPoolMap");
    // Real NVN only allows mapping pools without CPU_NO_ACCESS; we're always
    // host-memory-backed (no discrete VRAM to be inaccessible from), so just
    // hand back the real pointer.
    if pool.is_null() {
        0
    } else {
        (*pool).memory
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolGetBufferAddress(pool: *const NvnMemoryPool) -> u64 {
    record_hle_call("hle_nvnMemoryPoolGetBufferAddress");
    // No separate GPU virtual address space exists here -- the host pointer
    // *is* the "GPU address" by our own internal convention, consistent with
    // how our NVN implementation resolves texture/buffer storage elsewhere.
    if pool.is_null() {
        0
    } else {
        (*pool).memory as u64
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolSetDebugLabel(
    _pool: *mut NvnMemoryPool,
    _label: *const c_char,
) {
    record_hle_call("hle_nvnMemoryPoolSetDebugLabel");
    // Debug-only on real hardware too; no functional effect.
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolGetFlags(pool: *const NvnMemoryPool) -> i32 {
    record_hle_call("hle_nvnMemoryPoolGetFlags");
    if pool.is_null() {
        0
    } else {
        (*pool).flags
    }
}

/// NVNmappingRequest: maps `size` bytes of a VIRTUAL pool (at `virtual_offset`) onto PHYSICAL pool storage.
#[repr(C)]
pub struct NvnMappingRequest {
    pub physical_pool: *const NvnMemoryPool,
    pub physical_offset: i64,
    pub virtual_offset: i64,
    pub size: u64,
    pub storage_class: i32,
}

/// Virtual pool pages mapped onto physical pool memory, as (virtual start, size, physical start). Physical pool memory
/// is only 4 KB aligned, so the host pages cannot be remapped (16 KB pages); addresses are translated instead.
static VIRTUAL_MAPS: StdMutex<Vec<(usize, usize, usize)>> = StdMutex::new(Vec::new());
/// No virtual mappings exist (translate_virtual can skip the lock).
static VIRTUAL_MAPS_EMPTY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// The host address backing `addr`: inside a mapped range of a VIRTUAL pool, the physical memory it maps to.
pub fn translate_virtual(addr: usize) -> usize {
    if VIRTUAL_MAPS_EMPTY.load(std::sync::atomic::Ordering::Acquire) {
        return addr;
    }
    // Each thread keeps a copy of the mappings sorted by start, refreshed when the pool generation changes (every
    // mapping change bumps it): this runs several times per draw binding on the encoding threads, and the global lock
    // plus a linear scan of every mapping showed up in their profiles. Mappings never overlap (a new one splits what
    // was there), so the binary search finds the same one the scan did.
    thread_local! {
        static MAPS: std::cell::RefCell<(u64, Vec<(usize, usize, usize)>)> = const { std::cell::RefCell::new((0, Vec::new())) };
    }
    let gen = pools_gen();
    MAPS.with(|m| {
        let mut m = m.borrow_mut();
        if m.0 != gen {
            let mut v = VIRTUAL_MAPS.lock().unwrap().clone();
            v.sort_unstable_by_key(|e| e.0);
            *m = (gen, v);
        }
        let maps = &m.1;
        let i = maps.partition_point(|&(v, _, _)| v <= addr);
        match i.checked_sub(1).map(|i| maps[i]) {
            Some((v, s, p)) if addr < v + s => p + (addr - v),
            _ => addr,
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolMapVirtual(
    pool: *const NvnMemoryPool,
    num_requests: i32,
    requests: *const NvnMappingRequest,
) -> u8 {
    record_hle_call("hle_nvnMemoryPoolMapVirtual");
    if pool.is_null() || requests.is_null() || num_requests < 0 {
        return 0;
    }
    for i in 0..num_requests as usize {
        let r = &*requests.add(i);
        let (voff, size) = (r.virtual_offset as usize, r.size as usize);
        if voff + size > (*pool).size {
            return 0;
        }
        let vstart = (*pool).memory + voff;
        let mut maps = VIRTUAL_MAPS.lock().unwrap();
        // Split any existing mapping around the new range (a remap or an unmap replaces what was there).
        let mut kept = Vec::new();
        for &(v, s, p) in maps.iter() {
            if v + s <= vstart || v >= vstart + size {
                kept.push((v, s, p));
                continue;
            }
            if v < vstart {
                kept.push((v, vstart - v, p));
            }
            if v + s > vstart + size {
                let cut = vstart + size - v;
                kept.push((vstart + size, s - cut, p + cut));
            }
        }
        if !r.physical_pool.is_null() {
            kept.push((
                vstart,
                size,
                (*r.physical_pool).memory + r.physical_offset as usize,
            ));
        }
        *maps = kept;
        VIRTUAL_MAPS_EMPTY.store(maps.is_empty(), std::sync::atomic::Ordering::Release);
        bump_pools_gen();
        drop(maps);
        // Zero-copy buffers wrapping the old backing of this range are stale.
        guest_memory_released(vstart, size);
        println!(
            "[+] NVN: MapVirtual {:#x}+{:#x} -> {}",
            vstart,
            size,
            if r.physical_pool.is_null() {
                "unmapped".to_string()
            } else {
                format!(
                    "{:#x}",
                    (*r.physical_pool).memory + r.physical_offset as usize
                )
            }
        );
    }
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolFlushMappedRange(
    _pool: *const NvnMemoryPool,
    _offset: i64,
    _size: i64,
) -> u8 {
    record_hle_call("hle_nvnMemoryPoolFlushMappedRange");
    // Apple Silicon is a unified-memory architecture -- there's no CPU/GPU
    // cache-coherency gap to flush across, unlike the Tegra X1's split
    // pools. A no-op is the semantically *correct* behavior here, not a gap.
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolInvalidateMappedRange(
    _pool: *const NvnMemoryPool,
    _offset: i64,
    _size: i64,
) -> u8 {
    record_hle_call("hle_nvnMemoryPoolInvalidateMappedRange");
    1
}

// ---------------------------------------------------------------------------
// NVN CommandBuffer family.
//
// NVNcommandBuffer (0xA0 = 160 bytes) is opaque driver storage, same freedom
// as MemoryPool. Every CommandBuffer method takes the command buffer pointer
// as its first argument (x0), and the ~130 Bind*/Set*/Draw*/Clear*/Copy*/
// Dispatch* methods all return void in the real API -- NVN defers error
// reporting to a debug callback rather than synchronous return values, so a
// generic recorder is both safe (no return-value contract to violate, unlike
// the MemoryPoolInitialize bug) and faithful to how real NVN actually works.
//
// Scope for this pass: get the lifecycle and memory accounting exactly right
// (Initialize/BeginRecording/EndRecording/memory tracking -- these DO have
// real return values and real downstream consumers, like EndRecording's
// handle feeding into nvnQueueSubmitCommands), and record every drawing/
// state command into a real per-buffer list instead of dropping it, so
// nvnQueueSubmitCommands has real data to work with. Translating that
// recorded stream into actual Metal draw calls via the compositor channel is
// a separate, larger follow-up (it's the whole other half of the engine);
// this gets the NVN-facing contract correct without faking success.
#[repr(C)]
pub(crate) struct NvnCommandBuffer {
    device: usize,
    command_used: usize,
    command_size: usize,
    control_used: usize,
    control_size: usize,
    recording: u8,
    _pad: [u8; 7],
    ops: usize, // *mut Vec<RecordedNvnCommand>, heap-boxed
}

static CMD_HISTOGRAM: StdMutex<std::collections::BTreeMap<&'static str, u64>> =
    StdMutex::new(std::collections::BTreeMap::new());

fn dump_command_histogram(frame: u64) {
    let mut h = CMD_HISTOGRAM.lock().unwrap();
    let mut v: Vec<_> = h.iter().map(|(k, c)| (*c, *k)).collect();
    v.sort_by(|a, b| b.cmp(a));
    let top: Vec<String> = v
        .iter()
        .take(25)
        .map(|(c, k)| format!("{}={}", k, c))
        .collect();
    println!(
        "[+] NVN commands recorded through frame {}: {}",
        frame,
        top.join(" ")
    );
    h.clear();
}

pub(crate) struct RecordedNvnCommand {
    pub(crate) name: &'static str,
    /// Fully decoded form (pointer arguments copied, floats captured) for commands the Metal
    /// executor understands; None for the rest (recorded uninterpreted).
    pub(crate) typed: Option<crate::nvn::cmd::Cmd>,
    /// Raw arguments x1..x5 of uninterpreted commands (traces and the no-handler warning show what the game passes);
    /// boxed so decoded commands, the vast majority, stay small.
    pub(crate) args: Option<Box<[u64; 5]>>,
}

pub(crate) struct RecordedCommandList {
    pub(crate) ops: Vec<RecordedNvnCommand>,
    /// (ranges written by UpdateUniformBuffer, handles called), computed on first use (see metal::ubo_updated_ranges).
    /// Ranges its uniform updates and reports write, the lists it calls, its SAMPLES_PASSED report addresses.
    pub(crate) summary: std::sync::OnceLock<(Vec<(u64, u64)>, Vec<u64>, Vec<u64>)>,
    /// Frames presented when it was recorded (diagnostics: replayed lists vs. lists recorded this frame).
    pub(crate) recorded_frame: u64,
    /// Stamp of the last walk_command_lists that visited it.
    visit: std::sync::atomic::AtomicU64,
}

impl RecordedCommandList {
    pub(crate) fn summary(&self) -> &(Vec<(u64, u64)>, Vec<u64>, Vec<u64>) {
        self.summary.get_or_init(|| {
            let (mut ranges, mut calls, mut samples) = (Vec::new(), Vec::new(), Vec::new());
            for op in &self.ops {
                match &op.typed {
                    Some(crate::nvn::cmd::Cmd::UpdateUniformBuffer { addr, offset, data }) => {
                        let a = (*addr as i64 + *offset as i64) as u64;
                        ranges.push((a, a + data.len() as u64));
                    }
                    // Counter and value reports are written by the executor in stream order too (metal::write_counter).
                    Some(crate::nvn::cmd::Cmd::ReportCounter { addr, counter })
                        if *addr > 0x10000 =>
                    {
                        ranges.push((*addr, *addr + 16));
                        if *counter == 1 {
                            samples.push(*addr);
                        }
                    }
                    Some(crate::nvn::cmd::Cmd::ReportValue { addr, .. }) if *addr > 0x10000 => {
                        ranges.push((*addr, *addr + 16));
                    }
                    Some(crate::nvn::cmd::Cmd::CallCommands(h)) => {
                        calls.extend_from_slice(h.as_slice())
                    }
                    _ => {}
                }
            }
            (ranges, calls, samples)
        })
    }
}

/// The recorded list for `handle` without touching its eviction bookkeeping.
/// Visits every list reachable from `handles` through CallCommands once (under one lock: a submit reaches ~3,000 lists),
/// returning the number of distinct lists.
pub(crate) fn walk_command_lists(
    handles: &[u64],
    stack: &mut Vec<u64>,
    cache: &mut rustc_hash::FxHashMap<u64, std::sync::Arc<RecordedCommandList>>,
    mut f: impl FnMut(&RecordedCommandList),
) -> usize {
    static STAMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let stamp = STAMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let now = COMMAND_LIST_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut map = COMMAND_LISTS.lock().unwrap();
    stack.clear();
    stack.extend_from_slice(handles);
    let mut n = 0;
    while let Some(h) = stack.pop() {
        let Some(e) = map.get_mut(&h) else { continue };
        e.1 = now;
        if e.2 == LIST_NEW {
            e.2 = LIST_CALLED;
        }
        let l = e.0.clone();
        cache.insert(h, l.clone());
        if l.visit.swap(stamp, std::sync::atomic::Ordering::Relaxed) == stamp {
            continue;
        }
        n += 1;
        f(&l);
        stack.extend_from_slice(&l.summary().1);
    }
    n
}

// EndRecording handles -> finalized recorded-command snapshots. A command
// buffer gets reused across frames (BeginRecording clears it), so a snapshot
// has to be taken at EndRecording time rather than handing back a pointer
// into the live, mutable buffer.
// Handle -> list, bounded: handles can be re-submitted or CallCommands'd several times, so lists are
// not removed on use; the oldest are evicted instead of leaking forever.
/// Recorded command lists by handle. Value: (list, last use, eviction class, estimated bytes). Bounded by memory: past
/// the budget, lists are evicted by class, then least-recently-used:
///   LIST_DEAD       its command memory was handed out again (invalid in NVN too),
///   LIST_SUBMITTED  submitted to a queue and never called: the per-frame lists, one-shot in practice,
///   LIST_CALLED     run through CallCommands: TotK records each mesh's state/bind list once and calls it whenever the
///                   mesh is drawn, possibly after minutes off screen,
///   LIST_NEW        never executed yet (some are recorded at load time and first replayed much later).
/// Plain LRU evicted the per-mesh lists of everything out of view; looking back drew those meshes with the previous
/// draw's vertex buffers (geometry streaks that accumulated while moving the camera).
static COMMAND_LISTS: std::sync::LazyLock<
    StdMutex<rustc_hash::FxHashMap<u64, (std::sync::Arc<RecordedCommandList>, u64, u8, usize)>>,
> = std::sync::LazyLock::new(|| StdMutex::new(rustc_hash::FxHashMap::default()));
static COMMAND_LIST_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const COMMAND_LIST_BUDGET: usize = 512 << 20;
const LIST_DEAD: u8 = 0;
const LIST_SUBMITTED: u8 = 1;
const LIST_CALLED: u8 = 2;
const LIST_NEW: u8 = 3;
/// Diagnostics: class of each evicted list at eviction time (to tell which kind the game still replays).
static EVICTED_CLASS: std::sync::LazyLock<StdMutex<rustc_hash::FxHashMap<u64, u8>>> =
    std::sync::LazyLock::new(|| StdMutex::new(rustc_hash::FxHashMap::default()));
/// Class an evicted list had when it was evicted ("dead", "submitted", "called", "new").
pub(crate) fn evicted_list_class(handle: u64) -> &'static str {
    match EVICTED_CLASS.lock().unwrap().get(&handle) {
        Some(&LIST_DEAD) => "dead",
        Some(&LIST_SUBMITTED) => "submitted",
        Some(&LIST_CALLED) => "called",
        Some(_) => "new",
        None => "?",
    }
}

fn recorded_list_bytes(l: &RecordedCommandList) -> usize {
    l.ops.capacity() * std::mem::size_of::<RecordedNvnCommand>()
        + l.ops
            .iter()
            .map(|op| match &op.typed {
                Some(crate::nvn::cmd::Cmd::UpdateUniformBuffer { data, .. }) => data.len(),
                _ => 0,
            })
            .sum::<usize>()
}

/// Marks lists the queue executed directly (eviction prefers them).
pub(crate) fn mark_command_lists_submitted(handles: &[u64]) {
    let mut g = COMMAND_LISTS.lock().unwrap();
    for h in handles {
        if let Some(e) = g.get_mut(h) {
            if e.2 == LIST_NEW {
                e.2 = LIST_SUBMITTED;
            }
        }
    }
}
static COMMAND_LIST_CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Whether `handle` was ever returned by EndRecording (for diagnosing missing lists).
pub(crate) fn command_handle_issued(handle: u64) -> bool {
    handle != 0 && handle < NEXT_COMMAND_HANDLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The list for `handle`; `called`: run through CallCommands (not submitted to the queue).
pub(crate) fn command_list(
    handle: u64,
    called: bool,
) -> Option<std::sync::Arc<RecordedCommandList>> {
    let now = COMMAND_LIST_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut g = COMMAND_LISTS.lock().unwrap();
    let e = g.get_mut(&handle)?;
    e.1 = now;
    if called && e.2 != LIST_DEAD {
        e.2 = LIST_CALLED;
    } else if e.2 == LIST_NEW {
        e.2 = LIST_SUBMITTED;
    }
    Some(e.0.clone())
}

static NEXT_COMMAND_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferInitialize(
    cmd: *mut NvnCommandBuffer,
    device: usize,
) -> u8 {
    record_hle_call("hle_nvnCommandBufferInitialize");
    if cmd.is_null() {
        return 0;
    }
    std::ptr::write(
        cmd,
        NvnCommandBuffer {
            device,
            command_used: 0,
            command_size: 0,
            control_used: 0,
            control_size: 0,
            recording: 0,
            _pad: [0; 7],
            ops: Box::into_raw(Box::new(Vec::<RecordedNvnCommand>::new())) as usize,
        },
    );
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferFinalize(cmd: *mut NvnCommandBuffer) {
    record_hle_call("hle_nvnCommandBufferFinalize");
    if cmd.is_null() {
        return;
    }
    if (*cmd).ops != 0 {
        drop(Box::from_raw((*cmd).ops as *mut Vec<RecordedNvnCommand>));
    }
    std::ptr::write_bytes(cmd, 0, 1);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferAddCommandMemory(
    cmd: *mut NvnCommandBuffer,
    pool: *const NvnMemoryPool,
    offset: i64,
    size: usize,
) {
    record_hle_call("hle_nvnCommandBufferAddCommandMemory");
    if !cmd.is_null() {
        // Recording continues at the first byte of the new block; the queries describe only this block.
        (*cmd).command_size = size;
        (*cmd).command_used = 0;
        command_memory_reused(cmd as usize, (pool as usize, offset), size);
    }
}

/// NVN command memory, modelled as the driver uses it: a command buffer records into
/// the block added last, from its first byte, command sets one after another (BeginRecording does not rewind); the
/// GetCommandMemory* queries describe only that block. TotK measures its lists with those queries and sub-allocates
/// its command memory accordingly, so the byte range each list occupies here is the range the game believes it
/// occupies. A list stays valid until memory overlapping that range is added to a command buffer again; then it is
/// dead and the first to go when the list cache needs room. (Before, usage reset at every BeginRecording and sizes
/// summed over blocks, lists were matched by whole blocks, and the cache evicted lists the game still called: meshes
/// drawn with another mesh's vertex buffers.)
struct CommandMemory {
    /// Command buffer -> current block (pool, offset, size) and the block-relative offset its current recording began at.
    cur: rustc_hash::FxHashMap<usize, (usize, i64, usize, usize)>,
    /// (pool, start offset) -> (length, handle) of every recorded list not yet known dead.
    lists: std::collections::BTreeMap<(usize, i64), (usize, u64)>,
    max_len: usize,
}
static COMMAND_MEMORY: std::sync::LazyLock<StdMutex<CommandMemory>> =
    std::sync::LazyLock::new(|| {
        StdMutex::new(CommandMemory {
            cur: rustc_hash::FxHashMap::default(),
            lists: std::collections::BTreeMap::new(),
            max_len: 0,
        })
    });

fn command_memory_reused(cmd: usize, region: (usize, i64), size: usize) {
    // Adding a block writes nothing: lists in it die only when a new recording actually writes over them (TotK re-adds
    // a block to re-record its first display list and keeps calling the one recorded after it in the same block).
    COMMAND_MEMORY
        .lock()
        .unwrap()
        .cur
        .insert(cmd, (region.0, region.1, size, 0));
}

/// Lists overlapping [start, start + len) of `pool` are dead: a new recording wrote over them.
fn kill_overlapping_lists(m: &mut CommandMemory, pool: usize, start: i64, len: usize, by: u64) {
    let end = start + len as i64;
    let from = start - m.max_len as i64;
    let hit: Vec<((usize, i64), usize, u64)> = m
        .lists
        .range((pool, from)..(pool, end))
        .filter(|(&(_, o), &(l, h))| o + l as i64 > start && h != by)
        .map(|(k, &(l, h))| (*k, l, h))
        .collect();
    if hit.is_empty() {
        return;
    }
    // Removed right away (they cannot run validly any more): keeping them until the cache budget forced an eviction
    // held ~480 MB of dead lists, and each eviction then sorted ~48k entries and freed 128 MB under COMMAND_LISTS,
    // blocking the Presentation Thread's list lookups (~118 evictions in 150 s in Hateno). Freed after the lock.
    let mut freed: Vec<std::sync::Arc<RecordedCommandList>> = Vec::with_capacity(hit.len());
    let mut bytes = 0usize;
    {
        let mut g = COMMAND_LISTS.lock().unwrap();
        for (k, _, h) in &hit {
            m.lists.remove(k);
            if let Some(e) = g.remove(h) {
                bytes += e.3;
                freed.push(e.0);
            }
        }
    }
    COMMAND_LIST_BYTES.fetch_sub(bytes, std::sync::atomic::Ordering::Relaxed);
    let mut ev = EVICTED_CLASS.lock().unwrap();
    if ev.len() > 1 << 20 {
        ev.clear();
    }
    for (_, _, h) in hit {
        ev.insert(h, LIST_DEAD);
    }
    drop(ev);
    drop(freed);
}

/// Handles by command memory location (pool, offset) of the command set (see EndRecording).
static LOCATION_HANDLES: std::sync::LazyLock<StdMutex<rustc_hash::FxHashMap<(usize, i64), u64>>> =
    std::sync::LazyLock::new(|| StdMutex::new(rustc_hash::FxHashMap::default()));

/// Where the command set being ended starts in command memory, if the command buffer has a block.
fn command_list_location(cmd: usize) -> Option<(usize, i64)> {
    let m = COMMAND_MEMORY.lock().unwrap();
    m.cur
        .get(&cmd)
        .map(|&(pool, off, size, begin)| (pool, off + begin.min(size) as i64))
}

/// BeginRecording: the new command set starts where the block's previous one ended.
fn command_recording_begun(cmd: usize, used: usize) {
    let mut m = COMMAND_MEMORY.lock().unwrap();
    if let Some(c) = m.cur.get_mut(&cmd) {
        c.3 = used;
    }
}

/// EndRecording: the list occupies [begin, used) of the current block (clamped to it: the game believes it fits), and
/// any older list it overlaps has been overwritten.
fn command_list_recorded(cmd: usize, handle: u64, used: usize) {
    let mut m = COMMAND_MEMORY.lock().unwrap();
    if let Some(&(pool, off, size, begin)) = m.cur.get(&cmd) {
        let begin = begin.min(size);
        let len = used.min(size).saturating_sub(begin).max(1);
        let start = off + begin as i64;
        kill_overlapping_lists(&mut m, pool, start, len, handle);
        m.max_len = m.max_len.max(len);
        m.lists.insert((pool, start), (len, handle));
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferAddControlMemory(
    cmd: *mut NvnCommandBuffer,
    _pool: *const NvnMemoryPool,
    _offset: i64,
    size: usize,
) {
    record_hle_call("hle_nvnCommandBufferAddControlMemory");
    if !cmd.is_null() {
        (*cmd).control_size = size;
        (*cmd).control_used = 0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBeginRecording(cmd: *mut NvnCommandBuffer) {
    record_hle_call("hle_nvnCommandBufferBeginRecording");
    if cmd.is_null() {
        return;
    }
    (*cmd).recording = 1;
    // Command sets follow each other in the current block: no rewind.
    command_recording_begun(cmd as usize, (*cmd).command_used);
    if (*cmd).ops != 0 {
        let v = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
        v.clear();
        if v.capacity() < 2048 {
            v.reserve(2048);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferEndRecording(cmd: *mut NvnCommandBuffer) -> u64 {
    record_hle_call("hle_nvnCommandBufferEndRecording");
    if cmd.is_null() || (*cmd).ops == 0 {
        return 0;
    }
    (*cmd).recording = 0;
    let ops_vec = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
    {
        // Debug aid: per-name command counts, dumped periodically by the present path.
        let mut h = CMD_HISTOGRAM.lock().unwrap();
        for op in ops_vec.iter() {
            *h.entry(op.name).or_insert(0) += 1;
        }
    }
    let snapshot = Box::new(RecordedCommandList {
        // Exactly sized: the recording buffer keeps its reserved capacity (2048+ commands) for the next recording. Taking
        // the Vec itself stored that capacity with every list (~2048 x size_of::<RecordedNvnCommand>() per list, however
        // short) and made the next BeginRecording allocate it again.
        ops: ops_vec.drain(..).collect(),
        summary: std::sync::OnceLock::new(),
        recorded_frame: crate::gpu::backend().presented_frames(),
        visit: std::sync::atomic::AtomicU64::new(0),
    });
    // Here on the recording thread rather than on the Presentation Thread at submit (it scans every op).
    snapshot.summary();
    // An NVN command handle is the GPU address of the command set: recording again at the same place in command memory
    // yields the same handle, and the game may keep calling the one it stored first. Same location, same handle here.
    let handle = match command_list_location(cmd as usize) {
        Some(loc) => *LOCATION_HANDLES
            .lock()
            .unwrap()
            .entry(loc)
            .or_insert_with(|| {
                NEXT_COMMAND_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }),
        None => NEXT_COMMAND_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    };
    {
        let now = COMMAND_LIST_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let bytes = recorded_list_bytes(&snapshot);
        let mut g = COMMAND_LISTS.lock().unwrap();
        let old = g.insert(
            handle,
            (std::sync::Arc::from(snapshot), now, LIST_NEW, bytes),
        );
        drop(g);
        if let Some(old) = old {
            COMMAND_LIST_BYTES.fetch_sub(old.3, std::sync::atomic::Ordering::Relaxed);
            drop(old); // the replaced list is freed outside the lock
        }
        command_list_recorded(cmd as usize, handle, (*cmd).command_used);
        let mut g = COMMAND_LISTS.lock().unwrap();
        let total =
            COMMAND_LIST_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed) + bytes;
        if total > COMMAND_LIST_BUDGET {
            // Evict down to 3/4 of the budget: by class (see COMMAND_LISTS), oldest first within a class.
            let mut order: Vec<(u8, u64, u64, usize)> =
                g.iter().map(|(h, e)| (e.2, e.1, *h, e.3)).collect();
            order.sort_unstable();
            let mut per_class = [(0usize, 0usize); 4];
            for &(c, _, _, b) in &order {
                per_class[c as usize].0 += 1;
                per_class[c as usize].1 += b;
            }
            println!("[+] NVN: list cache by class (count, MB): dead {:?} submitted {:?} called {:?} new {:?}",
                (per_class[0].0, per_class[0].1 >> 20), (per_class[1].0, per_class[1].1 >> 20), (per_class[2].0, per_class[2].1 >> 20), (per_class[3].0, per_class[3].1 >> 20));
            let mut freed = 0usize;
            let mut gone = Vec::new();
            let mut ev = EVICTED_CLASS.lock().unwrap();
            if ev.len() > 1 << 20 {
                ev.clear();
            }
            for (c, _, h, b) in order {
                if total - freed <= COMMAND_LIST_BUDGET / 4 * 3 || h == handle {
                    break;
                }
                if let Some(e) = g.remove(&h) {
                    gone.push(e.0);
                }
                ev.insert(h, c);
                freed += b;
            }
            drop(ev);
            COMMAND_LIST_BYTES.fetch_sub(freed, std::sync::atomic::Ordering::Relaxed);
            println!(
                "[+] NVN: command list cache over {} MB: evicted {} MB (now {} lists)",
                COMMAND_LIST_BUDGET >> 20,
                freed >> 20,
                g.len()
            );
            drop(g);
            drop(gone); // freed outside the lock
        }
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferIsRecording(cmd: *const NvnCommandBuffer) -> u8 {
    record_hle_call("hle_nvnCommandBufferIsRecording");
    if cmd.is_null() {
        0
    } else {
        (*cmd).recording
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCommandMemoryUsed(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetCommandMemoryUsed");
    if cmd.is_null() {
        0
    } else {
        (*cmd).command_used
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCommandMemoryFree(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetCommandMemoryFree");
    if cmd.is_null() {
        0
    } else {
        (*cmd).command_size.saturating_sub((*cmd).command_used)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCommandMemorySize(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetCommandMemorySize");
    if cmd.is_null() {
        0
    } else {
        (*cmd).command_size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetControlMemoryUsed(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetControlMemoryUsed");
    if cmd.is_null() {
        0
    } else {
        (*cmd).control_used
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetControlMemoryFree(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetControlMemoryFree");
    if cmd.is_null() {
        0
    } else {
        (*cmd).control_size.saturating_sub((*cmd).control_used)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetControlMemorySize(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetControlMemorySize");
    if cmd.is_null() {
        0
    } else {
        (*cmd).control_size
    }
}

// Generic recorder for every void-returning Bind*/Set*/Draw*/Clear*/Copy*/
// Dispatch*/Push*/Pop*/Insert*/Report*/Wait*/Signal* method (~130 names, see
// the dispatch table below). x0 is always the command buffer; x1-x5 are
// recorded uninterpreted for whichever follow-up pass adds real Metal
// translation. `name` is baked in per-registration via a thin per-name
// wrapper generated by the macro below, so logging/debugging can tell them
// apart.
unsafe fn record_generic(
    name: &'static str,
    cmd: *mut NvnCommandBuffer,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) {
    record_hle_call(name);
    drawable_cmd_probe(name);
    if cmd.is_null() || (*cmd).ops == 0 {
        return;
    }
    // Settings of the command buffer object itself, not commands: they write no command memory. Counting them made
    // TotK's re-recorded display lists longer than the slots it sized for them, so they seemed to overwrite the next list.
    if matches!(
        name,
        "nvnCommandBufferSetMemoryCallback"
            | "nvnCommandBufferSetMemoryCallbackData"
            | "nvnCommandBufferSetCommandMemoryCallbackEnabled"
            | "nvnCommandBufferSetControlMemoryCallbackEnabled"
            | "nvnCommandBufferSetDebugLabel"
    ) {
        return;
    }
    let ops_vec = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
    ops_vec.push(RecordedNvnCommand {
        name,
        typed: None,
        args: Some(Box::new([a1, a2, a3, a4, a5])),
    });
    (*cmd).command_used += 32; // rough per-command footprint, just needs to trend upward plausibly
}

/// --log-drawables: counts the command against the drawable being drawn on this thread (gfx_cmd).
fn drawable_cmd_probe(name: &'static str) {
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| crate::settings::get().totk.diagnostics.log_drawables) {
        let (obj, func) = crate::nvn::cmd::current_drawable();
        if obj != 0 {
            crate::nvn::cmd::drawable_cmd_stats(func, Some(name));
        }
    }
}

/// Record a decoded command (see nvn/cmd.rs). `cmd` is the NvnCommandBuffer address.
pub(crate) unsafe fn record_typed(name: &'static str, cmd: usize, typed: crate::nvn::cmd::Cmd) {
    record_hle_call(name);
    drawable_cmd_probe(name);
    let cmd = cmd as *mut NvnCommandBuffer;
    if cmd.is_null() || (*cmd).ops == 0 {
        return;
    }
    let ops_vec = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
    ops_vec.push(RecordedNvnCommand {
        name,
        typed: Some(typed),
        args: None,
    });
    (*cmd).command_used += 32;
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
    hle_nvnCommandBufferSetAlphaRef => "nvnCommandBufferSetAlphaRef",
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
    hle_nvnCommandBufferSetLineWidth => "nvnCommandBufferSetLineWidth",
    hle_nvnCommandBufferSetMemoryCallback => "nvnCommandBufferSetMemoryCallback",
    hle_nvnCommandBufferSetMemoryCallbackData => "nvnCommandBufferSetMemoryCallbackData",
    hle_nvnCommandBufferSetOuterTessellationLevels => "nvnCommandBufferSetOuterTessellationLevels",
    hle_nvnCommandBufferSetPatchSize => "nvnCommandBufferSetPatchSize",
    hle_nvnCommandBufferSetPointSize => "nvnCommandBufferSetPointSize",
    hle_nvnCommandBufferSetPolygonOffsetClamp => "nvnCommandBufferSetPolygonOffsetClamp",
    hle_nvnCommandBufferSetProgramSubroutines => "nvnCommandBufferSetProgramSubroutines",
    hle_nvnCommandBufferSetProvokingVertexMode => "nvnCommandBufferSetProvokingVertexMode",
    hle_nvnCommandBufferSetRasterizerDiscard => "nvnCommandBufferSetRasterizerDiscard",
    hle_nvnCommandBufferSetRenderEnable => "nvnCommandBufferSetRenderEnable",
    hle_nvnCommandBufferSetRenderEnableConditional => "nvnCommandBufferSetRenderEnableConditional",
    hle_nvnCommandBufferSetSampleMask => "nvnCommandBufferSetSampleMask",
    hle_nvnCommandBufferSetSamplerPool => "nvnCommandBufferSetSamplerPool",
    hle_nvnCommandBufferSetShaderScratchMemory => "nvnCommandBufferSetShaderScratchMemory",
    hle_nvnCommandBufferSetStencilCullCriteria => "nvnCommandBufferSetStencilCullCriteria",
    hle_nvnCommandBufferSetStencilMask => "nvnCommandBufferSetStencilMask",
    hle_nvnCommandBufferSetStencilRef => "nvnCommandBufferSetStencilRef",
    hle_nvnCommandBufferSetStencilValueMask => "nvnCommandBufferSetStencilValueMask",
    hle_nvnCommandBufferSetSubpixelPrecisionBias => "nvnCommandBufferSetSubpixelPrecisionBias",
    hle_nvnCommandBufferSetTexturePool => "nvnCommandBufferSetTexturePool",
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

// GetMemoryCallback/GetMemoryCallbackData are pure getters over state nothing
// sets yet (SetMemoryCallback/SetMemoryCallbackData are no-op recorders above
// since we don't invoke memory callbacks) -- always null is correct here.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetMemoryCallback() -> usize {
    record_hle_call("hle_nvnCommandBufferGetMemoryCallback");
    0
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetMemoryCallbackData() -> usize {
    record_hle_call("hle_nvnCommandBufferGetMemoryCallbackData");
    0
}

// GetCopyRowStride/GetCopyImageStride: real NVN defaults (0 = driver picks
// the tightest valid stride); nothing has set a custom stride since
// SetCopyRowStride/SetCopyImageStride are no-op recorders above.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCopyRowStride() -> usize {
    record_hle_call("hle_nvnCommandBufferGetCopyRowStride");
    0
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCopyImageStride() -> usize {
    record_hle_call("hle_nvnCommandBufferGetCopyImageStride");
    0
}

// ---------------------------------------------------------------------------
// NVN Buffer family. NVNbuffer is 0x30 (48) bytes, NVNbufferBuilder is 0x40
// (64) bytes -- both opaque, same freedom as MemoryPool/CommandBuffer. This
// was the single highest-volume unimplemented call this session
// (nvnBufferGetAddress: 4365 calls, always returning 0) -- almost certainly
// why guest code elsewhere was dereferencing a null "buffer address" field.
#[repr(C)]
pub(crate) struct NvnBufferBuilder {
    device: usize,
    pool: usize, // *const NvnMemoryPool
    offset: i64,
    size: usize,
}

#[repr(C)]
pub(crate) struct NvnBuffer {
    address: u64, // our internal "GPU address" convention -- same host pointer value used throughout (see MemoryPoolGetBufferAddress)
    map_ptr: usize,
    size: usize,
    pool: usize,
    offset: i64,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderSetDefaults(b: *mut NvnBufferBuilder) {
    record_hle_call("hle_nvnBufferBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderSetDevice(b: *mut NvnBufferBuilder, device: usize) {
    record_hle_call("hle_nvnBufferBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderSetStorage(
    b: *mut NvnBufferBuilder,
    pool: usize,
    offset: i64,
    size: usize,
) {
    record_hle_call("hle_nvnBufferBuilderSetStorage");
    if !b.is_null() {
        (*b).pool = pool;
        (*b).offset = offset;
        (*b).size = size;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetSize(b: *const NvnBufferBuilder) -> usize {
    record_hle_call("hle_nvnBufferBuilderGetSize");
    if b.is_null() {
        0
    } else {
        (*b).size
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetMemoryOffset(b: *const NvnBufferBuilder) -> i64 {
    record_hle_call("hle_nvnBufferBuilderGetMemoryOffset");
    if b.is_null() {
        0
    } else {
        (*b).offset
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetMemoryPool(b: *const NvnBufferBuilder) -> usize {
    record_hle_call("hle_nvnBufferBuilderGetMemoryPool");
    // returns the NVNmemoryPool* passed to SetStorage (returning the struct by value made the caller read x0
    // while we wrote through x8).
    if b.is_null() {
        0
    } else {
        (*b).pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferInitialize(
    buf: *mut NvnBuffer,
    builder: *const NvnBufferBuilder,
) -> u8 {
    record_hle_call("hle_nvnBufferInitialize");
    if buf.is_null() || builder.is_null() || (*builder).pool == 0 {
        return 0;
    }
    let pool = &*((*builder).pool as *const NvnMemoryPool);
    let ptr = translate_virtual(pool.memory + (*builder).offset as usize);
    std::ptr::write(
        buf,
        NvnBuffer {
            address: ptr as u64,
            map_ptr: ptr,
            size: (*builder).size,
            pool: (*builder).pool,
            offset: (*builder).offset,
        },
    );
    1
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferFinalize(buf: *mut NvnBuffer) {
    record_hle_call("hle_nvnBufferFinalize");
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetAddress(buf: *const NvnBuffer) -> u64 {
    record_hle_call("hle_nvnBufferGetAddress");
    if buf.is_null() {
        0
    } else {
        (*buf).address
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferMap(buf: *const NvnBuffer) -> usize {
    record_hle_call("hle_nvnBufferMap");
    if buf.is_null() {
        0
    } else {
        (*buf).map_ptr
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetSize(buf: *const NvnBuffer) -> usize {
    record_hle_call("hle_nvnBufferGetSize");
    if buf.is_null() {
        0
    } else {
        (*buf).size
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferFlushMappedRange(
    _buf: *const NvnBuffer,
    _offset: i64,
    _size: i64,
) {
    record_hle_call("hle_nvnBufferFlushMappedRange");
    // Unified memory architecture -- correct no-op, see MemoryPoolFlushMappedRange.
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferInvalidateMappedRange(
    _buf: *const NvnBuffer,
    _offset: i64,
    _size: i64,
) {
    record_hle_call("hle_nvnBufferInvalidateMappedRange");
}

// ---------------------------------------------------------------------------
// NVN Texture family. NVNtexture and NVNtextureBuilder are both 0x80 bytes.
// Storage size, alignment and view offsets come from nvn_layout (Tegra block-linear,
// validated against the toolchain's BNTX output): the game memcpy's pre-swizzled
// data to the offsets reported here, so they must be exact. An earlier flat
// 4-bytes-per-pixel estimate asked the allocator for ~700MB for one texture.
#[repr(C)]
pub struct NvnTextureBuilder {
    pub device: usize,
    pub target: i32,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub levels: i32,
    pub format: i32,
    pub flags: i32,
    pub samples: i32,
    pub stride: i64,
    pub pool: usize,
    pub pool_offset: i64,
    /// nvnTextureBuilderSetSwizzle (NVN_TEXTURE_SWIZZLE_*: 0 zero, 1 one, 2..5 r, g, b, a).
    pub swizzle: [u8; 4],
    /// nvnTextureBuilderSetDepthStencilMode (0 depth, 1 stencil); only reported back, not yet used for sampling.
    pub depth_stencil_mode: i32,
    /// nvnTextureBuilderSetPackagedTextureLayout: the texture packager's layout descriptor (NVNpackagedTextureLayout,
    /// 8 bytes) for packaged texture data, and whether one was set.
    pub packaged_layout: [u8; 8],
    pub has_packaged_layout: u8,
}
const _: () = assert!(std::mem::size_of::<NvnTextureBuilder>() <= 0x80);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct NvnTexture {
    pub device: usize,
    pub target: i32,
    pub width: i32,
    pub height: i32,
    pub depth: i32,
    pub levels: i32,
    pub format: i32,
    pub flags: i32,
    pub samples: i32,
    pub stride: i64,
    pub pool: usize,
    pub pool_offset: i64,
    pub storage: usize,
    pub size: usize,
    /// The builder's swizzle; views compose theirs on top (see metal::translated::texture_for_binding).
    pub swizzle: [u8; 4],
    pub _reserved: [u8; 44],
}

const _: () = assert!(std::mem::size_of::<NvnTexture>() == 0x80);

fn builder_layout_params(b: &NvnTextureBuilder) -> crate::nvn::layout::TextureParams {
    crate::nvn::layout::TextureParams {
        target: b.target,
        width: b.width,
        height: b.height,
        depth: b.depth,
        levels: b.levels,
        format: b.format,
        flags: b.flags,
        samples: b.samples,
        stride: b.stride,
    }
}

fn texture_layout_params(t: &NvnTexture) -> crate::nvn::layout::TextureParams {
    crate::nvn::layout::TextureParams {
        target: t.target,
        width: t.width,
        height: t.height,
        depth: t.depth,
        levels: t.levels,
        format: t.format,
        flags: t.flags,
        samples: t.samples,
        stride: t.stride,
    }
}

unsafe fn texture_builder_storage_size(b: *const NvnTextureBuilder) -> usize {
    if b.is_null() {
        return 0;
    }
    crate::nvn::layout::storage_size(&builder_layout_params(&*b))
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetPackagedTextureLayout(
    b: *mut NvnTextureBuilder,
    layout: *const [u8; 8],
) {
    record_hle_call("hle_nvnTextureBuilderSetPackagedTextureLayout");
    if b.is_null() {
        return;
    }
    if layout.is_null() {
        (*b).has_packaged_layout = 0;
    } else {
        (*b).packaged_layout = *layout;
        (*b).has_packaged_layout = 1;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDefaults(b: *mut NvnTextureBuilder) {
    record_hle_call("hle_nvnTextureBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
        (*b).levels = 1;
        (*b).depth = 1;
        (*b).samples = 1;
        (*b).swizzle = [2, 3, 4, 5];
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDevice(b: *mut NvnTextureBuilder, device: usize) {
    record_hle_call("hle_nvnTextureBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetTarget(b: *mut NvnTextureBuilder, target: i32) {
    record_hle_call("hle_nvnTextureBuilderSetTarget");
    if !b.is_null() {
        (*b).target = target;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetWidth(b: *mut NvnTextureBuilder, width: i32) {
    record_hle_call("hle_nvnTextureBuilderSetWidth");
    if !b.is_null() {
        (*b).width = width;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetHeight(b: *mut NvnTextureBuilder, height: i32) {
    record_hle_call("hle_nvnTextureBuilderSetHeight");
    if !b.is_null() {
        (*b).height = height;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDepth(b: *mut NvnTextureBuilder, depth: i32) {
    record_hle_call("hle_nvnTextureBuilderSetDepth");
    if !b.is_null() {
        (*b).depth = depth;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetLevels(b: *mut NvnTextureBuilder, levels: i32) {
    record_hle_call("hle_nvnTextureBuilderSetLevels");
    if !b.is_null() {
        (*b).levels = levels;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetFormat(b: *mut NvnTextureBuilder, format: i32) {
    record_hle_call("hle_nvnTextureBuilderSetFormat");
    if !b.is_null() {
        (*b).format = format;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetFlags(b: *mut NvnTextureBuilder, flags: i32) {
    record_hle_call("hle_nvnTextureBuilderSetFlags");
    if !b.is_null() {
        (*b).flags = flags;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSamples(b: *mut NvnTextureBuilder, samples: i32) {
    record_hle_call("hle_nvnTextureBuilderSetSamples");
    if !b.is_null() {
        (*b).samples = samples;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetDepthStencilMode(
    _b: *mut NvnTextureBuilder,
    _mode: i32,
) {
    if !_b.is_null() {
        (*_b).depth_stencil_mode = _mode;
    }
    record_hle_call("hle_nvnTextureBuilderSetDepthStencilMode");
    // Selects which aspect (depth vs stencil) a combined depth/stencil
    // texture samples as -- affects shader reads, not storage layout, so
    // irrelevant until real Metal translation exists (same as SetSwizzle).
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
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSwizzle(
    b: *mut NvnTextureBuilder,
    r: i32,
    g: i32,
    bl: i32,
    a: i32,
) {
    record_hle_call("hle_nvnTextureBuilderSetSwizzle");
    if !b.is_null() {
        (*b).swizzle = [r as u8, g as u8, bl as u8, a as u8];
    }
    log_swizzle("SetSwizzle", -1, [r as u8, g as u8, bl as u8, a as u8]);
    if [r, g, bl, a].iter().any(|&c| !(0..=5).contains(&c))
        && crate::settings::get().log.log_swizzle
    {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 3 {
            let fp: usize;
            std::arch::asm!("mov {}, x29", out(reg) fp);
            println!("[swizzle] out-of-range SetSwizzle({}, {}, {}, {}) raw args {:#x} {:#x} {:#x} {:#x} callers {}", r, g, bl, a, r as u32, g as u32, bl as u32, a as u32, guest_chain(fp));
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetStride(b: *mut NvnTextureBuilder, stride: i64) {
    record_hle_call("hle_nvnTextureBuilderSetStride");
    if !b.is_null() {
        (*b).stride = stride;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetStorage(
    b: *mut NvnTextureBuilder,
    pool: usize,
    offset: i64,
) {
    record_hle_call("hle_nvnTextureBuilderSetStorage");
    if !b.is_null() {
        (*b).pool = pool;
        (*b).pool_offset = offset;
    }
}
#[no_mangle]
/// nvnTexture(Builder)GetZCullStorageSize: memory for nvnCommandBufferSave/RestoreZCullData. ZCull (the hardware's
/// coarse depth-culling cache) has no Metal counterpart and those commands are not executed, so nothing is needed.
pub unsafe extern "C" fn hle_nvnZCullStorageSize(_obj: *const u8) -> usize {
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStorageSize(b: *const NvnTextureBuilder) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetStorageSize");
    texture_builder_storage_size(b)
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStorageAlignment(
    b: *const NvnTextureBuilder,
) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetStorageAlignment");
    crate::nvn::layout::storage_alignment(if b.is_null() { 0 } else { (*b).flags })
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStorageClass() -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetStorageClass");
    0 // NVN_STORAGE_CLASS_POOL -- backed by an explicit memory pool, not device-internal storage
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureInitialize(
    tex: *mut NvnTexture,
    builder: *const NvnTextureBuilder,
) -> u8 {
    let lr: usize;
    std::arch::asm!("mov {}, x30", out(reg) lr);
    let base = crate::cpu::crash_handler::get_guest_base();
    let rel_lr = if lr >= base && base != 0 {
        lr - base
    } else {
        lr
    };
    // --log-tex-init: every texture initialization (with size and storage); off by default (TotK re-initializes
    // texture objects constantly).
    static LOG_INIT: OnceLock<bool> = OnceLock::new();
    let log_init = *LOG_INIT.get_or_init(|| crate::settings::get().log.log_tex_init);
    record_hle_call("hle_nvnTextureInitialize");
    if tex.is_null() || builder.is_null() {
        return 0;
    }
    if log_init {
        let b = &*builder;
        println!("[tex-init] tex={:p} {}x{}x{} lv{} fmt {} flags {:#x} target {} pool {:#x}+{:#x} size {:#x} layout {} caller=base+{:#x} thread {}", tex, b.width, b.height, b.depth, b.levels, b.format, b.flags, b.target, b.pool, b.pool_offset, texture_builder_storage_size(builder),
            if b.has_packaged_layout != 0 { format!("{:02x?}", b.packaged_layout) } else { "-".into() }, rel_lr, std::thread::current().name().unwrap_or("?"));
    }
    let storage = if (*builder).pool != 0 {
        let pool = &*((*builder).pool as *const NvnMemoryPool);
        translate_virtual(pool.memory + (*builder).pool_offset as usize)
    } else {
        0
    };
    std::ptr::write(
        tex,
        NvnTexture {
            device: (*builder).device,
            target: (*builder).target,
            width: (*builder).width,
            height: (*builder).height,
            depth: (*builder).depth.max(1),
            levels: (*builder).levels.max(1),
            format: (*builder).format,
            flags: (*builder).flags,
            samples: (*builder).samples.max(1),
            stride: (*builder).stride,
            pool: (*builder).pool,
            pool_offset: (*builder).pool_offset,
            storage,
            size: texture_builder_storage_size(builder),
            swizzle: if (*builder).swizzle == [0; 4] {
                [2, 3, 4, 5]
            } else {
                (*builder).swizzle
            },
            _reserved: [0; 44],
        },
    );
    log_swizzle("Initialize", (*tex).format, (*tex).swizzle);
    snapshot_texture(tex as usize, std::ptr::read(tex));
    // A pending upload belonged to the object's previous texture (the game wrote / flushed texels into that one):
    // TotK re-initializes texture objects for other textures constantly, render targets included, and a mark carried
    // over made the next resolve upload guest memory over a render target the GPU had just drawn (one-frame magenta
    // tiles over an unlit scene, 2026-10-13). The new texture's initial contents come from its creation path.
    DIRTY_TEXTURES.lock().unwrap().remove(&(tex as usize));
    1
}

/// Debug: --log-swizzle prints each distinct (call, format, swizzle) once.
fn log_swizzle(what: &str, format: i32, sw: [u8; 4]) {
    static ON: OnceLock<bool> = OnceLock::new();
    if !*ON.get_or_init(|| crate::settings::get().log.log_swizzle) {
        return;
    }
    static SEEN: StdMutex<Vec<(String, i32, [u8; 4])>> = StdMutex::new(Vec::new());
    let k = (what.to_string(), format, sw);
    let mut seen = SEEN.lock().unwrap();
    if !seen.contains(&k) {
        println!("[swizzle] {} format {} swizzle {:?}", what, format, sw);
        seen.push(k);
    }
}

// What the executor needs of an NVNtexture, captured when the game initializes it. The game may finalize or
// re-initialize the object after recording a command that names it (real NVN copies what it needs at record time),
// so the executor must not read the live struct. A new Initialize at the same address bumps the generation, which
// makes the executor drop its Metal texture and build a new one.
static TEX_SNAPSHOTS: std::sync::Mutex<Option<rustc_hash::FxHashMap<usize, (u64, NvnTexture)>>> =
    std::sync::Mutex::new(None);
static TEX_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
/// Bumped after every change to TEX_SNAPSHOTS or POOL_TEXTURES: a texture binding resolved while it had some value still
/// resolves the same way while it keeps it (metal::translated::bound_texture's fast path). Read before the lookups it
/// guards, bumped after the change: a race only costs a slow-path check.
static TEX_TABLE_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn tex_table_epoch() -> u64 {
    TEX_TABLE_EPOCH.load(std::sync::atomic::Ordering::Acquire)
}

fn bump_tex_table_epoch() {
    TEX_TABLE_EPOCH.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

fn snapshot_texture(addr: usize, t: NvnTexture) {
    let gen = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let old = TEX_SNAPSHOTS
        .lock()
        .unwrap()
        .get_or_insert_with(Default::default)
        .insert(addr, (gen, t));
    if let Some((og, ot)) = old {
        remember_snapshot(og, ot);
    }
    bump_tex_table_epoch();
    let mut idx = TEX_BY_STORAGE.lock().unwrap();
    let idx = idx.get_or_insert_with(Default::default);
    if let Some((_, o)) = old {
        unindex_texture(idx, addr, o.storage);
    }
    if t.storage > 0x10000 {
        idx.entry(t.storage).or_default().push(addr);
        // Bound for the alias search (a container starts at most its own size below a texture it contains).
        let span = (t.size as u64)
            .max(crate::nvn::layout::compute_layout(&texture_layout_params(&t)).total);
        MAX_TEXTURE_SPAN.fetch_max(span as usize, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Snapshots replaced at their address, by generation (bounded): commands recorded while an object described one
/// texture still name that texture after the game re-initializes or reuses the object (resolve_texture_ref).
const SNAPSHOT_HISTORY: usize = 1 << 16;
/// Generations are keyed by their low 32 bits, as commands record them.
struct SnapshotHistory {
    by_gen: rustc_hash::FxHashMap<u32, NvnTexture>,
    order: std::collections::VecDeque<u32>,
    /// Stable objects standing in for a replaced snapshot, by generation (their addresses are executor texture keys).
    copies: rustc_hash::FxHashMap<u32, Box<NvnTexture>>,
}
static TEX_HISTORY: std::sync::Mutex<Option<SnapshotHistory>> = std::sync::Mutex::new(None);

fn remember_snapshot(gen: u64, t: NvnTexture) {
    let mut h = TEX_HISTORY.lock().unwrap();
    let h = h.get_or_insert_with(|| SnapshotHistory {
        by_gen: Default::default(),
        order: Default::default(),
        copies: Default::default(),
    });
    h.by_gen.insert(gen as u32, t);
    h.order.push_back(gen as u32);
    if h.order.len() > SNAPSHOT_HISTORY {
        if let Some(g) = h.order.pop_front() {
            h.by_gen.remove(&g);
            if let Some(c) = h.copies.remove(&g) {
                let addr = &*c as *const NvnTexture as usize;
                if let Some(m) = TEX_SNAPSHOTS.lock().unwrap().as_mut() {
                    m.remove(&addr);
                }
            }
        }
    }
}

/// The snapshot generation a command naming `addr` records (0: none).
pub fn texture_ref_gen(addr: usize) -> u32 {
    texture_snapshot(addr).map_or(0, |(g, _)| g as u32)
}

/// The texture object a recorded reference (`addr` as captured with generation `gen`) names now: `addr` while it still
/// describes that texture, else a stable copy of the recorded description. Super Mario Odyssey records command lists once
/// and calls them every frame while the NVNtexture structs they named are re-initialized for other textures (its mip
/// chain and volume passes then targeted levels the new textures do not have).
pub fn resolve_texture_ref(addr: usize, gen: u32) -> usize {
    if addr == 0 || gen == 0 || texture_snapshot(addr).map_or(true, |(g, _)| g as u32 == gen) {
        return addr;
    }
    let mut h = TEX_HISTORY.lock().unwrap();
    let Some(h) = h.as_mut() else { return addr };
    let Some(&state) = h.by_gen.get(&gen) else { return addr };
    if let Some(c) = h.copies.get(&gen) {
        return &**c as *const NvnTexture as usize;
    }
    let b = Box::new(state);
    let copy = &*b as *const NvnTexture as usize;
    let g = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    TEX_SNAPSHOTS.lock().unwrap().get_or_insert_with(Default::default).insert(copy, (g, state));
    h.copies.insert(gen, b);
    bump_tex_table_epoch();
    copy
}

/// Largest memory size (declared or by layout) of any texture indexed so far.
static MAX_TEXTURE_SPAN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
pub fn max_texture_span() -> usize {
    MAX_TEXTURE_SPAN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Live (initialized, not finalized) NVNtextures by storage address, for resolving aliases: several texture
/// objects over the same GPU memory (mip-level or layer views, format reinterpretations) must share one
/// executor texture or rendering through one is invisible through the others.
static TEX_BY_STORAGE: std::sync::Mutex<Option<std::collections::BTreeMap<usize, Vec<usize>>>> =
    std::sync::Mutex::new(None);

fn unindex_texture(
    idx: &mut std::collections::BTreeMap<usize, Vec<usize>>,
    addr: usize,
    storage: usize,
) {
    if let Some(v) = idx.get_mut(&storage) {
        v.retain(|a| *a != addr);
        if v.is_empty() {
            idx.remove(&storage);
        }
    }
}

/// Live textures whose storage starts within `span` bytes at or below `storage`: (object address, generation, desc).
pub fn textures_near_storage(storage: usize, span: usize) -> Vec<(usize, u64, NvnTexture)> {
    let addrs: Vec<usize> = {
        let idx = TEX_BY_STORAGE.lock().unwrap();
        let Some(idx) = idx.as_ref() else {
            return Vec::new();
        };
        idx.range(storage.saturating_sub(span)..=storage)
            .flat_map(|(_, v)| v.iter().copied())
            .collect()
    };
    let snaps = TEX_SNAPSHOTS.lock().unwrap();
    let Some(snaps) = snaps.as_ref() else {
        return Vec::new();
    };
    addrs
        .into_iter()
        .filter_map(|a| snaps.get(&a).map(|(g, t)| (a, *g, *t)))
        .collect()
}

/// Called when a command naming the NVNtexture at `addr` is recorded. Real NVN reads the texture then; the executor
/// reads its snapshot later. Games copy initialized NVNtexture structs (they hold their descriptor inline) and bind the
/// copies, which never went through nvnTextureInitialize: the copy is snapshotted here, before the game frees or reuses
/// its memory (Super Mario Odyssey's opening demo: a 1600x900 target read back as garbage dimensions, gigabytes of
/// layout). A new copy at an address that held another texture gets a new generation.
pub fn capture_texture(addr: usize) {
    if addr < 0x10000 {
        return;
    }
    let live = unsafe { std::ptr::read(addr as *const NvnTexture) };
    let same = |a: &NvnTexture, b: &NvnTexture| {
        (a.target, a.width, a.height, a.depth, a.levels, a.format, a.flags, a.samples, a.stride, a.storage, a.size, a.swizzle)
            == (b.target, b.width, b.height, b.depth, b.levels, b.format, b.flags, b.samples, b.stride, b.storage, b.size, b.swizzle)
    };
    match texture_snapshot(addr) {
        Some((_, snap)) if same(&snap, &live) => {}
        // A finalized object (zeroed) keeps its snapshot: a command naming it is the game's business.
        _ if live.width <= 0 || live.height <= 0 => {}
        _ => snapshot_texture(addr, live),
    }
}

/// (generation, texture description) of the NVNtexture most recently initialized at `addr`.
pub fn texture_snapshot(addr: usize) -> Option<(u64, NvnTexture)> {
    TEX_SNAPSHOTS.lock().unwrap().as_ref()?.get(&addr).copied()
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureFinalize(tex: *mut NvnTexture) {
    record_hle_call("hle_nvnTextureFinalize");
    // The snapshot stays (recorded commands may still name this object), but it no longer anchors aliases. It gets a
    // new generation: the game may free the object while pool slots registered from it stay in use (NVN copied the
    // descriptor into the pool), and those slots must resolve to their registered state, not to whatever reuses the
    // memory (Super Mario Odyssey's opening demo read heap data as texture dimensions and allocated gigabytes).
    if let Some((_, t)) = texture_snapshot(tex as usize) {
        if let Some(idx) = TEX_BY_STORAGE.lock().unwrap().as_mut() {
            unindex_texture(idx, tex as usize, t.storage);
        }
        let gen = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if let Some(s) = TEX_SNAPSHOTS.lock().unwrap().as_mut() {
            s.insert(tex as usize, (gen, t));
        }
        bump_tex_table_epoch();
    }
    if !tex.is_null() {
        std::ptr::write_bytes(tex, 0, 1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetWidth(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetWidth");
    if tex.is_null() {
        0
    } else {
        (*tex).width
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetHeight(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetHeight");
    if tex.is_null() {
        0
    } else {
        (*tex).height
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetTextureAddress(tex: *const NvnTexture) -> u64 {
    record_hle_call("hle_nvnTextureGetTextureAddress");
    if tex.is_null() {
        0
    } else {
        (*tex).storage as u64
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetDepth(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetDepth");
    if tex.is_null() {
        0
    } else {
        (*tex).depth
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetLevels(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetLevels");
    if tex.is_null() {
        0
    } else {
        (*tex).levels
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetFormat(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetFormat");
    if tex.is_null() {
        0
    } else {
        (*tex).format
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetFlags(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetFlags");
    if tex.is_null() {
        0
    } else {
        (*tex).flags
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetSamples(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetSamples");
    if tex.is_null() {
        0
    } else {
        (*tex).samples
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetTarget(tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetTarget");
    if tex.is_null() {
        0
    } else {
        (*tex).target
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetStride(tex: *const NvnTexture) -> i64 {
    record_hle_call("hle_nvnTextureGetStride");
    if tex.is_null() {
        0
    } else {
        (*tex).stride
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetStorageSize(tex: *const NvnTexture) -> usize {
    record_hle_call("hle_nvnTextureGetStorageSize");
    if tex.is_null() {
        0
    } else {
        (*tex).size
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetStorageClass(_tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetStorageClass");
    0
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetMemoryPool(tex: *const NvnTexture) -> usize {
    record_hle_call("hle_nvnTextureGetMemoryPool");
    if tex.is_null() {
        0
    } else {
        (*tex).pool
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetMemoryOffset(tex: *const NvnTexture) -> i64 {
    record_hle_call("hle_nvnTextureGetMemoryOffset");
    if tex.is_null() {
        0
    } else {
        (*tex).pool_offset
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetViewOffset(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
) -> isize {
    record_hle_call("hle_nvnTextureGetViewOffset");
    if tex.is_null() {
        return 0;
    }
    let layout = crate::nvn::layout::compute_layout(&texture_layout_params(&*tex));
    let (lvl, layer) = if view.is_null() {
        (0, 0)
    } else {
        ((*view).min_level as u32, (*view).min_layer as u32)
    };
    crate::nvn::layout::view_offset(&layout, lvl, layer) as isize
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetDebugID(_tex: *const NvnTexture) -> u32 {
    record_hle_call("hle_nvnTextureGetDebugID");
    0
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetSwizzle(
    tex: *const NvnTexture,
    r: *mut i32,
    g: *mut i32,
    b: *mut i32,
    a: *mut i32,
) {
    record_hle_call("hle_nvnTextureGetSwizzle");
    // Four separate outputs. Writing all four through `r` as an array left g/b/a holding stale stack bytes:
    // TotK copies a texture's swizzle into a new texture this way, and the garbage (e.g. B = 156, G = ONE) turned
    // albedo textures cyan in Hateno.
    let sw = if tex.is_null() {
        [2, 3, 4, 5]
    } else {
        (*tex).swizzle
    };
    for (p, v) in [r, g, b, a].into_iter().zip(sw) {
        if !p.is_null() {
            *p = v as i32;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetDepthStencilMode(_tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetDepthStencilMode");
    0
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetRawStorageClass(_tex: *const NvnTexture) -> i32 {
    record_hle_call("hle_nvnTextureGetRawStorageClass");
    0
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureGetSparseTileLayout(
    _tex: *const NvnTexture,
    _layout: *mut (),
) -> u8 {
    record_hle_call("hle_nvnTextureGetSparseTileLayout");
    0
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureSetDebugLabel(_tex: *mut NvnTexture, _label: *const c_char) {
    record_hle_call("hle_nvnTextureSetDebugLabel");
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureCompare(a: *const NvnTexture, b: *const NvnTexture) -> u8 {
    record_hle_call("hle_nvnTextureCompare");
    if a == b {
        1
    } else {
        0
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSize1D(b: *mut NvnTextureBuilder, width: i32) {
    record_hle_call("hle_nvnTextureBuilderSetSize1D");
    if !b.is_null() {
        (*b).width = width;
        (*b).height = 1;
        (*b).depth = 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSize2D(
    b: *mut NvnTextureBuilder,
    width: i32,
    height: i32,
) {
    record_hle_call("hle_nvnTextureBuilderSetSize2D");
    if !b.is_null() {
        (*b).width = width;
        (*b).height = height;
        (*b).depth = 1;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetSize3D(
    b: *mut NvnTextureBuilder,
    width: i32,
    height: i32,
    depth: i32,
) {
    record_hle_call("hle_nvnTextureBuilderSetSize3D");
    if !b.is_null() {
        (*b).width = width;
        (*b).height = height;
        (*b).depth = depth;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetDevice(b: *const NvnTextureBuilder) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetDevice");
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetTarget(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetTarget");
    if b.is_null() {
        0
    } else {
        (*b).target
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetWidth(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetWidth");
    if b.is_null() {
        0
    } else {
        (*b).width
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetHeight(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetHeight");
    if b.is_null() {
        0
    } else {
        (*b).height
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetDepth(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetDepth");
    if b.is_null() {
        0
    } else {
        (*b).depth
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetLevels(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetLevels");
    if b.is_null() {
        0
    } else {
        (*b).levels
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetFormat(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetFormat");
    if b.is_null() {
        0
    } else {
        (*b).format
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetFlags(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetFlags");
    if b.is_null() {
        0
    } else {
        (*b).flags
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetSamples(b: *const NvnTextureBuilder) -> i32 {
    record_hle_call("hle_nvnTextureBuilderGetSamples");
    if b.is_null() {
        0
    } else {
        (*b).samples
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetStride(b: *const NvnTextureBuilder) -> i64 {
    record_hle_call("hle_nvnTextureBuilderGetStride");
    if b.is_null() {
        0
    } else {
        (*b).stride
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetMemoryPool(b: *const NvnTextureBuilder) -> usize {
    record_hle_call("hle_nvnTextureBuilderGetMemoryPool");
    if b.is_null() {
        0
    } else {
        (*b).pool
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetMemoryOffset(b: *const NvnTextureBuilder) -> i64 {
    record_hle_call("hle_nvnTextureBuilderGetMemoryOffset");
    if b.is_null() {
        0
    } else {
        (*b).pool_offset
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetViewOffset(
    b: *const NvnTextureBuilder,
    view: *const NvnTextureView,
) -> isize {
    record_hle_call("hle_nvnTextureBuilderGetViewOffset");
    if b.is_null() {
        return 0;
    }
    let layout = crate::nvn::layout::compute_layout(&builder_layout_params(&*b));
    let (lvl, layer) = if view.is_null() {
        (0, 0)
    } else {
        ((*view).min_level as u32, (*view).min_layer as u32)
    };
    crate::nvn::layout::view_offset(&layout, lvl, layer) as isize
}

// ---------------------------------------------------------------------------
// NVN Texture View family. NVNtextureView must be 0x10 bytes (or smaller): the game
// builds views in a stack slot directly followed by a live container (count / heap /
// array pointer at view+0x10, initialised *before* SetDefaults/SetLevels/SetLayers run
// in the per-subresource loop at guest+0x86cc60). Earlier 0x28/0x20-byte layouts
// clobbered that container and crashed its release at guest+0x18B7B38.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct NvnTextureView {
    pub min_level: u16,
    pub num_levels: u16,
    pub min_layer: u16,
    pub num_layers: u16,
    pub format: u16,
    pub target: u8,
    pub depth_stencil_mode: u8,
    pub swizzle: [u8; 4],
}
const _: () = assert!(std::mem::size_of::<NvnTextureView>() == 0x10);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetDefaults(view: *mut NvnTextureView) {
    record_hle_call("hle_nvnTextureViewSetDefaults");
    if !view.is_null() {
        std::ptr::write_bytes(view, 0, 1);
        (*view).swizzle = [2, 3, 4, 5];
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetLevels(
    view: *mut NvnTextureView,
    min_level: i32,
    num_levels: i32,
) {
    record_hle_call("hle_nvnTextureViewSetLevels");
    if !view.is_null() {
        (*view).min_level = min_level as u16;
        (*view).num_levels = num_levels as u16;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetLayers(
    view: *mut NvnTextureView,
    min_layer: i32,
    num_layers: i32,
) {
    record_hle_call("hle_nvnTextureViewSetLayers");
    if !view.is_null() {
        (*view).min_layer = min_layer as u16;
        (*view).num_layers = num_layers as u16;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetFormat(view: *mut NvnTextureView, format: i32) {
    record_hle_call("hle_nvnTextureViewSetFormat");
    if !view.is_null() {
        (*view).format = format as u16;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetSwizzle(
    view: *mut NvnTextureView,
    r: i32,
    g: i32,
    b: i32,
    a: i32,
) {
    record_hle_call("hle_nvnTextureViewSetSwizzle");
    if !view.is_null() {
        (*view).swizzle = [r as u8, g as u8, b as u8, a as u8];
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetDepthStencilMode(
    view: *mut NvnTextureView,
    mode: i32,
) {
    record_hle_call("hle_nvnTextureViewSetDepthStencilMode");
    if !view.is_null() {
        (*view).depth_stencil_mode = mode as u8;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewSetTarget(view: *mut NvnTextureView, target: i32) {
    record_hle_call("hle_nvnTextureViewSetTarget");
    if !view.is_null() {
        (*view).target = target as u8;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetLevels(
    view: *const NvnTextureView,
    min_level: *mut i32,
    num_levels: *mut i32,
) {
    record_hle_call("hle_nvnTextureViewGetLevels");
    if !view.is_null() {
        if !min_level.is_null() {
            *min_level = (*view).min_level as i32;
        }
        if !num_levels.is_null() {
            *num_levels = (*view).num_levels as i32;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetLayers(
    view: *const NvnTextureView,
    min_layer: *mut i32,
    num_layers: *mut i32,
) {
    record_hle_call("hle_nvnTextureViewGetLayers");
    if !view.is_null() {
        if !min_layer.is_null() {
            *min_layer = (*view).min_layer as i32;
        }
        if !num_layers.is_null() {
            *num_layers = (*view).num_layers as i32;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetFormat(view: *const NvnTextureView) -> i32 {
    record_hle_call("hle_nvnTextureViewGetFormat");
    if view.is_null() {
        0
    } else {
        (*view).format as i32
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetSwizzle(
    view: *const NvnTextureView,
    r: *mut i32,
    g: *mut i32,
    b: *mut i32,
    a: *mut i32,
) {
    record_hle_call("hle_nvnTextureViewGetSwizzle");
    // Four separate outputs; ZERO when no swizzle was set (the stored default).
    let sw = if view.is_null() {
        [0; 4]
    } else {
        (*view).swizzle
    };
    for (p, v) in [r, g, b, a].into_iter().zip(sw) {
        if !p.is_null() {
            *p = v as i32;
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetDepthStencilMode(view: *const NvnTextureView) -> i32 {
    record_hle_call("hle_nvnTextureViewGetDepthStencilMode");
    if view.is_null() {
        0
    } else {
        (*view).depth_stencil_mode as i32
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewGetTarget(view: *const NvnTextureView) -> i32 {
    record_hle_call("hle_nvnTextureViewGetTarget");
    if view.is_null() {
        0
    } else {
        (*view).target as i32
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureViewCompare(
    a: *const NvnTextureView,
    b: *const NvnTextureView,
) -> u8 {
    record_hle_call("hle_nvnTextureViewCompare");
    if a.is_null() || b.is_null() {
        return (a == b) as u8;
    }
    ((*a).target == (*b).target
        && (*a).format == (*b).format
        && (*a).min_level == (*b).min_level
        && (*a).num_levels == (*b).num_levels
        && (*a).min_layer == (*b).min_layer
        && (*a).num_layers == (*b).num_layers
        && (*a).swizzle == (*b).swizzle
        && (*a).depth_stencil_mode == (*b).depth_stencil_mode) as u8
}

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

const _: () = assert!(std::mem::size_of::<NvnSamplerBuilder>() == 0x40);

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
const _: () = assert!(std::mem::size_of::<NvnSampler>() <= 0x40);

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

// ── CPU texel access (nvnTextureWriteTexels & co) ───────────────────────────────────────────
// NVNcopyRegion: { xoffset, yoffset, zoffset, width, height, depth } (6 ints). Data is linear:
// tightly packed rows of `width` texels (blocks for compressed formats), slices back to back.
#[repr(C)]
pub struct NvnCopyRegion {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub w: i32,
    pub h: i32,
    pub d: i32,
}

// Textures the guest wrote texels into after the executor may have created their Metal copy; the executor
// re-uploads them from guest storage on next use (see metal/mod.rs, `texture`).
/// A set: texture() asks for every texture it resolves, and entries of textures never resolved again stay (the Vec this
/// was made the linear search ~6% of the encoding thread while walking).
static DIRTY_TEXTURES: std::sync::LazyLock<std::sync::Mutex<rustc_hash::FxHashSet<usize>>> =
    std::sync::LazyLock::new(Default::default);

/// Bumped by every mark_texture_dirty (lets the executor's binding memo notice pending re-uploads without a lock).
static DIRTY_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub fn texture_dirty_gen() -> u64 {
    DIRTY_GEN.load(std::sync::atomic::Ordering::Acquire)
}

pub fn mark_texture_dirty(tex: usize) {
    // Insert first: a reader that sees the new generation must also find the entry (memos record the generation and
    // skip take_texture_dirty while it is unchanged).
    DIRTY_TEXTURES.lock().unwrap().insert(tex);
    DIRTY_GEN.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
}

/// Whether `tex` has an upload pending (without taking it).
pub fn texture_dirty_pending(tex: usize) -> bool {
    DIRTY_TEXTURES.lock().unwrap().contains(&tex)
}

/// True (once) if the guest wrote into `tex` since the executor last looked.
pub fn take_texture_dirty(tex: usize) -> bool {
    DIRTY_TEXTURES.lock().unwrap().remove(&tex)
}

/// Writes one full mip level (layer 0) of tightly packed texels into the texture's guest memory (render-target
/// write-back, metal::writeback_pass).
/// `t` is the texture's recorded state (the object's memory may hold something else by now).
pub unsafe fn guest_write_level(t: &NvnTexture, level: usize, data: &[u8]) {
    let w = ((t.width as usize) >> level).max(1) as i32;
    let h = ((t.height as usize) >> level).max(1) as i32;
    let region = NvnCopyRegion {
        x: 0,
        y: 0,
        z: 0,
        w,
        h,
        d: 1,
    };
    let mut view: NvnTextureView = std::mem::zeroed();
    view.min_level = level as _;
    view.num_levels = 1;
    // Not marked dirty: the Metal texture already holds these texels (they were read from it), and the upload a dirty
    // mark triggers replaced newer GPU output with this copy (one-frame magenta tiles, 2026-10-13).
    texture_write_texels(t, &view, &region, data.as_ptr(), false);
}

/// `mark`: the texture's Metal copy is stale afterwards (the game wrote texels); false when the texels came from it.
unsafe fn texture_write_texels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *const u8,
    mark: bool,
) {
    if tex.is_null() || region.is_null() || data.is_null() || (*tex).storage == 0 {
        return;
    }
    if mark {
        mark_texture_dirty(tex as usize);
    }
    let t = &*tex;
    let r = &*region;
    let params = texture_layout_params(t);
    let layout = crate::nvn::layout::compute_layout(&params);
    let (level, layer) = if view.is_null() {
        (0usize, 0u32)
    } else {
        ((*view).min_level as usize, (*view).min_layer as u32)
    };
    let Some(info) = crate::nvn::layout::format_info(t.format) else {
        return;
    };
    let slice_data_bytes = (((r.w.max(0) as u32 + info.block_w - 1) / info.block_w) as usize)
        * (((r.h.max(0) as u32 + info.block_h - 1) / info.block_h) as usize)
        * info.bytes as usize;
    let is_3d = t.target == crate::nvn::layout::NVN_TARGET_3D;
    for z in 0..r.d.max(1) {
        let (slice_layer, z_off) = if is_3d {
            let lvl_sz = layout.level_sizes.get(level).copied().unwrap_or(0) as usize;
            let depth_l = ((t.depth.max(1) as usize) >> level).max(1);
            (layer, (r.z.max(0) as usize + z as usize) * lvl_sz / depth_l)
        } else {
            (layer + (r.z.max(0) + z) as u32, 0)
        };
        let base = layout.layer_stride as usize * slice_layer as usize + z_off;
        let src = data.add(z as usize * slice_data_bytes);
        if !crate::nvn::layout::write_texels_2d(
            (t.storage) as *mut u8,
            base,
            &params,
            level,
            r.x,
            r.y,
            r.w,
            r.h,
            src,
        ) {
            eprintln!(
                "[!] nvnTextureWriteTexels: unsupported layout (target {} format {})",
                t.target, t.format
            );
            return;
        }
    }
}
/// Bytes of one tightly packed row and one slice of `region` (blocks for compressed formats).
unsafe fn texel_region_pitches(t: &NvnTexture, r: &NvnCopyRegion) -> Option<(usize, usize)> {
    let info = crate::nvn::layout::format_info(t.format)?;
    let row =
        ((r.w.max(0) as u32 + info.block_w - 1) / info.block_w) as usize * info.bytes as usize;
    let rows = ((r.h.max(0) as u32 + info.block_h - 1) / info.block_h) as usize;
    Some((row, row * rows))
}

/// nvnTextureReadTexels: the inverse of texture_write_texels, from the texture's (block-linear) storage into linear
/// `data`. TotK copies baked lighting (TexToGo/RBake_*) this way into its lightmap atlas; left unimplemented, the
/// atlas stayed zero and lit surfaces as fully occluded.
unsafe fn texture_read_texels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *mut u8,
) -> bool {
    if tex.is_null() || region.is_null() || data.is_null() || (*tex).storage == 0 {
        return false;
    }
    let t = &*tex;
    let r = &*region;
    let params = texture_layout_params(t);
    let layout = crate::nvn::layout::compute_layout(&params);
    let (level, layer) = if view.is_null() {
        (0usize, 0u32)
    } else {
        ((*view).min_level as usize, (*view).min_layer as u32)
    };
    let Some((_, slice_data_bytes)) = texel_region_pitches(t, r) else {
        return false;
    };
    if t.target == crate::nvn::layout::NVN_TARGET_3D {
        eprintln!(
            "[!] nvnTextureReadTexels: 3D textures not supported (format {})",
            t.format
        );
        return false;
    }
    for z in 0..r.d.max(1) {
        let base = layout.layer_stride as usize * (layer + (r.z.max(0) + z) as u32) as usize;
        let dst = data.add(z as usize * slice_data_bytes);
        if !crate::nvn::layout::read_texels_2d(
            t.storage as *const u8,
            base,
            &params,
            level,
            r.x,
            r.y,
            r.w,
            r.h,
            dst,
        ) {
            eprintln!(
                "[!] nvnTextureReadTexels: unsupported layout (target {} format {})",
                t.target, t.format
            );
            return false;
        }
    }
    true
}
#[no_mangle]
unsafe fn watch_size_matches(t: &NvnTexture) -> bool {
    crate::settings::get()
        .trace
        .watch_size
        .is_some_and(|s| s.is(t.width, t.height))
}
/// Debug (--watch-size=WxH): logs texel reads / writes on textures of that size.
unsafe fn watch_texels(op: &str, tex: *const NvnTexture, view: *const NvnTextureView) {
    let w = crate::settings::get()
        .trace
        .watch_size
        .map(|s| (s.w as i32, s.h as i32));
    if let (Some((ww, wh)), false) = (w, tex.is_null()) {
        let t = &*tex;
        if t.width == ww && t.height == wh {
            println!(
                "[watch-size] {} texture {:p} storage {:#x} fmt {} levels {} view {:?}",
                op,
                tex,
                t.storage,
                t.format,
                t.levels,
                (!view.is_null()).then(|| *view)
            );
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureReadTexels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *mut u8,
) {
    record_hle_call("hle_nvnTextureReadTexels");
    watch_texels("ReadTexels", tex, view);
    texture_read_texels(tex, view, region, data);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureReadTexelsStrided(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *mut u8,
    row_stride: isize,
    image_stride: isize,
) {
    record_hle_call("hle_nvnTextureReadTexelsStrided");
    watch_texels("ReadTexelsStrided", tex, view);
    if tex.is_null() || region.is_null() || data.is_null() {
        return;
    }
    let Some((row, slice)) = texel_region_pitches(&*tex, &*region) else {
        return;
    };
    let r = &*region;
    let (rs, is) = (
        if row_stride > 0 {
            row_stride as usize
        } else {
            row
        },
        if image_stride > 0 {
            image_stride as usize
        } else {
            slice
        },
    );
    if rs == row && is == slice {
        texture_read_texels(tex, view, region, data);
        return;
    }
    // Read tightly packed, then scatter rows to the caller's strides.
    let depth = r.d.max(1) as usize;
    let mut tmp = vec![0u8; slice * depth];
    if !texture_read_texels(tex, view, region, tmp.as_mut_ptr()) {
        return;
    }
    let rows = if row > 0 { slice / row } else { 0 };
    for z in 0..depth {
        for y in 0..rows {
            std::ptr::copy_nonoverlapping(
                tmp.as_ptr().add(z * slice + y * row),
                data.add(z * is + y * rs),
                row,
            );
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureWriteTexels(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *const u8,
) {
    record_hle_call("hle_nvnTextureWriteTexels");
    watch_texels("WriteTexels", tex, view);
    if !tex.is_null() && !region.is_null() && !data.is_null() && watch_size_matches(&*tex) {
        let r = &*region;
        let n = (r.w.max(0) * r.h.max(0) * 4) as usize;
        let nz = std::slice::from_raw_parts(data, n)
            .iter()
            .filter(|&&b| b != 0)
            .count();
        println!(
            "[watch-size]   data {:p} region {}x{} at ({}, {}): {} of {} bytes nonzero",
            data, r.w, r.h, r.x, r.y, nz, n
        );
    }
    texture_write_texels(tex, view, region, data, true);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureWriteTexelsStrided(
    tex: *const NvnTexture,
    view: *const NvnTextureView,
    region: *const NvnCopyRegion,
    data: *const u8,
    row_stride: isize,
    image_stride: isize,
) {
    record_hle_call("hle_nvnTextureWriteTexelsStrided");
    watch_texels("WriteTexelsStrided", tex, view);
    if tex.is_null() || region.is_null() || data.is_null() {
        return;
    }
    let Some((row, slice)) = texel_region_pitches(&*tex, &*region) else {
        return;
    };
    let (rs, is) = (
        if row_stride > 0 {
            row_stride as usize
        } else {
            row
        },
        if image_stride > 0 {
            image_stride as usize
        } else {
            slice
        },
    );
    if rs == row && is == slice {
        texture_write_texels(tex, view, region, data, true);
        return;
    }
    // Gather the caller's strided rows into a tightly packed copy first.
    let depth = (*region).d.max(1) as usize;
    let rows = if row > 0 { slice / row } else { 0 };
    let mut tmp = vec![0u8; slice * depth];
    for z in 0..depth {
        for y in 0..rows {
            std::ptr::copy_nonoverlapping(
                data.add(z * is + y * rs),
                tmp.as_mut_ptr().add(z * slice + y * row),
                row,
            );
        }
    }
    texture_write_texels(tex, view, region, tmp.as_ptr(), true);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureFlushTexels(
    tex: *const NvnTexture,
    _view: *const NvnTextureView,
    _region: *const NvnCopyRegion,
) {
    record_hle_call("hle_nvnTextureFlushTexels"); // unified memory: nothing to flush, but the game may have written storage directly
    if !tex.is_null() {
        mark_texture_dirty(tex as usize);
    }
}
/// the packaged-data pointer is for the Windows reference implementation's conversion; "ignored on NX".
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderSetPackagedTextureData(
    _builder: *mut u8,
    _data: *const u8,
) {
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureInvalidateTexels(
    _tex: *const NvnTexture,
    _view: *const NvnTextureView,
    _region: *const NvnCopyRegion,
) {
    record_hle_call("hle_nvnTextureInvalidateTexels");
}

// ── remaining window / queue / device setters (no rendering state to keep yet) ──────────────
// Clip-space conventions set on the NVNdevice (NVNwindowOriginMode LOWER_LEFT=0 / UPPER_LEFT=1,
// NVNdepthMode NEAR_IS_MINUS_W=0 / NEAR_IS_ZERO=1). The executor needs them to map NVN clip space onto Metal's.
static WINDOW_ORIGIN_MODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
static DEPTH_MODE: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub fn window_origin_mode() -> i32 {
    WINDOW_ORIGIN_MODE.load(std::sync::atomic::Ordering::Relaxed)
}
pub fn depth_mode() -> i32 {
    DEPTH_MODE.load(std::sync::atomic::Ordering::Relaxed)
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
/// Vblanks each presented frame stays on screen (nvnWindowSetPresentInterval; 1 = 60 Hz, 2 = 30 Hz, 0 = no wait).
static PRESENT_INTERVAL: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(1);
pub fn present_interval() -> i32 {
    PRESENT_INTERVAL.load(std::sync::atomic::Ordering::Relaxed)
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowSetPresentInterval(_window: usize, interval: i32) {
    // Debug: --log-pacing prints the inputs of TotK's frame-pacing decision (guest+0x8f6670): the measured frame
    // duration in ticks at obj+0x118, the tick frequency it divides by, and the state at obj+0x248 (max, min, bias,
    // current interval). obj is the caller's saved x19 (stored at its frame pointer + 0x38).
    static LOG_PACING: OnceLock<bool> = OnceLock::new();
    if *LOG_PACING.get_or_init(|| crate::settings::get().log.log_pacing) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let fp: usize;
        std::arch::asm!("mov {}, x29", out(reg) fp);
        let base = crate::cpu::crash_handler::get_guest_base();
        let caller_fp = *(fp as *const usize);
        if n % 10 == 0
            && base != 0
            && caller_fp > 0x1_0000_0000
            && *((caller_fp + 8) as *const usize) == base + 0x8f633c
        {
            let obj = *((caller_fp + 0x38) as *const usize);
            let ticks = *((obj + 0x118) as *const i64);
            let freq = **((base + 0x462ab50) as *const *const i64);
            let st = obj + 0x248;
            let (mx, mn, bias, cur) = (
                *(st as *const i32),
                *((st + 4) as *const i32),
                *((st + 8) as *const f32),
                *((st + 0x14) as *const i32),
            );
            // Listeners that may veto lowering the interval: list at st+0x30 (sentinel), node {.., next @+8, obj @+0x10},
            // callback = (*(obj+0x28))[0] called with x0 = obj+0x28.
            let mut cbs = Vec::new();
            let head = st + 0x30;
            let mut node = *((head + 8) as *const usize);
            while node != head && node > 0x1_0000_0000 && cbs.len() < 8 {
                let obj = *((node + 0x10) as *const usize);
                let vt = *((obj + 0x28) as *const usize);
                let f = *(vt as *const usize);
                let fmt = |f: usize| {
                    if f >= base && f < base + 0x5000000 {
                        format!("guest+{:#x}", f - base)
                    } else {
                        format!("{:#x}", f)
                    }
                };
                // Delegate (guest+0x2aea334): object at d+8, member pointer at d+0x10 (virtual if d+0x18 is odd).
                let d = obj + 0x28;
                let (o, mp, adj) = (
                    *((d + 8) as *const usize),
                    *((d + 0x10) as *const usize),
                    *((d + 0x18) as *const isize),
                );
                let this = (o as isize + (adj >> 1)) as usize;
                let target = if adj & 1 != 0 && this > 0x1_0000_0000 {
                    *((*(this as *const usize) + mp) as *const usize)
                } else {
                    mp
                };
                cbs.push(format!("{} -> {}", fmt(f), fmt(target)));
                node = *((node + 8) as *const usize);
            }
            println!("[pacing] call {} set {} ticks {} freq {} -> {:.2} vblanks (+bias {:.2}) min {} max {} cur {} lowest-allowed {} cooldown {} listeners {:?}",
                n, interval, ticks, freq, ticks as f64 / freq as f64 * 60.0, bias, mx, mn, cur, *((st + 0x18) as *const i32), *((st + 0x20) as *const i32), cbs);
        }
    }
    let interval = if crate::host::overlay::fps_mode() == crate::settings::FpsMode::Stock30 {
        interval.max(2)
    } else {
        interval
    };
    if PRESENT_INTERVAL.swap(interval, std::sync::atomic::Ordering::Relaxed) != interval {
        let fp: usize;
        std::arch::asm!("mov {}, x29", out(reg) fp);
        println!(
            "[+] NVN: present interval = {} (guest callers: {})",
            interval,
            guest_chain(fp)
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetPresentInterval(_window: usize) -> i32 {
    present_interval()
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvn_noop_setter(_obj: usize, _a: usize, _b: usize) {}

// ── Builder/object getters and the setters they report (values as set, defaults from SetDefaults) ──
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetNumActiveTextures(
    b: *mut NvnWindowBuilder,
    n: i32,
) {
    if !b.is_null() {
        (*b).num_active = n;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetDevice(b: *const NvnWindowBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetNumTextures(b: *const NvnWindowBuilder) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).texture_count
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetTexture(
    b: *const NvnWindowBuilder,
    id: i32,
) -> usize {
    if b.is_null() || id < 0 || id >= (*b).texture_count.min(4) {
        0
    } else {
        (*b).textures[id as usize]
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderGetNumActiveTextures(
    b: *const NvnWindowBuilder,
) -> i32 {
    if b.is_null() {
        0
    } else if (*b).num_active > 0 {
        (*b).num_active
    } else {
        (*b).texture_count
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetNumTextures(w: *const NvnWindow) -> i32 {
    if w.is_null() {
        0
    } else {
        (*w).num_textures
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetNumActiveTextures(w: *const NvnWindow) -> i32 {
    if w.is_null() {
        0
    } else {
        (*w).texture_count
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowGetCrop(w: *const NvnWindow, crop: *mut [i32; 4]) {
    if !w.is_null() && !crop.is_null() {
        *crop = (*w).crop;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetQueueMemory(
    b: *mut NvnQueueBuilder,
    memory: usize,
    size: usize,
) {
    if !b.is_null() {
        (*b).memory = memory;
        (*b).memory_size = size;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetCommandFlushThreshold(
    b: *mut NvnQueueBuilder,
    threshold: i32,
) {
    if !b.is_null() {
        (*b).flush_threshold = threshold;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetQueuePriority(
    b: *mut NvnQueueBuilder,
    priority: i32,
) {
    if !b.is_null() {
        (*b).priority = priority;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetDevice(b: *const NvnQueueBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetFlags(b: *const NvnQueueBuilder) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).flags
    }
}
/// The Get*MemorySize / GetCommandFlushThreshold getters return whether the value was set (nonzero here).
unsafe fn queue_size_out(v: usize, out: *mut i32) -> u8 {
    if !out.is_null() {
        *out = v as i32;
    }
    (v != 0) as u8
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetCommandMemorySize(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).command_mem_size, out)
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetComputeMemorySize(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).compute_mem_size, out)
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetControlMemorySize(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).control_mem_size, out)
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetCommandFlushThreshold(
    b: *const NvnQueueBuilder,
    out: *mut i32,
) -> u8 {
    if b.is_null() {
        0
    } else {
        queue_size_out((*b).flush_threshold.max(0) as usize, out)
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetQueuePriority(b: *const NvnQueueBuilder) -> i32 {
    if b.is_null() {
        100
    } else {
        (*b).priority
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetMemory(b: *const NvnQueueBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).memory
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetMemorySize(b: *const NvnQueueBuilder) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).memory_size as i32
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetDevice(b: *const NvnBufferBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetMemoryPool(buf: *const NvnBuffer) -> usize {
    if buf.is_null() {
        0
    } else {
        (*buf).pool
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetMemoryOffset(buf: *const NvnBuffer) -> i64 {
    if buf.is_null() {
        0
    } else {
        (*buf).offset
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnMemoryPoolGetSize(pool: *const NvnMemoryPool) -> usize {
    if pool.is_null() {
        0
    } else {
        (*pool).size
    }
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
    crate::clock::now_ns()
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
pub unsafe extern "C" fn hle_nvnTextureBuilderGetSwizzle(
    b: *const NvnTextureBuilder,
    r: *mut i32,
    g: *mut i32,
    bl: *mut i32,
    a: *mut i32,
) {
    if b.is_null() {
        return;
    }
    // SetDefaults zeroes the swizzle; NVN's default is identity (R, G, B, A = 2..5).
    let sw = (*b).swizzle;
    let ident = sw == [0; 4];
    for (i, out) in [r, g, bl, a].into_iter().enumerate() {
        if !out.is_null() {
            *out = if ident { 2 + i as i32 } else { sw[i] as i32 };
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnTextureBuilderGetDepthStencilMode(
    b: *const NvnTextureBuilder,
) -> i32 {
    if b.is_null() {
        0
    } else {
        (*b).depth_stencil_mode
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
const _: () = assert!(std::mem::size_of::<NvnProgram>() == 0x80);

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
        std::ptr::write_bytes(p, 0, 1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnProgramSetDebugLabel(_p: *mut NvnProgram, _label: *const c_char) {
    record_hle_call("hle_nvnProgramSetDebugLabel");
}
unsafe fn program_set_shaders(
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

// ---------------------------------------------------------------------------
// NVN TexturePool & SamplerPool family. NVNtexturePool and NVNsamplerPool
// are 0x20 (32) bytes each. They manage descriptor free-lists for shader
// access. Initialize MUST return 1 (NVN_TRUE), and GetSize returns descriptor count.
#[repr(C)]
pub struct NvnTexturePool {
    pub memory_pool: usize,
    pub memory_offset: usize,
    pub num_descriptors: i32,
    pub _pad: [u8; 12],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolInitialize(
    pool: *mut NvnTexturePool,
    memory_pool: usize,
    offset: usize,
    num_descriptors: i32,
) -> u8 {
    record_hle_call("hle_nvnTexturePoolInitialize");
    if pool.is_null() {
        return 0;
    }
    std::ptr::write_bytes(pool, 0, 1);
    (*pool).memory_pool = memory_pool;
    (*pool).memory_offset = offset;
    (*pool).num_descriptors = num_descriptors;
    println!("[+] NVN: Initialized TexturePool at {:p}, memory_pool: 0x{:x}, offset: 0x{:x}, num_descriptors: {}",
        pool, memory_pool, offset, num_descriptors);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolSetDebugLabel(
    _pool: *mut NvnTexturePool,
    _label: *const c_char,
) {
    record_hle_call("hle_nvnTexturePoolSetDebugLabel");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolFinalize(pool: *mut NvnTexturePool) {
    record_hle_call("hle_nvnTexturePoolFinalize");
    if !pool.is_null() {
        std::ptr::write_bytes(pool, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolGetMemoryPool(pool: *const NvnTexturePool) -> usize {
    record_hle_call("hle_nvnTexturePoolGetMemoryPool");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolGetMemoryOffset(pool: *const NvnTexturePool) -> usize {
    record_hle_call("hle_nvnTexturePoolGetMemoryOffset");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_offset
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolGetSize(pool: *const NvnTexturePool) -> i32 {
    record_hle_call("hle_nvnTexturePoolGetSize");
    if pool.is_null() {
        0
    } else {
        (*pool).num_descriptors
    }
}

/// Texture pool entries: texture address and the view as it was at registration. NVN writes the descriptor when
/// RegisterTexture is called, so the view is copied: games pass views on the stack and reuse that memory.
/// A texture pool slot. Real NVN writes the texture's descriptor into the pool at RegisterTexture time, so
/// re-initializing the NVNtexture afterwards (TotK's texture streamer reuses objects for other textures) does not
/// affect draws that use the slot until the game registers again. Resolving slots to the live object made materials
/// sample whatever texture the object held later (stair edges drawn with the gloom-crystal texture, Zelda's hair
/// green). The slot therefore remembers the object's generation and state at registration: while the object is
/// unchanged it resolves to the object itself (the executor's fast, cached path); once the object has been
/// re-initialized it resolves to a private copy of the registered state. The view is copied too: games pass views on
/// the stack and reuse that memory.
struct PoolSlot {
    obj: usize,
    gen: u64,
    state: NvnTexture,
    view: Option<NvnTextureView>,
    /// Created on first use after the object changed; not in the storage index (it is not a texture object and must
    /// not join alias groups); the executor finds its Metal texture through the description cache.
    copy: Option<Box<NvnTexture>>,
}

static POOL_TEXTURES: std::sync::Mutex<Option<rustc_hash::FxHashMap<i32, PoolSlot>>> =
    std::sync::Mutex::new(None);

pub fn lookup_pool_texture(id: i32) -> Option<(usize, Option<NvnTextureView>)> {
    let mut map = POOL_TEXTURES.lock().unwrap();
    let slot = map.as_mut()?.get_mut(&id)?;
    let live_gen = texture_snapshot(slot.obj).map_or(0, |(g, _)| g);
    if live_gen == slot.gen || slot.obj == 0 {
        return Some((slot.obj, slot.view));
    }
    if slot.copy.is_none() {
        let b = Box::new(slot.state);
        let addr = &*b as *const NvnTexture as usize;
        let gen = TEX_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        TEX_SNAPSHOTS
            .lock()
            .unwrap()
            .get_or_insert_with(Default::default)
            .insert(addr, (gen, slot.state));
        slot.copy = Some(b);
        bump_tex_table_epoch();
    }
    Some((
        &**slot.copy.as_ref().unwrap() as *const NvnTexture as usize,
        slot.view,
    ))
}

/// Records a RegisterTexture / RegisterImage.
unsafe fn register_pool_slot(id: i32, texture: *const NvnTexture, view: Option<NvnTextureView>) {
    let (gen, state) = if texture.is_null() {
        (0, std::mem::zeroed())
    } else {
        texture_snapshot(texture as usize).unwrap_or_else(|| (0, std::ptr::read(texture)))
    };
    // Debug: --log-pool-reg logs every texture pool registration.
    static LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *LOG.get_or_init(|| crate::settings::get().log.log_pool_reg) {
        println!(
            "[poolreg] id={:#x} tex={:p} gen={} {}x{} fmt={} storage={:#x}",
            id, texture, gen, state.width, state.height, state.format, state.storage
        );
    }
    let mut map = POOL_TEXTURES.lock().unwrap();
    let map = map.get_or_insert_with(Default::default);
    if let Some(old) = map.get(&id) {
        if old.obj == texture as usize
            && old.gen == gen
            && old.view.map(view_bytes) == view.map(view_bytes)
        {
            return; // re-registration of the same state: keep the slot (and any copy)
        }
    }
    map.insert(
        id,
        PoolSlot {
            obj: texture as usize,
            gen,
            state,
            view,
            copy: None,
        },
    );
    bump_tex_table_epoch();
}

fn view_bytes(v: NvnTextureView) -> [u8; std::mem::size_of::<NvnTextureView>()] {
    unsafe { std::mem::transmute(v) }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolRegisterTexture(
    _pool: *const NvnTexturePool,
    id: i32,
    texture: *const NvnTexture,
    view: *const (),
) {
    record_hle_call("hle_nvnTexturePoolRegisterTexture");
    let view = (!view.is_null()).then(|| *(view as *const NvnTextureView));
    register_pool_slot(id, texture, view);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnTexturePoolRegisterImage(
    _pool: *const NvnTexturePool,
    id: i32,
    texture: *const NvnTexture,
    view: *const (),
) {
    record_hle_call("hle_nvnTexturePoolRegisterImage");
    let view = (!view.is_null()).then(|| *(view as *const NvnTextureView));
    register_pool_slot(id, texture, view);
}

#[repr(C)]
pub struct NvnSamplerPool {
    pub memory_pool: usize,
    pub memory_offset: usize,
    pub num_descriptors: i32,
    pub _pad: [u8; 12],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolInitialize(
    pool: *mut NvnSamplerPool,
    memory_pool: usize,
    offset: usize,
    num_descriptors: i32,
) -> u8 {
    record_hle_call("hle_nvnSamplerPoolInitialize");
    if pool.is_null() {
        return 0;
    }
    std::ptr::write_bytes(pool, 0, 1);
    (*pool).memory_pool = memory_pool;
    (*pool).memory_offset = offset;
    (*pool).num_descriptors = num_descriptors;
    println!("[+] NVN: Initialized SamplerPool at {:p}, memory_pool: 0x{:x}, offset: 0x{:x}, num_descriptors: {}",
        pool, memory_pool, offset, num_descriptors);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolSetDebugLabel(
    _pool: *mut NvnSamplerPool,
    _label: *const c_char,
) {
    record_hle_call("hle_nvnSamplerPoolSetDebugLabel");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolFinalize(pool: *mut NvnSamplerPool) {
    record_hle_call("hle_nvnSamplerPoolFinalize");
    if !pool.is_null() {
        std::ptr::write_bytes(pool, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolGetMemoryPool(pool: *const NvnSamplerPool) -> usize {
    record_hle_call("hle_nvnSamplerPoolGetMemoryPool");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolGetMemoryOffset(pool: *const NvnSamplerPool) -> usize {
    record_hle_call("hle_nvnSamplerPoolGetMemoryOffset");
    if pool.is_null() {
        0
    } else {
        (*pool).memory_offset
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolGetSize(pool: *const NvnSamplerPool) -> i32 {
    record_hle_call("hle_nvnSamplerPoolGetSize");
    if pool.is_null() {
        0
    } else {
        (*pool).num_descriptors
    }
}

/// Sampler state as registered in a sampler pool (NVN enum values).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SamplerDesc {
    pub min_filter: i32,
    pub mag_filter: i32,
    pub wrap_s: i32,
    pub wrap_t: i32,
    pub wrap_r: i32,
    pub compare_mode: i8,
    pub compare_func: i8,
    pub max_anisotropy: f32,
    pub lod_min: f32,
    pub lod_max: f32,
    pub lod_bias: f32,
    pub border_color: [f32; 4],
}

static POOL_SAMPLERS: std::sync::OnceLock<
    std::sync::Mutex<rustc_hash::FxHashMap<i32, SamplerDesc>>,
> = std::sync::OnceLock::new();

/// Sampler registered under `id` (handle bits 20.. of an NVNtextureHandle).
pub fn lookup_pool_sampler(id: i32) -> Option<SamplerDesc> {
    POOL_SAMPLERS.get()?.lock().unwrap().get(&id).copied()
}

/// Changes at every sampler registration: lookups memoized per thread (gfx sampler states) are valid while unchanged.
static SAMPLERS_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn samplers_gen() -> u64 {
    SAMPLERS_GEN.load(std::sync::atomic::Ordering::Acquire)
}

fn register_pool_sampler(id: i32, d: SamplerDesc) {
    // Bumped after the insert below as well, so a lookup that read the old generation cannot keep the old sampler.
    struct Bump;
    impl Drop for Bump {
        fn drop(&mut self) {
            SAMPLERS_GEN.fetch_add(1, std::sync::atomic::Ordering::Release);
        }
    }
    let _bump = Bump;
    POOL_SAMPLERS
        .get_or_init(|| std::sync::Mutex::new(rustc_hash::FxHashMap::default()))
        .lock()
        .unwrap()
        .insert(id, d);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolRegisterSampler(
    _pool: *const NvnSamplerPool,
    id: i32,
    sampler: *const NvnSampler,
) {
    record_hle_call("hle_nvnSamplerPoolRegisterSampler");
    if sampler.is_null() {
        return;
    }
    let s = &*sampler;
    register_pool_sampler(
        id,
        SamplerDesc {
            min_filter: s.min_filter,
            mag_filter: s.mag_filter,
            wrap_s: s.wrap_s,
            wrap_t: s.wrap_t,
            wrap_r: s.wrap_r,
            compare_mode: s.compare_mode,
            compare_func: s.compare_func,
            max_anisotropy: s.max_anisotropy,
            lod_min: s.lod_min,
            lod_max: s.lod_max,
            lod_bias: s.lod_bias,
            border_color: s.border_color,
        },
    );
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSamplerPoolRegisterSamplerBuilder(
    _pool: *const NvnSamplerPool,
    id: i32,
    builder: *const NvnSamplerBuilder,
) {
    record_hle_call("hle_nvnSamplerPoolRegisterSamplerBuilder");
    if builder.is_null() {
        return;
    }
    let b = &*builder;
    register_pool_sampler(
        id,
        SamplerDesc {
            min_filter: b.min_filter,
            mag_filter: b.mag_filter,
            wrap_s: b.wrap_s,
            wrap_t: b.wrap_t,
            wrap_r: b.wrap_r,
            compare_mode: b.compare_mode,
            compare_func: b.compare_func,
            max_anisotropy: b.max_anisotropy,
            lod_min: b.lod_min,
            lod_max: b.lod_max,
            lod_bias: b.lod_bias,
            border_color: b.border_color,
        },
    );
}

// ---------------------------------------------------------------------------
// NVN Sync & Event family. NVNsync is 0x40 (64) bytes, NVNevent is 0x40 (64) bytes.
#[repr(C)]
pub struct NvnSync {
    pub device: usize,
    /// Serial of the last command buffer submitted before nvnQueueFenceSync (0: never fenced, signaled).
    pub serial: u64,
    pub _pad: [u8; 48],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncInitialize(sync: *mut NvnSync, device: usize) -> u8 {
    record_hle_call("hle_nvnSyncInitialize");
    if sync.is_null() {
        return 0;
    }
    std::ptr::write_bytes(sync, 0, 1);
    (*sync).device = device;
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncFinalize(sync: *mut NvnSync) {
    record_hle_call("hle_nvnSyncFinalize");
    if !sync.is_null() {
        std::ptr::write_bytes(sync, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncSetDebugLabel(_sync: *mut NvnSync, _label: *const c_char) {
    record_hle_call("hle_nvnSyncSetDebugLabel");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnSyncWait(sync: *const NvnSync, timeout_ns: u64) -> i32 {
    record_hle_call("hle_nvnSyncWait");
    // NVN_SYNC_WAIT_RESULT: ALREADY_SIGNALED 0, CONDITION_SATISFIED 1, TIMEOUT_EXPIRED 2.
    let serial = if sync.is_null() { 0 } else { (*sync).serial };
    let gpu = crate::gpu::backend();
    if serial == 0 || gpu.wait_fence(serial, 0) {
        return 0;
    }
    let t0 = std::time::Instant::now();
    let r = if gpu.wait_fence(serial, timeout_ns) {
        1
    } else {
        2
    };
    if log_timestamps() {
        let now = crate::clock::now_ms();
        println!(
            "[ts] syncwait for submit {} (latest {}) {:.1} ms, ended t {:.3} ms thread {:?}",
            serial,
            gpu.submitted_serial(),
            t0.elapsed().as_secs_f64() * 1e3,
            now,
            std::thread::current().name()
        );
    }
    // Debug: --log-sync prints every 20th blocking wait: which submit it waited for, relative to the latest.
    static LOG: OnceLock<bool> = OnceLock::new();
    if *LOG.get_or_init(|| crate::settings::get().log.log_sync) {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        if N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 20 == 0 {
            let fp: usize;
            std::arch::asm!("mov {}, x29", out(reg) fp);
            println!("[sync] wait for submit {} (latest {}) took {:.1} ms timeout {} thread {:?} callers {}", serial, gpu.submitted_serial(),
                t0.elapsed().as_secs_f64() * 1e3, timeout_ns, std::thread::current().name(), guest_chain(fp));
        }
    }
    r
}

#[repr(C)]
pub struct NvnEvent {
    pub storage: usize,
    pub _pad: [u8; 56],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventInitialize(event: *mut NvnEvent, _builder: *const ()) -> u8 {
    record_hle_call("hle_nvnEventInitialize");
    if event.is_null() {
        return 0;
    }
    std::ptr::write_bytes(event, 0, 1);
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventFinalize(event: *mut NvnEvent) {
    record_hle_call("hle_nvnEventFinalize");
    if !event.is_null() {
        std::ptr::write_bytes(event, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventGetValue(_event: *const NvnEvent) -> u32 {
    record_hle_call("hle_nvnEventGetValue");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnEventSignal(_event: *mut NvnEvent, _mode: i32, _val: u32) {
    record_hle_call("hle_nvnEventSignal");
}

// ---------------------------------------------------------------------------
// NVN Window family. NVNwindow is 384 bytes, NVNwindowBuilder 0x40 (64)
// bytes. This is the actual framebuffer presentation path -- Present pushes
// a real packet into the compositor channel that already exists
// (compositor.rs) but has never been fed real data before now. Nothing
// consumes that channel into real pixels yet (that's the dedicated Metal-
// translation follow-up this session has flagged twice already), but the
// data now genuinely flows end-to-end from the game's present call instead
// of being silently dropped.
#[repr(C)]
pub(crate) struct NvnWindowBuilder {
    device: usize,
    textures: [usize; 4], // NVN swap chains are typically double/triple buffered; 4 is generous
    texture_count: i32,
    num_active: i32, // nvnWindowBuilderSetNumActiveTextures; 0 = all
}

#[repr(C)]
pub(crate) struct NvnWindow {
    textures: [usize; 4],
    /// Active textures (the ones AcquireTexture cycles through).
    texture_count: i32,
    next_index: u32,
    num_textures: i32,
    device: usize,
    crop: [i32; 4],
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetDefaults(b: *mut NvnWindowBuilder) {
    record_hle_call("hle_nvnWindowBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetDevice(b: *mut NvnWindowBuilder, device: usize) {
    record_hle_call("hle_nvnWindowBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowBuilderSetTextures(
    b: *mut NvnWindowBuilder,
    count: i32,
    textures: *const usize,
) {
    record_hle_call("hle_nvnWindowBuilderSetTextures");
    if b.is_null() || textures.is_null() {
        return;
    }
    let n = (count.max(0) as usize).min(4);
    (*b).texture_count = n as i32;
    for i in 0..n {
        (*b).textures[i] = *textures.add(i);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowInitialize(
    w: *mut NvnWindow,
    builder: *const NvnWindowBuilder,
) -> u8 {
    record_hle_call("hle_nvnWindowInitialize");
    if w.is_null() || builder.is_null() {
        return 0;
    }
    let b = &*builder;
    let active = if b.num_active > 0 {
        b.num_active.min(b.texture_count)
    } else {
        b.texture_count
    };
    // Default crop: the whole first texture.
    let crop = if b.textures[0] != 0 {
        let t = &*(b.textures[0] as *const NvnTexture);
        [0, 0, t.width, t.height]
    } else {
        [0; 4]
    };
    std::ptr::write(
        w,
        NvnWindow {
            textures: b.textures,
            texture_count: active,
            next_index: 0,
            num_textures: b.texture_count,
            device: b.device,
            crop,
        },
    );
    if crate::settings::get().system.fps == crate::settings::FpsMode::Stock30 {
        PRESENT_INTERVAL.store(2, std::sync::atomic::Ordering::Relaxed);
    }
    println!(
        "[+] NVN: Window initialized with {} swap textures ({} active)",
        b.texture_count, active
    );
    1
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowFinalize(w: *mut NvnWindow) {
    record_hle_call("hle_nvnWindowFinalize");
    if !w.is_null() {
        std::ptr::write_bytes(w, 0, 1);
    }
}
/// nvnWindowAcquireTexture calls and time spent in its vsync wait (reported on the gfx fps line).
pub static ACQUIRE_CALLS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static ACQUIRE_SLEEP_US: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowAcquireTexture(
    w: *mut NvnWindow,
    _fence: usize,
    out_index: *mut i32,
) -> i32 {
    record_hle_call("hle_nvnWindowAcquireTexture");
    if w.is_null() || (*w).texture_count == 0 {
        return 1; // NVN_WINDOW_ACQUIRE_TEXTURE_RESULT_NATIVE_ERROR
    }
    // Vsync: a texture becomes available `present interval` vblanks (60 Hz) after the previous one. Absolute deadlines
    // keep the average rate exact; if we fall far behind, resync instead of bursting.
    {
        static NEXT: StdMutex<Option<std::time::Instant>> = StdMutex::new(None);
        // Interval 0 is NVN's "no vsync": no wait here (frame-rate mods such as UltraCam use it with their own limiter;
        // a window's CAMetalLayer still paces presentation at the display's refresh rate).
        let interval = present_interval();
        let is_stock30 = crate::host::overlay::fps_mode() == crate::settings::FpsMode::Stock30;
        let frame = if is_stock30 {
            std::time::Duration::from_nanos(1_000_000_000 / 30)
        } else if interval == 0 {
            std::time::Duration::ZERO
        } else {
            std::time::Duration::from_nanos(16_666_667 * interval.max(1) as u64)
        };
        let wait = if frame.is_zero() {
            std::time::Duration::ZERO
        } else {
            let mut g = NEXT.lock().unwrap();
            let now = std::time::Instant::now();
            let deadline = match *g {
                Some(d) if d > now && d < now + frame * 2 => d,
                _ => now,
            };
            *g = Some(deadline + frame);
            deadline.saturating_duration_since(now)
        };
        ACQUIRE_CALLS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        if !wait.is_zero() && (is_stock30 || crate::host::overlay::is_vsync()) {
            ACQUIRE_SLEEP_US.fetch_add(
                wait.as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            hle_sleep_ns(wait.as_nanos() as u64);
        }
    }
    let idx = (*w).next_index % ((*w).texture_count as u32);
    (*w).next_index = (*w).next_index.wrapping_add(1);
    if !out_index.is_null() {
        *out_index = idx as i32;
    }
    0 // NVN_WINDOW_ACQUIRE_TEXTURE_RESULT_SUCCESS
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowSetCrop(
    _w: *mut NvnWindow,
    _x: i32,
    _y: i32,
    _w2: i32,
    _h: i32,
) {
    record_hle_call("hle_nvnWindowSetCrop");
    if !_w.is_null() {
        if _w2 <= 0 || _h <= 0 {
            let (tw, th) = if (*_w).textures[0] != 0 {
                let t = &*((*_w).textures[0] as *const NvnTexture);
                (t.width, t.height)
            } else {
                (0, 0)
            };
            (*_w).crop = [0, 0, tw, th];
        } else {
            (*_w).crop = [_x, _y, _w2, _h];
        }
        println!(
            "[+] NVN: WindowSetCrop x={} y={} w={} h={} (effective: {:?})",
            _x,
            _y,
            _w2,
            _h,
            (*_w).crop
        );
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnWindowSetNumActiveTextures(w: *mut NvnWindow, count: i32) {
    record_hle_call("hle_nvnWindowSetNumActiveTextures");
    if !w.is_null() {
        (*w).texture_count = count.max(1).min((*w).num_textures.max(1));
    }
}

// ---------------------------------------------------------------------------
// NVN Queue family. NVNqueue is 0x2000 (8192) bytes, NVNqueueBuilder 0x40
// (64) bytes. This was the real gap behind the free-list crash this
// session: `nvnQueueInitialize` and the rest of this family were requested
// (resolved via nvnDeviceGetProcAddress early in startup) but never
// implemented -- only `nvnQueueSubmitCommands` had a real body. Every other
// Queue call silently went to the unimplemented-stub pool, which never even
// got invoked (zero "unimplemented NVN function called" log lines for any
// nvnQueue* name across a full run) -- meaning the game's own Queue object
// never finished real construction, and whatever internal resource-
// recycling pool it owns was never seeded. `nvnQueuePresentTexture` was
// also previously misimplemented under an invented name
// (`hle_nvnWindowPresentTexture`, wrong owner type, wrong argument order)
// that doesn't exist in the real API, so the game's real present call would
// never have matched it either.
#[repr(C)]
pub(crate) struct NvnQueueBuilder {
    device: usize,
    flags: i32,
    command_mem_size: usize,
    compute_mem_size: usize,
    control_mem_size: usize,
    flush_threshold: i32, // 0 = not set
    priority: i32,
    memory: usize,
    memory_size: usize,
}
const _: () = assert!(std::mem::size_of::<NvnQueueBuilder>() <= 0x40);

#[repr(C)]
pub(crate) struct NvnQueue {
    device: usize,
    flags: i32,
    command_mem_used: usize,
    control_mem_used: usize,
    compute_mem_used: usize,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetDevice(b: *mut NvnQueueBuilder, device: usize) {
    record_hle_call("hle_nvnQueueBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetDefaults(b: *mut NvnQueueBuilder) {
    record_hle_call("hle_nvnQueueBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
        // Real NVN defaults are nonzero (a few MB each); match that order of
        // magnitude so GetQueueMemorySize/the game's own sizing math stays sane
        // even if it never calls the explicit Set*MemorySize overrides.
        (*b).command_mem_size = 0x40000;
        (*b).compute_mem_size = 0x40000;
        (*b).control_mem_size = 0x10000;
        (*b).priority = 100; // NVN_QUEUE_PRIORITY_NORMAL
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetFlags(b: *mut NvnQueueBuilder, flags: i32) {
    record_hle_call("hle_nvnQueueBuilderSetFlags");
    if !b.is_null() {
        (*b).flags = flags;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetCommandMemorySize(
    b: *mut NvnQueueBuilder,
    size: usize,
) {
    record_hle_call("hle_nvnQueueBuilderSetCommandMemorySize");
    if !b.is_null() {
        (*b).command_mem_size = size;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetComputeMemorySize(
    b: *mut NvnQueueBuilder,
    size: usize,
) {
    record_hle_call("hle_nvnQueueBuilderSetComputeMemorySize");
    if !b.is_null() {
        (*b).compute_mem_size = size;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderSetControlMemorySize(
    b: *mut NvnQueueBuilder,
    size: usize,
) {
    record_hle_call("hle_nvnQueueBuilderSetControlMemorySize");
    if !b.is_null() {
        (*b).control_mem_size = size;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueBuilderGetQueueMemorySize(b: *const NvnQueueBuilder) -> usize {
    record_hle_call("hle_nvnQueueBuilderGetQueueMemorySize");
    if b.is_null() {
        0
    } else {
        (*b).command_mem_size + (*b).compute_mem_size + (*b).control_mem_size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueInitialize(
    q: *mut NvnQueue,
    builder: *const NvnQueueBuilder,
) -> u8 {
    record_hle_call("hle_nvnQueueInitialize");
    if q.is_null() || builder.is_null() {
        return 0;
    }
    std::ptr::write(
        q,
        NvnQueue {
            device: (*builder).device,
            flags: (*builder).flags,
            command_mem_used: 0,
            control_mem_used: 0,
            compute_mem_used: 0,
        },
    );
    println!(
        "[+] NVN: Queue initialized (device={:#x}, flags={:#x})",
        (*builder).device,
        (*builder).flags
    );
    1
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFinalize(q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueFinalize");
    if !q.is_null() {
        std::ptr::write_bytes(q, 0, 1);
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFlush(_q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueFlush");
    // Real NVN kicks the GPU to start processing submitted command buffers.
    // Nothing is actually deferred on our side yet (no async GPU execution
    // model exists until the Metal-translation follow-up), so there's
    // nothing to flush -- a no-op is correct for the current execution
    // model, not a gap.
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFinish(_q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueFinish");
    // Blocks until everything submitted so far has completed on the GPU.
    crate::gpu::backend().wait_serial(crate::gpu::backend().submitted_serial(), u64::MAX);
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetError(q: *mut NvnQueue, _error_info: *mut ()) -> i32 {
    record_hle_call("hle_nvnQueueGetError");
    let _ = q;
    0 // NVN_QUEUE_GET_ERROR_RESULT_NONE
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetTotalCommandMemoryUsed(q: *mut NvnQueue) -> usize {
    record_hle_call("hle_nvnQueueGetTotalCommandMemoryUsed");
    if q.is_null() {
        0
    } else {
        (*q).command_mem_used
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetTotalControlMemoryUsed(q: *mut NvnQueue) -> usize {
    record_hle_call("hle_nvnQueueGetTotalControlMemoryUsed");
    if q.is_null() {
        0
    } else {
        (*q).control_mem_used
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueGetTotalComputeMemoryUsed(q: *mut NvnQueue) -> usize {
    record_hle_call("hle_nvnQueueGetTotalComputeMemoryUsed");
    if q.is_null() {
        0
    } else {
        (*q).compute_mem_used
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueResetMemoryUsageCounts(q: *mut NvnQueue) {
    record_hle_call("hle_nvnQueueResetMemoryUsageCounts");
    if !q.is_null() {
        (*q).command_mem_used = 0;
        (*q).control_mem_used = 0;
        (*q).compute_mem_used = 0;
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueuePresentTexture(
    _queue: *mut NvnQueue,
    _window: *mut NvnWindow,
    index: i32,
) {
    record_hle_call("hle_nvnQueuePresentTexture");
    // No waiting here: NVN queues the present and returns; the vsync / present-interval wait happens when the game
    // acquires the next window texture (see hle_nvnWindowAcquireTexture). TotK's frame pacer measures its update +
    // draw time (excluding that wait) to pick the present interval, so a wait here locked it at 20 fps.
    {
        static FRAMES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let f = FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if f == 1 || f % 600 == 0 {
            println!("[+] NVN present: frame {} (buffer index {})", f, index);
        }
        if f % 600 == 0 {
            dump_command_histogram(f);
        }
        if (crate::host::display::window_up() || crate::host::display::headless())
            && !_window.is_null()
            && index >= 0
            && (index as usize) < 4
        {
            crate::gpu::backend().present((*_window).textures[index as usize], (*_window).crop);
        }
    }
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueWaitSync(_queue: *mut NvnQueue, _sync: usize) -> u8 {
    record_hle_call("hle_nvnQueueWaitSync");
    1 // a GPU-side wait: there is one Metal queue, which already runs command buffers in order
}
#[no_mangle]
pub unsafe extern "C" fn hle_nvnQueueFenceSync(
    _queue: *mut NvnQueue,
    sync: *mut NvnSync,
    _condition: i32,
    _flags: i32,
) {
    record_hle_call("hle_nvnQueueFenceSync");
    // Signaled once all work submitted so far (submits execute during nvnQueueSubmitCommands) has completed.
    if !sync.is_null() {
        (*sync).serial = crate::gpu::backend().submitted_serial();
    }
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
fn bootstrap_fn(func_name: &str) -> Option<*const ()> {
    Some(match func_name {
        "nvnDeviceInitialize" => hle_nvnDeviceInitialize as *const (),
        "nvnDeviceGetProcAddress" => hle_nvnDeviceGetProcAddress as *const (),
        "nvnDeviceGetInteger" => hle_nvnDeviceGetInteger as *const (),
        "nvnQueueSubmitCommands" => hle_nvnQueueSubmitCommands as *const (),
        _ => return None,
    })
}

/// HLE trampolines for the functions the two lookups hand out, by function address (`set_guest_entries`).
static GUEST_ENTRIES: std::sync::OnceLock<std::collections::HashMap<usize, usize>> = std::sync::OnceLock::new();

/// Makes a trampoline (`make`) for every NVN function the game can ask for: the names are the `nvn...` strings in its
/// modules (`images`), which is where its lookups take them from. Trampolines cannot be made later (they are part of
/// the AOT image), and without one an NVN call would run on the game's stack buffer (horizon/thread_stack.rs).
pub fn set_guest_entries(images: &[(usize, usize)], make: &mut dyn FnMut(*const ()) -> usize) {
    let mut map = std::collections::HashMap::new();
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
            for f in [Some(resolve_proc(name)), bootstrap_fn(name)].into_iter().flatten() {
                map.entry(f as usize).or_insert_with(|| make(f));
            }
        }
    }
    let f = hle_nvn_generic_stub as *const ();
    map.entry(f as usize).or_insert_with(|| make(f));
    println!("[+] NVN: {} functions behind HLE trampolines", map.len());
    let _ = GUEST_ENTRIES.set(map);
}

fn guest_entry(f: *const ()) -> *const () {
    match GUEST_ENTRIES.get().and_then(|m| m.get(&(f as usize))) {
        Some(&t) => t as *const (),
        None => {
            eprintln!("[!] NVN: {:p} has no HLE trampoline; it runs on the game's stack", f);
            f
        }
    }
}

unsafe extern "C" fn hle_nvnDeviceInitialize(_device: *mut (), _params: *const ()) -> bool {
    println!("[+] NVN: Initialized virtual Tegra X1 device on Apple Silicon Metal");
    true
}

unsafe extern "C" fn hle_nvnQueueSubmitCommands(_queue: *mut (), num: i32, handles: *const u64) {
    record_hle_call("hle_nvnQueueSubmitCommands");
    if handles.is_null() || num <= 0 {
        return;
    }
    let hs: Vec<u64> = (0..num as usize)
        .map(|i| std::ptr::read_unaligned(handles.add(i)))
        .collect();
    crate::gpu::backend().submit(&hs);
    mark_command_lists_submitted(&hs);
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
