// nxport-pipelines merge [--index <shader cache index.txt>] [--keep <list>].. [--min-sources N] [--max-per-source N]
//                        -o <out> [<list>..]
// Merges render pipeline lists (see lib.rs) into one sorted list without duplicates or invalid entries. --keep lists
// are taken as they are; the other lists' entries need --min-sources lists that name them. With --index, entries must
// name stages of that shader cache.

use std::process::ExitCode;

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

fn run(args: Vec<String>) -> Result<(), String> {
    let mut it = args.into_iter();
    if it.next().as_deref() != Some("merge") {
        return Err("usage: nxport-pipelines merge [--index F] [--keep F].. [--min-sources N] [--max-per-source N] \
                    -o OUT [LIST..]"
            .into());
    }
    let (mut index, mut keep, mut sources, mut out) = (None, Vec::new(), Vec::new(), None);
    let mut o = nxport_pipelines::MergeOptions::default();
    let num = |v: Option<String>, name: &str| -> Result<usize, String> {
        v.and_then(|v| v.parse().ok()).ok_or_else(|| format!("{name} needs a number"))
    };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--index" => index = Some(it.next().ok_or("--index needs a file")?),
            "--keep" => keep.push(it.next().ok_or("--keep needs a file")?),
            "--min-sources" => o.min_sources = num(it.next(), "--min-sources")?,
            "--max-per-source" => o.max_per_source = num(it.next(), "--max-per-source")?,
            "-o" | "--out" => out = Some(it.next().ok_or("-o needs a file")?),
            _ if a.starts_with('-') => return Err(format!("unknown option {a}")),
            _ => sources.push(a),
        }
    }
    let out = out.ok_or("-o is required")?;
    let kinds = index.as_deref().map(read).transpose()?.map(|t| nxport_pipelines::stage_kinds(&t));
    o.kinds = kinds.as_ref();
    let keep: Vec<String> = keep.iter().map(|p| read(p)).collect::<Result<_, _>>()?;
    let sources: Vec<String> = sources.iter().map(|p| read(p)).collect::<Result<_, _>>()?;
    let keep: Vec<&str> = keep.iter().map(String::as_str).collect();
    let sources: Vec<&str> = sources.iter().map(String::as_str).collect();
    let m = nxport_pipelines::merge(&keep, &sources, &o);
    std::fs::write(&out, m.text()).map_err(|e| format!("{out}: {e}"))?;
    println!(
        "{} entries ({} read, {} rejected, {} waiting for more sources, {} lists skipped) -> {out}",
        m.entries.len(),
        m.read,
        m.rejected,
        m.pending,
        m.skipped_sources
    );
    Ok(())
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
