// Presenting a frame (nvnQueuePresentTexture): the end-of-frame work, the drawable and display pacing on the refresh
// grid.

use super::*;

/// Unlocked frame rates (present interval 0): the game measures each frame's time step from one frame start to the
/// next. Started whenever the previous frame was done, a frame that just missed a display refresh would stay on screen
/// a whole extra refresh the game had not counted, so motion would run ahead and fall behind in a slow wave. Frames
/// start on the refresh grid instead, learned from when drawables actually reached the screen: the time a frame measures is then whole refreshes, as long as frames stay on screen
/// (--log-frame-pacing shows the spread). Presenting each frame a fixed time after its start does not work: Metal does
/// not keep the times, and frames held back stall the drawable queue. --display-pacing (default on), F6 toggles.
pub(super) static DISPLAY_PACING: std::sync::OnceLock<std::sync::atomic::AtomicBool> = std::sync::OnceLock::new();
/// Host time (seconds, f64 bits) of the latest frame shown, and the display's refresh period.
pub(super) static SHOWN_LAST: AtomicU64 = AtomicU64::new(0);
pub(super) static SHOWN_PERIOD: AtomicU64 = AtomicU64::new(0);
/// Host time the current frame started (left nvnWindowAcquireTexture), for --log-frame-pacing.
pub(super) static FRAME_START: AtomicU64 = AtomicU64::new(0);

pub(super) fn display_pacing_flag() -> &'static std::sync::atomic::AtomicBool {
    DISPLAY_PACING.get_or_init(|| std::sync::atomic::AtomicBool::new(crate::settings::get().graphics.display_pacing))
}

pub fn display_pacing() -> bool {
    display_pacing_flag().load(Ordering::Relaxed)
}

pub fn toggle_display_pacing() {
    let on = !display_pacing_flag().fetch_xor(true, Ordering::Relaxed);
    println!("[+] Gfx: display pacing {}", if on { "on" } else { "off" });
}

/// How long to wait before starting the next frame so that it starts on the display's refresh grid (None: grid not known
/// yet, or the last frame shown is too long ago to extrapolate from).
/// The display's refresh period in seconds: from the rate the screen reports, else the smallest gap seen between frames
/// shown. The smallest gap alone is the frame rate's on a display faster than the game (a 120 Hz screen under a 60 fps
/// game reads as 60 Hz).
pub fn display_period() -> Option<f64> {
    let hz = crate::host::live::refresh_hz();
    if hz > 1.0 {
        return Some(1.0 / hz as f64);
    }
    let p = f64::from_bits(SHOWN_PERIOD.load(Ordering::Acquire));
    (p > 0.0).then_some(p)
}

pub fn display_pacing_wait() -> Option<std::time::Duration> {
    let (last, period) = (f64::from_bits(SHOWN_LAST.load(Ordering::Acquire)), display_period().unwrap_or(0.0));
    let now = host_secs();
    if period <= 0.0 || last <= 0.0 || now < last || now - last > 0.25 {
        return None;
    }
    let next = last + ((now - last) / period).ceil() * period;
    Some(std::time::Duration::from_secs_f64((next - now).max(0.0)))
}

/// The frame starts now (nvnWindowAcquireTexture returns).
pub fn frame_started() {
    FRAME_START.store(host_secs().to_bits(), Ordering::Release);
}

/// Called when `drawable` reached the screen: learns the refresh grid; --log-frame-pacing prints the frame's times.
pub(super) fn on_shown(drawable: &ProtocolObject<dyn objc2_metal::MTLDrawable>, n: u64, start: f64, presented: f64, paced: bool, step: f64) {
    let shown: f64 = unsafe { objc2::msg_send![drawable, presentedTime] };
    if shown <= 0.0 {
        return; // dropped (never shown)
    }
    let prev = f64::from_bits(SHOWN_LAST.swap(shown.to_bits(), Ordering::AcqRel));
    let dt = shown - prev;
    if prev > 0.0 && dt < 1.0 {
        crate::nvn::api::pacing_shown(dt);
    }
    if prev > 0.0 && dt > 1.0 / 500.0 && dt < 0.1 {
        // Shown times are whole refreshes apart: the smallest gap seen is the period.
        let p = f64::from_bits(SHOWN_PERIOD.load(Ordering::Acquire));
        if p <= 0.0 || dt < p * 0.9 {
            SHOWN_PERIOD.store(dt.to_bits(), Ordering::Release);
        }
    }
    if crate::settings::get().log.log_frame_pacing {
        println!(
            "[frame-pacing] #{} start {:.3} present +{:.2} ms shown +{:.2} ms (gap {:.2} ms) paced {} step {:.2} ms",
            n,
            start * 1e3,
            (presented - start) * 1e3,
            (shown - start) * 1e3,
            if prev > 0.0 { dt * 1e3 } else { 0.0 },
            paced,
            step * 1e3
        );
    }
}

