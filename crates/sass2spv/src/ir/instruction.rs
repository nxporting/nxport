#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Instruction {
    Absolute = 1,
    Add,
    AtomicAdd,
    AtomicAnd,
    AtomicCompareAndSwap,
    AtomicMinS32,
    AtomicMinU32,
    AtomicMaxS32,
    AtomicMaxU32,
    AtomicOr,
    AtomicSwap,
    AtomicXor,
    Ballot,
    Barrier,
    BitCount,
    BitfieldExtractS32,
    BitfieldExtractU32,
    BitfieldInsert,
    BitfieldReverse,
    BitwiseAnd,
    BitwiseExclusiveOr,
    BitwiseNot,
    BitwiseOr,
    Branch,
    BranchIfFalse,
    BranchIfTrue,
    Call,
    Ceiling,
    Clamp,
    ClampU32,
    Comment,
    CompareEqual,
    CompareGreater,
    CompareGreaterOrEqual,
    CompareGreaterOrEqualU32,
    CompareGreaterU32,
    CompareLess,
    CompareLessOrEqual,
    CompareLessOrEqualU32,
    CompareLessU32,
    CompareNotEqual,
    ConditionalSelect,
    ConvertFP32ToFP64,
    ConvertFP64ToFP32,
    ConvertFP32ToS32,
    ConvertFP32ToU32,
    ConvertFP64ToS32,
    ConvertFP64ToU32,
    ConvertS32ToFP32,
    ConvertS32ToFP64,
    ConvertU32ToFP32,
    ConvertU32ToFP64,
    Copy,
    Cosine,
    Ddx,
    Ddy,
    Discard,
    Divide,
    EmitVertex,
    EndPrimitive,
    ExponentB2,
    FSIBegin,
    FSIEnd,
    FindLSB,
    FindMSBS32,
    FindMSBU32,
    Floor,
    FusedMultiplyAdd,
    GroupMemoryBarrier,
    ImageLoad,
    ImageStore,
    ImageAtomic,
    IsNan,
    Load,
    Lod,
    LogarithmB2,
    LogicalAnd,
    LogicalExclusiveOr,
    LogicalNot,
    LogicalOr,
    LoopBreak,
    LoopContinue,
    MarkLabel,
    Maximum,
    MaximumU32,
    MemoryBarrier,
    Minimum,
    MinimumU32,
    Modulo,
    Multiply,
    MultiplyHighS32,
    MultiplyHighU32,
    Negate,
    PackDouble2x32,
    PackHalf2x16,
    ReciprocalSquareRoot,
    Return,
    Round,
    ShiftLeft,
    ShiftRightS32,
    ShiftRightU32,
    Shuffle,
    ShuffleDown,
    ShuffleUp,
    ShuffleXor,
    Sine,
    SquareRoot,
    Store,
    Subtract,
    SwizzleAdd,
    TextureSample,
    TextureQuerySamples,
    TextureQuerySize,
    Truncate,
    UnpackDouble2x32,
    UnpackHalf2x16,
    VectorExtract,
    VoteAll,
    VoteAllEqual,
    VoteAny,
}

pub const FP32: u32 = 1 << 16;
pub const FP64: u32 = 1 << 17;
pub const INST_MASK: u32 = 0xffff;

impl Instruction {
    pub fn is_atomic(&self) -> bool {
        matches!(
            self,
            Self::AtomicAdd
                | Self::AtomicAnd
                | Self::AtomicCompareAndSwap
                | Self::AtomicMaxS32
                | Self::AtomicMaxU32
                | Self::AtomicMinS32
                | Self::AtomicMinU32
                | Self::AtomicOr
                | Self::AtomicSwap
                | Self::AtomicXor
        )
    }

    pub fn is_comparison(&self) -> bool {
        matches!(
            self,
            Self::CompareEqual
                | Self::CompareGreater
                | Self::CompareGreaterOrEqual
                | Self::CompareGreaterOrEqualU32
                | Self::CompareGreaterU32
                | Self::CompareLess
                | Self::CompareLessOrEqual
                | Self::CompareLessOrEqualU32
                | Self::CompareLessU32
                | Self::CompareNotEqual
        )
    }

    pub fn is_texture_query(&self) -> bool {
        matches!(
            self,
            Self::Lod | Self::TextureQuerySamples | Self::TextureQuerySize
        )
    }

    pub fn is_image(&self) -> bool {
        matches!(self, Self::ImageAtomic | Self::ImageLoad | Self::ImageStore)
    }

    pub fn is_image_store(&self) -> bool {
        matches!(self, Self::ImageAtomic | Self::ImageStore)
    }

    pub fn is_subgroup_op(&self) -> bool {
        matches!(
            self,
            Self::Ballot
                | Self::Shuffle
                | Self::ShuffleDown
                | Self::ShuffleUp
                | Self::ShuffleXor
                | Self::VoteAll
                | Self::VoteAllEqual
                | Self::VoteAny
        )
    }
}
