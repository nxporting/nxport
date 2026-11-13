//! Render-pass grouping from a look-ahead over a submit's commands. The executor ends a Metal render pass whenever the
//! game binds other render targets; every pass end stores its attachments and the next pass on them loads them again
//! (Hateno: ~890 MB per frame, ~2.5 ms of GPU time). A group is a run of render-target binds whose draws could share
//! one pass: same size and sample count, at most 8 colour attachments and one depth in their union, and no command in
//! between that needs the earlier results stored (compute, copies, texture / buffer clears).

use crate::nvn::api::{PassEvent, RecordedCommandList};
use std::sync::Arc;

/// A render target: storage address, level, layer.
pub(super) type Key = (usize, u16, u16);

pub(super) struct Bind {
    pub colors: smallvec::SmallVec<[(u8, Key); 8]>,
    pub depth: Option<Key>,
    pub size: (u32, u32),
    pub samples: u32,
    /// Draw runs and clears under this bind (until the next one).
    pub draws: u32,
    pub clears: u32,
    /// What came between the previous bind and this one that needs stored results.
    pub break_before: Option<&'static str>,
    /// The game call chain that recorded the list holding the bind (--log-frame-map), and when.
    pub recorder: Option<([u32; 16], u64)>,
}

pub(super) struct Group {
    pub first: usize,
    pub last: usize,
    /// Union of the binds' colour targets with the slot each gets.
    pub colors: smallvec::SmallVec<[(u8, Key); 8]>,
    pub depth: Option<Key>,
    /// Binds whose targets sit at other slots than the game's (their fragment outputs need moving).
    pub remapped: usize,
}

fn key(r: &crate::nvn::cmd::RtRef) -> Option<(Key, (u32, u32), u32)> {
    let addr = crate::nvn::api::resolve_texture_ref(r.texture, r.gen);
    let (_, t) = unsafe { super::tex_info(addr) }?;
    let level = r.level as u32;
    let size = (((t.width as u32) >> level).max(1), ((t.height as u32) >> level).max(1));
    Some(((t.storage, r.level, r.layer), size, (t.samples as u32).max(1)))
}

/// The submit's render-target binds in execution order.
pub(super) fn scan(
    lists: &rustc_hash::FxHashMap<u64, Arc<RecordedCommandList>>,
    handles: &[u64],
) -> Vec<Bind> {
    let mut binds = Vec::new();
    let mut pending_break = None;
    for &h in handles {
        walk(lists, h, 0, &mut binds, &mut pending_break);
    }
    binds
}

fn walk(
    lists: &rustc_hash::FxHashMap<u64, Arc<RecordedCommandList>>,
    handle: u64,
    depth: u32,
    binds: &mut Vec<Bind>,
    pending_break: &mut Option<&'static str>,
) {
    if depth > 8 {
        return;
    }
    let Some(list) = lists.get(&handle) else { return };
    for e in list.pass_events() {
        match e {
            PassEvent::Targets(colors, d, index) => {
                let mut b = Bind {
                    colors: Default::default(),
                    depth: None,
                    size: (0, 0),
                    samples: 1,
                    draws: 0,
                    clears: 0,
                    break_before: pending_break.take(),
                    recorder: list
                        .recorder
                        .as_deref()
                        .and_then(|r| r.iter().find(|(i, _)| i == index))
                        .map(|(_, c)| (*c, list.recorded_frame)),
                };
                for (i, c) in colors.iter().enumerate() {
                    if c.texture == 0 {
                        continue;
                    }
                    if let Some((k, size, samples)) = key(c) {
                        b.colors.push((i as u8, k));
                        (b.size, b.samples) = (size, samples);
                    }
                }
                if let Some((k, size, samples)) = d.as_ref().filter(|d| d.texture != 0).and_then(key) {
                    b.depth = Some(k);
                    if b.colors.is_empty() {
                        (b.size, b.samples) = (size, samples);
                    }
                }
                binds.push(b);
            }
            PassEvent::Draw => {
                if let Some(b) = binds.last_mut() {
                    b.draws += 1;
                }
            }
            PassEvent::Clear => {
                if let Some(b) = binds.last_mut() {
                    b.clears += 1;
                }
            }
            PassEvent::Break(what) => *pending_break = Some(what),
            PassEvent::Call(h) => walk(lists, *h, depth + 1, binds, pending_break),
        }
    }
}

pub(super) fn groups(binds: &[Bind]) -> Vec<Group> {
    let mut out: Vec<Group> = Vec::new();
    for (i, b) in binds.iter().enumerate() {
        if b.colors.is_empty() && b.depth.is_none() {
            continue;
        }
        let joined = out.last_mut().is_some_and(|g| {
            let first = &binds[g.first];
            if b.break_before.is_some() || b.size != first.size || b.samples != first.samples {
                return false;
            }
            if b.depth.is_some() && g.depth.is_some() && b.depth != g.depth {
                return false;
            }
            let mut colors = g.colors.clone();
            let mut remapped = false;
            for &(slot, k) in &b.colors {
                if let Some(&(s, _)) = colors.iter().find(|(_, x)| *x == k) {
                    remapped |= s != slot;
                    continue;
                }
                let free = |s: u8| !colors.iter().any(|(x, _)| *x == s);
                let s = if free(slot) {
                    slot
                } else if let Some(s) = (0..8u8).find(|&s| free(s)) {
                    remapped = true;
                    s
                } else {
                    return false;
                };
                colors.push((s, k));
            }
            g.colors = colors;
            g.depth = g.depth.or(b.depth);
            g.last = i;
            g.remapped += remapped as usize;
            true
        });
        if !joined {
            out.push(Group { first: i, last: i, colors: b.colors.clone(), depth: b.depth, remapped: 0 });
        }
    }
    out
}

/// --log-pass-graph: the plan of the submit.
pub(super) fn log(binds: &[Bind], groups: &[Group]) {
    println!("[pg] plan {} binds, {} groups", binds.len(), groups.len());
    for (i, b) in binds.iter().enumerate() {
        let c: Vec<String> = b.colors.iter().map(|(s, k)| format!("c{}={:x}", s, k.0)).collect();
        println!(
            "[pg]   bind {} {}x{} {} d={} draws {} clears {}{}",
            i,
            b.size.0,
            b.size.1,
            c.join(" "),
            b.depth.map_or("-".to_string(), |k| format!("{:x}", k.0)),
            b.draws,
            b.clears,
            b.break_before.map_or(String::new(), |w| format!(" after {}", w))
        );
        if let Some((r, f)) = &b.recorder {
            let c: Vec<String> = r.iter().filter(|&&a| a != 0).map(|a| format!("{:x}", a)).collect();
            println!("[pg]     recorded f{} by {}", f, c.join(" "));
        }
    }
    for (n, g) in groups.iter().enumerate() {
        let draws: u32 = binds[g.first..=g.last].iter().map(|b| b.draws).sum();
        let first = &binds[g.first];
        println!(
            "[pg]   group {} binds {}..{} ({} binds, {} draw runs) {}x{} {} colours{} remapped binds {}{}",
            n,
            g.first,
            g.last,
            g.last - g.first + 1,
            draws,
            first.size.0,
            first.size.1,
            g.colors.len(),
            if g.depth.is_some() { " + depth" } else { "" },
            g.remapped,
            first.break_before.map_or(String::new(), |w| format!(" (after {})", w)),
        );
    }
}
