// The engine for iOS, built on Windows from this checkout: no Apple SDK involved. The engine declares what it calls
// itself, so the linker only needs to know which libraries to load: it gets stub libraries (.tbd files) naming them, and
// every symbol is looked up in them when the app starts (flat namespace). The C libraries the engine's dependencies
// build get their headers from zig, which ships macOS's libc, and are compiled with LLVM's clang for iOS (zig's own
// target has no iOS libc, and ld64.lld refuses objects made for macOS).
//
// Needs LLVM (clang, ld64.lld, llvm-ar) and the Rust target: rustup target add aarch64-apple-ios.

use std::path::{Path, PathBuf};
use std::process::Command;

const MIN_IOS: &str = "16.0";

const LIBRARIES: &[(&str, &str)] = &[
    ("System", "/usr/lib/libSystem.B.dylib"),
    ("c", "/usr/lib/libSystem.B.dylib"),
    ("m", "/usr/lib/libSystem.B.dylib"),
    ("pthread", "/usr/lib/libSystem.B.dylib"),
    ("dl", "/usr/lib/libSystem.B.dylib"),
    ("objc", "/usr/lib/libobjc.A.dylib"),
    ("iconv", "/usr/lib/libiconv.2.dylib"),
    ("c++", "/usr/lib/libc++.1.dylib"),
];

const FRAMEWORKS: &[&str] = &[
    "AVFAudio",
    "AudioToolbox",
    "CloudKit",
    "CoreData",
    "CoreFoundation",
    "CoreGraphics",
    "CoreHaptics",
    "CoreImage",
    "CoreLocation",
    "CoreText",
    "CoreVideo",
    "Foundation",
    "GameController",
    "Metal",
    "QuartzCore",
    "Security",
    "UIKit",
    "UserNotifications",
];

/// The checkout this program was built from, if it is still there.
pub fn source_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?.to_path_buf();
    root.join("crates/engine/Cargo.toml").is_file().then_some(root)
}

/// Where `build` leaves the engine.
pub fn built_engine() -> Option<PathBuf> {
    Some(source_root()?.join("target/engines/aarch64-apple-ios/release/nxport"))
}

fn tbd(install_name: &str, tag: &str) -> String {
    format!(
        "--- !tapi-tbd\ntbd-version: 4\ntargets: [ arm64-ios ]\ninstall-name: '{install_name}'\ncurrent-version: 1\ncompatibility-version: 1\nexports:\n  - targets: [ arm64-ios ]\n    symbols: [ '_nxport_stub_{tag}' ]\n...\n"
    )
}

