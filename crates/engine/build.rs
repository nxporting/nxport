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
}
