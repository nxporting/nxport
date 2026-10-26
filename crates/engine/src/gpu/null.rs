// A backend that draws nothing: submitted work completes at once, and presents only count frames. For running a game
// where there is no GPU backend yet (lifted hosts: docs/windows-port.md, step 6), so its CPU side can be brought up,
// and for measuring that side alone (--gpu=null).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use super::{Backend, ShaderStage};

pub struct Null {
    serial: AtomicU64,
    frames: AtomicU64,
    /// When the last 50 frames began, for the frame rate.
    interval: Mutex<Option<std::time::Instant>>,
}

pub static NULL: Null = Null { serial: AtomicU64::new(0), frames: AtomicU64::new(0), interval: Mutex::new(None) };

impl Backend for Null {
    fn set_program(&self, _program: usize, _stages: &[ShaderStage]) {}

    fn submit(&self, _lists: &[u64]) {
        self.serial.fetch_add(1, Ordering::AcqRel);
    }

    fn present(&self, _texture: usize, _crop: [i32; 4]) {
        let n = self.frames.fetch_add(1, Ordering::AcqRel) + 1;
        // The frame rate as the Vulkan backend reports it; with --exit-on-dump the last --dump-frame ends the run
        // (there is no image to write).
        if n % 50 == 0 {
            let now = std::time::Instant::now();
            let mut start = self.interval.lock().unwrap();
            if let Some(s) = *start {
                println!("[+] Gfx: present #{n} {:.1} fps (null)", 50.0 / now.duration_since(s).as_secs_f64());
            }
            *start = Some(now);
        }
        let dump = &crate::settings::get().dump;
        if dump.exit_on_dump && dump.dump_frame.iter().max() == Some(&n) {
            println!("[+] Dump complete, exiting on request (--exit-on-dump)");
            std::process::exit(0);
        }
    }

    fn submitted_serial(&self) -> u64 {
        self.serial.load(Ordering::Acquire)
    }

    fn wait_serial(&self, _serial: u64, _timeout_ns: u64) -> bool {
        true
    }

    fn wait_fence(&self, _serial: u64, _timeout_ns: u64) -> bool {
        true
    }

    fn presented_frames(&self) -> u64 {
        self.frames.load(Ordering::Acquire)
    }
}
