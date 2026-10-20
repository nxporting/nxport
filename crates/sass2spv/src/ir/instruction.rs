// IR instructions: an operation in the low 16 bits, the FP32 / FP64 type flags above it.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Instruction(pub u32);

#[rustfmt::skip]
impl Instruction {
    pub const ABSOLUTE: Self = Self(1);
    pub const ADD: Self = Self(2);
    pub const ATOMIC_ADD: Self = Self(3);
    pub const ATOMIC_AND: Self = Self(4);
    pub const ATOMIC_COMPARE_AND_SWAP: Self = Self(5);
    pub const ATOMIC_MIN_S32: Self = Self(6);
    pub const ATOMIC_MIN_U32: Self = Self(7);
    pub const ATOMIC_MAX_S32: Self = Self(8);
    pub const ATOMIC_MAX_U32: Self = Self(9);
    pub const ATOMIC_OR: Self = Self(10);
    pub const ATOMIC_SWAP: Self = Self(11);
    pub const ATOMIC_XOR: Self = Self(12);
    pub const BALLOT: Self = Self(13);
    pub const BARRIER: Self = Self(14);
    pub const BIT_COUNT: Self = Self(15);
    pub const BITFIELD_EXTRACT_S32: Self = Self(16);
    pub const BITFIELD_EXTRACT_U32: Self = Self(17);
    pub const BITFIELD_INSERT: Self = Self(18);
    pub const BITFIELD_REVERSE: Self = Self(19);
    pub const BITWISE_AND: Self = Self(20);
    pub const BITWISE_EXCLUSIVE_OR: Self = Self(21);
    pub const BITWISE_NOT: Self = Self(22);
    pub const BITWISE_OR: Self = Self(23);
    pub const BRANCH: Self = Self(24);
    pub const BRANCH_IF_FALSE: Self = Self(25);
    pub const BRANCH_IF_TRUE: Self = Self(26);
    pub const CALL: Self = Self(27);
    pub const CEILING: Self = Self(28);
    pub const CLAMP: Self = Self(29);
    pub const CLAMP_U32: Self = Self(30);
    pub const COMMENT: Self = Self(31);
    pub const COMPARE_EQUAL: Self = Self(32);
    pub const COMPARE_GREATER: Self = Self(33);
    pub const COMPARE_GREATER_OR_EQUAL: Self = Self(34);
    pub const COMPARE_GREATER_OR_EQUAL_U32: Self = Self(35);
    pub const COMPARE_GREATER_U32: Self = Self(36);
    pub const COMPARE_LESS: Self = Self(37);
    pub const COMPARE_LESS_OR_EQUAL: Self = Self(38);
    pub const COMPARE_LESS_OR_EQUAL_U32: Self = Self(39);
    pub const COMPARE_LESS_U32: Self = Self(40);
    pub const COMPARE_NOT_EQUAL: Self = Self(41);
    pub const CONDITIONAL_SELECT: Self = Self(42);
    pub const CONVERT_FP32_TO_FP64: Self = Self(43);
    pub const CONVERT_FP64_TO_FP32: Self = Self(44);
    pub const CONVERT_FP32_TO_S32: Self = Self(45);
    pub const CONVERT_FP32_TO_U32: Self = Self(46);
    pub const CONVERT_FP64_TO_S32: Self = Self(47);
    pub const CONVERT_FP64_TO_U32: Self = Self(48);
    pub const CONVERT_S32_TO_FP32: Self = Self(49);
    pub const CONVERT_S32_TO_FP64: Self = Self(50);
    pub const CONVERT_U32_TO_FP32: Self = Self(51);
    pub const CONVERT_U32_TO_FP64: Self = Self(52);
    pub const COPY: Self = Self(53);
    pub const COSINE: Self = Self(54);
    pub const DDX: Self = Self(55);
    pub const DDY: Self = Self(56);
    pub const DISCARD: Self = Self(57);
    pub const DIVIDE: Self = Self(58);
    pub const EMIT_VERTEX: Self = Self(59);
    pub const END_PRIMITIVE: Self = Self(60);
    pub const EXPONENT_B2: Self = Self(61);
    pub const FSI_BEGIN: Self = Self(62);
    pub const FSI_END: Self = Self(63);
    pub const FIND_LSB: Self = Self(64);
    pub const FIND_MSB_S32: Self = Self(65);
    pub const FIND_MSB_U32: Self = Self(66);
    pub const FLOOR: Self = Self(67);
    pub const FUSED_MULTIPLY_ADD: Self = Self(68);
    pub const GROUP_MEMORY_BARRIER: Self = Self(69);
    pub const IMAGE_LOAD: Self = Self(70);
    pub const IMAGE_STORE: Self = Self(71);
    pub const IMAGE_ATOMIC: Self = Self(72);
    pub const IS_NAN: Self = Self(73);
    pub const LOAD: Self = Self(74);
    pub const LOD: Self = Self(75);
    pub const LOGARITHM_B2: Self = Self(76);
    pub const LOGICAL_AND: Self = Self(77);
    pub const LOGICAL_EXCLUSIVE_OR: Self = Self(78);
    pub const LOGICAL_NOT: Self = Self(79);
    pub const LOGICAL_OR: Self = Self(80);
    pub const LOOP_BREAK: Self = Self(81);
    pub const LOOP_CONTINUE: Self = Self(82);
    pub const MARK_LABEL: Self = Self(83);
    pub const MAXIMUM: Self = Self(84);
    pub const MAXIMUM_U32: Self = Self(85);
    pub const MEMORY_BARRIER: Self = Self(86);
    pub const MINIMUM: Self = Self(87);
    pub const MINIMUM_U32: Self = Self(88);
    pub const MODULO: Self = Self(89);
    pub const MULTIPLY: Self = Self(90);
    pub const MULTIPLY_HIGH_S32: Self = Self(91);
    pub const MULTIPLY_HIGH_U32: Self = Self(92);
    pub const NEGATE: Self = Self(93);
    pub const PACK_DOUBLE_2X32: Self = Self(94);
    pub const PACK_HALF_2X16: Self = Self(95);
    pub const RECIPROCAL_SQUARE_ROOT: Self = Self(96);
    pub const RETURN: Self = Self(97);
    pub const ROUND: Self = Self(98);
    pub const SHIFT_LEFT: Self = Self(99);
    pub const SHIFT_RIGHT_S32: Self = Self(100);
    pub const SHIFT_RIGHT_U32: Self = Self(101);
    pub const SHUFFLE: Self = Self(102);
    pub const SHUFFLE_DOWN: Self = Self(103);
    pub const SHUFFLE_UP: Self = Self(104);
    pub const SHUFFLE_XOR: Self = Self(105);
    pub const SINE: Self = Self(106);
    pub const SQUARE_ROOT: Self = Self(107);
    pub const STORE: Self = Self(108);
    pub const SUBTRACT: Self = Self(109);
    pub const SWIZZLE_ADD: Self = Self(110);
    pub const TEXTURE_SAMPLE: Self = Self(111);
    pub const TEXTURE_QUERY_SAMPLES: Self = Self(112);
    pub const TEXTURE_QUERY_SIZE: Self = Self(113);
    pub const TRUNCATE: Self = Self(114);
    pub const UNPACK_DOUBLE_2X32: Self = Self(115);
    pub const UNPACK_HALF_2X16: Self = Self(116);
    pub const VECTOR_EXTRACT: Self = Self(117);
    pub const VOTE_ALL: Self = Self(118);
    pub const VOTE_ALL_EQUAL: Self = Self(119);
    pub const VOTE_ANY: Self = Self(120);
    pub const COUNT: Self = Self(121);
    pub const FP32: Self = Self(1 << 16);
    pub const FP64: Self = Self(1 << 17);
    pub const MASK: Self = Self(0xffff);
}

