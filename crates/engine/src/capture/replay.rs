// Runs a frame capture on the installed backend and records or checks its render passes.
//
// Guest memory goes back to its addresses, the textures only the GPU held are written into it in the guest's layout
// (where the Switch's GPU would have left them, so any backend uploads them like other guest data), and the frontend
// state and command lists are installed as the capturing run had them. The frame's command stream is then flattened
// (CallCommands inlined) and cut into passes: a pass is a run of commands with the same color targets bound. Each pass
// is submitted on its own and, once the GPU has finished it, every color target it drew or cleared is read back. In NVN
// terms nothing changes between those submits, so the results must not depend on where a backend cuts its own work.
//
// Recording (Options::record) writes the results as the capture's references (ref/); checking compares a run against
// them, pass by pass, and stops at the first pass that differs, with a diff image of each differing target.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::format::{self, Frame, GpuTexture, Op, SeqReader, SeqWriter};
use super::image::{self, Tolerance};
use crate::gpu::Texels;
use crate::nvn::cmd::{Cmd, RtRef};

#[derive(Clone, Debug, Default)]
pub struct Options {
    /// Write the results as the capture's references instead of checking against them.
    pub record: bool,
    /// Where diff images go (default: the capture's diff/).
    pub out: Option<PathBuf>,
    /// Check every pass instead of stopping at the first that differs.
    pub all: bool,
    /// Where the references are (default: the capture's ref/).
    pub refs: Option<PathBuf>,
    /// Also write every target read back as a PNG (to `out`).
    pub images: bool,
    /// The backend to run on: "metal" (default where there is Metal) or "vulkan".
    pub backend: String,
    /// Print the commands of these passes.
    pub trace_passes: Vec<usize>,
    /// (pass, k): run pass `pass` only up to its k-th draw (counting from 1) and stop there (finding the draw where two
    /// backends part: record on one, check on the other, bisect k).
    pub cut_draw: Option<(usize, u32)>,
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    pub passes: usize,
    pub targets: usize,
    /// First failing pass (index, description), when checking.
    pub first_failure: Option<(usize, String)>,
    pub failures: usize,
    /// Notes worth reading (unreadable targets, structure changes, the replay against the capturing run's image).
    pub notes: Vec<String>,
}

/// One pass as the references describe it.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct PassInfo {
    /// Flattened command range [first, end).
    first: usize,
    end: usize,
    draws: u32,
    clears: u32,
    /// Bound color targets read back: slot and description.
    targets: Vec<(u32, String)>,
}

#[derive(Serialize, Deserialize)]
struct References {
    version: u32,
    passes: Vec<PassInfo>,
}

/// A pass to run: its command range, its color targets, and whether it draws or clears into them.
struct Pass {
    first: usize,
    end: usize,
    colors: Vec<RtRef>,
    draws: u32,
    clears: u32,
}

fn flatten(
    lists: &std::collections::HashMap<u64, Vec<Op>>,
    h: u64,
    depth: u32,
    out: &mut Vec<Op>,
    missing: &mut u32,
) {
    let Some(ops) = lists.get(&h) else {
        *missing += 1;
        return;
    };
    for op in ops {
        match &op.typed {
            Some(Cmd::CallCommands(hs)) if depth < 16 => {
                for &c in hs.iter() {
                    flatten(lists, c, depth + 1, out, missing);
                }
            }
            _ => out.push(op.clone()),
        }
    }
}

fn same_targets(a: &[RtRef], b: &[RtRef]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            (x.texture, x.gen, x.level, x.layer, x.view_format)
                == (y.texture, y.gen, y.level, y.layer, y.view_format)
        })
}

fn passes(ops: &[Op]) -> Vec<Pass> {
    let mut out: Vec<Pass> = Vec::new();
    let mut cur = Pass {
        first: 0,
        end: 0,
        colors: Vec::new(),
        draws: 0,
        clears: 0,
    };
    for (i, op) in ops.iter().enumerate() {
        match &op.typed {
            Some(Cmd::SetRenderTargets { colors, .. }) if !same_targets(colors, &cur.colors) => {
                cur.end = i;
                if cur.end > cur.first {
                    out.push(std::mem::replace(
                        &mut cur,
                        Pass {
                            first: i,
                            end: i,
                            colors: colors.clone(),
                            draws: 0,
                            clears: 0,
                        },
                    ));
                } else {
                    cur.colors = colors.clone();
                }
            }
            Some(
                Cmd::DrawArrays { .. }
                | Cmd::DrawElements { .. }
                | Cmd::DrawArraysInstanced { .. }
                | Cmd::DrawElementsInstanced { .. },
            ) => cur.draws += 1,
            Some(Cmd::ClearColor { .. }) => cur.clears += 1,
            _ => {}
        }
    }
    cur.end = ops.len();
    if cur.end > cur.first {
        out.push(cur);
    }
    out
}

