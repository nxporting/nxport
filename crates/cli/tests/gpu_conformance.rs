// GPU backend conformance: frame captures replayed on the backend and checked against their references, pass by pass.
//
// Synthetic scenarios (nxport-cli gpu-scenario) are written fresh and checked against the references in tests/gpu/.
// Game captures (nxport --capture-frame) hold game data and stay outside the repository: every capture with references
// in a game's data directory (~/Library/Application Support/nxport/games/<title id>/captures/*/ref) is checked too, and
// without any that part is skipped. A capture's args.txt holds engine options for its replay, one per line (the shader
// cache its references were recorded with); its tolerance.toml how far results may stray (capture/image.rs).
//
// Re-recording a scenario's references after a deliberate change:
//   nxport-cli gpu-scenario NAME DIR && nxport-cli gpu-replay DIR --record --refs crates/cli/tests/gpu/NAME
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::process::Command;

fn cli() -> Command {
    Command::new(env!("CARGO_BIN_EXE_nxport-cli"))
}

fn run(mut c: Command) -> (bool, String) {
    let out = c.output().expect("runs nxport-cli");
    let text = String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

fn summary(text: &str) -> String {
    text.lines().filter(|l| l.starts_with("[gpu-replay] DIFFERS") || l.contains(" checked in ")).collect::<Vec<_>>().join("\n")
}

#[test]
fn scenarios() {
    let (ok, list) = run({
        let mut c = cli();
        c.args(["gpu-scenario", "list"]);
        c
    });
    assert!(ok, "{list}");
    let refs = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/gpu");
    let tmp = std::env::temp_dir().join(format!("nxport-gpu-scenarios-{}", std::process::id()));
    let mut failed = Vec::new();
    for name in list.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let dir = tmp.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        let (ok, text) = run({
            let mut c = cli();
            c.args(["gpu-scenario", name]).arg(&dir);
            c
        });
        assert!(ok, "writing scenario {name}:\n{text}");
        if !refs.join(name).join("passes.bin").is_file() {
            failed.push(format!("{name}: no references in {}", refs.join(name).display()));
            continue;
        }
        let (ok, text) = run({
            let mut c = cli();
            c.arg("gpu-replay").arg(&dir).arg("--all").arg("--refs").arg(refs.join(name));
            c
        });
        println!("{name}: {}", summary(&text));
        if !ok {
            failed.push(format!("{name} (diff images in {}):\n{}", dir.join("diff").display(), summary(&text)));
        }
    }
    assert!(failed.is_empty(), "scenarios that differ:\n{}", failed.join("\n"));
}

/// Captures with references in the games' data directories, with the game's shader cache if it has one.
fn game_captures() -> Vec<(PathBuf, Option<PathBuf>)> {
    let Some(home) = std::env::var_os("HOME") else { return Vec::new() };
    let games = PathBuf::from(home).join("Library/Application Support/nxport/games");
    let mut out = Vec::new();
    for game in std::fs::read_dir(&games).into_iter().flatten().flatten() {
        let cache = game.path().join("shader_cache");
        let cache = cache.join("index.txt").is_file().then_some(cache);
        for cap in std::fs::read_dir(game.path().join("captures")).into_iter().flatten().flatten() {
            if cap.path().join("ref/passes.bin").is_file() {
                out.push((cap.path(), cache.clone()));
            }
        }
    }
    out.sort();
    out
}

#[test]
fn game_frames() {
    let caps = game_captures();
    if caps.is_empty() {
        println!("no game captures with references: skipped");
        return;
    }
    let mut failed = Vec::new();
    for (dir, cache) in caps {
        let mut c = cli();
        c.arg("gpu-replay").arg(&dir).arg("--");
        let args: Vec<String> = std::fs::read_to_string(dir.join("args.txt"))
            .unwrap_or_default()
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        if let (Some(cache), false) = (cache, args.iter().any(|a| a.starts_with("--shader-cache"))) {
            c.arg(format!("--shader-cache={}", cache.display()));
        }
        c.args(args);
        let (ok, text) = run(c);
        println!("{}: {}", dir.display(), summary(&text));
        if !ok {
            failed.push(format!("{} (diff images in {}):\n{}", dir.display(), dir.join("diff").display(), summary(&text)));
        }
    }
    assert!(failed.is_empty(), "game captures that differ:\n{}", failed.join("\n"));
}
