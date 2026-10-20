use byteorder::{ByteOrder, LittleEndian};
use sass2spv::codegen::SpirvGenerator;
use sass2spv::decoders::decode;
use sass2spv::gpu_accessor::MemoryGpuAccessor;
use sass2spv::manifest::ShaderProgramInfo;
use sass2spv::options::{ShaderStage, TargetApi, TargetLanguage, TranslationFlags, TranslationOptions};
use sass2spv::translation::emit_shader;
use std::env;
use std::fs;
use std::path::Path;

fn print_usage() {
    println!(
        "Usage: sass2spv <input.sass> <output.spv> [--compute] [--cb1=OFFSET] [--local=X,Y,Z,SHARED] [--metal]\n       sass2spv --batch <list.txt> <outdir>"
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let raw_args: Vec<String> = env::args().collect();
    let mut positional = Vec::new();
    let mut is_compute = false;
    let mut cb1_arg: Option<i32> = None;
    let mut target_metal = false;
    let mut is_batch = false;
    let mut local_arg: Option<String> = None;

    for arg in raw_args.into_iter().skip(1) {
        if arg == "--batch" {
            is_batch = true;
        } else if arg == "--compute" {
            is_compute = true;
        } else if arg == "--metal" {
            target_metal = true;
        } else if let Some(val) = arg.strip_prefix("--cb1=") {
            cb1_arg = val.parse().ok();
        } else if let Some(val) = arg.strip_prefix("--local=") {
            local_arg = Some(val.to_string());
        } else if arg.starts_with("--") {
            // ignore unknown flags
        } else {
            positional.push(arg);
        }
    }

    if is_batch && positional.len() >= 2 {
        return run_batch(&positional[0], &positional[1], target_metal);
    }

    if positional.len() < 2 {
        print_usage();
        std::process::exit(1);
    }

    let input_path = &positional[0];
    let output_path = &positional[1];

    let stage_data = fs::read(input_path)?;
    let mut sass_bytes = &stage_data[..];

    let magic = if stage_data.len() >= 4 {
        LittleEndian::read_u32(&stage_data[..4])
    } else {
        0
    };

    if stage_data.len() > 0x100 && magic == 0x12345679 {
        sass_bytes = &stage_data[0x100..];
        is_compute = true;
    } else if stage_data.len() > 0x30 && magic == 0x12345678 {
        sass_bytes = &stage_data[0x30..];
    }

    let cb1_offset = cb1_arg.unwrap_or_else(|| MemoryGpuAccessor::find_const_bank(&stage_data));

    let mut accessor = MemoryGpuAccessor::new(sass_bytes, is_compute, Some(&stage_data), cb1_offset);

    if let Some(ls) = local_arg {
        let parts: Vec<u32> = ls.split(',').filter_map(|s| s.parse().ok()).collect();
        if parts.len() >= 4 {
            accessor.local_size_x = parts[0];
            accessor.local_size_y = parts[1];
            accessor.local_size_z = parts[2];
            accessor.shared_memory_size = parts[3];
            accessor.local_memory_size = parts[3];
        }
    }

    let api = if target_metal {
        TargetApi::Metal
    } else {
        TargetApi::Vulkan
    };

    let flags = if is_compute {
        TranslationFlags::COMPUTE
    } else {
        TranslationFlags::NONE
    };

    let options = TranslationOptions::new(TargetLanguage::Spirv, api, flags);

    let start_offset = if is_compute { 0 } else { 0x50 };
    let decoded = decode(&accessor, start_offset, is_compute);

    let is_s1 = input_path.ends_with("_s1.code") || input_path.contains("_s1.");
    let (stage, stage_str) = if is_compute {
        (ShaderStage::Compute, "Compute")
    } else if is_s1 {
        (ShaderStage::Fragment, "Fragment")
    } else {
        (ShaderStage::Vertex, "Vertex")
    };

    let ops = emit_shader(&decoded);
    let generator = SpirvGenerator::new(&options, stage);
    let spv_bytes = generator.generate(&ops);

    fs::write(output_path, &spv_bytes)?;

    let info = ShaderProgramInfo::extract_from_program(&decoded, &mut accessor, stage_str);
    let manifest_json = info.to_json_string()?;
    fs::write(format!("{}.json", output_path), manifest_json)?;

    Ok(())
}

fn run_batch(list_file: &str, out_dir: &str, target_metal: bool) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(out_dir)?;
    let content = fs::read_to_string(list_file)?;

    let api = if target_metal {
        TargetApi::Metal
    } else {
        TargetApi::Vulkan
    };

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let path = fields[0];
        let name = Path::new(path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let stage_data = match fs::read(path) {
            Ok(d) => d,
            Err(e) => {
                println!("RESULT\t{}\tFAIL\tread failed: {}", name, e);
                continue;
            }
        };

        let mut sass_bytes = &stage_data[..];
        let magic = if stage_data.len() >= 4 {
            LittleEndian::read_u32(&stage_data[..4])
        } else {
            0
        };

        let is_compute = (stage_data.len() > 0x100 && magic == 0x12345679)
            || (magic != 0x12345678 && fields.len() > 2);

        if is_compute && magic == 0x12345679 {
            sass_bytes = &stage_data[0x100..];
        } else if stage_data.len() > 0x30 && magic == 0x12345678 {
            sass_bytes = &stage_data[0x30..];
        }

        let cb1_offset = if fields.len() > 1 && !fields[1].is_empty() {
            fields[1].parse::<i32>().unwrap_or(-1)
        } else {
            MemoryGpuAccessor::find_const_bank(&stage_data)
        };

        let mut accessor = MemoryGpuAccessor::new(sass_bytes, is_compute, Some(&stage_data), cb1_offset);

        if is_compute {
            let parse_int = |idx: usize, def: u32| -> u32 {
                if fields.len() > idx {
                    fields[idx].parse::<u32>().unwrap_or(def)
                } else {
                    def
                }
            };
            accessor.local_size_x = parse_int(2, 1);
            accessor.local_size_y = parse_int(3, 1);
            accessor.local_size_z = parse_int(4, 1);
            accessor.shared_memory_size = parse_int(5, 0);
            accessor.local_memory_size = parse_int(6, 0);
        }

        let flags = if is_compute {
            TranslationFlags::COMPUTE
        } else {
            TranslationFlags::NONE
        };

        let options = TranslationOptions::new(TargetLanguage::Spirv, api, flags);

        let start_offset = if is_compute { 0 } else { 0x50 };
        let decoded = decode(&accessor, start_offset, is_compute);

        let is_s1 = name.ends_with("_s1") || name.contains("_s1_");
        let (stage, stage_str) = if is_compute {
            (ShaderStage::Compute, "Compute")
        } else if is_s1 {
            (ShaderStage::Fragment, "Fragment")
        } else {
            (ShaderStage::Vertex, "Vertex")
        };

        let ops = emit_shader(&decoded);
        let generator = SpirvGenerator::new(&options, stage);
        let spv_bytes = generator.generate(&ops);

        let spv_out = Path::new(out_dir).join(format!("{}.spv", name));
        if let Err(e) = fs::write(&spv_out, &spv_bytes) {
            println!("RESULT\t{}\tFAIL\twrite spv failed: {}", name, e);
            continue;
        }

        let info = ShaderProgramInfo::extract_from_program(&decoded, &mut accessor, stage_str);
        if let Ok(manifest_json) = info.to_json_string() {
            let manifest_out = Path::new(out_dir).join(format!("{}.spv.json", name));
            let _ = fs::write(manifest_out, manifest_json);
        }

        let preamble_offset = if is_compute { 0x100 } else { 0x30 };
        println!(
            "RESULT\t{}\tOK\t{}\t{}\t{}",
            name, stage_str, decoded.max_end_address, preamble_offset
        );
    }

    Ok(())
}
