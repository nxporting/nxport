// The engines the packaged apps run, built from the same source as the engine library linked here (whose code makes
// the iOS guest image, which only matches code patched by that same source): target/engines/<target>/release/nxport
// for macOS and iOS, passed to the packaging code as NXPORT_ENGINE_MACOS / NXPORT_ENGINE_IOS.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let root = manifest.parent().and_then(Path::parent).unwrap().to_path_buf();
    let target_dir = root.join("target/engines");
    for p in ["crates/engine/src", "crates/engine/build.rs", "crates/engine/Cargo.toml", "crates/sass2spv/src", "Cargo.lock"] {
        println!("cargo:rerun-if-changed={}", root.join(p).display());
    }
    // The Apple engines link against Apple SDKs, so only a macOS host builds them; elsewhere the paths are empty and
    // packaging falls back to engines next to this tool (pack::locate_engine_binary, ipa::ios_artifact).
    if !cfg!(target_os = "macos") {
        println!("cargo:rustc-env=NXPORT_ENGINE_MACOS=");
        println!("cargo:rustc-env=NXPORT_ENGINE_IOS=");
        return;
    }
    for (target, var) in [("aarch64-apple-darwin", "NXPORT_ENGINE_MACOS"), ("aarch64-apple-ios", "NXPORT_ENGINE_IOS")] {
        let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        c.current_dir(&root)
            .args(["build", "--release", "-p", "nxport", "--target", target, "--target-dir"])
            .arg(&target_dir);
        // A separate cargo run: none of this build's settings (target, flags, profile) may leak into it.
        for (k, _) in std::env::vars_os() {
            let k = k.to_string_lossy();
            if (k.starts_with("CARGO_") && k != "CARGO_HOME") || k.starts_with("RUSTC") || k == "RUSTFLAGS" || k == "TARGET" {
                c.env_remove(&*k);
            }
        }
        if target.ends_with("-ios") {
            c.env("IPHONEOS_DEPLOYMENT_TARGET", "16.0");
        }
        match c.status() {
            Ok(s) if s.success() => {}
            Ok(s) => panic!("building the engine for {target} failed ({s})"),
            Err(e) => panic!("cannot run cargo to build the engine for {target}: {e}"),
        }
        let bin = target_dir.join(target).join("release/nxport");
        if target.ends_with("-ios") {
            let shim_src = root.join("tools/ios/movie_shim.dylib");
            let shim_dest = target_dir.join(target).join("release/movie_shim.dylib");
            if shim_src.is_file() && !shim_dest.is_file() {
                let _ = std::fs::copy(&shim_src, &shim_dest);
            }
        }
        println!("cargo:rustc-env={var}={}", bin.display());
    }
}
