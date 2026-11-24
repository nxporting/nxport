// What a run reached, for automated testing that drives games without a person (--coverage-log=FILE): the first time
// the game opens each file, uses each shader stage and calls each engine function, one line each with the presented
// frame it happened at ("<frame> <kind> <key>"). A driver that tries input sequences keeps the ones whose runs reach
// something new and builds on them (coverage-guided input), so runs go further into a game than fixed presses do.

use std::collections::HashSet;
use std::io::Write;
use std::sync::{Mutex, OnceLock};

struct Log {
    seen: HashSet<(Kind, String)>,
    out: std::fs::File,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Kind {
    File,
    Shader,
    Call,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::File => "file",
            Kind::Shader => "shader",
            Kind::Call => "call",
        }
    }
}

fn log() -> Option<&'static Mutex<Log>> {
    static LOG: OnceLock<Option<Mutex<Log>>> = OnceLock::new();
    LOG.get_or_init(|| {
        let path = crate::settings::get().debug.coverage_log.clone()?;
        match std::fs::File::create(&path) {
            Ok(out) => Some(Mutex::new(Log { seen: HashSet::new(), out })),
            Err(e) => {
                println!("[!] Coverage log {}: {e}", path.display());
                None
            }
        }
    })
    .as_ref()
}

/// Whether the run writes a coverage log.
pub(crate) fn on() -> bool {
    log().is_some()
}

/// The game reached `key` of `kind`: written once, with the presented frame.
pub(crate) fn note(kind: Kind, key: &str) {
    let Some(log) = log() else { return };
    let Ok(mut l) = log.lock() else { return };
    if l.seen.contains(&(kind, key.to_string())) {
        return;
    }
    l.seen.insert((kind, key.to_string()));
    let frame = crate::gpu::try_backend().map_or(0, |b| b.presented_frames());
    let _ = writeln!(l.out, "{frame} {} {key}", kind.name());
}
