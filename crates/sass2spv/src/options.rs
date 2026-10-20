use bitflags::bitflags;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLanguage {
    Spirv,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetApi {
    Vulkan,
    Metal,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct TranslationFlags: u32 {
        const NONE = 0;
        const COMPUTE = 1 << 0;
        const DEBUG_MODE = 1 << 1;
    }
}

#[derive(Debug, Clone)]
pub struct TranslationOptions {
    pub language: TargetLanguage,
    pub api: TargetApi,
    pub flags: TranslationFlags,
    /// Whether to lower subgroup (SIMD) operations in non-fragment/non-compute stages
    /// (e.g. Vertex, Geometry) to single-lane semantics.
    /// Metal strictly only supports SIMD-group operations in Fragment and Compute stages.
    pub single_lane_subgroups_in_vertex: bool,
}

impl TranslationOptions {
    pub fn new(language: TargetLanguage, api: TargetApi, flags: TranslationFlags) -> Self {
        let single_lane_subgroups_in_vertex = api == TargetApi::Metal;
        Self {
            language,
            api,
            flags,
            single_lane_subgroups_in_vertex,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ShaderStage {
    Compute,
    Vertex,
    TessellationControl,
    TessellationEvaluation,
    Geometry,
    Fragment,
}

impl ShaderStage {
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

    pub const ALL_PIPELINE_STAGES: [ShaderStage; 6] = [
        ShaderStage::Vertex,
        ShaderStage::TessellationControl,
        ShaderStage::TessellationEvaluation,
        ShaderStage::Geometry,
        ShaderStage::Fragment,
        ShaderStage::Compute,
    ];

    pub fn from_slot(slot: usize) -> Option<Self> {
        match slot {
            0 => Some(Self::Vertex),
            1 => Some(Self::TessellationControl),
            2 => Some(Self::TessellationEvaluation),
            3 => Some(Self::Geometry),
            4 => Some(Self::Fragment),
            5 => Some(Self::Compute),
            _ => None,
        }
    }

    pub fn slot(self) -> usize {
        match self {
            Self::Vertex => 0,
            Self::TessellationControl => 1,
            Self::TessellationEvaluation => 2,
            Self::Geometry => 3,
            Self::Fragment => 4,
            Self::Compute => 5,
        }
    }

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

    pub fn sph_type(self) -> Option<u32> {
        match self {
            Self::Vertex => Some(1),
            Self::TessellationControl => Some(2),
            Self::TessellationEvaluation => Some(3),
            Self::Geometry => Some(4),
            Self::Fragment => Some(5),
            Self::Compute => None,
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
}

impl std::fmt::Display for ShaderStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Compute => write!(f, "Compute"),
            Self::Vertex => write!(f, "Vertex"),
            Self::TessellationControl => write!(f, "TessellationControl"),
            Self::TessellationEvaluation => write!(f, "TessellationEvaluation"),
            Self::Geometry => write!(f, "Geometry"),
            Self::Fragment => write!(f, "Fragment"),
        }
    }
}
