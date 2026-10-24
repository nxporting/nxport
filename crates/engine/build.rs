fn main() {
    // nx_native: hosts that run guest code natively (cpu/, horizon/, host/, the Metal backend). Apple Silicon only so
    // far; Windows on arm64 is meant to join (docs/windows-port.md). Other hosts build the platform-independent parts.
    // nx_runtime: hosts that run games at all, natively or as lifted code (guest/, horizon/, nvn/, gpu/): the native
    // hosts and Windows so far.
    println!("cargo::rustc-check-cfg=cfg(nx_native)");
    println!("cargo::rustc-check-cfg=cfg(nx_runtime)");
    let vendor = std::env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default();
    let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let native = vendor == "apple" && arch == "aarch64";
    if native {
        println!("cargo::rustc-cfg=nx_native");
    }
    if native || os == "windows" {
        println!("cargo::rustc-cfg=nx_runtime");
    }
    // Translated Vulkan stages on disk stay valid while the translation does not change (vulkan/stage_cache.rs).
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let mut files = Vec::new();
    collect_rs(&manifest.join("../sass2spv/src"), &mut files);
    files.push(manifest.join("src/vulkan/shader.rs"));
    files.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        for b in std::fs::read(f).unwrap_or_default() {
            h = (h ^ b as u64).wrapping_mul(0x100_0000_01b3);
        }
    }
    println!("cargo:rustc-env=NX_TRANSLATION_HASH={h:016x}");
    // NativeAOT shader translator for macOS runtime translation (built if not already present; dlopened at runtime).
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let workspace_root = manifest_dir
        .parent()
        .and_then(|p| p.parent())
        .unwrap_or(&manifest_dir);
    let aot_dir = workspace_root.join("target/native_sass2spv");
    if !aot_dir.join("libsass2spv.dylib").is_file() && !aot_dir.join("sass2spv.dylib").is_file() {
        let script = workspace_root.join("tools/build_native_sass2spv.sh");
        if script.is_file() {
            let _ = std::process::Command::new(&script).arg(&aot_dir).status();
        }
    }
}

fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}
