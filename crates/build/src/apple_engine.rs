// The engines for iOS and macOS, built on Windows from this checkout: no Apple SDK involved. The engine declares what it
// calls itself, so the linker only needs to know which libraries to load: it gets stub libraries (.tbd files) naming
// them, and every symbol is looked up in them when the app starts (flat namespace). The C libraries the engine's
// dependencies build get their headers from zig, which ships macOS's libc, and are compiled with LLVM's clang for the
// engine's system (zig's own target has no iOS libc, and ld64.lld refuses objects made for another platform).
//
// Needs LLVM (clang, ld64.lld, llvm-ar) and the Rust target: rustup target add aarch64-apple-ios (or aarch64-apple-darwin).

use std::path::{Path, PathBuf};
use std::process::Command;

/// The Apple system an engine is built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Ios,
    MacOs,
}

impl Os {
    pub fn name(self) -> &'static str {
        match self {
            Os::Ios => "iOS",
            Os::MacOs => "macOS",
        }
    }

    /// Rust's target.
    pub fn rust_target(self) -> &'static str {
        match self {
            Os::Ios => "aarch64-apple-ios",
            Os::MacOs => "aarch64-apple-darwin",
        }
    }

    /// The environment variable that tells the Rust and C compilers the oldest system the engine runs on, and that system.
    fn deployment(self) -> (&'static str, &'static str) {
        match self {
            Os::Ios => ("IPHONEOS_DEPLOYMENT_TARGET", "16.0"),
            Os::MacOs => ("MACOSX_DEPLOYMENT_TARGET", "11.0"),
        }
    }

    /// clang's and the stub libraries' name for the system.
    fn short(self) -> &'static str {
        match self {
            Os::Ios => "ios",
            Os::MacOs => "macos",
        }
    }

    /// The frameworks the engine links, each with the folder its binary is in (macOS keeps versions; iOS frameworks are
    /// flat).
    fn frameworks(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Os::Ios => IOS_FRAMEWORKS,
            Os::MacOs => MACOS_FRAMEWORKS,
        }
    }

    /// The linker's flags beyond the stubs. macOS: the platform and SDK versions a native build records
    /// (.cargo/config.toml, written for clang as the linker driver, so this build replaces it): an SDK below 13 makes the
    /// kernel preserve x18 (cpu/trampoline.rs own_build_version).
    fn link_args(self) -> &'static [&'static str] {
        match self {
            Os::Ios => &[],
            Os::MacOs => &["-platform_version", "macos", "11.0", "12.0"],
        }
    }
}

/// `-l` names and the library each loads, the same on both systems.
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

const IOS_FRAMEWORKS: &[(&str, &str)] = &[
    ("AVFAudio", ""),
    ("AudioToolbox", ""),
    ("CloudKit", ""),
    ("CoreData", ""),
    ("CoreFoundation", ""),
    ("CoreGraphics", ""),
    ("CoreHaptics", ""),
    ("CoreImage", ""),
    ("CoreLocation", ""),
    ("CoreText", ""),
    ("CoreVideo", ""),
    ("Foundation", ""),
    ("GameController", ""),
    ("Metal", ""),
    ("QuartzCore", ""),
    ("Security", ""),
    ("UIKit", ""),
    ("UserNotifications", ""),
];

/// What the engine built natively on a Mac loads (otool -L).
const MACOS_FRAMEWORKS: &[(&str, &str)] = &[
    ("AppKit", "Versions/C/"),
    ("AudioToolbox", "Versions/A/"),
    ("CloudKit", "Versions/A/"),
    ("CoreData", "Versions/A/"),
    ("CoreFoundation", "Versions/A/"),
    ("CoreGraphics", "Versions/A/"),
    ("CoreHaptics", "Versions/A/"),
    ("CoreImage", "Versions/A/"),
    ("CoreText", "Versions/A/"),
    ("CoreVideo", "Versions/A/"),
    ("Foundation", "Versions/C/"),
    ("GameController", "Versions/A/"),
    ("Metal", "Versions/A/"),
    ("QuartzCore", "Versions/A/"),
];

/// The checkout this program was built from, if it is still there.
pub fn source_root() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?.parent()?.to_path_buf();
    root.join("crates/engine/Cargo.toml").is_file().then_some(root)
}

/// Where `build` leaves the engine (where build.rs leaves it on a Mac).
pub fn built_engine(os: Os) -> Option<PathBuf> {
    Some(source_root()?.join("target/engines").join(os.rust_target()).join("release/nxport"))
}

