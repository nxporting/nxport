// The builder is a single file on Windows: SDL3.dll is linked delay-loaded, so the program starts without it beside,
// and loads the copy it carries before it uses SDL (nxport::sdl_library, main.rs).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo::rustc-link-arg-bins=/DELAYLOAD:SDL3.dll");
        println!("cargo::rustc-link-arg-bins=delayimp.lib");
    }
}
