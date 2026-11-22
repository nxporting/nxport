// SOURCE_HASH: FNV-1a 64 over the translator's sources, so caches of its output know when it changed. Paths with `/`,
// text with LF line ends and without comments (source_text::code_only): the same code checked out with CRLF (files
// edited on Windows) or on another OS, or with other comments, hashes the same, so the engine doesn't drop a shader
// cache an identical translator wrote (a builder's prebuilt cache included).

include!("src/source_text.rs");

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
        let path = f.to_string_lossy().replace('\\', "/");
        let text = code_only(&std::fs::read(f).unwrap());
        for b in path.bytes().chain(text) {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=SASS2SPV_SOURCE_HASH={h:016x}");
    println!("cargo:rerun-if-changed=src");
}
