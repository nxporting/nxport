// SOURCE_HASH: FNV-1a 64 over the translator's sources, so caches of its output know when it changed.

fn visit(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            visit(&p, files);
        } else if p.extension().is_some_and(|x| x == "rs") {
            files.push(p);
        }
    }
}

fn main() {
    let mut files = Vec::new();
    visit(std::path::Path::new("src"), &mut files);
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in &files {
        for b in f.to_string_lossy().bytes().chain(std::fs::read(f).unwrap()) {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=SASS2SPV_SOURCE_HASH={h:016x}");
    println!("cargo:rerun-if-changed=src");
}
