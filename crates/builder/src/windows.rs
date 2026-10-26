// The Windows target's build steps: the C compiler (zig cc, downloaded once), the shared pipeline list, the Vulkan
// shader cache from the game's shader stages (when shaders are translated ahead of time), the app folder (nxport.exe,
// settings.toml, mods, game files, caches), then the game's code lifted to C with the app's mods and settings and
// compiled into the app's guest.dll.
//
// The lift and the shader cache run in a worker process (this program again, with --worker): both set up engine state
// that exists once per process (the installed settings, the guest's memory at its fixed address, the shader cache's
// directory), which a second build from the same window could not set up again.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};

use nxport::settings::SettingsPatch;

use crate::app::{extract_stages_step, pipeline_list, Job, Output, Request, Step};

/// The Vulkan backend's cache can be built offline (nxport_build::vulkan_precompile).
pub const PRECOMPILES_SHADERS: bool = true;

/// Sets the running step's sub-step to `done` of `total`, which is that share of the step.
fn progress(job: &Mutex<Job>, sub: String, done: usize, total: usize) {
    crate::app::progress(job, sub, done, total, if total > 0 { done as f32 / total as f32 } else { 0.0 });
}

pub fn steps(r: Request, steps: &mut Vec<Step>) -> Output {
    let Request { name, title_id, work, dump, mods, out_dir, edits, prebuild, bundle_game, job, .. } = r;

    let app = out_dir.join(name.replace([':', '/'], " -"));
    let toolchains = crate::app::data_dir().join("toolchains");
    let (stages, cache, vulkan_cache) = (work.join("stages"), work.join("shader_cache"), work.join("vulkan_cache"));
    let c_dir = work.join("lifted");
    // The app's own mods (pack leaves out those for other programs): the code is lifted with what the app runs.
    let app_mods: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));

    {
        let (toolchains, job) = (toolchains.clone(), job.clone());
        steps.push((
            "Getting the C compiler".into(),
            Box::new(move || {
                let cc = nxport_build::zig::cc(&toolchains, &move |done, total| {
                    let mb = |b: u64| (b >> 20) as usize;
                    progress(&job, format!("Downloading zig {}", nxport_build::zig::VERSION), mb(done), mb(total));
                })?;
                println!("  {}", cc.join(" "));
                Ok(())
            }),
        ));
    }

    {
        let cache = cache.clone();
        steps.push((
            "Getting the shared pipeline list".into(),
            Box::new(move || {
                std::fs::create_dir_all(&cache).map_err(|e| format!("{}: {e}", cache.display()))?;
                pipeline_list(title_id, &cache, false)
            }),
        ));
    }

    // Without the cache the app translates every shader as the game loads it.
    if prebuild {
        steps.push(extract_stages_step(&job, &dump, &stages));
        let list = cache.join("pipelines.txt");
        let w = Work {
            kind: WorkKind::Shaders,
            dump: dump.clone(),
            into: vulkan_cache.clone(),
            mods: None,
            stages: Some(stages.clone()),
            pipeline_list: Some(list),
            edits: SettingsPatch::default(),
        };
        let (job, file) = (job.clone(), work.join("shaders.toml"));
        steps.push(("Building the shader cache".into(), Box::new(move || run_worker(&job, &file, &w))));
    }

    {
        let (dump, app, app_mods) = (dump.clone(), app.clone(), app_mods.clone());
        let list = cache.join("pipelines.txt");
        let edits = edits.clone();
        steps.push((
            "Packaging the app".into(),
            Box::new(move || {
                *app_mods.lock().unwrap() = nxport_build::winapp::pack(&nxport_build::winapp::Options {
                    dump: &dump,
                    mods: mods.as_deref(),
                    app: &app,
                    edits,
                    bundle_game,
                    pipeline_list: list.is_file().then_some(list.as_path()),
                    shader_cache: prebuild.then_some(vulkan_cache.as_path()),
                })?;
                Ok(())
            }),
        ));
    }

    {
        let (c_dir, job, file) = (c_dir.clone(), job.clone(), work.join("lift.toml"));
        steps.push((
            "Lifting the game's code".into(),
            Box::new(move || {
                let w = Work {
                    kind: WorkKind::Lift,
                    dump,
                    into: c_dir,
                    mods: app_mods.lock().unwrap().clone(),
                    stages: None,
                    pipeline_list: None,
                    edits,
                };
                run_worker(&job, &file, &w)
            }),
        ));
    }

    let out: Output = Arc::new(Mutex::new(None));
    {
        let out = out.clone();
        steps.push((
            "Compiling the game's code".into(),
            Box::new(move || {
                let cc = nxport_build::zig::cc(&toolchains, &|_, _| {})?;
                let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
                // Also copies modules.txt beside the library.
                nxport_build::lift::compile(&c_dir, &app.join("guest.dll"), &cc, cpus.saturating_sub(2).max(2), &|done, total| {
                    progress(&job, "C files".into(), done, total)
                })
                .map_err(|e| e.to_string())?;
                // The linker's import library (the engine loads the library at run time; the .pdb stays, for crashes).
                for e in std::fs::read_dir(&app).into_iter().flatten().flatten() {
                    if e.path().extension().is_some_and(|x| x == "lib") {
                        let _ = std::fs::remove_file(e.path());
                    }
                }
                *out.lock().unwrap() = Some(app);
                Ok(())
            }),
        ));
    }
    out
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum WorkKind {
    /// The game's code lifted to C files in `into`, with `mods` and `edits` (nxport_build::lift::lift_game).
    Lift,
    /// The Vulkan shader cache in `into` from the shader stages in `stages` and the pipelines `pipeline_list` names
    /// (nxport_build::vulkan_precompile).
    Shaders,
}

