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

    // Vendor SDL3: point the linker at the pre-built SDL3.dll import lib and
    // copy SDL3.dll next to the output binary so it can be found at runtime.
    if os == "windows" {
        let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
        let workspace_root = manifest_dir
            .parent()
            .and_then(|p| p.parent())
            .unwrap_or(&manifest_dir);
        let sdl3_lib = workspace_root.join("vendor/SDL3/lib");
        if sdl3_lib.exists() {
            // Link against SDL3.lib (import lib) so the linker resolves SDL3 symbols.
            println!("cargo::rustc-link-search=native={}", sdl3_lib.display());
            // Copy SDL3.dll next to the built binary so it is found at runtime.
            let dll_src = sdl3_lib.join("SDL3.dll");
            let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap());
            // OUT_DIR is target/<profile>/build/<crate>-<hash>/out — walk up to target/<profile>.
            let mut exe_dir = out_dir.clone();
            for _ in 0..3 { exe_dir.pop(); }
            let dll_dst = exe_dir.join("SDL3.dll");
            if dll_src.exists() && !dll_dst.exists() {
                std::fs::copy(&dll_src, &dll_dst).ok();
            }
            println!("cargo::rerun-if-changed={}", dll_src.display());
        }
    }

    compile_shader("src/vulkan/astc.comp", "astc.spv");
}

/// A GLSL compute shader to SPIR-V in OUT_DIR (included by the Vulkan backend).
fn compile_shader(src: &str, out: &str) {
    println!("cargo::rerun-if-changed={src}");
    let code = std::fs::read_to_string(src).unwrap();
    let module = naga::front::glsl::Frontend::default()
        .parse(&naga::front::glsl::Options::from(naga::ShaderStage::Compute), &code)
        .unwrap_or_else(|e| panic!("{src}: {}", e.emit_to_string(&code)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{src}: {}", e.emit_to_string(&code)));
    let words = naga::back::spv::write_vec(&module, &info, &naga::back::spv::Options::default(), None)
        .unwrap_or_else(|e| panic!("{src}: {e}"));
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join(out), bytes).unwrap();
}