fn describe(slot: usize, rt: &RtRef, t: Option<&Texels>) -> String {
    match t {
        Some(t) => format!(
            "c{} {:#x} level {} layer {} {}x{} fmt {}",
            slot, rt.texture, rt.level, rt.layer, t.width, t.height, t.format
        ),
        None => format!(
            "c{} {:#x} level {} layer {} (not readable)",
            slot, rt.texture, rt.level, rt.layer
        ),
    }
}

/// Maps the capture's memory at its addresses and fills it.
fn map_memory(dir: &Path) -> Result<(usize, u64), String> {
    let mut ranges = 0;
    let mut bytes = 0u64;
    format::read_memory(
        &dir.join("memory.bin"),
        |start, len| unsafe {
            crate::os::reserve(crate::os::At::Exactly(start as usize), len as usize)
                .map_err(|e| format!("memory at {start:#x} (+{len:#x}): {e}"))?;
            crate::os::commit(start as usize, len as usize, crate::os::Prot::ReadWrite)
                .map_err(|e| format!("memory at {start:#x} (+{len:#x}): {e}"))?;
            ranges += 1;
            Ok(())
        },
        |addr, page| {
            unsafe { std::ptr::copy_nonoverlapping(page.as_ptr(), addr as *mut u8, page.len()) };
            bytes += page.len() as u64;
            Ok(())
        },
    )?;
    Ok((ranges, bytes))
}

/// Writes the textures only the GPU held into guest memory, oldest first (where they share memory, the texture written
/// last is what memory holds). Returns (written, skipped).
fn restore_gpu_textures(dir: &Path) -> Result<(usize, usize), String> {
    let mut all: Vec<GpuTexture> = Vec::new();
    let mut r = SeqReader::open(&dir.join("textures.bin"))?;
    while let Some(t) = r.next::<GpuTexture>()? {
        all.push(t);
    }
    all.sort_by_key(|t| t.last_written);
    let (mut written, mut skipped) = (0, 0);
    for t in &all {
        let p = &t.params;
        let layout = crate::nvn::layout::compute_layout(p);
        let ok = unsafe {
            if p.target == crate::nvn::layout::NVN_TARGET_3D {
                crate::nvn::layout::write_texels_3d_slice(
                    t.storage as *mut u8,
                    p,
                    t.level as usize,
                    t.layer as usize,
                    t.texels.data.as_ptr(),
                )
            } else {
                crate::nvn::layout::write_texels_2d(
                    t.storage as *mut u8,
                    t.layer as usize * layout.layer_stride as usize,
                    p,
                    t.level as usize,
                    0,
                    0,
                    t.texels.width as i32,
                    t.texels.height as i32,
                    t.texels.data.as_ptr(),
                )
            }
        };
        if ok {
            written += 1;
        } else {
            skipped += 1;
        }
    }
    Ok((written, skipped))
}

