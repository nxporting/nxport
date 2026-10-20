#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum StorageKind {
    None = 0,
    Input,
    InputPerPatch,
    Output,
    OutputPerPatch,
    ConstantBuffer,
    StorageBuffer,
    LocalMemory,
    SharedMemory,
    GlobalMemory,
}

impl StorageKind {
    pub fn is_input_or_output(&self) -> bool {
        matches!(
            self,
            Self::Input | Self::InputPerPatch | Self::Output | Self::OutputPerPatch
        )
    }

    pub fn is_output(&self) -> bool {
        matches!(self, Self::Output | Self::OutputPerPatch)
    }

    pub fn is_per_patch(&self) -> bool {
        matches!(self, Self::InputPerPatch | Self::OutputPerPatch)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum IoVariable {
    Invalid = 0,
    BackColorDiffuse,
    BackColorSpecular,
    BaseInstance,
    BaseVertex,
    ClipDistance,
    CtaId,
    DrawIndex,
    FogCoord,
    FragmentCoord,
    FragmentOutputColor,
    FragmentOutputDepth,
    FrontColorDiffuse,
    FrontColorSpecular,
    FrontFacing,
    GlobalId,
    InstanceId,
    InstanceIndex,
    InvocationId,
    Layer,
    PatchVertices,
    PointCoord,
    PointSize,
    Position,
    PrimitiveId,
    SubgroupEqMask,
    SubgroupGeMask,
    SubgroupGtMask,
    SubgroupLaneId,
    SubgroupLeMask,
    SubgroupLtMask,
    TessellationCoord,
    TessellationLevelInner,
    TessellationLevelOuter,
    TextureCoord,
    ThreadId,
    ThreadKill,
    UserDefined,
    VertexId,
    VertexIndex,
    ViewportIndex,
    ViewportMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AggregateType(pub u32);

impl AggregateType {
    pub const INVALID: Self = Self(0);
    pub const VOID: Self = Self(1);
    pub const BOOL: Self = Self(2);
    pub const FP32: Self = Self(3);
    pub const FP64: Self = Self(4);
    pub const S32: Self = Self(5);
    pub const U32: Self = Self(6);

    pub const ELEMENT_TYPE_MASK: u32 = 0xff;
    pub const ELEMENT_COUNT_SHIFT: u32 = 8;
    pub const ELEMENT_COUNT_MASK: u32 = 3 << Self::ELEMENT_COUNT_SHIFT;

    pub const SCALAR: Self = Self(0 << Self::ELEMENT_COUNT_SHIFT);
    pub const VECTOR2: Self = Self(1 << Self::ELEMENT_COUNT_SHIFT);
    pub const VECTOR3: Self = Self(2 << Self::ELEMENT_COUNT_SHIFT);
    pub const VECTOR4: Self = Self(3 << Self::ELEMENT_COUNT_SHIFT);

    pub const ARRAY: Self = Self(1 << 10);

    pub fn element_type(&self) -> Self {
        Self(self.0 & Self::ELEMENT_TYPE_MASK)
    }

    pub fn element_count(&self) -> usize {
        match (self.0 & Self::ELEMENT_COUNT_MASK) >> Self::ELEMENT_COUNT_SHIFT {
            1 => 2,
            2 => 3,
            3 => 4,
            _ => 1,
        }
    }

    pub fn get_size_in_bytes(&self) -> usize {
        let elem_size = match self.element_type() {
            Self::BOOL | Self::FP32 | Self::S32 | Self::U32 => 4,
            Self::FP64 => 8,
            _ => 0,
        };
        elem_size * self.element_count()
    }
}

impl std::ops::BitOr for AggregateType {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitAnd for AggregateType {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}
