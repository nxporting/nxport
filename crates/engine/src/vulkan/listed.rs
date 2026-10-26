// Pipeline list entries (gpu::pipeline_list; the format: crates/pipelines) for this backend: each new pipeline is
// recorded as an entry, and from the first draw the listed entries' pipelines are made on worker threads. An entry is
// turned into the same stage translations and pipeline description a draw with its state makes (draw.rs's state
// functions, target_format as exec.rs resolves targets); entries whose stages this game has not shown yet (no code in
// the stage cache) are skipped.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use ash::vk;
use nxport_pipelines::Entry;
use sass2spv::accessor::GraphicsState;

use super::device::Dev;
use super::exec::{desc_streams, fragment_graphics_state, vertex_graphics_state, vertex_streams};
use super::shader::{self, Stage};
use super::{format, pipelines, stage_cache, state, texture};

/// The shader cache name of a stage.
pub(super) fn name(full: u64) -> String {
    format!("s_{full:016x}")
}

fn full_of(name: &str) -> Option<u64> {
    u64::from_str_radix(name.strip_prefix("s_")?, 16).ok()
}

/// The format a target of NVN format `tex` viewed as `view` (0: its own) renders in, and the NVN format its fragment
/// outputs follow (as exec.rs's target and bc_proxy resolve targets).
pub(super) unsafe fn target_format(dev: &Dev, tex: u32, view: u32) -> Option<(vk::Format, i32)> {
    let (tex, view) = (tex as i32, view as i32);
    if tex == 0 {
        return None;
    }
    if view != 0 {
        if let (Some(i), Some(v)) = (crate::nvn::layout::format_info(tex), crate::nvn::layout::format_info(view)) {
            if i.block_w > 1 && v.block_w == 1 && v.bytes == i.bytes {
                if let Some(f) = texture::device_format(dev, view) {
                    return Some((f, view));
                }
            }
        }
    }
    let own = texture::device_format(dev, tex)?;
    let fmt = if view != 0 && format::view_compatible(tex, view) {
        texture::device_format(dev, view).filter(|f| format::aspects(*f) == format::aspects(own)).unwrap_or(own)
    } else {
        own
    };
    let rendered = if view != 0 && texture::device_format(dev, view) == Some(fmt) { view } else { tex };
    Some((fmt, rendered))
}

/// An entry's color target formats (unbound slots after the last target left out), the NVN formats they render in
/// by slot, and its depth format.
unsafe fn targets(dev: &Dev, n: &nxport_pipelines::Nvn) -> (Vec<vk::Format>, Vec<(usize, i32)>, vk::Format) {
    let mut colors = Vec::new();
    let mut rendered = Vec::new();
    for (i, &(t, v)) in n.colors.iter().enumerate() {
        match target_format(dev, t, v) {
            Some((f, r)) => {
                colors.push(f);
                rendered.push((i, r));
            }
            None => colors.push(vk::Format::UNDEFINED),
        }
    }
    while colors.last() == Some(&vk::Format::UNDEFINED) {
        colors.pop();
    }
    let depth = target_format(dev, n.depth.0, n.depth.1).map_or(vk::Format::UNDEFINED, |f| f.0);
    (colors, rendered, depth)
}

/// The vertex stage's translation state for an entry (`inputs`: the stage's input locations).
pub(super) fn vertex_state(e: &Entry, inputs: &[u32]) -> GraphicsState {
    vertex_graphics_state(inputs.iter().zip(&e.attr_bits).filter_map(|(&l, b)| b.map(|b| (l as usize, b))), e.depth_gl)
}

/// The fragment stage's translation state for an entry and whether its discarding fragments write depth.
pub(super) unsafe fn fragment_state(dev: &Dev, e: &Entry) -> Option<(GraphicsState, bool)> {
    let (_, rendered, depth) = targets(dev, e.nvn.as_ref()?);
    let discard = dev.discard_writes_depth && depth != vk::Format::UNDEFINED && format::aspects(depth).contains(vk::ImageAspectFlags::DEPTH);
    Some((fragment_graphics_state(rendered.into_iter()), discard))
}

/// The pipeline description of an entry with these stages.
pub(super) unsafe fn desc(dev: &Dev, e: &Entry, v: &Stage, f: Option<&Stage>) -> Option<pipelines::Desc> {
    let n = e.nvn.as_ref()?;
    let topology = state::topology(n.mode as i32)?;
    let restart = n.restart && matches!(topology, vk::PrimitiveTopology::LINE_STRIP | vk::PrimitiveTopology::TRIANGLE_STRIP | vk::PrimitiveTopology::TRIANGLE_FAN);
    let attribs = v.known.inputs.iter().zip(&e.attr_bits).filter_map(|(&l, b)| b.map(|b| (l, b)));
    let vl = vertex_streams(dev, attribs, &e.streams);
    let (colors, _, depth) = targets(dev, n);
    let (blend, color_state, channel_mask, depth_stencil) =
        pipelines::normalize(&e.blends, e.color_state, Some(e.channel_mask), Some(n.depth_stencil), &colors, depth);
    Some(pipelines::Desc {
        vs: v.id,
        fs: f.map(|f| f.id),
        topology: topology.as_raw(),
        restart,
        streams: desc_streams(&vl),
        blend,
        color_state,
        channel_mask,
        depth_stencil,
        polygon: n.polygon,
        multisample: n.multisample,
        depth_clamp: n.depth_clamp,
        colors: colors.iter().map(|f| f.as_raw()).collect(),
        depth: depth.as_raw(),
        samples: n.samples,
        upper_left: !e.y_negate,
        gl_depth: e.depth_gl,
    })
}

