// Maxwell shader programs to SPIR-V: a port of Ryujinx's shader translator, producing the same output.

pub mod accessor;
pub mod codegen;
pub mod collections;
pub mod decoders;
pub mod instructions;
pub mod ir;
pub mod manifest;
pub mod msl;
pub mod pipeline;
pub mod spv;
pub mod structured;
pub mod translation;
pub mod types;

pub use manifest::{ManifestCBuffer, ManifestSBuffer, ManifestTextureOrImage, ShaderManifest};
pub use pipeline::{manifest_json, translate_stage, translate_stage_to, ShaderProgram, Target, MSL_ENTRY_PLACEHOLDER};
pub use types::ShaderStage;

/// Changes whenever the translator's source does.
pub const SOURCE_HASH: &str = env!("SASS2SPV_SOURCE_HASH");
