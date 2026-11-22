// nvnQueueSubmitCommands: executing a submit's command lists, the suspension iOS needs while the app is inactive, and
// reporting GPU errors of failed command buffers.

use super::*;

/// Prints everything Metal reports about a failed command buffer, including the per-encoder status that names the
/// debug groups (draw labels, see translated/) that were running when the GPU faulted or timed out.
pub(super) fn report_gpu_error(
    cb: &ProtocolObject<dyn MTLCommandBuffer>,
    labels: &[String],
    details: &[Vec<String>],
) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSString};
    let Some(err) = cb.error() else {
        println!("[!!] GPU: command buffer ended in an error state without an error object");
        return;
    };
    println!(
        "[!!] GPU: command buffer failed: code {} -- {}",
        err.code(),
        err.localizedDescription()
    );
    let mut first_bad: Option<usize> = None;
    unsafe {
        let info = err.userInfo();
        let key = objc2_metal::MTLCommandBufferEncoderInfoErrorKey;
        if let Some(infos) = info.objectForKey(key) {
            let infos: &NSArray<AnyObject> =
                &*(Retained::as_ptr(&infos) as *const NSArray<AnyObject>);
            for i in 0..infos.count() {
                let enc = infos.objectAtIndex(i);
                let label: Option<Retained<NSString>> = msg_send![&*enc, label];
                let state: isize = msg_send![&*enc, errorState];
                let signposts: Option<Retained<NSArray<NSString>>> =
                    msg_send![&*enc, debugSignposts];
                let marks: Vec<String> = signposts
                    .map(|a| {
                        (0..a.count())
                            .map(|j| a.objectAtIndex(j).to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                // MTLCommandEncoderErrorState: 0 unknown, 1 completed, 2 affected, 3 pending, 4 faulted
                if state != 1 && first_bad.is_none() {
                    first_bad = Some(i);
                }
                if state == 4 || (state != 1 && Some(i) == first_bad) {
                    println!(
                        "[!!] GPU:   encoder {} state={} label={:?} draws={:?}",
                        i,
                        state,
                        label.map(|l| l.to_string()),
                        marks
                    );
                }
            }
        }
    }
    println!(
        "[!!] GPU: {} encoders were recorded; the first one that did not complete is {:?}",
        labels.len(),
        first_bad
    );
    // The GPU runs independent render passes concurrently and out of order, so "first encoder not completed" does not
    // single out the faulting pass. List every translated pass in the buffer and one full state line per distinct pair.
    println!("[!!] GPU: translated passes in the failed command buffer:");
    let mut seen_pairs: Vec<String> = Vec::new();
    for (i, l) in labels.iter().enumerate() {
        if !l.starts_with("translated pass") {
            continue;
        }
        println!("[!!] GPU:   encoder {} = {}", i, l);
        if let Some(pair) = l
            .split(" | ")
            .nth(1)
            .map(|p| p.split(" (").next().unwrap_or("").to_string())
        {
            if !seen_pairs.contains(&pair) {
                seen_pairs.push(pair);
                if let Some(d) = details.get(i).and_then(|d| d.first()) {
                    println!("[!!] GPU:     state: {}", d);
                }
            }
        }
    }
}

/// Waits for `cb` like waitUntilCompleted, but a command buffer still running after GPU_HANG_TIMEOUT means the GPU is
/// hung (a shader that never finishes, a runaway draw): left alone, macOS stalls the display and can freeze the whole
/// machine. Print what the command buffer contained and exit, which releases the GPU immediately.
pub(super) fn wait_or_abort(cb: &ProtocolObject<dyn MTLCommandBuffer>, labels: &[String]) {
    const GPU_HANG_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
    let start = std::time::Instant::now();
    loop {
        match cb.status() {
            MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error => return,
            _ => {}
        }
        if start.elapsed() > GPU_HANG_TIMEOUT {
            println!("[!!] GPU HANG: a command buffer has run for {:?}; exiting to release the GPU. Its encoders:", start.elapsed());
            for (i, l) in labels.iter().enumerate() {
                println!(
                    "[!!]   encoder {}: {}",
                    i,
                    l.chars().take(600).collect::<String>()
                );
            }
            std::process::exit(4);
        }
        std::thread::sleep(std::time::Duration::from_micros(200));
    }
}

/// iOS refuses GPU work from an app that is not active (the command buffer fails, which halts rendering for good):
/// host/display_ios.rs sets this while the app is inactive, and submits and presents wait for it to clear, stalling the
/// guest's presentation thread like a paused console.
pub static SUSPENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(super) fn wait_while_suspended() {
    while SUSPENDED.load(Ordering::Acquire) {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}


/// nvnQueueSubmitCommands: execute the given command handles in order.
///
/// Safety net: a GPU hang ("progress timeout") makes macOS reset the GPU and, if work keeps arriving, freeze the whole
/// machine. On the first error the GPU is treated as broken, the details are printed, and no further rendering is
/// submitted.
pub fn submit(handles: &[u64]) {
    let _engine = report_guard::engine_scope();
    wait_while_suspended();
    journal(|| format!("submit {} lists t {:.3}", handles.len(), host_secs() * 1e3));
    if replay_spec().is_some() {
        FRAME_SUBMITS.lock().unwrap().push(handles.to_vec());
    }
    if !timeline_enabled() {
        return submit_inner(handles);
    }
    let t0 = std::time::Instant::now();
    if let Some(prev) = *TL_LAST_SUBMIT_END.lock().unwrap() {
        TL_GAP_US.fetch_add(
            t0.duration_since(prev).as_micros() as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }
    let c0 = thread_cpu_ns();
    submit_inner(handles);
    TL_ENCODE_US.fetch_add(
        t0.elapsed().as_micros() as u64,
        std::sync::atomic::Ordering::Relaxed,
    );
    TL_ENCODE_CPU_US.fetch_add(
        (thread_cpu_ns() - c0) / 1000,
        std::sync::atomic::Ordering::Relaxed,
    );
    *TL_LAST_SUBMIT_END.lock().unwrap() = Some(std::time::Instant::now());
}

/// Starts a submit: records the uniform-buffer ranges any UpdateUniformBuffer in it writes (all lists reachable from
/// `handles`), returning the command buffer to encode into. See `ubo_updated`.
/// Bumped when texture() drops or re-points other objects' Metal textures: invalidates every binding memo (global, so
/// encoding threads check it without the texture cache's lock).
pub(super) static TEX_EPOCH: AtomicU64 = AtomicU64::new(0);

pub(super) fn tex_epoch() -> u64 {
    TEX_EPOCH.load(Ordering::Acquire)
}

pub(super) fn bump_tex_epoch() {
    TEX_EPOCH.fetch_add(1, Ordering::AcqRel);
}

/// Gfx::texture memo: [hits, misses with the table epoch unchanged, misses after table changes].
pub(super) static TEX_MEMO_STATS: [std::sync::atomic::AtomicU64; 3] =
    [const { std::sync::atomic::AtomicU64::new(0) }; 3];
/// Table-epoch changes seen per frame by the encoder (nvn::api::tex_table_epoch deltas at submit starts).
pub(super) static UBO_INDEXED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(super) static LISTS_SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(super) fn cur_cb_and_ranges(
    st: &mut State,
    cb: &Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    handles: &[u64],
) -> Retained<ProtocolObject<dyn MTLCommandBuffer>> {
    st.ubo_updated.clear();
    st.submit_lists.clear();
    let mut stack = std::mem::take(&mut st.lists_stack);
    let mut ranges = std::mem::take(&mut st.ubo_updated);
    let mut submit_lists = std::mem::take(&mut st.submit_lists);
    let mut samples: Vec<usize> = Vec::new();
    let guard = report_guard::mode() != crate::settings::ReportGuard::Off;
    let n = crate::nvn::api::walk_command_lists(handles, &mut stack, &mut submit_lists, |l| {
        let s = l.summary();
        ranges.extend_from_slice(&s.0);
        if guard {
            samples.extend(
                s.2.iter()
                    .map(|&a| crate::nvn::api::translate_virtual(a as usize)),
            );
        }
    });
    report_guard::begin_submit(&samples);
    st.ubo_updated = ranges;
    st.lists_stack = stack;
    st.submit_lists = submit_lists;
    if pass_graph::on() {
        let binds = pass_plan::scan(&st.submit_lists, handles);
        pass_plan::log(&binds, &pass_plan::groups(&binds));
    }
    LISTS_SEEN.fetch_add(n as u64, std::sync::atomic::Ordering::Relaxed);
    // Sorted, merged: ubo_updated is a binary search.
    st.ubo_updated.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(st.ubo_updated.len());
    for &(a, b) in &st.ubo_updated {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    UBO_RANGES.fetch_add(merged.len() as u64, std::sync::atomic::Ordering::Relaxed);
    st.ubo_updated = merged;
    cb.clone()
}

/// Whether [addr, addr+len) overlaps memory an UpdateUniformBuffer of the current submit writes.
pub(super) fn ubo_updated(ranges: &[(u64, u64)], addr: u64, len: u64) -> bool {
    // First merged range ending after addr: overlaps iff it starts before addr + len.
    let i = ranges.partition_point(|&(_, b)| b <= addr);
    ranges.get(i).is_some_and(|&(a, _)| a < addr + len)
}

pub(super) static UBO_RANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(super) fn submit_inner(handles: &[u64]) {
    autoreleasepool(|_| {
        let Some(g) = GFX.get() else { return };
        while retire_oldest(g, true) {}
        while groups_in_flight(&g.state.lock().unwrap()) >= frames_in_flight() {
            let t0 = std::time::Instant::now();
            retire_oldest(g, false);
            SUBMIT_WAIT_US.fetch_add(
                t0.elapsed().as_micros() as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
        }
        if g.state.lock().unwrap().gpu_halted {
            return;
        }
        let desc = MTLCommandBufferDescriptor::new();
        desc.setErrorOptions(MTLCommandBufferErrorOption::EncoderExecutionStatus);
        let Some(cb) = g.queue.commandBufferWithDescriptor(&desc) else {
            return;
        };
        {
            let mut st = g.state.lock().unwrap();
            st.stats.submits += 1;
            if pass_graph::on() {
                println!("[pg] submit {} lists", handles.len());
            }
            // The issuer thread's encode state (parallel encoding) resets its per-submit memory in stream order.
            st.issuer.push(issuer::Op::SubmitStart);
            st.cx.arena.reset();
            st.cx.ubo_copies.clear();
            st.cx.max_index_memo.clear();
            // Its buffers are arena memory, which is reused once the GPU is done with it (Arena::reset).
            st.cx.support_cache.clear();
            st.cx.driver_cb_memo = [None, None];
            st.cx.index_memo = None;
            st.plan_arena.reset();
            st.plan_ubo.clear();
            st.ubo_plan_epoch += 1;
            st.rts_dirty = true;
            st.cached_rts = None;
            // Memos validate everything they depend on and persist across submits (cleared here, the first bindings of
            // every frame would go through full resolution).
            if st.cx.bind_memo.len() > 1 << 16 {
                st.cx.bind_memo.clear();
            }
            if early_fence() {
                let prev: Vec<(u64, u64)> = std::mem::take(&mut st.gpu_dirty).into_iter().collect();
                if !prev.is_empty() {
                    // The previous submit's tail may go into the next command buffer.
                    st.carried_dirty.push_back((submitted_serial() + 1, prev));
                }
                let done = completed_serial();
                st.carried_dirty.retain(|(s, _)| *s > done);
                let carried: Vec<(u64, u64)> = st
                    .carried_dirty
                    .iter()
                    .flat_map(|(_, r)| r.iter().copied())
                    .collect();
                for (a, b) in carried {
                    mark_gpu_dirty(&mut st, a, b);
                }
            } else {
                st.gpu_dirty.clear();
            }
            st.lb.enc_labels.clear();
            st.lb.enc_details.clear();
            st.cur_cb = Some(cur_cb_and_ranges(&mut st, &cb, handles));
            st.cb_started = Some(std::time::Instant::now());
            for h in handles {
                let Some(cur) = st.cur_cb.clone() else { break };
                unsafe { g.run_list(&mut st, &cur, *h, 0) };
            }
            g.end_encoder(&mut st);
            st.submit_lists.clear();
        }
        if gfx_mode() == GfxMode::Dry {
            // Nothing reaches the GPU: the encoded work is dropped (all encoders are already ended).
            g.state.lock().unwrap().occl.pending.clear();
            return;
        }
        let mut st = g.state.lock().unwrap();
        let Some(cb) = st.cur_cb.take() else { return };
        tail_log_label(&st, &cb);
        // Targets written only by tiny-pass kernels end no render pass: everything of the submit is in this one.
        rt_guard::pass_end();
        let serial = push_inflight(&mut st, cb.clone());
        st.issuer.commit_serial(cb, Some(serial)); // after the queued encoder ops, on the issuer thread (metal/issuer.rs)
    });
}
