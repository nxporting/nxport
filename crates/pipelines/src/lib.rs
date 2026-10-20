// Render pipeline lists: the draw state that selects each render pipeline a game used, without any shader code.
//
// An entry names its shaders by shader cache entry (`s_<hash of the stage's code>`) and holds the draw's NVN vertex
// attribute, stream and blend bits and its target formats; the engine derives the Metal pipeline from them as it does
// for a draw, so lists stay valid across engine builds. One entry per line:
//   p1 <vs> <fs|-> <point size bits> <depth_gl> <y_negate> <color state> <channel mask> <depth format>
//      <8 color formats> <8 blend states> <attribute bits> <stream bits>
// Lists are comma separated ('-' an unset attribute, '.' an empty list); formats are MTLPixelFormat values.
// A list starts with `# nxport pipelines title <title id> build <main build id>`.

use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    Vertex,
    Fragment,
    Other,
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

fn is_entry_name(s: &str) -> bool {
    s.len() == 18 && s.starts_with("s_") && s[2..].bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
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
        let name = |s: &str| is_entry_name(s).then(|| s.to_string());
        let flag = |s: &str| match s {
            "0" => Some(false),
            "1" => Some(true),
            _ => None,
        };
        Some(Entry {
            vs: name(vs)?,
            fs: if *fs == "-" { None } else { Some(name(fs)?) },
            point_size: u32::try_from(hex(ps)?).ok()?,
            depth_gl: flag(dg)?,
            y_negate: flag(yn)?,
            color_state: u32::try_from(hex(cs)?).ok()?,
            channel_mask: u32::try_from(hex(cm)?).ok()?,
            depth_pf: dpf.parse().ok()?,
            color_pfs: parse_list(cpfs, |p| p.parse().ok())?.try_into().ok()?,
            blends: parse_list(blends, hex)?.try_into().ok()?,
            attr_bits: parse_list(attrs, |a| {
                if a == "-" {
                    Some(None)
                } else {
                    hex(a).and_then(|a| u32::try_from(a).ok()).map(Some)
                }
            })?,
            streams: parse_list(streams, hex)?,
        })
    }

    /// Whether the entry's values are in range (formats, blend fields, list lengths) and, with `kind` (the shader
    /// cache's stage kinds by entry), whether its shaders exist with the right kinds.
    pub fn valid(&self, kind: Option<&dyn Fn(&str) -> Option<StageKind>>) -> bool {
        let pf_ok = |f: u64| f < 1024;
        // NVNblendState's fields end at bit 47.
        let blend_ok = |b: u64| b >> 47 == 0;
        let kinds_ok = kind.map_or(true, |kind| {
            kind(&self.vs) == Some(StageKind::Vertex)
                && self.fs.as_deref().map_or(true, |f| kind(f) == Some(StageKind::Fragment))
        });
        kinds_ok
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

/// The title id and build of a list's header.
pub fn parse_header(line: &str) -> Option<(u64, String)> {
    let mut f = line.strip_prefix("# nxport pipelines title ")?.split_whitespace();
    let title = hex(f.next()?)?;
    let build = match (f.next(), f.next()) {
        (Some("build"), Some(b)) => b.to_string(),
        _ => "-".into(),
    };
    Some((title, build))
}

/// The stage kinds of a shader cache index (index.txt text) by entry name.
pub fn stage_kinds(index_text: &str) -> HashMap<String, StageKind> {
    index_text
        .lines()
        .filter_map(|l| match l.split_whitespace().collect::<Vec<_>>().as_slice() {
            ["S", _, _, full, stage, _] => Some((
                format!("s_{full}"),
                match *stage {
                    "Vertex" => StageKind::Vertex,
                    "Fragment" => StageKind::Fragment,
                    _ => StageKind::Other,
                },
            )),
            _ => None,
        })
        .collect()
}

#[derive(Default)]
pub struct MergeOptions<'a> {
    /// Stage kinds of the shader cache entries must refer to (stage_kinds); None: shader names are not checked.
    pub kinds: Option<&'a HashMap<String, StageKind>>,
    /// Entries of a source (other than `keep`) are kept only when at least this many sources list them (0 or 1: all).
    pub min_sources: usize,
    /// Sources with more entries than this are left out entirely (0: no limit).
    pub max_per_source: usize,
}

