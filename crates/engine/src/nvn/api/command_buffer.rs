// NVN command buffers: command and control memory, recording into typed command lists, the list registry and capture.

use super::*;

// ---------------------------------------------------------------------------
// NVN CommandBuffer family.
//
// NVNcommandBuffer (0xA0 = 160 bytes) is opaque driver storage, same freedom
// as MemoryPool. Every CommandBuffer method takes the command buffer pointer
// as its first argument (x0), and the ~130 Bind*/Set*/Draw*/Clear*/Copy*/
// Dispatch* methods all return void in the real API -- NVN defers error
// reporting to a debug callback rather than synchronous return values, so a
// generic recorder is both safe (no return-value contract to violate, unlike
// the MemoryPoolInitialize bug) and faithful to how real NVN actually works.
//
// Scope for this pass: get the lifecycle and memory accounting exactly right
// (Initialize/BeginRecording/EndRecording/memory tracking -- these DO have
// real return values and real downstream consumers, like EndRecording's
// handle feeding into nvnQueueSubmitCommands), and record every drawing/
// state command into a real per-buffer list instead of dropping it, so
// nvnQueueSubmitCommands has real data to work with. Translating that
// recorded stream into actual Metal draw calls via the compositor channel is
// a separate, larger follow-up (it's the whole other half of the engine);
// this gets the NVN-facing contract correct without faking success.
#[repr(C)]
pub(crate) struct NvnCommandBuffer {
    device: usize,
    command_used: usize,
    command_size: usize,
    control_used: usize,
    control_size: usize,
    recording: u8,
    _pad: [u8; 7],
    ops: usize, // *mut Vec<RecordedNvnCommand>, heap-boxed
}

pub(crate) static CMD_HISTOGRAM: StdMutex<std::collections::BTreeMap<&'static str, u64>> =
    StdMutex::new(std::collections::BTreeMap::new());

pub(crate) fn dump_command_histogram(frame: u64) {
    let mut h = CMD_HISTOGRAM.lock().unwrap();
    let mut v: Vec<_> = h.iter().map(|(k, c)| (*c, *k)).collect();
    v.sort_by(|a, b| b.cmp(a));
    let top: Vec<String> = v
        .iter()
        .take(25)
        .map(|(c, k)| format!("{}={}", k, c))
        .collect();
    println!(
        "[+] NVN commands recorded through frame {}: {}",
        frame,
        top.join(" ")
    );
    h.clear();
}

pub(crate) struct RecordedNvnCommand {
    pub(crate) name: &'static str,
    /// Fully decoded form (pointer arguments copied, floats captured) for commands the Metal
    /// executor understands; None for the rest (recorded uninterpreted).
    pub(crate) typed: Option<crate::nvn::cmd::Cmd>,
    /// Raw arguments x1..x5 of uninterpreted commands (traces and the no-handler warning show what the game passes);
    /// boxed so decoded commands, the vast majority, stay small.
    pub(crate) args: Option<Box<[u64; 5]>>,
}

pub(crate) struct RecordedCommandList {
    pub(crate) ops: Vec<RecordedNvnCommand>,
    /// (ranges written by UpdateUniformBuffer, handles called), computed on first use (see metal::ubo_updated_ranges).
    /// Ranges its uniform updates and reports write, the lists it calls, its SAMPLES_PASSED report addresses.
    pub(crate) summary: std::sync::OnceLock<(Vec<(u64, u64)>, Vec<u64>, Vec<u64>)>,
    /// Its render-pass events, computed on first use (see `pass_events`).
    pub(crate) pass_events: std::sync::OnceLock<Vec<PassEvent>>,
    /// (op index, call chain) of its pass-level commands (--log-frame-map).
    pub(crate) recorder: Option<Box<Vec<(u32, [u32; 16])>>>,
    /// Frames presented when it was recorded (diagnostics: replayed lists vs. lists recorded this frame).
    pub(crate) recorded_frame: u64,
}

