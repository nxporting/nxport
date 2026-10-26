// crates/build/src/vulkan_precompile.rs
// Offline shader and pipeline precompiler for Vulkan (docs/windows-port.md Step 4).
//
// Extracts shader stages from a game dump (RomFS), translates Maxwell SASS to SPIR-V using sass2spv,
// populates the Vulkan stage cache (stages.txt, code/, t/), compiles graphics and compute pipelines
// using the Vulkan driver into a vk::PipelineCache, and saves the binary cache (pipelines.vkcache).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nxport_pipelines::Entry;
use sass2spv::Target;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum VulkanPrecompilePhase {
    ExtractingStages,
    TranslatingShaders,
    CompilingPipelines,
}

impl VulkanPrecompilePhase {
    pub const ALL: [VulkanPrecompilePhase; 3] = [
        VulkanPrecompilePhase::ExtractingStages,
        VulkanPrecompilePhase::TranslatingShaders,
        VulkanPrecompilePhase::CompilingPipelines,
    ];

    pub const NUM_PHASES: usize = 3;

    #[inline]
    pub fn phase_index(self) -> usize {
        match self {
            Self::ExtractingStages => 0,
            Self::TranslatingShaders => 1,
            Self::CompilingPipelines => 2,
        }
    }

    #[inline]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExtractingStages => "Extracting shader stages from RomFS",
            Self::TranslatingShaders => "Translating shaders to SPIR-V",
            Self::CompilingPipelines => "Compiling Vulkan pipelines",
        }
    }
}

impl std::fmt::Display for VulkanPrecompilePhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

pub type ProgressCallback = Arc<dyn Fn(VulkanPrecompilePhase, usize, usize) + Send + Sync>;

#[derive(Clone)]
pub struct VulkanPrecompileOptions {
    /// The game dump directory (holding ExeFS/ and RomFS/).
    pub dump_dir: PathBuf,
    /// Explicit RomFS directory if not `<dump_dir>/RomFS` or `<dump_dir>/romfs`.
    pub romfs_dir: Option<PathBuf>,
    /// Optional stages directory if stages were already extracted (e.g. from an earlier extract or miss dump).
    pub stages_dir: Option<PathBuf>,
    /// Where the Vulkan shader cache is written. If None, writes to the game's default
    /// data directory (e.g. %APPDATA%\nxport\games\<title_id>\vulkan_stages).
    pub out_cache_dir: Option<PathBuf>,
    /// Explicit pipeline list file (pipelines.txt). If None, checks the cache dir, the game data dir,
    /// or fetches the shared pipeline list from GitHub.
    pub pipeline_list: Option<PathBuf>,
    /// Whether to download the shared pipeline list from GitHub if not found locally.
    pub download_shared: bool,
    /// Number of parallel worker threads for translation and pipeline compilation.
    pub jobs: Option<usize>,
    /// Progress reporting callback: (phase, done, total).
    pub on_progress: Option<ProgressCallback>,
}

#[derive(Debug, Clone, Default)]
pub struct VulkanPrecompileStats {
    pub stages_extracted: usize,
    pub stages_translated: usize,
    pub stages_failed: usize,
    pub pipelines_compiled: usize,
    pub pipelines_skipped: usize,
    pub cache_dir: PathBuf,
    pub vkcache_bytes: u64,
}

const SHARED_PIPELINES_URL: &str = "https://github.com/nxporting/pipelines/releases/latest/download";

fn infer_stage_slot(path: &Path) -> usize {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if let Some(pos) = stem.rfind("_s") {
        if let Ok(slot) = stem[pos + 2..].parse::<usize>() {
            return slot;
        }
    }
    // Default: 0 = vertex
    0
}

fn fetch_shared_pipelines(title_id: u64, dest: &Path) -> bool {
    let url = format!("{SHARED_PIPELINES_URL}/{title_id:016x}.txt");
    let tmp = dest.with_extension("download");
    let ok = std::process::Command::new("curl.exe")
        .args(["-fsSL", "--max-time", "30", "-o"])
        .arg(&tmp)
        .arg(&url)
        .status()
        .is_ok_and(|s| s.success());
    if ok {
        let _ = std::fs::rename(&tmp, dest);
        true
    } else {
        let _ = std::fs::remove_file(&tmp);
        false
    }
}

