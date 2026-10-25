// nxport-builder: turns a game dump into a native app.

mod app;
#[cfg(target_os = "macos")]
mod apple;
mod log;
mod window;
#[cfg(windows)]
mod windows;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::main()
}
