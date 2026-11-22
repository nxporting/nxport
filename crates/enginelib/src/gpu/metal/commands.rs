// Executing recorded command lists: the NVN commands (render targets, draws, clears, counters, conditional rendering)
// in order.

use super::*;

impl Gfx {
    pub(super) unsafe fn run_list(
        &self,
        st: &mut State,
        _cb: &ProtocolObject<dyn MTLCommandBuffer>,
        handle: u64,
        depth: u32,
    ) {
        if depth > 8 {
            return;
        }
        let Some(list) = st
            .submit_lists
            .get(&handle)
            .cloned()
            .or_else(|| crate::nvn::api::command_list(handle, depth > 0))
        else {
            // Its state changes and draws would silently vanish: always worth knowing.
            static MISSING: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
            let n = MISSING.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n <= 40 || n.is_power_of_two() {
                println!("[!] Gfx: command list handle {:#x} is not recorded ({}, class at eviction: {}, {}); its commands are lost ({} so far)", handle,
                    if crate::nvn::api::command_handle_issued(handle) { "evicted" } else { "never issued by EndRecording" },
                    crate::nvn::api::evicted_list_class(handle), if depth > 0 { "called" } else { "submitted" }, n);
            }
            return;
        };
        let replay_log = replay_log_enabled();
        let tracing = tracing_frame();
        let list_old = list.recorded_frame < presented_frames();
        let Some(mut cur) = st.cur_cb.clone() else {
            return;
        };
        // --log-cmd-time=N: executor time per NVN command kind over frames N..N+60 (CallCommands includes its list).
        let mut cmd_t: Option<(&'static str, std::time::Instant)> = None;
        let cmd_time = cmd_time_on();
        if cmd_time && depth > 0 {
            let draws = list
                .ops
                .iter()
                .filter(|o| {
                    matches!(
                        o.typed,
                        Some(
                            Cmd::DrawArrays { .. }
                                | Cmd::DrawArraysInstanced { .. }
                                | Cmd::DrawElements { .. }
                                | Cmd::DrawElementsInstanced { .. }
                        )
                    )
                })
                .count();
            let calls = list
                .ops
                .iter()
                .filter(|o| matches!(o.typed, Some(Cmd::CallCommands(_))))
                .count();
            let age = presented_frames().saturating_sub(list.recorded_frame);
            let kind = match (age, draws > 0, calls > 0) {
                (0, false, false) => "called list: new, state only",
                (0, true, _) => "called list: new, draws",
                (0, false, true) => "called list: new, calls",
                (_, false, false) => "called list: old, state only",
                (_, true, _) => "called list: old, draws",
                (_, false, true) => "called list: old, calls",
            };
            note_cmd_time(kind, list.ops.len() as u64 * 1000);
        }
        let reuse_log = draw_reuse_log();
        for (op_index, op) in list.ops.iter().enumerate() {
            st.in_old_list = list_old && depth > 0;
            if reuse_log {
                st.cur_list_op = Some((std::sync::Arc::as_ptr(&list) as usize, op_index, list_old));
            }
            if cmd_time {
                let now = std::time::Instant::now();
                if let Some((n, t)) = cmd_t.replace((op.name, now)) {
                    note_cmd_time(n, (now - t).as_nanos() as u64);
                }
            }
            if replay_log
                && matches!(
                    op.typed,
                    Some(
                        Cmd::DrawArrays { .. }
                            | Cmd::DrawArraysInstanced { .. }
                            | Cmd::DrawElements { .. }
                            | Cmd::DrawElementsInstanced { .. }
                    )
                )
            {
                replay_note(st, handle, op_index, list_old, op);
            }
            // Commands go into the current command buffer, which `maybe_split` may have committed and replaced, also
            // inside called lists: TotK issues its occlusion queries from called lists, and game threads wait for them
            // (metal/report_guard.rs), so the command buffer holding them is committed right after them.
            if self.maybe_split(st) {
                let Some(new_cb) = st.cur_cb.clone() else {
                    return;
                };
                cur = new_cb;
            }
            let cb: &ProtocolObject<dyn MTLCommandBuffer> = &cur;
            if tracing {
                match &op.typed {
                    Some(Cmd::BindVertexBuffer { index, addr, size }) => println!(
                        "[cmdtrace] BindVertexBuffer {} {:#x}+{:#x}",
                        index, addr, size
                    ),
                    Some(Cmd::BindProgram { program, .. }) => {
                        println!("[cmdtrace] BindProgram {:#x}", program)
                    }
                    Some(Cmd::DrawElements {
                        count, base_vertex, ..
                    }) => println!(
                        "[cmdtrace] DrawElements count={} base_vertex={}",
                        count, base_vertex
                    ),
                    Some(Cmd::DrawElementsInstanced {
                        count, base_vertex, ..
                    }) => println!(
                        "[cmdtrace] DrawElementsInstanced count={} base_vertex={}",
                        count, base_vertex
                    ),
                    Some(Cmd::DrawArrays { count, .. }) => {
                        println!("[cmdtrace] DrawArrays count={}", count)
                    }
                    Some(Cmd::CallCommands(h)) => println!("[cmdtrace] CallCommands {:x?}", h),
                    Some(Cmd::SetRenderEnable(e)) => println!("[cmdtrace] SetRenderEnable {}", e),
                    Some(Cmd::Barrier(bits)) => println!("[cmdtrace] Barrier {:#x}", bits),
                    Some(Cmd::ResetCounter { counter }) => {
                        println!("[cmdtrace] ResetCounter {}", counter)
                    }
                    Some(Cmd::ReportCounter { counter, addr }) => {
                        println!("[cmdtrace] ReportCounter {} {:#x}", counter, addr)
                    }
                    Some(Cmd::BindUniformBuffer {
                        stage,
                        index,
                        addr,
                        size,
                    }) => println!(
                        "[cmdtrace] BindUniformBuffer s{} i{} {:#x}+{:#x}",
                        stage, index, addr, size
                    ),
                    Some(Cmd::CopyBufferToBuffer { src, dst, size }) => println!(
                        "[cmdtrace] CopyBufferToBuffer {:#x} -> {:#x} ({:#x} bytes)",
                        src, dst, size
                    ),
                    Some(Cmd::ClearBuffer { dst, size, value }) => println!(
                        "[cmdtrace] ClearBuffer {:#x} ({:#x} bytes) = {:#x}",
                        dst, size, value
                    ),
                    Some(Cmd::BindStorageBuffer {
                        stage,
                        index,
                        addr,
                        size,
                    }) => println!(
                        "[cmdtrace] BindStorageBuffer s{} i{} {:#x}+{:#x}",
                        stage, index, addr, size
                    ),
                    Some(Cmd::BindTexture {
                        stage,
                        index,
                        handle,
                    }) => println!("[cmdtrace] BindTexture s{} i{} {:#x}", stage, index, handle),
                    Some(Cmd::ReportValue { value, addr }) => {
                        println!("[cmdtrace] ReportValue {} {:#x}", value, addr)
                    }
                    Some(Cmd::SetRenderEnableConditional { mode, counters }) => println!(
                        "[cmdtrace] SetRenderEnableConditional mode={} {:#x}",
                        mode, counters
                    ),
                    Some(Cmd::ClearColor { index, mask, .. }) => {
                        println!("[cmdtrace] ClearColor c{} mask={:x}", index, mask)
                    }
                    Some(Cmd::SetRenderTargets { colors, depth }) => println!(
                        "[cmdtrace] SetRenderTargets {:x?} depth={:x?}",
                        colors.iter().map(|c| c.texture).collect::<Vec<_>>(),
                        depth.as_ref().map(|d| d.texture)
                    ),
                    Some(_) => println!("[cmdtrace] {}", op.name),
                    None => println!(
                        "[cmdtrace] (ignored) {} args={:x?}",
                        op.name,
                        op.args.as_deref().map_or(&[][..], |a| &a[..])
                    ),
                }
            }
            let Some(cmd) = &op.typed else {
                // A dropped draw or dispatch silently removes geometry (or the GPU work that feeds it): say so once.
                if (op.name.contains("Draw") || op.name.contains("Dispatch"))
                    && !st.ignored_cmds.contains_key(op.name)
                {
                    static WARNED: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
                    let mut w = WARNED.lock().unwrap();
                    if !w.contains(&op.name) {
                        w.push(op.name);
                        println!(
                            "[!] Gfx: {} is not executed (no handler), args={:x?}",
                            op.name,
                            op.args.as_deref().map_or(&[][..], |a| &a[..])
                        );
                    }
                }
                *st.ignored_cmds.entry(op.name).or_insert(0) += 1;
                continue;
            };
            if st.stats.submits <= 3 {
                match cmd {
                    Cmd::SetRenderTargets { colors, .. } => {
                        for (i, c) in colors.iter().enumerate() {
                            println!("[DEBUG GFX] SetRT[{}] tex=0x{:x}", i, c.texture);
                        }
                    }
                    Cmd::ClearColor { index, rgba, .. } => {
                        let rt_addr = st
                            .colors
                            .get(*index as usize)
                            .map(|c| c.texture)
                            .unwrap_or(0);
                        println!("[DEBUG GFX] ClearColor rt=0x{:x} rgba={:?}", rt_addr, rgba);
                    }
                    Cmd::DrawElements {
                        mode,
                        count,
                        base_vertex,
                        ..
                    } => {
                        let rt_addr = st.colors.first().map(|c| c.texture).unwrap_or(0);
                        println!(
                            "[DEBUG GFX] DrawElements rt=0x{:x} count={} mode={} base_vtx={}",
                            rt_addr, count, mode, base_vertex
                        );
                    }
                    Cmd::DrawArrays {
                        mode, first, count, ..
                    } => {
                        let rt_addr = st.colors.first().map(|c| c.texture).unwrap_or(0);
                        println!(
                            "[DEBUG GFX] DrawArrays rt=0x{:x} first={} count={} mode={}",
                            rt_addr, first, count, mode
                        );
                    }
                    Cmd::BindVertexBuffer { index, addr, size } => {
                        println!(
                            "[DEBUG GFX] BindVB[{}] addr=0x{:x} size={}",
                            index, addr, size
                        );
                    }
                    Cmd::BindTexture {
                        stage,
                        index,
                        handle,
                    } => {
                        println!(
                            "[DEBUG GFX] BindTex stage={} index={} handle=0x{:x}",
                            stage, index, handle
                        );
                    }
                    _ => {}
                }
            }
            if st.rasterizer_discard
                && matches!(
                    cmd,
                    Cmd::DrawArrays { .. } | Cmd::DrawArraysInstanced { .. } | Cmd::DrawElements { .. } | Cmd::DrawElementsInstanced { .. }
                )
            {
                journal(|| format!("  discard-skip program {:#x}", st.program.unwrap_or(0)));
                continue;
            }
            if !st.render_enabled
                && matches!(
                    cmd,
                    Cmd::ClearColor { .. }
                        | Cmd::ClearDepthStencil { .. }
                        | Cmd::DrawArrays { .. }
                        | Cmd::DrawArraysInstanced { .. }
                        | Cmd::DrawElements { .. }
                        | Cmd::DrawElementsInstanced { .. }
                )
            {
                if tracing_frame() {
                    let count = match cmd {
                        Cmd::DrawArrays { count, .. } | Cmd::DrawElements { count, .. } => {
                            *count as i64
                        }
                        Cmd::DrawArraysInstanced { count, .. }
                        | Cmd::DrawElementsInstanced { count, .. } => *count as i64,
                        _ => -1,
                    };
                    println!(
                        "[ftrace] COND-SKIP count={} program={:#x}",
                        count,
                        st.program.unwrap_or(0)
                    );
                }
                journal(|| format!("  cond-skip program {:#x}", st.program.unwrap_or(0)));
                continue;
            }
            match cmd {
                Cmd::SetRenderEnable(enable) => {
                    if journal_on() && st.render_enabled != *enable {
                        journal(|| format!("  render-enable {}", enable));
                    }
                    st.render_enabled = *enable;
                }
                Cmd::SetPointSize(sz) => {
                    if tracing_frame() {
                        println!("[cmdtrace] SetPointSize {}", sz);
                    }
                    st.point_size = *sz;
                }
                Cmd::SetRenderEnableConditional { mode, counters } => self.render_enable_conditional(st, *mode, *counters),
                Cmd::BindTexturesDeferred {
                    stage,
                    first,
                    count,
                    handles,
                } => apply_textures_deferred(st, *stage, *first, *count, *handles),
                Cmd::SetRenderTargets { colors, depth } => self.set_render_targets(st, colors, depth.as_deref()),
                Cmd::ClearColor { index, rgba, mask } => {
                    st.stats.clears += 1;
                    let index = match st.color_dups.get(*index as usize) {
                        Some(Some(i)) => *i as i32,
                        _ => *index,
                    };
                    self.clear(st, cb, index, *rgba, *mask);
                }
                Cmd::ClearDepthStencil {
                    depth,
                    depth_mask,
                    stencil,
                    stencil_mask,
                } => {
                    st.stats.clears += 1;
                    self.clear_depth(st, cb, *depth, *depth_mask, *stencil, *stencil_mask);
                }
                Cmd::BindProgram { program, stages } => apply_program(st, *program, *stages),
                Cmd::DispatchCompute { x, y, z } => {
                    if translated::enabled() {
                        self.end_encoder(st);
                        self.dispatch_compute(st, cb, *x, *y, *z);
                    }
                }
                Cmd::DrawArrays {
                    mode,
                    first,
                    count,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_arrays(st, cb, *mode, *first, *count, 0, 1);
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::DrawArraysInstanced {
                    mode,
                    first,
                    count,
                    base_instance,
                    instances,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_arrays(st, cb, *mode, *first, *count, *base_instance, *instances);
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::DrawElements {
                    mode,
                    index_type,
                    count,
                    index_addr,
                    base_vertex,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_elements(
                            st,
                            cb,
                            *mode,
                            *index_type,
                            *count,
                            *index_addr,
                            *base_vertex,
                            0,
                            1,
                        );
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::DrawElementsInstanced {
                    mode,
                    index_type,
                    count,
                    index_addr,
                    base_vertex,
                    base_instance,
                    instances,
                    caller,
                } => {
                    st.cur_caller = *caller;
                    st.stats.draws += 1;
                    if st.occl.counting {
                        st.occl.draws_seen += 1;
                    }
                    if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                        let t = (*caller != 0).then(std::time::Instant::now);
                        self.draw_elements(
                            st,
                            cb,
                            *mode,
                            *index_type,
                            *count,
                            *index_addr,
                            *base_vertex,
                            *base_instance,
                            *instances,
                        );
                        if let Some(t) = t {
                            crate::nvn::cmd::note_draw_time(*caller, t.elapsed().as_nanos() as u64)
                        }
                    }
                }
                Cmd::ResetCounter { counter } => {
                    if *counter == 1 && self.real_samples() {
                        self.occl_reset(st);
                    }
                }
                Cmd::ReportCounter { counter, addr } => self.report_counter(st, *counter, *addr),
                Cmd::ReportValue { value, addr } => {
                    rt_guard::superseded(crate::nvn::api::translate_virtual(*addr as usize), 16);
                    write_counter(0, *addr, Some(*value as u64));
                    st.plan_ubo.invalidate(*addr, *addr + 16);
                    st.cx.ubo_copies.invalidate(*addr, *addr + 16);
                    st.ubo_plan_epoch += 1;
                    GUEST_WRITES.fetch_add(1, Ordering::Relaxed);
                }
                Cmd::BindBlendState(s) => {
                    st.blend_states[(*s & 7) as usize] = *s;
                }
                Cmd::BindColorState(s) => {
                    st.color_state = *s;
                }
                Cmd::BindChannelMaskState(s) => {
                    st.channel_mask = *s;
                }
                Cmd::SetViewport { x, y, w, h } => set_viewport(st, *x, *y, *w, *h),
                Cmd::SetScissor { x, y, w, h } => {
                    st.scissor = Some((*x, *y, *w, *h));
                }
                // Bindings and sampler states are memoized by handle: a pool change invalidates them.
                Cmd::SetTexturePool(pool) => {
                    if crate::nvn::api::set_active_texture_pool(*pool) {
                        crate::nvn::api::active_pools_changed();
                    }
                }
                Cmd::SetSamplerPool(pool) => {
                    if crate::nvn::api::set_active_sampler_pool(*pool) {
                        crate::nvn::api::active_pools_changed();
                    }
                }
                Cmd::BindVertexBuffer { index, addr, size } => {
                    st.vertex_buffers.insert(*index, (*addr, *size));
                }
                Cmd::BindUniformBuffer {
                    stage,
                    index,
                    addr,
                    size,
                } => {
                    st.uniform_buffers.insert((*stage, *index), (*addr, *size));
                }
                Cmd::BindStorageBuffer {
                    stage,
                    index,
                    addr,
                    size,
                } => {
                    if (0..6).contains(stage) && (0..16).contains(index) {
                        st.storage_buffers[*stage as usize][*index as usize] = (*addr, *size);
                    }
                }
                Cmd::UpdateUniformBuffer { addr, offset, data } => update_uniform(st, *addr, *offset, data),
                Cmd::BindVertexAttribState(attribs) => {
                    st.vertex_attribs = **attribs;
                }
                Cmd::BindVertexStreamState(streams) => {
                    st.vertex_streams.clone_from(streams);
                }
                Cmd::BindImage {
                    stage,
                    index,
                    handle,
                } => {
                    st.images_bound.insert((*stage, *index), *handle);
                }
                Cmd::BindTexture {
                    stage,
                    index,
                    handle,
                } => {
                    st.textures_bound.insert((*stage, *index), *handle);
                }
                Cmd::CopyBufferToTexture { .. }
                | Cmd::CopyTextureToBuffer { .. }
                | Cmd::CopyTextureToTexture(..)
                | Cmd::CopyBufferToBuffer { .. }
                | Cmd::ClearBuffer { .. }
                | Cmd::ClearTexture(..) => {
                    if let Some(w) = watch_storage() {
                        let dst = match cmd {
                            Cmd::CopyBufferToTexture { dst, .. } => Some(dst.texture),
                            Cmd::CopyTextureToTexture(p) => Some(p.dst.texture),
                            Cmd::ClearTexture(p) => Some(p.dst.texture),
                            _ => None,
                        };
                        if dst
                            .and_then(|d| tex_info(d))
                            .is_some_and(|(_, t)| t.storage == w)
                        {
                            println!(
                                "[watch] f{} copy/clear into storage {:#x}: {:?}",
                                presented_frames() + 1,
                                w,
                                cmd
                            );
                        }
                    }
                    if let Some((ww, wh)) = watch_size() {
                        let dst = match cmd {
                            Cmd::CopyBufferToTexture { dst, .. } => Some(dst.texture),
                            Cmd::CopyTextureToTexture(p) => Some(p.dst.texture),
                            Cmd::ClearTexture(p) => Some(p.dst.texture),
                            _ => None,
                        };
                        if let Some((_, t)) = dst
                            .and_then(|d| tex_info(d))
                            .filter(|(_, t)| t.width as u32 == ww && t.height as u32 == wh)
                        {
                            println!("[watch-size] f{} copy/clear into storage {:#x} fmt {} levels {}: {:?}", presented_frames() + 1, t.storage, t.format, t.levels, cmd);
                        }
                        let src = match cmd {
                            Cmd::CopyTextureToBuffer { src, .. } => Some(src.texture),
                            Cmd::CopyTextureToTexture(p) => Some(p.src.texture),
                            _ => None,
                        };
                        if let Some((_, t)) = src
                            .and_then(|d| tex_info(d))
                            .filter(|(_, t)| t.width as u32 == ww && t.height as u32 == wh)
                        {
                            println!(
                                "[watch-size] f{} copy out of storage {:#x} fmt {} levels {}: {:?}",
                                presented_frames() + 1,
                                t.storage,
                                t.format,
                                t.levels,
                                cmd
                            );
                        }
                    }
                    if gfx_mode() >= GfxMode::All || gfx_mode() == GfxMode::Dry {
                        self.exec_copy(st, cb, cmd);
                    }
                }
                Cmd::BindDepthStencilState(s) => {
                    st.depth_stencil = *s;
                }
                Cmd::BindPolygonState(s) => {
                    st.polygon = *s;
                }
                Cmd::BindMultisampleState(s) => {
                    st.multisample = *s;
                }
                Cmd::Barrier(bits) => {
                    // INVALIDATE_TEXTURE: texture reads after this see render-target writes from before it. The ordering
                    // that makes those writes complete may come in the same call or an earlier one ("either by
                    // setting an ORDER bit in the barrier or by waiting on a NVNsync object"): Kirby issues ORDER_PRIMITIVES
                    // and INVALIDATE_TEXTURE as separate barriers: requiring both bits in one call would never start a
                    // new epoch there.
                    if bits & 0x10 != 0 {
                        st.fb_epoch += 1;
                    }
                }
                Cmd::SetDepthRange { near, far } => {
                    st.depth_range = (*near, *far);
                }
                Cmd::SetStencilValue { kind, faces, value } => {
                    // NVNface: FRONT 1, BACK 2, FRONT_AND_BACK 3. Stencil values are 8-bit here (D24S8 / D32FS8).
                    let k = (*kind as usize).min(2);
                    if faces & 1 != 0 {
                        st.stencil[k][0] = *value as u8;
                    }
                    if faces & 2 != 0 {
                        st.stencil[k][1] = *value as u8;
                    }
                }
                Cmd::SetDepthBounds { enable, min, max } => {
                    st.depth_bounds = enable.then_some((*min, *max));
                    // Recorded but not applied yet (Metal has no depth bounds test); say so once if a game enables it.
                    static WARNED: std::sync::atomic::AtomicBool =
                        std::sync::atomic::AtomicBool::new(false);
                    if *enable && !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        println!(
                            "[!] Gfx: depth bounds test enabled ({}..{}) but not emulated",
                            min, max
                        );
                    }
                    if tracing_frame() {
                        println!("[ftrace] DEPTH BOUNDS {:?}", st.depth_bounds);
                    }
                }
                Cmd::SetDepthClamp(on) => {
                    st.depth_clamp = *on;
                }
                Cmd::SetRasterizerDiscard(on) => {
                    st.rasterizer_discard = *on;
                }
                Cmd::SetPrimitiveRestart { enable, index } => {
                    st.prim_restart = enable.then_some(*index);
                }
                Cmd::SetPolygonOffset {
                    factor,
                    units,
                    clamp,
                } => {
                    st.polygon_offset = (*factor, *units, *clamp);
                }
                Cmd::SetBlendColor(c) => {
                    st.blend_color = *c;
                }
                Cmd::RenderState(r) => apply_render_state(st, r),
                Cmd::VertexInput(v) => apply_vertex_input(st, v),
                Cmd::ModelDraw(m) => {
                    use crate::nvn::cmd::ModelItem;
                    for item in &m.items {
                        match item {
                            // The material from the game's objects: the model's vertex input, then the material's
                            // bindings (replay side B runs the recorded list instead).
                            ModelItem::Unit { material: Some(mat), .. } if !replay_b() => {
                                for c in mat.iter() {
                                    match c {
                                        Cmd::VertexInput(v) => apply_vertex_input(st, v),
                                        Cmd::BindTexture { stage, index, handle } => {
                                            st.textures_bound.insert((*stage, *index), *handle);
                                        }
                                        Cmd::BindUniformBuffer { stage, index, addr, size } => {
                                            st.uniform_buffers.insert((*stage, *index), (*addr, *size));
                                        }
                                        Cmd::BindStorageBuffer { stage, index, addr, size } => {
                                            if (0..6).contains(stage) && (0..16).contains(index) {
                                                st.storage_buffers[*stage as usize][*index as usize] = (*addr, *size);
                                            }
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            ModelItem::Unit { list, .. } => {
                                let c = cur.clone();
                                self.run_list(st, &c, *list, depth + 1);
                                if let Some(new_cb) = &st.cur_cb {
                                    if Retained::as_ptr(&cur) != Retained::as_ptr(new_cb) {
                                        cur = new_cb.clone();
                                    }
                                }
                            }
                            ModelItem::Bind { stage, index, kind } => apply_model_bind(st, *stage, *index, *kind),
                            ModelItem::RenderState(r) => apply_render_state(st, r),
                            ModelItem::Program { program, stages } => apply_program(st, *program, *stages),
                            ModelItem::Targets { colors, depth } => self.set_render_targets(st, colors, depth.as_ref()),
                            ModelItem::Fixed(f) => apply_fixed(st, *f),
                            ModelItem::UpdateUniform { addr, offset, data } => update_uniform(st, *addr, *offset, data),
                            ModelItem::PointSize(sz) => st.point_size = *sz,
                            ModelItem::PrimitiveRestart { enable, index } => st.prim_restart = enable.then_some(*index),
                            ModelItem::Barrier(bits) => {
                                if bits & 0x10 != 0 {
                                    st.fb_epoch += 1;
                                }
                            }
                            ModelItem::RenderEnable(e) => {
                                if journal_on() && st.render_enabled != *e {
                                    journal(|| format!("  render-enable {}", e));
                                }
                                st.render_enabled = *e;
                            }
                            ModelItem::RenderEnableConditional { mode, counters } => {
                                self.render_enable_conditional(st, *mode, *counters)
                            }
                            ModelItem::ResetCounter(counter) => {
                                if *counter == 1 && self.real_samples() {
                                    self.occl_reset(st);
                                }
                            }
                            ModelItem::ReportCounter { counter, addr } => self.report_counter(st, *counter, *addr),
                            ModelItem::Dispatch { x, y, z } => {
                                if translated::enabled() {
                                    self.end_encoder(st);
                                    let c = cur.clone();
                                    self.dispatch_compute(st, &c, *x, *y, *z);
                                }
                            }
                            ModelItem::Clear { .. } | ModelItem::ClearDepth { .. } if !st.render_enabled => {
                                journal(|| format!("  cond-skip program {:#x}", st.program.unwrap_or(0)));
                            }
                            ModelItem::Clear { index, rgba, mask } => {
                                st.stats.clears += 1;
                                let index = match st.color_dups.get(*index as usize) {
                                    Some(Some(i)) => *i as i32,
                                    _ => *index,
                                };
                                let c = cur.clone();
                                self.clear(st, &c, index, *rgba, *mask);
                            }
                            ModelItem::ClearDepth { depth, depth_mask, stencil, stencil_mask } => {
                                st.stats.clears += 1;
                                let c = cur.clone();
                                self.clear_depth(st, &c, *depth, *depth_mask, *stencil, *stencil_mask);
                            }
                            ModelItem::Viewport { x, y, w, h, near, far } => {
                                st.scissor = Some((*x, *y, *w, *h));
                                set_viewport(st, *x, *y, *w, *h);
                                st.depth_range = (*near, *far);
                            }
                            ModelItem::VertexAttribs(a) => st.vertex_attribs = *a,
                            ModelItem::VertexStreams(v) => st.vertex_streams.clone_from(v),
                            ModelItem::TexturesDeferred { stage, first, count, handles } => {
                                apply_textures_deferred(st, *stage, *first, *count, *handles)
                            }
                            // Conditional rendering off: draws are skipped as the commands' are.
                            ModelItem::Draw(_) | ModelItem::DrawArrays { .. } if !st.render_enabled => {
                                journal(|| format!("  cond-skip program {:#x}", st.program.unwrap_or(0)));
                            }
                            ModelItem::DrawArrays { mode, first, count, base_instance, instances } => {
                                st.cur_caller = 0;
                                st.stats.draws += 1;
                                if st.occl.counting {
                                    st.occl.draws_seen += 1;
                                }
                                if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                                    self.draw_arrays(st, &cur, *mode, *first, *count, *base_instance, *instances);
                                }
                            }
                            ModelItem::Draw(r) => {
                                st.cur_caller = 0;
                                st.stats.draws += 1;
                                if st.occl.counting {
                                    st.occl.draws_seen += 1;
                                }
                                if gfx_mode() >= GfxMode::Draws || gfx_mode() == GfxMode::Dry {
                                    self.draw_elements(
                                        st,
                                        &cur,
                                        r.mode,
                                        r.index_type,
                                        r.count,
                                        r.index_addr,
                                        r.base_vertex,
                                        r.base_instance,
                                        r.instances,
                                    );
                                }
                            }
                        }
                    }
                }
                Cmd::CallCommands(handles) => {
                    // --log-draw-reuse: the call path through lists from earlier frames (per-frame lists change
                    // every frame and are left out).
                    let saved_ctx = st.call_ctx;
                    if reuse_log && list_old {
                        st.call_ctx = st.call_ctx.rotate_left(17)
                            ^ (std::sync::Arc::as_ptr(&list) as u64)
                                .wrapping_mul(0x9e3779b97f4a7c15)
                            ^ op_index as u64;
                    }
                    for h in handles {
                        self.run_list(st, cb, *h, depth + 1);
                    }
                    st.call_ctx = saved_ctx;
                    if let Some(new_cb) = &st.cur_cb {
                        if Retained::as_ptr(&cur) != Retained::as_ptr(new_cb) {
                            cur = new_cb.clone();
                        }
                    }
                }
            }
        }
        if let Some((n, t)) = cmd_t {
            note_cmd_time(n, t.elapsed().as_nanos() as u64);
        }
    }

    /// nvnCommandBufferSetRenderEnableConditional (and the native item).
    pub(super) unsafe fn render_enable_conditional(&self, st: &mut State, mode: i32, counters: u64) {
        let (mode, counters) = (&mode, &counters);
        // Counter reports earlier in the stream were written when they executed, so this sees them in order,
        // except SAMPLES_PASSED reports: those are written when their command buffer completes, so a
        // condition on one from this submit or an uncompleted one is not known yet while encoding. The GPU
        // would evaluate it after the report; Metal has no predication, so such draws render (Ryujinx's
        // fallback when a counter is not available). Skipping them instead would cull TotK's Hyrule Field grass
        // patches (both reports still read 0).
        // Reports of command buffers the fence watcher resolves sit in `occl_watched` until it writes them; a
        // condition on one reads stale memory until then, so it counts as unresolved too.
        let unresolved = |a: u64| {
            st.occl.pending.iter().any(|(_, r)| *r == a)
                || st.inflight.iter().any(|f| {
                    f.occl.iter().any(|(_, r)| *r == a)
                        || f.occl_watched.as_ref().is_some_and(|w| {
                            w.lock()
                                .unwrap()
                                .as_ref()
                                .is_some_and(|v| v.iter().any(|(_, r)| *r == a))
                        })
                })
        };
        let pending = *counters > 0x10000
            && (unresolved(*counters) || unresolved(*counters + 16));
        st.render_enabled = if pending {
            true
        } else if *counters > 0x10000 {
            // Report pages held by --report-guard are read through their aliases (the engine never waits).
            let p = report_guard::engine_ptr(*counters as usize) as *const u64;
            let equal = p.read_unaligned()
                == (report_guard::engine_ptr(*counters as usize + 16) as *const u64)
                    .read_unaligned();
            equal == (*mode == 0)
        } else {
            true
        };
        let read2 = |c: u64| unsafe {
            (
                (report_guard::engine_ptr(c as usize) as *const u64).read_unaligned(),
                (report_guard::engine_ptr(c as usize + 16) as *const u64)
                    .read_unaligned(),
            )
        };
        if journal_on() && *counters > 0x10000 {
            let (a, b) = read2(*counters);
            let en = st.render_enabled;
            journal(|| {
                format!(
                    "  cond mode {} @{:#x} [{:#x}, {:#x}]{} -> {}",
                    mode,
                    counters,
                    a,
                    b,
                    if pending { " pending" } else { "" },
                    en
                )
            });
        }
        if tracing_frame() && *counters > 0x10000 {
            let (a, b) = read2(*counters);
            println!("[cmdtrace]   conditional mode={} @{:#x} [{:#x}, {:#x}]{} -> render_enabled={}", mode, counters, a, b, if pending { " (report pending)" } else { "" }, st.render_enabled);
        }
    }

    /// nvnCommandBufferReportCounter (and the native item).
    pub(super) unsafe fn report_counter(&self, st: &mut State, counter: i32, addr: u64) {
        let (counter, addr) = (&counter, &addr);
        // Reports are written into guest memory by the engine: older render-target holds there end.
        rt_guard::superseded(crate::nvn::api::translate_virtual(*addr as usize), 16);
        if *counter == 1 && self.real_samples() {
            self.occl_report(st, *addr);
        } else {
            write_counter(*counter, *addr, None);
            if *counter == 0 && gpu_timestamps() && *addr >= 0x10000 {
                // Rewritten with the GPU's time once the command buffer completes (the CPU time stays as a
                // provisional value).
                st.ts_pending.push((*addr, st.lb.enc_labels.is_empty()));
            }
            if *counter == 1 {
                report_guard::close(crate::nvn::api::translate_virtual(*addr as usize));
            }
        }
        st.plan_ubo.invalidate(*addr, *addr + 16);
        st.cx.ubo_copies.invalidate(*addr, *addr + 16);
        st.ubo_plan_epoch += 1;
        GUEST_WRITES.fetch_add(1, Ordering::Relaxed);
    }

    /// nvnCommandBufferSetRenderTargets (and the native targets item): the pass's render targets.
    pub(super) unsafe fn set_render_targets(&self, st: &mut State, colors: &[crate::nvn::cmd::RtRef], depth: Option<&crate::nvn::cmd::RtRef>) {
        let resolve = |r: &crate::nvn::cmd::RtRef| crate::nvn::cmd::RtRef {
            texture: crate::nvn::api::resolve_texture_ref(r.texture, r.gen),
            ..*r
        };
        let colors: Vec<crate::nvn::cmd::RtRef> = colors.iter().map(resolve).collect();
        let depth = depth.map(resolve);
        if let Some(w) = watch_storage() {
            for (i, c) in colors.iter().enumerate() {
                if tex_info(c.texture).is_some_and(|(_, t)| t.storage == w) {
                    println!("[watch] f{} render target c{} = texture 0x{:x} (storage {:#x})", presented_frames() + 1, i, c.texture, w);
                    // --watch-trace: trace the rest of this frame (and --trace-trigger-frames after it).
                    if crate::settings::get().trace.watch_trace
                        && TRACE_TRIGGERED_FRAME
                            .load(std::sync::atomic::Ordering::Relaxed)
                            == 0
                    {
                        TRACE_TRIGGERED_FRAME.store(
                            presented_frames() + 1,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                    }
                }
            }
        }
        // Debug: --log-rt-frames=a-b logs every render-target bind (storage, size, format) in frames a..b.
        if let Some(crate::settings::Span {
            first: fa,
            last: fb,
        }) = crate::settings::get().log.log_rt_frames
        {
            let f = presented_frames() + 1;
            if (fa..=fb).contains(&f) {
                for (i, c) in colors.iter().enumerate() {
                    if let Some((_, t)) = tex_info(c.texture) {
                        println!("[rt-log] f{} c{} storage {:#x} {}x{}x{} fmt {} levels {} level {} layer {} view_format {}", f, i, t.storage, t.width, t.height, t.depth, t.format, t.levels, c.level, c.layer, c.view_format);
                    }
                }
            }
        }
        // Render targets in CPU-visible memory reach guest memory when the CPU touches them (metal/rt_guard.rs).
        // Every target (depth too) ends older holds on its memory.
        if rt_guard::enabled() {
            for c in colors.iter().chain(depth.iter()) {
                let Some((_, t)) = tex_info(c.texture) else { continue };
                if let Some(tex) = self.texture(&mut st.tc, c.texture) {
                    rt_guard::hold(&tex, &t, c.level as usize, c.layer as usize);
                }
            }
        }
        if rt_writeback_enabled() {
            let f = presented_frames() + 1;
            for c in colors.iter() {
                let Some((_, t)) = tex_info(c.texture) else {
                    continue;
                };
                // The guard covers targets in CPU-visible pools (rt_guard::hold); the rest keep this.
                if rt_guard::enabled() && crate::nvn::api::memory_pool_cpu_writable(t.storage) {
                    continue;
                }
                // Only targets not rendered in the previous 4 frames: per-frame targets, swapchain images and
                // ping-pong pairs are skipped after their first use, textures rendered once (generated maps)
                // are written back.
                let prev = st.rt_history.insert(t.storage, f);
                if prev.map_or(true, |p| p + 4 < f) {
                    st.writeback.0.insert(c.texture, f);
                }
            }
            if st.rt_history.len() > 1 << 14 {
                st.rt_history.retain(|_, &mut v| v + 600 > f);
            }
        }
        if let Some((ww, wh)) = watch_size() {
            for (i, c) in colors.iter().enumerate() {
                if let Some((_, t)) = tex_info(c.texture)
                    .filter(|(_, t)| t.width as u32 == ww && t.height as u32 == wh)
                {
                    println!("[watch-size] f{} render target c{} = texture {:#x} storage {:#x} fmt {} levels {} view {:?}",
                        presented_frames() + 1, i, c.texture, t.storage, t.format, t.levels, c);
                }
            }
        }
        // Two slots naming one surface (same storage, level and layer): on the GPU both write the same
        // memory and a slot a shader does not write leaves it alone. Two Metal attachments of one texture
        // each store their own tile at the end of the pass, so an unwritten slot would store its stale tile
        // over the other's output (Mario Kart 8 Deluxe binds its G-buffer texture at slots 3 and 4 and writes
        // only 3). The later slot stays unattached; its clears go to the earlier one.
        let mut colors = colors;
        let surface = |c: &crate::nvn::cmd::RtRef| {
            tex_info(c.texture).map(|(_, t)| (t.storage, c.level, c.layer))
        };
        let surfaces: Vec<_> = colors.iter().map(surface).collect();
        st.color_dups = (0..colors.len())
            .map(|j| match surfaces[j] {
                Some(s) if s.0 != 0 => (0..j).find(|&i| surfaces[i] == Some(s)),
                _ => None,
            })
            .collect();
        for (j, d) in st.color_dups.iter().enumerate() {
            if d.is_some() {
                colors[j] = crate::nvn::cmd::RtRef::default();
            }
        }
        st.colors = colors;
        st.depth = depth;
        st.rts_dirty = true;
        st.cached_rts = None;
    }

    pub(super) unsafe fn draw_elements(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        mode: i32,
        index_type: i32,
        count: i32,
        index_addr: u64,
        base_vertex: i32,
        base_instance: i32,
        instances: i32,
    ) {
        if translated::enabled()
            && self.draw_translated(
                st,
                cb,
                mode,
                translated::DrawKind::Elements {
                    index_type,
                    count,
                    index_addr,
                    base_vertex,
                    base_instance,
                    instances,
                },
            )
        {
            return;
        }
        // No translation: the draw is skipped (translated.rs warns once per program).
    }

    pub(super) unsafe fn draw_arrays(
        &self,
        st: &mut State,
        cb: &ProtocolObject<dyn MTLCommandBuffer>,
        mode: i32,
        first: i32,
        count: i32,
        base_instance: i32,
        instances: i32,
    ) {
        if translated::enabled()
            && self.draw_translated(
                st,
                cb,
                mode,
                translated::DrawKind::Arrays {
                    first,
                    count,
                    base_instance,
                    instances,
                },
            )
        {
            return;
        }
        // No translation: the draw is skipped (translated.rs warns once per program).
    }
}
pub(crate) fn apply_vertex_input(st: &mut State, v: &crate::nvn::cmd::VertexInput) {
    st.vertex_attribs = v.attribs;
    st.vertex_streams.clone_from(&v.streams);
    for &(i, a, n) in &v.buffers {
        st.vertex_buffers.insert(i, (a, n));
    }
}

pub(crate) fn apply_render_state(st: &mut State, r: &crate::nvn::cmd::RenderState) {
    for &b in &r.blend {
        st.blend_states[(b & 7) as usize] = b;
    }
    st.color_state = r.color;
    st.channel_mask = r.channel_mask;
    st.blend_color = r.blend_color;
    st.depth_stencil = r.depth_stencil;
    if let Some((mask, reference)) = r.stencil {
        st.stencil = [[reference; 2], [mask; 2], [0xff; 2]];
    }
    st.polygon = r.polygon;
    st.polygon_offset = r.polygon_offset;
    st.depth_clamp = r.depth_clamp;
}

pub(crate) fn apply_model_bind(st: &mut State, stage: i32, index: i32, kind: crate::nvn::cmd::ModelBindKind) {
    use crate::nvn::cmd::ModelBindKind;
    match kind {
        ModelBindKind::Uniform { addr, size } => {
            st.uniform_buffers.insert((stage, index), (addr, size));
        }
        ModelBindKind::Storage { addr, size } => {
            if (0..6).contains(&stage) && (0..16).contains(&index) {
                st.storage_buffers[stage as usize][index as usize] = (addr, size);
            }
        }
        ModelBindKind::Texture(handle) => {
            st.textures_bound.insert((stage, index), handle);
        }
        ModelBindKind::Vertex { addr, size } => {
            st.vertex_buffers.insert(index, (addr, size));
        }
    }
}

pub(crate) fn apply_program(st: &mut State, program: usize, stages: i32) {
    // NVN_SHADER_STAGE_COMPUTE_BIT: compute and graphics programs are bound independently.
    if stages & 0x20 != 0 {
        st.compute_program = Some(program);
    }
    if stages & 0x1F != 0 || stages == 0 {
        st.program = Some(program);
    }
}

pub(crate) unsafe fn apply_textures_deferred(st: &mut State, stage: i32, first: i32, count: i32, handles: u64) {
    for i in 0..count.clamp(0, 32) {
        let h = if handles > 0x10000 { std::ptr::read_unaligned((handles as *const u64).add(i as usize)) } else { 0 };
        st.textures_bound.insert((stage, first + i), h);
    }
}

pub(crate) fn set_viewport(st: &mut State, x: i32, y: i32, w: i32, h: i32) {
    st.viewport = Some((x, y, w, h));
    // The main 3D passes (3+ colour targets: the G-buffer) show the game's render resolution (its dynamic resolution
    // scales the viewport): reported with the fps line.
    if st.colors.len() >= 3 && w > 0 && h > 0 {
        // Largest since the last report (area in the high bits).
        let (w, h) = ((w as u64).min(0xFFFFF), (h as u64).min(0xFFFFF));
        SCENE_VIEWPORT.fetch_max((w * h) << 40 | w << 20 | h, Ordering::Relaxed);
    }
}

pub(crate) fn apply_fixed(st: &mut State, f: crate::nvn::cmd::FixedState) {
    use crate::nvn::cmd::FixedState;
    match f {
        FixedState::Blend(v) => st.blend_states[(v & 7) as usize] = v,
        FixedState::Color(v) => st.color_state = v,
        FixedState::ChannelMask(v) => st.channel_mask = v,
        FixedState::BlendColor(c) => st.blend_color = c,
        FixedState::DepthStencil(v) => st.depth_stencil = v,
        FixedState::Stencil { kind, faces, value } => {
            // NVNface: FRONT 1, BACK 2, FRONT_AND_BACK 3.
            let k = (kind as usize).min(2);
            if faces & 1 != 0 {
                st.stencil[k][0] = value as u8;
            }
            if faces & 2 != 0 {
                st.stencil[k][1] = value as u8;
            }
        }
        FixedState::DepthBounds { enable, min, max } => st.depth_bounds = enable.then_some((min, max)),
        FixedState::Polygon(v) => st.polygon = v,
        FixedState::PolygonOffset { factor, units, clamp } => st.polygon_offset = (factor, units, clamp),
        FixedState::Multisample(v) => st.multisample = v,
        FixedState::DepthClamp(v) => st.depth_clamp = v,
        FixedState::Scissor { x, y, w, h } => st.scissor = Some((x, y, w, h)),
        FixedState::Viewport { x, y, w, h } => set_viewport(st, x, y, w, h),
        FixedState::DepthRange { near, far } => st.depth_range = (near, far),
    }
}

pub(crate) unsafe fn update_uniform(st: &mut State, addr: u64, offset: isize, data: &[u8]) {
    if addr > 0x10000 && !data.is_empty() {
        let dest = (addr as isize + offset) as *mut u8;
        std::ptr::copy_nonoverlapping(data.as_ptr(), dest, data.len());
        // Plan-side copies and encode-side copies overlapping the written range are stale now.
        let (lo, hi) = (dest as u64, dest as u64 + data.len() as u64);
        st.plan_ubo.invalidate(lo, hi);
        st.cx.ubo_copies.invalidate(lo, hi);
        st.ubo_plan_epoch += 1;
        GUEST_WRITES.fetch_add(1, Ordering::Relaxed);
    }
}
