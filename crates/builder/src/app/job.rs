// A build job: its steps (each a named closure run on the worker thread), progress reports, cancelling, and running
// the steps in order.

use super::*;

/// A build's progress: its steps, the running one's sub-step, and its outcome.
#[derive(Default)]
pub struct Job {
    pub step: String,
    pub steps_done: usize,
    pub steps: usize,
    pub sub_step: Option<String>,
    pub sub_done: usize,
    pub sub_total: usize,
    pub sub_frac: f32,
    pub result: Option<Result<PathBuf, String>>,
}

impl Job {
    /// Where the running build is, in one line (empty before the first step).
    pub(crate) fn status(&self) -> String {
        if let Some(sub) = &self.sub_step {
            if self.sub_total > 0 {
                format!("{} ({} of {}): {} ({}/{})", self.step, self.steps_done + 1, self.steps, sub, self.sub_done, self.sub_total)
            } else {
                format!("{} ({} of {}): {}", self.step, self.steps_done + 1, self.steps, sub)
            }
        } else if self.steps > 0 {
            format!("{} ({} of {})", self.step, self.steps_done + 1, self.steps)
        } else {
            String::new()
        }
    }
}

/// Sets the running step's sub-step: `done` of `total` (0: no count), which is `frac` of the step.
#[allow(dead_code)]
pub fn progress(job: &Mutex<Job>, sub: impl Into<String>, done: usize, total: usize, frac: f32) {
    let mut j = job.lock().unwrap();
    j.sub_step = Some(sub.into());
    j.sub_done = done;
    j.sub_total = total;
    j.sub_frac = j.sub_frac.max(frac.min(1.0));
}

/// Sets the running step's sub-step for a build of `phases` phases in order: `done` of `total` in phase `phase`.
#[allow(dead_code)]
pub fn phase_progress(job: &Mutex<Job>, sub: String, phase: usize, phases: usize, done: usize, total: usize) {
    let in_phase = if total > 0 { (done as f32 / total as f32).min(1.0) } else { 0.0 };
    progress(job, sub, done, total, (phase as f32 + in_phase) / phases.max(1) as f32);
}

/// Threads a shader cache build uses: all but two of the CPU's, at least two.
#[allow(dead_code)]
pub fn shader_jobs() -> usize {
    std::thread::available_parallelism().map_or(6, |n| n.get()).saturating_sub(2).max(2)
}

/// Stop after the running step.
pub(crate) static CANCEL: AtomicBool = AtomicBool::new(false);

pub type Step = (String, Box<dyn FnOnce() -> Result<(), String> + Send>);

/// What the build makes, once its last step is done.
pub type Output = Arc<Mutex<Option<PathBuf>>>;

pub(crate) fn run_steps(job: &Arc<Mutex<Job>>, steps: Vec<Step>) -> Result<(), String> {
    for (i, (name, f)) in steps.into_iter().enumerate() {
        if CANCEL.load(Ordering::Relaxed) {
            return Err("Stopped".into());
        }
        {
            let mut j = job.lock().unwrap();
            j.step = name.clone();
            j.steps_done = i;
            j.sub_step = None;
            j.sub_done = 0;
            j.sub_total = 0;
            j.sub_frac = 0.0;
        }
        println!("== {name}");
        f().map_err(|e| format!("{name}: {e}"))?;
        {
            let mut j = job.lock().unwrap();
            j.steps_done = i + 1;
            j.sub_step = None;
            j.sub_done = 0;
            j.sub_total = 0;
            j.sub_frac = 0.0;
        }
    }
    Ok(())
}
