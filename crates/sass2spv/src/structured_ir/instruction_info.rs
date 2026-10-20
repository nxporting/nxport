use crate::ir::instruction::Instruction;
use crate::ir::storage_kind::AggregateType;

pub struct InstructionInfo;

impl InstructionInfo {
    pub fn get_dest_var_type(inst: Instruction) -> AggregateType {
        match inst {
            Instruction::AtomicAdd
            | Instruction::AtomicAnd
            | Instruction::AtomicCompareAndSwap
            | Instruction::AtomicMaxU32
            | Instruction::AtomicMinU32
            | Instruction::AtomicOr
            | Instruction::AtomicSwap
            | Instruction::AtomicXor
            | Instruction::Ballot
            | Instruction::BitfieldExtractU32
            | Instruction::ClampU32
            | Instruction::ConvertFP32ToU32
            | Instruction::ConvertFP64ToU32 => AggregateType::U32,

            Instruction::AtomicMaxS32
            | Instruction::AtomicMinS32
            | Instruction::BitCount
            | Instruction::BitfieldExtractS32
            | Instruction::BitfieldInsert
            | Instruction::BitfieldReverse
            | Instruction::BitwiseAnd
            | Instruction::BitwiseExclusiveOr
            | Instruction::BitwiseNot
            | Instruction::BitwiseOr
            | Instruction::ConvertFP32ToS32
            | Instruction::ConvertFP64ToS32
            | Instruction::FindLSB
            | Instruction::FindMSBS32
            | Instruction::FindMSBU32
            | Instruction::ImageAtomic => AggregateType::S32,

            Instruction::CompareEqual
            | Instruction::CompareGreater
            | Instruction::CompareGreaterOrEqual
            | Instruction::CompareGreaterOrEqualU32
            | Instruction::CompareGreaterU32
            | Instruction::CompareLess
            | Instruction::CompareLessOrEqual
            | Instruction::CompareLessOrEqualU32
            | Instruction::CompareLessU32
            | Instruction::CompareNotEqual
            | Instruction::IsNan
            | Instruction::LogicalAnd
            | Instruction::LogicalExclusiveOr
            | Instruction::LogicalNot
            | Instruction::LogicalOr
            | Instruction::VoteAll
            | Instruction::VoteAllEqual
            | Instruction::VoteAny => AggregateType::BOOL,

            Instruction::ConvertFP32ToFP64
            | Instruction::ConvertS32ToFP64
            | Instruction::ConvertU32ToFP64 => AggregateType::FP64,

            Instruction::ConvertFP64ToFP32
            | Instruction::ConvertS32ToFP32
            | Instruction::ConvertU32ToFP32
            | Instruction::Ddx
            | Instruction::Ddy
            | Instruction::ImageLoad
            | Instruction::Load
            | Instruction::Lod => AggregateType::FP32,

            Instruction::BranchIfTrue
            | Instruction::BranchIfFalse
            | Instruction::ImageStore
            | Instruction::Discard
            | Instruction::Return => AggregateType::VOID,

            _ => AggregateType::SCALAR,
        }
    }

    pub fn get_src_var_type(inst: Instruction, index: usize) -> AggregateType {
        match inst {
            Instruction::BranchIfTrue | Instruction::BranchIfFalse => AggregateType::BOOL,
            Instruction::LogicalAnd
            | Instruction::LogicalExclusiveOr
            | Instruction::LogicalNot
            | Instruction::LogicalOr => AggregateType::BOOL,
            Instruction::ConvertFP32ToFP64
            | Instruction::ConvertFP32ToS32
            | Instruction::ConvertFP32ToU32 => AggregateType::FP32,
            Instruction::ConvertFP64ToFP32
            | Instruction::ConvertFP64ToS32
            | Instruction::ConvertFP64ToU32 => AggregateType::FP64,
            Instruction::ConvertS32ToFP32 | Instruction::ConvertS32ToFP64 => AggregateType::S32,
            Instruction::ConvertU32ToFP32 | Instruction::ConvertU32ToFP64 => AggregateType::U32,
            Instruction::ConditionalSelect => {
                if index == 0 {
                    AggregateType::BOOL
                } else {
                    AggregateType::SCALAR
                }
            }
            Instruction::CompareGreaterOrEqualU32
            | Instruction::CompareGreaterU32
            | Instruction::CompareLessOrEqualU32
            | Instruction::CompareLessU32 => AggregateType::U32,
            _ => AggregateType::SCALAR,
        }
    }

    pub fn is_unary(inst: Instruction) -> bool {
        matches!(
            inst,
            Instruction::Absolute
                | Instruction::BitCount
                | Instruction::BitfieldReverse
                | Instruction::BitwiseNot
                | Instruction::Ceiling
                | Instruction::ConvertFP32ToFP64
                | Instruction::ConvertFP32ToS32
                | Instruction::ConvertFP32ToU32
                | Instruction::ConvertFP64ToFP32
                | Instruction::ConvertFP64ToS32
                | Instruction::ConvertFP64ToU32
                | Instruction::ConvertS32ToFP32
                | Instruction::ConvertS32ToFP64
                | Instruction::ConvertU32ToFP32
                | Instruction::ConvertU32ToFP64
                | Instruction::Copy
                | Instruction::Cosine
                | Instruction::Ddx
                | Instruction::Ddy
                | Instruction::ExponentB2
                | Instruction::FindLSB
                | Instruction::FindMSBS32
                | Instruction::FindMSBU32
                | Instruction::Floor
                | Instruction::IsNan
                | Instruction::LogarithmB2
                | Instruction::LogicalNot
                | Instruction::Negate
                | Instruction::ReciprocalSquareRoot
                | Instruction::Round
                | Instruction::Sine
                | Instruction::SquareRoot
                | Instruction::Truncate
        )
    }
}
