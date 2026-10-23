// --capture-frame=N: writes presented frame N as a capture (format.rs) when the game presents it.
//
// The frame is captured as it stands after its last submit: the game's thread that waits for the frame's fence is
// held until the capture is written (as --replay holds it), the GPU is drained, and guest memory then holds what the
// frame's commands read, the CPU's writes for this frame and the GPU's results alike. Replaying from there reproduces
// the frame (--replay re-runs frames from the same point). Inline uniform updates are the exception: memory has the
// last update, so a draw that read a range before the frame updated it reads the newer data in a replay.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use super::format::{self, Frame, GpuTexture, MemoryWriter, SeqWriter, PAGE};
use crate::nvn::cmd::Cmd;

static SUBMITS: Mutex<Vec<Vec<u64>>> = Mutex::new(Vec::new());
/// Set from the frame's first submit until its capture is written: fence waits for its work are held.
static CAPTURING: AtomicBool = AtomicBool::new(false);
/// Fence serials above this belong to the captured frame.
static HOLD_ABOVE: AtomicU64 = AtomicU64::new(u64::MAX);
/// The thread that submits (and presents): never held.
static SUBMITTER: Mutex<Option<std::thread::ThreadId>> = Mutex::new(None);

fn target() -> Option<u64> {
    crate::settings::get().replay.capture_frame
}

/// nvnQueueSubmitCommands, before the backend runs the lists.
pub(crate) fn on_submit(handles: &[u64]) {
    let Some(n) = target() else { return };
    let gpu = crate::gpu::backend();
    if gpu.presented_frames() + 1 != n {
        return;
    }
    let mut s = SUBMITS.lock().unwrap();
    if s.is_empty() {
        HOLD_ABOVE.store(gpu.submitted_serial(), Ordering::Release);
        *SUBMITTER.lock().unwrap() = Some(std::thread::current().id());
        CAPTURING.store(true, Ordering::Release);
    }
    s.push(handles.to_vec());
}