impl RecordedCommandList {
    pub(crate) fn summary(&self) -> &(Vec<(u64, u64)>, Vec<u64>, Vec<u64>) {
        self.summary.get_or_init(|| {
            let (mut ranges, mut calls, mut samples) = (Vec::new(), Vec::new(), Vec::new());
            for op in &self.ops {
                match &op.typed {
                    Some(crate::nvn::cmd::Cmd::UpdateUniformBuffer { addr, offset, data }) => {
                        let a = (*addr as i64 + *offset as i64) as u64;
                        ranges.push((a, a + data.len() as u64));
                    }
                    // Counter and value reports are written by the executor in stream order too (metal::write_counter).
                    Some(crate::nvn::cmd::Cmd::ReportCounter { addr, counter })
                        if *addr > 0x10000 =>
                    {
                        ranges.push((*addr, *addr + 16));
                        if *counter == 1 {
                            samples.push(*addr);
                        }
                    }
                    Some(crate::nvn::cmd::Cmd::ReportValue { addr, .. }) if *addr > 0x10000 => {
                        ranges.push((*addr, *addr + 16));
                    }
                    Some(crate::nvn::cmd::Cmd::CallCommands(h)) => {
                        calls.extend_from_slice(h.as_slice())
                    }
                    Some(crate::nvn::cmd::Cmd::ModelDraw(m)) => {
                        for i in &m.items {
                            match i {
                                crate::nvn::cmd::ModelItem::Unit { list, .. } => calls.push(*list),
                                crate::nvn::cmd::ModelItem::UpdateUniform { addr, offset, data } => {
                                    let a = (*addr as i64 + *offset as i64) as u64;
                                    ranges.push((a, a + data.len() as u64));
                                }
                                crate::nvn::cmd::ModelItem::ReportCounter { counter, addr } if *addr > 0x10000 => {
                                    ranges.push((*addr, *addr + 16));
                                    if *counter == 1 {
                                        samples.push(*addr);
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            (ranges, calls, samples)
        })
    }

}

/// Drops `l` on a thread of its own (nvn-free): the last reference to a replaced list frees all its commands.
pub(crate) fn free_later(l: std::sync::Arc<RecordedCommandList>) {
    static TX: std::sync::OnceLock<Option<StdMutex<std::sync::mpsc::Sender<std::sync::Arc<RecordedCommandList>>>>> = std::sync::OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<std::sync::Arc<RecordedCommandList>>();
        std::thread::Builder::new().name("nvn-free".into()).spawn(move || while rx.recv().is_ok() {}).ok().map(|_| StdMutex::new(tx))
    });
    if let Some(tx) = tx {
        // Sent back (and dropped here) only if the thread is gone.
        let _ = tx.lock().unwrap().send(l);
    }
}

/// What a command list does that decides how its draws can share render passes, in order: render-target binds, draws
/// (consecutive ones as one), clears of the bound targets, commands that need earlier passes' results stored (compute,
/// copies, buffer / texture clears) and the lists it calls.
#[derive(Clone, Debug)]
pub(crate) enum PassEvent {
    /// The bind, with its op index in the list.
    Targets(Vec<crate::nvn::cmd::RtRef>, Option<crate::nvn::cmd::RtRef>, u32),
    Draw,
    Clear,
    Break(&'static str),
    Call(u64),
}

impl RecordedCommandList {
    pub(crate) fn pass_events(&self) -> &[PassEvent] {
        use crate::nvn::cmd::Cmd;
        self.pass_events.get_or_init(|| {
            let mut ev = Vec::new();
            for (index, op) in self.ops.iter().enumerate() {
                let e = match &op.typed {
                    Some(Cmd::SetRenderTargets { colors, depth }) => PassEvent::Targets(colors.clone(), depth.as_deref().copied(), index as u32),
                    Some(
                        Cmd::DrawArrays { .. }
                        | Cmd::DrawElements { .. }
                        | Cmd::DrawArraysInstanced { .. }
                        | Cmd::DrawElementsInstanced { .. },
                    ) => PassEvent::Draw,
                    Some(Cmd::ModelDraw(m)) => {
                        for item in &m.items {
                            match item {
                                crate::nvn::cmd::ModelItem::Unit { list, .. } => ev.push(PassEvent::Call(*list)),
                                crate::nvn::cmd::ModelItem::Draw(_) if !matches!(ev.last(), Some(PassEvent::Draw)) => {
                                    ev.push(PassEvent::Draw)
                                }
                                _ => {}
                            }
                        }
                        continue;
                    }
                    Some(Cmd::ClearColor { .. } | Cmd::ClearDepthStencil { .. }) => PassEvent::Clear,
                    Some(Cmd::DispatchCompute { .. }) => PassEvent::Break("compute"),
                    Some(
                        Cmd::CopyBufferToTexture { .. }
                        | Cmd::CopyTextureToBuffer { .. }
                        | Cmd::CopyTextureToTexture { .. }
                        | Cmd::CopyBufferToBuffer { .. },
                    ) => PassEvent::Break("copy"),
                    Some(Cmd::ClearBuffer { .. } | Cmd::ClearTexture { .. }) => PassEvent::Break("clear texture / buffer"),
                    Some(Cmd::CallCommands(h)) => {
                        ev.extend(h.iter().map(|&h| PassEvent::Call(h)));
                        continue;
                    }
                    _ => continue,
                };
                if matches!(e, PassEvent::Draw) && matches!(ev.last(), Some(PassEvent::Draw)) {
                    continue;
                }
                ev.push(e);
            }
            ev
        })
    }
}

/// The recorded list for `handle` without touching its eviction bookkeeping.
/// Visits every list reachable from `handles` through CallCommands once (under one lock: a submit reaches ~4,500 lists),
/// returning the number of distinct lists.
///
/// The walk is on the Presentation Thread before recording starts, and the lists were made on the game's threads (another
/// L3 cache on Zen 2): it is cache misses, a few per list. So it touches the map entry and the list's reference count
/// only: the entry's last use (unique to this walk, written under the lock) says it was visited, and the entry carries
/// the handles the list calls. A visit stamp in the list and its summary read there were two lines more per list:
/// TotK on Great Sky Island 1.03 -> 0.62 ms a frame for 4,460 lists (Ryzen 7 3700X).
pub(crate) fn walk_command_lists(
    handles: &[u64],
    stack: &mut Vec<u64>,
    cache: &mut rustc_hash::FxHashMap<u64, std::sync::Arc<RecordedCommandList>>,
    f: impl FnMut(&RecordedCommandList),
) -> usize {
    walk_command_lists_at(list_walk_clock(), handles, stack, cache, f)
}

/// The submitted lists `handles` into `cache`, marked as walk_command_lists marks them, without following their calls
/// (Vulkan's vk-decode fetches those, walk_command_lists_at): held from the submit on.
pub(crate) fn pin_command_lists(handles: &[u64], cache: &mut rustc_hash::FxHashMap<u64, std::sync::Arc<RecordedCommandList>>) {
    let now = COMMAND_LIST_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut map = COMMAND_LISTS.lock().unwrap();
    for h in handles {
        let Some(e) = map.get_mut(h) else { continue };
        e.1 = now;
        if e.2 == LIST_NEW {
            e.2 = LIST_CALLED;
        }
        cache.insert(*h, e.0.clone());
    }
}

/// A tick for walk_command_lists_at: walks with the same one visit each list once between them.
pub(crate) fn list_walk_clock() -> u64 {
    COMMAND_LIST_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// walk_command_lists with the visit tick `now` (list_walk_clock): a submit's roots walked one at a time (one lock
/// each, Vulkan's vk-decode) visit each list once.
pub(crate) fn walk_command_lists_at(
    now: u64,
    handles: &[u64],
    stack: &mut Vec<u64>,
    cache: &mut rustc_hash::FxHashMap<u64, std::sync::Arc<RecordedCommandList>>,
    mut f: impl FnMut(&RecordedCommandList),
) -> usize {
    let mut map = COMMAND_LISTS.lock().unwrap();
    stack.clear();
    stack.extend_from_slice(handles);
    let mut n = 0;
    while let Some(h) = stack.pop() {
        let Some(e) = map.get_mut(&h) else { continue };
        // Lists called again (a mesh's list, once per pass drawing it) are in `cache` already.
        if e.1 == now {
            continue;
        }
        e.1 = now;
        if e.2 == LIST_NEW {
            e.2 = LIST_CALLED;
        }
        cache.insert(h, e.0.clone());
        n += 1;
        f(&e.0);
        stack.extend_from_slice(&e.4);
    }
    n
}

/// The lists `handles` into `cache`, marked as walk_command_lists_at marks them with the tick `now`, without following
/// their calls: vk-decode fetches each CallCommands' lists as its walk reaches it (decode.rs).
pub(crate) fn fetch_command_lists_at(now: u64, handles: &[u64], cache: &mut rustc_hash::FxHashMap<u64, std::sync::Arc<RecordedCommandList>>) {
    let mut map = COMMAND_LISTS.lock().unwrap();
    for h in handles {
        let Some(e) = map.get_mut(h) else { continue };
        if e.1 == now {
            continue;
        }
        e.1 = now;
        if e.2 == LIST_NEW {
            e.2 = LIST_CALLED;
        }
        cache.insert(*h, e.0.clone());
    }
}

// EndRecording handles -> finalized recorded-command snapshots. A command
// buffer gets reused across frames (BeginRecording clears it), so a snapshot
// has to be taken at EndRecording time rather than handing back a pointer
// into the live, mutable buffer.
// Handle -> list, bounded: handles can be re-submitted or CallCommands'd several times, so lists are
// not removed on use; the oldest are evicted instead of leaking forever.
/// Recorded command lists by handle. Value: (list, last use, eviction class, estimated bytes, handles it calls). Bounded by memory: past
/// the budget, lists are evicted by class, then least-recently-used:
///   LIST_DEAD       its command memory was handed out again (invalid in NVN too),
///   LIST_SUBMITTED  submitted to a queue and never called: the per-frame lists, one-shot in practice,
///   LIST_CALLED     run through CallCommands: TotK records each mesh's state/bind list once and calls it whenever the
///                   mesh is drawn, possibly after minutes off screen,
///   LIST_NEW        never executed yet (some are recorded at load time and first replayed much later).
/// Plain LRU evicted the per-mesh lists of everything out of view; looking back drew those meshes with the previous
/// draw's vertex buffers (geometry streaks that accumulated while moving the camera).
pub(crate) static COMMAND_LISTS: std::sync::LazyLock<
    StdMutex<rustc_hash::FxHashMap<u64, (std::sync::Arc<RecordedCommandList>, u64, u8, usize, Box<[u64]>)>>,
> = std::sync::LazyLock::new(|| StdMutex::new(rustc_hash::FxHashMap::default()));

pub(crate) static COMMAND_LIST_BYTES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) const COMMAND_LIST_BUDGET: usize = 512 << 20;

pub(crate) const LIST_DEAD: u8 = 0;

pub(crate) const LIST_SUBMITTED: u8 = 1;

pub(crate) const LIST_CALLED: u8 = 2;

pub(crate) const LIST_NEW: u8 = 3;

/// Diagnostics: class of each evicted list at eviction time (to tell which kind the game still replays).
pub(crate) static EVICTED_CLASS: std::sync::LazyLock<StdMutex<rustc_hash::FxHashMap<u64, u8>>> =
    std::sync::LazyLock::new(|| StdMutex::new(rustc_hash::FxHashMap::default()));

/// Class an evicted list had when it was evicted ("dead", "submitted", "called", "new").
pub(crate) fn evicted_list_class(handle: u64) -> &'static str {
    match EVICTED_CLASS.lock().unwrap().get(&handle) {
        Some(&LIST_DEAD) => "dead",
        Some(&LIST_SUBMITTED) => "submitted",
        Some(&LIST_CALLED) => "called",
        Some(_) => "new",
        None => "?",
    }
}

pub(crate) fn recorded_list_bytes(l: &RecordedCommandList) -> usize {
    l.ops.capacity() * std::mem::size_of::<RecordedNvnCommand>()
        + l.ops
            .iter()
            .map(|op| match &op.typed {
                Some(crate::nvn::cmd::Cmd::UpdateUniformBuffer { data, .. }) => data.len(),
                _ => 0,
            })
            .sum::<usize>()
}

/// Marks lists the queue executed directly (eviction prefers them).
pub(crate) fn mark_command_lists_submitted(handles: &[u64]) {
    let mut g = COMMAND_LISTS.lock().unwrap();
    for h in handles {
        if let Some(e) = g.get_mut(h) {
            if e.2 == LIST_NEW {
                e.2 = LIST_SUBMITTED;
            }
        }
    }
}

pub(crate) static COMMAND_LIST_CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Whether `handle` was ever returned by EndRecording (for diagnosing missing lists).
pub(crate) fn command_handle_issued(handle: u64) -> bool {
    handle != 0 && handle < NEXT_COMMAND_HANDLE.load(std::sync::atomic::Ordering::Relaxed)
}

/// The list for `handle`; `called`: run through CallCommands (not submitted to the queue).
pub(crate) fn command_list(
    handle: u64,
    called: bool,
) -> Option<std::sync::Arc<RecordedCommandList>> {
    let now = COMMAND_LIST_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut g = COMMAND_LISTS.lock().unwrap();
    let e = g.get_mut(&handle)?;
    e.1 = now;
    if called && e.2 != LIST_DEAD {
        e.2 = LIST_CALLED;
    } else if e.2 == LIST_NEW {
        e.2 = LIST_SUBMITTED;
    }
    Some(e.0.clone())
}

pub(crate) static NEXT_COMMAND_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferInitialize(
    cmd: *mut NvnCommandBuffer,
    device: usize,
) -> u8 {
    record_hle_call("hle_nvnCommandBufferInitialize");
    if cmd.is_null() {
        return 0;
    }
    std::ptr::write(
        cmd,
        NvnCommandBuffer {
            device,
            command_used: 0,
            command_size: 0,
            control_used: 0,
            control_size: 0,
            recording: 0,
            _pad: [0; 7],
            ops: Box::into_raw(Box::new(Vec::<RecordedNvnCommand>::new())) as usize,
        },
    );
    1
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferFinalize(cmd: *mut NvnCommandBuffer) {
    record_hle_call("hle_nvnCommandBufferFinalize");
    if cmd.is_null() {
        return;
    }
    if (*cmd).ops != 0 {
        drop(Box::from_raw((*cmd).ops as *mut Vec<RecordedNvnCommand>));
    }
    std::ptr::write_bytes(cmd, 0, 1);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferAddCommandMemory(
    cmd: *mut NvnCommandBuffer,
    pool: *const NvnMemoryPool,
    offset: i64,
    size: usize,
) {
    record_hle_call("hle_nvnCommandBufferAddCommandMemory");
    if !cmd.is_null() {
        // Recording continues at the first byte of the new block; the queries describe only this block.
        (*cmd).command_size = size;
        (*cmd).command_used = 0;
        command_memory_reused(cmd as usize, (pool as usize, offset), size);
    }
}

/// NVN command memory, modelled as the driver uses it: a command buffer records into
/// the block added last, from its first byte, command sets one after another (BeginRecording does not rewind); the
/// GetCommandMemory* queries describe only that block. TotK measures its lists with those queries and sub-allocates
/// its command memory accordingly, so the byte range each list occupies here is the range the game believes it
/// occupies. A list stays valid until memory overlapping that range is added to a command buffer again; then it is
/// dead and the first to go when the list cache needs room. (Before, usage reset at every BeginRecording and sizes
/// summed over blocks, lists were matched by whole blocks, and the cache evicted lists the game still called: meshes
/// drawn with another mesh's vertex buffers.)
pub(crate) struct CommandMemory {
    /// Command buffer -> current block (pool, offset, size) and the block-relative offset its current recording began at.
    pub(crate) cur: rustc_hash::FxHashMap<usize, (usize, i64, usize, usize)>,
    /// (pool, start offset) -> (length, handle) of every recorded list not yet known dead.
    pub(crate) lists: std::collections::BTreeMap<(usize, i64), (usize, u64)>,
    pub(crate) max_len: usize,
}

pub(crate) static COMMAND_MEMORY: std::sync::LazyLock<StdMutex<CommandMemory>> =
    std::sync::LazyLock::new(|| {
        StdMutex::new(CommandMemory {
            cur: rustc_hash::FxHashMap::default(),
            lists: std::collections::BTreeMap::new(),
            max_len: 0,
        })
    });

pub(crate) fn command_memory_reused(cmd: usize, region: (usize, i64), size: usize) {
    // Adding a block writes nothing: lists in it die only when a new recording actually writes over them (TotK re-adds
    // a block to re-record its first display list and keeps calling the one recorded after it in the same block).
    COMMAND_MEMORY
        .lock()
        .unwrap()
        .cur
        .insert(cmd, (region.0, region.1, size, 0));
}

/// Lists overlapping [start, start + len) of `pool` are dead: a new recording wrote over them.
pub(crate) fn kill_overlapping_lists(m: &mut CommandMemory, pool: usize, start: i64, len: usize, by: u64) {
    let end = start + len as i64;
    let from = start - m.max_len as i64;
    let hit: Vec<((usize, i64), usize, u64)> = m
        .lists
        .range((pool, from)..(pool, end))
        .filter(|(&(_, o), &(l, h))| o + l as i64 > start && h != by)
        .map(|(k, &(l, h))| (*k, l, h))
        .collect();
    if hit.is_empty() {
        return;
    }
    // Removed right away (they cannot run validly any more): keeping them until the cache budget forced an eviction
    // held ~480 MB of dead lists, and each eviction then sorted ~48k entries and freed 128 MB under COMMAND_LISTS,
    // blocking the Presentation Thread's list lookups (~118 evictions in 150 s in Hateno). Freed after the lock.
    let mut freed: Vec<std::sync::Arc<RecordedCommandList>> = Vec::with_capacity(hit.len());
    let mut bytes = 0usize;
    {
        let mut g = COMMAND_LISTS.lock().unwrap();
        for (k, _, h) in &hit {
            m.lists.remove(k);
            if let Some(e) = g.remove(h) {
                bytes += e.3;
                freed.push(e.0);
            }
        }
    }
    COMMAND_LIST_BYTES.fetch_sub(bytes, std::sync::atomic::Ordering::Relaxed);
    let mut ev = EVICTED_CLASS.lock().unwrap();
    if ev.len() > 1 << 20 {
        ev.clear();
    }
    for (_, _, h) in hit {
        ev.insert(h, LIST_DEAD);
    }
    drop(ev);
    // Off the game's thread, as a replaced list: EndRecording made about 1,400 frees a frame in TotK, and here they were
    // under the COMMAND_MEMORY lock.
    for l in freed {
        free_later(l);
    }
}

/// Handles by command memory location (pool, offset) of the command set (see EndRecording).
pub(crate) static LOCATION_HANDLES: std::sync::LazyLock<StdMutex<rustc_hash::FxHashMap<(usize, i64), u64>>> =
    std::sync::LazyLock::new(|| StdMutex::new(rustc_hash::FxHashMap::default()));

/// Where the command set being ended starts in command memory, if the command buffer has a block.
pub(crate) fn command_list_location(cmd: usize) -> Option<(usize, i64)> {
    let m = COMMAND_MEMORY.lock().unwrap();
    m.cur
        .get(&cmd)
        .map(|&(pool, off, size, begin)| (pool, off + begin.min(size) as i64))
}

/// BeginRecording: the new command set starts where the block's previous one ended.
pub(crate) fn command_recording_begun(cmd: usize, used: usize) {
    let mut m = COMMAND_MEMORY.lock().unwrap();
    if let Some(c) = m.cur.get_mut(&cmd) {
        c.3 = used;
    }
}

/// EndRecording: the list occupies [begin, used) of the current block (clamped to it: the game believes it fits), and
/// any older list it overlaps has been overwritten.
pub(crate) fn command_list_recorded(cmd: usize, handle: u64, used: usize) {
    let mut m = COMMAND_MEMORY.lock().unwrap();
    if let Some(&(pool, off, size, begin)) = m.cur.get(&cmd) {
        let begin = begin.min(size);
        let len = used.min(size).saturating_sub(begin).max(1);
        let start = off + begin as i64;
        kill_overlapping_lists(&mut m, pool, start, len, handle);
        m.max_len = m.max_len.max(len);
        m.lists.insert((pool, start), (len, handle));
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferAddControlMemory(
    cmd: *mut NvnCommandBuffer,
    _memory: *mut libc::c_void, // control memory is plain CPU memory, not a pool range like command memory
    size: usize,
) {
    record_hle_call("hle_nvnCommandBufferAddControlMemory");
    if !cmd.is_null() {
        (*cmd).control_size = size;
        (*cmd).control_used = 0;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferBeginRecording(cmd: *mut NvnCommandBuffer) {
    record_hle_call("hle_nvnCommandBufferBeginRecording");
    if cmd.is_null() {
        return;
    }
    (*cmd).recording = 1;
    // Command sets follow each other in the current block: no rewind.
    command_recording_begun(cmd as usize, (*cmd).command_used);
    if (*cmd).ops != 0 {
        let v = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
        v.clear();
        if v.capacity() < 2048 {
            v.reserve(2048);
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferEndRecording(cmd: *mut NvnCommandBuffer) -> u64 {
    record_hle_call("hle_nvnCommandBufferEndRecording");
    if cmd.is_null() || (*cmd).ops == 0 {
        return 0;
    }
    (*cmd).recording = 0;
    let ops_vec = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
    // Debug aid (--log-cmd-histogram): per-name command counts, dumped periodically by the present path. A map update per
    // command under one lock for every list, on the game's threads.
    if crate::settings::get().log.log_cmd_histogram {
        let mut h = CMD_HISTOGRAM.lock().unwrap();
        for op in ops_vec.iter() {
            *h.entry(op.name).or_insert(0) += 1;
        }
    }
    let snapshot = Box::new(RecordedCommandList {
        // Exactly sized: the recording buffer keeps its reserved capacity (2048+ commands) for the next recording. Taking
        // the Vec itself stored that capacity with every list (~2048 x size_of::<RecordedNvnCommand>() per list, however
        // short) and made the next BeginRecording allocate it again.
        ops: ops_vec.drain(..).collect(),
        summary: std::sync::OnceLock::new(),
        pass_events: std::sync::OnceLock::new(),
        recorder: crate::nvn::frame_map::take_recorder(cmd as usize),
        recorded_frame: crate::gpu::backend().presented_frames(),
    });
    // Here on the recording thread rather than on the Presentation Thread at submit (they scan every op).
    snapshot.summary();
    // An NVN command handle is the GPU address of the command set: recording again at the same place in command memory
    // yields the same handle, and the game may keep calling the one it stored first. Same location, same handle here.
    let handle = match command_list_location(cmd as usize) {
        Some(loc) => *LOCATION_HANDLES
            .lock()
            .unwrap()
            .entry(loc)
            .or_insert_with(|| {
                NEXT_COMMAND_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            }),
        None => NEXT_COMMAND_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
    };
    {
        let now = COMMAND_LIST_CLOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let bytes = recorded_list_bytes(&snapshot);
        let calls = snapshot.summary().1.clone().into_boxed_slice();
        let mut g = COMMAND_LISTS.lock().unwrap();
        let old = g.insert(
            handle,
            (std::sync::Arc::from(snapshot), now, LIST_NEW, bytes, calls),
        );
        drop(g);
        if let Some(old) = old {
            COMMAND_LIST_BYTES.fetch_sub(old.3, std::sync::atomic::Ordering::Relaxed);
            // The replaced list is freed outside the lock, and off the game's thread (its commands' memory, freed one
            // allocation at a time, was ~2% of guest-main).
            free_later(old.0);
        }
        command_list_recorded(cmd as usize, handle, (*cmd).command_used);
        let mut g = COMMAND_LISTS.lock().unwrap();
        let total =
            COMMAND_LIST_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed) + bytes;
        if total > COMMAND_LIST_BUDGET {
            // Evict down to 3/4 of the budget: by class (see COMMAND_LISTS), oldest first within a class.
            let mut order: Vec<(u8, u64, u64, usize)> =
                g.iter().map(|(h, e)| (e.2, e.1, *h, e.3)).collect();
            order.sort_unstable();
            let mut per_class = [(0usize, 0usize); 4];
            for &(c, _, _, b) in &order {
                per_class[c as usize].0 += 1;
                per_class[c as usize].1 += b;
            }
            println!("[+] NVN: list cache by class (count, MB): dead {:?} submitted {:?} called {:?} new {:?}",
                (per_class[0].0, per_class[0].1 >> 20), (per_class[1].0, per_class[1].1 >> 20), (per_class[2].0, per_class[2].1 >> 20), (per_class[3].0, per_class[3].1 >> 20));
            let mut freed = 0usize;
            let mut gone = Vec::new();
            let mut ev = EVICTED_CLASS.lock().unwrap();
            if ev.len() > 1 << 20 {
                ev.clear();
            }
            for (c, _, h, b) in order {
                if total - freed <= COMMAND_LIST_BUDGET / 4 * 3 || h == handle {
                    break;
                }
                if let Some(e) = g.remove(&h) {
                    gone.push(e.0);
                }
                ev.insert(h, c);
                freed += b;
            }
            drop(ev);
            COMMAND_LIST_BYTES.fetch_sub(freed, std::sync::atomic::Ordering::Relaxed);
            println!(
                "[+] NVN: command list cache over {} MB: evicted {} MB (now {} lists)",
                COMMAND_LIST_BUDGET >> 20,
                freed >> 20,
                g.len()
            );
            drop(g);
            drop(gone); // freed outside the lock
        }
    }
    handle
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferIsRecording(cmd: *const NvnCommandBuffer) -> u8 {
    record_hle_call("hle_nvnCommandBufferIsRecording");
    if cmd.is_null() {
        0
    } else {
        (*cmd).recording
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCommandMemoryUsed(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetCommandMemoryUsed");
    if cmd.is_null() {
        0
    } else {
        (*cmd).command_used
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCommandMemoryFree(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetCommandMemoryFree");
    if cmd.is_null() {
        0
    } else {
        (*cmd).command_size.saturating_sub((*cmd).command_used)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCommandMemorySize(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetCommandMemorySize");
    if cmd.is_null() {
        0
    } else {
        (*cmd).command_size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetControlMemoryUsed(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetControlMemoryUsed");
    if cmd.is_null() {
        0
    } else {
        (*cmd).control_used
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetControlMemoryFree(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetControlMemoryFree");
    if cmd.is_null() {
        0
    } else {
        (*cmd).control_size.saturating_sub((*cmd).control_used)
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetControlMemorySize(
    cmd: *const NvnCommandBuffer,
) -> usize {
    record_hle_call("hle_nvnCommandBufferGetControlMemorySize");
    if cmd.is_null() {
        0
    } else {
        (*cmd).control_size
    }
}

// Generic recorder for every void-returning Bind*/Set*/Draw*/Clear*/Copy*/
// Dispatch*/Push*/Pop*/Insert*/Report*/Wait*/Signal* method (~130 names, see
// the dispatch table below). x0 is always the command buffer; x1-x5 are
// recorded uninterpreted for whichever follow-up pass adds real Metal
// translation. `name` is baked in per-registration via a thin per-name
// wrapper generated by the macro below, so logging/debugging can tell them
// apart.
pub(crate) unsafe fn record_generic(
    name: &'static str,
    cmd: *mut NvnCommandBuffer,
    a1: u64,
    a2: u64,
    a3: u64,
    a4: u64,
    a5: u64,
) {
    record_hle_call(name);
    crate::nvn::frame_map::note(name, cmd as usize);
    drawable_cmd_probe(name);
    if cmd.is_null() || (*cmd).ops == 0 {
        return;
    }
    // Settings of the command buffer object itself, not commands: they write no command memory. Counting them made
    // TotK's re-recorded display lists longer than the slots it sized for them, so they seemed to overwrite the next list.
    if matches!(
        name,
        "nvnCommandBufferSetMemoryCallback"
            | "nvnCommandBufferSetMemoryCallbackData"
            | "nvnCommandBufferSetCommandMemoryCallbackEnabled"
            | "nvnCommandBufferSetControlMemoryCallbackEnabled"
            | "nvnCommandBufferSetDebugLabel"
    ) {
        return;
    }
    if buffer(cmd as usize, || RecordedNvnCommand { name, typed: None, args: Some(Box::new([a1, a2, a3, a4, a5])) }) {
        return;
    }
    if capture(|| {
        // Registers past a call's arguments hold whatever the caller left there.
        let n = match name {
            "nvnCommandBufferSetTiledCacheAction"
            | "nvnCommandBufferSetRasterizerDiscard"
            | "nvnCommandBufferSetSampleMask"
            | "nvnCommandBufferSetConservativeRasterEnable" => 1,
            "nvnCommandBufferSetTiledCacheTileSize" => 2,
            // Float arguments travel in s registers.
            "nvnCommandBufferSetDepthBounds" => 1,
            _ => 5,
        };
        let a = [a1, a2, a3, a4, a5];
        std::iter::once(name.to_string()).chain(a[..n].iter().map(|v| format!("{v:#x}"))).collect::<Vec<_>>().join(" ")
    }) {
        return;
    }
    let ops_vec = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
    ops_vec.push(RecordedNvnCommand {
        name,
        typed: None,
        args: Some(Box::new([a1, a2, a3, a4, a5])),
    });
    (*cmd).command_used += 32; // rough per-command footprint, just needs to trend upward plausibly
}

/// --log-drawables: counts the command against the drawable being drawn on this thread (gfx_cmd).
pub(crate) fn drawable_cmd_probe(name: &'static str) {
    static ON: OnceLock<bool> = OnceLock::new();
    if *ON.get_or_init(|| crate::settings::get().totk.diagnostics.log_drawables) {
        let (obj, func) = crate::nvn::cmd::current_drawable();
        if obj != 0 {
            crate::nvn::cmd::drawable_cmd_stats(func, Some(name));
        }
    }
}

pub(crate) enum Capture {
    /// Commands are collected and not recorded.
    Dry(Vec<String>),
    /// Commands are recorded and collected until there are as many as expected, then passed to the report.
    Watch { expected: Vec<String>, got: Vec<String>, report: fn(&[String], &[String]) },
    /// Commands are recorded and collected until capture_collected.
    Collect(Vec<String>),
    /// Typed commands are collected and not recorded (`ok` false once an untyped command came).
    Typed { cmds: Vec<crate::nvn::cmd::Cmd>, ok: bool },
    /// Commands are kept, with their command buffer, to be recorded later (commit_buffered) or dropped.
    Buffer(Vec<(usize, RecordedNvnCommand)>),
}

/// Commands kept by capture_buffered.
pub(crate) struct Buffered(Vec<(usize, RecordedNvnCommand)>);

/// Runs `f` with this thread's commands kept instead of recorded.
/// Nests: inside another capture of this thread, that capture is kept aside for `f` and put back after it (commands
/// committed then go to it, see commit_buffered).
pub(crate) fn capture_buffered<R>(f: impl FnOnce() -> R) -> (R, Buffered) {
    let outer = CAPTURE.with(|c| c.borrow_mut().replace(Capture::Buffer(Vec::new())));
    let r = f();
    let inner = CAPTURE.with(|c| std::mem::replace(&mut *c.borrow_mut(), outer));
    match inner {
        Some(Capture::Buffer(v)) => (r, Buffered(v)),
        _ => (r, Buffered(Vec::new())),
    }
}

/// Records commands kept by capture_buffered, in order: into the enclosing buffered capture when there is one.
pub(crate) unsafe fn commit_buffered(b: Buffered) {
    let mut b = Some(b);
    CAPTURE.with(|c| {
        if let Some(Capture::Buffer(v)) = c.borrow_mut().as_mut() {
            v.extend(b.take().unwrap().0);
        }
    });
    let Some(b) = b else { return };
    for (cmd, op) in b.0 {
        let cmd = cmd as *mut NvnCommandBuffer;
        if cmd.is_null() || (*cmd).ops == 0 {
            continue;
        }
        (*((*cmd).ops as *mut Vec<RecordedNvnCommand>)).push(op);
        (*cmd).command_used += 32;
    }
}

/// Keeps the command for capture_buffered (true) when this thread is buffering.
pub(crate) fn buffer(cmd: usize, op: impl FnOnce() -> RecordedNvnCommand) -> bool {
    CAPTURE.with(|c| match c.borrow_mut().as_mut() {
        Some(Capture::Buffer(v)) => {
            v.push((cmd, op()));
            true
        }
        _ => false,
    })
}

/// Runs `f` with this thread's commands collected (as text) instead of recorded.
pub fn capture_dry(f: impl FnOnce()) -> Vec<String> {
    CAPTURE.with(|c| *c.borrow_mut() = Some(Capture::Dry(Vec::new())));
    f();
    match CAPTURE.with(|c| c.borrow_mut().take()) {
        Some(Capture::Dry(v)) => v,
        _ => Vec::new(),
    }
}

/// A note in this thread's dry capture, if any (--compare-port marks the game functions a port calls).
pub fn capture_note(note: impl FnOnce() -> String) {
    if !NOTES.with(|n| n.get()) {
        return;
    }
    let note = note();
    CAPTURE.with(|c| {
        if let Some(Capture::Dry(v)) = c.borrow_mut().as_mut() {
            v.push(note);
        }
    });
}

/// `capture_dry` with notes of the game functions called (capture_note).
pub fn capture_dry_noted(f: impl FnOnce()) -> Vec<String> {
    NOTES.with(|n| n.set(true));
    let v = capture_dry(f);
    NOTES.with(|n| n.set(false));
    v
}

/// `capture_dry` inside another capture of this thread (which it leaves as it was).
pub fn capture_dry_nested(f: impl FnOnce()) -> Vec<String> {
    let outer = CAPTURE.with(|c| c.borrow_mut().take());
    let v = capture_dry(f);
    CAPTURE.with(|c| *c.borrow_mut() = outer);
    v
}

/// The typed commands `f` would record on this thread, not recorded; None when any of them is not a typed command.
pub fn capture_typed_dry(f: impl FnOnce()) -> Option<Vec<crate::nvn::cmd::Cmd>> {
    let outer = CAPTURE.with(|c| c.borrow_mut().take());
    CAPTURE.with(|c| *c.borrow_mut() = Some(Capture::Typed { cmds: Vec::new(), ok: true }));
    f();
    let got = CAPTURE.with(|c| c.borrow_mut().take());
    CAPTURE.with(|c| *c.borrow_mut() = outer);
    match got {
        Some(Capture::Typed { cmds, ok: true }) => Some(cmds),
        _ => None,
    }
}

/// The recording `handle` refers to now (a recording at the same command memory keeps its handle), without
/// touching its eviction state.
pub(crate) fn command_list_peek(handle: u64) -> Option<std::sync::Arc<RecordedCommandList>> {
    COMMAND_LISTS.lock().unwrap().get(&handle).map(|e| e.0.clone())
}

/// A recorded list's commands as captures show them (None for an unknown handle).
pub(crate) fn list_texts(handle: u64) -> Option<Vec<String>> {
    let list = COMMAND_LISTS.lock().unwrap().get(&handle).map(|e| e.0.clone())?;
    Some(
        list.ops
            .iter()
            .map(|op| match &op.typed {
                Some(t) => format!("{} {t:?}", op.name),
                None => op.name.to_string(),
            })
            .collect(),
    )
}

/// Collects this thread's next `expected.len()` commands (recorded as usual), then calls `report(expected, got)`.
pub fn capture_watch(expected: Vec<String>, report: fn(&[String], &[String])) {
    if expected.is_empty() {
        return report(&[], &[]);
    }
    CAPTURE.with(|c| *c.borrow_mut() = Some(Capture::Watch { expected, got: Vec::new(), report }));
}

/// Collects this thread's commands (recorded as usual) until capture_collected.
pub fn capture_collect() {
    CAPTURE.with(|c| *c.borrow_mut() = Some(Capture::Collect(Vec::new())));
}

/// The commands collected since capture_collect (empty if this thread was not collecting); ends the collection.
pub fn capture_collected() -> Vec<String> {
    match CAPTURE.with(|c| c.borrow_mut().take()) {
        Some(Capture::Collect(v)) => v,
        _ => Vec::new(),
    }
}

/// Ends an unfinished watch: (expected, got so far).
pub fn capture_finish() -> Option<(Vec<String>, Vec<String>)> {
    match CAPTURE.with(|c| c.borrow_mut().take()) {
        Some(Capture::Watch { expected, got, .. }) => Some((expected, got)),
        _ => None,
    }
}

/// This thread captures for a comparison (not a buffered apply run, which nested ports may join).
pub fn capturing_to_compare() -> bool {
    CAPTURE.with(|c| matches!(&*c.borrow(), Some(c) if !matches!(c, Capture::Buffer(_))))
}

/// Collects the command when this thread captures; true when it must not be recorded.
pub(crate) fn capture(text: impl FnOnce() -> String) -> bool {
    CAPTURE.with(|c| {
        let mut c = c.borrow_mut();
        match c.as_mut() {
            None => false,
            Some(Capture::Dry(v)) => {
                v.push(text());
                true
            }
            Some(Capture::Collect(v)) => {
                v.push(text());
                false
            }
            Some(Capture::Typed { ok, .. }) => {
                *ok = false;
                true
            }
            Some(Capture::Buffer(_)) => true,
            Some(Capture::Watch { expected, got, .. }) => {
                got.push(text());
                if got.len() == expected.len() {
                    if let Some(Capture::Watch { expected, got, report }) = c.take() {
                        drop(c);
                        report(&expected, &got);
                    }
                }
                false
            }
        }
    })
}

/// Record a decoded command (see nvn/cmd.rs). `cmd` is the NvnCommandBuffer address.
/// Commands recorded so far into the list `cmd` is recording.
pub(crate) unsafe fn recording_len(cmd: usize) -> Option<usize> {
    let cmd = cmd as *const NvnCommandBuffer;
    if cmd.is_null() || (*cmd).ops == 0 {
        return None;
    }
    Some((*((*cmd).ops as *const Vec<RecordedNvnCommand>)).len())
}

pub(crate) unsafe fn record_typed(name: &'static str, cmd: usize, typed: crate::nvn::cmd::Cmd) {
    record_hle_call(name);
    crate::nvn::frame_map::note(name, cmd);
    drawable_cmd_probe(name);
    let typed_capture = CAPTURE.with(|c| match c.borrow_mut().as_mut() {
        Some(Capture::Typed { cmds, .. }) => {
            cmds.push(typed.clone());
            true
        }
        _ => false,
    });
    if typed_capture {
        return;
    }
    let mut typed = Some(typed);
    if buffer(cmd, || RecordedNvnCommand { name, typed: typed.take(), args: None }) {
        return;
    }
    let typed = typed.unwrap();
    if capture(|| {
        let mut t = typed.clone();
        if let crate::nvn::cmd::Cmd::DrawElementsInstanced { caller, .. } = &mut t {
            *caller = 0;
        }
        format!("{name} {t:?}")
    }) {
        return;
    }
    let cmd = cmd as *mut NvnCommandBuffer;
    if cmd.is_null() || (*cmd).ops == 0 {
        return;
    }
    let ops_vec = &mut *((*cmd).ops as *mut Vec<RecordedNvnCommand>);
    ops_vec.push(RecordedNvnCommand {
        name,
        typed: Some(typed),
        args: None,
    });
    (*cmd).command_used += 32;
}

// Commands with a float argument: it comes in s0, not in the x registers the generic recorders read.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetAlphaRef(cmd: *mut NvnCommandBuffer, reference: f32) {
    record_generic("nvnCommandBufferSetAlphaRef", cmd, reference.to_bits() as u64, 0, 0, 0, 0);
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferSetLineWidth(cmd: *mut NvnCommandBuffer, width: f32) {
    record_generic("nvnCommandBufferSetLineWidth", cmd, width.to_bits() as u64, 0, 0, 0, 0);
}

// GetMemoryCallback/GetMemoryCallbackData are pure getters over state nothing
// sets yet (SetMemoryCallback/SetMemoryCallbackData are no-op recorders above
// since we don't invoke memory callbacks) -- always null is correct here.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetMemoryCallback() -> usize {
    record_hle_call("hle_nvnCommandBufferGetMemoryCallback");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetMemoryCallbackData() -> usize {
    record_hle_call("hle_nvnCommandBufferGetMemoryCallbackData");
    0
}

// GetCopyRowStride/GetCopyImageStride: real NVN defaults (0 = driver picks
// the tightest valid stride); nothing has set a custom stride since
// SetCopyRowStride/SetCopyImageStride are no-op recorders above.
#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCopyRowStride() -> usize {
    record_hle_call("hle_nvnCommandBufferGetCopyRowStride");
    0
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnCommandBufferGetCopyImageStride() -> usize {
    record_hle_call("hle_nvnCommandBufferGetCopyImageStride");
    0
}
