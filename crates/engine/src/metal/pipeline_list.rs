// Render pipelines by the draw state that selects them (the format: crates/pipelines), kept per game (<data
// dir>/pipelines.txt, --pipeline-list) and compiled on the pipeline threads once the game starts drawing: a pipeline
// seen in an earlier session, or listed by someone else, costs no compile when it is first drawn. The engine derives
// each entry's Metal pipeline as it does for a draw (translated::derive_layout, pipeline_key).

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

pub use nxport_pipelines::Entry;
use nxport_pipelines::header;

fn path() -> Option<PathBuf> {
    let s = crate::settings::get();
    s.files.pipeline_list.clone().or_else(|| crate::profile::data_subdir("pipelines.txt"))
}

struct List {
    seen: HashSet<Entry>,
    entries: Vec<Entry>,
    file: Option<std::fs::File>,
}

fn list_state() -> &'static Mutex<List> {
    static L: OnceLock<Mutex<List>> = OnceLock::new();
    L.get_or_init(|| {
        let mut l = List { seen: HashSet::new(), entries: Vec::new(), file: None };
        // The list the app ships with its shader cache (the shared one at build time), then this player's own.
        let shipped = super::shader_cache::cache_dir().join("pipelines.txt");
        for p in [Some(shipped), path()].into_iter().flatten() {
            let Ok(text) = std::fs::read_to_string(&p) else { continue };
            for e in text.lines().filter_map(Entry::parse) {
                if l.seen.insert(e.clone()) {
                    l.entries.push(e);
                }
            }
        }
        Mutex::new(l)
    })
}

/// Adds a pipeline the game drew with to the list (once).
pub(super) fn record(e: Entry) {
    if !crate::settings::get().graphics.record_pipelines {
        return;
    }
    let mut l = list_state().lock().unwrap();
    if !l.seen.insert(e.clone()) {
        return;
    }
    if l.file.is_none() {
        let Some(p) = path() else { return };
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let new = !p.exists();
        match std::fs::OpenOptions::new().create(true).append(true).open(&p) {
            Ok(mut f) => {
                if new {
                    let _ = writeln!(
                        f,
                        "{}",
                        header(
                            crate::game::title_id().unwrap_or(0),
                            crate::profile::main_build_id().unwrap_or("-")
                        )
                    );
                }
                l.file = Some(f);
            }
            Err(err) => {
                eprintln!("[!] Pipelines: {}: {err}", p.display());
                return;
            }
        }
    }
    let line = e.line();
    if let Some(f) = l.file.as_mut() {
        let _ = writeln!(f, "{line}");
    }
}

/// Queues the list's pipelines for compilation (once, when the game first draws: the window origin and depth mode
/// the entries were recorded with are known by then).
pub(super) fn precompile() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if !crate::settings::get().graphics.precompile_pipelines {
            return;
        }
        if crate::settings::get().graphics.share_pipelines {
            std::thread::Builder::new()
                .name("pipe-share".into())
                .spawn(share)
                .expect("spawn pipeline share thread");
        }
        let entries = list_state().lock().unwrap().entries.clone();
        if entries.is_empty() {
            return;
        }
        std::thread::Builder::new()
            .name("pipe-precompile".into())
            .spawn(move || {
                let (mut queued, mut skipped) = (0, 0);
                for e in &entries {
                    if super::translated::queue_precompile(e) {
                        queued += 1;
                    } else {
                        skipped += 1;
                    }
                }
                println!(
                    "[+] Pipelines: {queued} of {} listed queued for compilation ({skipped} with missing shaders or other \
                     state)",
                    entries.len()
                );
            })
            .expect("spawn pipeline precompile thread");
    });
}

/// The shared list's submission endpoint.
const SHARE_URL: Option<&str> = None;
/// Entries per submission (the endpoint refuses more).
const SHARE_MAX: usize = 4000;

/// Sends the entries of this player's list not sent before (<list>.sent counts them) to the shared list.
fn share() {
    let s = crate::settings::get();
    let Some(url) = s.graphics.share_pipelines_url.as_deref().or(SHARE_URL) else {
        return;
    };
    let Some(list) = path() else { return };
    let sent_path = list.with_extension("sent");
    let Ok(text) = std::fs::read_to_string(&list) else { return };
    let mut sent: usize = std::fs::read_to_string(&sent_path).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0);
    let lines: Vec<&str> = text.lines().filter(|l| Entry::parse(l).is_some()).collect();
    let head = header(crate::game::title_id().unwrap_or(0), crate::profile::main_build_id().unwrap_or("-"));
    while sent < lines.len() {
        let n = (lines.len() - sent).min(SHARE_MAX);
        let body = format!("{head}\n{}\n", lines[sent..sent + n].join("\n"));
        match post(url, body.as_bytes()) {
            Ok(()) => {
                sent += n;
                let _ = std::fs::write(&sent_path, sent.to_string());
                println!("[+] Pipelines: shared {n}");
            }
            Err(e) => {
                println!("[!] Pipelines: sharing failed: {e}");
                return;
            }
        }
    }
}

/// An HTTPS POST of `body` (text/plain); Ok on a 2xx response.
fn post(url: &str, body: &[u8]) -> Result<(), String> {
    use objc2_foundation::{
        NSData, NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURLSession, NSURLSessionTaskState, NSURL,
    };
    let u = NSURL::URLWithString(&NSString::from_str(url)).ok_or("bad URL")?;
    let req = NSMutableURLRequest::requestWithURL(&u);
    req.setHTTPMethod(&NSString::from_str("POST"));
    req.setValue_forHTTPHeaderField(Some(&NSString::from_str("text/plain")), &NSString::from_str("Content-Type"));
    req.setHTTPBody(Some(&NSData::with_bytes(body)));
    req.setTimeoutInterval(30.0);
    let task = NSURLSession::sharedSession().dataTaskWithRequest(&req);
    task.resume();
    while task.state() != NSURLSessionTaskState::Completed {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    if let Some(e) = task.error() {
        return Err(e.localizedDescription().to_string());
    }
    let status = task
        .response()
        .and_then(|r| r.downcast::<NSHTTPURLResponse>().ok())
        .map_or(0, |r| r.statusCode());
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(format!("HTTP {status}"))
    }
}