pub fn present(nvn_tex: usize, crop: [i32; 4]) {
    autoreleasepool(|_| {
        wait_while_suspended();
        let Some(g) = GFX.get() else { return };
        if let Some((first, reps, every)) = replay_spec() {
            let submits = std::mem::take(&mut *FRAME_SUBMITS.lock().unwrap());
            let n = presented_frames();
            if n == first || (every > 0 && n > first && (n - first) % every == 0) {
                REPLAY_HOLD.store(true, std::sync::atomic::Ordering::Release);
                replay_frame(g, n, reps, &submits, nvn_tex);
                REPLAY_HOLD.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        count_display_frame();
        tail_log_present(presented_frames());
        log_hitch();
        {
            static FROM: OnceLock<Option<u64>> = OnceLock::new();
            if let Some(n) = *FROM.get_or_init(|| crate::settings::get().trace.trace_from_frame) {
                if presented_frames() == n {
                    unsafe { crate::guest::exec::crash_handler::install_deferred_trace_points() };
                }
            }
        }
        dump_main_image();
        let src = {
            let mut st = g.state.lock().unwrap();
            // The frame's last command buffers may still be queued on the issuer thread (commits go through it). Metal runs
            // command buffers in commit order, so the present buffer (committed directly below) could otherwise overtake
            // the rendering it shows.
            st.issuer.sync();
            pass_prof::on_present(&g.device, presented_frames());
            report_cmd_time();
            if rt_writeback_enabled() {
                unsafe { writeback_pass(g, &mut st) };
            }
            let Some(t) = (unsafe { g.texture(&mut st.tc, nvn_tex) }) else {
                return;
            };
            let (pf, twin) = metal_format(
                unsafe { tex_info(nvn_tex) }
                    .map(|(_, t)| t.format)
                    .unwrap_or(0),
            );
            // sRGB window textures hold display-ready bytes: view them as plain UNORM so the blit
            // doesn't linearise them and the (non-sRGB) drawable then double-encode.
            match twin {
                Some(plain) if plain != pf && view_format_ok(&t, plain) => {
                    t.newTextureViewWithPixelFormat(plain).unwrap_or(t)
                }
                _ => t,
            }
        };
        pass_graph::at_present(presented_frames() + 1, &src);
        if let Some(layer) = g.layer.get().filter(|_| gfx_mode() != GfxMode::Dry) {
            // The drawable (and on iOS the letterboxed layer) follows the shape the game presents: mods render other aspect
            // ratios (UltraCam at the iPhone's 19.5:9).
            {
                let (w, h) = if crop[2] > 0 && crop[3] > 0 {
                    (crop[2] as u64, crop[3] as u64)
                } else {
                    (src.width() as u64, src.height() as u64)
                };
                static CONTENT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                #[cfg(target_os = "macos")]
                if crate::host::display::follow_window_shape(layer) {
                    CONTENT.store(0, Ordering::Relaxed);
                }
                if CONTENT.swap(w << 32 | h, Ordering::Relaxed) != w << 32 | h {
                    crate::host::display::content_size_changed(layer, w as f64, h as f64);
                }
            }
            // Display link: the drawable for the next refresh it scheduled (and when that is); else the layer's.
            let mode = crate::host::live::present_mode();
            let mut link_shown = None;
            let drawable = if mode == crate::settings::PresentMode::DisplayLink {
                let limit = if crate::host::overlay::fps_mode() == crate::settings::FpsMode::Stock30 {
                    30
                } else {
                    crate::host::live::frame_limit()
                };
                let hz = crate::host::live::refresh_hz().max(60.0);
                super::display_link::run(layer, limit as f32, hz);
                // No drawable in time: the frame is not shown (the layer's own nextDrawable is not allowed while a
                // display link is attached).
                let Some((d, t)) = super::display_link::next(std::time::Duration::from_millis(100)) else { return };
                link_shown = Some(t);
                d
            } else {
                super::display_link::stop();
                if super::display_link::active() {
                    return; // a link being made right now: skip this frame rather than raise
                }
                let Some(d) = layer.nextDrawable() else {
                    return;
                };
                d
            };
            let pass = MTLRenderPassDescriptor::renderPassDescriptor();
            unsafe {
                let att = pass.colorAttachments().objectAtIndexedSubscript(0);
                att.setTexture(Some(&drawable.texture()));
                att.setLoadAction(MTLLoadAction::DontCare);
                att.setStoreAction(MTLStoreAction::Store);
            }
            let Some(cb) = g.queue.commandBuffer() else {
                return;
            };
            if let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) {
                enc.setRenderPipelineState(&g.blit);
                unsafe { enc.setFragmentTexture_atIndex(Some(&src), 0) };
                let tw = src.width() as f32;
                let th = src.height() as f32;
                let (cx, cy, cw, ch) = if crop[2] > 0 && crop[3] > 0 {
                    (
                        crop[0] as f32,
                        crop[1] as f32,
                        crop[2] as f32,
                        crop[3] as f32,
                    )
                } else {
                    (0.0, 0.0, tw, th)
                };
                let is_lower_left = crate::nvn::api::window_origin_mode() == 0;
                let crop_uv: [f32; 4] = if is_lower_left {
                    // NVN LOWER_LEFT window origin (Kirby; TotK uses UPPER_LEFT): rendering keeps the Switch's memory row order
                    // (clip y is negated, see translated::y_negate), where the first row is the bottom of the screen.
                    // Display rectangle starts at y = cy (bottom) and ends at y = cy + ch (top).
                    // Screen top (in.uv.y = 0) maps to (cy + ch) / th; screen bottom (in.uv.y = 1) maps to cy / th.
                    [cx / tw, (cy + ch) / th, cw / tw, -ch / th]
                } else {
                    // UPPER_LEFT (TotK): row 0 is the top of the screen.
                    // Screen top (in.uv.y = 0) maps to cy / th; screen bottom (in.uv.y = 1) maps to (cy + ch) / th.
                    [cx / tw, cy / th, cw / tw, ch / th]
                };
                unsafe {
                    enc.setFragmentBytes_length_atIndex(
                        std::ptr::NonNull::from(&crop_uv).cast(),
                        16,
                        0,
                    )
                };
                unsafe {
                    enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3)
                };
                enc.endEncoding();
            }
            crate::host::overlay::render(&g.device, &cb, &drawable.texture());
            {
                let (n, start, presented, paced, step) = (
                    presented_frames() + 1,
                    f64::from_bits(FRAME_START.load(Ordering::Acquire)),
                    host_secs(),
                    display_pacing(),
                    crate::profile::frame_step::last_step_seconds() as f64,
                );
                let handler = block2::RcBlock::new(
                    move |d: std::ptr::NonNull<ProtocolObject<dyn objc2_metal::MTLDrawable>>| {
                        on_shown(unsafe { d.as_ref() }, n, start, presented, paced, step)
                    },
                );
                unsafe { objc2_metal::MTLDrawable::addPresentedHandler(&*drawable, block2::RcBlock::as_ptr(&handler)) };
            }
            // A frame time of n >= 2 refreshes: the frame stays on screen n refreshes, as a console's present interval
            // holds it. The game presents a few ms into its frame, and a plain present would flip a frame ready early one
            // refresh after the previous one. Displays flip only at refreshes, so any minimum between n - 1 and n periods
            // means n; the middle one leaves the most margin both ways.
            let target = crate::nvn::api::FRAME_TARGET_NS.load(Ordering::Relaxed) as f64 / 1e9;
            let refreshes = display_period().map_or(0.0, |p| (target / p).round());
            if let Some(t) = link_shown {
                super::display_link::presented(t);
                cb.presentDrawable(ProtocolObject::from_ref(&*drawable));
            } else if mode == crate::settings::PresentMode::Adaptive {
                // A minimum time on screen (the limit's frame, a little under it; none without a limit): presenting this
                // way lets a variable-refresh display follow the frames instead of its fixed refresh.
                cb.presentDrawable_afterMinimumDuration(ProtocolObject::from_ref(&*drawable), (target - 0.002).max(0.0));
            } else if refreshes >= 2.0 && display_pacing() && crate::host::overlay::paces_to_refresh() {
                let period = display_period().unwrap_or_default();
                cb.presentDrawable_afterMinimumDuration(ProtocolObject::from_ref(&*drawable), (refreshes - 0.5) * period);
            } else {
                cb.presentDrawable(ProtocolObject::from_ref(&*drawable));
            }
            cb.commit();
            // Remember it: it is never waited on, so check how it ended when the next frame is presented. A failure here
            // would make the next rendering buffer an "innocent victim" in `submit`'s report.
            let prev_present = {
                let mut st = g.state.lock().unwrap();
                st.present_cb.replace(cb.clone())
            };
            if let Some(p) = prev_present {
                if p.status() == MTLCommandBufferStatus::Error {
                    let victim = p.error().map_or(false, |e| {
                        e.localizedDescription().to_string().contains("victim")
                    });
                    if !victim {
                        println!("[!!] GPU: the window PRESENT command buffer failed (this, not the rendering buffer, is the culprit):");
                        report_gpu_error(&p, &[], &[]);
                    }
                }
            }
        }
        static P: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let pn = P.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        // AOT dump run with mods (guest/exec/aot.rs): write the image once mods finished patching code, then stop.
        if crate::guest::exec::aot::deferred_dump_frame() == Some(pn) {
            if let Err(e) = unsafe { crate::guest::exec::aot::dump_after_mods() } {
                eprintln!("[!!] AOT: {}", e);
                std::process::exit(1);
            }
            println!(
                "[+] AOT: image written at frame {} (mods applied); exiting (--aot-dump)",
                pn
            );
            std::process::exit(0);
        }
        PRESENTED.store(pn, std::sync::atomic::Ordering::Relaxed);
        // At most frames_in_flight() command buffers are in flight, so memory freed that many presents ago (plus margin)
        // is no longer read by the GPU.
        crate::nvn::api::flush_pending_unmaps(pn, frames_in_flight() as u64 + 2);
        if crate::settings::get().log.hle_history_at == Some(pn) {
            crate::horizon::call_log::summarize_hle_call_history();
        }
        if pn == 1 || pn % 100 == 0 {
            println!("[DEBUG GFX] present #{} nvn_tex=0x{:x}", pn, nvn_tex);
            if translated::enabled() {
                let st = g.state.lock().unwrap();
                println!(
                "[gfx] draws={} translated={} fallbacks={} pipelines={} pipeline_failures={} copies={}",
                st.stats.draws,
                TRANSLATED_DRAWS.load(Ordering::Relaxed),
                st.stats.draws.saturating_sub(TRANSLATED_DRAWS.load(Ordering::Relaxed)),
                { let sh = st.sh.lock().unwrap(); sh.translated_pipelines.len() },
                st.sh.lock().unwrap().translated_pipelines_failed,
                st.stats.copies
            );
                println!(
                    "[gfx] instanced draws={} max_instances={} total_instances={} max_count={}",
                    st.cx.instanced_stats.0,
                    st.cx.instanced_stats.1,
                    st.cx.instanced_stats.2,
                    st.cx.instanced_stats.3
                );
                let mut hist: Vec<_> = st.cx.translated_hist.iter().collect();
                hist.sort_by(|a, b| b.1.cmp(a.1));
                for ((vs, fs, rt, depth), n) in hist.into_iter().take(10) {
                    let name = |p: usize| {
                        if p == 0 {
                            "-".to_string()
                        } else {
                            unsafe { &*(p as *const crate::gpu::metal::shader_cache::CachedStage) }
                                .entry
                                .clone()
                        }
                    };
                    println!(
                        "[gfx]   {:6} x vs={} fs={} rt0_fmt={} depth_fmt={}",
                        n,
                        name(*vs),
                        name(*fs),
                        rt,
                        depth
                    );
                }
            }
        }
        if crate::settings::get().dump.dump_first_nonzero {
            if pn % 10 == 0 {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                if has_non_zero_pixel(&src) {
                    dump_ppm(&src, &format!("/tmp/nxport_frame_FIRST_NONZERO_{}.ppm", pn));
                    println!("[*** SUCCESS ***] Found non-zero pixels at frame {}! Dumped to /tmp/nxport_frame_FIRST_NONZERO_{}.ppm", pn, pn);
                    std::process::exit(0);
                }
                let st = g.state.lock().unwrap();
                for (&addr, t) in &st.tc.textures {
                    if (addr == 0x303602020 || addr == 0x3036020c0)
                        && addr != nvn_tex
                        && has_non_zero_pixel(t)
                    {
                        dump_ppm(
                            t,
                            &format!("/tmp/nxport_frame_FIRST_NONZERO_{}_backbuffer.ppm", pn),
                        );
                        println!("[*** SUCCESS ***] Found non-zero pixels in swapchain backbuffer 0x{:x} at frame {}!", addr, pn);
                        std::process::exit(0);
                    }
                }
            }
            if pn >= 1500 {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                dump_ppm(&src, "/tmp/nxport_frame_1500.ppm");
                println!("[+] Reached frame 1500, dumped to /tmp/nxport_frame_1500.ppm");
                std::process::exit(0);
            }
        }
        translated::ASYNC_WAITED_US.store(0, Ordering::Relaxed);
        if let Some(b) = translated::debug_vpos_buffer() {
            let at: u64 = crate::settings::get().debug.debug_vpos_dump_at;
            // Records start fresh with frame `at` (append counters at the buffer start are reset once the GPU is idle).
            if pn + 1 == at {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                unsafe {
                    std::ptr::write_bytes(
                        b.0.contents().as_ptr() as *mut u8,
                        0,
                        translated::debug_vpos_len(),
                    )
                };
            }
            if pn == at {
                if let Some(cb) = g.queue.commandBuffer() {
                    cb.commit();
                    cb.waitUntilCompleted();
                }
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        b.0.contents().as_ptr() as *const u8,
                        translated::debug_vpos_len(),
                    )
                };
                let _ = std::fs::write("/tmp/nxport_vpos.bin", bytes);
                println!(
                    "[+] Debug: vertex debug buffer written to /tmp/nxport_vpos.bin (frame {})",
                    pn
                );
            }
        }
        glitch_scan(g, pn, &src);
        report_guard::frame_end(pn);
        rt_guard::frame_end(pn);
        tex_mem_report(g, pn);
        tex_backing_frame(g, pn);
        frame_history(g, pn, &src);
        // --dump-frame=N or N,M,... writes the presented frame as /tmp/nxport_frame_N.ppm (nxport-builder ppm2png converts);
        // with --exit-on-dump the process exits after the last listed frame.
        let dump_frames: &[u64] = &crate::settings::get().dump.dump_frame;
        // --dump-on-trigger=N: dump N frames after the triggered (traced) frame (1 = that frame).
        let trig_off: u64 = crate::settings::get()
            .dump
            .dump_on_trigger
            .unwrap_or(1)
            .max(1)
            - 1;
        let captured = CAPTURE_REQUESTED.load(std::sync::atomic::Ordering::Relaxed)
            && trace_triggered_frame() == pn;
        let triggered_dump = captured
            || (crate::settings::get().dump.dump_on_trigger.is_some()
                && trace_triggered_frame() != 0
                && trace_triggered_frame() + trig_off == pn);
        if dump_frames.contains(&pn) || triggered_dump {
            if let Some(cb) = g.queue.commandBuffer() {
                cb.commit();
                cb.waitUntilCompleted();
            }
            dump_ppm(&src, &format!("/tmp/nxport_frame_{}.ppm", pn));
            {
                let st = g.state.lock().unwrap();
                for (&addr, t) in &st.tc.textures {
                    if (addr == 0x303602020 || addr == 0x3036020c0) && addr != nvn_tex {
                        dump_ppm(t, &format!("/tmp/nxport_frame_{}_other.ppm", pn));
                        println!(
                            "[+] Gfx: dumped other swapchain buffer 0x{:x} (non-zero={})",
                            addr,
                            has_non_zero_pixel(t)
                        );
                    }
                }
            }
            // --dump-mem=0xADDR:LEN[,..]: print guest memory as f32 once the frame's GPU work has completed.
            for &crate::settings::MemRange { addr, len } in &crate::settings::get().dump.dump_mem {
                {
                    let p = crate::nvn::api::translate_virtual(addr) as *const f32;
                    let v: Vec<f32> = (0..len / 4)
                        .map(|i| unsafe { p.add(i).read_unaligned() })
                        .collect();
                    for (row, c) in v.chunks(4).enumerate() {
                        println!("[mem] {:#x} [{}] {:?}", addr + row * 16, row, c);
                    }
                }
            }
            // --dump-mem-raw=0xADDR:LEN:path[,..]: write guest memory bytes to a file (LEN decimal), same moment as above.
            for d in &crate::settings::get().dump.dump_mem_raw {
                let crate::settings::MemRange { addr, len } = d.range;
                let p = crate::nvn::api::translate_virtual(addr) as *const u8;
                let bytes = unsafe { std::slice::from_raw_parts(p, len) };
                match std::fs::write(&d.path, bytes) {
                    Ok(()) => println!("[mem] {:#x}+{} written to {}", addr, len, d.path.display()),
                    Err(e) => println!(
                        "[mem] {:#x}+{}: cannot write {}: {}",
                        addr,
                        len,
                        d.path.display(),
                        e
                    ),
                }
            }
            let want = &crate::settings::get().dump.dump_tex_size;
            if !want.is_empty() {
                // Debug: dump every cached texture of the given sizes (e.g. 2048x4096,800x450), whatever its role.
                let st = g.state.lock().unwrap();
                for (&addr, t) in &st.tc.textures {
                    if want.iter().any(|w| w.is(t.width(), t.height())) {
                        // Block-compressed textures: write level 0 slice 0 as raw blocks (row-major, block rows tightly
                        // packed), for byte comparison with another emulator's copy.
                        let bc_bytes = match t.pixelFormat() {
                            MTLPixelFormat::BC1_RGBA
                            | MTLPixelFormat::BC1_RGBA_sRGB
                            | MTLPixelFormat::BC4_RUnorm
                            | MTLPixelFormat::BC4_RSnorm => Some(8),
                            MTLPixelFormat::BC2_RGBA
                            | MTLPixelFormat::BC2_RGBA_sRGB
                            | MTLPixelFormat::BC3_RGBA
                            | MTLPixelFormat::BC3_RGBA_sRGB
                            | MTLPixelFormat::BC5_RGUnorm
                            | MTLPixelFormat::BC5_RGSnorm
                            | MTLPixelFormat::BC7_RGBAUnorm
                            | MTLPixelFormat::BC7_RGBAUnorm_sRGB => Some(16),
                            _ => None,
                        };
                        if let Some(bb) = bc_bytes {
                            let (bw, bh) = ((t.width() + 3) / 4, (t.height() + 3) / 4);
                            let mut raw = vec![0u8; bw * bh * bb];
                            let region = MTLRegion {
                                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                                size: MTLSize {
                                    width: t.width(),
                                    height: t.height(),
                                    depth: 1,
                                },
                            };
                            unsafe {
                                t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                                    std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                                    bw * bb,
                                    region,
                                    0,
                                )
                            };
                            let path = format!(
                                "/tmp/nxport_frame_{}_tex_{:x}_{}x{}_{:?}.raw",
                                pn,
                                addr,
                                t.width(),
                                t.height(),
                                t.pixelFormat()
                            );
                            let _ = std::fs::write(&path, &raw);
                            println!("[+] Gfx: texture 0x{:x} raw blocks -> {}", addr, path);
                            continue;
                        }
                        if matches!(
                            t.pixelFormat(),
                            MTLPixelFormat::Depth16Unorm | MTLPixelFormat::Depth32Float
                        ) {
                            // Depth textures are private: copy each slice into a buffer on the GPU and dump it as greyscale.
                            use objc2_metal::MTLBlitCommandEncoder;
                            let bpp = if t.pixelFormat() == MTLPixelFormat::Depth16Unorm {
                                2
                            } else {
                                4
                            };
                            let (w, h) = (t.width(), t.height());
                            let slices = t.arrayLength().max(1);
                            let Some(buf) = g.device.newBufferWithLength_options(
                                w * h * bpp * slices,
                                MTLResourceOptions::StorageModeShared,
                            ) else {
                                continue;
                            };
                            if let Some(cb) = g.queue.commandBuffer() {
                                st.issuer.sync(); // other command-buffer use: queued encoder ops first (metal/issuer.rs)
                                if let Some(blit) = cb.blitCommandEncoder() {
                                    for sl in 0..slices {
                                        unsafe {
                                            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toBuffer_destinationOffset_destinationBytesPerRow_destinationBytesPerImage(
                                            t, sl, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: w, height: h, depth: 1 }, &buf, sl * w * h * bpp, w * bpp, w * h * bpp,
                                        );
                                        }
                                    }
                                    blit.endEncoding();
                                }
                                st.issuer.sync(); // other command-buffer use: queued encoder ops first (metal/issuer.rs)
                                cb.commit();
                                cb.waitUntilCompleted();
                            }
                            let data = unsafe {
                                std::slice::from_raw_parts(
                                    buf.contents().as_ptr() as *const u8,
                                    w * h * bpp * slices,
                                )
                            };
                            for sl in 0..slices {
                                let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                                let (mut lo, mut hi, mut ones) = (f32::MAX, f32::MIN, 0usize);
                                for i in 0..w * h {
                                    let o = (sl * w * h + i) * bpp;
                                    let d = if bpp == 2 {
                                        u16::from_le_bytes([data[o], data[o + 1]]) as f32 / 65535.0
                                    } else {
                                        f32::from_le_bytes([
                                            data[o],
                                            data[o + 1],
                                            data[o + 2],
                                            data[o + 3],
                                        ])
                                    };
                                    lo = lo.min(d);
                                    hi = hi.max(d);
                                    if d >= 0.9999 {
                                        ones += 1;
                                    }
                                    let v = (d.clamp(0.0, 1.0) * 255.0) as u8;
                                    out.extend([v, v, v]);
                                }
                                let path = format!(
                                    "/tmp/nxport_frame_{}_tex_{:x}_slice{}_depth.ppm",
                                    pn, addr, sl
                                );
                                let _ = std::fs::write(&path, out);
                                println!("[+] Gfx: depth texture 0x{:x} slice {}: range {:.4}..{:.4}, {:.1}% at far plane", addr, sl, lo, hi, ones as f64 * 100.0 / (w * h) as f64);
                            }
                            continue;
                        }
                        // Cube and array textures: one file per face/slice and mip level through 2D views.
                        let slices = match t.textureType() {
                            MTLTextureType::TypeCube => 6,
                            MTLTextureType::Type2DArray => t.arrayLength(),
                            _ => 1,
                        };
                        if slices > 1 {
                            for lvl in 0..t.mipmapLevelCount() {
                                for sl in 0..slices {
                                    let Some(v) = (unsafe {
                                        t.newTextureViewWithPixelFormat_textureType_levels_slices(
                                            t.pixelFormat(),
                                            MTLTextureType::Type2D,
                                            objc2_foundation::NSRange::new(lvl, 1),
                                            objc2_foundation::NSRange::new(sl, 1),
                                        )
                                    }) else {
                                        continue;
                                    };
                                    let lv = if lvl == 0 {
                                        String::new()
                                    } else {
                                        format!("_l{}", lvl)
                                    };
                                    let path = format!(
                                        "/tmp/nxport_frame_{}_tex_{:x}_slice{}{}_{:?}.ppm",
                                        pn,
                                        addr,
                                        sl,
                                        lv,
                                        t.pixelFormat()
                                    );
                                    println!(
                                        "[+] Gfx: texture 0x{:x} slice {} level {} {:?}: {}",
                                        addr,
                                        sl,
                                        lvl,
                                        t.pixelFormat(),
                                        dump_any(&v, &path)
                                    );
                                }
                            }
                            continue;
                        }
                        let path = format!(
                            "/tmp/nxport_frame_{}_tex_{:x}_{:?}.ppm",
                            pn,
                            addr,
                            t.pixelFormat()
                        );
                        println!(
                            "[+] Gfx: texture 0x{:x} {:?} {:?}: {}",
                            addr,
                            t.textureType(),
                            t.pixelFormat(),
                            dump_any(t, &path)
                        );
                    }
                }
            }
            if !crate::settings::get().dump.dump_final_only {
                let st = g.state.lock().unwrap();
                println!(
                    "[+] Gfx: frame {} drew into {} color targets:",
                    pn,
                    st.frame_rts.len()
                );
                for (i, (_, t, n)) in st.frame_rts.iter().enumerate() {
                    let path = format!(
                        "/tmp/nxport_frame_{}_rt{:02}_{}x{}_{:?}.ppm",
                        pn,
                        i,
                        t.width(),
                        t.height(),
                        t.pixelFormat()
                    );
                    let summary = dump_any(t, &path);
                    println!(
                        "[+] Gfx:   rt{:02} {}x{} {:?} draws={} {}",
                        i,
                        t.width(),
                        t.height(),
                        t.pixelFormat(),
                        n,
                        summary
                    );
                }
            }
            {
                let mut st = g.state.lock().unwrap();
                for (name, buf, w, h, bpp) in std::mem::take(&mut st.depth_snapshots) {
                    let data = unsafe {
                        std::slice::from_raw_parts(
                            buf.contents().as_ptr() as *const u8,
                            w * h * bpp,
                        )
                    };
                    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
                    let (mut lo, mut hi, mut far) = (f32::MAX, f32::MIN, 0usize);
                    for i in 0..w * h {
                        let o = i * bpp;
                        let d = if bpp == 2 {
                            u16::from_le_bytes([data[o], data[o + 1]]) as f32 / 65535.0
                        } else {
                            f32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]])
                        };
                        lo = lo.min(d);
                        hi = hi.max(d);
                        if d >= 0.9999 {
                            far += 1;
                        }
                        let v = (d.clamp(0.0, 1.0) * 255.0) as u8;
                        out.extend([v, v, v]);
                    }
                    let _ =
                        std::fs::write(format!("/tmp/nxport_snap_{}_{}x{}.ppm", name, w, h), out);
                    println!("[+] Gfx:   depth snapshot {} {}x{}: range {:.4}..{:.4}, {:.1}% at far plane", name, w, h, lo, hi, far as f64 * 100.0 / (w * h) as f64);
                }
                for (name, t) in std::mem::take(&mut st.snapshots) {
                    let path = format!(
                        "/tmp/nxport_snap_{}_{}x{}_{:?}.ppm",
                        name,
                        t.width(),
                        t.height(),
                        t.pixelFormat()
                    );
                    println!("[+] Gfx:   snapshot {} {}", name, dump_any(&t, &path));
                }
            }
            if crate::settings::get().dump.exit_on_dump && Some(&pn) == dump_frames.iter().max() {
                println!("[+] Dump complete, exiting on request (--exit-on-dump)");
                std::process::exit(0);
            }
        }
        let mut st_guard = g.state.lock().unwrap();
        let st = &mut *st_guard;
        st.frame_rts.clear();
        st.tc.tex_check_budget_used = 0;
        translated::trim_view_cache(&mut st.tc.view_cache);
        sweep_shared_storage(&mut st.tc, &st.colors, st.depth.as_ref());
        st.root_memo.clear();
        if st.tc.alias_views.len() > 256 {
            st.tc.alias_views.clear();
            st.tc.alias_view_keys.clear();
        }
        if crate::settings::get().log.cmd_stats_at == Some(pn) {
            let mut v: Vec<_> = st.ignored_cmds.iter().map(|(k, n)| (*k, *n)).collect();
            v.sort_by(|a, b| b.1.cmp(&a.1));
            println!(
                "[cmd-stats] frame {}: {} kinds of recorded commands had no executor handler:",
                pn,
                v.len()
            );
            for (k, n) in v {
                println!("[cmd-stats]   {:6} x {}", n, k);
            }
        }
        st.ignored_cmds.clear();
        st.stats.passes += 1;
        {
            static PREV_GPU: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            static PREV_WAIT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            static PREV_VSYNC: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let cur_gpu = GPU_BUSY_US.load(Ordering::Relaxed);
            let cur_wait = SUBMIT_WAIT_US.load(Ordering::Relaxed);
            let cur_vsync = crate::nvn::api::ACQUIRE_SLEEP_US.load(Ordering::Relaxed);
            let gpu_delta = cur_gpu.saturating_sub(PREV_GPU.swap(cur_gpu, Ordering::Relaxed));
            let wait_delta = cur_wait.saturating_sub(PREV_WAIT.swap(cur_wait, Ordering::Relaxed));
            let vsync_delta = cur_vsync.saturating_sub(PREV_VSYNC.swap(cur_vsync, Ordering::Relaxed));
            let (footprint, _) = process_memory_stats();
            crate::host::overlay::record_present(
                gpu_delta as f32 / 1000.0,
                wait_delta as f32 / 1000.0,
                vsync_delta as f32 / 1000.0,
                st.stats.submits,
                st.stats.draws,
                st.stats.passes,
                footprint,
            );
        }
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if n == 1 || n % 50 == 0 {
            let s = st.stats;
            // Frame rate over the last 50 presents (wall clock).
            static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
            let now = std::time::Instant::now();
            let fps = LAST
                .lock()
                .unwrap()
                .replace(now)
                .map_or(0.0, |t| 50.0 / now.duration_since(t).as_secs_f64());
            let gpu = GPU_BUSY_US.swap(0, std::sync::atomic::Ordering::Relaxed) as f64 / 50_000.0;
            let wait =
                SUBMIT_WAIT_US.swap(0, std::sync::atomic::Ordering::Relaxed) as f64 / 50_000.0;
            let acq = crate::nvn::api::ACQUIRE_CALLS.swap(0, std::sync::atomic::Ordering::Relaxed);
            let vsync = crate::nvn::api::ACQUIRE_SLEEP_US
                .swap(0, std::sync::atomic::Ordering::Relaxed) as f64
                / 50_000.0;
            let hashed = REFRESH_HASHED.swap(0, std::sync::atomic::Ordering::Relaxed) as f64
                / 50.0
                / 1048576.0;
            let (footprint, avail) = process_memory_stats();
            let mem_str = if avail > 0 {
                format!(
                    " mem {} MB (avail {} MB)",
                    footprint / 1048576,
                    avail / 1048576
                )
            } else if footprint > 0 {
                format!(" mem {} MB", footprint / 1048576)
            } else {
                String::new()
            };
            let vp = SCENE_VIEWPORT.swap(0, Ordering::Relaxed);
            println!("[+] Gfx: present #{} {:.1} fps gpu {:.1} ms/frame submit-wait {:.1} ms/frame vsync-wait {:.1} ms/frame ({} acquires) interval {} (submits {} clears {} draws-decoded {} passes {}) refresh-hashed {:.1} MB/frame{} scene {}x{}",
            n, fps, gpu, wait, vsync, acq, crate::nvn::api::present_interval(), s.submits, s.clears, s.draws, s.passes, hashed, mem_str, vp >> 20 & 0xFFFFF, vp & 0xFFFFF);
            // --log-sizes: sizes of the executor's caches every 50 frames (to find what grows over a session).
            static SIZES: OnceLock<bool> = OnceLock::new();
            if (n == 1 || n % 50 == 0)
                && *SIZES.get_or_init(|| crate::settings::get().log.log_sizes)
            {
                use objc2_metal::MTLResource;
                let mut tex_bytes = 0usize;
                let mut root_seen: rustc_hash::FxHashSet<usize> = Default::default();
                for t in st.tc.textures.values() {
                    let ptr = objc2::rc::Retained::as_ptr(t) as *const () as usize;
                    if root_seen.insert(ptr) {
                        tex_bytes += t.allocatedSize();
                    }
                }
                let mut alias_bytes = 0usize;
                for (r, v, _) in st.tc.alias_views.values() {
                    let ptr_r = objc2::rc::Retained::as_ptr(r) as *const () as usize;
                    if root_seen.insert(ptr_r) {
                        alias_bytes += r.allocatedSize();
                    }
                    let ptr_v = objc2::rc::Retained::as_ptr(v) as *const () as usize;
                    if root_seen.insert(ptr_v) {
                        alias_bytes += v.allocatedSize();
                    }
                }
                let shs = st.sh.clone();
                let shs = shs.lock().unwrap();
                println!("[sizes] #{} tex_bytes {} MB alias_bytes {} MB | textures {} tex_gens {} alias_of {} tex_by_desc {} tex_checked {} tex_sparse {} tex_track_gen {} view_cache {} gpu_written {} pool_buffers {} const_banks {} samplers {} repack {} ({} MB) refresh_memo {} alias_views {} rt_format_views {} bc_proxies {} pipelines {} {}",
                n, tex_bytes >> 20, alias_bytes >> 20,
                st.tc.textures.len(), st.tc.tex_gens.len(), st.tc.alias_of.len(), st.tc.tex_by_desc.len(), st.tc.tex_checked.len(), st.tc.tex_sparse.len(), st.tc.tex_track_gen.len(),
                st.tc.view_cache.len(), st.tc.gpu_written.len(), translated::pool_buffers_len(), shs.const_banks.len(), shs.samplers.len(), shs.repack_cache.len(), shs.repack_bytes >> 20, st.tc.refresh_memo.len(),
                st.tc.alias_views.len(), st.rt_format_views.len(), st.bc_proxies.len(), shs.translated_pipelines.len(), crate::nvn::api::debug_table_sizes());
            }
            if crate::host::thread_stats::enabled() {
                crate::host::thread_stats::report(n, 50);
            }
            if timeline_enabled() {
                use std::sync::atomic::Ordering::Relaxed;
                let ms = |a: &std::sync::atomic::AtomicU64| a.swap(0, Relaxed) as f64 / 50_000.0;
                println!("[timeline] #{} per frame: encode {:.1} ms (CPU {:.1}), gap between submits {:.1} ms, guest blocked in SyncWait {:.1} ms, GPU tail after last submit {:.1} ms; alias views {:.1}, new textures {:.1}, uploads {:.1}, root walks {:.1} (tag hits {:.1}), alias views created {:.1} (cached {}), binding views created {:.1} (cache clears {:.2}), ubo copies {:.0} reused {:.0} ({:.0} KB, {:.0} clears), feedback passes split {:.1}, issuer syncs {:.1} ({:.2} ms waiting), texture bindings fast {:.0} memo {:.0} resolved {:.0}, texture sets reused {:.0} resolved {:.0}, command lists {:.0}, uniform update ranges {:.0}, indexed copies {:.0}, texture memo hits {:.0} misses {:.0} / {:.0} after table changes, table changes {:.0}, slow resolutions {:.0} ({:.2} ms), alias roots re-initialized {:.1}, objects re-pointed {:.1}",
                n, ms(&TL_ENCODE_US), ms(&TL_ENCODE_CPU_US), ms(&TL_GAP_US), ms(&TL_SYNC_US), ms(&TL_TAIL_US),
                TEXSTAT[0].swap(0, Relaxed) as f64 / 50.0, TEXSTAT[1].swap(0, Relaxed) as f64 / 50.0, TEXSTAT[2].swap(0, Relaxed) as f64 / 50.0,
                translated::ROOT_STATS[1].swap(0, Relaxed) as f64 / 50.0, translated::ROOT_STATS[0].swap(0, Relaxed) as f64 / 50.0, ALIAS_NEW.swap(0, Relaxed) as f64 / 50.0, st.tc.alias_views.len(),
                translated::BINDING_VIEW_STATS[0].swap(0, Relaxed) as f64 / 50.0, translated::BINDING_VIEW_STATS[1].swap(0, Relaxed) as f64 / 50.0,
                translated::UBO_STATS[0].swap(0, Relaxed) as f64 / 50.0, translated::UBO_STATS[1].swap(0, Relaxed) as f64 / 50.0,
                translated::UBO_STATS[2].swap(0, Relaxed) as f64 / 50.0 / 1024.0, translated::UBO_STATS[3].swap(0, Relaxed) as f64 / 50.0, FB_INPLACE.swap(0, Relaxed) as f64 / 50.0,
                issuer::SYNC_STATS[0].swap(0, Relaxed) as f64 / 50.0, ms(&issuer::SYNC_STATS[1]),
                BIND_STATS[0].swap(0, Relaxed) as f64 / 50.0, BIND_STATS[1].swap(0, Relaxed) as f64 / 50.0, BIND_STATS[2].swap(0, Relaxed) as f64 / 50.0, translated::TEX_REUSE_STATS[0].swap(0, Relaxed) as f64 / 50.0, translated::TEX_REUSE_STATS[1].swap(0, Relaxed) as f64 / 50.0, LISTS_SEEN.swap(0, Relaxed) as f64 / 50.0, UBO_RANGES.swap(0, Relaxed) as f64 / 50.0, UBO_INDEXED.swap(0, Relaxed) as f64 / 50.0,
                TEX_MEMO_STATS[0].swap(0, Relaxed) as f64 / 50.0, TEX_MEMO_STATS[1].swap(0, Relaxed) as f64 / 50.0, TEX_MEMO_STATS[2].swap(0, Relaxed) as f64 / 50.0,
                {
                    static LAST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                    let e = crate::nvn::api::tex_table_epoch();
                    e.saturating_sub(LAST.swap(e, Relaxed)) as f64 / 50.0
                }, SLOW_RES[0].swap(0, Relaxed) as f64 / 50.0, SLOW_RES[1].swap(0, Relaxed) as f64 / 50.0 / 1e6,
                EPOCH_BUMPS[0].swap(0, Relaxed) as f64 / 50.0, EPOCH_BUMPS[1].swap(0, Relaxed) as f64 / 50.0);
                if tile_feedback::enabled() {
                    let s = &tile_feedback::STATS;
                    println!(
                        "[timeline] tile feedback: {:.1} draws per frame read the tile; variants made {}, not provable {}",
                        s[1].swap(0, Relaxed) as f64 / 50.0,
                        s[0].load(Relaxed),
                        s[2].load(Relaxed)
                    );
                }
            }
        }
    });
}