impl Instruction {
    /// The operation without its type flags.
    pub fn base(self) -> Self {
        Self(self.0 & Self::MASK.0)
    }

    pub fn is_atomic(self) -> bool {
        matches!(
            self.base(),
            Self::ATOMIC_ADD | Self::ATOMIC_AND | Self::ATOMIC_COMPARE_AND_SWAP | Self::ATOMIC_MAX_S32 | Self::ATOMIC_MAX_U32
                | Self::ATOMIC_MIN_S32 | Self::ATOMIC_MIN_U32 | Self::ATOMIC_OR | Self::ATOMIC_SWAP | Self::ATOMIC_XOR
        )
    }

    pub fn is_comparison(self) -> bool {
        matches!(
            self.base(),
            Self::COMPARE_EQUAL | Self::COMPARE_GREATER | Self::COMPARE_GREATER_OR_EQUAL | Self::COMPARE_GREATER_OR_EQUAL_U32
                | Self::COMPARE_GREATER_U32 | Self::COMPARE_LESS | Self::COMPARE_LESS_OR_EQUAL | Self::COMPARE_LESS_OR_EQUAL_U32
                | Self::COMPARE_LESS_U32 | Self::COMPARE_NOT_EQUAL
        )
    }

    pub fn is_texture_query(self) -> bool {
        matches!(self.base(), Self::LOD | Self::TEXTURE_QUERY_SAMPLES | Self::TEXTURE_QUERY_SIZE)
    }