/// The translation keys an entry gives its stages (Stage::id.key), for checking recorded entries.
pub(super) unsafe fn stage_keys(dev: &Dev, e: &Entry, v: &Stage, f: Option<&Stage>) -> Option<(u64, Option<u64>)> {
    let vk_ = shader::translation_key(dev, &v.known.prep, v.known.full, &Some(vertex_state(e, &v.known.inputs)), 0, 0, None, false);
    let fk = match f {
        Some(f) => {
            let (fs, discard) = fragment_state(dev, e)?;
            let varyings = dev.link_varyings.then_some(v.outputs);
            Some(shader::translation_key(dev, &f.known.prep, f.known.full, &Some(fs), 0, shader::FRAGMENT_BINDING_BASE, varyings, discard))
        }
        None => None,
    };
    Some((vk_, fk))
}

/// The stages of an entry, translated as a draw with its state would have them (from the stage cache's code).
unsafe fn stages(dev: &Dev, e: &Entry) -> Option<(Stage, Option<Stage>)> {
    let get = |name: &str, state: &GraphicsState, base: u32, inputs: Option<u32>, discard: bool| -> Option<Stage> {
        let k = stage_cache::by_name(full_of(name)?)?;
        let code = stage_cache::code(k.full)?;
        shader::get(dev, &code, k.prep, Some(k.clone()), &|_| Some(state.clone()), 0, base, inputs, discard).ok()
    };
    let vk_ = stage_cache::by_name(full_of(&e.vs)?)?;
    let v = get(&e.vs, &vertex_state(e, &vk_.inputs), 0, None, false)?;
    let f = match &e.fs {
        Some(fs) => {
            let (state, discard) = fragment_state(dev, e)?;
            Some(get(fs, &state, shader::FRAGMENT_BINDING_BASE, dev.link_varyings.then_some(v.outputs), discard)?)
        }
        None => None,
    };
    Some((v, f))
}

static TARGET: OnceLock<(Arc<Dev>, vk::PipelineCache)> = OnceLock::new();

/// Hands the list to the workers (once, at the first draw).
pub(super) fn start(dev: Arc<Dev>, cache: vk::PipelineCache) {
    if TARGET.set((dev, cache)).is_ok() {
        crate::gpu::pipeline_list::precompile(compile);
    }
}

fn compile(entries: Vec<Entry>) {
    let Some((dev, cache)) = TARGET.get().cloned() else { return };
    let total = entries.iter().filter(|e| e.nvn.is_some()).count();
    let queue = Arc::new(Mutex::new(entries));
    let built = Arc::new(AtomicUsize::new(0));
    let threads = std::thread::available_parallelism().map_or(2, |n| (n.get() / 3).clamp(1, 4));
    let running = Arc::new(AtomicUsize::new(threads));
    let t0 = std::time::Instant::now();
    for i in 0..threads {
        let (dev, queue, built, running) = (dev.clone(), queue.clone(), built.clone(), running.clone());
        let _ = std::thread::Builder::new().name(format!("vk-pipe-compile{i}")).spawn(move || unsafe {
            loop {
                let Some(e) = queue.lock().unwrap().pop() else { break };
                let Some((v, f)) = stages(&dev, &e) else { continue };
                let Some(d) = desc(&dev, &e, &v, f.as_ref()) else { continue };
                let Some(lay) = pipelines::made_layout(v.id, f.as_ref().map(|f| f.id)).or_else(|| {
                    let l = pipelines::layout(&dev, &v, f.as_ref())?;
                    Some(pipelines::put_layout(v.id, f.as_ref().map(|f| f.id), l))
                }) else {
                    continue;
                };
                if let Ok(p) = pipelines::create(&dev, &d, &v, f.as_ref(), lay.1, cache) {
                    pipelines::put(super::exec::hash_of(&d), p, lay.1);
                    built.fetch_add(1, Ordering::Relaxed);
                }
            }
            if running.fetch_sub(1, Ordering::AcqRel) == 1 {
                println!(
                    "[vulkan] pipeline list: made {} of {total} entries' pipelines in {:.1} s (the others name stages not seen in this game yet)",
                    built.load(Ordering::Relaxed),
                    t0.elapsed().as_secs_f64()
                );
            }
        });
    }
}