/// nvnSyncWait: a wait for the captured frame's work returns once the capture is written, so the game does not move on
/// and change the memory being captured.
pub(crate) fn hold(serial: u64) {
    if !CAPTURING.load(Ordering::Acquire) || serial <= HOLD_ABOVE.load(Ordering::Acquire) {
        return;
    }
    if *SUBMITTER.lock().unwrap() == Some(std::thread::current().id()) {
        return;
    }
    while CAPTURING.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// nvnQueuePresentTexture, before the backend presents.
pub(crate) fn on_present(texture: usize, crop: [i32; 4]) {
    let Some(n) = target() else { return };
    if crate::gpu::backend().presented_frames() + 1 != n {
        return;
    }
    let submits = std::mem::take(&mut *SUBMITS.lock().unwrap());
    let dir = crate::settings::get()
        .replay
        .capture_dir
        .clone()
        .or_else(|| crate::profile::data_subdir("captures").map(|d| d.join(format!("frame_{n}"))))
        .unwrap_or_else(|| PathBuf::from(format!("capture_frame_{n}")));
    let t0 = std::time::Instant::now();
    match write(&dir, n, &submits, texture, crop) {
        Ok(summary) => println!(
            "[capture] frame {} written to {} in {:.1} s: {}",
            n,
            dir.display(),
            t0.elapsed().as_secs_f64(),
            summary
        ),
        Err(e) => eprintln!("[capture] frame {n}: {e}"),
    }
    CAPTURING.store(false, Ordering::Release);
    if crate::settings::get().replay.capture_exit {
        std::process::exit(0);
    }
}

/// Guest memory a command reads or writes, beyond what textures and pools cover: (address, length).
fn cmd_ranges(c: &Cmd, out: &mut Vec<(u64, u64)>) {
    let mut add = |a: u64, len: u64| {
        if a > 0x10000 && len > 0 {
            let a = crate::nvn::api::translate_virtual(a as usize) as u64;
            out.push((a, a + len));
        }
    };
    match c {
        Cmd::BindVertexBuffer { addr, size, .. }
        | Cmd::BindUniformBuffer { addr, size, .. }
        | Cmd::BindStorageBuffer { addr, size, .. } => add(*addr, *size as u64),
        Cmd::UpdateUniformBuffer { addr, offset, data } => {
            add((*addr as i64 + *offset as i64) as u64, data.len() as u64)
        }
        Cmd::DrawElements { index_addr, count, .. } | Cmd::DrawElementsInstanced { index_addr, count, .. } => {
            add(*index_addr, *count as u64 * 4)
        }
        Cmd::ReportCounter { addr, .. } | Cmd::ReportValue { addr, .. } => add(*addr, 16),
        Cmd::SetRenderEnableConditional { counters, .. } => add(*counters, 24),
        Cmd::BindTexturesDeferred { handles, count, .. } => add(*handles, *count as u64 * 8),
        Cmd::CopyBufferToBuffer { src, dst, size } => {
            add(*src, *size as u64);
            add(*dst, *size as u64);
        }
        Cmd::ClearBuffer { dst, size, .. } => add(*dst, *size as u64),
        _ => {}
    }
}

fn write(dir: &Path, n: u64, submits: &[Vec<u64>], present: usize, crop: [i32; 4]) -> Result<String, String> {
    let gpu = crate::gpu::backend();
    gpu.wait_serial(gpu.submitted_serial(), u64::MAX);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let roots: Vec<u64> = submits.iter().flatten().copied().collect();
    let lists = crate::nvn::api::export_lists(&roots);
    let frontend = crate::nvn::api::export_frontend();

    // Memory: every pool, program control sections, and whatever commands name outside them.
    let mut ranges: Vec<(u64, u64)> = frontend.pools.iter().map(|&(b, s, _)| (b, b + s)).collect();
    for (_, stages) in &frontend.programs {
        for &(_, control) in stages {
            if control > 0x10000 {
                let len = unsafe { (*((control + 0x20) as *const u32) as u64).clamp(0x400, 0x4000) };
                ranges.push((control, control + len));
            }
        }
    }
    let mut named = Vec::new();
    for (_, ops) in &lists {
        for op in ops {
            if let Some(c) = &op.typed {
                cmd_ranges(c, &mut named);
            }
        }
    }
    let pooled = format::merge_ranges(ranges.clone());
    let inside = |a: u64, b: u64| {
        let i = pooled.partition_point(|r| r.0 <= a);
        i > 0 && pooled[i - 1].1 >= b
    };
    let outside: Vec<(u64, u64)> = named.into_iter().filter(|&(a, b)| !inside(a, b)).collect();
    let n_outside = outside.len();
    ranges.extend(outside);
    let ranges = format::merge_ranges(ranges);

    // The presented image, as this run rendered it.
    if let Some(img) = gpu.read_texture(present, crate::nvn::api::texture_ref_gen(present), 0, 0) {
        format::write_bin(&dir.join("presented.bin"), &img)?;
    }

    // Textures only the GPU holds, inside the captured memory.
    let covered = |a: u64, len: u64| {
        let i = ranges.partition_point(|r| r.0 <= a);
        i > 0 && ranges[i - 1].1 >= a + len
    };
    let mut tw = SeqWriter::create(&dir.join("textures.bin"))?;
    let (mut n_tex, mut tex_bytes, mut outside_tex) = (0usize, 0usize, 0usize);
    let mut err = Ok(());
    let skipped = gpu.gpu_only_textures(&mut |t| {
        let len = crate::nvn::layout::compute_layout(&t.params).total;
        if !covered(t.storage as u64, len) {
            outside_tex += 1;
            return;
        }
        n_tex += 1;
        tex_bytes += t.texels.data.len();
        if err.is_ok() {
            err = tw.push(&GpuTexture {
                storage: t.storage as u64,
                params: t.params,
                level: t.level,
                layer: t.layer,
                last_written: t.last_written,
                texels: t.texels,
            });
        }
    });
    err?;
    tw.finish()?;

    let mut mw = MemoryWriter::create(&dir.join("memory.bin"), ranges.len())?;
    let mut unreadable = 0u64;
    for &(s, e) in &ranges {
        mw.begin_range(s, e - s)?;
        for p in (s..e).step_by(PAGE) {
            let ok = unsafe { crate::cpu::crash_handler::readable_len(p as usize, PAGE) } == PAGE;
            if ok {
                mw.page(Some(unsafe { std::slice::from_raw_parts(p as *const u8, PAGE) }))?;
            } else {
                unreadable += 1;
                mw.page(None)?;
            }
        }
    }
    let (mem_bytes, zero_pages) = (mw.bytes, mw.zero_pages);
    mw.finish()?;

    let n_lists = lists.len();
    let n_ops: usize = lists.iter().map(|(_, o)| o.len()).sum();
    let frame = Frame {
        version: format::VERSION,
        title: crate::game::title_id().map(|t| format!("{t:016x}")).unwrap_or_default(),
        frame: n,
        frontend,
        submits: submits.to_vec(),
        lists,
        present: Some((present as u64, crop)),
    };
    format::write_bin(&dir.join("frame.bin"), &frame)?;
    Ok(format!(
        "{} submits, {} lists, {} commands; memory {} ranges, {} MB in pages ({} zero, {} unreadable; {} command ranges outside pools); {} GPU-only texture subresources ({} MB; {} outside captured memory, {} not readable)",
        submits.len(),
        n_lists,
        n_ops,
        ranges.len(),
        mem_bytes >> 20,
        zero_pages,
        unreadable,
        n_outside,
        n_tex,
        tex_bytes >> 20,
        outside_tex,
        skipped
    ))
}
