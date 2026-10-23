// crate::gpu::Backend for the Metal executor.

use crate::gpu::{Backend, Fault, ShaderStage};

pub(crate) struct Metal;
pub(crate) static METAL: Metal = Metal;

impl Backend for Metal {
    fn set_program(&self, program: usize, stages: &[ShaderStage]) {
        super::programs::register(program, stages);
    }

    fn submit(&self, lists: &[u64]) {
        super::submit(lists);
    }

    fn present(&self, texture: usize, crop: [i32; 4]) {
        super::present(texture, crop);
    }

    fn submitted_serial(&self) -> u64 {
        super::submitted_serial()
    }

    fn wait_serial(&self, serial: u64, timeout_ns: u64) -> bool {
        super::wait_serial(serial, timeout_ns)
    }

    fn wait_fence(&self, serial: u64, timeout_ns: u64) -> bool {
        // --replay: the frame is being re-executed; the game's thread must not move on until it is done.
        while super::replay_hold() {
            std::thread::sleep(std::time::Duration::from_micros(200));
        }
        if timeout_ns == 0 {
            return super::wait_fence(serial, 0);
        }
        let t0 = std::time::Instant::now();
        let done = super::wait_fence(serial, timeout_ns);
        if super::timeline_enabled() {
            use std::sync::atomic::Ordering::Relaxed;
            super::TL_SYNC_US.fetch_add(t0.elapsed().as_micros() as u64, Relaxed);
            if serial >= super::submitted_serial() {
                // Waited for the newest submit: the time since it was encoded is the GPU tail.
                if let Some(us) = super::since_last_submit_us() {
                    super::TL_TAIL_US.fetch_add(us, Relaxed);
                }
            }
        }
        done
    }

    fn presented_frames(&self) -> u64 {
        super::presented_frames()
    }

    fn access_fault(&self, f: &Fault) -> bool {
        unsafe {
            super::report_guard::fault(f.addr, f.pc, f.lr, f.write)
                || super::rt_guard::fault(f.addr, f.pc, f.lr, super::report_guard::may_wait(f.pc), f.write)
                || super::tex_backing::fault(f.addr, f.pc, f.lr, f.write)
                // dirty_track would claim pages it once tracked and unprotect them without waiting: this goes first.
                || super::fence_guard::fault(f.addr)
        }
    }

    fn late_access_fault(&self, f: &Fault) -> bool {
        super::early_fence()
            && unsafe { super::early_fence_fault(f.addr, f.pc, f.write, f.lr, f.fp) }
    }

    fn describe_page(&self, addr: usize) -> String {
        super::describe_fault_page(addr)
    }

    fn pages_reprotected(&self, addr: usize, len: usize) {
        unsafe {
            super::report_guard::reprotect_held(addr, len);
            super::tex_backing::disc_reprotect(addr, len);
            super::rt_guard::reprotect_held(addr, len);
        }
    }

    fn before_host_write(&self, addr: usize, len: usize) {
        unsafe {
            super::fence_guard::before_host_write(addr, len);
            super::tex_backing::open_overlapping(addr, len);
        }
    }

    fn release_for_gpu(&self, addr: usize, len: usize) {
        unsafe { super::tex_backing::open_overlapping(addr, len) };
    }
}