fn tbd(os: Os, install_name: &str, tag: &str) -> String {
    let t = format!("arm64-{}", os.short());
    format!(
        "--- !tapi-tbd\ntbd-version: 4\ntargets: [ {t} ]\ninstall-name: '{install_name}'\ncurrent-version: 1\ncompatibility-version: 1\nexports:\n  - targets: [ {t} ]\n    symbols: [ '_nxport_stub_{tag}' ]\n...\n"
    )
}

fn write_stub_sdk(os: Os, sdk: &Path) -> Result<(), String> {
    let write = |path: PathBuf, text: String| -> Result<(), String> {
        let parent = path.parent().unwrap();
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    };
    for (name, install) in LIBRARIES {
        write(sdk.join(format!("usr/lib/lib{name}.tbd")), tbd(os, install, &name.replace('+', "x")))?;
    }
    for (f, folder) in os.frameworks() {
        let install = format!("/System/Library/Frameworks/{f}.framework/{folder}{f}");
        write(sdk.join(format!("System/Library/Frameworks/{f}.framework/{f}.tbd")), tbd(os, &install, f))?;
    }
    Ok(())
}

/// The directory of LLVM's tools: on the PATH, else where its installer puts them.
fn llvm_bin() -> Result<PathBuf, String> {
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>());
    on_path
        .chain([PathBuf::from(r"C:\Program Files\LLVM\bin")])
        .find(|d| d.join("ld64.lld.exe").is_file() && d.join("clang.exe").is_file())
        .ok_or_else(|| "building an Apple engine needs LLVM (clang and ld64.lld): https://github.com/llvm/llvm-project/releases".to_string())
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

