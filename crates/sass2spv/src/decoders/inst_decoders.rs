// Generated from Ryujinx.Graphics.Shader/Decoders/InstDecoders.cs - do not edit
#![allow(non_camel_case_types, non_snake_case, dead_code, clippy::all)]

use super::register::RegisterConsts;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum AlSize {
    #[default]
    _32 = 0,
    _64 = 1,
    _96 = 2,
    _128 = 3,
}

impl AlSize {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::_32,
            1 => Self::_64,
            2 => Self::_96,
            3 => Self::_128,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum AtomSize {
    #[default]
    U32 = 0,
    S32 = 1,
    U64 = 2,
    F32FtzRn = 3,
    F16x2FtzRn = 4,
    S64 = 5,
}

impl AtomSize {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U32,
            1 => Self::S32,
            2 => Self::U64,
            3 => Self::F32FtzRn,
            4 => Self::F16x2FtzRn,
            5 => Self::S64,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum AtomOp {
    #[default]
    Add = 0,
    Min = 1,
    Max = 2,
    Inc = 3,
    Dec = 4,
    And = 5,
    Or = 6,
    Xor = 7,
    Exch = 8,
    Safeadd = 10,
}

impl AtomOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Add,
            1 => Self::Min,
            2 => Self::Max,
            3 => Self::Inc,
            4 => Self::Dec,
            5 => Self::And,
            6 => Self::Or,
            7 => Self::Xor,
            8 => Self::Exch,
            10 => Self::Safeadd,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum AtomsSize {
    #[default]
    U32 = 0,
    S32 = 1,
    U64 = 2,
    S64 = 3,
}

impl AtomsSize {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U32,
            1 => Self::S32,
            2 => Self::U64,
            3 => Self::S64,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum BarMode {
    #[default]
    Bar = 0,
    Result = 1,
    Warp = 2,
}

impl BarMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Bar,
            1 => Self::Result,
            2 => Self::Warp,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum BarOp {
    #[default]
    Sync = 0,
    Arv = 1,
    Red = 2,
    Scan = 3,
    SyncAll = 4,
}

impl BarOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Sync,
            1 => Self::Arv,
            2 => Self::Red,
            3 => Self::Scan,
            4 => Self::SyncAll,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum BarRedOp {
    #[default]
    Popc = 0,
    And = 1,
    Or = 2,
}

impl BarRedOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Popc,
            1 => Self::And,
            2 => Self::Or,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Bpt {
    #[default]
    DrainIllegal = 0,
    Cal = 1,
    Pause = 2,
    Trap = 3,
    Int = 4,
    Drain = 5,
}

impl Bpt {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::DrainIllegal,
            1 => Self::Cal,
            2 => Self::Pause,
            3 => Self::Trap,
            4 => Self::Int,
            5 => Self::Drain,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Ccc {
    #[default]
    F = 0,
    Lt = 1,
    Eq = 2,
    Le = 3,
    Gt = 4,
    Ne = 5,
    Ge = 6,
    Num = 7,
    Nan = 8,
    Ltu = 9,
    Equ = 10,
    Leu = 11,
    Gtu = 12,
    Neu = 13,
    Geu = 14,
    T = 15,
    Off = 16,
    Lo = 17,
    Sff = 18,
    Ls = 19,
    Hi = 20,
    Sft = 21,
    Hs = 22,
    Oft = 23,
    CsmTa = 24,
    CsmTr = 25,
    CsmMx = 26,
    FcsmTa = 27,
    FcsmTr = 28,
    FcsmMx = 29,
    Rle = 30,
    Rgt = 31,
}

impl Ccc {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::F,
            1 => Self::Lt,
            2 => Self::Eq,
            3 => Self::Le,
            4 => Self::Gt,
            5 => Self::Ne,
            6 => Self::Ge,
            7 => Self::Num,
            8 => Self::Nan,
            9 => Self::Ltu,
            10 => Self::Equ,
            11 => Self::Leu,
            12 => Self::Gtu,
            13 => Self::Neu,
            14 => Self::Geu,
            15 => Self::T,
            16 => Self::Off,
            17 => Self::Lo,
            18 => Self::Sff,
            19 => Self::Ls,
            20 => Self::Hi,
            21 => Self::Sft,
            22 => Self::Hs,
            23 => Self::Oft,
            24 => Self::CsmTa,
            25 => Self::CsmTr,
            26 => Self::CsmMx,
            27 => Self::FcsmTa,
            28 => Self::FcsmTr,
            29 => Self::FcsmMx,
            30 => Self::Rle,
            31 => Self::Rgt,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum CacheType {
    #[default]
    U = 1,
    C = 2,
    I = 3,
    Crs = 4,
}

impl CacheType {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::U,
            2 => Self::C,
            3 => Self::I,
            4 => Self::Crs,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum CctlOp {
    #[default]
    Pf1 = 1,
    Pf1_5 = 2,
    Pf2 = 3,
    Wb = 4,
    Iv = 5,
    Ivall = 6,
    Rs = 7,
    Rslb = 9,
}

impl CctlOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Pf1,
            2 => Self::Pf1_5,
            3 => Self::Pf2,
            4 => Self::Wb,
            5 => Self::Iv,
            6 => Self::Ivall,
            7 => Self::Rs,
            9 => Self::Rslb,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum CctltOp {
    #[default]
    Ivth = 1,
}

impl CctltOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Ivth,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum BoolOp {
    #[default]
    And = 0,
    Or = 1,
    Xor = 2,
}

impl BoolOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::And,
            1 => Self::Or,
            2 => Self::Xor,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum SReg {
    #[default]
    LaneId = 0,
    Clock = 1,
    VirtCfg = 2,
    VirtId = 3,
    Pm0 = 4,
    Pm1 = 5,
    Pm2 = 6,
    Pm3 = 7,
    Pm4 = 8,
    Pm5 = 9,
    Pm6 = 10,
    Pm7 = 11,
    OrderingTicket = 15,
    PrimType = 16,
    InvocationId = 17,
    YDirection = 18,
    ThreadKill = 19,
    ShaderType = 20,
    DirectCbeWriteAddressLow = 21,
    DirectCbeWriteAddressHigh = 22,
    DirectCbeWriteEnabled = 23,
    MachineId0 = 24,
    MachineId1 = 25,
    MachineId2 = 26,
    MachineId3 = 27,
    Affinity = 28,
    InvocationInfo = 29,
    WScaleFactorXY = 30,
    WScaleFactorZ = 31,
    TId = 32,
    TIdX = 33,
    TIdY = 34,
    TIdZ = 35,
    CtaParam = 36,
    CtaIdX = 37,
    CtaIdY = 38,
    CtaIdZ = 39,
    Ntid = 40,
    CirQueueIncrMinusOne = 41,
    Nlatc = 42,
    Swinlo = 48,
    Swinsz = 49,
    Smemsz = 50,
    Smembanks = 51,
    LWinLo = 52,
    LWinSz = 53,
    LMemLoSz = 54,
    LMemHiOff = 55,
    EqMask = 56,
    LtMask = 57,
    LeMask = 58,
    GtMask = 59,
    GeMask = 60,
    RegAlloc = 61,
    CtxAddr = 62,
    GlobalErrorStatus = 64,
    WarpErrorStatus = 66,
    WarpErrorStatusClear = 67,
    PmHi0 = 72,
    PmHi1 = 73,
    PmHi2 = 74,
    PmHi3 = 75,
    PmHi4 = 76,
    PmHi5 = 77,
    PmHi6 = 78,
    PmHi7 = 79,
    ClockLo = 80,
    ClockHi = 81,
    GlobalTimerLo = 82,
    GlobalTimerHi = 83,
    HwTaskId = 96,
    CircularQueueEntryIndex = 97,
    CircularQueueEntryAddressLow = 98,
    CircularQueueEntryAddressHigh = 99,
}

impl SReg {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::LaneId,
            1 => Self::Clock,
            2 => Self::VirtCfg,
            3 => Self::VirtId,
            4 => Self::Pm0,
            5 => Self::Pm1,
            6 => Self::Pm2,
            7 => Self::Pm3,
            8 => Self::Pm4,
            9 => Self::Pm5,
            10 => Self::Pm6,
            11 => Self::Pm7,
            15 => Self::OrderingTicket,
            16 => Self::PrimType,
            17 => Self::InvocationId,
            18 => Self::YDirection,
            19 => Self::ThreadKill,
            20 => Self::ShaderType,
            21 => Self::DirectCbeWriteAddressLow,
            22 => Self::DirectCbeWriteAddressHigh,
            23 => Self::DirectCbeWriteEnabled,
            24 => Self::MachineId0,
            25 => Self::MachineId1,
            26 => Self::MachineId2,
            27 => Self::MachineId3,
            28 => Self::Affinity,
            29 => Self::InvocationInfo,
            30 => Self::WScaleFactorXY,
            31 => Self::WScaleFactorZ,
            32 => Self::TId,
            33 => Self::TIdX,
            34 => Self::TIdY,
            35 => Self::TIdZ,
            36 => Self::CtaParam,
            37 => Self::CtaIdX,
            38 => Self::CtaIdY,
            39 => Self::CtaIdZ,
            40 => Self::Ntid,
            41 => Self::CirQueueIncrMinusOne,
            42 => Self::Nlatc,
            48 => Self::Swinlo,
            49 => Self::Swinsz,
            50 => Self::Smemsz,
            51 => Self::Smembanks,
            52 => Self::LWinLo,
            53 => Self::LWinSz,
            54 => Self::LMemLoSz,
            55 => Self::LMemHiOff,
            56 => Self::EqMask,
            57 => Self::LtMask,
            58 => Self::LeMask,
            59 => Self::GtMask,
            60 => Self::GeMask,
            61 => Self::RegAlloc,
            62 => Self::CtxAddr,
            64 => Self::GlobalErrorStatus,
            66 => Self::WarpErrorStatus,
            67 => Self::WarpErrorStatusClear,
            72 => Self::PmHi0,
            73 => Self::PmHi1,
            74 => Self::PmHi2,
            75 => Self::PmHi3,
            76 => Self::PmHi4,
            77 => Self::PmHi5,
            78 => Self::PmHi6,
            79 => Self::PmHi7,
            80 => Self::ClockLo,
            81 => Self::ClockHi,
            82 => Self::GlobalTimerLo,
            83 => Self::GlobalTimerHi,
            96 => Self::HwTaskId,
            97 => Self::CircularQueueEntryIndex,
            98 => Self::CircularQueueEntryAddressLow,
            99 => Self::CircularQueueEntryAddressHigh,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum RoundMode {
    #[default]
    Rn = 0,
    Rm = 1,
    Rp = 2,
    Rz = 3,
}

impl RoundMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Rn,
            1 => Self::Rm,
            2 => Self::Rp,
            3 => Self::Rz,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum FComp {
    #[default]
    F = 0,
    Lt = 1,
    Eq = 2,
    Le = 3,
    Gt = 4,
    Ne = 5,
    Ge = 6,
    Num = 7,
    Nan = 8,
    Ltu = 9,
    Equ = 10,
    Leu = 11,
    Gtu = 12,
    Neu = 13,
    Geu = 14,
    T = 15,
}

impl FComp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::F,
            1 => Self::Lt,
            2 => Self::Eq,
            3 => Self::Le,
            4 => Self::Gt,
            5 => Self::Ne,
            6 => Self::Ge,
            7 => Self::Num,
            8 => Self::Nan,
            9 => Self::Ltu,
            10 => Self::Equ,
            11 => Self::Leu,
            12 => Self::Gtu,
            13 => Self::Neu,
            14 => Self::Geu,
            15 => Self::T,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum IntegerRound {
    #[default]
    Pass = 1,
    Round = 4,
    Floor = 5,
    Ceil = 6,
    Trunc = 7,
}

impl IntegerRound {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Pass,
            4 => Self::Round,
            5 => Self::Floor,
            6 => Self::Ceil,
            7 => Self::Trunc,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum IDstFmt {
    #[default]
    U16 = 1,
    U32 = 2,
    U64 = 3,
    S16 = 5,
    S32 = 6,
    S64 = 7,
}

impl IDstFmt {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::U16,
            2 => Self::U32,
            3 => Self::U64,
            5 => Self::S16,
            6 => Self::S32,
            7 => Self::S64,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ISrcFmt {
    #[default]
    U8 = 0,
    U16 = 1,
    U32 = 2,
    U64 = 3,
    S8 = 4,
    S16 = 5,
    S32 = 6,
    S64 = 7,
}

impl ISrcFmt {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U8,
            1 => Self::U16,
            2 => Self::U32,
            3 => Self::U64,
            4 => Self::S8,
            5 => Self::S16,
            6 => Self::S32,
            7 => Self::S64,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ISrcDstFmt {
    #[default]
    U8 = 0,
    U16 = 1,
    U32 = 2,
    S8 = 4,
    S16 = 5,
    S32 = 6,
}

impl ISrcDstFmt {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U8,
            1 => Self::U16,
            2 => Self::U32,
            4 => Self::S8,
            5 => Self::S16,
            6 => Self::S32,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum RoundMode2 {
    #[default]
    Round = 0,
    Floor = 1,
    Ceil = 2,
    Trunc = 3,
}

impl RoundMode2 {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Round,
            1 => Self::Floor,
            2 => Self::Ceil,
            3 => Self::Trunc,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ChkModeF {
    #[default]
    Divide = 0,
}

impl ChkModeF {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Divide,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Fmz {
    #[default]
    Ftz = 1,
    Fmz = 2,
}

impl Fmz {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Ftz,
            2 => Self::Fmz,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum MultiplyScale {
    #[default]
    NoScale = 0,
    D2 = 1,
    D4 = 2,
    D8 = 3,
    M8 = 4,
    M4 = 5,
    M2 = 6,
}

impl MultiplyScale {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::NoScale,
            1 => Self::D2,
            2 => Self::D4,
            3 => Self::D8,
            4 => Self::M8,
            5 => Self::M4,
            6 => Self::M2,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum OFmt {
    #[default]
    F16 = 0,
    F32 = 1,
    MrgH0 = 2,
    MrgH1 = 3,
}

impl OFmt {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::F16,
            1 => Self::F32,
            2 => Self::MrgH0,
            3 => Self::MrgH1,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum HalfSwizzle {
    #[default]
    F16 = 0,
    F32 = 1,
    H0H0 = 2,
    H1H1 = 3,
}

impl HalfSwizzle {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::F16,
            1 => Self::F32,
            2 => Self::H0H0,
            3 => Self::H1H1,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ByteSel {
    #[default]
    B0 = 0,
    B1 = 1,
    B2 = 2,
    B3 = 3,
}

impl ByteSel {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::B0,
            1 => Self::B1,
            2 => Self::B2,
            3 => Self::B3,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum DstFmt {
    #[default]
    F16 = 1,
    F32 = 2,
    F64 = 3,
}

impl DstFmt {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::F16,
            2 => Self::F32,
            3 => Self::F64,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum AvgMode {
    #[default]
    NoNeg = 0,
    NegB = 1,
    NegA = 2,
    PlusOne = 3,
}

impl AvgMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::NoNeg,
            1 => Self::NegB,
            2 => Self::NegA,
            3 => Self::PlusOne,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Lrs {
    #[default]
    None = 0,
    RightShift = 1,
    LeftShift = 2,
}

impl Lrs {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::None,
            1 => Self::RightShift,
            2 => Self::LeftShift,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum HalfSelect {
    #[default]
    B32 = 0,
    H0 = 1,
    H1 = 2,
}

impl HalfSelect {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::B32,
            1 => Self::H0,
            2 => Self::H1,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum IComp {
    #[default]
    F = 0,
    Lt = 1,
    Eq = 2,
    Le = 3,
    Gt = 4,
    Ne = 5,
    Ge = 6,
    T = 7,
}

impl IComp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::F,
            1 => Self::Lt,
            2 => Self::Eq,
            3 => Self::Le,
            4 => Self::Gt,
            5 => Self::Ne,
            6 => Self::Ge,
            7 => Self::T,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum XMode {
    #[default]
    Xlo = 1,
    Xmed = 2,
    Xhi = 3,
}

impl XMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Xlo,
            2 => Self::Xmed,
            3 => Self::Xhi,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum IpaOp {
    #[default]
    Pass = 0,
    Multiply = 1,
    Constant = 2,
    Sc = 3,
}

impl IpaOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Pass,
            1 => Self::Multiply,
            2 => Self::Constant,
            3 => Self::Sc,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum IBase {
    #[default]
    Patch = 1,
    Prim = 2,
    Attr = 3,
}

impl IBase {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Patch,
            2 => Self::Prim,
            3 => Self::Attr,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum CacheOpLd {
    #[default]
    Ca = 0,
    Cg = 1,
    Ci = 2,
    Cv = 3,
}

impl CacheOpLd {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Ca,
            1 => Self::Cg,
            2 => Self::Ci,
            3 => Self::Cv,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum CacheOpSt {
    #[default]
    Wb = 0,
    Cg = 1,
    Ci = 2,
    Wt = 3,
}

impl CacheOpSt {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Wb,
            1 => Self::Cg,
            2 => Self::Ci,
            3 => Self::Wt,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum LsSize {
    #[default]
    U8 = 0,
    S8 = 1,
    U16 = 2,
    S16 = 3,
    B32 = 4,
    B64 = 5,
    B128 = 6,
    UB128 = 7,
}

impl LsSize {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U8,
            1 => Self::S8,
            2 => Self::U16,
            3 => Self::S16,
            4 => Self::B32,
            5 => Self::B64,
            6 => Self::B128,
            7 => Self::UB128,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum LsSize2 {
    #[default]
    U8 = 0,
    S8 = 1,
    U16 = 2,
    S16 = 3,
    B32 = 4,
    B64 = 5,
    B128 = 6,
}

impl LsSize2 {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U8,
            1 => Self::S8,
            2 => Self::U16,
            3 => Self::S16,
            4 => Self::B32,
            5 => Self::B64,
            6 => Self::B128,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum AddressMode {
    #[default]
    Il = 1,
    Is = 2,
    Isl = 3,
}

impl AddressMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Il,
            2 => Self::Is,
            3 => Self::Isl,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum CacheOp2 {
    #[default]
    Lu = 1,
    Ci = 2,
    Cv = 3,
}

impl CacheOp2 {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Lu,
            2 => Self::Ci,
            3 => Self::Cv,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum PredicateOp {
    #[default]
    F = 0,
    T = 1,
    Z = 2,
    Nz = 3,
}

impl PredicateOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::F,
            1 => Self::T,
            2 => Self::Z,
            3 => Self::Nz,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum LogicOp {
    #[default]
    And = 0,
    Or = 1,
    Xor = 2,
    PassB = 3,
}

impl LogicOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::And,
            1 => Self::Or,
            2 => Self::Xor,
            3 => Self::PassB,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Membar {
    #[default]
    Cta = 0,
    Gl = 1,
    Sys = 2,
    Vc = 3,
}

impl Membar {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Cta,
            1 => Self::Gl,
            2 => Self::Sys,
            3 => Self::Vc,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Ivall {
    #[default]
    Ivalld = 1,
    Ivallt = 2,
    Ivalltd = 3,
}

impl Ivall {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Ivalld,
            2 => Self::Ivallt,
            3 => Self::Ivalltd,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum MufuOp {
    #[default]
    Cos = 0,
    Sin = 1,
    Ex2 = 2,
    Lg2 = 3,
    Rcp = 4,
    Rsq = 5,
    Rcp64h = 6,
    Rsq64h = 7,
    Sqrt = 8,
}

impl MufuOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Cos,
            1 => Self::Sin,
            2 => Self::Ex2,
            3 => Self::Lg2,
            4 => Self::Rcp,
            5 => Self::Rsq,
            6 => Self::Rcp64h,
            7 => Self::Rsq64h,
            8 => Self::Sqrt,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum OutType {
    #[default]
    Emit = 1,
    Cut = 2,
    EmitThenCut = 3,
}

impl OutType {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Emit,
            2 => Self::Cut,
            3 => Self::EmitThenCut,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum PixMode {
    #[default]
    Covmask = 1,
    Covered = 2,
    Offset = 3,
    CentroidOffset = 4,
    MyIndex = 5,
}

impl PixMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Covmask,
            2 => Self::Covered,
            3 => Self::Offset,
            4 => Self::CentroidOffset,
            5 => Self::MyIndex,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum PMode {
    #[default]
    F4e = 1,
    B4e = 2,
    Rc8 = 3,
    Ecl = 4,
    Ecr = 5,
    Rc16 = 6,
}

impl PMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::F4e,
            2 => Self::B4e,
            3 => Self::Rc8,
            4 => Self::Ecl,
            5 => Self::Ecr,
            6 => Self::Rc16,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum RedOp {
    #[default]
    Add = 0,
    Min = 1,
    Max = 2,
    Inc = 3,
    Dec = 4,
    And = 5,
    Or = 6,
    Xor = 7,
}

impl RedOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Add,
            1 => Self::Min,
            2 => Self::Max,
            3 => Self::Inc,
            4 => Self::Dec,
            5 => Self::And,
            6 => Self::Or,
            7 => Self::Xor,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum XModeShf {
    #[default]
    Hi = 1,
    X = 2,
    Xhi = 3,
}

impl XModeShf {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Hi,
            2 => Self::X,
            3 => Self::Xhi,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum MaxShift {
    #[default]
    U64 = 2,
    S64 = 3,
}

impl MaxShift {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            2 => Self::U64,
            3 => Self::S64,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ShflMode {
    #[default]
    Idx = 0,
    Up = 1,
    Down = 2,
    Bfly = 3,
}

impl ShflMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Idx,
            1 => Self::Up,
            2 => Self::Down,
            3 => Self::Bfly,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Clamp {
    #[default]
    Ign = 0,
    Trap = 2,
}

impl Clamp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Ign,
            2 => Self::Trap,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum SuatomSize {
    #[default]
    U32 = 0,
    S32 = 1,
    U64 = 2,
    F32FtzRn = 3,
    F16x2FtzRn = 4,
    S64 = 5,
    Sd32 = 6,
    Sd64 = 7,
}

impl SuatomSize {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U32,
            1 => Self::S32,
            2 => Self::U64,
            3 => Self::F32FtzRn,
            4 => Self::F16x2FtzRn,
            5 => Self::S64,
            6 => Self::Sd32,
            7 => Self::Sd64,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum SuDim {
    #[default]
    _1d = 0,
    _1dBuffer = 1,
    _1dArray = 2,
    _2d = 3,
    _2dArray = 4,
    _3d = 5,
}

impl SuDim {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::_1d,
            1 => Self::_1dBuffer,
            2 => Self::_1dArray,
            3 => Self::_2d,
            4 => Self::_2dArray,
            5 => Self::_3d,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum SuatomOp {
    #[default]
    Add = 0,
    Min = 1,
    Max = 2,
    Inc = 3,
    Dec = 4,
    And = 5,
    Or = 6,
    Xor = 7,
    Exch = 8,
}

impl SuatomOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Add,
            1 => Self::Min,
            2 => Self::Max,
            3 => Self::Inc,
            4 => Self::Dec,
            5 => Self::And,
            6 => Self::Or,
            7 => Self::Xor,
            8 => Self::Exch,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum SuSize {
    #[default]
    U8 = 0,
    S8 = 1,
    U16 = 2,
    S16 = 3,
    B32 = 4,
    B64 = 5,
    B128 = 6,
    UB128 = 7,
}

impl SuSize {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U8,
            1 => Self::S8,
            2 => Self::U16,
            3 => Self::S16,
            4 => Self::B32,
            5 => Self::B64,
            6 => Self::B128,
            7 => Self::UB128,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum SuRgba {
    #[default]
    R = 1,
    G = 2,
    Rg = 3,
    B = 4,
    Rb = 5,
    Gb = 6,
    Rgb = 7,
    A = 8,
    Ra = 9,
    Ga = 10,
    Rga = 11,
    Ba = 12,
    Rba = 13,
    Gba = 14,
    Rgba = 15,
}

impl SuRgba {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::R,
            2 => Self::G,
            3 => Self::Rg,
            4 => Self::B,
            5 => Self::Rb,
            6 => Self::Gb,
            7 => Self::Rgb,
            8 => Self::A,
            9 => Self::Ra,
            10 => Self::Ga,
            11 => Self::Rga,
            12 => Self::Ba,
            13 => Self::Rba,
            14 => Self::Gba,
            15 => Self::Rgba,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum Lod {
    #[default]
    Lz = 1,
    Lb = 2,
    Ll = 3,
    Lba = 6,
    Lla = 7,
}

impl Lod {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Lz,
            2 => Self::Lb,
            3 => Self::Ll,
            6 => Self::Lba,
            7 => Self::Lla,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum TexDim {
    #[default]
    _1d = 0,
    Array1d = 1,
    _2d = 2,
    Array2d = 3,
    _3d = 4,
    Array3d = 5,
    Cube = 6,
    ArrayCube = 7,
}

impl TexDim {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::_1d,
            1 => Self::Array1d,
            2 => Self::_2d,
            3 => Self::Array2d,
            4 => Self::_3d,
            5 => Self::Array3d,
            6 => Self::Cube,
            7 => Self::ArrayCube,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum TexsTarget {
    #[default]
    Texture1DLodZero = 0,
    Texture2D = 1,
    Texture2DLodZero = 2,
    Texture2DLodLevel = 3,
    Texture2DDepthCompare = 4,
    Texture2DLodLevelDepthCompare = 5,
    Texture2DLodZeroDepthCompare = 6,
    Texture2DArray = 7,
    Texture2DArrayLodZero = 8,
    Texture2DArrayLodZeroDepthCompare = 9,
    Texture3D = 10,
    Texture3DLodZero = 11,
    TextureCube = 12,
    TextureCubeLodLevel = 13,
}

impl TexsTarget {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Texture1DLodZero,
            1 => Self::Texture2D,
            2 => Self::Texture2DLodZero,
            3 => Self::Texture2DLodLevel,
            4 => Self::Texture2DDepthCompare,
            5 => Self::Texture2DLodLevelDepthCompare,
            6 => Self::Texture2DLodZeroDepthCompare,
            7 => Self::Texture2DArray,
            8 => Self::Texture2DArrayLodZero,
            9 => Self::Texture2DArrayLodZeroDepthCompare,
            10 => Self::Texture3D,
            11 => Self::Texture3DLodZero,
            12 => Self::TextureCube,
            13 => Self::TextureCubeLodLevel,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum TldsTarget {
    #[default]
    Texture1DLodZero = 0x0,
    Texture1DLodLevel = 0x1,
    Texture2DLodZero = 0x2,
    Texture2DLodZeroOffset = 0x4,
    Texture2DLodLevel = 0x5,
    Texture2DLodZeroMultisample = 0x6,
    Texture3DLodZero = 0x7,
    Texture2DArrayLodZero = 0x8,
    Texture2DLodLevelOffset = 0xc,
}

impl TldsTarget {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0x0 => Self::Texture1DLodZero,
            0x1 => Self::Texture1DLodLevel,
            0x2 => Self::Texture2DLodZero,
            0x4 => Self::Texture2DLodZeroOffset,
            0x5 => Self::Texture2DLodLevel,
            0x6 => Self::Texture2DLodZeroMultisample,
            0x7 => Self::Texture3DLodZero,
            0x8 => Self::Texture2DArrayLodZero,
            0xc => Self::Texture2DLodLevelOffset,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum TexComp {
    #[default]
    R = 0,
    G = 1,
    B = 2,
    A = 3,
}

impl TexComp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::R,
            1 => Self::G,
            2 => Self::B,
            3 => Self::A,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum TexOffset {
    #[default]
    None = 0,
    Aoffi = 1,
    Ptp = 2,
}

impl TexOffset {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::None,
            1 => Self::Aoffi,
            2 => Self::Ptp,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum TexQuery {
    #[default]
    TexHeaderDimension = 1,
    TexHeaderTextureType = 2,
    TexHeaderSamplerPos = 5,
    TexSamplerFilter = 16,
    TexSamplerLod = 18,
    TexSamplerWrap = 20,
    TexSamplerBorderColor = 22,
}

impl TexQuery {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::TexHeaderDimension,
            2 => Self::TexHeaderTextureType,
            5 => Self::TexHeaderSamplerPos,
            16 => Self::TexSamplerFilter,
            18 => Self::TexSamplerLod,
            20 => Self::TexSamplerWrap,
            22 => Self::TexSamplerBorderColor,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum VectorSelect {
    #[default]
    U8B0 = 0,
    U8B1 = 1,
    U8B2 = 2,
    U8B3 = 3,
    U16H0 = 4,
    U16H1 = 5,
    U32 = 6,
    S8B0 = 8,
    S8B1 = 9,
    S8B2 = 10,
    S8B3 = 11,
    S16H0 = 12,
    S16H1 = 13,
    S32 = 14,
}

impl VectorSelect {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U8B0,
            1 => Self::U8B1,
            2 => Self::U8B2,
            3 => Self::U8B3,
            4 => Self::U16H0,
            5 => Self::U16H1,
            6 => Self::U32,
            8 => Self::S8B0,
            9 => Self::S8B1,
            10 => Self::S8B2,
            11 => Self::S8B3,
            12 => Self::S16H0,
            13 => Self::S16H1,
            14 => Self::S32,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum VideoOp {
    #[default]
    Mrg16h = 0,
    Mrg16l = 1,
    Mrg8b0 = 2,
    Mrg8b2 = 3,
    Acc = 4,
    Min = 5,
    Max = 6,
}

impl VideoOp {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Mrg16h,
            1 => Self::Mrg16l,
            2 => Self::Mrg8b0,
            3 => Self::Mrg8b2,
            4 => Self::Acc,
            5 => Self::Min,
            6 => Self::Max,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum VideoRed {
    #[default]
    Acc = 1,
}

impl VideoRed {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Acc,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum LaneMask4 {
    #[default]
    Z = 1,
    W = 2,
    Zw = 3,
    X = 4,
    Xz = 5,
    Xw = 6,
    Xzw = 7,
    Y = 8,
    Yz = 9,
    Yw = 10,
    Yzw = 11,
    Xy = 12,
    Xyz = 13,
    Xyw = 14,
    Xyzw = 15,
}

impl LaneMask4 {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Z,
            2 => Self::W,
            3 => Self::Zw,
            4 => Self::X,
            5 => Self::Xz,
            6 => Self::Xw,
            7 => Self::Xzw,
            8 => Self::Y,
            9 => Self::Yz,
            10 => Self::Yw,
            11 => Self::Yzw,
            12 => Self::Xy,
            13 => Self::Xyz,
            14 => Self::Xyw,
            15 => Self::Xyzw,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ASelect4 {
    #[default]
    _0000 = 0,
    _1111 = 1,
    _2222 = 2,
    _3333 = 3,
    _3210 = 4,
    _5432 = 6,
    _6543 = 7,
    _3201 = 8,
    _3012 = 9,
    _0213 = 10,
    _3120 = 11,
    _1230 = 12,
    _2310 = 13,
}

impl ASelect4 {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::_0000,
            1 => Self::_1111,
            2 => Self::_2222,
            3 => Self::_3333,
            4 => Self::_3210,
            6 => Self::_5432,
            7 => Self::_6543,
            8 => Self::_3201,
            9 => Self::_3012,
            10 => Self::_0213,
            11 => Self::_3120,
            12 => Self::_1230,
            13 => Self::_2310,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum BSelect4 {
    #[default]
    _4444 = 0,
    _5555 = 1,
    _6666 = 2,
    _7777 = 3,
    _7654 = 4,
    _5432 = 6,
    _4321 = 7,
    _4567 = 8,
    _6745 = 9,
    _5476 = 10,
}

impl BSelect4 {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::_4444,
            1 => Self::_5555,
            2 => Self::_6666,
            3 => Self::_7777,
            4 => Self::_7654,
            6 => Self::_5432,
            7 => Self::_4321,
            8 => Self::_4567,
            9 => Self::_6745,
            10 => Self::_5476,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum VideoScale {
    #[default]
    Shr7 = 1,
    Shr15 = 2,
}

impl VideoScale {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            1 => Self::Shr7,
            2 => Self::Shr15,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum VoteMode {
    #[default]
    All = 0,
    Any = 1,
    Eq = 2,
}

impl VoteMode {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::All,
            1 => Self::Any,
            2 => Self::Eq,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum XmadCop {
    #[default]
    Cfull = 0,
    Clo = 1,
    Chi = 2,
    Csfu = 3,
    Cbcc = 4,
}

impl XmadCop {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Cfull,
            1 => Self::Clo,
            2 => Self::Chi,
            3 => Self::Csfu,
            4 => Self::Cbcc,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum XmadCop2 {
    #[default]
    Cfull = 0,
    Clo = 1,
    Chi = 2,
    Csfu = 3,
}

impl XmadCop2 {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::Cfull,
            1 => Self::Clo,
            2 => Self::Chi,
            3 => Self::Csfu,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ImadspASelect {
    #[default]
    U32 = 0,
    S32 = 1,
    U24 = 2,
    S24 = 3,
    U16h0 = 4,
    S16h0 = 5,
    U16h1 = 6,
    S16h1 = 7,
}

impl ImadspASelect {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U32,
            1 => Self::S32,
            2 => Self::U24,
            3 => Self::S24,
            4 => Self::U16h0,
            5 => Self::S16h0,
            6 => Self::U16h1,
            7 => Self::S16h1,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u32)]
pub enum ImadspBSelect {
    #[default]
    U24 = 0,
    S24 = 1,
    U16h0 = 2,
    S16h0 = 3,
}

impl ImadspBSelect {
    pub fn from_u64(val: u64) -> Self {
        match val as u32 {
            0 => Self::U24,
            1 => Self::S24,
            2 => Self::U16h0,
            3 => Self::S16h0,
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstConditional {
    pub opcode: u64,
}

impl InstConditional {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstAl2p {
    pub opcode: u64,
}

impl InstAl2p {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn al_size(&self) -> AlSize {
        AlSize::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn aio(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn imm11(&self) -> i32 {
        (((self.opcode >> 20) & 0x7FF)) as i32
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 44) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstAld {
    pub opcode: u64,
}

impl InstAld {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm11(&self) -> i32 {
        (((self.opcode >> 20) & 0x7FF)) as i32
    }

    pub fn p(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn o(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn al_size(&self) -> AlSize {
        AlSize::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn phys(&self) -> bool {
        !self.p() && self.imm11() == 0 && self.src_a() != RegisterConsts::REGISTER_ZERO_INDEX as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstAst {
    pub opcode: u64,
}

impl InstAst {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm11(&self) -> i32 {
        (((self.opcode >> 20) & 0x7FF)) as i32
    }

    pub fn p(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn al_size(&self) -> AlSize {
        AlSize::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn phys(&self) -> bool {
        !self.p() && self.imm11() == 0 && self.src_a() != RegisterConsts::REGISTER_ZERO_INDEX as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstAtom {
    pub opcode: u64,
}

impl InstAtom {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 28) & 0xFFFFF)) as i32
    }

    pub fn size(&self) -> AtomSize {
        AtomSize::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn op(&self) -> AtomOp {
        AtomOp::from_u64((((self.opcode >> 52) & 0xF)) as u64)
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstAtomCas {
    pub opcode: u64,
}

impl InstAtomCas {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn bc_rz(&self) -> i32 {
        (((self.opcode >> 50) & 0x3)) as i32
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstAtoms {
    pub opcode: u64,
}

impl InstAtoms {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm22(&self) -> i32 {
        (((self.opcode >> 30) & 0x3FFFFF)) as i32
    }

    pub fn atoms_size(&self) -> AtomsSize {
        AtomsSize::from_u64((((self.opcode >> 28) & 0x3)) as u64)
    }

    pub fn atom_op(&self) -> AtomOp {
        AtomOp::from_u64((((self.opcode >> 52) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstAtomsCas {
    pub opcode: u64,
}

impl InstAtomsCas {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn atoms_bc_rz(&self) -> i32 {
        (((self.opcode >> 28) & 0x3)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstB2r {
    pub opcode: u64,
}

impl InstB2r {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 45) & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn mode(&self) -> BarMode {
        BarMode::from_u64((((self.opcode >> 32) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBar {
    pub opcode: u64,
}

impl InstBar {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm12(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn bar_op(&self) -> BarOp {
        BarOp::from_u64((((self.opcode >> 32) & 0x7)) as u64)
    }

    pub fn bar_red_op(&self) -> BarRedOp {
        BarRedOp::from_u64((((self.opcode >> 35) & 0x3)) as u64)
    }

    pub fn a_fix_bar(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn b_fix_bar(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBfeR {
    pub opcode: u64,
}

impl InstBfeR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn brev(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBfeI {
    pub opcode: u64,
}

impl InstBfeI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn brev(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBfeC {
    pub opcode: u64,
}

impl InstBfeC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn brev(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBfiR {
    pub opcode: u64,
}

impl InstBfiR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBfiI {
    pub opcode: u64,
}

impl InstBfiI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBfiC {
    pub opcode: u64,
}

impl InstBfiC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBfiRc {
    pub opcode: u64,
}

impl InstBfiRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBpt {
    pub opcode: u64,
}

impl InstBpt {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFF)) as i32
    }

    pub fn bpt(&self) -> Bpt {
        Bpt::from_u64((((self.opcode >> 6) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBra {
    pub opcode: u64,
}

impl InstBra {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

    pub fn lmt(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn u(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBrk {
    pub opcode: u64,
}

impl InstBrk {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstBrx {
    pub opcode: u64,
}

impl InstBrx {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

    pub fn lmt(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCal {
    pub opcode: u64,
}

impl InstCal {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn inc(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCctl {
    pub opcode: u64,
}

impl InstCctl {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm30(&self) -> i32 {
        (((self.opcode >> 22) & 0x3FFFFFFF)) as i32
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn cache(&self) -> CacheType {
        CacheType::from_u64((((self.opcode >> 4) & 0x7)) as u64)
    }

    pub fn cctl_op(&self) -> CctlOp {
        CctlOp::from_u64(((self.opcode & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCctll {
    pub opcode: u64,
}

impl InstCctll {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm22(&self) -> i32 {
        (((self.opcode >> 22) & 0x3FFFFF)) as i32
    }

    pub fn cache(&self) -> i32 {
        (((self.opcode >> 4) & 0x3)) as i32
    }

    pub fn cctl_op(&self) -> CctlOp {
        CctlOp::from_u64(((self.opcode & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCctlt {
    pub opcode: u64,
}

impl InstCctlt {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ts_idx13(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn cctlt_op(&self) -> CctltOp {
        CctltOp::from_u64(((self.opcode & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCctltR {
    pub opcode: u64,
}

impl InstCctltR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cctlt_op(&self) -> CctltOp {
        CctltOp::from_u64(((self.opcode & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCont {
    pub opcode: u64,
}

impl InstCont {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCset {
    pub opcode: u64,
}

impl InstCset {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64((((self.opcode >> 8) & 0x1F)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCsetp {
    pub opcode: u64,
}

impl InstCsetp {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64((((self.opcode >> 8) & 0x1F)) as u64)
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstCs2r {
    pub opcode: u64,
}

impl InstCs2r {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn s_reg(&self) -> SReg {
        SReg::from_u64((((self.opcode >> 20) & 0xFF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDaddR {
    pub opcode: u64,
}

impl InstDaddR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDaddI {
    pub opcode: u64,
}

impl InstDaddI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDaddC {
    pub opcode: u64,
}

impl InstDaddC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDepbar {
    pub opcode: u64,
}

impl InstDepbar {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn le(&self) -> bool {
        (self.opcode & 0x20000000) != 0
    }

    pub fn sbid(&self) -> i32 {
        (((self.opcode >> 26) & 0x7)) as i32
    }

    pub fn pend_cnt(&self) -> i32 {
        (((self.opcode >> 20) & 0x3F)) as i32
    }

    pub fn imm6(&self) -> i32 {
        ((self.opcode & 0x3F)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDfmaR {
    pub opcode: u64,
}

impl InstDfmaR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 50) & 0x3)) as u64)
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDfmaI {
    pub opcode: u64,
}

impl InstDfmaI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 50) & 0x3)) as u64)
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDfmaC {
    pub opcode: u64,
}

impl InstDfmaC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 50) & 0x3)) as u64)
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDfmaRc {
    pub opcode: u64,
}

impl InstDfmaRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 50) & 0x3)) as u64)
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDmnmxR {
    pub opcode: u64,
}

impl InstDmnmxR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDmnmxI {
    pub opcode: u64,
}

impl InstDmnmxI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDmnmxC {
    pub opcode: u64,
}

impl InstDmnmxC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDmulR {
    pub opcode: u64,
}

impl InstDmulR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDmulI {
    pub opcode: u64,
}

impl InstDmulI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDmulC {
    pub opcode: u64,
}

impl InstDmulC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDsetR {
    pub opcode: u64,
}

impl InstDsetR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDsetI {
    pub opcode: u64,
}

impl InstDsetI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDsetC {
    pub opcode: u64,
}

impl InstDsetC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDsetpR {
    pub opcode: u64,
}

impl InstDsetpR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDsetpI {
    pub opcode: u64,
}

impl InstDsetpI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstDsetpC {
    pub opcode: u64,
}

impl InstDsetpC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstExit {
    pub opcode: u64,
}

impl InstExit {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

    pub fn keep_ref_cnt(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstF2fR {
    pub opcode: u64,
}

impl InstF2fR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn dst_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 8) & 0x3)) as u64)
    }

    pub fn src_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 10) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> IntegerRound {
        IntegerRound::from_u64(((((self.opcode >> 40) & 0x4) | ((self.opcode >> 39) & 0x3))) as u64)
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstF2fI {
    pub opcode: u64,
}

impl InstF2fI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn dst_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 8) & 0x3)) as u64)
    }

    pub fn src_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 10) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> IntegerRound {
        IntegerRound::from_u64(((((self.opcode >> 40) & 0x4) | ((self.opcode >> 39) & 0x3))) as u64)
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstF2fC {
    pub opcode: u64,
}

impl InstF2fC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn dst_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 8) & 0x3)) as u64)
    }

    pub fn src_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 10) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> IntegerRound {
        IntegerRound::from_u64(((((self.opcode >> 40) & 0x4) | ((self.opcode >> 39) & 0x3))) as u64)
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstF2iR {
    pub opcode: u64,
}

impl InstF2iR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn i_dst_fmt(&self) -> IDstFmt {
        IDstFmt::from_u64(((((self.opcode >> 10) & 0x4) | ((self.opcode >> 8) & 0x3))) as u64)
    }

    pub fn src_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 10) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> RoundMode2 {
        RoundMode2::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstF2iI {
    pub opcode: u64,
}

impl InstF2iI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn i_dst_fmt(&self) -> IDstFmt {
        IDstFmt::from_u64(((((self.opcode >> 10) & 0x4) | ((self.opcode >> 8) & 0x3))) as u64)
    }

    pub fn src_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 10) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> RoundMode2 {
        RoundMode2::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstF2iC {
    pub opcode: u64,
}

impl InstF2iC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn i_dst_fmt(&self) -> IDstFmt {
        IDstFmt::from_u64(((((self.opcode >> 10) & 0x4) | ((self.opcode >> 8) & 0x3))) as u64)
    }

    pub fn src_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 10) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> RoundMode2 {
        RoundMode2::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFaddR {
    pub opcode: u64,
}

impl InstFaddR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFaddI {
    pub opcode: u64,
}

impl InstFaddI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFaddC {
    pub opcode: u64,
}

impl InstFaddC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFadd32i {
    pub opcode: u64,
}

impl InstFadd32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x200000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFchkR {
    pub opcode: u64,
}

impl InstFchkR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn chk_mode_f(&self) -> ChkModeF {
        ChkModeF::from_u64((((self.opcode >> 39) & 0x3F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFchkI {
    pub opcode: u64,
}

impl InstFchkI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn chk_mode_f(&self) -> ChkModeF {
        ChkModeF::from_u64((((self.opcode >> 39) & 0x3F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFchkC {
    pub opcode: u64,
}

impl InstFchkC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn chk_mode_f(&self) -> ChkModeF {
        ChkModeF::from_u64((((self.opcode >> 39) & 0x3F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFcmpR {
    pub opcode: u64,
}

impl InstFcmpR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFcmpI {
    pub opcode: u64,
}

impl InstFcmpI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFcmpC {
    pub opcode: u64,
}

impl InstFcmpC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFcmpRc {
    pub opcode: u64,
}

impl InstFcmpRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFfmaR {
    pub opcode: u64,
}

impl InstFfmaR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFfmaI {
    pub opcode: u64,
}

impl InstFfmaI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFfmaC {
    pub opcode: u64,
}

impl InstFfmaC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFfmaRc {
    pub opcode: u64,
}

impl InstFfmaRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFfma32i {
    pub opcode: u64,
}

impl InstFfma32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x200000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFloR {
    pub opcode: u64,
}

impl InstFloR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFloI {
    pub opcode: u64,
}

impl InstFloI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFloC {
    pub opcode: u64,
}

impl InstFloC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn sh(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFmnmxR {
    pub opcode: u64,
}

impl InstFmnmxR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFmnmxI {
    pub opcode: u64,
}

impl InstFmnmxI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFmnmxC {
    pub opcode: u64,
}

impl InstFmnmxC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFmulR {
    pub opcode: u64,
}

impl InstFmulR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn scale(&self) -> MultiplyScale {
        MultiplyScale::from_u64((((self.opcode >> 41) & 0x7)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFmulI {
    pub opcode: u64,
}

impl InstFmulI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn scale(&self) -> MultiplyScale {
        MultiplyScale::from_u64((((self.opcode >> 41) & 0x7)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFmulC {
    pub opcode: u64,
}

impl InstFmulC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn scale(&self) -> MultiplyScale {
        MultiplyScale::from_u64((((self.opcode >> 41) & 0x7)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFmul32i {
    pub opcode: u64,
}

impl InstFmul32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFsetR {
    pub opcode: u64,
}

impl InstFsetR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFsetC {
    pub opcode: u64,
}

impl InstFsetC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFsetI {
    pub opcode: u64,
}

impl InstFsetI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFsetpR {
    pub opcode: u64,
}

impl InstFsetpR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFsetpI {
    pub opcode: u64,
}

impl InstFsetpI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFsetpC {
    pub opcode: u64,
}

impl InstFsetpC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstFswzadd {
    pub opcode: u64,
}

impl InstFswzadd {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x4000000000) != 0
    }

    pub fn pn_word(&self) -> i32 {
        (((self.opcode >> 28) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstGetcrsptr {
    pub opcode: u64,
}

impl InstGetcrsptr {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstGetlmembase {
    pub opcode: u64,
}

impl InstGetlmembase {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHadd2R {
    pub opcode: u64,
}

impl InstHadd2R {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn b_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 28) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x40000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHadd2I {
    pub opcode: u64,
}

impl InstHadd2I {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn bimm_h0(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FF)) as i32
    }

    pub fn bimm_h1(&self) -> i32 {
        (((self.opcode >> 47) & 0x200) | ((self.opcode >> 30) & 0x1FF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHadd2C {
    pub opcode: u64,
}

impl InstHadd2C {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHadd232i {
    pub opcode: u64,
}

impl InstHadd232i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHfma2R {
    pub opcode: u64,
}

impl InstHfma2R {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn b_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 28) & 0x3)) as u64)
    }

    pub fn c_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 35) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x40000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 37) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHfma2I {
    pub opcode: u64,
}

impl InstHfma2I {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn bimm_h0(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FF)) as i32
    }

    pub fn bimm_h1(&self) -> i32 {
        (((self.opcode >> 47) & 0x200) | ((self.opcode >> 30) & 0x1FF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn c_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 57) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHfma2C {
    pub opcode: u64,
}

impl InstHfma2C {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn c_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 57) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHfma2Rc {
    pub opcode: u64,
}

impl InstHfma2Rc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn c_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 57) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHfma232i {
    pub opcode: u64,
}

impl InstHfma232i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 57) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHmul2R {
    pub opcode: u64,
}

impl InstHmul2R {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn b_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 28) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x40000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHmul2I {
    pub opcode: u64,
}

impl InstHmul2I {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn bimm_h0(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FF)) as i32
    }

    pub fn bimm_h1(&self) -> i32 {
        (((self.opcode >> 47) & 0x200) | ((self.opcode >> 30) & 0x1FF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHmul2C {
    pub opcode: u64,
}

impl InstHmul2C {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn o_fmt(&self) -> OFmt {
        OFmt::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHmul232i {
    pub opcode: u64,
}

impl InstHmul232i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn fmz(&self) -> Fmz {
        Fmz::from_u64((((self.opcode >> 55) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHset2R {
    pub opcode: u64,
}

impl InstHset2R {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn b_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 28) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x40000000) != 0
    }

    pub fn bval(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn cmp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 35) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHset2I {
    pub opcode: u64,
}

impl InstHset2I {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn bimm_h0(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FF)) as i32
    }

    pub fn bimm_h1(&self) -> i32 {
        (((self.opcode >> 47) & 0x200) | ((self.opcode >> 30) & 0x1FF)) as i32
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn bval(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn cmp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 49) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHset2C {
    pub opcode: u64,
}

impl InstHset2C {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn bval(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn cmp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 49) & 0xF)) as u64)
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHsetp2R {
    pub opcode: u64,
}

impl InstHsetp2R {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x40000000) != 0
    }

    pub fn f_comp2(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 35) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn h_and(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn b_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 28) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHsetp2I {
    pub opcode: u64,
}

impl InstHsetp2I {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn bimm_h0(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FF)) as i32
    }

    pub fn bimm_h1(&self) -> i32 {
        (((self.opcode >> 47) & 0x200) | ((self.opcode >> 30) & 0x1FF)) as i32
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 49) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn h_and(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstHsetp2C {
    pub opcode: u64,
}

impl InstHsetp2C {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn f_comp(&self) -> FComp {
        FComp::from_u64((((self.opcode >> 49) & 0xF)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn ftz(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn h_and(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn a_swizzle(&self) -> HalfSwizzle {
        HalfSwizzle::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstI2fR {
    pub opcode: u64,
}

impl InstI2fR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn i_src_fmt(&self) -> ISrcFmt {
        ISrcFmt::from_u64(((((self.opcode >> 11) & 0x4) | ((self.opcode >> 10) & 0x3))) as u64)
    }

    pub fn dst_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 8) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstI2fI {
    pub opcode: u64,
}

impl InstI2fI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn i_src_fmt(&self) -> ISrcFmt {
        ISrcFmt::from_u64(((((self.opcode >> 11) & 0x4) | ((self.opcode >> 10) & 0x3))) as u64)
    }

    pub fn dst_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 8) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstI2fC {
    pub opcode: u64,
}

impl InstI2fC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn round_mode(&self) -> RoundMode {
        RoundMode::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

    pub fn i_src_fmt(&self) -> ISrcFmt {
        ISrcFmt::from_u64(((((self.opcode >> 11) & 0x4) | ((self.opcode >> 10) & 0x3))) as u64)
    }

    pub fn dst_fmt(&self) -> DstFmt {
        DstFmt::from_u64((((self.opcode >> 8) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstI2iR {
    pub opcode: u64,
}

impl InstI2iR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn i_dst_fmt(&self) -> ISrcDstFmt {
        ISrcDstFmt::from_u64(((((self.opcode >> 10) & 0x4) | ((self.opcode >> 8) & 0x3))) as u64)
    }

    pub fn i_src_fmt(&self) -> ISrcDstFmt {
        ISrcDstFmt::from_u64(((((self.opcode >> 11) & 0x4) | ((self.opcode >> 10) & 0x3))) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstI2iI {
    pub opcode: u64,
}

impl InstI2iI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn i_dst_fmt(&self) -> ISrcDstFmt {
        ISrcDstFmt::from_u64(((((self.opcode >> 10) & 0x4) | ((self.opcode >> 8) & 0x3))) as u64)
    }

    pub fn i_src_fmt(&self) -> ISrcDstFmt {
        ISrcDstFmt::from_u64(((((self.opcode >> 11) & 0x4) | ((self.opcode >> 10) & 0x3))) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstI2iC {
    pub opcode: u64,
}

impl InstI2iC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn i_dst_fmt(&self) -> ISrcDstFmt {
        ISrcDstFmt::from_u64(((((self.opcode >> 10) & 0x4) | ((self.opcode >> 8) & 0x3))) as u64)
    }

    pub fn i_src_fmt(&self) -> ISrcDstFmt {
        ISrcDstFmt::from_u64(((((self.opcode >> 11) & 0x4) | ((self.opcode >> 10) & 0x3))) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIaddR {
    pub opcode: u64,
}

impl InstIaddR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIaddI {
    pub opcode: u64,
}

impl InstIaddI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIaddC {
    pub opcode: u64,
}

impl InstIaddC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIadd32i {
    pub opcode: u64,
}

impl InstIadd32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 55) & 0x3)) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIadd3R {
    pub opcode: u64,
}

impl InstIadd3R {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn lrs(&self) -> Lrs {
        Lrs::from_u64((((self.opcode >> 37) & 0x3)) as u64)
    }

    pub fn apart(&self) -> HalfSelect {
        HalfSelect::from_u64((((self.opcode >> 35) & 0x3)) as u64)
    }

    pub fn bpart(&self) -> HalfSelect {
        HalfSelect::from_u64((((self.opcode >> 33) & 0x3)) as u64)
    }

    pub fn cpart(&self) -> HalfSelect {
        HalfSelect::from_u64((((self.opcode >> 31) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIadd3I {
    pub opcode: u64,
}

impl InstIadd3I {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIadd3C {
    pub opcode: u64,
}

impl InstIadd3C {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn neg_c(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIcmpR {
    pub opcode: u64,
}

impl InstIcmpR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIcmpI {
    pub opcode: u64,
}

impl InstIcmpI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIcmpC {
    pub opcode: u64,
}

impl InstIcmpC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIcmpRc {
    pub opcode: u64,
}

impl InstIcmpRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIde {
    pub opcode: u64,
}

impl InstIde {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm16(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

    pub fn di(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIdpR {
    pub opcode: u64,
}

impl InstIdpR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn is_hi(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn src_a_sign(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn is_dp(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn src_b_sign(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIdpC {
    pub opcode: u64,
}

impl InstIdpC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn is_hi(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn src_a_sign(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn is_dp(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn src_b_sign(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadR {
    pub opcode: u64,
}

impl InstImadR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadI {
    pub opcode: u64,
}

impl InstImadI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadC {
    pub opcode: u64,
}

impl InstImadC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadRc {
    pub opcode: u64,
}

impl InstImadRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImad32i {
    pub opcode: u64,
}

impl InstImad32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x200000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 55) & 0x3)) as u64)
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadspR {
    pub opcode: u64,
}

impl InstImadspR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn a_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn b_select(&self) -> ImadspBSelect {
        ImadspBSelect::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn c_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64(((((self.opcode >> 50) & 0x6) | ((self.opcode >> 48) & 0x1))) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadspI {
    pub opcode: u64,
}

impl InstImadspI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn a_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn b_select(&self) -> ImadspBSelect {
        ImadspBSelect::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn c_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64(((((self.opcode >> 50) & 0x6) | ((self.opcode >> 48) & 0x1))) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadspC {
    pub opcode: u64,
}

impl InstImadspC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn a_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn b_select(&self) -> ImadspBSelect {
        ImadspBSelect::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn c_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64(((((self.opcode >> 50) & 0x6) | ((self.opcode >> 48) & 0x1))) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImadspRc {
    pub opcode: u64,
}

impl InstImadspRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn a_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn b_select(&self) -> ImadspBSelect {
        ImadspBSelect::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn c_select(&self) -> ImadspASelect {
        ImadspASelect::from_u64(((((self.opcode >> 50) & 0x6) | ((self.opcode >> 48) & 0x1))) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImnmxR {
    pub opcode: u64,
}

impl InstImnmxR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x_mode(&self) -> XMode {
        XMode::from_u64((((self.opcode >> 43) & 0x3)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImnmxI {
    pub opcode: u64,
}

impl InstImnmxI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x_mode(&self) -> XMode {
        XMode::from_u64((((self.opcode >> 43) & 0x3)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImnmxC {
    pub opcode: u64,
}

impl InstImnmxC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x_mode(&self) -> XMode {
        XMode::from_u64((((self.opcode >> 43) & 0x3)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImulR {
    pub opcode: u64,
}

impl InstImulR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImulI {
    pub opcode: u64,
}

impl InstImulI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImulC {
    pub opcode: u64,
}

impl InstImulC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x20000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstImul32i {
    pub opcode: u64,
}

impl InstImul32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn hilo(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIpa {
    pub opcode: u64,
}

impl InstIpa {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ipa_op(&self) -> IpaOp {
        IpaOp::from_u64((((self.opcode >> 54) & 0x3)) as u64)
    }

    pub fn msi(&self) -> i32 {
        (((self.opcode >> 52) & 0x3)) as i32
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn idx(&self) -> bool {
        (self.opcode & 0x4000000000) != 0
    }

    pub fn imm10(&self) -> i32 {
        (((self.opcode >> 28) & 0x3FF)) as i32
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 47) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIsberd {
    pub opcode: u64,
}

impl InstIsberd {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn al_size(&self) -> AlSize {
        AlSize::from_u64((((self.opcode >> 47) & 0x3)) as u64)
    }

    pub fn i_base(&self) -> IBase {
        IBase::from_u64((((self.opcode >> 33) & 0x3)) as u64)
    }

    pub fn o(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn p(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIscaddR {
    pub opcode: u64,
}

impl InstIscaddR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn imm5(&self) -> i32 {
        (((self.opcode >> 39) & 0x1F)) as i32
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIscaddI {
    pub opcode: u64,
}

impl InstIscaddI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn imm5(&self) -> i32 {
        (((self.opcode >> 39) & 0x1F)) as i32
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIscaddC {
    pub opcode: u64,
}

impl InstIscaddC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn imm5(&self) -> i32 {
        (((self.opcode >> 39) & 0x1F)) as i32
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIscadd32i {
    pub opcode: u64,
}

impl InstIscadd32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn imm5(&self) -> i32 {
        (((self.opcode >> 53) & 0x1F)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIsetR {
    pub opcode: u64,
}

impl InstIsetR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIsetI {
    pub opcode: u64,
}

impl InstIsetI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIsetC {
    pub opcode: u64,
}

impl InstIsetC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIsetpR {
    pub opcode: u64,
}

impl InstIsetpR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIsetpI {
    pub opcode: u64,
}

impl InstIsetpI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstIsetpC {
    pub opcode: u64,
}

impl InstIsetpC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn i_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 49) & 0x7)) as u64)
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn bop(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstJcal {
    pub opcode: u64,
}

impl InstJcal {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

    pub fn inc(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstJmp {
    pub opcode: u64,
}

impl InstJmp {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn lmt(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

    pub fn u(&self) -> bool {
        (self.opcode & 0x80) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstJmx {
    pub opcode: u64,
}

impl InstJmx {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn lmt(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstKil {
    pub opcode: u64,
}

impl InstKil {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLd {
    pub opcode: u64,
}

impl InstLd {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 58) & 0x7)) as i32
    }

    pub fn cache_op(&self) -> CacheOpLd {
        CacheOpLd::from_u64((((self.opcode >> 56) & 0x3)) as u64)
    }

    pub fn ls_size(&self) -> LsSize {
        LsSize::from_u64((((self.opcode >> 53) & 0x7)) as u64)
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLdc {
    pub opcode: u64,
}

impl InstLdc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ls_size(&self) -> LsSize2 {
        LsSize2::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn address_mode(&self) -> AddressMode {
        AddressMode::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 36) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLdg {
    pub opcode: u64,
}

impl InstLdg {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ls_size(&self) -> LsSize {
        LsSize::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOpLd {
        CacheOpLd::from_u64((((self.opcode >> 46) & 0x3)) as u64)
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLdl {
    pub opcode: u64,
}

impl InstLdl {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ls_size(&self) -> LsSize2 {
        LsSize2::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOp2 {
        CacheOp2::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLds {
    pub opcode: u64,
}

impl InstLds {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ls_size(&self) -> LsSize2 {
        LsSize2::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn u(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLeaR {
    pub opcode: u64,
}

impl InstLeaR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn imm_u5(&self) -> i32 {
        (((self.opcode >> 39) & 0x1F)) as i32
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLeaI {
    pub opcode: u64,
}

impl InstLeaI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn imm_u5(&self) -> i32 {
        (((self.opcode >> 39) & 0x1F)) as i32
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLeaC {
    pub opcode: u64,
}

impl InstLeaC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn imm_u5(&self) -> i32 {
        (((self.opcode >> 39) & 0x1F)) as i32
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLeaHiR {
    pub opcode: u64,
}

impl InstLeaHiR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x4000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x2000000000) != 0
    }

    pub fn imm_u5(&self) -> i32 {
        (((self.opcode >> 28) & 0x1F)) as i32
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLeaHiC {
    pub opcode: u64,
}

impl InstLeaHiC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x200000000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn imm_u5(&self) -> i32 {
        (((self.opcode >> 51) & 0x1F)) as i32
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLepc {
    pub opcode: u64,
}

impl InstLepc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLongjmp {
    pub opcode: u64,
}

impl InstLongjmp {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLopR {
    pub opcode: u64,
}

impl InstLopR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

    pub fn predicate_op(&self) -> PredicateOp {
        PredicateOp::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn lop(&self) -> LogicOp {
        LogicOp::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLopI {
    pub opcode: u64,
}

impl InstLopI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

    pub fn predicate_op(&self) -> PredicateOp {
        PredicateOp::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn logic_op(&self) -> LogicOp {
        LogicOp::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLopC {
    pub opcode: u64,
}

impl InstLopC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

    pub fn predicate_op(&self) -> PredicateOp {
        PredicateOp::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn logic_op(&self) -> LogicOp {
        LogicOp::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLop3R {
    pub opcode: u64,
}

impl InstLop3R {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

    pub fn predicate_op(&self) -> PredicateOp {
        PredicateOp::from_u64((((self.opcode >> 36) & 0x3)) as u64)
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x4000000000) != 0
    }

    pub fn imm(&self) -> i32 {
        (((self.opcode >> 28) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLop3I {
    pub opcode: u64,
}

impl InstLop3I {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x200000000000000) != 0
    }

    pub fn imm(&self) -> i32 {
        (((self.opcode >> 48) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLop3C {
    pub opcode: u64,
}

impl InstLop3C {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn imm(&self) -> i32 {
        (((self.opcode >> 48) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstLop32i {
    pub opcode: u64,
}

impl InstLop32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x200000000000000) != 0
    }

    pub fn logic_op(&self) -> LogicOp {
        LogicOp::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstMembar {
    pub opcode: u64,
}

impl InstMembar {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn membar(&self) -> Membar {
        Membar::from_u64((((self.opcode >> 8) & 0x3)) as u64)
    }

    pub fn ivall(&self) -> Ivall {
        Ivall::from_u64(((self.opcode & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstMovR {
    pub opcode: u64,
}

impl InstMovR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn quad_mask(&self) -> i32 {
        (((self.opcode >> 39) & 0xF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstMovI {
    pub opcode: u64,
}

impl InstMovI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn quad_mask(&self) -> i32 {
        (((self.opcode >> 39) & 0xF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstMovC {
    pub opcode: u64,
}

impl InstMovC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn quad_mask(&self) -> i32 {
        (((self.opcode >> 39) & 0xF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstMov32i {
    pub opcode: u64,
}

impl InstMov32i {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn quad_mask(&self) -> i32 {
        (((self.opcode >> 12) & 0xF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstMufu {
    pub opcode: u64,
}

impl InstMufu {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn mufu_op(&self) -> MufuOp {
        MufuOp::from_u64((((self.opcode >> 20) & 0xF)) as u64)
    }

    pub fn abs_a(&self) -> bool {
        (self.opcode & 0x400000000000) != 0
    }

    pub fn neg_a(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstNop {
    pub opcode: u64,
}

impl InstNop {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm16(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

    pub fn trig(&self) -> bool {
        (self.opcode & 0x2000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64((((self.opcode >> 8) & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstOutR {
    pub opcode: u64,
}

impl InstOutR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn out_type(&self) -> OutType {
        OutType::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstOutI {
    pub opcode: u64,
}

impl InstOutI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn out_type(&self) -> OutType {
        OutType::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstOutC {
    pub opcode: u64,
}

impl InstOutC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn out_type(&self) -> OutType {
        OutType::from_u64((((self.opcode >> 39) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstP2rR {
    pub opcode: u64,
}

impl InstP2rR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn ccpr(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstP2rI {
    pub opcode: u64,
}

impl InstP2rI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn ccpr(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstP2rC {
    pub opcode: u64,
}

impl InstP2rC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn ccpr(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPbk {
    pub opcode: u64,
}

impl InstPbk {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPcnt {
    pub opcode: u64,
}

impl InstPcnt {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPexit {
    pub opcode: u64,
}

impl InstPexit {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPixld {
    pub opcode: u64,
}

impl InstPixld {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 45) & 0x7)) as i32
    }

    pub fn pix_mode(&self) -> PixMode {
        PixMode::from_u64((((self.opcode >> 31) & 0x7)) as u64)
    }

    pub fn imm8(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPlongjmp {
    pub opcode: u64,
}

impl InstPlongjmp {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPopcR {
    pub opcode: u64,
}

impl InstPopcR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPopcI {
    pub opcode: u64,
}

impl InstPopcI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPopcC {
    pub opcode: u64,
}

impl InstPopcC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPret {
    pub opcode: u64,
}

impl InstPret {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn inc(&self) -> bool {
        (self.opcode & 0x40) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPrmtR {
    pub opcode: u64,
}

impl InstPrmtR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn p_mode(&self) -> PMode {
        PMode::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPrmtI {
    pub opcode: u64,
}

impl InstPrmtI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn p_mode(&self) -> PMode {
        PMode::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPrmtC {
    pub opcode: u64,
}

impl InstPrmtC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn p_mode(&self) -> PMode {
        PMode::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPrmtRc {
    pub opcode: u64,
}

impl InstPrmtRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn p_mode(&self) -> PMode {
        PMode::from_u64((((self.opcode >> 48) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPset {
    pub opcode: u64,
}

impl InstPset {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn src2_pred(&self) -> i32 {
        (((self.opcode >> 12) & 0x7)) as i32
    }

    pub fn src2_pred_inv(&self) -> bool {
        (self.opcode & 0x8000) != 0
    }

    pub fn src1_pred(&self) -> i32 {
        (((self.opcode >> 29) & 0x7)) as i32
    }

    pub fn src1_pred_inv(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bool_op_ab(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn bool_op_c(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn b_val(&self) -> bool {
        (self.opcode & 0x100000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstPsetp {
    pub opcode: u64,
}

impl InstPsetp {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn src2_pred(&self) -> i32 {
        (((self.opcode >> 12) & 0x7)) as i32
    }

    pub fn src2_pred_inv(&self) -> bool {
        (self.opcode & 0x8000) != 0
    }

    pub fn src1_pred(&self) -> i32 {
        (((self.opcode >> 29) & 0x7)) as i32
    }

    pub fn src1_pred_inv(&self) -> bool {
        (self.opcode & 0x100000000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn bool_op_ab(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn bool_op_c(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstR2b {
    pub opcode: u64,
}

impl InstR2b {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn mode(&self) -> BarMode {
        BarMode::from_u64((((self.opcode >> 32) & 0x3)) as u64)
    }

    pub fn name(&self) -> i32 {
        (((self.opcode >> 28) & 0xF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstR2pR {
    pub opcode: u64,
}

impl InstR2pR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn ccpr(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstR2pI {
    pub opcode: u64,
}

impl InstR2pI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn ccpr(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstR2pC {
    pub opcode: u64,
}

impl InstR2pC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn byte_sel(&self) -> ByteSel {
        ByteSel::from_u64((((self.opcode >> 41) & 0x3)) as u64)
    }

    pub fn ccpr(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstRam {
    pub opcode: u64,
}

impl InstRam {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstRed {
    pub opcode: u64,
}

impl InstRed {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 28) & 0xFFFFF)) as i32
    }

    pub fn red_size(&self) -> AtomSize {
        AtomSize::from_u64((((self.opcode >> 20) & 0x7)) as u64)
    }

    pub fn red_op(&self) -> RedOp {
        RedOp::from_u64((((self.opcode >> 23) & 0x7)) as u64)
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstRet {
    pub opcode: u64,
}

impl InstRet {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstRroR {
    pub opcode: u64,
}

impl InstRroR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn rro_op(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstRroI {
    pub opcode: u64,
}

impl InstRroI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn rro_op(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstRroC {
    pub opcode: u64,
}

impl InstRroC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn abs_b(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn neg_b(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn rro_op(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstRtt {
    pub opcode: u64,
}

impl InstRtt {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstS2r {
    pub opcode: u64,
}

impl InstS2r {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn s_reg(&self) -> SReg {
        SReg::from_u64((((self.opcode >> 20) & 0xFF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSam {
    pub opcode: u64,
}

impl InstSam {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSelR {
    pub opcode: u64,
}

impl InstSelR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSelI {
    pub opcode: u64,
}

impl InstSelI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSelC {
    pub opcode: u64,
}

impl InstSelC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSetcrsptr {
    pub opcode: u64,
}

impl InstSetcrsptr {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSetlmembase {
    pub opcode: u64,
}

impl InstSetlmembase {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShfLR {
    pub opcode: u64,
}

impl InstShfLR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x_mode_shf(&self) -> XModeShf {
        XModeShf::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn max_shift(&self) -> MaxShift {
        MaxShift::from_u64((((self.opcode >> 37) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShfRR {
    pub opcode: u64,
}

impl InstShfRR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x_mode_shf(&self) -> XModeShf {
        XModeShf::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn max_shift(&self) -> MaxShift {
        MaxShift::from_u64((((self.opcode >> 37) & 0x3)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShfLI {
    pub opcode: u64,
}

impl InstShfLI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x_mode_shf(&self) -> XModeShf {
        XModeShf::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn max_shift(&self) -> MaxShift {
        MaxShift::from_u64((((self.opcode >> 37) & 0x3)) as u64)
    }

    pub fn imm6(&self) -> i32 {
        (((self.opcode >> 20) & 0x3F)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShfRI {
    pub opcode: u64,
}

impl InstShfRI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn x_mode_shf(&self) -> XModeShf {
        XModeShf::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn max_shift(&self) -> MaxShift {
        MaxShift::from_u64((((self.opcode >> 37) & 0x3)) as u64)
    }

    pub fn imm6(&self) -> i32 {
        (((self.opcode >> 20) & 0x3F)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShfl {
    pub opcode: u64,
}

impl InstShfl {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn src_b_imm(&self) -> i32 {
        (((self.opcode >> 20) & 0x1F)) as i32
    }

    pub fn src_c_imm(&self) -> i32 {
        (((self.opcode >> 34) & 0x1FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn shfl_mode(&self) -> ShflMode {
        ShflMode::from_u64((((self.opcode >> 30) & 0x3)) as u64)
    }

    pub fn c_fix_shfl(&self) -> bool {
        (self.opcode & 0x20000000) != 0
    }

    pub fn b_fix_shfl(&self) -> bool {
        (self.opcode & 0x10000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 48) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShlR {
    pub opcode: u64,
}

impl InstShlR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShlI {
    pub opcode: u64,
}

impl InstShlI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShlC {
    pub opcode: u64,
}

impl InstShlC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x80000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShrR {
    pub opcode: u64,
}

impl InstShrR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x_mode(&self) -> XMode {
        XMode::from_u64((((self.opcode >> 43) & 0x3)) as u64)
    }

    pub fn brev(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShrI {
    pub opcode: u64,
}

impl InstShrI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn imm20(&self) -> i32 {
        (((self.opcode >> 37) & 0x80000) | ((self.opcode >> 20) & 0x7FFFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x_mode(&self) -> XMode {
        XMode::from_u64((((self.opcode >> 43) & 0x3)) as u64)
    }

    pub fn brev(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstShrC {
    pub opcode: u64,
}

impl InstShrC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x_mode(&self) -> XMode {
        XMode::from_u64((((self.opcode >> 43) & 0x3)) as u64)
    }

    pub fn brev(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn m(&self) -> bool {
        (self.opcode & 0x8000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSsy {
    pub opcode: u64,
}

impl InstSsy {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

    pub fn ca(&self) -> bool {
        (self.opcode & 0x20) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSt {
    pub opcode: u64,
}

impl InstSt {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 58) & 0x7)) as i32
    }

    pub fn cache_op(&self) -> CacheOpSt {
        CacheOpSt::from_u64((((self.opcode >> 56) & 0x3)) as u64)
    }

    pub fn ls_size(&self) -> LsSize {
        LsSize::from_u64((((self.opcode >> 53) & 0x7)) as u64)
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn imm32(&self) -> i32 {
        ((self.opcode >> 20)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstStg {
    pub opcode: u64,
}

impl InstStg {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ls_size(&self) -> LsSize2 {
        LsSize2::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOpSt {
        CacheOpSt::from_u64((((self.opcode >> 46) & 0x3)) as u64)
    }

    pub fn e(&self) -> bool {
        (self.opcode & 0x200000000000) != 0
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstStl {
    pub opcode: u64,
}

impl InstStl {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ls_size(&self) -> LsSize2 {
        LsSize2::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOpSt {
        CacheOpSt::from_u64((((self.opcode >> 44) & 0x3)) as u64)
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstStp {
    pub opcode: u64,
}

impl InstStp {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn wait(&self) -> bool {
        (self.opcode & 0x80000000) != 0
    }

    pub fn imm8(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSts {
    pub opcode: u64,
}

impl InstSts {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ls_size(&self) -> LsSize2 {
        LsSize2::from_u64((((self.opcode >> 48) & 0x7)) as u64)
    }

    pub fn imm24(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuatomB {
    pub opcode: u64,
}

impl InstSuatomB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn size(&self) -> SuatomSize {
        SuatomSize::from_u64((((self.opcode >> 36) & 0x7)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn op(&self) -> SuatomOp {
        SuatomOp::from_u64((((self.opcode >> 29) & 0xF)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x10000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuatom {
    pub opcode: u64,
}

impl InstSuatom {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn size(&self) -> SuatomSize {
        SuatomSize::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn op(&self) -> SuatomOp {
        SuatomOp::from_u64((((self.opcode >> 29) & 0xF)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x10000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuatomB2 {
    pub opcode: u64,
}

impl InstSuatomB2 {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn size(&self) -> SuatomSize {
        SuatomSize::from_u64((((self.opcode >> 36) & 0x7)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn op(&self) -> SuatomOp {
        SuatomOp::from_u64((((self.opcode >> 29) & 0xF)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x10000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuatomCasB {
    pub opcode: u64,
}

impl InstSuatomCasB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn size(&self) -> SuatomSize {
        SuatomSize::from_u64((((self.opcode >> 36) & 0x7)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 30) & 0x7)) as i32
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x10000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuatomCas {
    pub opcode: u64,
}

impl InstSuatomCas {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn size(&self) -> SuatomSize {
        SuatomSize::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 30) & 0x7)) as i32
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x10000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuldDB {
    pub opcode: u64,
}

impl InstSuldDB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn dest_pred2(&self) -> i32 {
        (((self.opcode >> 30) & 0x7)) as i32
    }

    pub fn cache_op(&self) -> CacheOpLd {
        CacheOpLd::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x800000) != 0
    }

    pub fn size(&self) -> SuSize {
        SuSize::from_u64((((self.opcode >> 20) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuldD {
    pub opcode: u64,
}

impl InstSuldD {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn dest_pred2(&self) -> i32 {
        (((self.opcode >> 30) & 0x7)) as i32
    }

    pub fn cache_op(&self) -> CacheOpLd {
        CacheOpLd::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x800000) != 0
    }

    pub fn size(&self) -> SuSize {
        SuSize::from_u64((((self.opcode >> 20) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuldB {
    pub opcode: u64,
}

impl InstSuldB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn dest_pred2(&self) -> i32 {
        (((self.opcode >> 30) & 0x7)) as i32
    }

    pub fn cache_op(&self) -> CacheOpLd {
        CacheOpLd::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn rgba(&self) -> SuRgba {
        SuRgba::from_u64((((self.opcode >> 20) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuld {
    pub opcode: u64,
}

impl InstSuld {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn dest_pred2(&self) -> i32 {
        (((self.opcode >> 30) & 0x7)) as i32
    }

    pub fn cache_op(&self) -> CacheOpLd {
        CacheOpLd::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn rgba(&self) -> SuRgba {
        SuRgba::from_u64((((self.opcode >> 20) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSuredB {
    pub opcode: u64,
}

impl InstSuredB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn op(&self) -> RedOp {
        RedOp::from_u64((((self.opcode >> 24) & 0x7)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x800000) != 0
    }

    pub fn size(&self) -> SuatomSize {
        SuatomSize::from_u64((((self.opcode >> 20) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSured {
    pub opcode: u64,
}

impl InstSured {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn op(&self) -> RedOp {
        RedOp::from_u64((((self.opcode >> 24) & 0x7)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x800000) != 0
    }

    pub fn size(&self) -> SuatomSize {
        SuatomSize::from_u64((((self.opcode >> 20) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSustDB {
    pub opcode: u64,
}

impl InstSustDB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOpSt {
        CacheOpSt::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x800000) != 0
    }

    pub fn size(&self) -> SuSize {
        SuSize::from_u64((((self.opcode >> 20) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSustD {
    pub opcode: u64,
}

impl InstSustD {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOpSt {
        CacheOpSt::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn ba(&self) -> bool {
        (self.opcode & 0x800000) != 0
    }

    pub fn size(&self) -> SuSize {
        SuSize::from_u64((((self.opcode >> 20) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSustB {
    pub opcode: u64,
}

impl InstSustB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOpSt {
        CacheOpSt::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn rgba(&self) -> SuRgba {
        SuRgba::from_u64((((self.opcode >> 20) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSust {
    pub opcode: u64,
}

impl InstSust {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn clamp(&self) -> Clamp {
        Clamp::from_u64((((self.opcode >> 49) & 0x3)) as u64)
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn dim(&self) -> SuDim {
        SuDim::from_u64((((self.opcode >> 33) & 0x7)) as u64)
    }

    pub fn cache_op(&self) -> CacheOpSt {
        CacheOpSt::from_u64((((self.opcode >> 24) & 0x3)) as u64)
    }

    pub fn rgba(&self) -> SuRgba {
        SuRgba::from_u64((((self.opcode >> 20) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstSync {
    pub opcode: u64,
}

impl InstSync {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn ccc(&self) -> Ccc {
        Ccc::from_u64(((self.opcode & 0x1F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTex {
    pub opcode: u64,
}

impl InstTex {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn lc(&self) -> bool {
        (self.opcode & 0x400000000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn lod(&self) -> Lod {
        Lod::from_u64((((self.opcode >> 55) & 0x7)) as u64)
    }

    pub fn aoffi(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn dc(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTexB {
    pub opcode: u64,
}

impl InstTexB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn lcb(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn lodb(&self) -> Lod {
        Lod::from_u64((((self.opcode >> 37) & 0x7)) as u64)
    }

    pub fn aoffib(&self) -> bool {
        (self.opcode & 0x1000000000) != 0
    }

    pub fn dc(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTexs {
    pub opcode: u64,
}

impl InstTexs {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn target(&self) -> TexsTarget {
        TexsTarget::from_u64((((self.opcode >> 53) & 0xF)) as u64)
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 50) & 0x7)) as i32
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn dest2(&self) -> i32 {
        (((self.opcode >> 28) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTld {
    pub opcode: u64,
}

impl InstTld {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn lod(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn toff(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn ms(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn cl(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTldB {
    pub opcode: u64,
}

impl InstTldB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn lod(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn toff(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn ms(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn cl(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTlds {
    pub opcode: u64,
}

impl InstTlds {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn target(&self) -> TldsTarget {
        TldsTarget::from_u64((((self.opcode >> 53) & 0xF)) as u64)
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 50) & 0x7)) as i32
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn dest2(&self) -> i32 {
        (((self.opcode >> 28) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTld4 {
    pub opcode: u64,
}

impl InstTld4 {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn lc(&self) -> bool {
        (self.opcode & 0x400000000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn tex_comp(&self) -> TexComp {
        TexComp::from_u64((((self.opcode >> 56) & 0x3)) as u64)
    }

    pub fn toff(&self) -> TexOffset {
        TexOffset::from_u64((((self.opcode >> 54) & 0x3)) as u64)
    }

    pub fn dc(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTld4B {
    pub opcode: u64,
}

impl InstTld4B {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn lc(&self) -> bool {
        (self.opcode & 0x10000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn tex_comp(&self) -> TexComp {
        TexComp::from_u64((((self.opcode >> 38) & 0x3)) as u64)
    }

    pub fn toff(&self) -> TexOffset {
        TexOffset::from_u64((((self.opcode >> 36) & 0x3)) as u64)
    }

    pub fn dc(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTld4s {
    pub opcode: u64,
}

impl InstTld4s {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn tex_comp(&self) -> TexComp {
        TexComp::from_u64((((self.opcode >> 52) & 0x3)) as u64)
    }

    pub fn aoffi(&self) -> bool {
        (self.opcode & 0x8000000000000) != 0
    }

    pub fn dc(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn dest2(&self) -> i32 {
        (((self.opcode >> 28) & 0xFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTmml {
    pub opcode: u64,
}

impl InstTmml {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTmmlB {
    pub opcode: u64,
}

impl InstTmmlB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTxa {
    pub opcode: u64,
}

impl InstTxa {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn ndv(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTxd {
    pub opcode: u64,
}

impl InstTxd {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn lc(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn toff(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTxdB {
    pub opcode: u64,
}

impl InstTxdB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 51) & 0x7)) as i32
    }

    pub fn lc(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn toff(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn dim(&self) -> TexDim {
        TexDim::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTxq {
    pub opcode: u64,
}

impl InstTxq {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn tid_b(&self) -> i32 {
        (((self.opcode >> 36) & 0x1FFF)) as i32
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn tex_query(&self) -> TexQuery {
        TexQuery::from_u64((((self.opcode >> 22) & 0x3F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstTxqB {
    pub opcode: u64,
}

impl InstTxqB {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn nodep(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn w_mask(&self) -> i32 {
        (((self.opcode >> 31) & 0xF)) as i32
    }

    pub fn tex_query(&self) -> TexQuery {
        TexQuery::from_u64((((self.opcode >> 22) & 0x3F)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVabsdiff {
    pub opcode: u64,
}

impl InstVabsdiff {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn d_format(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 46) & 0x8) | ((self.opcode >> 28) & 0x7))) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn video_op(&self) -> VideoOp {
        VideoOp::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVabsdiff4 {
    pub opcode: u64,
}

impl InstVabsdiff4 {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn v_red(&self) -> VideoRed {
        VideoRed::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn lane_mask4(&self) -> LaneMask4 {
        LaneMask4::from_u64(((((self.opcode >> 49) & 0xC) | ((self.opcode >> 36) & 0x3))) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

    pub fn src_b_fmt(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn src_a_fmt(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn d_format(&self) -> bool {
        (self.opcode & 0x4000000000) != 0
    }

    pub fn asel4(&self) -> ASelect4 {
        ASelect4::from_u64((((self.opcode >> 32) & 0xF)) as u64)
    }

    pub fn bsel4(&self) -> BSelect4 {
        BSelect4::from_u64((((self.opcode >> 28) & 0xF)) as u64)
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVadd {
    pub opcode: u64,
}

impl InstVadd {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm16(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 56) & 0x3)) as u64)
    }

    pub fn d_format(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 46) & 0x8) | ((self.opcode >> 28) & 0x7))) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn video_op(&self) -> VideoOp {
        VideoOp::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVmad {
    pub opcode: u64,
}

impl InstVmad {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm16(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 46) & 0x8) | ((self.opcode >> 28) & 0x7))) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn avg_mode(&self) -> AvgMode {
        AvgMode::from_u64((((self.opcode >> 53) & 0x3)) as u64)
    }

    pub fn video_scale(&self) -> VideoScale {
        VideoScale::from_u64((((self.opcode >> 51) & 0x3)) as u64)
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVmnmx {
    pub opcode: u64,
}

impl InstVmnmx {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm16(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn d_format(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 46) & 0x8) | ((self.opcode >> 28) & 0x7))) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn video_op(&self) -> VideoOp {
        VideoOp::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn mn(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVote {
    pub opcode: u64,
}

impl InstVote {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn vote_mode(&self) -> VoteMode {
        VoteMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn vp_dest(&self) -> i32 {
        (((self.opcode >> 45) & 0x7)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVotevtg {
    pub opcode: u64,
}

impl InstVotevtg {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn vote_mode(&self) -> VoteMode {
        VoteMode::from_u64((((self.opcode >> 48) & 0x3)) as u64)
    }

    pub fn imm28(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFFFFF)) as i32
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVset {
    pub opcode: u64,
}

impl InstVset {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn v_comp(&self) -> IComp {
        IComp::from_u64((((self.opcode >> 54) & 0x7)) as u64)
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 46) & 0x8) | ((self.opcode >> 28) & 0x7))) as u64)
    }

    pub fn video_op(&self) -> VideoOp {
        VideoOp::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVsetp {
    pub opcode: u64,
}

impl InstVsetp {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn imm16(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 46) & 0x8) | ((self.opcode >> 28) & 0x7))) as u64)
    }

    pub fn v_comp(&self) -> IComp {
        IComp::from_u64(((((self.opcode >> 45) & 0x4) | ((self.opcode >> 43) & 0x3))) as u64)
    }

    pub fn bool_op(&self) -> BoolOp {
        BoolOp::from_u64((((self.opcode >> 45) & 0x3)) as u64)
    }

    pub fn src_pred(&self) -> i32 {
        (((self.opcode >> 39) & 0x7)) as i32
    }

    pub fn src_pred_inv(&self) -> bool {
        (self.opcode & 0x40000000000) != 0
    }

    pub fn dest_pred(&self) -> i32 {
        (((self.opcode >> 3) & 0x7)) as i32
    }

    pub fn dest_pred_inv(&self) -> i32 {
        ((self.opcode & 0x7)) as i32
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVshl {
    pub opcode: u64,
}

impl InstVshl {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn mv(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn d_format(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn video_op(&self) -> VideoOp {
        VideoOp::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstVshr {
    pub opcode: u64,
}

impl InstVshr {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn mv(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn d_format(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn a_select(&self) -> VectorSelect {
        VectorSelect::from_u64(((((self.opcode >> 45) & 0x8) | ((self.opcode >> 36) & 0x7))) as u64)
    }

    pub fn b_select(&self) -> VectorSelect {
        VectorSelect::from_u64((((self.opcode >> 28) & 0x7)) as u64)
    }

    pub fn sat(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn video_op(&self) -> VideoOp {
        VideoOp::from_u64((((self.opcode >> 51) & 0x7)) as u64)
    }

    pub fn b_video(&self) -> bool {
        (self.opcode & 0x4000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstXmadR {
    pub opcode: u64,
}

impl InstXmadR {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_b(&self) -> i32 {
        (((self.opcode >> 20) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn hilo_a(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn xmad_cop(&self) -> XmadCop {
        XmadCop::from_u64((((self.opcode >> 50) & 0x7)) as u64)
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x4000000000) != 0
    }

    pub fn mrg(&self) -> bool {
        (self.opcode & 0x2000000000) != 0
    }

    pub fn psl(&self) -> bool {
        (self.opcode & 0x1000000000) != 0
    }

    pub fn hilo_b(&self) -> bool {
        (self.opcode & 0x800000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstXmadI {
    pub opcode: u64,
}

impl InstXmadI {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn imm16(&self) -> i32 {
        (((self.opcode >> 20) & 0xFFFF)) as i32
    }

    pub fn hilo_a(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn xmad_cop(&self) -> XmadCop {
        XmadCop::from_u64((((self.opcode >> 50) & 0x7)) as u64)
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x4000000000) != 0
    }

    pub fn mrg(&self) -> bool {
        (self.opcode & 0x2000000000) != 0
    }

    pub fn psl(&self) -> bool {
        (self.opcode & 0x1000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstXmadC {
    pub opcode: u64,
}

impl InstXmadC {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn mrg(&self) -> bool {
        (self.opcode & 0x100000000000000) != 0
    }

    pub fn psl(&self) -> bool {
        (self.opcode & 0x80000000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn hilo_a(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn hilo_b(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn xmad_cop(&self) -> XmadCop2 {
        XmadCop2::from_u64((((self.opcode >> 50) & 0x3)) as u64)
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstXmadRc {
    pub opcode: u64,
}

impl InstXmadRc {
    pub const fn new(opcode: u64) -> Self {
        Self { opcode }
    }

    pub fn dest(&self) -> i32 {
        ((self.opcode & 0xFF)) as i32
    }

    pub fn src_a(&self) -> i32 {
        (((self.opcode >> 8) & 0xFF)) as i32
    }

    pub fn src_c(&self) -> i32 {
        (((self.opcode >> 39) & 0xFF)) as i32
    }

    pub fn cbuf_slot(&self) -> i32 {
        (((self.opcode >> 34) & 0x1F)) as i32
    }

    pub fn cbuf_offset(&self) -> i32 {
        (((self.opcode >> 20) & 0x3FFF)) as i32
    }

    pub fn pred(&self) -> i32 {
        (((self.opcode >> 16) & 0x7)) as i32
    }

    pub fn pred_inv(&self) -> bool {
        (self.opcode & 0x80000) != 0
    }

    pub fn write_cc(&self) -> bool {
        (self.opcode & 0x800000000000) != 0
    }

    pub fn x(&self) -> bool {
        (self.opcode & 0x40000000000000) != 0
    }

    pub fn hilo_a(&self) -> bool {
        (self.opcode & 0x20000000000000) != 0
    }

    pub fn hilo_b(&self) -> bool {
        (self.opcode & 0x10000000000000) != 0
    }

    pub fn xmad_cop(&self) -> XmadCop2 {
        XmadCop2::from_u64((((self.opcode >> 50) & 0x3)) as u64)
    }

    pub fn b_signed(&self) -> bool {
        (self.opcode & 0x2000000000000) != 0
    }

    pub fn a_signed(&self) -> bool {
        (self.opcode & 0x1000000000000) != 0
    }

}