pub fn precompile(options: &VulkanPrecompileOptions) -> Result<VulkanPrecompileStats, String> {
    let mut stats = VulkanPrecompileStats::default();

    // 1. Identify the game and install settings
    let mut s = nxport::settings::Settings::default();
    s.files.game_dir = Some(options.dump_dir.clone());
    let _ = nxport::settings::try_install(s);

    let ident = nxport::profile::identify(&options.dump_dir);
    let title_id = ident.title_id.unwrap_or(0);
    if let Some(id) = nxport::nso::nso_build_id(&options.dump_dir.join("ExeFS/main").display().to_string()) {
        nxport::profile::set_main_build_id(&id);
    }

    // 2. Configure output cache directory
    if let Some(out_dir) = &options.out_cache_dir {
        nxport::vulkan::stage_cache::set_cache_dir(out_dir.clone());
    }
    let cache_dir = nxport::vulkan::stage_cache::dir()
        .cloned()
        .ok_or_else(|| "Failed to determine Vulkan stage cache directory".to_string())?;
    stats.cache_dir = cache_dir.clone();
    std::fs::create_dir_all(cache_dir.join("code")).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(cache_dir.join("t")).map_err(|e| e.to_string())?;

    let parallelism = std::thread::available_parallelism().map_or(4, |n| n.get());
    let jobs = options.jobs.unwrap_or(parallelism).max(1);

    // 3. Stage extraction
    let mut stage_files: Vec<(PathBuf, Option<PathBuf>, usize)> = Vec::new();
    let stages_src = options.stages_dir.as_deref();

    let has_existing_stages = stages_src.is_some_and(|d| {
        std::fs::read_dir(d).is_ok_and(|entries| {
            entries.flatten().any(|e| e.path().extension().is_some_and(|ext| ext == "code"))
        })
    });

    let extracted_dir = cache_dir.join("extracted_stages");
    if has_existing_stages {
        let dir = stages_src.unwrap();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for e in entries.flatten() {
                let p = e.path();
                if p.extension().is_some_and(|ext| ext == "code") {
                    let ctrl = p.with_extension("ctrl");
                    let slot = infer_stage_slot(&p);
                    stage_files.push((p, ctrl.exists().then_some(ctrl), slot));
                }
            }
        }
        stats.stages_extracted = stage_files.len();
    } else {
        // Locate RomFS
        let romfs = options.romfs_dir.clone().or_else(|| {
            let direct = options.dump_dir.join("RomFS");
            if direct.is_dir() {
                Some(direct)
            } else {
                let lower = options.dump_dir.join("romfs");
                lower.is_dir().then_some(lower)
            }
        });

        if let Some(romfs) = romfs {
            if let Some(cb) = &options.on_progress {
                cb(VulkanPrecompilePhase::ExtractingStages, 0, 1);
            }
            let dirs = crate::extract_shaders::default_dirs(&romfs);
            let dirs_str: Vec<&str> = dirs.iter().map(String::as_str).collect();
            let cb_extract = options.on_progress.as_ref().map(|cb| {
                let cb = cb.clone();
                move |_phase, done, total| cb(VulkanPrecompilePhase::ExtractingStages, done, total)
            });

            std::fs::create_dir_all(&extracted_dir).map_err(|e| e.to_string())?;
            crate::extract_shaders::extract_all(
                &romfs,
                &extracted_dir,
                &dirs_str,
                None,
                cb_extract.as_ref().map(|cb| cb as _),
            )
            .map_err(|e| format!("RomFS shader extraction failed: {e}"))?;

            if let Ok(entries) = std::fs::read_dir(&extracted_dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.extension().is_some_and(|ext| ext == "code") {
                        let ctrl = p.with_extension("ctrl");
                        let slot = infer_stage_slot(&p);
                        stage_files.push((p, ctrl.exists().then_some(ctrl), slot));
                    }
                }
            }
            stats.stages_extracted = stage_files.len();
        }
    }

    // 4. Initialize Vulkan device and pipeline cache
    let dev = Arc::new(nxport::vulkan::device::Dev::new(false)?);
    let vk_cache = unsafe { nxport::vulkan::stage_cache::open_pipeline_cache(&dev) };

    // 5. Phase 2: Translate stages to SPIR-V and register in stage_cache
    let total_stages = stage_files.len();
    if let Some(cb) = &options.on_progress {
        cb(VulkanPrecompilePhase::TranslatingShaders, 0, total_stages);
    }

    let stages_done = AtomicUsize::new(0);
    let stages_translated = AtomicUsize::new(0);
    let stages_failed = AtomicUsize::new(0);
    let chunk_size = (total_stages / jobs).max(1);

    std::thread::scope(|s| {
        for chunk in stage_files.chunks(chunk_size) {
            let dev = dev.clone();
            let on_progress = options.on_progress.clone();
            let stages_done = &stages_done;
            let stages_translated = &stages_translated;
            let stages_failed = &stages_failed;

            s.spawn(move || {
                for (code_path, ctrl_path, slot) in chunk {
                    let Ok(code) = std::fs::read(code_path) else {
                        stages_failed.fetch_add(1, Ordering::Relaxed);
                        continue;
                    };
                    let ctrl = ctrl_path.as_ref().and_then(|p| std::fs::read(p).ok());
                    let prep = nxport::vulkan::shader::prep(&code, ctrl.as_deref(), *slot);

                    let res = (|| -> Result<(), String> {
                        // Check if already registered
                        let full = nxport::vulkan::stage_cache::fnv1a64(&code[..prep.code_len.max(code.len())]);
                        if nxport::vulkan::stage_cache::by_name(full).is_some() {
                            return Ok(());
                        }

                        let p = sass2spv::translate_stage_for(
                            &code,
                            prep.hint,
                            prep.cb1,
                            prep.compute_fields,
                            &Target::Spirv,
                            None,
                            dev.shader_host.clone(),
                        )?;
                        if p.binary_code.is_empty() {
                            return Err("empty SPIR-V".into());
                        }

                        let named_len = nxport::vulkan::shader::named_len(&code, p.size);
                        let inputs = nxport::vulkan::shader::spirv_inputs(&p.binary_code);
                        let k = nxport::vulkan::stage_cache::remember(&code, named_len, prep, inputs);
                        let m = p.manifest();

                        let key = nxport::vulkan::shader::translation_key(
                            &dev, &prep, k.full, &None, 0, 0, None, false,
                        );
                        nxport::vulkan::stage_cache::store(key, &p.binary_code, &m);



                        Ok(())
                    })();

                    match res {
                        Ok(()) => {
                            stages_translated.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            stages_failed.fetch_add(1, Ordering::Relaxed);
                        }
                    }

                    let done = stages_done.fetch_add(1, Ordering::Relaxed) + 1;
                    if let Some(cb) = &on_progress {
                        if done % 50 == 0 || done == total_stages {
                            cb(VulkanPrecompilePhase::TranslatingShaders, done, total_stages);
                        }
                    }
                }
            });
        }
    });

    stats.stages_translated = stages_translated.load(Ordering::Relaxed);
    stats.stages_failed = stages_failed.load(Ordering::Relaxed);

    // 6. Phase 3: Compile graphics pipelines from pipeline list
    let mut pipeline_list_text = None;

    if let Some(path) = &options.pipeline_list {
        pipeline_list_text = std::fs::read_to_string(path).ok();
    }
    if pipeline_list_text.is_none() {
        let p = cache_dir.join("pipelines.txt");
        if p.exists() {
            pipeline_list_text = std::fs::read_to_string(&p).ok();
        }
    }
    if pipeline_list_text.is_none() {
        if let Some(game_data) = nxport::profile::data_dir() {
            let p = game_data.join("pipelines.txt");
            if p.exists() {
                pipeline_list_text = std::fs::read_to_string(&p).ok();
            } else {
                let shared = game_data.join("pipelines_shared.txt");
                if shared.exists() {
                    pipeline_list_text = std::fs::read_to_string(&shared).ok();
                }
            }
        }
    }
    if pipeline_list_text.is_none() && options.download_shared && title_id != 0 {
        let shared_dest = cache_dir.join("pipelines_shared.txt");
        if fetch_shared_pipelines(title_id, &shared_dest) {
            pipeline_list_text = std::fs::read_to_string(&shared_dest).ok();
        }
    }

    if let Some(text) = pipeline_list_text {
        let mut entries: Vec<Entry> = text
            .lines()
            .filter_map(|l| {
                let l = l.trim();
                if l.is_empty() || l.starts_with('#') {
                    None
                } else {
                    Entry::parse(l)
                }
            })
            .map(nxport::vulkan::listed::ensure_nvn)
            .collect();

        // Deduplicate entries
        let mut seen = HashSet::new();
        entries.retain(|e| seen.insert(e.clone()));

        let total_pipelines = entries.len();
        if let Some(cb) = &options.on_progress {
            cb(VulkanPrecompilePhase::CompilingPipelines, 0, total_pipelines);
        }

        let pipe_queue = Arc::new(Mutex::new(entries));
        let pipes_compiled = Arc::new(AtomicUsize::new(0));
        let pipes_done = Arc::new(AtomicUsize::new(0));
        let cache_mutex = Arc::new(Mutex::new(vk_cache));

        let compile_threads = jobs.clamp(1, 8);
        std::thread::scope(|s| {
            for _ in 0..compile_threads {
                let dev = dev.clone();
                let queue = pipe_queue.clone();
                let compiled = pipes_compiled.clone();
                let done_count = pipes_done.clone();
                let on_progress = options.on_progress.clone();
                let cache_mutex = cache_mutex.clone();

                s.spawn(move || unsafe {
                    loop {
                        let e = {
                            let mut q = queue.lock().unwrap();
                            q.pop()
                        };
                        let Some(e) = e else { break };

                        if let Some((v, f)) = nxport::vulkan::listed::stages(&dev, &e) {
                            if let Some(d) = nxport::vulkan::listed::desc(&dev, &e, &v, f.as_ref()) {
                                if let Some(lay) = nxport::vulkan::pipelines::layout(&dev, &v, None, f.as_ref()) {
                                    let ok = {
                                        let _guard = cache_mutex.lock().unwrap();
                                        nxport::vulkan::pipelines::create(
                                            &dev,
                                            &d,
                                            &v,
                                            None,
                                            f.as_ref(),
                                            lay.1,
                                            vk_cache,
                                        )
                                        .is_ok()
                                    };
                                    if ok {
                                        compiled.fetch_add(1, Ordering::Relaxed);
                                    }
                                    dev.device.destroy_pipeline_layout(lay.1, None);
                                    dev.device.destroy_descriptor_set_layout(lay.0, None);
                                }
                            }
                            dev.device.destroy_shader_module(v.module, None);
                            dev.device.destroy_descriptor_set_layout(v.set_layout, None);
                            if let Some(f) = f {
                                dev.device.destroy_shader_module(f.module, None);
                                dev.device.destroy_descriptor_set_layout(f.set_layout, None);
                            }
                        }

                        let done = done_count.fetch_add(1, Ordering::Relaxed) + 1;
                        if let Some(cb) = &on_progress {
                            if done % 20 == 0 || done == total_pipelines {
                                cb(VulkanPrecompilePhase::CompilingPipelines, done, total_pipelines);
                            }
                        }
                    }
                });
            }
        });

        stats.pipelines_compiled = pipes_compiled.load(Ordering::Relaxed);
        stats.pipelines_skipped = total_pipelines.saturating_sub(stats.pipelines_compiled);
    }

    // 7. Save pipeline cache
    unsafe {
        nxport::vulkan::stage_cache::save_pipeline_cache(&dev, vk_cache);
        dev.device.destroy_pipeline_cache(vk_cache, None);
    }

    let cache_file = cache_dir.join("pipelines.vkcache");
    if let Ok(meta) = std::fs::metadata(&cache_file) {
        stats.vkcache_bytes = meta.len();
        // If an explicit out_cache_dir was provided, also copy pipelines.vkcache to out_cache_dir root
        if let Some(out_dir) = &options.out_cache_dir {
            let root_cache = out_dir.join("pipelines.vkcache");
            if root_cache != cache_file {
                let _ = std::fs::copy(&cache_file, root_cache);
            }
        }
    }

    Ok(stats)
}
