// Shader-wide types (Ryujinx.Graphics.Shader's root namespace).

use std::fmt;

/// Writes a [Flags] enum value the way .NET's Enum.ToString does: an exact name if there is one, else the names of
/// the flags it is made of (largest first while subtracting, listed in ascending order), else the number.
pub fn net_flags_name(value: u32, names: &[(u32, &str)]) -> String {
    if let Some((_, n)) = names.iter().find(|(v, _)| *v == value) {
        return n.to_string();
    }
    let mut sorted: Vec<(u32, &str)> = names.to_vec();
    sorted.sort_by_key(|e| e.0);
    let mut rest = value;
    let mut picked = Vec::new();
    for &(v, n) in sorted.iter().rev() {
        if v != 0 && rest & v == v {
            rest -= v;
            picked.push(n);
        }
    }
    if rest != 0 {
        return value.to_string();
    }
    picked.reverse();
    picked.join(", ")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SamplerType(pub u32);

impl SamplerType {
    pub const NONE: Self = Self(0);
    pub const TEXTURE_1D: Self = Self(1);
    pub const TEXTURE_BUFFER: Self = Self(2);
    pub const TEXTURE_2D: Self = Self(3);
    pub const TEXTURE_3D: Self = Self(4);
    pub const TEXTURE_CUBE: Self = Self(5);
    pub const MASK: Self = Self(0xff);
    pub const ARRAY: Self = Self(1 << 8);
    pub const MULTISAMPLE: Self = Self(1 << 9);
    pub const SHADOW: Self = Self(1 << 10);

    pub fn base(self) -> Self {
        Self(self.0 & Self::MASK.0)
    }

    pub fn has(self, f: Self) -> bool {
        self.0 & f.0 != 0
    }

    pub fn dimensions(self) -> i32 {
        match self.base() {
            Self::TEXTURE_1D | Self::TEXTURE_BUFFER => 1,
            Self::TEXTURE_2D => 2,
            Self::TEXTURE_3D | Self::TEXTURE_CUBE => 3,
            _ => panic!("invalid sampler type {self}"),
        }
    }
}

impl std::ops::BitOr for SamplerType {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

impl std::ops::BitAnd for SamplerType {
    type Output = Self;
    fn bitand(self, o: Self) -> Self {
        Self(self.0 & o.0)
    }
}

impl std::ops::Not for SamplerType {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl fmt::Display for SamplerType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        const NAMES: &[(u32, &str)] = &[
            (0, "None"), (1, "Texture1D"), (2, "TextureBuffer"), (3, "Texture2D"), (4, "Texture3D"), (5, "TextureCube"),
            (0xff, "Mask"), (1 << 8, "Array"), (1 << 9, "Multisample"), (1 << 10, "Shadow"),
        ];
        f.write_str(&net_flags_name(self.0, NAMES))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum TextureFormat {
    #[default]
    Unknown,
    R8Unorm,
    R8Snorm,
    R8Uint,
    R8Sint,
    R16Float,
    R16Unorm,
    R16Snorm,
    R16Uint,
    R16Sint,
    R32Float,
    R32Uint,
    R32Sint,
    R8G8Unorm,
    R8G8Snorm,
    R8G8Uint,
    R8G8Sint,
    R16G16Float,
    R16G16Unorm,
    R16G16Snorm,
    R16G16Uint,
    R16G16Sint,
    R32G32Float,
    R32G32Uint,
    R32G32Sint,
    R8G8B8A8Unorm,
    R8G8B8A8Snorm,
    R8G8B8A8Uint,
    R8G8B8A8Sint,
    R16G16B16A16Float,
    R16G16B16A16Unorm,
    R16G16B16A16Snorm,
    R16G16B16A16Uint,
    R16G16B16A16Sint,
    R32G32B32A32Float,
    R32G32B32A32Uint,
    R32G32B32A32Sint,
    R10G10B10A2Unorm,
    R10G10B10A2Uint,
    R11G11B10Float,
}

impl TextureFormat {
    /// The type of a component read from an image of this format (GetComponentType).
    pub fn component_type(self) -> crate::translation::definitions::AggregateType {
        use crate::translation::definitions::AggregateType as A;
        use TextureFormat as F;
        match self {
            F::R8Uint | F::R16Uint | F::R32Uint | F::R8G8Uint | F::R16G16Uint | F::R32G32Uint | F::R8G8B8A8Uint | F::R16G16B16A16Uint
            | F::R32G32B32A32Uint | F::R10G10B10A2Uint => A::U32,
            F::R8Sint | F::R16Sint | F::R32Sint | F::R8G8Sint | F::R16G16Sint | F::R32G32Sint | F::R8G8B8A8Sint | F::R16G16B16A16Sint
            | F::R32G32B32A32Sint => A::S32,
            _ => A::FP32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum ShaderStage {
    Compute,
    Vertex,
    TessellationControl,
    TessellationEvaluation,
    Geometry,
    Fragment,
}

impl ShaderStage {
    pub fn is_vtg(self) -> bool {
        matches!(self, Self::Vertex | Self::TessellationControl | Self::TessellationEvaluation | Self::Geometry)
    }

    pub fn supports_render_scale(self) -> bool {
        matches!(self, Self::Vertex | Self::Fragment | Self::Compute)
    }

    pub const ALL_PIPELINE_STAGES: [ShaderStage; 6] =
        [Self::Vertex, Self::TessellationControl, Self::TessellationEvaluation, Self::Geometry, Self::Fragment, Self::Compute];

    pub fn from_str_name(s: &str) -> Option<Self> {
        match s {
            "Compute" | "comp" => Some(Self::Compute),
            "Vertex" | "vert" => Some(Self::Vertex),
            "TessellationControl" | "tesc" => Some(Self::TessellationControl),
            "TessellationEvaluation" | "tese" => Some(Self::TessellationEvaluation),
            "Geometry" | "geom" => Some(Self::Geometry),
            "Fragment" | "frag" => Some(Self::Fragment),
            _ => None,
        }
    }

    pub const fn snake_name(self) -> &'static str {
        match self {
            Self::Vertex => "vertex",
            Self::TessellationControl => "tess_control",
            Self::TessellationEvaluation => "tess_eval",
            Self::Geometry => "geometry",
            Self::Fragment => "fragment",
            Self::Compute => "compute",
        }
    }

    /// spirv-cross's stage argument.
    pub fn xcross_arg(self) -> &'static str {
        match self {
            Self::Vertex => "vert",
            Self::Fragment => "frag",
            Self::Geometry => "geom",
            Self::TessellationControl => "tesc",
            Self::TessellationEvaluation => "tese",
            Self::Compute => "comp",
        }
    }

    /// Stage of a shader program header's type field.
    pub fn from_sph_type(sph_type: u32) -> Option<Self> {
        match sph_type {
            1 => Some(Self::Vertex),
            2 => Some(Self::TessellationControl),
            3 => Some(Self::TessellationEvaluation),
            4 => Some(Self::Geometry),
            5 => Some(Self::Fragment),
            _ => None,
        }
    }

    /// Index in per-stage tables (vertex first, compute last).
    pub fn slot(self) -> usize {
        Self::ALL_PIPELINE_STAGES.iter().position(|&s| s == self).unwrap()
    }

    pub fn from_slot(slot: usize) -> Option<Self> {
        Self::ALL_PIPELINE_STAGES.get(slot).copied()
    }
}

impl fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SetBindingPair {
    pub set_index: i32,
    pub binding: i32,
}

impl SetBindingPair {
    pub fn new(set_index: i32, binding: i32) -> Self {
        SetBindingPair { set_index, binding }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttributeType {
    Float,
    Sint,
    Uint,
    Sscaled,
    Uscaled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaTestOp {
    Never = 1,
    Less,
    Equal,
    LessOrEqual,
    Greater,
    NotEqual,
    GreaterOrEqual,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputTopology {
    Points,
    Lines,
    LinesAdjacency,
    Triangles,
    TrianglesAdjacency,
}

impl InputTopology {
    pub fn input_vertex_count(self) -> i32 {
        match self {
            Self::Points => 1,
            Self::Lines => 2,
            Self::LinesAdjacency => 4,
            Self::Triangles => 3,
            Self::TrianglesAdjacency => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputTopology {
    PointList = 1,
    LineStrip = 6,
    TriangleStrip = 7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TessPatchType {
    Isolines,
    Triangles,
    Quads,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TessSpacing {
    EqualSpacing,
    FractionalEventSpacing,
    FractionalOddSpacing,
}

/// How a texture handle's offsets are packed (TextureHandleType).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextureHandleType {
    CombinedSampler = 0,
    SeparateSamplerHandle = 1,
    SeparateSamplerId = 2,
    SeparateConstantSamplerHandle = 3,
    Direct = 4,
}

/// TextureHandle.PackSlots.
pub fn pack_slots(cbuf_slot0: i32, cbuf_slot1: i32) -> i32 {
    cbuf_slot0 | ((cbuf_slot1 + 1) << 16)
}

/// TextureHandle.PackOffsets.
pub fn pack_offsets(cbuf_offset0: i32, cbuf_offset1: i32, ty: TextureHandleType) -> i32 {
    cbuf_offset0 | (cbuf_offset1 << 14) | ((ty as i32) << 28)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampler_type_names_match_dotnet() {
        assert_eq!((SamplerType::TEXTURE_2D | SamplerType::ARRAY).to_string(), "Texture2D, Array");
        assert_eq!((SamplerType::TEXTURE_2D | SamplerType::SHADOW).to_string(), "Texture2D, Shadow");
        assert_eq!(SamplerType::TEXTURE_CUBE.to_string(), "TextureCube");
        assert_eq!((SamplerType::TEXTURE_1D | SamplerType::ARRAY | SamplerType::SHADOW).to_string(), "Texture1D, Array, Shadow");
    }
}
