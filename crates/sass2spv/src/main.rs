// Batch translation: each line of the list is "path[\tcb1[\tlx\tly\tlz\tshared\tlocal]]"; writes
// <outdir>/<name>.spv and <name>.spv.json (--batch) or <name>.metal with entry point s_<name> (--batch-msl), and prints
// one tab-separated RESULT line per stage.

use std::io::Write;

use sass2spv::manifest_json;

/// "path[\tcb1[\tlx\tly\tlz\tshared\tlocal]]": more than two fields mark a compute stage.
fn translate(fields: &[&str], target: &sass2spv::Target) -> Result<(sass2spv::ShaderProgram, bool), String> {
    let stage = std::fs::read(fields[0]).map_err(|e| e.to_string())?;
    let num = |i: usize| fields.get(i).and_then(|f| f.parse::<i32>().ok()).unwrap_or(0);
    let compute_fields = (fields.len() > 2).then(|| [num(2), num(3), num(4), num(5)]);
    let program = sass2spv::translate_stage_to(&stage, if fields.len() > 2 { 3 } else { 0 }, num(1), compute_fields, target)?;
    let compute = program.info.stage == sass2spv::ShaderStage::Compute;
    Ok((program, compute))
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 4 || (args[1] != "--batch" && args[1] != "--batch-msl") {
        eprintln!("usage: sass2spv --batch|--batch-msl <list> <outdir>");
        std::process::exit(1);
    }
    let msl = args[1] == "--batch-msl";
    let out_dir = std::path::Path::new(&args[3]);
    std::fs::create_dir_all(out_dir).unwrap();
    let list = std::fs::read_to_string(&args[2]).unwrap();
    std::panic::set_hook(Box::new(|_| {}));
    let stdout = std::io::stdout();
    for line in list.lines() {
        let fields: Vec<&str> = line.trim().split('\t').collect();
        if fields[0].is_empty() {
            continue;
        }
        let name = std::path::Path::new(fields[0]).file_stem().unwrap().to_string_lossy().into_owned();
        let target = if msl { sass2spv::Target::Msl(format!("s_{name}")) } else { sass2spv::Target::Spirv };
        let result = std::panic::catch_unwind(|| translate(&fields, &target)).unwrap_or_else(|p| {
            let msg = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default();
            Err(format!("panic: {msg}"))
        });
        let mut out = stdout.lock();
        match result {
            Ok((program, compute)) if msl => {
                std::fs::write(out_dir.join(format!("{name}.metal")), &program.msl).unwrap();
                let _ = writeln!(out, "RESULT\t{name}\tOK\t{}\t{}\t{}", program.info.stage, program.size, if compute { 0x100 } else { 0x30 });
            }
            Ok((program, compute)) if !program.binary_code.is_empty() => {
                let spv = out_dir.join(format!("{name}.spv"));
                std::fs::write(&spv, &program.binary_code).unwrap();
                std::fs::write(out_dir.join(format!("{name}.spv.json")), manifest_json(&program)).unwrap();
                let _ = writeln!(out, "RESULT\t{name}\tOK\t{}\t{}\t{}", program.info.stage, program.size, if compute { 0x100 } else { 0x30 });
            }
            Ok(_) => {
                let _ = writeln!(out, "RESULT\t{name}\tFAIL\tempty binary");
            }
            Err(e) => {
                let first = e.lines().next().unwrap_or("").replace('\t', " ");
                let _ = writeln!(out, "RESULT\t{name}\tFAIL\t{first}");
            }
        }
    }
}