/// What a worker process does, in the file its command line names.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Work {
    kind: WorkKind,
    dump: PathBuf,
    into: PathBuf,
    mods: Option<PathBuf>,
    stages: Option<PathBuf>,
    pipeline_list: Option<PathBuf>,
    /// The settings above the game's profile (last: a table).
    #[serde(default)]
    edits: SettingsPatch,
}

/// The line prefix a worker reports progress with: `@ <fraction of the step> <done> <total> <what>`.
const PROGRESS: &str = "@ ";

/// Runs `w` in a worker process (writing it to `file` for it), showing its progress in `job` and the rest of what it
/// prints in the log.
fn run_worker(job: &Arc<Mutex<Job>>, file: &Path, w: &Work) -> Result<(), String> {
    let text = toml::to_string(w).map_err(|e| e.to_string())?;
    std::fs::write(file, text).map_err(|e| format!("{}: {e}", file.display()))?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = nxport_build::lift::command(exe)
        .arg("--worker")
        .arg(file)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("starting the worker: {e}"))?;
    let stderr = child.stderr.take().unwrap();
    let last_error = Arc::new(Mutex::new(None::<String>));
    let errors = {
        let last_error = last_error.clone();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                println!("{line}");
                *last_error.lock().unwrap() = Some(line);
            }
        })
    };
    for line in BufReader::new(child.stdout.take().unwrap()).lines().map_while(Result::ok) {
        match line.strip_prefix(PROGRESS).map(|p| p.splitn(4, ' ').collect::<Vec<_>>()).as_deref() {
            Some([frac, done, total, what]) => {
                let mut j = job.lock().unwrap();
                j.sub_step = Some(what.to_string());
                j.sub_done = done.parse().unwrap_or(0);
                j.sub_total = total.parse().unwrap_or(0);
                j.sub_frac = j.sub_frac.max(frac.parse().unwrap_or(0.0));
            }
            _ => println!("{line}"),
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    let _ = errors.join();
    if status.success() {
        return Ok(());
    }
    // main's error, printed as `Error: "<message>"`.
    let last = last_error.lock().unwrap().take().unwrap_or_default();
    let message = last.strip_prefix("Error: ").map_or(last.as_str(), |m| m.trim_matches('"'));
    Err(if message.is_empty() { format!("the worker failed ({status})") } else { message.to_string() })
}

fn report(frac: f32, done: usize, total: usize, what: &str) {
    println!("{PROGRESS}{frac} {done} {total} {what}");
}

/// A worker process: does the work described in `file` (run_worker's).
pub fn worker(file: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let w: Work = toml::from_str(&text).map_err(|e| format!("{}: {e}", file.display()))?;
    match w.kind {
        WorkKind::Lift => {
            let s = nxport_build::lift::game_settings(&w.dump, w.edits, w.mods.as_deref());
            let lifted = nxport_build::lift::lift_game(s, &w.into, &|i, n, name| {
                report(i as f32 / n.max(1) as f32, i, n, &format!("Lifting {name}"))
            })
            .map_err(|e| e.to_string())?;
            for (name, r) in &lifted {
                nxport_build::lift::report(name, r);
            }
        }
        WorkKind::Shaders => {
            use nxport_build::vulkan_precompile::{self as vp, VulkanPrecompilePhase as Phase};
            std::fs::create_dir_all(&w.into).map_err(|e| format!("{}: {e}", w.into.display()))?;
            nxport::vulkan::stage_cache::set_cache_dir(w.into.clone());
            let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
            let stats = vp::precompile(&vp::VulkanPrecompileOptions {
                dump_dir: w.dump,
                romfs_dir: None,
                stages_dir: w.stages,
                out_cache_dir: None,
                pipeline_list: w.pipeline_list.filter(|l| l.is_file()),
                download_shared: false,
                jobs: Some(cpus.saturating_sub(2).max(2)),
                on_progress: Some(Arc::new(|phase: Phase, done, total| {
                    let f = if total > 0 { (done as f32 / total as f32).min(1.0) } else { 0.0 };
                    report((phase.phase_index() as f32 + f) / Phase::NUM_PHASES as f32, done, total, phase.as_str());
                })),
            })?;
            println!(
                "  {} stages translated ({} failed), {} of {} pipelines compiled, {:.1} MB pipeline cache",
                stats.stages_translated,
                stats.stages_failed,
                stats.pipelines_compiled,
                stats.pipelines_compiled + stats.pipelines_skipped,
                stats.vkcache_bytes as f64 / 1e6
            );
        }
    }
    Ok(())
}
