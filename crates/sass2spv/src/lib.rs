// Maxwell shader translator, ported from Ryujinx.Graphics.Shader (MIT, Copyright (c) Ryujinx Team and Contributors;
// see LICENSE).

pub mod codegen;
pub mod decoders;
pub mod gpu_accessor;
pub mod instructions;
pub mod ir;
pub mod manifest;
pub mod options;
pub mod structured_ir;
pub mod translation;

pub use decoders::{get_op, InstEmitter, InstName, InstOp, InstProps};
pub use gpu_accessor::MemoryGpuAccessor;
pub use manifest::{
    ManifestCBuffer, ManifestSBuffer, ManifestTextureOrImage, ShaderManifest, ShaderProgramInfo,
};
pub use options::{ShaderStage, TargetApi, TargetLanguage, TranslationFlags, TranslationOptions};

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub fn translate_stage(
    stage_data: &[u8],
    stage: ShaderStage,
    api: TargetApi,
) -> Result<(Vec<u8>, ShaderProgramInfo), String> {
    if stage_data.len() < 0x80 {
        return Err("Stage data too short".into());
    }
    let is_compute = stage == ShaderStage::Compute;
    let sass = if is_compute {
        stage_data
    } else {
        if stage_data.len() <= 0x30 {
            return Err("Graphics stage data missing 0x30 preamble".into());
        }
        &stage_data[0x30..]
    };

    let cb1 = MemoryGpuAccessor::find_const_bank(stage_data);
    let mut accessor = MemoryGpuAccessor::new(sass, is_compute, Some(stage_data), cb1);

    let start_offset = if is_compute { 0 } else { 0x50 };
    let decoded = decoders::decoder::decode(&accessor, start_offset, is_compute);

    let stage_str = match stage {
        ShaderStage::Compute => "Compute",
        ShaderStage::Fragment => "Fragment",
        _ => "Vertex",
    };

    let flags = if is_compute {
        TranslationFlags::COMPUTE
    } else {
        TranslationFlags::NONE
    };

    let options = TranslationOptions::new(TargetLanguage::Spirv, api, flags);
    let ops = translation::emit_shader(&decoded);
    let generator = codegen::SpirvGenerator::new(&options, stage);
    let spv_bytes = generator.generate(&ops);

    let info = ShaderProgramInfo::extract_from_program(&decoded, &mut accessor, stage_str);

    Ok((spv_bytes, info))
}

