fn main() {
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
