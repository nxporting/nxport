// The driver's share of recording, on a thread of its own (vk-record): the Presentation thread works out what each draw
// binds and hands the commands that bind and draw it to this thread in batches, which it records in order.

use super::draw::{write_set, DescWrites};
use super::*;

/// Commands handed over at a time.
const BATCH: usize = 512;

/// Small (copied into the batch for every command): larger arguments are boxed or kept in the batch's `words`.
pub(super) enum Op {
    /// Begins recording into the command buffer.
    Begin(vk::CommandBuffer),
    /// Ends it (Recorder::wait_ended, try_ended).
    End,
    Pipeline(vk::PipelineBindPoint, vk::Pipeline),
    /// Binds set 0 with its dynamic offsets (`words[start..start + len]`).
    Sets(vk::PipelineBindPoint, vk::PipelineLayout, vk::DescriptorSet, u32, u32),
    /// Writes the descriptors of a set allocated for them (before a command binds it).
    Write(vk::DescriptorSet, Box<DescWrites>),
    /// Pushes the descriptors.
    Push(vk::PipelineBindPoint, vk::PipelineLayout, Box<DescWrites>),
    VertexBuffer(u32, vk::Buffer, u64),
    IndexBuffer(vk::Buffer, u64, vk::IndexType),
    Draw(u32, u32, u32, u32),
    DrawIndexed(u32, u32, u32, i32, u32),
    Barrier,
    Checkpoint(&'static str, Box<[u64; 4]>),
    /// Anything else (owning what it records).
    Cmd(Box<dyn FnOnce(&Dev, vk::CommandBuffer)>),
}

/// Ops own what they record (handles, plain values); the Presentation thread no longer touches them once handed over.
#[derive(Default)]
struct Batch {
    ops: Vec<Op>,
    words: Vec<u32>,
}
unsafe impl Send for Batch {}

pub(super) struct Recorder {
    batch: Batch,
    tx: std::sync::mpsc::Sender<Batch>,
    spare: std::sync::mpsc::Receiver<Batch>,
    ended: std::sync::mpsc::Receiver<()>,
}

impl Recorder {
    pub(super) fn new(dev: std::sync::Arc<Dev>) -> Result<Recorder, String> {
        let (tx, rx) = std::sync::mpsc::channel::<Batch>();
        let (spare_tx, spare) = std::sync::mpsc::channel();
        let (ended_tx, ended) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("vk-record".into())
            .spawn(move || {
                let mut cb = vk::CommandBuffer::null();
                while let Ok(mut b) = rx.recv() {
                    for op in b.ops.drain(..) {
                        unsafe { run(&dev, &mut cb, op, &b.words, &ended_tx) };
                    }
                    b.words.clear();
                    let _ = spare_tx.send(b);
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Recorder { batch: Batch { ops: Vec::with_capacity(BATCH), words: Vec::new() }, tx, spare, ended })
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

    /// Binds set 0 with `offsets`.
    pub(super) fn sets(&mut self, point: vk::PipelineBindPoint, layout: vk::PipelineLayout, set: vk::DescriptorSet, offsets: &[u32]) {
        let start = self.batch.words.len() as u32;
        self.batch.words.extend_from_slice(offsets);
        self.op(Op::Sets(point, layout, set, start, offsets.len() as u32));
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

unsafe fn run(dev: &Dev, cb: &mut vk::CommandBuffer, op: Op, words: &[u32], ended: &std::sync::mpsc::Sender<()>) {
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
        Op::Pipeline(point, p) => d.cmd_bind_pipeline(c, point, p),
        Op::Sets(point, layout, set, start, len) => {
            d.cmd_bind_descriptor_sets(c, point, layout, 0, &[set], &words[start as usize..(start + len) as usize])
        }
        Op::Write(set, w) => write_set(dev, c, set, &w, None),
        Op::Push(point, layout, w) => write_set(dev, c, vk::DescriptorSet::null(), &w, Some((point, layout))),
        Op::VertexBuffer(binding, b, o) => d.cmd_bind_vertex_buffers(c, binding, &[b], &[o]),
        Op::IndexBuffer(b, o, ty) => d.cmd_bind_index_buffer(c, b, o, ty),
        Op::Draw(n, i, first, base) => d.cmd_draw(c, n, i, first, base),
        Op::DrawIndexed(n, i, first, base_vertex, base) => d.cmd_draw_indexed(c, n, i, first, base_vertex, base),
        Op::Barrier => dev.full_barrier(c),
        Op::Checkpoint(what, args) => dev.checkpoint(c, what, *args),
        Op::Cmd(f) => f(dev, c),
    }
}
