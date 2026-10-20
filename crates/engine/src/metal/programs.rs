// Registry of NVN programs: for each program object, its stages (code and control section, as the game gave
// nvnProgramSetShaders[Ext]) with their translated stage from the shader cache. Draws look programs up by address.
//
// With --dump-shaders=dir, every distinct stage's control section and the first 64KB of code
// are written to `dir` (code size isn't passed through NVN; the Maxwell decompiler follows control
// flow from the entry so an over-long window is fine).

use rustc_hash::FxHashMap as HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use super::shader_cache::{self, CachedStage};

#[derive(Clone, Copy, Debug)]
pub struct StageCode {
    pub data: u64, // NVNbufferAddress == host pointer of the code (starts with the 0x50-byte SPH)
    pub control: u64, // host pointer to the GLSLC control section
    /// Translated Metal stage from the AOT shader cache, if this exact code is in it.
    pub cached: Option<&'static CachedStage>,
}

static CACHE_HITS: AtomicU64 = AtomicU64::new(0);
static CACHE_MISSES: AtomicU64 = AtomicU64::new(0);

static PROGRAMS: OnceLock<Mutex<HashMap<usize, std::sync::Arc<Vec<StageCode>>>>> = OnceLock::new();
static DUMPED: OnceLock<Mutex<std::collections::HashSet<u64>>> = OnceLock::new();

/// Bytes to dump of a stage's GLSLC control section: its full size (the u32 at +0x20, as --dump-ctrl reads it;
/// the cbuf 1 offset the cache builder needs sits at +0x700), or nothing without a control section.
unsafe fn control_section_len(control: usize) -> usize {
    if control < 0x1_0000_0000 {
        return 0;
    }
    (*((control + 0x20) as *const u32) as usize).clamp(0x400, 0x4000)
}

fn programs() -> &'static Mutex<HashMap<usize, std::sync::Arc<Vec<StageCode>>>> {
    PROGRAMS.get_or_init(|| Mutex::new(HashMap::default()))
}

pub fn register(program: usize, stages: &[crate::gpu::ShaderStage]) {
    let mut stages: Vec<StageCode> = stages
        .iter()
        .map(|s| StageCode {
            data: s.data,
            control: s.control,
            cached: None,
        })
        .collect();
    for (i, st) in stages.iter_mut().enumerate() {
        st.cached = unsafe { shader_cache::lookup(st.data) };
        if st.cached.is_none() {
            st.cached = unsafe { shader_cache::lookup_or_translate_dynamic(st.data, st.control, i) };
        }
        let (hits, misses) = match st.cached {
            Some(_) => (
                CACHE_HITS.fetch_add(1, Ordering::Relaxed) + 1,
                CACHE_MISSES.load(Ordering::Relaxed),
            ),
            None => (
                CACHE_HITS.load(Ordering::Relaxed),
                CACHE_MISSES.fetch_add(1, Ordering::Relaxed) + 1,
            ),
        };
        if st.cached.is_none() && misses <= 400 {
            let weak = if st.data >= 0x1_0000_0000 {
                shader_cache::fnv1a64(unsafe {
                    std::slice::from_raw_parts(st.data as *const u8, 0x130)
                })
            } else {
                0
            };
            println!(
                "[shader-cache] MISS program=0x{:x} stage#{} data=0x{:x} weak={:016x}",
                program, i, st.data, weak
            );
        }
        if st.cached.is_none() {
            dump_missing(st, i);
        }
        if (hits + misses) % 500 == 0 {
            println!(
                "[shader-cache] {} hits / {} misses over {} stages",
                hits,
                misses,
                hits + misses
            );
        }
    }
    if let Some(dir) = &crate::settings::get().dump.dump_shaders {
        let dumped = DUMPED.get_or_init(|| Mutex::new(Default::default()));
        let mut seen = dumped.lock().unwrap();
        let _ = std::fs::create_dir_all(&dir);
        for (i, st) in stages.iter().enumerate() {
            if st.data < 0x1_0000_0000 || !seen.insert(st.data) {
                continue;
            }
            unsafe {
                // Stages near the end of a mapping: a fixed 64 KiB window would read past it.
                let code = std::slice::from_raw_parts(st.data as *const u8, crate::cpu::crash_handler::readable_len(st.data as usize, 0x10000));
                let ctrl = std::slice::from_raw_parts(
                    st.control as *const u8,
                    control_section_len(st.control as usize),
                );
                let tag = format!("{:012x}", st.data);
                let _ = std::fs::write(dir.join(format!("{}_s{}.code", tag, i)), code);
                let _ = std::fs::write(dir.join(format!("{}_s{}.ctrl", tag, i)), ctrl);
            }
        }
    }
    // --dump-ctrl=dir: the full GLSLC control section of each distinct stage (size at +0x20), named by the code's
    // weak cache hash so it can be matched with shader_cache/index.txt offline.
    if let Some(dir) = &crate::settings::get().dump.dump_ctrl {
        static SEEN: OnceLock<Mutex<std::collections::HashSet<u64>>> = OnceLock::new();
        let mut seen = SEEN
            .get_or_init(|| Mutex::new(Default::default()))
            .lock()
            .unwrap();
        let _ = std::fs::create_dir_all(&dir);
        for st in stages.iter() {
            if st.data < 0x1_0000_0000 || st.control < 0x1_0000_0000 || !seen.insert(st.data) {
                continue;
            }
            unsafe {
                let size = (*((st.control + 0x20) as *const u32) as usize).clamp(0x100, 0x4000);
                let ctrl = std::slice::from_raw_parts(st.control as *const u8, size);
                let weak =
                    shader_cache::fnv1a64(std::slice::from_raw_parts(st.data as *const u8, 0x130));
                let _ = std::fs::write(dir.join(format!("{:016x}.ctrl", weak)), ctrl);
            }
        }
    }
    programs()
        .lock()
        .unwrap()
        .insert(program, std::sync::Arc::new(stages));
}

