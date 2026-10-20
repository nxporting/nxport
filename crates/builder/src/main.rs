// nxport-builder: turns a game dump into a native app. The window is macOS-only so far (AppKit + imgui, app_macos.rs);
// a Windows front end is part of the Windows port (docs/windows-port.md).

#[cfg(target_os = "macos")]
mod app_macos;

#[cfg(target_os = "macos")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app_macos::main()
}

#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("nxport-builder has no window on this host yet; nxport-cli has the commands that run here");
    std::process::ExitCode::FAILURE
}
