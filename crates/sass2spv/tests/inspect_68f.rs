use sass2spv::decoders::decoder::decode;
use sass2spv::gpu_accessor::MemoryGpuAccessor;
use sass2spv::translation::emit_shader;
use sass2spv::codegen::spirv::SpirvGenerator;
use sass2spv::options::{ShaderStage, TargetApi, TargetLanguage, TranslationFlags, TranslationOptions};

#[test]
fn test_inspect_68f() {
    let bytes = std::fs::read("../../shader_dump_missing/68f81b0825e32d44_s0.code").unwrap();
    let sass = &bytes[0x30..];
    let cb1 = MemoryGpuAccessor::find_const_bank(&bytes);
    let accessor = MemoryGpuAccessor::new(sass, false, Some(&bytes), cb1);
    let program = decode(&accessor, 0x50, false);
    println!("Decoded {} blocks for 68f81b0825e32d44_s0:", program.blocks.len());
    for (i, b) in program.blocks.iter().enumerate() {
        println!("Block {}: 0x{:x} - 0x{:x}, ops={}", i, b.address, b.end_address, b.op_codes.len());
        for op in &b.op_codes {
            if op.name == sass2spv::decoders::inst_name::InstName::Bra ||
               op.name == sass2spv::decoders::inst_name::InstName::Brx ||
               op.name == sass2spv::decoders::inst_name::InstName::Exit {
                println!("   {:?} (addr=0x{:x}) target=0x{:x}", op.name, op.address, op.get_absolute_address());
            }
        }
    }

    let ops = emit_shader(&program);
    println!("Emitted {} ops", ops.len());
    let options = TranslationOptions::new(TargetLanguage::Spirv, TargetApi::Metal, TranslationFlags::empty());
    let gen = SpirvGenerator::new(&options, ShaderStage::Vertex);
    let spv = gen.generate(&ops);
    use rspirv::binary::Disassemble;
    let mut loader = rspirv::dr::Loader::new();
    let words: Vec<u32> = spv.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect();
    rspirv::binary::parse_words(&words, &mut loader).unwrap();
    let module = loader.module();
    println!("SPIR-V disassembly:\n{}", module.disassemble());
}
