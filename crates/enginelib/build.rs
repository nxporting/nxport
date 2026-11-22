fn main() {
    // nx_native: hosts that run guest code natively (guest/exec/, horizon/, host/, the Metal backend): Apple Silicon.
    // Other hosts build the platform-independent parts.
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

    bundle_profile_mods();
    compile_shader("src/gpu/vulkan/astc.comp", naga::ShaderStage::Compute, "astc.spv");
    compile_shader("src/gpu/vulkan/relayout.comp", naga::ShaderStage::Compute, "relayout.spv");
    compile_shader("src/gpu/vulkan/overlay.vert", naga::ShaderStage::Vertex, "overlay_vert.spv");
    compile_shader("src/gpu/vulkan/overlay.frag", naga::ShaderStage::Fragment, "overlay_frag.spv");
}

/// A GLSL shader to SPIR-V in OUT_DIR (included by the Vulkan backend).
fn compile_shader(src: &str, stage: naga::ShaderStage, out: &str) {
    println!("cargo::rerun-if-changed={src}");
    let code = std::fs::read_to_string(src).unwrap();
    let module = naga::front::glsl::Frontend::default()
        .parse(&naga::front::glsl::Options::from(stage), &code)
        .unwrap_or_else(|e| panic!("{src}: {}", e.emit_to_string(&code)));
    let info = naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::all())
        .validate(&module)
        .unwrap_or_else(|e| panic!("{src}: {}", e.emit_to_string(&code)));
    // Clip space as Vulkan has it: naga's default flips y for wgpu.
    let mut options = naga::back::spv::Options::default();
    options.flags.remove(naga::back::spv::WriterFlags::ADJUST_COORDINATE_SPACE);
    let words = naga::back::spv::write_vec(&module, &info, &options, None)
        .unwrap_or_else(|e| panic!("{src}: {e}"));
    let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join(out), bytes).unwrap();
}


/// The mods profiles carry (src/profile/<game>/mods/<mod>/romfs|exefs/...) as tables in OUT_DIR/profile_mods.rs, one per
/// game: `pub static <GAME>: &[BundledMod]`. They may hold only patches (.ips, .pchtxt) under romfs/ or exefs/, never
/// game content: anything else fails the build.
fn bundle_profile_mods() {
    use std::fmt::Write;
    let root = std::path::Path::new("src/profile");
    println!("cargo::rerun-if-changed={}", root.display());
    let mut out = String::new();
    let mut games: Vec<_> = std::fs::read_dir(root).unwrap().flatten().filter(|e| e.path().join("mods").is_dir()).collect();
    games.sort_by_key(|e| e.file_name());
    for game in games {
        let game_name = game.file_name().to_string_lossy().into_owned();
        let mods_dir = game.path().join("mods");
        println!("cargo::rerun-if-changed={}", mods_dir.display());
        writeln!(out, "pub static {}: &[BundledMod] = &[", game_name.to_ascii_uppercase()).unwrap();
        let mut mods: Vec<_> = std::fs::read_dir(&mods_dir).unwrap().flatten().filter(|e| e.path().is_dir()).collect();
        mods.sort_by_key(|e| e.file_name());
        for m in mods {
            let name = m.file_name().to_string_lossy().into_owned();
            let mut files = Vec::new();
            collect_files(&m.path(), &m.path(), &mut files);
            files.sort();
            writeln!(out, "    BundledMod {{ name: {name:?}, files: &[").unwrap();
            for rel in files {
                let ok_dir = rel.starts_with("romfs/") || rel.starts_with("exefs/");
                let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
                if !ok_dir || !(ext == "ips" || ext == "pchtxt") {
                    panic!("{}/{rel}: a profile's mods hold only patches (.ips, .pchtxt) under romfs/ or exefs/", m.path().display());
                }
                let abs = std::fs::canonicalize(m.path().join(&rel)).unwrap();
                println!("cargo::rerun-if-changed={}", abs.display());
                writeln!(out, "        ({rel:?}, include_bytes!({:?})),", abs.display().to_string()).unwrap();
            }
            writeln!(out, "    ] }},").unwrap();
        }
        writeln!(out, "];").unwrap();
    }
    std::fs::write(std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("profile_mods.rs"), out).unwrap();
}

/// Files under `dir` (paths relative to `base`, with '/').
fn collect_files(base: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if e.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if p.is_dir() {
            collect_files(base, &p, out);
        } else {
            let rel = p.strip_prefix(base).unwrap().components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            out.push(rel);
        }
    }
}
