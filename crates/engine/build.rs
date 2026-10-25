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