/// Runs the capture in `dir` on the installed backend. Must be called once per process, before anything else used
/// the frontend or the backend.
pub fn run(dir: &Path, opts: &Options) -> Result<Report, String> {
    let frame: Frame = format::read_bin(&dir.join("frame.bin"))?;
    if frame.version != format::VERSION {
        return Err(format!(
            "{}: capture version {}, this build reads {}",
            dir.display(),
            frame.version,
            format::VERSION
        ));
    }
    let tol: Tolerance = match std::fs::read_to_string(dir.join("tolerance.toml")) {
        Ok(s) => toml::from_str(&s).map_err(|e| format!("tolerance.toml: {e}"))?,
        Err(_) => Tolerance::default(),
    };
    let t0 = std::time::Instant::now();
    let (ranges, bytes) = map_memory(dir)?;
    let (restored, not_restored) = restore_gpu_textures(dir)?;
    println!(
        "[gpu-replay] {}: frame {} of {}; memory {} ranges, {} MB; {} GPU-only texture subresources restored ({} not); {:.1} s",
        dir.display(),
        frame.frame,
        frame.title,
        ranges,
        bytes >> 20,
        restored,
        not_restored,
        t0.elapsed().as_secs_f64()
    );
    let lists: std::collections::HashMap<u64, Vec<Op>> = frame.lists.into_iter().collect();
    for (h, ops) in &lists {
        crate::nvn::api::insert_command_list(*h, ops.clone());
    }
    crate::nvn::api::import_frontend(&frame.frontend);
    let programs = frame.frontend.programs.clone();

    let mut ops = Vec::new();
    let mut missing = 0;
    for submit in &frame.submits {
        for &h in submit {
            flatten(&lists, h, 0, &mut ops, &mut missing);
        }
    }
    let mut passes = passes(&ops);
    if let Some((cp, k)) = opts.cut_draw {
        passes.truncate(cp + 1);
        if let Some(p) = passes.get_mut(cp) {
            let mut seen = 0;
            for i in p.first..p.end {
                if matches!(
                    ops[i].typed,
                    Some(Cmd::DrawArrays { .. } | Cmd::DrawElements { .. } | Cmd::DrawArraysInstanced { .. } | Cmd::DrawElementsInstanced { .. })
                ) {
                    seen += 1;
                    if seen == k {
                        p.end = i + 1;
                        break;
                    }
                }
            }
            p.draws = seen.min(k);
        }
    }
    let mut report = Report {
        passes: passes.len(),
        ..Default::default()
    };
    if missing > 0 {
        report.notes.push(format!(
            "{missing} called command lists missing from the capture"
        ));
    }

    let refdir = opts.refs.clone().unwrap_or_else(|| dir.join("ref"));
    let (mut writer, mut reader, refs) = if opts.record {
        std::fs::create_dir_all(&refdir).map_err(|e| format!("{}: {e}", refdir.display()))?;
        (
            Some(SeqWriter::create(&refdir.join("images.bin"))?),
            None,
            None,
        )
    } else {
        let refs: References = format::read_bin(&refdir.join("passes.bin"))
            .map_err(|e| format!("{e} (record references first)"))?;
        if refs.passes.len() != passes.len() {
            report.notes.push(format!(
                "{} passes, the references have {}",
                passes.len(),
                refs.passes.len()
            ));
        }
        (
            None,
            Some(SeqReader::open(&refdir.join("images.bin"))?),
            Some(refs),
        )
    };
    let out = opts.out.clone().unwrap_or_else(|| dir.join("diff"));
    let gpu = crate::gpu::backend();
    let mut infos = Vec::new();
    let mut handle = 1u64 << 62;
    let t1 = std::time::Instant::now();
    for (pi, p) in passes.iter().enumerate() {
        handle += 1;
        if opts.trace_passes.contains(&pi) {
            // The state it inherits: the latest command of each kind (and binding slot) before it.
            let mut latest: std::collections::BTreeMap<String, (usize, String)> = Default::default();
            for (k, op) in ops[..p.first].iter().enumerate() {
                if let Some(c) = &op.typed {
                    let d = format!("{c:?}");
                    let kind = d.split([' ', '(', '{']).next().unwrap_or("").to_string();
                    if kind.starts_with("Draw") || kind.starts_with("Clear") || kind.starts_with("Copy") || kind == "Barrier" {
                        continue;
                    }
                    let slot = match c {
                        Cmd::BindTexture { stage, index, .. }
                        | Cmd::BindImage { stage, index, .. }
                        | Cmd::BindUniformBuffer { stage, index, .. }
                        | Cmd::BindStorageBuffer { stage, index, .. } => format!(" {stage}/{index}"),
                        Cmd::BindVertexBuffer { index, .. } => format!(" {index}"),
                        Cmd::BindBlendState(b) => format!(" {}", b & 0xF),
                        _ => String::new(),
                    };
                    latest.insert(kind + &slot, (k, d));
                }
            }
            println!("[gpu-replay] pass {pi}: state from earlier passes");
            for (k, (at, d)) in &latest {
                println!("[gpu-replay]   {at:5} {k}: {}", d.chars().take(300).collect::<String>());
            }
            for op in ops[..p.end].iter().rev() {
                if let Some(Cmd::BindProgram { program, stages: bits }) = &op.typed {
                    let stages = programs.iter().find(|(a, _)| *a == *program as u64).map(|(_, s)| s.clone());
                    println!("[gpu-replay]   program {program:#x} (stage bits {bits:#x}) stages (code, control): {stages:x?}");
                    if bits & 0x1F != 0 {
                        break;
                    }
                }
            }
            println!("[gpu-replay] pass {pi}: commands {}..{}", p.first, p.end);
            for (k, op) in ops[p.first..p.end].iter().enumerate() {
                match &op.typed {
                    Some(c) => println!("[gpu-replay]   {:5} {:?}", p.first + k, c),
                    None => println!("[gpu-replay]   {:5} {} {:x?}", p.first + k, op.name, op.args),
                }
            }
        }
        crate::nvn::api::insert_command_list(handle, ops[p.first..p.end].to_vec());
        gpu.submit(&[handle]);
        gpu.wait_serial(gpu.submitted_serial(), u64::MAX);
        let mut info = PassInfo {
            first: p.first,
            end: p.end,
            draws: p.draws,
            clears: p.clears,
            targets: Vec::new(),
        };
        if p.draws + p.clears > 0 {
            for (slot, rt) in p.colors.iter().enumerate() {
                if rt.texture == 0 {
                    continue;
                }
                let img = gpu.read_texture(rt.texture, rt.gen, rt.level as u32, rt.layer as u32);
                let desc = describe(slot, rt, img.as_ref());
                report.targets += 1;
                if let (true, Some(t)) = (opts.images, &img) {
                    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
                    image::write_png(&out.join(format!("pass{pi:04}_c{slot}_out.png")), t.width, t.height, &image::to_rgba8(t))?;
                }
                if let Some(w) = writer.as_mut() {
                    w.push(&img)?;
                } else if let Some(r) = reader.as_mut() {
                    let reference: Option<Texels> = r.next::<Option<Texels>>()?.flatten();
                    let failed = match (&reference, &img) {
                        (Some(a), Some(b)) => {
                            let d = image::compare(a, b, &tol);
                            if d.passes(&tol) {
                                None
                            } else {
                                std::fs::create_dir_all(&out)
                                    .map_err(|e| format!("{}: {e}", out.display()))?;
                                let (w, h, rgba) = image::diff_image(a, b, &tol);
                                let path = out.join(format!("pass{pi:04}_c{slot}.png"));
                                image::write_png(&path, w, h, &rgba)?;
                                Some(format!(
                                    "{} pixels of {} differ (max {:.4}, per channel {:.3?}, by size {:?}, box {:?}); {}",
                                    d.differing,
                                    d.pixels,
                                    d.max,
                                    d.channel_max,
                                    d.histogram,
                                    d.bbox,
                                    path.display()
                                ))
                            }
                        }
                        (None, None) => None,
                        (Some(_), None) => Some("not readable in this run".to_string()),
                        (None, Some(_)) => None,
                    };
                    if let Some(why) = failed {
                        report.failures += 1;
                        let what = format!(
                            "pass {pi} ({} draws, {} clears) {desc}: {why}",
                            p.draws, p.clears
                        );
                        println!("[gpu-replay] DIFFERS {what}");
                        if report.first_failure.is_none() {
                            report.first_failure = Some((pi, what));
                        }
                    }
                }
                info.targets.push((slot as u32, desc));
            }
        }
        infos.push(info);
        if report.first_failure.is_some() && !opts.all {
            break;
        }
    }
    let _ = refs;

    // The presented image: against the references, and (recording) against what the capturing run showed.
    let present = frame.present.and_then(|(t, _)| {
        gpu.read_texture(
            t as usize,
            crate::nvn::api::texture_ref_gen(t as usize),
            0,
            0,
        )
    });
    if let Some(w) = writer {
        w.finish()?;
        format::write_bin(
            &refdir.join("passes.bin"),
            &References {
                version: format::VERSION,
                passes: infos,
            },
        )?;
        if let Some(img) = &present {
            format::write_bin(&refdir.join("present.bin"), img)?;
            if let Ok(live) = format::read_bin::<Texels>(&dir.join("presented.bin")) {
                let d = image::compare(&live, img, &tol);
                report.notes.push(format!(
                    "replay vs the capturing run's presented image: {} of {} pixels differ (max {:.4})",
                    d.differing, d.pixels, d.max
                ));
                if d.differing > 0 {
                    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
                    let (w, h, rgba) = image::diff_image(&live, img, &tol);
                    image::write_png(&out.join("present_vs_live.png"), w, h, &rgba)?;
                }
            }
        }
    } else if report.first_failure.is_none() || opts.all {
        if let (Some(img), Ok(reference)) = (
            &present,
            format::read_bin::<Texels>(&refdir.join("present.bin")),
        ) {
            let d = image::compare(&reference, img, &tol);
            if !d.passes(&tol) {
                report.failures += 1;
                std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
                let (w, h, rgba) = image::diff_image(&reference, img, &tol);
                image::write_png(&out.join("present.png"), w, h, &rgba)?;
                let what = format!(
                    "presented image: {} of {} pixels differ (max {:.4})",
                    d.differing, d.pixels, d.max
                );
                println!("[gpu-replay] DIFFERS {what}");
                report.first_failure.get_or_insert((passes.len(), what));
            }
        }
    }
    println!(
        "[gpu-replay] {} passes, {} targets {} in {:.1} s{}",
        report.passes,
        report.targets,
        if opts.record { "recorded" } else { "checked" },
        t1.elapsed().as_secs_f64(),
        match &report.first_failure {
            Some((i, _)) => format!("; {} differ, first at pass {}", report.failures, i),
            None if opts.record => String::new(),
            None => "; all match".to_string(),
        }
    );
    for n in &report.notes {
        println!("[gpu-replay] {n}");
    }
    Ok(report)
}
