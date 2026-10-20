pub mod spirv;

pub use spirv::SpirvGenerator;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoders::decoder::decode;
    use crate::gpu_accessor::MemoryGpuAccessor;
    use crate::options::{ShaderStage, TargetApi, TargetLanguage, TranslationFlags, TranslationOptions};
    use crate::translation::emit_shader;
    use byteorder::{ByteOrder, LittleEndian};

    #[test]
    fn test_generate_spirv_from_real_shader() {
        let bytes = std::fs::read("../../shader_dump_missing/3461ed5d7d63fb45_s0.code").unwrap();
        let sass = &bytes[0x30..];
        let cb1 = MemoryGpuAccessor::find_const_bank(&bytes);
        let accessor = MemoryGpuAccessor::new(sass, false, Some(&bytes), cb1);
        let program = decode(&accessor, 0x50, false);
        let ops = emit_shader(&program);

        let options = TranslationOptions::new(
            TargetLanguage::Spirv,
            TargetApi::Metal,
            TranslationFlags::NONE,
        );

        let generator = SpirvGenerator::new(&options, ShaderStage::Vertex);
        let spv_bytes = generator.generate(&ops);

        assert!(spv_bytes.len() >= 20);
        let magic = LittleEndian::read_u32(&spv_bytes[0..4]);
        assert_eq!(magic, 0x07230203, "Valid SPIR-V magic header required");

        let version = LittleEndian::read_u32(&spv_bytes[4..8]);
        println!(
            "Successfully emitted {} bytes of valid SPIR-V! (Magic: 0x{:08x}, Version: 0x{:08x})",
            spv_bytes.len(),
            magic,
            version
        );
    }
}
