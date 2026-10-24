// The interface between the NVN frontend (crate::nvn) and a GPU backend (crate::metal). The frontend records command
// lists and hands them over at submit; the backend executes them, signals fences by serial and presents. The backend
// also keeps guest memory coherent with the GPU's view of it, through page guards consulted by the fault handler and
// by write tracking.

// Lifted hosts until they have a GPU backend, and frame capture scenarios (which record commands without running them).
pub mod null;

use std::sync::OnceLock;

/// A guest memory access that faulted on a protected page.
#[derive(Clone, Copy, Debug)]
pub struct Fault {
    pub addr: usize,
    pub pc: usize,
    pub lr: usize,
    pub fp: usize,
    pub write: bool,
}

/// One stage of a program as the guest gave it (nvnProgramSetShaders): its code and its compiler control section.
#[derive(Clone, Copy, Debug)]
pub struct ShaderStage {
    pub data: u64,
    pub control: u64,
}

/// One level / layer of a texture in the guest's texel encoding (its NVN format), rows tightly packed.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Texels {
    pub width: u32,
    pub height: u32,
    pub format: i32,
    pub data: Vec<u8>,
}

/// A texture level / layer whose contents only the backend holds: GPU work wrote it and guest memory does not have it.
pub struct GpuOnlyTexture {
    pub storage: usize,
    pub params: crate::nvn::layout::TextureParams,
    pub level: u32,
    pub layer: u32,
    pub texels: Texels,
    /// Frame it was last rendered in (0: unknown); of textures sharing memory, the latest one is what memory holds.
    pub last_written: u64,
}

pub trait Backend: Send + Sync {
    /// The stages of program object `program` (its guest address) changed.
    fn set_program(&self, program: usize, stages: &[ShaderStage]);
    /// Executes recorded command lists (nvnQueueSubmitCommands) in order.
    fn submit(&self, lists: &[u64]);
    /// Shows a window texture (the guest address of its NVN texture object), cropped to `crop` (x, y, w, h; 0 = whole).
    fn present(&self, texture: usize, crop: [i32; 4]);
    /// Serial of the newest submitted GPU work. Fences record it when they are placed.
    fn submitted_serial(&self) -> u64;
    /// Waits until all work up to `serial` has completed; false on timeout. A timeout of 0 polls.
    fn wait_serial(&self, serial: u64, timeout_ns: u64) -> bool;
    /// The guest's fence wait (nvnSyncWait). May return before `wait_serial` would, when the backend overlaps frames
    /// and keeps guest memory coherent by other means.
    fn wait_fence(&self, serial: u64, timeout_ns: u64) -> bool;
    /// Frames presented so far.
    fn presented_frames(&self) -> u64;

    /// A fault on a page the backend protects for GPU coherence, consulted before write tracking. True if handled:
    /// the access is retried.
    fn access_fault(&self, _f: &Fault) -> bool {
        false
    }
    /// Diagnostic traps, consulted after write tracking.
    fn late_access_fault(&self, _f: &Fault) -> bool {
        false
    }
    /// Which of the backend's page guards know `addr` (crash reports).
    fn describe_page(&self, _addr: usize) -> String {
        String::new()
    }
    /// Write tracking changed the protection of these pages; guards that need them unreadable reapply theirs.
    fn pages_reprotected(&self, _addr: usize, _len: usize) {}
    /// The engine is about to write guest memory itself (file reads, HLE output).
    fn before_host_write(&self, _addr: usize, _len: usize) {}
    /// GPU work will write this range: guards that hold it must let go.
    fn release_for_gpu(&self, _addr: usize, _len: usize) {}

    /// Level `level`, layer `layer` of the texture a recorded reference names (`texture` with its snapshot generation
    /// `gen`, as RtRef / TexRef hold them), after all submitted work has completed. None when the backend cannot read
    /// that texture or format back.
    fn read_texture(&self, _texture: usize, _gen: u32, _level: u32, _layer: u32) -> Option<Texels> {
        None
    }
    /// Every texture level / layer whose contents only the backend holds, after all submitted work has completed. A
    /// frame capture writes them into its copy of guest memory, where the Switch's GPU would have left them. Returns
    /// how many it skipped (formats or kinds it cannot read back).
    fn gpu_only_textures(&self, _f: &mut dyn FnMut(GpuOnlyTexture)) -> usize {
        0
    }
    /// The GPU's own time for the last completed submit, in ns, where the backend measures it.
    fn last_submit_gpu_ns(&self) -> Option<u64> {
        None
    }
    /// Turns per-pass GPU timing of whole submits on or off; returns the last profiled submit's passes (GPU ns,
    /// description), for backends that measure it.
    fn profile_passes(&self, _on: bool) -> Option<Vec<(u64, String)>> {
        None
    }
    /// CPU time of the last submit, in ns: recording it, then waiting for the GPU (backends that measure it).
    fn last_submit_cpu_ns(&self) -> Option<(u64, u64)> {
        None
    }
}

static BACKEND: OnceLock<&'static dyn Backend> = OnceLock::new();

/// Installs the backend; called once, before the guest starts (it may submit before the backend has a device or
/// window, and the backend must then queue or drop that work itself).
pub fn install(b: &'static dyn Backend) {
    if BACKEND.set(b).is_err() {
        panic!("GPU backend installed twice");
    }
}

/// The backend, installed before any guest code runs.
pub fn backend() -> &'static dyn Backend {
    *BACKEND
        .get()
        .expect("GPU backend used before it was installed")
}
