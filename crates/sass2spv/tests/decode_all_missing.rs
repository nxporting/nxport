use sass2spv::decoders::decoder::decode;
use sass2spv::decoders::inst_name::InstName;
use sass2spv::gpu_accessor::MemoryGpuAccessor;
use sass2spv::options::{ShaderStage, TargetApi, TargetLanguage, TranslationFlags, TranslationOptions};
use sass2spv::translation::emit_shader;
use sass2spv::codegen::SpirvGenerator;
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[test]
fn test_decode_all_missing_shaders() {
    let dir = Path::new("../../shader_dump_missing");
    if !dir.exists() {
        return;
    }

    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("code"))
        .collect();
    entries.sort_by_key(|e| e.path());

    println!("Found {} missing shader files", entries.len());

    let mut opcode_counts: BTreeMap<InstName, usize> = BTreeMap::new();
    let mut success_count = 0;

    for entry in &entries {
        let path = entry.path();
        let bytes = fs::read(&path).unwrap();
        let filename = path.file_name().unwrap().to_str().unwrap();

        let is_s1 = filename.ends_with("_s1.code");
        let start_offset = if is_s1 { 0x50 } else { 0x50 };
        let stage = if is_s1 { ShaderStage::Fragment } else { ShaderStage::Vertex };

        let sass = &bytes[0x30..];
        let cb1 = MemoryGpuAccessor::find_const_bank(&bytes);
        let accessor = MemoryGpuAccessor::new(sass, false, Some(&bytes), cb1);

        let program = decode(&accessor, start_offset, false);

        for block in &program.blocks {
            for op in &block.op_codes {
                *opcode_counts.entry(op.name).or_insert(0) += 1;
            }
        }

        let ops = emit_shader(&program);

        let options = TranslationOptions::new(
            TargetLanguage::Spirv,
            TargetApi::Metal,
            TranslationFlags::NONE,
        );

        let generator = SpirvGenerator::new(&options, stage);
        let spv = generator.generate(&ops);

        assert!(spv.len() >= 20, "Shader {} SPIR-V too short", filename);

        // Verify with spirv-cross --msl if spirv-cross is installed on system (skip the 3 7600-inst shaders in fast test)
        let is_huge = filename.starts_with("3ae4") || filename.starts_with("5b04") || filename.starts_with("d5f9");
        if !is_huge {
            let temp_spv = format!("/tmp/test_{}.spv", filename);
            let _ = fs::write(&temp_spv, &spv);
            let msl_res = std::process::Command::new("spirv-cross")
                .arg("--msl")
                .arg(&temp_spv)
                .output();
            if let Ok(out) = msl_res {
                assert!(
                    out.status.success(),
                    "spirv-cross --msl failed on {}: {}",
                    filename,
                    String::from_utf8_lossy(&out.stderr)
                );
            }
        }

        success_count += 1;
    }

    println!("Successfully decoded and compiled {} / {} shaders to SPIR-V!", success_count, entries.len());
    println!("Opcode histogram across all missing UI shaders:");
    for (name, count) in opcode_counts {
        println!("  {:?}: {}", name, count);
    }
}
