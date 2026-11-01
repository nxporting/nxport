// The driver's share of recording, on a thread of its own (vk-record): the Presentation thread works out what each draw
// binds and hands the commands that bind and draw it to this thread in batches, which it records in order. Descriptor
// sets are found or written here too (bind).

use super::draw::{write_set, DescWrites};
use super::*;

/// Commands handed over at a time.
const BATCH: usize = 512;

/// Small (copied into the batch for every command): larger arguments are boxed.
pub(super) enum Op {
    /// Begins recording into the command buffer.
    Begin(vk::CommandBuffer),
    /// Ends it (Recorder::wait_ended, try_ended).
    End,
    /// A submit begins: sets written for the one before are not used again (their pools are reset once it completed).
    Submit,
    /// What the command buffer has bound is not known (a new one).
    Unbound,
    Pipeline(vk::PipelineBindPoint, vk::Pipeline),
    /// Binds a draw's or dispatch's descriptors (bind).
    Bind(Box<BindJob>),
    VertexBuffer(u32, vk::Buffer, u64),
    IndexBuffer(vk::Buffer, u64, vk::IndexType),
    Draw(u32, u32, u32, u32),
    DrawIndexed(u32, u32, u32, i32, u32),
    Barrier,
    Checkpoint(&'static str, Box<[u64; 4]>),
    /// Copies guest memory (src, into dst, len bytes) as it was at uniform epoch `epoch` (Ctx::ubo_epoch), for the draws
    /// recorded after it: copy_guest.
    Fill(usize, usize, u32, u64),
    /// Bindless textures: pushes the batch's words at this index with this layout (Recorder::words).
    Words(vk::PipelineLayout, u32),
    /// Anything else (owning what it records).
    Cmd(Box<dyn FnOnce(&Dev, vk::CommandBuffer)>),
}

/// A draw's or dispatch's descriptors, bound at `point` with `layout`: pushed, or in a set of `set_layout`. Handed back
/// for reuse once bound (Recorder::job).
pub(super) struct BindJob {
    pub(super) w: DescWrites,
    pub(super) point: vk::PipelineBindPoint,
    pub(super) layout: vk::PipelineLayout,
    pub(super) set_layout: vk::DescriptorSetLayout,
    pub(super) push: bool,
    /// The set it binds (1 for graphics with bindless textures: the global set is 0).
    pub(super) first_set: u32,
}

/// Ops own what they record (handles, plain values); the Presentation thread no longer touches them once handed over.
#[derive(Default)]
struct Batch {
    ops: Vec<Op>,
    /// Bindless textures: the push constant words Op::Words name (kept here, not boxed per draw).
    words: Vec<Words>,
}

/// A draw's bindless texture words (sass2spv::accessor::BINDLESS_PUSH_WORDS).
pub(super) type Words = [u32; sass2spv::accessor::BINDLESS_PUSH_WORDS as usize];
unsafe impl Send for Batch {}
unsafe impl Send for BindJob {}

/// Descriptor pools: those the submits being recorded allocated sets from (taken with the submit, Recorder::used_pools)
/// and reset ones to allocate from again (Recorder::give_pool).
#[derive(Default)]
struct Pools {
    used: std::sync::Mutex<Vec<vk::DescriptorPool>>,
    free: std::sync::Mutex<Vec<vk::DescriptorPool>>,
}

pub(super) struct Recorder {
    batch: Batch,
    tx: std::sync::mpsc::Sender<Batch>,
    spare: std::sync::mpsc::Receiver<Batch>,
    ended: std::sync::mpsc::Receiver<()>,
    jobs: std::sync::mpsc::Receiver<Box<BindJob>>,
    pools: std::sync::Arc<Pools>,
}

impl Recorder {
    pub(super) fn new(dev: std::sync::Arc<Dev>) -> Result<Recorder, String> {
        let (tx, rx) = std::sync::mpsc::channel::<Batch>();
        let (spare_tx, spare) = std::sync::mpsc::channel();
        let (ended_tx, ended) = std::sync::mpsc::channel();
        let (jobs_tx, jobs) = std::sync::mpsc::channel();
        let pools = std::sync::Arc::new(Pools::default());
        let shared = pools.clone();
        std::thread::Builder::new()
            .name("vk-record".into())
            .spawn(move || {
                let mut cb = vk::CommandBuffer::null();
                let mut sets = Sets::default();
                let mut pinned = false;
                while let Ok(mut b) = rx.recv() {
                    // On the Presentation Thread's L3 cache once its first submit chose it (vulkan::REC_AFFINITY).
                    if !pinned {
                        let mask = crate::vulkan::rec_affinity();
                        if mask != 0 {
                            unsafe { crate::vulkan::pin_current_thread(mask) };
                            pinned = true;
                        }
                    }
                    for op in b.ops.drain(..) {
                        match op {
                            Op::Words(layout, i) => unsafe {
                                let w = &b.words[i as usize];
                                let bytes = std::slice::from_raw_parts(w.as_ptr() as *const u8, std::mem::size_of_val(w));
                                dev.device.cmd_push_constants(cb, layout, vk::ShaderStageFlags::ALL_GRAPHICS, 0, bytes);
                            },
                            Op::Bind(mut j) => unsafe {
                                bind(&dev, cb, &mut sets, &shared, &j);
                                j.w.buffers.clear();
                                j.w.images.clear();
                                let _ = jobs_tx.send(j);
                            },
                            op => unsafe { run(&dev, &mut cb, &mut sets, op, &ended_tx) },
                        }
                    }
                    b.words.clear();
                    let _ = spare_tx.send(b);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Recorder { batch: Batch { ops: Vec::with_capacity(BATCH), words: Vec::new() }, tx, spare, ended, jobs, pools })
    }

    pub(super) fn op(&mut self, op: Op) {
        self.batch.ops.push(op);
        if self.batch.ops.len() >= BATCH {
            self.flush();
        }
    }

    /// A command recorded from what `f` owns.
    pub(super) fn cmd(&mut self, f: impl FnOnce(&Dev, vk::CommandBuffer) + 'static) {
        self.op(Op::Cmd(Box::new(f)));
    }

    /// Copies guest memory [src, src + len) into `dst` as it is at uniform epoch `epoch`, before the commands after it.
    pub(super) fn fill(&mut self, src: usize, dst: *mut u8, len: usize, epoch: u64) {
        self.op(Op::Fill(src, dst as usize, len as u32, epoch));
    }

    /// Pushes the bindless texture words `w` for the draws after it (Op::Words).
    pub(super) fn words(&mut self, layout: vk::PipelineLayout, w: &Words) {
        self.batch.words.push(*w);
        let i = self.batch.words.len() as u32 - 1;
        self.op(Op::Words(layout, i));
    }

    /// An empty BindJob to fill (one vk-record handed back, else a new one).
    pub(super) fn job(&mut self) -> Box<BindJob> {
        let j = self.jobs.try_recv().unwrap_or_else(|_| {
            Box::new(BindJob {
                w: DescWrites::default(),
                point: vk::PipelineBindPoint::GRAPHICS,
                layout: vk::PipelineLayout::null(),
                set_layout: vk::DescriptorSetLayout::null(),
                push: false,
                first_set: 0,
            })
        });
        // Its memory was last in vk-record's cache: fetched for writing now, while the draw's descriptors are worked out.
        #[cfg(target_arch = "x86_64")]
        {
            let p = &*j as *const BindJob as *const i8;
            for o in (0..std::mem::size_of::<BindJob>()).step_by(64) {
                unsafe { std::arch::x86_64::_mm_prefetch::<{ std::arch::x86_64::_MM_HINT_ET0 }>(p.add(o)) };
            }
        }
        j
    }

    /// The pools the submit allocated sets from (once vk-record ended its last command buffer).
    pub(super) fn used_pools(&self) -> Vec<vk::DescriptorPool> {
        std::mem::take(&mut *self.pools.used.lock().unwrap())
    }

    /// A pool reset after the submit that used it completed.
    pub(super) fn give_pool(&self, p: vk::DescriptorPool) {
        self.pools.free.lock().unwrap().push(p);
    }

    fn flush(&mut self) {
        if self.batch.ops.is_empty() {
            return;
        }
        let next = self.spare.try_recv().unwrap_or_else(|_| Batch { ops: Vec::with_capacity(BATCH), words: Vec::new() });
        let b = std::mem::replace(&mut self.batch, next);
        let _ = self.tx.send(b);
    }

    /// Ends the command buffer, without waiting for it (wait_ended, try_ended).
    pub(super) fn end_async(&mut self) {
        self.op(Op::End);
        self.flush();
    }

    /// Waits until the oldest command buffer ended and not waited for is recorded (it can be submitted).
    pub(super) fn wait_ended(&mut self) {
        let _ = self.ended.recv();
    }

    /// Whether the oldest command buffer ended and not waited for is recorded (counting it as waited for if so).
    pub(super) fn try_ended(&mut self) -> bool {
        self.ended.try_recv().is_ok()
    }
}

/// vk-record's descriptor state: sets written in this submit by contents, the pool they come from, and per bind point
/// (graphics, compute) the set bound with its layout and dynamic offsets.
#[derive(Default)]
struct Sets {
    memo: rustc_hash::FxHashMap<u64, vk::DescriptorSet>,
    pool: Option<vk::DescriptorPool>,
    bound: [Option<(vk::PipelineLayout, vk::DescriptorSet, smallvec::SmallVec<[u32; 32]>)>; 2],
}

/// Binds `j`'s descriptors into `cb`: a set with the same contents written earlier in this submit is bound again (draws of
/// one mesh or material), and the set bound last stays bound across pipeline changes (same layout).
unsafe fn bind(dev: &Dev, cb: vk::CommandBuffer, s: &mut Sets, pools: &Pools, j: &BindJob) {
    let w = &j.w;
    if w.buffers.is_empty() && w.images.is_empty() {
        return;
    }
    let key = (!j.push).then(|| {
        use std::hash::Hasher;
        let mut h = rustc_hash::FxHasher::default();
        h.write_u64(vk::Handle::as_raw(j.set_layout));
        for (binding, ty, info) in &w.buffers {
            h.write_u32(*binding);
            h.write_i32(ty.as_raw());
            h.write_u64(vk::Handle::as_raw(info.buffer));
            if *ty != vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC {
                h.write_u64(info.offset);
            }
            h.write_u64(info.range);
        }
        for (binding, ty, infos) in &w.images {
            h.write_u32(*binding);
            h.write_i32(ty.as_raw());
            for i in infos {
                h.write_u64(vk::Handle::as_raw(i.sampler));
                h.write_u64(vk::Handle::as_raw(i.image_view));
            }
        }
        h.finish()
    });
    // Dynamic offsets in binding order.
    let mut dynamic: smallvec::SmallVec<[(u32, u32); 32]> =
        w.buffers.iter().filter(|b| b.1 == vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC).map(|b| (b.0, b.2.offset as u32)).collect();
    dynamic.sort_unstable_by_key(|d| d.0);
    let offsets: smallvec::SmallVec<[u32; 32]> = dynamic.iter().map(|d| d.1).collect();
    let point = (j.point == vk::PipelineBindPoint::COMPUTE) as usize;
    let set = match key.and_then(|k| s.memo.get(&k)) {
        Some(&set) => {
            if s.bound[point].as_ref().is_some_and(|(l, b, o)| *l == j.layout && *b == set && *o == offsets) {
                return;
            }
            set
        }
        None if j.push => {
            write_set(dev, cb, vk::DescriptorSet::null(), w, Some((j.point, j.layout)));
            s.bound[point] = None;
            return;
        }
        None => {
            let Some(set) = alloc_set(dev, s, pools, j.set_layout) else { return };
            if let Some(k) = key {
                s.memo.insert(k, set);
            }
            write_set(dev, cb, set, w, None);
            set
        }
    };
    dev.device.cmd_bind_descriptor_sets(cb, j.point, j.layout, j.first_set, &[set], &offsets);
    s.bound[point] = Some((j.layout, set, offsets));
}

unsafe fn alloc_set(dev: &Dev, s: &mut Sets, pools: &Pools, layout: vk::DescriptorSetLayout) -> Option<vk::DescriptorSet> {
    let layouts = [layout];
    let alloc = |p: vk::DescriptorPool| {
        dev.device.allocate_descriptor_sets(&vk::DescriptorSetAllocateInfo::default().descriptor_pool(p).set_layouts(&layouts)).ok().map(|s| s[0])
    };
    if let Some(set) = s.pool.and_then(alloc) {
        return Some(set);
    }
    let reset = pools.free.lock().unwrap().pop();
    let p = match reset {
        Some(p) => p,
        None => {
            let sizes = [
                vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER, descriptor_count: 1024 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_BUFFER_DYNAMIC, descriptor_count: 16384 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_BUFFER, descriptor_count: 8192 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER, descriptor_count: 16384 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_IMAGE, descriptor_count: 2048 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::UNIFORM_TEXEL_BUFFER, descriptor_count: 1024 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::STORAGE_TEXEL_BUFFER, descriptor_count: 1024 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLED_IMAGE, descriptor_count: 4096 },
                vk::DescriptorPoolSize { ty: vk::DescriptorType::SAMPLER, descriptor_count: 4096 },
            ];
            dev.device.create_descriptor_pool(&vk::DescriptorPoolCreateInfo::default().max_sets(2048).pool_sizes(&sizes), None).ok()?
        }
    };
    pools.used.lock().unwrap().push(p);
    s.pool = Some(p);
    alloc(p)
}

unsafe fn run(dev: &Dev, cb: &mut vk::CommandBuffer, sets: &mut Sets, op: Op, ended: &std::sync::mpsc::Sender<()>) {
    let d = &dev.device;
    let c = *cb;
    match op {
        Op::Begin(b) => {
            *cb = b;
            let _ = d.begin_command_buffer(b, &vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT));
        }
        Op::End => {
            let _ = d.end_command_buffer(c);
            *cb = vk::CommandBuffer::null();
            let _ = ended.send(());
        }
        Op::Submit => {
            sets.memo.clear();
            sets.pool = None;
            sets.bound = Default::default();
        }
        Op::Unbound => sets.bound = Default::default(),
        Op::Pipeline(point, p) => d.cmd_bind_pipeline(c, point, p),
        // Bound by the vk-record loop, which hands the job back.
        Op::Bind(_) => unreachable!(),
        Op::Words(..) => unreachable!(),
        Op::VertexBuffer(binding, b, o) => d.cmd_bind_vertex_buffers(c, binding, &[b], &[o]),
        Op::IndexBuffer(b, o, ty) => d.cmd_bind_index_buffer(c, b, o, ty),
        Op::Draw(n, i, first, base) => d.cmd_draw(c, n, i, first, base),
        Op::DrawIndexed(n, i, first, base_vertex, base) => d.cmd_draw_indexed(c, n, i, first, base_vertex, base),
        Op::Barrier => dev.full_barrier(c),
        Op::Checkpoint(what, args) => dev.checkpoint(c, what, *args),
        Op::Fill(src, dst, len, epoch) => copy_guest(src, dst as *mut u8, len as usize, epoch),
        Op::Cmd(f) => f(dev, c),
    }
}

/// Bytes of guest memory the submit's own writes (inline uniform updates, reports) replaced while it was recorded:
/// (uniform epoch the write began, address, previous contents). Copies vk-record makes later than the commands
/// before such a write put them back. Cleared when a submit begins (vk-record finished the one before).
static OVERWRITTEN: std::sync::Mutex<Vec<(u64, usize, Vec<u8>)>> = std::sync::Mutex::new(Vec::new());
/// The latest epoch in OVERWRITTEN (0: none), so copies made at it or later skip the lock.
static OVERWRITTEN_LATEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Guest memory [a, a + len) is about to be written by the submit's commands, beginning uniform epoch `epoch`; its
/// contents are read at `from` (the engine's alias of a guarded report page, else `a`).
pub(super) unsafe fn overwriting(epoch: u64, a: usize, from: *const u8, len: usize) {
    let old = std::slice::from_raw_parts(from, len).to_vec();
    OVERWRITTEN.lock().unwrap().push((epoch, a, old));
    OVERWRITTEN_LATEST.store(epoch, std::sync::atomic::Ordering::SeqCst);
}

/// A submit begins.
pub(super) fn clear_overwritten() {
    OVERWRITTEN.lock().unwrap().clear();
    OVERWRITTEN_LATEST.store(0, std::sync::atomic::Ordering::SeqCst);
}

/// A copy of guest memory [src, src + len) at `dst` as the commands of uniform epoch `epoch` saw it: pages the game wrote
/// since the submit began take their saved contents (guard::patch), and what the submit's own later writes replaced is
/// put back (latest first, so the earliest of them, which held the epoch's bytes, is last).
unsafe fn copy_guest(src: usize, dst: *mut u8, len: usize, epoch: u64) {
    std::ptr::copy_nonoverlapping(src as *const u8, dst, len);
    crate::vulkan::guard::patch(src, dst, len);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    if OVERWRITTEN_LATEST.load(std::sync::atomic::Ordering::SeqCst) <= epoch {
        return;
    }
    for (e, a, old) in OVERWRITTEN.lock().unwrap().iter().rev() {
        let (x, y) = ((*a).max(src), (a + old.len()).min(src + len));
        if *e > epoch && x < y {
            std::ptr::copy_nonoverlapping(old.as_ptr().add(x - a), dst.add(x - src), y - x);
        }
    }
}