fn write_stub_sdk(sdk: &Path) -> Result<(), String> {
    let write = |path: PathBuf, text: String| -> Result<(), String> {
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    for (name, install) in LIBRARIES {
        write(sdk.join(format!("usr/lib/lib{name}.tbd")), tbd(install, &name.replace('+', "x")))?;
    }
    for f in FRAMEWORKS {
        let install = format!("/System/Library/Frameworks/{f}.framework/{f}");
        write(sdk.join(format!("System/Library/Frameworks/{f}.framework/{f}.tbd")), tbd(&install, f))?;
    }
    Ok(())
}

/// The directory of LLVM's tools: on the PATH, else where its installer puts them.
fn llvm_bin() -> Result<PathBuf, String> {
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
    on_path
        .chain([PathBuf::from(r"C:\Program Files\LLVM\bin")])
        .find(|d| d.join("ld64.lld.exe").is_file() && d.join("clang.exe").is_file())
        .ok_or_else(|| "building the iOS engine needs LLVM (clang and ld64.lld): https://github.com/llvm/llvm-project/releases".to_string())
}

/// zig's headers for Apple platforms, as clang arguments.
fn libc_includes(zig: &Path) -> String {
    let inc = zig.join("lib/libc/include");
    ["any-darwin-any"]
        .iter()
        .map(|d| format!("-isystem {}", inc.join(d).display().to_string().replace('\\', "/")))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Builds the engine for iOS from this checkout (cargo does nothing when it is up to date); returns its path.
pub fn build(toolchains: &Path) -> Result<PathBuf, String> {
    let root = source_root().ok_or("the iOS engine builds from nxport's source, which is not here")?;
    let llvm = llvm_bin()?;
    let zig = crate::zig::cc(toolchains, &|_, _| {})?;
    let zig_dir = Path::new(&zig[0]).parent().ok_or("no zig directory")?.to_path_buf();
    if zig_dir.display().to_string().contains(' ') {
        return Err(format!("{}: the compiler's arguments cannot hold a path with spaces", zig_dir.display()));
    }
    let target_dir = root.join("target/engines");
    let sdk = target_dir.join("ios-stub-sdk");
    write_stub_sdk(&sdk)?;
    let libc = libc_includes(&zig_dir);
    let cxx = zig_dir.join("lib/libcxx").display().to_string().replace('\\', "/");
    let cxxabi = zig_dir.join("lib/libcxxabi").display().to_string().replace('\\', "/");
    let target = format!("--target=arm64-apple-ios{MIN_IOS}");
    let path = std::env::join_paths(std::iter::once(llvm.clone()).chain(std::env::var_os("PATH").iter().flat_map(std::env::split_paths)))
        .map_err(|e| e.to_string())?;
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(&root).args(["build", "--release", "-p", "nxport", "--bin", "nxport", "--target", "aarch64-apple-ios", "--target-dir"]).arg(&target_dir);
    // A separate cargo run: none of a surrounding build's settings may leak into it.
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy();
        if (k.starts_with("CARGO_") && k != "CARGO_HOME") || k.starts_with("RUSTC") || k == "RUSTFLAGS" || k == "TARGET" {
            c.env_remove(&*k);
        }
    }
    c.env("PATH", path)
        .env("SDKROOT", &sdk)
        .env("IPHONEOS_DEPLOYMENT_TARGET", MIN_IOS)
        .env("CRATE_CC_NO_DEFAULTS", "1")
        .env("CC_aarch64_apple_ios", format!("clang {target} -nostdlibinc {libc} -D__DARWIN_UNIX03=1 -Wno-everything"))
        .env(
            "CXX_aarch64_apple_ios",
            format!(
                "clang++ {target} -nostdlibinc -nostdinc++ -isystem {cxx}/include -isystem {cxxabi}/include {libc} -D_LIBCPP_ABI_VERSION=1 -D_LIBCPP_ABI_NAMESPACE=__1 -D_LIBCPP_HAS_THREADS=1 -D_LIBCPP_HAS_MONOTONIC_CLOCK -D_LIBCPP_HAS_TERMINAL -D_LIBCPP_HAS_MUSL_LIBC=0 -D_LIBCPP_DISABLE_VISIBILITY_ANNOTATIONS -D_LIBCPP_HAS_VENDOR_AVAILABILITY_ANNOTATIONS=0 -D_LIBCPP_HAS_FILESYSTEM=1 -D_LIBCPP_HAS_RANDOM_DEVICE -D_LIBCPP_HAS_LOCALIZATION -D_LIBCPP_HAS_UNICODE -D_LIBCPP_HAS_WIDE_CHARACTERS -D_LIBCPP_HAS_NO_STD_MODULES -D_LIBCPP_PSTL_BACKEND_SERIAL -D_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_NONE -D_LIBCPP_ASSERTION_SEMANTIC_DEFAULT=_LIBCPP_ASSERTION_SEMANTIC_IGNORE -D_LIBCPP_DISABLE_AVAILABILITY -Wno-everything"
            ),
        )
        .env("AR_aarch64_apple_ios", "llvm-ar")
        .env("CARGO_TARGET_AARCH64_APPLE_IOS_LINKER", llvm.join("ld64.lld.exe"))
        .env("CARGO_TARGET_AARCH64_APPLE_IOS_RUSTFLAGS", "-C linker-flavor=ld64.lld -C link-arg=-flat_namespace -C link-arg=-undefined -C link-arg=suppress");
    let out = c.output().map_err(|e| format!("cargo: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        let hint = if err.contains("aarch64-apple-ios") && err.contains("target may not be installed") { "\nrustup target add aarch64-apple-ios" } else { "" };
        let tail: Vec<&str> = err.lines().rev().take(12).collect();
        return Err(format!("building the iOS engine failed:\n{}{hint}", tail.into_iter().rev().collect::<Vec<_>>().join("\n")));
    }
    built_engine().filter(|e| e.is_file()).ok_or_else(|| "the iOS engine was not written".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_name_each_library_once_per_install_name() {
        let dir = std::env::temp_dir().join(format!("nxport-ios-stubs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_stub_sdk(&dir).unwrap();
        let t = std::fs::read_to_string(dir.join("System/Library/Frameworks/Metal.framework/Metal.tbd")).unwrap();
        assert!(t.contains("install-name: '/System/Library/Frameworks/Metal.framework/Metal'"));
        assert!(t.contains("arm64-ios"));
        let c = std::fs::read_to_string(dir.join("usr/lib/libc.tbd")).unwrap();
        assert!(c.contains("libSystem.B.dylib"));
        assert!(dir.join("usr/lib/libc++.tbd").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