/// A stage the cache lacks draws nothing (its program's draws are skipped): write it to
/// shader_dump_missing/ in the game's data directory (named by weak hash, once) so the next cache build (nxport-cli
/// shaders build with that directory) translates it. Every play session collects what it was missing; games use stage
/// variants that depend on settings and places. --no-miss-dump disables it; --miss-dump-dir picks the directory.
fn dump_missing(st: &StageCode, index: usize) {
    static OFF: OnceLock<bool> = OnceLock::new();
    if *OFF.get_or_init(|| crate::settings::get().files.no_miss_dump) || st.data < 0x1_0000_0000 {
        return;
    }
    static DIR: OnceLock<Option<std::path::PathBuf>> = OnceLock::new();
    let Some(dir) = DIR
        .get_or_init(|| crate::settings::get().files.miss_dump_dir.clone().or_else(|| crate::profile::data_subdir("shader_dump_missing")))
        .as_ref()
    else {
        return;
    };
    unsafe {
        let weak = shader_cache::fnv1a64(std::slice::from_raw_parts(st.data as *const u8, 0x130));
        let base = dir
            .join(format!("{:016x}_s{}", weak, index))
            .display()
            .to_string();
        if std::path::Path::new(&format!("{}.code", base)).exists() {
            return;
        }
        let _ = std::fs::create_dir_all(dir);
        // Stages near the end of a mapping: a fixed 64 KiB window would read past it.
        let code = std::slice::from_raw_parts(st.data as *const u8, crate::cpu::crash_handler::readable_len(st.data as usize, 0x10000));
        let ctrl = std::slice::from_raw_parts(
            st.control as *const u8,
            control_section_len(st.control as usize),
        );
        if std::fs::write(format!("{}.code", base), code).is_ok()
            && std::fs::write(format!("{}.ctrl", base), ctrl).is_ok()
        {
            println!("[shader-cache] missing stage written to {}.code (rebuild the cache to translate it)", base);
        }
    }
}

/// Shared, not copied: looked up for every draw.
pub fn stages(program: usize) -> Option<std::sync::Arc<Vec<StageCode>>> {
    programs().lock().unwrap().get(&program).cloned()
}
