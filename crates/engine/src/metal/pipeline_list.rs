// Render pipelines by the draw state that selects them (the format: crates/pipelines), kept per game (<data
// dir>/pipelines.txt, --pipeline-list) and compiled on the pipeline threads once the game starts drawing: a pipeline
// seen in an earlier session, or listed by someone else, costs no compile when it is first drawn. The shared list is
// downloaded at startup (<data dir>/pipelines_shared.txt, also the builder's copy) and its new entries compiled too. The engine derives
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

fn shared_path() -> Option<PathBuf> {
    crate::profile::data_subdir("pipelines_shared.txt")
}

struct List {
    seen: HashSet<Entry>,
    entries: Vec<Entry>,
    file: Option<std::fs::File>,
    /// precompile() has taken `entries`: entries added later are queued as they arrive.
    precompiling: bool,
}

impl List {
    /// Adds the entries not seen yet; returns them.
    fn add(&mut self, text: &str) -> Vec<Entry> {
        let mut new = Vec::new();
        for e in text.lines().filter_map(Entry::parse) {
            if self.seen.insert(e.clone()) {
                self.entries.push(e.clone());
                new.push(e);
            }
        }
        new
    }
}

fn list_state() -> &'static Mutex<List> {
    static L: OnceLock<Mutex<List>> = OnceLock::new();
    L.get_or_init(|| {
        let mut l = List { seen: HashSet::new(), entries: Vec::new(), file: None, precompiling: false };
        // The list the app ships with its shader cache (the shared one at build time), the last download of the shared
        // list, then this player's own.
        let shipped = super::shader_cache::cache_dir().join("pipelines.txt");
        for p in [Some(shipped), shared_path(), path()].into_iter().flatten() {
            if let Ok(text) = std::fs::read_to_string(&p) {
                l.add(&text);
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
        let entries = {
            let mut l = list_state().lock().unwrap();
            l.precompiling = true;
            l.entries.clone()
        };
        queue(entries);
    });
}

/// Queues entries for compilation on a thread of their own.
fn queue(entries: Vec<Entry>) {
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
                "[+] Pipelines: {queued} of {} listed queued for compilation ({skipped} the same pipeline as another entry, or with missing shaders or other \
                 state)",
                entries.len()
            );
        })
        .expect("spawn pipeline precompile thread");
}

/// Where the shared lists are published (<url>/<title id>.txt).
const FETCH_URL: &str = "https://github.com/nxporting/pipelines/releases/latest/download";

/// Downloads the shared list in the background (when it changed since the last download) and adds its new entries.
pub(super) fn fetch() {
    let s = crate::settings::get();
    if !s.graphics.fetch_pipelines || !s.graphics.precompile_pipelines {
        return;
    }
    let (Some(title), Some(dest)) = (crate::game::title_id(), shared_path()) else { return };
    let url = format!("{}/{title:016x}.txt", s.graphics.fetch_pipelines_url.as_deref().unwrap_or(FETCH_URL));
    std::thread::Builder::new()
        .name("pipe-fetch".into())
        .spawn(move || {
            let tag_path = dest.with_extension("etag");
            let tag = std::fs::read_to_string(&tag_path).ok().filter(|_| dest.exists());
            let r = match request("GET", &url, None, tag.as_deref()) {
                Ok(r) => r,
                Err(e) => return println!("[!] Pipelines: no shared list ({e}); using the last one"),
            };
            if r.status == 304 {
                return println!("[+] Pipelines: shared list unchanged");
            }
            let text = String::from_utf8_lossy(&r.body).into_owned();
            let ours = text
                .lines()
                .next()
                .and_then(nxport_pipelines::parse_header)
                .is_some_and(|(t, _)| t == title);
            if !(200..300).contains(&r.status) || !ours {
                return println!("[!] Pipelines: no shared list (HTTP {})", r.status);
            }
            let tmp = dest.with_extension("download");
            if std::fs::write(&tmp, &text).and_then(|_| std::fs::rename(&tmp, &dest)).is_ok() {
                match &r.etag {
                    Some(t) => drop(std::fs::write(&tag_path, t)),
                    None => drop(std::fs::remove_file(&tag_path)),
                }
            }
            let (new, queue_now) = {
                let mut l = list_state().lock().unwrap();
                let new = l.add(&text);
                (new, l.precompiling)
            };
            println!(
                "[+] Pipelines: shared list downloaded ({} entries, {} not listed before)",
                text.lines().filter(|l| l.starts_with("p1 ")).count(),
                new.len()
            );
            if queue_now {
                queue(new);
            }
        })
        .expect("spawn pipeline fetch thread");
}