/// Builds the engine for `os` from this checkout (cargo does nothing when it is up to date); returns its path.
/// `compiling` gets the name of each crate as cargo starts compiling it.
pub fn build(os: Os, toolchains: &Path, compiling: &dyn Fn(&str)) -> Result<PathBuf, String> {
    let name = os.name();
    let root = source_root().ok_or_else(|| format!("the {name} engine builds from nxport's source, which is not here"))?;
    let llvm = llvm_bin()?;
    let zig = crate::zig::cc(toolchains, &|_, _| {})?;
    let zig_dir = Path::new(&zig[0]).parent().ok_or("no zig directory")?.to_path_buf();
    if zig_dir.display().to_string().contains(' ') {
        return Err(format!("{}: the compiler's arguments cannot hold a path with spaces", zig_dir.display()));
    }
    let target_dir = root.join("target/engines");
    let sdk = target_dir.join(format!("{}-stub-sdk", os.short()));
    write_stub_sdk(os, &sdk)?;
    let libc = libc_includes(&zig_dir);
    let cxx = zig_dir.join("lib/libcxx").display().to_string().replace('\\', "/");
    let cxxabi = zig_dir.join("lib/libcxxabi").display().to_string().replace('\\', "/");
    let (deployment_var, min) = os.deployment();
    let clang_target = format!("--target=arm64-apple-{}{min}", os.short());
    let rust_target = os.rust_target();
    let env_target = rust_target.replace('-', "_");
    let path = std::env::join_paths(std::iter::once(llvm.clone()).chain(std::env::var_os("PATH").iter().flat_map(std::env::split_paths)))
        .map_err(|e| e.to_string())?;
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(&root).args(["build", "--release", "-p", "nxport", "--bin", "nxport", "--target", rust_target, "--target-dir"]).arg(&target_dir);
    // A separate cargo run: none of a surrounding build's settings may leak into it.
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy();
        if (k.starts_with("CARGO_") && k != "CARGO_HOME") || k.starts_with("RUSTC") || k == "RUSTFLAGS" || k == "TARGET" {
            c.env_remove(&*k);
        }
    }
    // The target's flags, in place of any .cargo/config.toml gives it (those are written for clang as the linker).
    let mut rustflags = vec!["-C", "linker-flavor=ld64.lld", "-C", "link-arg=-flat_namespace", "-C", "link-arg=-undefined", "-C", "link-arg=suppress"]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
    for a in os.link_args() {
        rustflags.extend(["-C".to_string(), format!("link-arg={a}")]);
    }
    c.env("PATH", path)
        .env("SDKROOT", &sdk)
        .env(deployment_var, min)
        .env("CRATE_CC_NO_DEFAULTS", "1")
        .env(format!("CC_{env_target}"), format!("clang {clang_target} -nostdlibinc {libc} -D__DARWIN_UNIX03=1 -Wno-everything"))
        .env(
            format!("CXX_{env_target}"),
            format!(
                "clang++ {clang_target} -nostdlibinc -nostdinc++ -isystem {cxx}/include -isystem {cxxabi}/include {libc} -D_LIBCPP_ABI_VERSION=1 -D_LIBCPP_ABI_NAMESPACE=__1 -D_LIBCPP_HAS_THREADS=1 -D_LIBCPP_HAS_MONOTONIC_CLOCK -D_LIBCPP_HAS_TERMINAL -D_LIBCPP_HAS_MUSL_LIBC=0 -D_LIBCPP_DISABLE_VISIBILITY_ANNOTATIONS -D_LIBCPP_HAS_VENDOR_AVAILABILITY_ANNOTATIONS=0 -D_LIBCPP_HAS_FILESYSTEM=1 -D_LIBCPP_HAS_RANDOM_DEVICE -D_LIBCPP_HAS_LOCALIZATION -D_LIBCPP_HAS_UNICODE -D_LIBCPP_HAS_WIDE_CHARACTERS -D_LIBCPP_HAS_NO_STD_MODULES -D_LIBCPP_PSTL_BACKEND_SERIAL -D_LIBCPP_HARDENING_MODE=_LIBCPP_HARDENING_MODE_NONE -D_LIBCPP_ASSERTION_SEMANTIC_DEFAULT=_LIBCPP_ASSERTION_SEMANTIC_IGNORE -D_LIBCPP_DISABLE_AVAILABILITY -Wno-everything"
            ),
        )
        .env(format!("AR_{env_target}"), "llvm-ar")
        .env(format!("CARGO_TARGET_{}_LINKER", env_target.to_uppercase()), llvm.join("ld64.lld.exe"))
        .env("CARGO_ENCODED_RUSTFLAGS", rustflags.join("\x1f"));
    // cargo reports on stderr: each "Compiling <crate> <version>" line as it starts, kept whole for the error.
    let mut child = c
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("cargo: {e}"))?;
    let mut err = String::new();
    {
        use std::io::BufRead;
        for line in std::io::BufReader::new(child.stderr.take().unwrap()).lines().map_while(Result::ok) {
            if let Some(name) = line.trim_start().strip_prefix("Compiling ").and_then(|r| r.split_whitespace().next()) {
                compiling(name);
            }
            err.push_str(&line);
            err.push('\n');
        }
    }
    let status = child.wait().map_err(|e| format!("cargo: {e}"))?;
    if !status.success() {
        let hint = if err.contains(rust_target) && err.contains("target may not be installed") { format!("\nrustup target add {rust_target}") } else { String::new() };
        let tail: Vec<&str> = err.lines().rev().take(12).collect();
        return Err(format!("building the {name} engine failed:\n{}{hint}", tail.into_iter().rev().collect::<Vec<_>>().join("\n")));
    }
    built_engine(os).filter(|e| e.is_file()).ok_or_else(|| format!("the {name} engine was not written"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stubs_name_each_library_once_per_install_name() {
        let dir = std::env::temp_dir().join(format!("nxport-ios-stubs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_stub_sdk(Os::Ios, &dir).unwrap();
        let t = std::fs::read_to_string(dir.join("System/Library/Frameworks/Metal.framework/Metal.tbd")).unwrap();
        assert!(t.contains("install-name: '/System/Library/Frameworks/Metal.framework/Metal'"));
        assert!(t.contains("arm64-ios"));
        let c = std::fs::read_to_string(dir.join("usr/lib/libc.tbd")).unwrap();
        assert!(c.contains("libSystem.B.dylib"));
        assert!(dir.join("usr/lib/libc++.tbd").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn macos_stubs_name_versioned_frameworks() {
        let dir = std::env::temp_dir().join(format!("nxport-macos-stubs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        write_stub_sdk(Os::MacOs, &dir).unwrap();
        let t = std::fs::read_to_string(dir.join("System/Library/Frameworks/Metal.framework/Metal.tbd")).unwrap();
        assert!(t.contains("install-name: '/System/Library/Frameworks/Metal.framework/Versions/A/Metal'"));
        assert!(t.contains("arm64-macos"));
        let f = std::fs::read_to_string(dir.join("System/Library/Frameworks/Foundation.framework/Foundation.tbd")).unwrap();
        assert!(f.contains("Foundation.framework/Versions/C/Foundation'"));
        assert!(!dir.join("System/Library/Frameworks/UIKit.framework").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
