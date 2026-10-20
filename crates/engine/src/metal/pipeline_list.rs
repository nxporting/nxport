// Render pipelines by the draw state that selects them, kept per game (<data dir>/pipelines.txt, --pipeline-list) and
// compiled on the pipeline threads once the game starts drawing: a pipeline seen in an earlier session, or listed by
// someone else, costs no compile when it is first drawn.
//
// An entry names its shaders by cache entry and holds the draw's NVN vertex attribute, stream and blend bits and its
// target formats; the engine derives the Metal pipeline from them as it does for a draw (translated::derive_layout,
// pipeline_key), so lists stay valid across engine builds. One entry per line:
//   p1 <vs> <fs|-> <point size bits> <depth_gl> <y_negate> <color state> <channel mask> <depth format>
//      <8 color formats> <8 blend states> <attribute bits> <stream bits>
// Lists are comma separated ('-' an unset attribute, '.' an empty list); formats are MTLPixelFormat values.

use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Entry {
    pub vs: String,
    pub fs: Option<String>,
    pub point_size: u32,
    pub depth_gl: bool,
    pub y_negate: bool,
    pub color_state: u32,
    pub channel_mask: u32,
    pub depth_pf: u64,
    pub color_pfs: [u64; 8],
    pub blends: [u64; 8],
    pub attr_bits: Vec<Option<u32>>,
    pub streams: Vec<u64>,
}

fn list<T>(v: &[T], f: impl Fn(&T) -> String) -> String {
    if v.is_empty() {
        ".".into()
    } else {
        v.iter().map(f).collect::<Vec<_>>().join(",")
    }
}

fn parse_list<T>(s: &str, f: impl Fn(&str) -> Option<T>) -> Option<Vec<T>> {
    if s == "." {
        return Some(Vec::new());
    }
    s.split(',').map(f).collect()
}

fn hex(s: &str) -> Option<u64> {
    u64::from_str_radix(s, 16).ok()
}

impl Entry {
    pub fn line(&self) -> String {
        format!(
            "p1 {} {} {:x} {} {} {:x} {:x} {} {} {} {} {}",
            self.vs,
            self.fs.as_deref().unwrap_or("-"),
            self.point_size,
            self.depth_gl as u8,
            self.y_negate as u8,
            self.color_state,
            self.channel_mask,
            self.depth_pf,
            list(&self.color_pfs, |p| p.to_string()),
            list(&self.blends, |b| format!("{b:x}")),
            list(&self.attr_bits, |a| a.map_or("-".into(), |a| format!("{a:x}"))),
            list(&self.streams, |s| format!("{s:x}")),
        )
    }

    pub fn parse(line: &str) -> Option<Entry> {
        let f: Vec<&str> = line.split_whitespace().collect();
        let ["p1", vs, fs, ps, dg, yn, cs, cm, dpf, cpfs, blends, attrs, streams] = f.as_slice() else {
            return None;
        };
        let entry = |s: &str| (s.starts_with("s_") && s.len() == 18).then(|| s.to_string());
        Some(Entry {
            vs: entry(vs)?,
            fs: if *fs == "-" { None } else { Some(entry(fs)?) },
            point_size: hex(ps)? as u32,
            depth_gl: *dg == "1",
            y_negate: *yn == "1",
            color_state: hex(cs)? as u32,
            channel_mask: hex(cm)? as u32,
            depth_pf: dpf.parse().ok()?,
            color_pfs: parse_list(cpfs, |p| p.parse().ok())?.try_into().ok()?,
            blends: parse_list(blends, hex)?.try_into().ok()?,
            attr_bits: parse_list(attrs, |a| {
                if a == "-" {
                    Some(None)
                } else {
                    hex(a).map(|a| Some(a as u32))
                }
            })?,
            streams: parse_list(streams, hex)?,
        })
    }

    /// Whether the entry's values are in range: shader kinds (from `kind`, the cache index's stage kind by entry),
    /// formats, blend fields, list lengths.
    pub fn valid(&self, kind: impl Fn(&str) -> Option<super::shader_cache::StageKind>) -> bool {
        use super::shader_cache::StageKind;
        let pf_ok = |f: u64| f < 1024;
        // NVNblendState's fields end at bit 47 (nvn/state.rs).
        let blend_ok = |b: u64| b >> 47 == 0;
        kind(&self.vs) == Some(StageKind::Vertex)
            && self.fs.as_deref().map_or(true, |f| kind(f) == Some(StageKind::Fragment))
            && f32::from_bits(self.point_size).is_finite()
            && pf_ok(self.depth_pf)
            && self.color_pfs.iter().all(|&f| pf_ok(f))
            && self.blends.iter().all(|&b| blend_ok(b))
            && self.attr_bits.len() <= 32
            && self.streams.len() <= 32
    }
}

/// The header line lists start with.
pub fn header(title_id: u64, build: &str) -> String {
    format!("# nxport pipelines title {title_id:016x} build {build}")
}

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
        if let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) {
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

#[cfg(test)]
mod tests {
    use super::Entry;

    #[test]
    fn line_round_trip() {
        let e = Entry {
            vs: "s_7db879f41611c1ae".into(),
            fs: None,
            point_size: 1.0f32.to_bits(),
            depth_gl: true,
            y_negate: false,
            color_state: 1,
            channel_mask: 0xffff_ffff,
            depth_pf: 252,
            color_pfs: [71, 0, 0, 0, 0, 0, 0, 0],
            blends: [0x6401_1185_1850, 1, 2, 3, 4, 5, 6, 7],
            attr_bits: vec![Some(0x520), None],
            streams: vec![],
        };
        assert!(Entry::parse(&e.line()) == Some(e.clone()));
        assert!(Entry::parse("p1 s_x - 0").is_none());
    }
}