/// The shared list's submission endpoint.
const SHARE_URL: Option<&str> = Some("https://nxport-pipelines.greenlord.workers.dev/submit");
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
        match request("POST", url, Some(body.as_bytes()), None)
            .and_then(|r| if (200..300).contains(&r.status) { Ok(()) } else { Err(format!("HTTP {}", r.status)) })
        {
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

struct Response {
    status: isize,
    body: Vec<u8>,
    etag: Option<String>,
}

/// An HTTP request (`body` as text/plain; `if_none_match`: the ETag of the copy we have); Err when no response came.
fn request(method: &str, url: &str, body: Option<&[u8]>, if_none_match: Option<&str>) -> Result<Response, String> {
    use objc2::rc::Retained;
    use objc2_foundation::{
        NSData, NSError, NSHTTPURLResponse, NSMutableURLRequest, NSString, NSURLResponse, NSURLSession, NSURL,
    };
    let u = NSURL::URLWithString(&NSString::from_str(url)).ok_or("bad URL")?;
    let req = NSMutableURLRequest::requestWithURL(&u);
    req.setHTTPMethod(&NSString::from_str(method));
    if let Some(body) = body {
        req.setValue_forHTTPHeaderField(Some(&NSString::from_str("text/plain")), &NSString::from_str("Content-Type"));
        req.setHTTPBody(Some(&NSData::with_bytes(body)));
    }
    if let Some(tag) = if_none_match {
        req.setValue_forHTTPHeaderField(Some(&NSString::from_str(tag)), &NSString::from_str("If-None-Match"));
    }
    req.setTimeoutInterval(30.0);
    // The URL cache would answer our own If-None-Match: a 304 would come back as a 200 with its stored copy.
    req.setCachePolicy(objc2_foundation::NSURLRequestCachePolicy::ReloadIgnoringLocalCacheData);
    type Done = (Option<Retained<NSData>>, Option<Retained<NSURLResponse>>, Option<Retained<NSError>>);
    let (tx, rx) = std::sync::mpsc::channel::<Done>();
    let tx = std::sync::Mutex::new(Some(tx));
    let handler = block2::RcBlock::new(move |data: *mut NSData, resp: *mut NSURLResponse, err: *mut NSError| {
        // SAFETY: the session passes retained-for-the-call objects or null.
        let done = unsafe { (Retained::retain(data), Retained::retain(resp), Retained::retain(err)) };
        if let Some(tx) = tx.lock().unwrap().take() {
            let _ = tx.send(done);
        }
    });
    // SAFETY: the handler matches the completion handler's signature.
    let task = unsafe { NSURLSession::sharedSession().dataTaskWithRequest_completionHandler(&req, &handler) };
    task.resume();
    let (data, resp, err) = rx.recv().map_err(|_| "no response")?;
    if let Some(e) = err {
        return Err(e.localizedDescription().to_string());
    }
    let resp = resp.and_then(|r| r.downcast::<NSHTTPURLResponse>().ok()).ok_or("no HTTP response")?;
    let etag = resp.valueForHTTPHeaderField(&NSString::from_str("ETag")).map(|t| t.to_string());
    Ok(Response { status: resp.statusCode(), body: data.map(|d| d.to_vec()).unwrap_or_default(), etag })
}