    pub fn is_image(self) -> bool {
        matches!(self.base(), Self::IMAGE_ATOMIC | Self::IMAGE_LOAD | Self::IMAGE_STORE)
    }

    pub fn is_image_store(self) -> bool {
        matches!(self.base(), Self::IMAGE_ATOMIC | Self::IMAGE_STORE)
    }
}

impl std::ops::BitOr for Instruction {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

impl std::ops::BitAnd for Instruction {
    type Output = Self;
    fn bitand(self, o: Self) -> Self {
        Self(self.0 & o.0)
    }
}

impl std::ops::Not for Instruction {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl Instruction {
    /// Instruction.ToString() (a [Flags] enum).
    pub fn name(self) -> String {
        const NAMES: &[(u32, &str)] = &[
        (0x1, "Absolute"),
        (0x2, "Add"),
        (0x3, "AtomicAdd"),
        (0x4, "AtomicAnd"),
        (0x5, "AtomicCompareAndSwap"),
        (0x6, "AtomicMinS32"),
        (0x7, "AtomicMinU32"),
        (0x8, "AtomicMaxS32"),
        (0x9, "AtomicMaxU32"),
        (0xa, "AtomicOr"),
        (0xb, "AtomicSwap"),
        (0xc, "AtomicXor"),
        (0xd, "Ballot"),
        (0xe, "Barrier"),
        (0xf, "BitCount"),
        (0x10, "BitfieldExtractS32"),
        (0x11, "BitfieldExtractU32"),
        (0x12, "BitfieldInsert"),
        (0x13, "BitfieldReverse"),
        (0x14, "BitwiseAnd"),
        (0x15, "BitwiseExclusiveOr"),
        (0x16, "BitwiseNot"),
        (0x17, "BitwiseOr"),
        (0x18, "Branch"),
        (0x19, "BranchIfFalse"),
        (0x1a, "BranchIfTrue"),
        (0x1b, "Call"),
        (0x1c, "Ceiling"),
        (0x1d, "Clamp"),
        (0x1e, "ClampU32"),
        (0x1f, "Comment"),
        (0x20, "CompareEqual"),
        (0x21, "CompareGreater"),
        (0x22, "CompareGreaterOrEqual"),
        (0x23, "CompareGreaterOrEqualU32"),
        (0x24, "CompareGreaterU32"),
        (0x25, "CompareLess"),
        (0x26, "CompareLessOrEqual"),
        (0x27, "CompareLessOrEqualU32"),
        (0x28, "CompareLessU32"),
        (0x29, "CompareNotEqual"),
        (0x2a, "ConditionalSelect"),
        (0x2b, "ConvertFP32ToFP64"),
        (0x2c, "ConvertFP64ToFP32"),
        (0x2d, "ConvertFP32ToS32"),
        (0x2e, "ConvertFP32ToU32"),
        (0x2f, "ConvertFP64ToS32"),
        (0x30, "ConvertFP64ToU32"),
        (0x31, "ConvertS32ToFP32"),
        (0x32, "ConvertS32ToFP64"),
        (0x33, "ConvertU32ToFP32"),
        (0x34, "ConvertU32ToFP64"),
        (0x35, "Copy"),
        (0x36, "Cosine"),
        (0x37, "Ddx"),
        (0x38, "Ddy"),
        (0x39, "Discard"),
        (0x3a, "Divide"),
        (0x3b, "EmitVertex"),
        (0x3c, "EndPrimitive"),
        (0x3d, "ExponentB2"),
        (0x3e, "FSIBegin"),
        (0x3f, "FSIEnd"),
        (0x40, "FindLSB"),
        (0x41, "FindMSBS32"),
        (0x42, "FindMSBU32"),
        (0x43, "Floor"),
        (0x44, "FusedMultiplyAdd"),
        (0x45, "GroupMemoryBarrier"),
        (0x46, "ImageLoad"),
        (0x47, "ImageStore"),
        (0x48, "ImageAtomic"),
        (0x49, "IsNan"),
        (0x4a, "Load"),
        (0x4b, "Lod"),
        (0x4c, "LogarithmB2"),
        (0x4d, "LogicalAnd"),
        (0x4e, "LogicalExclusiveOr"),
        (0x4f, "LogicalNot"),
        (0x50, "LogicalOr"),
        (0x51, "LoopBreak"),
        (0x52, "LoopContinue"),
        (0x53, "MarkLabel"),
        (0x54, "Maximum"),
        (0x55, "MaximumU32"),
        (0x56, "MemoryBarrier"),
        (0x57, "Minimum"),
        (0x58, "MinimumU32"),
        (0x59, "Modulo"),
        (0x5a, "Multiply"),
        (0x5b, "MultiplyHighS32"),
        (0x5c, "MultiplyHighU32"),
        (0x5d, "Negate"),
        (0x5e, "PackDouble2x32"),
        (0x5f, "PackHalf2x16"),
        (0x60, "ReciprocalSquareRoot"),
        (0x61, "Return"),
        (0x62, "Round"),
        (0x63, "ShiftLeft"),
        (0x64, "ShiftRightS32"),
        (0x65, "ShiftRightU32"),
        (0x66, "Shuffle"),
        (0x67, "ShuffleDown"),
        (0x68, "ShuffleUp"),
        (0x69, "ShuffleXor"),
        (0x6a, "Sine"),
        (0x6b, "SquareRoot"),
        (0x6c, "Store"),
        (0x6d, "Subtract"),
        (0x6e, "SwizzleAdd"),
        (0x6f, "TextureSample"),
        (0x70, "TextureQuerySamples"),
        (0x71, "TextureQuerySize"),
        (0x72, "Truncate"),
        (0x73, "UnpackDouble2x32"),
        (0x74, "UnpackHalf2x16"),
        (0x75, "VectorExtract"),
        (0x76, "VoteAll"),
        (0x77, "VoteAllEqual"),
        (0x78, "VoteAny"),
        (0x79, "Count"),
        (0x10000, "FP32"),
        (0x20000, "FP64"),
        (0xffff, "Mask"),
        ];
        crate::types::net_flags_name(self.0, NAMES)
    }
}