#[derive(Default, Debug)]
pub struct Merged {
    pub entries: BTreeSet<Entry>,
    pub title: Option<u64>,
    pub build: String,
    pub read: usize,
    pub rejected: usize,
    /// Valid entries left out because too few sources listed them.
    pub pending: usize,
    /// Sources left out (too many entries, or another game's).
    pub skipped_sources: usize,
}

impl Merged {
    pub fn text(&self) -> String {
        let mut t = header(self.title.unwrap_or(0), &self.build) + "\n";
        for e in &self.entries {
            t += &e.line();
            t.push('\n');
        }
        t
    }
}

/// Merges `keep` (lists taken as they are, e.g. the published list) and `sources` (e.g. submissions) into one list
/// without duplicates or invalid entries. All lists must be of one game: the first header decides, sources of another
/// game are skipped.
pub fn merge(keep: &[&str], sources: &[&str], o: &MergeOptions) -> Merged {
    let mut m = Merged::default();
    let mut builds = BTreeSet::new();
    let kind_fn = o.kinds.map(|k| move |s: &str| k.get(s).copied());
    let kind: Option<&dyn Fn(&str) -> Option<StageKind>> = kind_fn.as_ref().map(|f| f as _);
    let mut read = |text: &str, m: &mut Merged| -> Option<BTreeSet<Entry>> {
        let mut out = BTreeSet::new();
        for line in text.lines() {
            if let Some((title, build)) = parse_header(line) {
                if *m.title.get_or_insert(title) != title {
                    return None;
                }
                builds.insert(build);
                continue;
            }
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            m.read += 1;
            match Entry::parse(line).filter(|e| e.valid(kind)) {
                Some(e) => {
                    out.insert(e);
                }
                None => m.rejected += 1,
            }
        }
        Some(out)
    };
    for text in keep {
        match read(text, &mut m) {
            Some(es) => m.entries.extend(es),
            None => m.skipped_sources += 1,
        }
    }
    let mut counts: BTreeMap<Entry, usize> = BTreeMap::new();
    for text in sources {
        match read(text, &mut m) {
            Some(es) if o.max_per_source == 0 || es.len() <= o.max_per_source => {
                for e in es {
                    *counts.entry(e).or_default() += 1;
                }
            }
            _ => m.skipped_sources += 1,
        }
    }
    for (e, n) in counts {
        if n >= o.min_sources.max(1) {
            m.entries.insert(e);
        } else if !m.entries.contains(&e) {
            m.pending += 1;
        }
    }
    m.build = if builds.len() == 1 { builds.into_iter().next().unwrap() } else { "-".into() };
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> Entry {
        Entry {
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
        }
    }

    #[test]
    fn line_round_trip() {
        let e = entry();
        assert_eq!(Entry::parse(&e.line()), Some(e));
        assert!(Entry::parse("p1 s_x - 0").is_none());
        assert!(Entry::parse(&entry().line().replace("s_7db8", "s_7DB8")).is_none());
    }

    #[test]
    fn merge_counts_sources() {
        let h = header(0x0100f2c0115b6000, "ab");
        let a = format!("{h}\n{}\n", entry().line());
        let mut e2 = entry();
        e2.color_state = 3;
        let b = format!("{h}\n{}\n{}\ngarbage\n", entry().line(), e2.line());
        let m = merge(&[], &[&a, &b], &MergeOptions { min_sources: 2, ..Default::default() });
        assert_eq!(m.entries.len(), 1);
        assert_eq!((m.pending, m.rejected), (1, 1));
        let other = format!("{}\n{}\n", header(1, "x"), e2.line());
        let m = merge(&[&a], &[&b, &other], &MergeOptions::default());
        assert_eq!((m.entries.len(), m.skipped_sources), (2, 1));
        assert_eq!(merge(&[&m.text()], &[], &MergeOptions::default()).text(), m.text());
    }
}
