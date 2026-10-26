// Asynchronous graphics pipeline compilation for the Vulkan backend (--async-pipelines).
// Missing pipelines compile on worker threads (`vk-pipe-compileN`), the draw waits up to a per-frame budget
// (`--async-wait-us`), and the draw is skipped until the pipeline is ready.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use ash::vk;

use super::device::Dev;
use super::pipelines::{self, Desc};
use super::shader::Stage;

pub(super) fn is_async() -> bool {
    crate::host::overlay::is_async_shaders()
}

pub(super) enum AsyncPipe {
    Ready(vk::Pipeline),
    Pending,
    Failed,
}

enum SlotState {
    Pending,
    Running,
    Ready(vk::Pipeline),
    Failed,
}

struct Slot(Mutex<SlotState>, Condvar);

impl Slot {
    fn new() -> Arc<Slot> {
        Arc::new(Slot(Mutex::new(SlotState::Pending), Condvar::new()))
    }

    fn finish(&self, built: Option<vk::Pipeline>) {
        *self.0.lock().unwrap() = match built {
            Some(p) => SlotState::Ready(p),
            None => SlotState::Failed,
        };
        self.1.notify_all();
    }
}

pub(super) struct PipeJob {
    pub dev: Arc<Dev>,
    pub desc: Desc,
    pub vstage: Stage,
    pub gstage: Option<Stage>,
    pub fstage: Option<Stage>,
    pub layout: vk::PipelineLayout,
    pub cache: vk::PipelineCache,
}

struct SendJob(PipeJob, Arc<Slot>);

unsafe impl Send for SendJob {}

#[derive(Default)]
struct Queue {
    draws: VecDeque<SendJob>,
    slots: HashMap<u64, Arc<Slot>>,
}

static ASYNC_QUEUE: OnceLock<(Mutex<Queue>, Condvar)> = OnceLock::new();
pub(super) static ASYNC_STARTED: AtomicU64 = AtomicU64::new(0);
pub(super) static ASYNC_SKIPS: AtomicU64 = AtomicU64::new(0);
pub(super) static ASYNC_WAITED_US: AtomicU64 = AtomicU64::new(0);

fn async_queue() -> &'static (Mutex<Queue>, Condvar) {
    ASYNC_QUEUE.get_or_init(|| {
        let threads = std::thread::available_parallelism().map_or(2, |n| (n.get() / 3).clamp(1, 4));
        for i in 0..threads {
            std::thread::Builder::new()
                .name(format!("vk-async-compile{i}"))
                .spawn(move || loop {
                    let job = {
                        let (q, cv) = async_queue();
                        let mut g = q.lock().unwrap();
                        loop {
                            if let Some(j) = g.draws.pop_front() {
                                break j;
                            }
                            g = cv.wait(g).unwrap();
                        }
                    };
                    let SendJob(j, slot) = job;
                    let take = {
                        let mut st = slot.0.lock().unwrap();
                        if matches!(*st, SlotState::Pending) {
                            *st = SlotState::Running;
                            true
                        } else {
                            false
                        }
                    };
                    if take {
                        let res = unsafe {
                            pipelines::create(&j.dev, &j.desc, &j.vstage, j.gstage.as_ref(), j.fstage.as_ref(), j.layout, j.cache)
                        };
                        match res {
                            Ok(p) => {
                                pipelines::put(super::exec::hash_of(&j.desc), p, j.layout);
                                slot.finish(Some(p));
                            }
                            Err(_) => {
                                slot.finish(None);
                            }
                        }
                    }
                })
                .expect("spawn vk-async-compile thread");
        }
        (Mutex::new(Queue::default()), Condvar::new())
    })
}

pub(super) fn query_async_pipeline(key: u64, job: PipeJob) -> AsyncPipe {
    let (q, cv) = async_queue();
    let (slot, new) = {
        let mut g = q.lock().unwrap();
        match g.slots.get(&key) {
            Some(s) => (s.clone(), false),
            None => {
                let s = Slot::new();
                g.slots.insert(key, s.clone());
                (s, true)
            }
        }
    };
    if new {
        ASYNC_STARTED.fetch_add(1, Ordering::Relaxed);
        let mut g = q.lock().unwrap();
        g.draws.push_back(SendJob(job, slot.clone()));
        drop(g);
        cv.notify_one();
    }

    static WAIT: OnceLock<u64> = OnceLock::new();
    let budget = *WAIT.get_or_init(|| crate::settings::get().graphics.async_wait_us);
    let left = budget.saturating_sub(ASYNC_WAITED_US.load(Ordering::Relaxed));
    let t0 = std::time::Instant::now();
    let g = slot.0.lock().unwrap();
    let (g, _) = slot
        .1
        .wait_timeout_while(
            g,
            std::time::Duration::from_micros(if new { left } else { 0 }),
            |s| matches!(s, SlotState::Pending | SlotState::Running),
        )
        .unwrap();
    ASYNC_WAITED_US.fetch_add(t0.elapsed().as_micros() as u64, Ordering::Relaxed);
    let r = match *g {
        SlotState::Ready(p) => AsyncPipe::Ready(p),
        SlotState::Failed => AsyncPipe::Failed,
        SlotState::Pending | SlotState::Running => AsyncPipe::Pending,
    };
    drop(g);
    if matches!(r, AsyncPipe::Pending) {
        ASYNC_SKIPS.fetch_add(1, Ordering::Relaxed);
    }
    r
}
