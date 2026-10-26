fn main() -> std::process::ExitCode {
    let layers = match nxport::settings::from_args(std::env::args_os()) {
        Ok(l) => l,
        Err(e) => e.exit(),
    };
    match nxport::run(layers) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            nxport::config::app_error(&e.to_string());
            std::process::ExitCode::FAILURE
        }
    }
}
