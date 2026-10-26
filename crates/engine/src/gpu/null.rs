// A backend that draws nothing: submitted work completes at once, and presents only count frames. For running a game
// where there is no GPU backend yet (lifted hosts: docs/windows-port.md, step 6), so its CPU side can be brought up.

use std::sync::atomic::{AtomicU64, Ordering};

use super::{Backend, ShaderStage};

pub struct Null {
    serial: AtomicU64,
    frames: AtomicU64,
}

pub static NULL: Null = Null { serial: AtomicU64::new(0), frames: AtomicU64::new(0) };

impl Backend for Null {
    fn set_program(&self, _program: usize, _stages: &[ShaderStage]) {}

    fn submit(&self, _lists: &[u64]) {
        self.serial.fetch_add(1, Ordering::AcqRel);
    }

    fn present(&self, _texture: usize, _crop: [i32; 4]) {
        let n = self.frames.fetch_add(1, Ordering::AcqRel) + 1;
        if n % 300 == 0 {
            println!("[+] GPU (null): {n} frames presented");
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
