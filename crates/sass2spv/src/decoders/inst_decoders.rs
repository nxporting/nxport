#![allow(unused_parens, non_camel_case_types, clippy::all)]
// Generated from Ryujinx's Decoders/InstDecoders.cs - do not edit.

#![allow(dead_code)]

use super::register::REGISTER_ZERO_INDEX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AlSize(pub u32);

#[rustfmt::skip]
impl AlSize {
    pub const B32: Self = Self(0);
    pub const B64: Self = Self(1);
    pub const B96: Self = Self(2);
    pub const B128: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AtomSize(pub u32);

#[rustfmt::skip]
impl AtomSize {
    pub const U32: Self = Self(0);
    pub const S32: Self = Self(1);
    pub const U64: Self = Self(2);
    pub const F32_FTZ_RN: Self = Self(3);
    pub const F16X2_FTZ_RN: Self = Self(4);
    pub const S64: Self = Self(5);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AtomOp(pub u32);

#[rustfmt::skip]
impl AtomOp {
    pub const ADD: Self = Self(0);
    pub const MIN: Self = Self(1);
    pub const MAX: Self = Self(2);
    pub const INC: Self = Self(3);
    pub const DEC: Self = Self(4);
    pub const AND: Self = Self(5);
    pub const OR: Self = Self(6);
    pub const XOR: Self = Self(7);
    pub const EXCH: Self = Self(8);
    pub const SAFEADD: Self = Self(10);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AtomsSize(pub u32);

#[rustfmt::skip]
impl AtomsSize {
    pub const U32: Self = Self(0);
    pub const S32: Self = Self(1);
    pub const U64: Self = Self(2);
    pub const S64: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BarMode(pub u32);

#[rustfmt::skip]
impl BarMode {
    pub const BAR: Self = Self(0);
    pub const RESULT: Self = Self(1);
    pub const WARP: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BarOp(pub u32);

#[rustfmt::skip]
impl BarOp {
    pub const SYNC: Self = Self(0);
    pub const ARV: Self = Self(1);
    pub const RED: Self = Self(2);
    pub const SCAN: Self = Self(3);
    pub const SYNC_ALL: Self = Self(4);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BarRedOp(pub u32);

#[rustfmt::skip]
impl BarRedOp {
    pub const POPC: Self = Self(0);
    pub const AND: Self = Self(1);
    pub const OR: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Bpt(pub u32);

#[rustfmt::skip]
impl Bpt {
    pub const DRAIN_ILLEGAL: Self = Self(0);
    pub const CAL: Self = Self(1);
    pub const PAUSE: Self = Self(2);
    pub const TRAP: Self = Self(3);
    pub const INT: Self = Self(4);
    pub const DRAIN: Self = Self(5);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ccc(pub u32);

#[rustfmt::skip]
impl Ccc {
    pub const F: Self = Self(0);
    pub const LT: Self = Self(1);
    pub const EQ: Self = Self(2);
    pub const LE: Self = Self(3);
    pub const GT: Self = Self(4);
    pub const NE: Self = Self(5);
    pub const GE: Self = Self(6);
    pub const NUM: Self = Self(7);
    pub const NAN: Self = Self(8);
    pub const LTU: Self = Self(9);
    pub const EQU: Self = Self(10);
    pub const LEU: Self = Self(11);
    pub const GTU: Self = Self(12);
    pub const NEU: Self = Self(13);
    pub const GEU: Self = Self(14);
    pub const T: Self = Self(15);
    pub const OFF: Self = Self(16);
    pub const LO: Self = Self(17);
    pub const SFF: Self = Self(18);
    pub const LS: Self = Self(19);
    pub const HI: Self = Self(20);
    pub const SFT: Self = Self(21);
    pub const HS: Self = Self(22);
    pub const OFT: Self = Self(23);
    pub const CSM_TA: Self = Self(24);
    pub const CSM_TR: Self = Self(25);
    pub const CSM_MX: Self = Self(26);
    pub const FCSM_TA: Self = Self(27);
    pub const FCSM_TR: Self = Self(28);
    pub const FCSM_MX: Self = Self(29);
    pub const RLE: Self = Self(30);
    pub const RGT: Self = Self(31);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheType(pub u32);

#[rustfmt::skip]
impl CacheType {
    pub const U: Self = Self(1);
    pub const C: Self = Self(2);
    pub const I: Self = Self(3);
    pub const CRS: Self = Self(4);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CctlOp(pub u32);

#[rustfmt::skip]
impl CctlOp {
    pub const PF1: Self = Self(1);
    pub const PF1_5: Self = Self(2);
    pub const PF2: Self = Self(3);
    pub const WB: Self = Self(4);
    pub const IV: Self = Self(5);
    pub const IVALL: Self = Self(6);
    pub const RS: Self = Self(7);
    pub const RSLB: Self = Self(9);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CctltOp(pub u32);

#[rustfmt::skip]
impl CctltOp {
    pub const IVTH: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BoolOp(pub u32);

#[rustfmt::skip]
impl BoolOp {
    pub const AND: Self = Self(0);
    pub const OR: Self = Self(1);
    pub const XOR: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SReg(pub u32);

#[rustfmt::skip]
impl SReg {
    pub const LANE_ID: Self = Self(0);
    pub const CLOCK: Self = Self(1);
    pub const VIRT_CFG: Self = Self(2);
    pub const VIRT_ID: Self = Self(3);
    pub const PM0: Self = Self(4);
    pub const PM1: Self = Self(5);
    pub const PM2: Self = Self(6);
    pub const PM3: Self = Self(7);
    pub const PM4: Self = Self(8);
    pub const PM5: Self = Self(9);
    pub const PM6: Self = Self(10);
    pub const PM7: Self = Self(11);
    pub const ORDERING_TICKET: Self = Self(15);
    pub const PRIM_TYPE: Self = Self(16);
    pub const INVOCATION_ID: Self = Self(17);
    pub const Y_DIRECTION: Self = Self(18);
    pub const THREAD_KILL: Self = Self(19);
    pub const SHADER_TYPE: Self = Self(20);
    pub const DIRECT_CBE_WRITE_ADDRESS_LOW: Self = Self(21);
    pub const DIRECT_CBE_WRITE_ADDRESS_HIGH: Self = Self(22);
    pub const DIRECT_CBE_WRITE_ENABLED: Self = Self(23);
    pub const MACHINE_ID0: Self = Self(24);
    pub const MACHINE_ID1: Self = Self(25);
    pub const MACHINE_ID2: Self = Self(26);
    pub const MACHINE_ID3: Self = Self(27);
    pub const AFFINITY: Self = Self(28);
    pub const INVOCATION_INFO: Self = Self(29);
    pub const W_SCALE_FACTOR_XY: Self = Self(30);
    pub const W_SCALE_FACTOR_Z: Self = Self(31);
    pub const T_ID: Self = Self(32);
    pub const T_ID_X: Self = Self(33);
    pub const T_ID_Y: Self = Self(34);
    pub const T_ID_Z: Self = Self(35);
    pub const CTA_PARAM: Self = Self(36);
    pub const CTA_ID_X: Self = Self(37);
    pub const CTA_ID_Y: Self = Self(38);
    pub const CTA_ID_Z: Self = Self(39);
    pub const NTID: Self = Self(40);
    pub const CIR_QUEUE_INCR_MINUS_ONE: Self = Self(41);
    pub const NLATC: Self = Self(42);
    pub const SWINLO: Self = Self(48);
    pub const SWINSZ: Self = Self(49);
    pub const SMEMSZ: Self = Self(50);
    pub const SMEMBANKS: Self = Self(51);
    pub const L_WIN_LO: Self = Self(52);
    pub const L_WIN_SZ: Self = Self(53);
    pub const L_MEM_LO_SZ: Self = Self(54);
    pub const L_MEM_HI_OFF: Self = Self(55);
    pub const EQ_MASK: Self = Self(56);
    pub const LT_MASK: Self = Self(57);
    pub const LE_MASK: Self = Self(58);
    pub const GT_MASK: Self = Self(59);
    pub const GE_MASK: Self = Self(60);
    pub const REG_ALLOC: Self = Self(61);
    pub const CTX_ADDR: Self = Self(62);
    pub const GLOBAL_ERROR_STATUS: Self = Self(64);
    pub const WARP_ERROR_STATUS: Self = Self(66);
    pub const WARP_ERROR_STATUS_CLEAR: Self = Self(67);
    pub const PM_HI0: Self = Self(72);
    pub const PM_HI1: Self = Self(73);
    pub const PM_HI2: Self = Self(74);
    pub const PM_HI3: Self = Self(75);
    pub const PM_HI4: Self = Self(76);
    pub const PM_HI5: Self = Self(77);
    pub const PM_HI6: Self = Self(78);
    pub const PM_HI7: Self = Self(79);
    pub const CLOCK_LO: Self = Self(80);
    pub const CLOCK_HI: Self = Self(81);
    pub const GLOBAL_TIMER_LO: Self = Self(82);
    pub const GLOBAL_TIMER_HI: Self = Self(83);
    pub const HW_TASK_ID: Self = Self(96);
    pub const CIRCULAR_QUEUE_ENTRY_INDEX: Self = Self(97);
    pub const CIRCULAR_QUEUE_ENTRY_ADDRESS_LOW: Self = Self(98);
    pub const CIRCULAR_QUEUE_ENTRY_ADDRESS_HIGH: Self = Self(99);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoundMode(pub u32);

#[rustfmt::skip]
impl RoundMode {
    pub const RN: Self = Self(0);
    pub const RM: Self = Self(1);
    pub const RP: Self = Self(2);
    pub const RZ: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FComp(pub u32);

#[rustfmt::skip]
impl FComp {
    pub const F: Self = Self(0);
    pub const LT: Self = Self(1);
    pub const EQ: Self = Self(2);
    pub const LE: Self = Self(3);
    pub const GT: Self = Self(4);
    pub const NE: Self = Self(5);
    pub const GE: Self = Self(6);
    pub const NUM: Self = Self(7);
    pub const NAN: Self = Self(8);
    pub const LTU: Self = Self(9);
    pub const EQU: Self = Self(10);
    pub const LEU: Self = Self(11);
    pub const GTU: Self = Self(12);
    pub const NEU: Self = Self(13);
    pub const GEU: Self = Self(14);
    pub const T: Self = Self(15);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IntegerRound(pub u32);

#[rustfmt::skip]
impl IntegerRound {
    pub const PASS: Self = Self(1);
    pub const ROUND: Self = Self(4);
    pub const FLOOR: Self = Self(5);
    pub const CEIL: Self = Self(6);
    pub const TRUNC: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IDstFmt(pub u32);

#[rustfmt::skip]
impl IDstFmt {
    pub const U16: Self = Self(1);
    pub const U32: Self = Self(2);
    pub const U64: Self = Self(3);
    pub const S16: Self = Self(5);
    pub const S32: Self = Self(6);
    pub const S64: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ISrcFmt(pub u32);

#[rustfmt::skip]
impl ISrcFmt {
    pub const U8: Self = Self(0);
    pub const U16: Self = Self(1);
    pub const U32: Self = Self(2);
    pub const U64: Self = Self(3);
    pub const S8: Self = Self(4);
    pub const S16: Self = Self(5);
    pub const S32: Self = Self(6);
    pub const S64: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ISrcDstFmt(pub u32);

#[rustfmt::skip]
impl ISrcDstFmt {
    pub const U8: Self = Self(0);
    pub const U16: Self = Self(1);
    pub const U32: Self = Self(2);
    pub const S8: Self = Self(4);
    pub const S16: Self = Self(5);
    pub const S32: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoundMode2(pub u32);

#[rustfmt::skip]
impl RoundMode2 {
    pub const ROUND: Self = Self(0);
    pub const FLOOR: Self = Self(1);
    pub const CEIL: Self = Self(2);
    pub const TRUNC: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ChkModeF(pub u32);

#[rustfmt::skip]
impl ChkModeF {
    pub const DIVIDE: Self = Self(0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Fmz(pub u32);

#[rustfmt::skip]
impl Fmz {
    pub const FTZ: Self = Self(1);
    pub const FMZ: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MultiplyScale(pub u32);

#[rustfmt::skip]
impl MultiplyScale {
    pub const NO_SCALE: Self = Self(0);
    pub const D2: Self = Self(1);
    pub const D4: Self = Self(2);
    pub const D8: Self = Self(3);
    pub const M8: Self = Self(4);
    pub const M4: Self = Self(5);
    pub const M2: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OFmt(pub u32);

#[rustfmt::skip]
impl OFmt {
    pub const F16: Self = Self(0);
    pub const F32: Self = Self(1);
    pub const MRG_H0: Self = Self(2);
    pub const MRG_H1: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HalfSwizzle(pub u32);

#[rustfmt::skip]
impl HalfSwizzle {
    pub const F16: Self = Self(0);
    pub const F32: Self = Self(1);
    pub const H0_H0: Self = Self(2);
    pub const H1_H1: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ByteSel(pub u32);

#[rustfmt::skip]
impl ByteSel {
    pub const B0: Self = Self(0);
    pub const B1: Self = Self(1);
    pub const B2: Self = Self(2);
    pub const B3: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DstFmt(pub u32);

#[rustfmt::skip]
impl DstFmt {
    pub const F16: Self = Self(1);
    pub const F32: Self = Self(2);
    pub const F64: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AvgMode(pub u32);

#[rustfmt::skip]
impl AvgMode {
    pub const NO_NEG: Self = Self(0);
    pub const NEG_B: Self = Self(1);
    pub const NEG_A: Self = Self(2);
    pub const PLUS_ONE: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lrs(pub u32);

#[rustfmt::skip]
impl Lrs {
    pub const NONE: Self = Self(0);
    pub const RIGHT_SHIFT: Self = Self(1);
    pub const LEFT_SHIFT: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HalfSelect(pub u32);

#[rustfmt::skip]
impl HalfSelect {
    pub const B32: Self = Self(0);
    pub const H0: Self = Self(1);
    pub const H1: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IComp(pub u32);

#[rustfmt::skip]
impl IComp {
    pub const F: Self = Self(0);
    pub const LT: Self = Self(1);
    pub const EQ: Self = Self(2);
    pub const LE: Self = Self(3);
    pub const GT: Self = Self(4);
    pub const NE: Self = Self(5);
    pub const GE: Self = Self(6);
    pub const T: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct XMode(pub u32);

#[rustfmt::skip]
impl XMode {
    pub const XLO: Self = Self(1);
    pub const XMED: Self = Self(2);
    pub const XHI: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IpaOp(pub u32);

#[rustfmt::skip]
impl IpaOp {
    pub const PASS: Self = Self(0);
    pub const MULTIPLY: Self = Self(1);
    pub const CONSTANT: Self = Self(2);
    pub const SC: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IBase(pub u32);

#[rustfmt::skip]
impl IBase {
    pub const PATCH: Self = Self(1);
    pub const PRIM: Self = Self(2);
    pub const ATTR: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheOpLd(pub u32);

#[rustfmt::skip]
impl CacheOpLd {
    pub const CA: Self = Self(0);
    pub const CG: Self = Self(1);
    pub const CI: Self = Self(2);
    pub const CV: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheOpSt(pub u32);

#[rustfmt::skip]
impl CacheOpSt {
    pub const WB: Self = Self(0);
    pub const CG: Self = Self(1);
    pub const CI: Self = Self(2);
    pub const WT: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LsSize(pub u32);

#[rustfmt::skip]
impl LsSize {
    pub const U8: Self = Self(0);
    pub const S8: Self = Self(1);
    pub const U16: Self = Self(2);
    pub const S16: Self = Self(3);
    pub const B32: Self = Self(4);
    pub const B64: Self = Self(5);
    pub const B128: Self = Self(6);
    pub const UB128: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LsSize2(pub u32);

#[rustfmt::skip]
impl LsSize2 {
    pub const U8: Self = Self(0);
    pub const S8: Self = Self(1);
    pub const U16: Self = Self(2);
    pub const S16: Self = Self(3);
    pub const B32: Self = Self(4);
    pub const B64: Self = Self(5);
    pub const B128: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AddressMode(pub u32);

#[rustfmt::skip]
impl AddressMode {
    pub const IL: Self = Self(1);
    pub const IS: Self = Self(2);
    pub const ISL: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CacheOp2(pub u32);

#[rustfmt::skip]
impl CacheOp2 {
    pub const LU: Self = Self(1);
    pub const CI: Self = Self(2);
    pub const CV: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PredicateOp(pub u32);

#[rustfmt::skip]
impl PredicateOp {
    pub const F: Self = Self(0);
    pub const T: Self = Self(1);
    pub const Z: Self = Self(2);
    pub const NZ: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LogicOp(pub u32);

#[rustfmt::skip]
impl LogicOp {
    pub const AND: Self = Self(0);
    pub const OR: Self = Self(1);
    pub const XOR: Self = Self(2);
    pub const PASS_B: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Membar(pub u32);

#[rustfmt::skip]
impl Membar {
    pub const CTA: Self = Self(0);
    pub const GL: Self = Self(1);
    pub const SYS: Self = Self(2);
    pub const VC: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ivall(pub u32);

#[rustfmt::skip]
impl Ivall {
    pub const IVALLD: Self = Self(1);
    pub const IVALLT: Self = Self(2);
    pub const IVALLTD: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MufuOp(pub u32);

#[rustfmt::skip]
impl MufuOp {
    pub const COS: Self = Self(0);
    pub const SIN: Self = Self(1);
    pub const EX2: Self = Self(2);
    pub const LG2: Self = Self(3);
    pub const RCP: Self = Self(4);
    pub const RSQ: Self = Self(5);
    pub const RCP64H: Self = Self(6);
    pub const RSQ64H: Self = Self(7);
    pub const SQRT: Self = Self(8);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutType(pub u32);

#[rustfmt::skip]
impl OutType {
    pub const EMIT: Self = Self(1);
    pub const CUT: Self = Self(2);
    pub const EMIT_THEN_CUT: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PixMode(pub u32);

#[rustfmt::skip]
impl PixMode {
    pub const COVMASK: Self = Self(1);
    pub const COVERED: Self = Self(2);
    pub const OFFSET: Self = Self(3);
    pub const CENTROID_OFFSET: Self = Self(4);
    pub const MY_INDEX: Self = Self(5);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PMode(pub u32);

#[rustfmt::skip]
impl PMode {
    pub const F4E: Self = Self(1);
    pub const B4E: Self = Self(2);
    pub const RC8: Self = Self(3);
    pub const ECL: Self = Self(4);
    pub const ECR: Self = Self(5);
    pub const RC16: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RedOp(pub u32);

#[rustfmt::skip]
impl RedOp {
    pub const ADD: Self = Self(0);
    pub const MIN: Self = Self(1);
    pub const MAX: Self = Self(2);
    pub const INC: Self = Self(3);
    pub const DEC: Self = Self(4);
    pub const AND: Self = Self(5);
    pub const OR: Self = Self(6);
    pub const XOR: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct XModeShf(pub u32);

#[rustfmt::skip]
impl XModeShf {
    pub const HI: Self = Self(1);
    pub const X: Self = Self(2);
    pub const XHI: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MaxShift(pub u32);

#[rustfmt::skip]
impl MaxShift {
    pub const U64: Self = Self(2);
    pub const S64: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ShflMode(pub u32);

#[rustfmt::skip]
impl ShflMode {
    pub const IDX: Self = Self(0);
    pub const UP: Self = Self(1);
    pub const DOWN: Self = Self(2);
    pub const BFLY: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Clamp(pub u32);

#[rustfmt::skip]
impl Clamp {
    pub const IGN: Self = Self(0);
    pub const TRAP: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SuatomSize(pub u32);

#[rustfmt::skip]
impl SuatomSize {
    pub const U32: Self = Self(0);
    pub const S32: Self = Self(1);
    pub const U64: Self = Self(2);
    pub const F32_FTZ_RN: Self = Self(3);
    pub const F16X2_FTZ_RN: Self = Self(4);
    pub const S64: Self = Self(5);
    pub const SD32: Self = Self(6);
    pub const SD64: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SuDim(pub u32);

#[rustfmt::skip]
impl SuDim {
    pub const D1D: Self = Self(0);
    pub const D1D_BUFFER: Self = Self(1);
    pub const D1D_ARRAY: Self = Self(2);
    pub const D2D: Self = Self(3);
    pub const D2D_ARRAY: Self = Self(4);
    pub const D3D: Self = Self(5);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SuatomOp(pub u32);

#[rustfmt::skip]
impl SuatomOp {
    pub const ADD: Self = Self(0);
    pub const MIN: Self = Self(1);
    pub const MAX: Self = Self(2);
    pub const INC: Self = Self(3);
    pub const DEC: Self = Self(4);
    pub const AND: Self = Self(5);
    pub const OR: Self = Self(6);
    pub const XOR: Self = Self(7);
    pub const EXCH: Self = Self(8);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SuSize(pub u32);

#[rustfmt::skip]
impl SuSize {
    pub const U8: Self = Self(0);
    pub const S8: Self = Self(1);
    pub const U16: Self = Self(2);
    pub const S16: Self = Self(3);
    pub const B32: Self = Self(4);
    pub const B64: Self = Self(5);
    pub const B128: Self = Self(6);
    pub const UB128: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SuRgba(pub u32);

#[rustfmt::skip]
impl SuRgba {
    pub const R: Self = Self(1);
    pub const G: Self = Self(2);
    pub const RG: Self = Self(3);
    pub const B: Self = Self(4);
    pub const RB: Self = Self(5);
    pub const GB: Self = Self(6);
    pub const RGB: Self = Self(7);
    pub const A: Self = Self(8);
    pub const RA: Self = Self(9);
    pub const GA: Self = Self(10);
    pub const RGA: Self = Self(11);
    pub const BA: Self = Self(12);
    pub const RBA: Self = Self(13);
    pub const GBA: Self = Self(14);
    pub const RGBA: Self = Self(15);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Lod(pub u32);

#[rustfmt::skip]
impl Lod {
    pub const LZ: Self = Self(1);
    pub const LB: Self = Self(2);
    pub const LL: Self = Self(3);
    pub const LBA: Self = Self(6);
    pub const LLA: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexDim(pub u32);

#[rustfmt::skip]
impl TexDim {
    pub const D1D: Self = Self(0);
    pub const ARRAY1D: Self = Self(1);
    pub const D2D: Self = Self(2);
    pub const ARRAY2D: Self = Self(3);
    pub const D3D: Self = Self(4);
    pub const ARRAY3D: Self = Self(5);
    pub const CUBE: Self = Self(6);
    pub const ARRAY_CUBE: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexsTarget(pub u32);

#[rustfmt::skip]
impl TexsTarget {
    pub const TEXTURE1_D_LOD_ZERO: Self = Self(0);
    pub const TEXTURE2_D: Self = Self(1);
    pub const TEXTURE2_D_LOD_ZERO: Self = Self(2);
    pub const TEXTURE2_D_LOD_LEVEL: Self = Self(3);
    pub const TEXTURE2_D_DEPTH_COMPARE: Self = Self(4);
    pub const TEXTURE2_D_LOD_LEVEL_DEPTH_COMPARE: Self = Self(5);
    pub const TEXTURE2_D_LOD_ZERO_DEPTH_COMPARE: Self = Self(6);
    pub const TEXTURE2_D_ARRAY: Self = Self(7);
    pub const TEXTURE2_D_ARRAY_LOD_ZERO: Self = Self(8);
    pub const TEXTURE2_D_ARRAY_LOD_ZERO_DEPTH_COMPARE: Self = Self(9);
    pub const TEXTURE3_D: Self = Self(10);
    pub const TEXTURE3_D_LOD_ZERO: Self = Self(11);
    pub const TEXTURE_CUBE: Self = Self(12);
    pub const TEXTURE_CUBE_LOD_LEVEL: Self = Self(13);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TldsTarget(pub u32);

#[rustfmt::skip]
impl TldsTarget {
    pub const TEXTURE1_D_LOD_ZERO: Self = Self(0x0);
    pub const TEXTURE1_D_LOD_LEVEL: Self = Self(0x1);
    pub const TEXTURE2_D_LOD_ZERO: Self = Self(0x2);
    pub const TEXTURE2_D_LOD_ZERO_OFFSET: Self = Self(0x4);
    pub const TEXTURE2_D_LOD_LEVEL: Self = Self(0x5);
    pub const TEXTURE2_D_LOD_ZERO_MULTISAMPLE: Self = Self(0x6);
    pub const TEXTURE3_D_LOD_ZERO: Self = Self(0x7);
    pub const TEXTURE2_D_ARRAY_LOD_ZERO: Self = Self(0x8);
    pub const TEXTURE2_D_LOD_LEVEL_OFFSET: Self = Self(0xc);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexComp(pub u32);

#[rustfmt::skip]
impl TexComp {
    pub const R: Self = Self(0);
    pub const G: Self = Self(1);
    pub const B: Self = Self(2);
    pub const A: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexOffset(pub u32);

#[rustfmt::skip]
impl TexOffset {
    pub const NONE: Self = Self(0);
    pub const AOFFI: Self = Self(1);
    pub const PTP: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TexQuery(pub u32);

#[rustfmt::skip]
impl TexQuery {
    pub const TEX_HEADER_DIMENSION: Self = Self(1);
    pub const TEX_HEADER_TEXTURE_TYPE: Self = Self(2);
    pub const TEX_HEADER_SAMPLER_POS: Self = Self(5);
    pub const TEX_SAMPLER_FILTER: Self = Self(16);
    pub const TEX_SAMPLER_LOD: Self = Self(18);
    pub const TEX_SAMPLER_WRAP: Self = Self(20);
    pub const TEX_SAMPLER_BORDER_COLOR: Self = Self(22);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VectorSelect(pub u32);

#[rustfmt::skip]
impl VectorSelect {
    pub const U8_B0: Self = Self(0);
    pub const U8_B1: Self = Self(1);
    pub const U8_B2: Self = Self(2);
    pub const U8_B3: Self = Self(3);
    pub const U16_H0: Self = Self(4);
    pub const U16_H1: Self = Self(5);
    pub const U32: Self = Self(6);
    pub const S8_B0: Self = Self(8);
    pub const S8_B1: Self = Self(9);
    pub const S8_B2: Self = Self(10);
    pub const S8_B3: Self = Self(11);
    pub const S16_H0: Self = Self(12);
    pub const S16_H1: Self = Self(13);
    pub const S32: Self = Self(14);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VideoOp(pub u32);

#[rustfmt::skip]
impl VideoOp {
    pub const MRG16H: Self = Self(0);
    pub const MRG16L: Self = Self(1);
    pub const MRG8B0: Self = Self(2);
    pub const MRG8B2: Self = Self(3);
    pub const ACC: Self = Self(4);
    pub const MIN: Self = Self(5);
    pub const MAX: Self = Self(6);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VideoRed(pub u32);

#[rustfmt::skip]
impl VideoRed {
    pub const ACC: Self = Self(1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LaneMask4(pub u32);

#[rustfmt::skip]
impl LaneMask4 {
    pub const Z: Self = Self(1);
    pub const W: Self = Self(2);
    pub const ZW: Self = Self(3);
    pub const X: Self = Self(4);
    pub const XZ: Self = Self(5);
    pub const XW: Self = Self(6);
    pub const XZW: Self = Self(7);
    pub const Y: Self = Self(8);
    pub const YZ: Self = Self(9);
    pub const YW: Self = Self(10);
    pub const YZW: Self = Self(11);
    pub const XY: Self = Self(12);
    pub const XYZ: Self = Self(13);
    pub const XYW: Self = Self(14);
    pub const XYZW: Self = Self(15);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ASelect4(pub u32);

#[rustfmt::skip]
impl ASelect4 {
    pub const S0000: Self = Self(0);
    pub const S1111: Self = Self(1);
    pub const S2222: Self = Self(2);
    pub const S3333: Self = Self(3);
    pub const S3210: Self = Self(4);
    pub const S5432: Self = Self(6);
    pub const S6543: Self = Self(7);
    pub const S3201: Self = Self(8);
    pub const S3012: Self = Self(9);
    pub const S0213: Self = Self(10);
    pub const S3120: Self = Self(11);
    pub const S1230: Self = Self(12);
    pub const S2310: Self = Self(13);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BSelect4(pub u32);

#[rustfmt::skip]
impl BSelect4 {
    pub const S4444: Self = Self(0);
    pub const S5555: Self = Self(1);
    pub const S6666: Self = Self(2);
    pub const S7777: Self = Self(3);
    pub const S7654: Self = Self(4);
    pub const S5432: Self = Self(6);
    pub const S4321: Self = Self(7);
    pub const S4567: Self = Self(8);
    pub const S6745: Self = Self(9);
    pub const S5476: Self = Self(10);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VideoScale(pub u32);

#[rustfmt::skip]
impl VideoScale {
    pub const SHR7: Self = Self(1);
    pub const SHR15: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct VoteMode(pub u32);

#[rustfmt::skip]
impl VoteMode {
    pub const ALL: Self = Self(0);
    pub const ANY: Self = Self(1);
    pub const EQ: Self = Self(2);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct XmadCop(pub u32);

#[rustfmt::skip]
impl XmadCop {
    pub const CFULL: Self = Self(0);
    pub const CLO: Self = Self(1);
    pub const CHI: Self = Self(2);
    pub const CSFU: Self = Self(3);
    pub const CBCC: Self = Self(4);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct XmadCop2(pub u32);

#[rustfmt::skip]
impl XmadCop2 {
    pub const CFULL: Self = Self(0);
    pub const CLO: Self = Self(1);
    pub const CHI: Self = Self(2);
    pub const CSFU: Self = Self(3);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImadspASelect(pub u32);

#[rustfmt::skip]
impl ImadspASelect {
    pub const U32: Self = Self(0);
    pub const S32: Self = Self(1);
    pub const U24: Self = Self(2);
    pub const S24: Self = Self(3);
    pub const U16H0: Self = Self(4);
    pub const S16H0: Self = Self(5);
    pub const U16H1: Self = Self(6);
    pub const S16H1: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImadspBSelect(pub u32);

#[rustfmt::skip]
impl ImadspBSelect {
    pub const U24: Self = Self(0);
    pub const S24: Self = Self(1);
    pub const U16H0: Self = Self(2);
    pub const S16H0: Self = Self(3);
}

#[derive(Clone, Copy, Debug)]
pub struct InstConditional(pub u64);

#[rustfmt::skip]
impl InstConditional {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstAl2p(pub u64);

#[rustfmt::skip]
impl InstAl2p {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn al_size(&self) -> AlSize { AlSize(((self.0 >> 47) & 0x3) as u32) }
    pub fn aio(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn imm11(&self) -> i32 { (((self.0 >> 20) & 0x7FF) as i32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 44) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstAld(pub u64);

#[rustfmt::skip]
impl InstAld {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm11(&self) -> i32 { (((self.0 >> 20) & 0x7FF) as i32) }
    pub fn p(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn o(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn al_size(&self) -> AlSize { AlSize(((self.0 >> 47) & 0x3) as u32) }
    pub fn phys(&self) -> bool { !self.p() && self.imm11() == 0 && self.src_a() != REGISTER_ZERO_INDEX }
}

#[derive(Clone, Copy, Debug)]
pub struct InstAst(pub u64);

#[rustfmt::skip]
impl InstAst {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm11(&self) -> i32 { (((self.0 >> 20) & 0x7FF) as i32) }
    pub fn p(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn al_size(&self) -> AlSize { AlSize(((self.0 >> 47) & 0x3) as u32) }
    pub fn phys(&self) -> bool { !self.p() && self.imm11() == 0 && self.src_a() != REGISTER_ZERO_INDEX }
}

#[derive(Clone, Copy, Debug)]
pub struct InstAtom(pub u64);

#[rustfmt::skip]
impl InstAtom {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm20(&self) -> i32 { (((self.0 >> 28) & 0xFFFFF) as i32) }
    pub fn size(&self) -> AtomSize { AtomSize(((self.0 >> 49) & 0x7) as u32) }
    pub fn op(&self) -> AtomOp { AtomOp(((self.0 >> 52) & 0xF) as u32) }
    pub fn e(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstAtomCas(pub u64);

#[rustfmt::skip]
impl InstAtomCas {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn bc_rz(&self) -> i32 { (((self.0 >> 50) & 0x3) as i32) }
    pub fn e(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstAtoms(pub u64);

#[rustfmt::skip]
impl InstAtoms {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm22(&self) -> i32 { (((self.0 >> 30) & 0x3FFFFF) as i32) }
    pub fn atoms_size(&self) -> AtomsSize { AtomsSize(((self.0 >> 28) & 0x3) as u32) }
    pub fn atom_op(&self) -> AtomOp { AtomOp(((self.0 >> 52) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstAtomsCas(pub u64);

#[rustfmt::skip]
impl InstAtomsCas {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn atoms_bc_rz(&self) -> i32 { (((self.0 >> 28) & 0x3) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstB2r(pub u64);

#[rustfmt::skip]
impl InstB2r {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 45) & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn mode(&self) -> BarMode { BarMode(((self.0 >> 32) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBar(pub u64);

#[rustfmt::skip]
impl InstBar {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm12(&self) -> i32 { (((self.0 >> 20) & 0xFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn bar_op(&self) -> BarOp { BarOp(((self.0 >> 32) & 0x7) as u32) }
    pub fn bar_red_op(&self) -> BarRedOp { BarRedOp(((self.0 >> 35) & 0x3) as u32) }
    pub fn a_fix_bar(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn b_fix_bar(&self) -> bool { (self.0 & 0x80000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBfeR(pub u64);

#[rustfmt::skip]
impl InstBfeR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn brev(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBfeI(pub u64);

#[rustfmt::skip]
impl InstBfeI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn brev(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBfeC(pub u64);

#[rustfmt::skip]
impl InstBfeC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn brev(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBfiR(pub u64);

#[rustfmt::skip]
impl InstBfiR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBfiI(pub u64);

#[rustfmt::skip]
impl InstBfiI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBfiC(pub u64);

#[rustfmt::skip]
impl InstBfiC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBfiRc(pub u64);

#[rustfmt::skip]
impl InstBfiRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBpt(pub u64);

#[rustfmt::skip]
impl InstBpt {
    pub fn imm20(&self) -> i32 { (((self.0 >> 20) & 0xFFFFF) as i32) }
    pub fn bpt(&self) -> Bpt { Bpt(((self.0 >> 6) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBra(pub u64);

#[rustfmt::skip]
impl InstBra {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
    pub fn lmt(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn u(&self) -> bool { (self.0 & 0x80) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBrk(pub u64);

#[rustfmt::skip]
impl InstBrk {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstBrx(pub u64);

#[rustfmt::skip]
impl InstBrx {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
    pub fn lmt(&self) -> bool { (self.0 & 0x40) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCal(pub u64);

#[rustfmt::skip]
impl InstCal {
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn inc(&self) -> bool { (self.0 & 0x40) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCctl(pub u64);

#[rustfmt::skip]
impl InstCctl {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm30(&self) -> i32 { (((self.0 >> 22) & 0x3FFFFFFF) as i32) }
    pub fn e(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn cache(&self) -> CacheType { CacheType(((self.0 >> 4) & 0x7) as u32) }
    pub fn cctl_op(&self) -> CctlOp { CctlOp((self.0 & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCctll(pub u64);

#[rustfmt::skip]
impl InstCctll {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm22(&self) -> i32 { (((self.0 >> 22) & 0x3FFFFF) as i32) }
    pub fn cache(&self) -> i32 { (((self.0 >> 4) & 0x3) as i32) }
    pub fn cctl_op(&self) -> CctlOp { CctlOp((self.0 & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCctlt(pub u64);

#[rustfmt::skip]
impl InstCctlt {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ts_idx13(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn cctlt_op(&self) -> CctltOp { CctltOp((self.0 & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCctltR(pub u64);

#[rustfmt::skip]
impl InstCctltR {
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cctlt_op(&self) -> CctltOp { CctltOp((self.0 & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCont(pub u64);

#[rustfmt::skip]
impl InstCont {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCset(pub u64);

#[rustfmt::skip]
impl InstCset {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc(((self.0 >> 8) & 0x1F) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCsetp(pub u64);

#[rustfmt::skip]
impl InstCsetp {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc(((self.0 >> 8) & 0x1F) as u32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstCs2r(pub u64);

#[rustfmt::skip]
impl InstCs2r {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn s_reg(&self) -> SReg { SReg(((self.0 >> 20) & 0xFF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDaddR(pub u64);

#[rustfmt::skip]
impl InstDaddR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDaddI(pub u64);

#[rustfmt::skip]
impl InstDaddI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDaddC(pub u64);

#[rustfmt::skip]
impl InstDaddC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDepbar(pub u64);

#[rustfmt::skip]
impl InstDepbar {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn le(&self) -> bool { (self.0 & 0x20000000) != 0 }
    pub fn sbid(&self) -> i32 { (((self.0 >> 26) & 0x7) as i32) }
    pub fn pend_cnt(&self) -> i32 { (((self.0 >> 20) & 0x3F) as i32) }
    pub fn imm6(&self) -> i32 { ((self.0 & 0x3F) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDfmaR(pub u64);

#[rustfmt::skip]
impl InstDfmaR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 50) & 0x3) as u32) }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDfmaI(pub u64);

#[rustfmt::skip]
impl InstDfmaI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 50) & 0x3) as u32) }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDfmaC(pub u64);

#[rustfmt::skip]
impl InstDfmaC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 50) & 0x3) as u32) }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDfmaRc(pub u64);

#[rustfmt::skip]
impl InstDfmaRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 50) & 0x3) as u32) }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDmnmxR(pub u64);

#[rustfmt::skip]
impl InstDmnmxR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDmnmxI(pub u64);

#[rustfmt::skip]
impl InstDmnmxI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDmnmxC(pub u64);

#[rustfmt::skip]
impl InstDmnmxC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDmulR(pub u64);

#[rustfmt::skip]
impl InstDmulR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDmulI(pub u64);

#[rustfmt::skip]
impl InstDmulI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDmulC(pub u64);

#[rustfmt::skip]
impl InstDmulC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDsetR(pub u64);

#[rustfmt::skip]
impl InstDsetR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDsetI(pub u64);

#[rustfmt::skip]
impl InstDsetI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDsetC(pub u64);

#[rustfmt::skip]
impl InstDsetC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDsetpR(pub u64);

#[rustfmt::skip]
impl InstDsetpR {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn abs_a(&self) -> bool { (self.0 & 0x80) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDsetpI(pub u64);

#[rustfmt::skip]
impl InstDsetpI {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn abs_a(&self) -> bool { (self.0 & 0x80) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstDsetpC(pub u64);

#[rustfmt::skip]
impl InstDsetpC {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn abs_a(&self) -> bool { (self.0 & 0x80) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstExit(pub u64);

#[rustfmt::skip]
impl InstExit {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
    pub fn keep_ref_cnt(&self) -> bool { (self.0 & 0x20) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstF2fR(pub u64);

#[rustfmt::skip]
impl InstF2fR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn dst_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 8) & 0x3) as u32) }
    pub fn src_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 10) & 0x3) as u32) }
    pub fn round_mode(&self) -> IntegerRound { IntegerRound(((((self.0 >> 40) & 0x4) as i32) | (((self.0 >> 39) & 0x3) as i32)) as u32) }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstF2fI(pub u64);

#[rustfmt::skip]
impl InstF2fI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn dst_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 8) & 0x3) as u32) }
    pub fn src_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 10) & 0x3) as u32) }
    pub fn round_mode(&self) -> IntegerRound { IntegerRound(((((self.0 >> 40) & 0x4) as i32) | (((self.0 >> 39) & 0x3) as i32)) as u32) }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstF2fC(pub u64);

#[rustfmt::skip]
impl InstF2fC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn dst_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 8) & 0x3) as u32) }
    pub fn src_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 10) & 0x3) as u32) }
    pub fn round_mode(&self) -> IntegerRound { IntegerRound(((((self.0 >> 40) & 0x4) as i32) | (((self.0 >> 39) & 0x3) as i32)) as u32) }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstF2iR(pub u64);

#[rustfmt::skip]
impl InstF2iR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn i_dst_fmt(&self) -> IDstFmt { IDstFmt(((((self.0 >> 10) & 0x4) as i32) | (((self.0 >> 8) & 0x3) as i32)) as u32) }
    pub fn src_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 10) & 0x3) as u32) }
    pub fn round_mode(&self) -> RoundMode2 { RoundMode2(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstF2iI(pub u64);

#[rustfmt::skip]
impl InstF2iI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn i_dst_fmt(&self) -> IDstFmt { IDstFmt(((((self.0 >> 10) & 0x4) as i32) | (((self.0 >> 8) & 0x3) as i32)) as u32) }
    pub fn src_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 10) & 0x3) as u32) }
    pub fn round_mode(&self) -> RoundMode2 { RoundMode2(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstF2iC(pub u64);

#[rustfmt::skip]
impl InstF2iC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn i_dst_fmt(&self) -> IDstFmt { IDstFmt(((((self.0 >> 10) & 0x4) as i32) | (((self.0 >> 8) & 0x3) as i32)) as u32) }
    pub fn src_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 10) & 0x3) as u32) }
    pub fn round_mode(&self) -> RoundMode2 { RoundMode2(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFaddR(pub u64);

#[rustfmt::skip]
impl InstFaddR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFaddI(pub u64);

#[rustfmt::skip]
impl InstFaddI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFaddC(pub u64);

#[rustfmt::skip]
impl InstFaddC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFadd32i(pub u64);

#[rustfmt::skip]
impl InstFadd32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn abs_b(&self) -> bool { (self.0 & 0x200000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFchkR(pub u64);

#[rustfmt::skip]
impl InstFchkR {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn chk_mode_f(&self) -> ChkModeF { ChkModeF(((self.0 >> 39) & 0x3F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFchkI(pub u64);

#[rustfmt::skip]
impl InstFchkI {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn chk_mode_f(&self) -> ChkModeF { ChkModeF(((self.0 >> 39) & 0x3F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFchkC(pub u64);

#[rustfmt::skip]
impl InstFchkC {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn chk_mode_f(&self) -> ChkModeF { ChkModeF(((self.0 >> 39) & 0x3F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFcmpR(pub u64);

#[rustfmt::skip]
impl InstFcmpR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFcmpI(pub u64);

#[rustfmt::skip]
impl InstFcmpI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFcmpC(pub u64);

#[rustfmt::skip]
impl InstFcmpC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFcmpRc(pub u64);

#[rustfmt::skip]
impl InstFcmpRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFfmaR(pub u64);

#[rustfmt::skip]
impl InstFfmaR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 53) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFfmaI(pub u64);

#[rustfmt::skip]
impl InstFfmaI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 53) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFfmaC(pub u64);

#[rustfmt::skip]
impl InstFfmaC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 53) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFfmaRc(pub u64);

#[rustfmt::skip]
impl InstFfmaRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 53) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFfma32i(pub u64);

#[rustfmt::skip]
impl InstFfma32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x200000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 53) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFloR(pub u64);

#[rustfmt::skip]
impl InstFloR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFloI(pub u64);

#[rustfmt::skip]
impl InstFloI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFloC(pub u64);

#[rustfmt::skip]
impl InstFloC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn sh(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFmnmxR(pub u64);

#[rustfmt::skip]
impl InstFmnmxR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFmnmxI(pub u64);

#[rustfmt::skip]
impl InstFmnmxI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFmnmxC(pub u64);

#[rustfmt::skip]
impl InstFmnmxC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFmulR(pub u64);

#[rustfmt::skip]
impl InstFmulR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 44) & 0x3) as u32) }
    pub fn scale(&self) -> MultiplyScale { MultiplyScale(((self.0 >> 41) & 0x7) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFmulI(pub u64);

#[rustfmt::skip]
impl InstFmulI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 44) & 0x3) as u32) }
    pub fn scale(&self) -> MultiplyScale { MultiplyScale(((self.0 >> 41) & 0x7) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFmulC(pub u64);

#[rustfmt::skip]
impl InstFmulC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 44) & 0x3) as u32) }
    pub fn scale(&self) -> MultiplyScale { MultiplyScale(((self.0 >> 41) & 0x7) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFmul32i(pub u64);

#[rustfmt::skip]
impl InstFmul32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 53) & 0x3) as u32) }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFsetR(pub u64);

#[rustfmt::skip]
impl InstFsetR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFsetC(pub u64);

#[rustfmt::skip]
impl InstFsetC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFsetI(pub u64);

#[rustfmt::skip]
impl InstFsetI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFsetpR(pub u64);

#[rustfmt::skip]
impl InstFsetpR {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x80) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFsetpI(pub u64);

#[rustfmt::skip]
impl InstFsetpI {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x80) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFsetpC(pub u64);

#[rustfmt::skip]
impl InstFsetpC {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x80) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 48) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstFswzadd(pub u64);

#[rustfmt::skip]
impl InstFswzadd {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn ndv(&self) -> bool { (self.0 & 0x4000000000) != 0 }
    pub fn pn_word(&self) -> i32 { (((self.0 >> 28) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstGetcrsptr(pub u64);

#[rustfmt::skip]
impl InstGetcrsptr {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstGetlmembase(pub u64);

#[rustfmt::skip]
impl InstGetlmembase {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHadd2R(pub u64);

#[rustfmt::skip]
impl InstHadd2R {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn b_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 28) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x40000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHadd2I(pub u64);

#[rustfmt::skip]
impl InstHadd2I {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn bimm_h0(&self) -> i32 { (((self.0 >> 20) & 0x3FF) as i32) }
    pub fn bimm_h1(&self) -> i32 { (((self.0 >> 47) & 0x200) as i32) | (((self.0 >> 30) & 0x1FF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHadd2C(pub u64);

#[rustfmt::skip]
impl InstHadd2C {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHadd232i(pub u64);

#[rustfmt::skip]
impl InstHadd232i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 53) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHfma2R(pub u64);

#[rustfmt::skip]
impl InstHfma2R {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn b_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 28) & 0x3) as u32) }
    pub fn c_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 35) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x40000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 37) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHfma2I(pub u64);

#[rustfmt::skip]
impl InstHfma2I {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn bimm_h0(&self) -> i32 { (((self.0 >> 20) & 0x3FF) as i32) }
    pub fn bimm_h1(&self) -> i32 { (((self.0 >> 47) & 0x200) as i32) | (((self.0 >> 30) & 0x1FF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn c_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 53) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 57) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHfma2C(pub u64);

#[rustfmt::skip]
impl InstHfma2C {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn c_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 53) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 57) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHfma2Rc(pub u64);

#[rustfmt::skip]
impl InstHfma2Rc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn c_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 53) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 57) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHfma232i(pub u64);

#[rustfmt::skip]
impl InstHfma232i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 57) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHmul2R(pub u64);

#[rustfmt::skip]
impl InstHmul2R {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn b_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 28) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x40000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHmul2I(pub u64);

#[rustfmt::skip]
impl InstHmul2I {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn bimm_h0(&self) -> i32 { (((self.0 >> 20) & 0x3FF) as i32) }
    pub fn bimm_h1(&self) -> i32 { (((self.0 >> 47) & 0x200) as i32) | (((self.0 >> 30) & 0x1FF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHmul2C(pub u64);

#[rustfmt::skip]
impl InstHmul2C {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn o_fmt(&self) -> OFmt { OFmt(((self.0 >> 49) & 0x3) as u32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHmul232i(pub u64);

#[rustfmt::skip]
impl InstHmul232i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 53) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn fmz(&self) -> Fmz { Fmz(((self.0 >> 55) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHset2R(pub u64);

#[rustfmt::skip]
impl InstHset2R {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn b_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 28) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x40000000) != 0 }
    pub fn bval(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn cmp(&self) -> FComp { FComp(((self.0 >> 35) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHset2I(pub u64);

#[rustfmt::skip]
impl InstHset2I {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn bimm_h0(&self) -> i32 { (((self.0 >> 20) & 0x3FF) as i32) }
    pub fn bimm_h1(&self) -> i32 { (((self.0 >> 47) & 0x200) as i32) | (((self.0 >> 30) & 0x1FF) as i32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn bval(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn cmp(&self) -> FComp { FComp(((self.0 >> 49) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHset2C(pub u64);

#[rustfmt::skip]
impl InstHset2C {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn bval(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn cmp(&self) -> FComp { FComp(((self.0 >> 49) & 0xF) as u32) }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn ftz(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHsetp2R(pub u64);

#[rustfmt::skip]
impl InstHsetp2R {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x40000000) != 0 }
    pub fn f_comp2(&self) -> FComp { FComp(((self.0 >> 35) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn h_and(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
    pub fn b_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 28) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHsetp2I(pub u64);

#[rustfmt::skip]
impl InstHsetp2I {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn bimm_h0(&self) -> i32 { (((self.0 >> 20) & 0x3FF) as i32) }
    pub fn bimm_h1(&self) -> i32 { (((self.0 >> 47) & 0x200) as i32) | (((self.0 >> 30) & 0x1FF) as i32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 49) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn h_and(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstHsetp2C(pub u64);

#[rustfmt::skip]
impl InstHsetp2C {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn abs_a(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn f_comp(&self) -> FComp { FComp(((self.0 >> 49) & 0xF) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn ftz(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn h_and(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn a_swizzle(&self) -> HalfSwizzle { HalfSwizzle(((self.0 >> 47) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstI2fR(pub u64);

#[rustfmt::skip]
impl InstI2fR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn i_src_fmt(&self) -> ISrcFmt { ISrcFmt(((((self.0 >> 11) & 0x4) as i32) | (((self.0 >> 10) & 0x3) as i32)) as u32) }
    pub fn dst_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 8) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstI2fI(pub u64);

#[rustfmt::skip]
impl InstI2fI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn i_src_fmt(&self) -> ISrcFmt { ISrcFmt(((((self.0 >> 11) & 0x4) as i32) | (((self.0 >> 10) & 0x3) as i32)) as u32) }
    pub fn dst_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 8) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstI2fC(pub u64);

#[rustfmt::skip]
impl InstI2fC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn round_mode(&self) -> RoundMode { RoundMode(((self.0 >> 39) & 0x3) as u32) }
    pub fn i_src_fmt(&self) -> ISrcFmt { ISrcFmt(((((self.0 >> 11) & 0x4) as i32) | (((self.0 >> 10) & 0x3) as i32)) as u32) }
    pub fn dst_fmt(&self) -> DstFmt { DstFmt(((self.0 >> 8) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstI2iR(pub u64);

#[rustfmt::skip]
impl InstI2iR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn i_dst_fmt(&self) -> ISrcDstFmt { ISrcDstFmt(((((self.0 >> 10) & 0x4) as i32) | (((self.0 >> 8) & 0x3) as i32)) as u32) }
    pub fn i_src_fmt(&self) -> ISrcDstFmt { ISrcDstFmt(((((self.0 >> 11) & 0x4) as i32) | (((self.0 >> 10) & 0x3) as i32)) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstI2iI(pub u64);

#[rustfmt::skip]
impl InstI2iI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn i_dst_fmt(&self) -> ISrcDstFmt { ISrcDstFmt(((((self.0 >> 10) & 0x4) as i32) | (((self.0 >> 8) & 0x3) as i32)) as u32) }
    pub fn i_src_fmt(&self) -> ISrcDstFmt { ISrcDstFmt(((((self.0 >> 11) & 0x4) as i32) | (((self.0 >> 10) & 0x3) as i32)) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstI2iC(pub u64);

#[rustfmt::skip]
impl InstI2iC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn i_dst_fmt(&self) -> ISrcDstFmt { ISrcDstFmt(((((self.0 >> 10) & 0x4) as i32) | (((self.0 >> 8) & 0x3) as i32)) as u32) }
    pub fn i_src_fmt(&self) -> ISrcDstFmt { ISrcDstFmt(((((self.0 >> 11) & 0x4) as i32) | (((self.0 >> 10) & 0x3) as i32)) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIaddR(pub u64);

#[rustfmt::skip]
impl InstIaddR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 48) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIaddI(pub u64);

#[rustfmt::skip]
impl InstIaddI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 48) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIaddC(pub u64);

#[rustfmt::skip]
impl InstIaddC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 48) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIadd32i(pub u64);

#[rustfmt::skip]
impl InstIadd32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 55) & 0x3) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIadd3R(pub u64);

#[rustfmt::skip]
impl InstIadd3R {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn lrs(&self) -> Lrs { Lrs(((self.0 >> 37) & 0x3) as u32) }
    pub fn apart(&self) -> HalfSelect { HalfSelect(((self.0 >> 35) & 0x3) as u32) }
    pub fn bpart(&self) -> HalfSelect { HalfSelect(((self.0 >> 33) & 0x3) as u32) }
    pub fn cpart(&self) -> HalfSelect { HalfSelect(((self.0 >> 31) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIadd3I(pub u64);

#[rustfmt::skip]
impl InstIadd3I {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIadd3C(pub u64);

#[rustfmt::skip]
impl InstIadd3C {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn neg_c(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIcmpR(pub u64);

#[rustfmt::skip]
impl InstIcmpR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIcmpI(pub u64);

#[rustfmt::skip]
impl InstIcmpI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIcmpC(pub u64);

#[rustfmt::skip]
impl InstIcmpC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIcmpRc(pub u64);

#[rustfmt::skip]
impl InstIcmpRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIde(pub u64);

#[rustfmt::skip]
impl InstIde {
    pub fn imm16(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
    pub fn di(&self) -> bool { (self.0 & 0x20) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIdpR(pub u64);

#[rustfmt::skip]
impl InstIdpR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn is_hi(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn src_a_sign(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn is_dp(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn src_b_sign(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIdpC(pub u64);

#[rustfmt::skip]
impl InstIdpC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn is_hi(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn src_a_sign(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn is_dp(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn src_b_sign(&self) -> bool { (self.0 & 0x800000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadR(pub u64);

#[rustfmt::skip]
impl InstImadR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadI(pub u64);

#[rustfmt::skip]
impl InstImadI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadC(pub u64);

#[rustfmt::skip]
impl InstImadC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadRc(pub u64);

#[rustfmt::skip]
impl InstImadRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 51) & 0x3) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImad32i(pub u64);

#[rustfmt::skip]
impl InstImad32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn b_signed(&self) -> bool { (self.0 & 0x200000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 55) & 0x3) as u32) }
    pub fn a_signed(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadspR(pub u64);

#[rustfmt::skip]
impl InstImadspR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn a_select(&self) -> ImadspASelect { ImadspASelect(((self.0 >> 48) & 0x7) as u32) }
    pub fn b_select(&self) -> ImadspBSelect { ImadspBSelect(((self.0 >> 53) & 0x3) as u32) }
    pub fn c_select(&self) -> ImadspASelect { ImadspASelect(((((self.0 >> 50) & 0x6) as i32) | (((self.0 >> 48) & 0x1) as i32)) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadspI(pub u64);

#[rustfmt::skip]
impl InstImadspI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn a_select(&self) -> ImadspASelect { ImadspASelect(((self.0 >> 48) & 0x7) as u32) }
    pub fn b_select(&self) -> ImadspBSelect { ImadspBSelect(((self.0 >> 53) & 0x3) as u32) }
    pub fn c_select(&self) -> ImadspASelect { ImadspASelect(((((self.0 >> 50) & 0x6) as i32) | (((self.0 >> 48) & 0x1) as i32)) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadspC(pub u64);

#[rustfmt::skip]
impl InstImadspC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn a_select(&self) -> ImadspASelect { ImadspASelect(((self.0 >> 48) & 0x7) as u32) }
    pub fn b_select(&self) -> ImadspBSelect { ImadspBSelect(((self.0 >> 53) & 0x3) as u32) }
    pub fn c_select(&self) -> ImadspASelect { ImadspASelect(((((self.0 >> 50) & 0x6) as i32) | (((self.0 >> 48) & 0x1) as i32)) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImadspRc(pub u64);

#[rustfmt::skip]
impl InstImadspRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn a_select(&self) -> ImadspASelect { ImadspASelect(((self.0 >> 48) & 0x7) as u32) }
    pub fn b_select(&self) -> ImadspBSelect { ImadspBSelect(((self.0 >> 53) & 0x3) as u32) }
    pub fn c_select(&self) -> ImadspASelect { ImadspASelect(((((self.0 >> 50) & 0x6) as i32) | (((self.0 >> 48) & 0x1) as i32)) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImnmxR(pub u64);

#[rustfmt::skip]
impl InstImnmxR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x_mode(&self) -> XMode { XMode(((self.0 >> 43) & 0x3) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImnmxI(pub u64);

#[rustfmt::skip]
impl InstImnmxI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x_mode(&self) -> XMode { XMode(((self.0 >> 43) & 0x3) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImnmxC(pub u64);

#[rustfmt::skip]
impl InstImnmxC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x_mode(&self) -> XMode { XMode(((self.0 >> 43) & 0x3) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImulR(pub u64);

#[rustfmt::skip]
impl InstImulR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImulI(pub u64);

#[rustfmt::skip]
impl InstImulI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImulC(pub u64);

#[rustfmt::skip]
impl InstImulC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x20000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstImul32i(pub u64);

#[rustfmt::skip]
impl InstImul32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn a_signed(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn b_signed(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn hilo(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIpa(pub u64);

#[rustfmt::skip]
impl InstIpa {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ipa_op(&self) -> IpaOp { IpaOp(((self.0 >> 54) & 0x3) as u32) }
    pub fn msi(&self) -> i32 { (((self.0 >> 52) & 0x3) as i32) }
    pub fn sat(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn idx(&self) -> bool { (self.0 & 0x4000000000) != 0 }
    pub fn imm10(&self) -> i32 { (((self.0 >> 28) & 0x3FF) as i32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 47) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIsberd(pub u64);

#[rustfmt::skip]
impl InstIsberd {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn al_size(&self) -> AlSize { AlSize(((self.0 >> 47) & 0x3) as u32) }
    pub fn i_base(&self) -> IBase { IBase(((self.0 >> 33) & 0x3) as u32) }
    pub fn o(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn p(&self) -> bool { (self.0 & 0x80000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIscaddR(pub u64);

#[rustfmt::skip]
impl InstIscaddR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn imm5(&self) -> i32 { (((self.0 >> 39) & 0x1F) as i32) }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 48) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIscaddI(pub u64);

#[rustfmt::skip]
impl InstIscaddI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn imm5(&self) -> i32 { (((self.0 >> 39) & 0x1F) as i32) }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 48) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIscaddC(pub u64);

#[rustfmt::skip]
impl InstIscaddC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn imm5(&self) -> i32 { (((self.0 >> 39) & 0x1F) as i32) }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 48) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIscadd32i(pub u64);

#[rustfmt::skip]
impl InstIscadd32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn imm5(&self) -> i32 { (((self.0 >> 53) & 0x1F) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIsetR(pub u64);

#[rustfmt::skip]
impl InstIsetR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIsetI(pub u64);

#[rustfmt::skip]
impl InstIsetI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIsetC(pub u64);

#[rustfmt::skip]
impl InstIsetC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn b_val(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIsetpR(pub u64);

#[rustfmt::skip]
impl InstIsetpR {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIsetpI(pub u64);

#[rustfmt::skip]
impl InstIsetpI {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstIsetpC(pub u64);

#[rustfmt::skip]
impl InstIsetpC {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn i_comp(&self) -> IComp { IComp(((self.0 >> 49) & 0x7) as u32) }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn bop(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstJcal(pub u64);

#[rustfmt::skip]
impl InstJcal {
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
    pub fn inc(&self) -> bool { (self.0 & 0x40) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstJmp(pub u64);

#[rustfmt::skip]
impl InstJmp {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn lmt(&self) -> bool { (self.0 & 0x40) != 0 }
    pub fn u(&self) -> bool { (self.0 & 0x80) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstJmx(pub u64);

#[rustfmt::skip]
impl InstJmx {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn lmt(&self) -> bool { (self.0 & 0x40) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstKil(pub u64);

#[rustfmt::skip]
impl InstKil {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLd(pub u64);

#[rustfmt::skip]
impl InstLd {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 58) & 0x7) as i32) }
    pub fn cache_op(&self) -> CacheOpLd { CacheOpLd(((self.0 >> 56) & 0x3) as u32) }
    pub fn ls_size(&self) -> LsSize { LsSize(((self.0 >> 53) & 0x7) as u32) }
    pub fn e(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLdc(pub u64);

#[rustfmt::skip]
impl InstLdc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ls_size(&self) -> LsSize2 { LsSize2(((self.0 >> 48) & 0x7) as u32) }
    pub fn address_mode(&self) -> AddressMode { AddressMode(((self.0 >> 44) & 0x3) as u32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 36) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLdg(pub u64);

#[rustfmt::skip]
impl InstLdg {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ls_size(&self) -> LsSize { LsSize(((self.0 >> 48) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOpLd { CacheOpLd(((self.0 >> 46) & 0x3) as u32) }
    pub fn e(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLdl(pub u64);

#[rustfmt::skip]
impl InstLdl {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ls_size(&self) -> LsSize2 { LsSize2(((self.0 >> 48) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOp2 { CacheOp2(((self.0 >> 44) & 0x3) as u32) }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLds(pub u64);

#[rustfmt::skip]
impl InstLds {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ls_size(&self) -> LsSize2 { LsSize2(((self.0 >> 48) & 0x7) as u32) }
    pub fn u(&self) -> bool { (self.0 & 0x100000000000) != 0 }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLeaR(pub u64);

#[rustfmt::skip]
impl InstLeaR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn imm_u5(&self) -> i32 { (((self.0 >> 39) & 0x1F) as i32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLeaI(pub u64);

#[rustfmt::skip]
impl InstLeaI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn imm_u5(&self) -> i32 { (((self.0 >> 39) & 0x1F) as i32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLeaC(pub u64);

#[rustfmt::skip]
impl InstLeaC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn imm_u5(&self) -> i32 { (((self.0 >> 39) & 0x1F) as i32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLeaHiR(pub u64);

#[rustfmt::skip]
impl InstLeaHiR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x4000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x2000000000) != 0 }
    pub fn imm_u5(&self) -> i32 { (((self.0 >> 28) & 0x1F) as i32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLeaHiC(pub u64);

#[rustfmt::skip]
impl InstLeaHiC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x200000000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn imm_u5(&self) -> i32 { (((self.0 >> 51) & 0x1F) as i32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLepc(pub u64);

#[rustfmt::skip]
impl InstLepc {
}

#[derive(Clone, Copy, Debug)]
pub struct InstLongjmp(pub u64);

#[rustfmt::skip]
impl InstLongjmp {
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLopR(pub u64);

#[rustfmt::skip]
impl InstLopR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
    pub fn predicate_op(&self) -> PredicateOp { PredicateOp(((self.0 >> 44) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn lop(&self) -> LogicOp { LogicOp(((self.0 >> 41) & 0x3) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x8000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLopI(pub u64);

#[rustfmt::skip]
impl InstLopI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
    pub fn predicate_op(&self) -> PredicateOp { PredicateOp(((self.0 >> 44) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn logic_op(&self) -> LogicOp { LogicOp(((self.0 >> 41) & 0x3) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x8000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLopC(pub u64);

#[rustfmt::skip]
impl InstLopC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
    pub fn predicate_op(&self) -> PredicateOp { PredicateOp(((self.0 >> 44) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn logic_op(&self) -> LogicOp { LogicOp(((self.0 >> 41) & 0x3) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x8000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLop3R(pub u64);

#[rustfmt::skip]
impl InstLop3R {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
    pub fn predicate_op(&self) -> PredicateOp { PredicateOp(((self.0 >> 36) & 0x3) as u32) }
    pub fn x(&self) -> bool { (self.0 & 0x4000000000) != 0 }
    pub fn imm(&self) -> i32 { (((self.0 >> 28) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLop3I(pub u64);

#[rustfmt::skip]
impl InstLop3I {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x200000000000000) != 0 }
    pub fn imm(&self) -> i32 { (((self.0 >> 48) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLop3C(pub u64);

#[rustfmt::skip]
impl InstLop3C {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn imm(&self) -> i32 { (((self.0 >> 48) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstLop32i(pub u64);

#[rustfmt::skip]
impl InstLop32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn x(&self) -> bool { (self.0 & 0x200000000000000) != 0 }
    pub fn logic_op(&self) -> LogicOp { LogicOp(((self.0 >> 53) & 0x3) as u32) }
    pub fn neg_a(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstMembar(pub u64);

#[rustfmt::skip]
impl InstMembar {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn membar(&self) -> Membar { Membar(((self.0 >> 8) & 0x3) as u32) }
    pub fn ivall(&self) -> Ivall { Ivall((self.0 & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstMovR(pub u64);

#[rustfmt::skip]
impl InstMovR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn quad_mask(&self) -> i32 { (((self.0 >> 39) & 0xF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstMovI(pub u64);

#[rustfmt::skip]
impl InstMovI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn quad_mask(&self) -> i32 { (((self.0 >> 39) & 0xF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstMovC(pub u64);

#[rustfmt::skip]
impl InstMovC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn quad_mask(&self) -> i32 { (((self.0 >> 39) & 0xF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstMov32i(pub u64);

#[rustfmt::skip]
impl InstMov32i {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn quad_mask(&self) -> i32 { (((self.0 >> 12) & 0xF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstMufu(pub u64);

#[rustfmt::skip]
impl InstMufu {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn mufu_op(&self) -> MufuOp { MufuOp(((self.0 >> 20) & 0xF) as u32) }
    pub fn abs_a(&self) -> bool { (self.0 & 0x400000000000) != 0 }
    pub fn neg_a(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstNop(pub u64);

#[rustfmt::skip]
impl InstNop {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm16(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
    pub fn trig(&self) -> bool { (self.0 & 0x2000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc(((self.0 >> 8) & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstOutR(pub u64);

#[rustfmt::skip]
impl InstOutR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn out_type(&self) -> OutType { OutType(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstOutI(pub u64);

#[rustfmt::skip]
impl InstOutI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn out_type(&self) -> OutType { OutType(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstOutC(pub u64);

#[rustfmt::skip]
impl InstOutC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn out_type(&self) -> OutType { OutType(((self.0 >> 39) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstP2rR(pub u64);

#[rustfmt::skip]
impl InstP2rR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn ccpr(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstP2rI(pub u64);

#[rustfmt::skip]
impl InstP2rI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn ccpr(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstP2rC(pub u64);

#[rustfmt::skip]
impl InstP2rC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn ccpr(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPbk(pub u64);

#[rustfmt::skip]
impl InstPbk {
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPcnt(pub u64);

#[rustfmt::skip]
impl InstPcnt {
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPexit(pub u64);

#[rustfmt::skip]
impl InstPexit {
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPixld(pub u64);

#[rustfmt::skip]
impl InstPixld {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 45) & 0x7) as i32) }
    pub fn pix_mode(&self) -> PixMode { PixMode(((self.0 >> 31) & 0x7) as u32) }
    pub fn imm8(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPlongjmp(pub u64);

#[rustfmt::skip]
impl InstPlongjmp {
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPopcR(pub u64);

#[rustfmt::skip]
impl InstPopcR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPopcI(pub u64);

#[rustfmt::skip]
impl InstPopcI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPopcC(pub u64);

#[rustfmt::skip]
impl InstPopcC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPret(pub u64);

#[rustfmt::skip]
impl InstPret {
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn inc(&self) -> bool { (self.0 & 0x40) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPrmtR(pub u64);

#[rustfmt::skip]
impl InstPrmtR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn p_mode(&self) -> PMode { PMode(((self.0 >> 48) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPrmtI(pub u64);

#[rustfmt::skip]
impl InstPrmtI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn p_mode(&self) -> PMode { PMode(((self.0 >> 48) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPrmtC(pub u64);

#[rustfmt::skip]
impl InstPrmtC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn p_mode(&self) -> PMode { PMode(((self.0 >> 48) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPrmtRc(pub u64);

#[rustfmt::skip]
impl InstPrmtRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn p_mode(&self) -> PMode { PMode(((self.0 >> 48) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPset(pub u64);

#[rustfmt::skip]
impl InstPset {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn src2_pred(&self) -> i32 { (((self.0 >> 12) & 0x7) as i32) }
    pub fn src2_pred_inv(&self) -> bool { (self.0 & 0x8000) != 0 }
    pub fn src1_pred(&self) -> i32 { (((self.0 >> 29) & 0x7) as i32) }
    pub fn src1_pred_inv(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bool_op_ab(&self) -> BoolOp { BoolOp(((self.0 >> 24) & 0x3) as u32) }
    pub fn bool_op_c(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn b_val(&self) -> bool { (self.0 & 0x100000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstPsetp(pub u64);

#[rustfmt::skip]
impl InstPsetp {
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn src2_pred(&self) -> i32 { (((self.0 >> 12) & 0x7) as i32) }
    pub fn src2_pred_inv(&self) -> bool { (self.0 & 0x8000) != 0 }
    pub fn src1_pred(&self) -> i32 { (((self.0 >> 29) & 0x7) as i32) }
    pub fn src1_pred_inv(&self) -> bool { (self.0 & 0x100000000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn bool_op_ab(&self) -> BoolOp { BoolOp(((self.0 >> 24) & 0x3) as u32) }
    pub fn bool_op_c(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstR2b(pub u64);

#[rustfmt::skip]
impl InstR2b {
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn mode(&self) -> BarMode { BarMode(((self.0 >> 32) & 0x3) as u32) }
    pub fn name(&self) -> i32 { (((self.0 >> 28) & 0xF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstR2pR(pub u64);

#[rustfmt::skip]
impl InstR2pR {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn ccpr(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstR2pI(pub u64);

#[rustfmt::skip]
impl InstR2pI {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn ccpr(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstR2pC(pub u64);

#[rustfmt::skip]
impl InstR2pC {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn byte_sel(&self) -> ByteSel { ByteSel(((self.0 >> 41) & 0x3) as u32) }
    pub fn ccpr(&self) -> bool { (self.0 & 0x10000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstRam(pub u64);

#[rustfmt::skip]
impl InstRam {
}

#[derive(Clone, Copy, Debug)]
pub struct InstRed(pub u64);

#[rustfmt::skip]
impl InstRed {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm20(&self) -> i32 { (((self.0 >> 28) & 0xFFFFF) as i32) }
    pub fn red_size(&self) -> AtomSize { AtomSize(((self.0 >> 20) & 0x7) as u32) }
    pub fn red_op(&self) -> RedOp { RedOp(((self.0 >> 23) & 0x7) as u32) }
    pub fn e(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstRet(pub u64);

#[rustfmt::skip]
impl InstRet {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstRroR(pub u64);

#[rustfmt::skip]
impl InstRroR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn rro_op(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstRroI(pub u64);

#[rustfmt::skip]
impl InstRroI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn rro_op(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstRroC(pub u64);

#[rustfmt::skip]
impl InstRroC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn abs_b(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn neg_b(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn rro_op(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstRtt(pub u64);

#[rustfmt::skip]
impl InstRtt {
}

#[derive(Clone, Copy, Debug)]
pub struct InstS2r(pub u64);

#[rustfmt::skip]
impl InstS2r {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn s_reg(&self) -> SReg { SReg(((self.0 >> 20) & 0xFF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSam(pub u64);

#[rustfmt::skip]
impl InstSam {
}

#[derive(Clone, Copy, Debug)]
pub struct InstSelR(pub u64);

#[rustfmt::skip]
impl InstSelR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSelI(pub u64);

#[rustfmt::skip]
impl InstSelI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSelC(pub u64);

#[rustfmt::skip]
impl InstSelC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSetcrsptr(pub u64);

#[rustfmt::skip]
impl InstSetcrsptr {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSetlmembase(pub u64);

#[rustfmt::skip]
impl InstSetlmembase {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShfLR(pub u64);

#[rustfmt::skip]
impl InstShfLR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x_mode_shf(&self) -> XModeShf { XModeShf(((self.0 >> 48) & 0x3) as u32) }
    pub fn max_shift(&self) -> MaxShift { MaxShift(((self.0 >> 37) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShfRR(pub u64);

#[rustfmt::skip]
impl InstShfRR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x_mode_shf(&self) -> XModeShf { XModeShf(((self.0 >> 48) & 0x3) as u32) }
    pub fn max_shift(&self) -> MaxShift { MaxShift(((self.0 >> 37) & 0x3) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShfLI(pub u64);

#[rustfmt::skip]
impl InstShfLI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x_mode_shf(&self) -> XModeShf { XModeShf(((self.0 >> 48) & 0x3) as u32) }
    pub fn max_shift(&self) -> MaxShift { MaxShift(((self.0 >> 37) & 0x3) as u32) }
    pub fn imm6(&self) -> i32 { (((self.0 >> 20) & 0x3F) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShfRI(pub u64);

#[rustfmt::skip]
impl InstShfRI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn x_mode_shf(&self) -> XModeShf { XModeShf(((self.0 >> 48) & 0x3) as u32) }
    pub fn max_shift(&self) -> MaxShift { MaxShift(((self.0 >> 37) & 0x3) as u32) }
    pub fn imm6(&self) -> i32 { (((self.0 >> 20) & 0x3F) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShfl(pub u64);

#[rustfmt::skip]
impl InstShfl {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn src_b_imm(&self) -> i32 { (((self.0 >> 20) & 0x1F) as i32) }
    pub fn src_c_imm(&self) -> i32 { (((self.0 >> 34) & 0x1FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn shfl_mode(&self) -> ShflMode { ShflMode(((self.0 >> 30) & 0x3) as u32) }
    pub fn c_fix_shfl(&self) -> bool { (self.0 & 0x20000000) != 0 }
    pub fn b_fix_shfl(&self) -> bool { (self.0 & 0x10000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 48) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShlR(pub u64);

#[rustfmt::skip]
impl InstShlR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShlI(pub u64);

#[rustfmt::skip]
impl InstShlI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShlC(pub u64);

#[rustfmt::skip]
impl InstShlC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x80000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShrR(pub u64);

#[rustfmt::skip]
impl InstShrR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x_mode(&self) -> XMode { XMode(((self.0 >> 43) & 0x3) as u32) }
    pub fn brev(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShrI(pub u64);

#[rustfmt::skip]
impl InstShrI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn imm20(&self) -> i32 { (((self.0 >> 37) & 0x80000) as i32) | (((self.0 >> 20) & 0x7FFFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x_mode(&self) -> XMode { XMode(((self.0 >> 43) & 0x3) as u32) }
    pub fn brev(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstShrC(pub u64);

#[rustfmt::skip]
impl InstShrC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x_mode(&self) -> XMode { XMode(((self.0 >> 43) & 0x3) as u32) }
    pub fn brev(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn m(&self) -> bool { (self.0 & 0x8000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSsy(pub u64);

#[rustfmt::skip]
impl InstSsy {
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
    pub fn ca(&self) -> bool { (self.0 & 0x20) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSt(pub u64);

#[rustfmt::skip]
impl InstSt {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 58) & 0x7) as i32) }
    pub fn cache_op(&self) -> CacheOpSt { CacheOpSt(((self.0 >> 56) & 0x3) as u32) }
    pub fn ls_size(&self) -> LsSize { LsSize(((self.0 >> 53) & 0x7) as u32) }
    pub fn e(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn imm32(&self) -> i32 { ((self.0 >> 20) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstStg(pub u64);

#[rustfmt::skip]
impl InstStg {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ls_size(&self) -> LsSize2 { LsSize2(((self.0 >> 48) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOpSt { CacheOpSt(((self.0 >> 46) & 0x3) as u32) }
    pub fn e(&self) -> bool { (self.0 & 0x200000000000) != 0 }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstStl(pub u64);

#[rustfmt::skip]
impl InstStl {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ls_size(&self) -> LsSize2 { LsSize2(((self.0 >> 48) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOpSt { CacheOpSt(((self.0 >> 44) & 0x3) as u32) }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstStp(pub u64);

#[rustfmt::skip]
impl InstStp {
    pub fn wait(&self) -> bool { (self.0 & 0x80000000) != 0 }
    pub fn imm8(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSts(pub u64);

#[rustfmt::skip]
impl InstSts {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ls_size(&self) -> LsSize2 { LsSize2(((self.0 >> 48) & 0x7) as u32) }
    pub fn imm24(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuatomB(pub u64);

#[rustfmt::skip]
impl InstSuatomB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn size(&self) -> SuatomSize { SuatomSize(((self.0 >> 36) & 0x7) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn op(&self) -> SuatomOp { SuatomOp(((self.0 >> 29) & 0xF) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x10000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuatom(pub u64);

#[rustfmt::skip]
impl InstSuatom {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn size(&self) -> SuatomSize { SuatomSize(((self.0 >> 51) & 0x7) as u32) }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn op(&self) -> SuatomOp { SuatomOp(((self.0 >> 29) & 0xF) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x10000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuatomB2(pub u64);

#[rustfmt::skip]
impl InstSuatomB2 {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn size(&self) -> SuatomSize { SuatomSize(((self.0 >> 36) & 0x7) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn op(&self) -> SuatomOp { SuatomOp(((self.0 >> 29) & 0xF) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x10000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuatomCasB(pub u64);

#[rustfmt::skip]
impl InstSuatomCasB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn size(&self) -> SuatomSize { SuatomSize(((self.0 >> 36) & 0x7) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 30) & 0x7) as i32) }
    pub fn ba(&self) -> bool { (self.0 & 0x10000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuatomCas(pub u64);

#[rustfmt::skip]
impl InstSuatomCas {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn size(&self) -> SuatomSize { SuatomSize(((self.0 >> 51) & 0x7) as u32) }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 30) & 0x7) as i32) }
    pub fn ba(&self) -> bool { (self.0 & 0x10000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuldDB(pub u64);

#[rustfmt::skip]
impl InstSuldDB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn dest_pred2(&self) -> i32 { (((self.0 >> 30) & 0x7) as i32) }
    pub fn cache_op(&self) -> CacheOpLd { CacheOpLd(((self.0 >> 24) & 0x3) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x800000) != 0 }
    pub fn size(&self) -> SuSize { SuSize(((self.0 >> 20) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuldD(pub u64);

#[rustfmt::skip]
impl InstSuldD {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn dest_pred2(&self) -> i32 { (((self.0 >> 30) & 0x7) as i32) }
    pub fn cache_op(&self) -> CacheOpLd { CacheOpLd(((self.0 >> 24) & 0x3) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x800000) != 0 }
    pub fn size(&self) -> SuSize { SuSize(((self.0 >> 20) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuldB(pub u64);

#[rustfmt::skip]
impl InstSuldB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn dest_pred2(&self) -> i32 { (((self.0 >> 30) & 0x7) as i32) }
    pub fn cache_op(&self) -> CacheOpLd { CacheOpLd(((self.0 >> 24) & 0x3) as u32) }
    pub fn rgba(&self) -> SuRgba { SuRgba(((self.0 >> 20) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuld(pub u64);

#[rustfmt::skip]
impl InstSuld {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn dest_pred2(&self) -> i32 { (((self.0 >> 30) & 0x7) as i32) }
    pub fn cache_op(&self) -> CacheOpLd { CacheOpLd(((self.0 >> 24) & 0x3) as u32) }
    pub fn rgba(&self) -> SuRgba { SuRgba(((self.0 >> 20) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSuredB(pub u64);

#[rustfmt::skip]
impl InstSuredB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn op(&self) -> RedOp { RedOp(((self.0 >> 24) & 0x7) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x800000) != 0 }
    pub fn size(&self) -> SuatomSize { SuatomSize(((self.0 >> 20) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSured(pub u64);

#[rustfmt::skip]
impl InstSured {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn op(&self) -> RedOp { RedOp(((self.0 >> 24) & 0x7) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x800000) != 0 }
    pub fn size(&self) -> SuatomSize { SuatomSize(((self.0 >> 20) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSustDB(pub u64);

#[rustfmt::skip]
impl InstSustDB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOpSt { CacheOpSt(((self.0 >> 24) & 0x3) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x800000) != 0 }
    pub fn size(&self) -> SuSize { SuSize(((self.0 >> 20) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSustD(pub u64);

#[rustfmt::skip]
impl InstSustD {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOpSt { CacheOpSt(((self.0 >> 24) & 0x3) as u32) }
    pub fn ba(&self) -> bool { (self.0 & 0x800000) != 0 }
    pub fn size(&self) -> SuSize { SuSize(((self.0 >> 20) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSustB(pub u64);

#[rustfmt::skip]
impl InstSustB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOpSt { CacheOpSt(((self.0 >> 24) & 0x3) as u32) }
    pub fn rgba(&self) -> SuRgba { SuRgba(((self.0 >> 20) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSust(pub u64);

#[rustfmt::skip]
impl InstSust {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn clamp(&self) -> Clamp { Clamp(((self.0 >> 49) & 0x3) as u32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn dim(&self) -> SuDim { SuDim(((self.0 >> 33) & 0x7) as u32) }
    pub fn cache_op(&self) -> CacheOpSt { CacheOpSt(((self.0 >> 24) & 0x3) as u32) }
    pub fn rgba(&self) -> SuRgba { SuRgba(((self.0 >> 20) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstSync(pub u64);

#[rustfmt::skip]
impl InstSync {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn ccc(&self) -> Ccc { Ccc((self.0 & 0x1F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTex(pub u64);

#[rustfmt::skip]
impl InstTex {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn lc(&self) -> bool { (self.0 & 0x400000000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn lod(&self) -> Lod { Lod(((self.0 >> 55) & 0x7) as u32) }
    pub fn aoffi(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn dc(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn ndv(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTexB(pub u64);

#[rustfmt::skip]
impl InstTexB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn lcb(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn lodb(&self) -> Lod { Lod(((self.0 >> 37) & 0x7) as u32) }
    pub fn aoffib(&self) -> bool { (self.0 & 0x1000000000) != 0 }
    pub fn dc(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn ndv(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTexs(pub u64);

#[rustfmt::skip]
impl InstTexs {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn target(&self) -> TexsTarget { TexsTarget(((self.0 >> 53) & 0xF) as u32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 50) & 0x7) as i32) }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn dest2(&self) -> i32 { (((self.0 >> 28) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTld(pub u64);

#[rustfmt::skip]
impl InstTld {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn lod(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn toff(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn ms(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn cl(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTldB(pub u64);

#[rustfmt::skip]
impl InstTldB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn lod(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn toff(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn ms(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn cl(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTlds(pub u64);

#[rustfmt::skip]
impl InstTlds {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn target(&self) -> TldsTarget { TldsTarget(((self.0 >> 53) & 0xF) as u32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 50) & 0x7) as i32) }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn dest2(&self) -> i32 { (((self.0 >> 28) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTld4(pub u64);

#[rustfmt::skip]
impl InstTld4 {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn lc(&self) -> bool { (self.0 & 0x400000000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn tex_comp(&self) -> TexComp { TexComp(((self.0 >> 56) & 0x3) as u32) }
    pub fn toff(&self) -> TexOffset { TexOffset(((self.0 >> 54) & 0x3) as u32) }
    pub fn dc(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn ndv(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTld4B(pub u64);

#[rustfmt::skip]
impl InstTld4B {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn lc(&self) -> bool { (self.0 & 0x10000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn tex_comp(&self) -> TexComp { TexComp(((self.0 >> 38) & 0x3) as u32) }
    pub fn toff(&self) -> TexOffset { TexOffset(((self.0 >> 36) & 0x3) as u32) }
    pub fn dc(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn ndv(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTld4s(pub u64);

#[rustfmt::skip]
impl InstTld4s {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn tex_comp(&self) -> TexComp { TexComp(((self.0 >> 52) & 0x3) as u32) }
    pub fn aoffi(&self) -> bool { (self.0 & 0x8000000000000) != 0 }
    pub fn dc(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn dest2(&self) -> i32 { (((self.0 >> 28) & 0xFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTmml(pub u64);

#[rustfmt::skip]
impl InstTmml {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn ndv(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTmmlB(pub u64);

#[rustfmt::skip]
impl InstTmmlB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn ndv(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTxa(pub u64);

#[rustfmt::skip]
impl InstTxa {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn ndv(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTxd(pub u64);

#[rustfmt::skip]
impl InstTxd {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn lc(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn toff(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTxdB(pub u64);

#[rustfmt::skip]
impl InstTxdB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 51) & 0x7) as i32) }
    pub fn lc(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn toff(&self) -> bool { (self.0 & 0x800000000) != 0 }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn dim(&self) -> TexDim { TexDim(((self.0 >> 28) & 0x7) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTxq(pub u64);

#[rustfmt::skip]
impl InstTxq {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn tid_b(&self) -> i32 { (((self.0 >> 36) & 0x1FFF) as i32) }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn tex_query(&self) -> TexQuery { TexQuery(((self.0 >> 22) & 0x3F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstTxqB(pub u64);

#[rustfmt::skip]
impl InstTxqB {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn nodep(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn w_mask(&self) -> i32 { (((self.0 >> 31) & 0xF) as i32) }
    pub fn tex_query(&self) -> TexQuery { TexQuery(((self.0 >> 22) & 0x3F) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVabsdiff(pub u64);

#[rustfmt::skip]
impl InstVabsdiff {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn d_format(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 46) & 0x8) as i32) | (((self.0 >> 28) & 0x7) as i32)) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn video_op(&self) -> VideoOp { VideoOp(((self.0 >> 51) & 0x7) as u32) }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVabsdiff4(pub u64);

#[rustfmt::skip]
impl InstVabsdiff4 {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn v_red(&self) -> VideoRed { VideoRed(((self.0 >> 53) & 0x3) as u32) }
    pub fn lane_mask4(&self) -> LaneMask4 { LaneMask4(((((self.0 >> 49) & 0xC) as i32) | (((self.0 >> 36) & 0x3) as i32)) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
    pub fn src_b_fmt(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn src_a_fmt(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn d_format(&self) -> bool { (self.0 & 0x4000000000) != 0 }
    pub fn asel4(&self) -> ASelect4 { ASelect4(((self.0 >> 32) & 0xF) as u32) }
    pub fn bsel4(&self) -> BSelect4 { BSelect4(((self.0 >> 28) & 0xF) as u32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVadd(pub u64);

#[rustfmt::skip]
impl InstVadd {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm16(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 56) & 0x3) as u32) }
    pub fn d_format(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 46) & 0x8) as i32) | (((self.0 >> 28) & 0x7) as i32)) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn video_op(&self) -> VideoOp { VideoOp(((self.0 >> 51) & 0x7) as u32) }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVmad(pub u64);

#[rustfmt::skip]
impl InstVmad {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm16(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 46) & 0x8) as i32) | (((self.0 >> 28) & 0x7) as i32)) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn avg_mode(&self) -> AvgMode { AvgMode(((self.0 >> 53) & 0x3) as u32) }
    pub fn video_scale(&self) -> VideoScale { VideoScale(((self.0 >> 51) & 0x3) as u32) }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVmnmx(pub u64);

#[rustfmt::skip]
impl InstVmnmx {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm16(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn d_format(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 46) & 0x8) as i32) | (((self.0 >> 28) & 0x7) as i32)) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn video_op(&self) -> VideoOp { VideoOp(((self.0 >> 51) & 0x7) as u32) }
    pub fn mn(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVote(pub u64);

#[rustfmt::skip]
impl InstVote {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn vote_mode(&self) -> VoteMode { VoteMode(((self.0 >> 48) & 0x3) as u32) }
    pub fn vp_dest(&self) -> i32 { (((self.0 >> 45) & 0x7) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVotevtg(pub u64);

#[rustfmt::skip]
impl InstVotevtg {
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn vote_mode(&self) -> VoteMode { VoteMode(((self.0 >> 48) & 0x3) as u32) }
    pub fn imm28(&self) -> i32 { (((self.0 >> 20) & 0xFFFFFFF) as i32) }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVset(pub u64);

#[rustfmt::skip]
impl InstVset {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn v_comp(&self) -> IComp { IComp(((self.0 >> 54) & 0x7) as u32) }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 46) & 0x8) as i32) | (((self.0 >> 28) & 0x7) as i32)) as u32) }
    pub fn video_op(&self) -> VideoOp { VideoOp(((self.0 >> 51) & 0x7) as u32) }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVsetp(pub u64);

#[rustfmt::skip]
impl InstVsetp {
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn imm16(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 46) & 0x8) as i32) | (((self.0 >> 28) & 0x7) as i32)) as u32) }
    pub fn v_comp(&self) -> IComp { IComp(((((self.0 >> 45) & 0x4) as i32) | (((self.0 >> 43) & 0x3) as i32)) as u32) }
    pub fn bool_op(&self) -> BoolOp { BoolOp(((self.0 >> 45) & 0x3) as u32) }
    pub fn src_pred(&self) -> i32 { (((self.0 >> 39) & 0x7) as i32) }
    pub fn src_pred_inv(&self) -> bool { (self.0 & 0x40000000000) != 0 }
    pub fn dest_pred(&self) -> i32 { (((self.0 >> 3) & 0x7) as i32) }
    pub fn dest_pred_inv(&self) -> i32 { ((self.0 & 0x7) as i32) }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVshl(pub u64);

#[rustfmt::skip]
impl InstVshl {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn mv(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn d_format(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((self.0 >> 28) & 0x7) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn video_op(&self) -> VideoOp { VideoOp(((self.0 >> 51) & 0x7) as u32) }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstVshr(pub u64);

#[rustfmt::skip]
impl InstVshr {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn mv(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn d_format(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn a_select(&self) -> VectorSelect { VectorSelect(((((self.0 >> 45) & 0x8) as i32) | (((self.0 >> 36) & 0x7) as i32)) as u32) }
    pub fn b_select(&self) -> VectorSelect { VectorSelect(((self.0 >> 28) & 0x7) as u32) }
    pub fn sat(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn video_op(&self) -> VideoOp { VideoOp(((self.0 >> 51) & 0x7) as u32) }
    pub fn b_video(&self) -> bool { (self.0 & 0x4000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstXmadR(pub u64);

#[rustfmt::skip]
impl InstXmadR {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_b(&self) -> i32 { (((self.0 >> 20) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn hilo_a(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn xmad_cop(&self) -> XmadCop { XmadCop(((self.0 >> 50) & 0x7) as u32) }
    pub fn b_signed(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x4000000000) != 0 }
    pub fn mrg(&self) -> bool { (self.0 & 0x2000000000) != 0 }
    pub fn psl(&self) -> bool { (self.0 & 0x1000000000) != 0 }
    pub fn hilo_b(&self) -> bool { (self.0 & 0x800000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstXmadI(pub u64);

#[rustfmt::skip]
impl InstXmadI {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn imm16(&self) -> i32 { (((self.0 >> 20) & 0xFFFF) as i32) }
    pub fn hilo_a(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn xmad_cop(&self) -> XmadCop { XmadCop(((self.0 >> 50) & 0x7) as u32) }
    pub fn b_signed(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x4000000000) != 0 }
    pub fn mrg(&self) -> bool { (self.0 & 0x2000000000) != 0 }
    pub fn psl(&self) -> bool { (self.0 & 0x1000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstXmadC(pub u64);

#[rustfmt::skip]
impl InstXmadC {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn mrg(&self) -> bool { (self.0 & 0x100000000000000) != 0 }
    pub fn psl(&self) -> bool { (self.0 & 0x80000000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn hilo_a(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn hilo_b(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn xmad_cop(&self) -> XmadCop2 { XmadCop2(((self.0 >> 50) & 0x3) as u32) }
    pub fn b_signed(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

#[derive(Clone, Copy, Debug)]
pub struct InstXmadRc(pub u64);

#[rustfmt::skip]
impl InstXmadRc {
    pub fn dest(&self) -> i32 { ((self.0 & 0xFF) as i32) }
    pub fn src_a(&self) -> i32 { (((self.0 >> 8) & 0xFF) as i32) }
    pub fn src_c(&self) -> i32 { (((self.0 >> 39) & 0xFF) as i32) }
    pub fn cbuf_slot(&self) -> i32 { (((self.0 >> 34) & 0x1F) as i32) }
    pub fn cbuf_offset(&self) -> i32 { (((self.0 >> 20) & 0x3FFF) as i32) }
    pub fn pred(&self) -> i32 { (((self.0 >> 16) & 0x7) as i32) }
    pub fn pred_inv(&self) -> bool { (self.0 & 0x80000) != 0 }
    pub fn write_cc(&self) -> bool { (self.0 & 0x800000000000) != 0 }
    pub fn x(&self) -> bool { (self.0 & 0x40000000000000) != 0 }
    pub fn hilo_a(&self) -> bool { (self.0 & 0x20000000000000) != 0 }
    pub fn hilo_b(&self) -> bool { (self.0 & 0x10000000000000) != 0 }
    pub fn xmad_cop(&self) -> XmadCop2 { XmadCop2(((self.0 >> 50) & 0x3) as u32) }
    pub fn b_signed(&self) -> bool { (self.0 & 0x2000000000000) != 0 }
    pub fn a_signed(&self) -> bool { (self.0 & 0x1000000000000) != 0 }
}

