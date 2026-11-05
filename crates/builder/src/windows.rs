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

use crate::app::{extract_stages_step, pipeline_list, Job, Output, Request, Step, Target};

/// The Vulkan backend's cache can be built offline (nxport_build::vulkan_precompile).
pub const PRECOMPILES_SHADERS: bool = true;

/// Sets the running step's sub-step to `done` of `total`, which is that share of the step.
fn progress(job: &Mutex<Job>, sub: String, done: usize, total: usize) {
    crate::app::progress(job, sub, done, total, if total > 0 { done as f32 / total as f32 } else { 0.0 });
}

pub fn steps(r: Request, steps: &mut Vec<Step>) -> Output {
    if r.target == Target::Ipa {
        return ipa_steps(r, steps);
    }
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

    // Without the cache the app translates every shader as the game loads it. Each step is skipped when what it was
    // last built from is unchanged: the stages by the game's version (main's build id), the cache by that, the
    // translator and the pipeline list.
    if prebuild {
        let main = nxport::profile::main_build_id_of(&dump).unwrap_or_default();
        let stamp = Stamp { file: work.join("stages.from"), inputs: format!("main {main}\n"), output: stages.clone() };
        let extract = extract_stages_step(&job, &dump, &stages);
        steps.push((extract.0, Box::new(move || stamp.run("The shader stages are those of the last build", extract.1))));

        let list = cache.join("pipelines.txt");
        let w = Work {
            kind: WorkKind::Shaders,
            dump: dump.clone(),
            into: vulkan_cache.clone(),
            mods: None,
            stages: Some(stages.clone()),
            pipeline_list: Some(list.clone()),
            patched_image: None,
            base: None,
            bundle_game: false,
            entitlements: false,
            bundle_id: None,
            edits: SettingsPatch::default(),
        };
        let (job, file, from, built) = (job.clone(), work.join("shaders.toml"), work.join("vulkan_cache.from"), vulkan_cache.clone());
        steps.push((
            "Building the shader cache".into(),
            Box::new(move || {
                // The list is written by an earlier step.
                use nxport::vulkan::stage_cache::{fnv1a64, translator};
                let pipelines = std::fs::read(&list).map_or(0, |b| fnv1a64(&b));
                let inputs = format!("translator {}\nmain {main}\npipelines {pipelines:016x}\n", translator());
                let stamp = Stamp { file: from, inputs, output: built.join(translator()) };
                stamp.run("The shader cache is that of the last build", move || run_worker(&job, &file, &w))
            }),
        ));
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

    // The game's code without mods, kept between builds: lifting and compiling it takes minutes. Code mods change it as
    // it loads (IPS, pchtxt): that, and the modules of plugin mods, go into an overlay library beside it
    // (nxport_build::lift::lift_overlay). What plugin mods patch while the game runs is handled while it runs
    // (engine live_patch.rs, which the overlay's presence turns on): a run ahead of time could not know a patch's moment.
    let base = work.join("base");
    {
        let (dump, c_dir, job, edits, toolchains, app) = (dump.clone(), c_dir.clone(), job.clone(), edits.clone(), toolchains.clone(), app.clone());
        let (base, lift_file, base_file, from) = (base.clone(), work.join("lift.toml"), work.join("base_code.toml"), work.join("base.from"));
        let profiles = work.join(PROFILES_DIR);
        steps.push((
            "Lifting and compiling the game's code".into(),
            Box::new(move || {
                let engine = std::env::current_exe().and_then(std::fs::read).map_err(|e| e.to_string())?;
                let main = nxport::profile::main_build_id_of(&dump).unwrap_or_default();
                let inputs = format!(
                    "engine {:016x}\nmain {main}\n{}{}",
                    nxport::vulkan::stage_cache::fnv1a64(&engine),
                    profiles_listed(&profiles, &main),
                    toml::to_string(&edits).unwrap_or_default()
                );
                let stamp = Stamp { file: from, inputs, output: base.clone() };
                let built = base.clone();
                stamp.run("The game's code is that of the last build", || {
                    let _ = std::fs::remove_dir_all(&built);
                    std::fs::create_dir_all(&built).map_err(|e| format!("{}: {e}", built.display()))?;
                    let work = |kind, into: PathBuf| Work {
                        kind,
                        dump: dump.clone(),
                        into,
                        mods: None,
                        stages: None,
                        pipeline_list: None,
                        patched_image: None,
                        base: None,
                        bundle_game: false,
                        entitlements: false,
                        bundle_id: None,
                        edits: edits.clone(),
                    };
                    run_worker(&job, &lift_file, &work(WorkKind::Lift, c_dir.clone()))?;
                    let profile = profile_for(&profiles, &main, &c_dir)?;
                    compile_guest(&toolchains, &c_dir, &built.join("guest.dll"), profile.as_deref(), &job)?;
                    run_worker(&job, &base_file, &work(WorkKind::BaseCode, built.join(nxport_build::lift::BASE_CODE_FILE)))?;
                    // Gigabytes of C and objects, needed again only with a new base.
                    let _ = std::fs::remove_dir_all(&c_dir);
                    Ok(())
                })?;
                for name in ["guest.dll", "guest.pdb", nxport_build::lift::MODULES_FILE] {
                    let (from, to) = (base.join(name), app.join(name));
                    let _ = std::fs::remove_file(&to);
                    if from.is_file() && std::fs::hard_link(&from, &to).is_err() {
                        std::fs::copy(&from, &to).map_err(|e| format!("{}: {e}", to.display()))?;
                    }
                }
                Ok(())
            }),
        ));
    }
    let overlay = app.join("overlay");
    let lift_overlay = |file: &str| {
        let (dump, into, job, app_mods, edits, file, base, toolchains, overlay) = (
            dump.clone(),
            work.join("lifted_overlay"),
            job.clone(),
            app_mods.clone(),
            edits.clone(),
            work.join(file),
            base.join("guest.dll"),
            toolchains.clone(),
            overlay.clone(),
        );
        Box::new(move || {
            let mods = app_mods.lock().unwrap().clone();
            let _ = std::fs::remove_dir_all(&overlay);
            if !has_code_mods(mods.as_deref(), title_id) {
                return Ok(());
            }
            let _ = std::fs::remove_dir_all(&into);
            let w = Work {
                kind: WorkKind::Overlay,
                dump,
                into: into.clone(),
                mods,
                stages: None,
                pipeline_list: None,
                patched_image: None,
                base: Some(base),
                bundle_game: false,
                entitlements: false,
                bundle_id: None,
                edits,
            };
            run_worker(&job, &file, &w)?;
            std::fs::create_dir_all(&overlay).map_err(|e| format!("{}: {e}", overlay.display()))?;
            compile_guest(&toolchains, &into, &overlay.join("guest_overlay.dll"), None, &job)
        }) as Box<dyn FnOnce() -> Result<(), String> + Send>
    };
    steps.push(("Lifting what the mods change".into(), lift_overlay("overlay.toml")));
    let out: Output = Arc::new(Mutex::new(None));
    {
        let (out, app) = (out.clone(), app.clone());
        steps.push((
            "Finishing the app".into(),
            Box::new(move || {
                *out.lock().unwrap() = Some(app);
                Ok(())
            }),
        ));
    }
    out
}

/// The iOS target: the engine for iOS from this checkout, then the app in a worker (the guest image needs a process of
/// its own). Apple's Metal compiler being Mac-only, the app has no shader cache: it translates as the game loads.
fn ipa_steps(r: Request, steps: &mut Vec<Step>) -> Output {
    let Request { name, work, dump, mods, out_dir, edits, bundle_game, entitlements, bundle_id, job, .. } = r;
    let toolchains = crate::app::data_dir().join("toolchains");
    steps.push((
        "Building the engine for iOS".into(),
        Box::new(move || {
            if nxport_build::ios_engine::source_root().is_some() {
                let engine = nxport_build::ios_engine::build(&toolchains)?;
                println!("  {}", engine.display());
            }
            Ok(())
        }),
    ));
    let out: Output = Arc::new(Mutex::new(None));
    {
        let (out, file) = (out.clone(), work.join("ipa.toml"));
        steps.push((
            "Building the iOS app".into(),
            Box::new(move || {
                std::fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
                std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
                let w = Work {
                    kind: WorkKind::Ipa,
                    dump,
                    into: out_dir.clone(),
                    mods,
                    stages: None,
                    pipeline_list: None,
                    patched_image: None,
                    base: None,
                    bundle_game,
                    entitlements,
                    bundle_id,
                    edits,
                };
                run_worker(&job, &file, &w)?;
                *out.lock().unwrap() = Some(nxport_build::ipa::path_for(&out_dir, &name));
                Ok(())
            }),
        ));
    }
    out
}

/// The folder of a game's build directory that holds its profiles, each <main's build id>-<lift::lifted_hash>.profdata.
const PROFILES_DIR: &str = "profiles";

/// This version of the game's profiles, by name and size: one added or taken away builds the code again.
fn profiles_listed(dir: &Path, main: &str) -> String {
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| !main.is_empty() && e.file_name().to_string_lossy().starts_with(main))
        .map(|e| format!("profile {} {}\n", e.file_name().to_string_lossy(), e.metadata().map_or(0, |m| m.len())))
        .collect();
    found.sort();
    found.concat()
}

/// Where profiles are published, gzipped, as `<main's build id>.profdata.gz`.
const PROFILES_URL: &str = "https://github.com/nxporting/profiles/releases/latest/download";
/// Beside PROFILES_DIR: profiles downloaded. Not in profiles_listed, so one published later does not rebuild the code by itself.
const FETCHED_PROFILES_DIR: &str = "profiles_fetched";

/// The profile for this version of the game: one put in `dir`, made for exactly this lifted C or for the version, or one
/// published. The compiler matches functions by their control flow and builds those a lifter change has altered without
/// it, so an older profile only gains less. Without one the code is built without: slower, and otherwise the same.
fn profile_for(dir: &Path, main: &str, c_dir: &Path) -> Result<Option<PathBuf>, String> {
    let exact = dir.join(format!("{main}-{}.profdata", nxport_build::lift::lifted_hash(c_dir)?));
    if exact.is_file() {
        return Ok(Some(exact));
    }
    let name = format!("{main}.profdata");
    let profile = dir.join(&name);
    if profile.is_file() {
        return Ok(Some(profile));
    }
    let fetched = dir.with_file_name(FETCHED_PROFILES_DIR).join(&name);
    if fetched.is_file() {
        return Ok(Some(fetched));
    }
    match nxport_build::lift::fetch_gzipped(&format!("{PROFILES_URL}/{name}.gz"), &fetched) {
        Ok(()) => Ok(Some(fetched)),
        Err(e) => {
            println!("  No profile for this code: building without one ({} is not there; {e})", profile.display());
            Ok(None)
        }
    }
}

/// Compiles the lifted C in `c_dir` into the guest library `library` (and its modules.txt), optimised by `profile`. A
/// profile the compiler cannot read costs the optimisation, not the build.
fn compile_guest(toolchains: &Path, c_dir: &Path, library: &Path, profile: Option<&Path>, job: &Arc<Mutex<Job>>) -> Result<(), String> {
    let cc = nxport_build::zig::cc(toolchains, &|_, _| {})?;
    let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
    let jobs = cpus.saturating_sub(2).max(2);
    let on_file = |done, total| progress(job, "C files".into(), done, total);
    // Also copies modules.txt beside the library.
    if let Err(e) = nxport_build::lift::compile(c_dir, library, &cc, jobs, profile, &on_file) {
        if profile.is_none() {
            return Err(e.to_string());
        }
        println!("  The profile did not work, building without it: {e}");
        // A failed compile records no flags, and what it did compile would be taken as it is.
        let _ = std::fs::remove_dir_all(c_dir.join("obj"));
        nxport_build::lift::compile(c_dir, library, &cc, jobs, None, &on_file).map_err(|e| e.to_string())?;
    }
    // The linker's import library (the engine loads the library at run time; the .pdb stays, for crashes).
    let dir = library.parent().unwrap_or(Path::new("."));
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().extension().is_some_and(|x| x == "lib") {
            let _ = std::fs::remove_file(e.path());
        }
    }
    Ok(())
}

/// Whether any mod in `mods` has code: an exefs folder, an exefs.nsp or a patch set. Mods of files only need no lifting at all.
fn has_code_mods(mods: Option<&Path>, title: u64) -> bool {
    let Some(mods) = mods else { return false };
    let code = |d: &Path| std::fs::read_dir(d).is_ok_and(|mut rd| rd.next().is_some());
    nxport::mods::discover_mods(mods, Some(title)).iter().any(|(_, d)| code(&nxport::mods::mod_subdir(d, "exefs")) || nxport::mods::has_exefs_nsp(d))
        || !nxport::mods::patch_sets(mods, 2).is_empty()
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum WorkKind {
    /// The game's code lifted to C files in `into`, with `mods` and `edits` (nxport_build::lift::lift_game).
    Lift,
    /// The code the lift holds, as the file `into`, for overlays (nxport_build::lift::write_base_code).
    BaseCode,
    /// What `mods` change against `base`, lifted to C files in `into` (nxport_build::lift::lift_overlay).
    Overlay,
    /// The iOS app, in the folder `into` (nxport_build::ipa).
    Ipa,
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
    /// Lift: the image a first lift's run with plugin mods dumped (their patches; nxport_build::lift::lift_game).
    #[serde(default)]
    patched_image: Option<PathBuf>,
    /// Overlay: the guest library of the game without mods.
    #[serde(default)]
    base: Option<PathBuf>,
    /// Ipa: as nxport_build::ipa::Options.
    #[serde(default)]
    bundle_game: bool,
    #[serde(default)]
    entitlements: bool,
    #[serde(default)]
    bundle_id: Option<String>,
    /// The settings above the game's profile (last: a table).
    #[serde(default)]
    edits: SettingsPatch,
}

/// What a step's `output` was last built from, kept in `file`: a later build from the same `inputs` skips the step.
struct Stamp {
    file: PathBuf,
    inputs: String,
    output: PathBuf,
}

impl Stamp {
    fn run(self, kept: &str, step: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
        if self.output.is_dir() && std::fs::read_to_string(&self.file).is_ok_and(|s| s == self.inputs) {
            println!("  {kept} ({})", self.file.display());
            return Ok(());
        }
        // Stale until the step finishes: a step that fails or is stopped leaves no stamp.
        let _ = std::fs::remove_file(&self.file);
        step()?;
        std::fs::write(&self.file, &self.inputs).map_err(|e| format!("{}: {e}", self.file.display()))
    }
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
            let lifted = nxport_build::lift::lift_game(s, w.patched_image.as_deref(), &w.into, &|i, n, name| {
                report(i as f32 / n.max(1) as f32, i, n, &format!("Lifting {name}"))
            })
            .map_err(|e| e.to_string())?;
            for (name, r) in &lifted {
                nxport_build::lift::report(name, r);
            }
        }
        WorkKind::BaseCode => {
            let s = nxport_build::lift::game_settings(&w.dump, w.edits, None);
            nxport_build::lift::write_base_code(s, &w.into).map_err(|e| e.to_string())?;
        }
        WorkKind::Overlay => {
            let s = nxport_build::lift::game_settings(&w.dump, w.edits, w.mods.as_deref());
            let base = w.base.ok_or("no base library")?;
            let lifted = nxport_build::lift::lift_overlay(s, w.patched_image.as_deref(), &base, &w.into).map_err(|e| e.to_string())?;
            for (name, r) in &lifted {
                nxport_build::lift::report(name, r);
            }
        }
        WorkKind::Ipa => {
            let ipa = nxport_build::ipa::build(
                &nxport_build::ipa::Options {
                    dump: &w.dump,
                    mods: w.mods.as_deref(),
                    cache: None,
                    out_dir: &w.into,
                    edits: w.edits,
                    bundle_game: w.bundle_game,
                    entitlements: w.entitlements,
                    bundle_id: w.bundle_id,
                },
                &|step| println!("[+] {step}"),
            )?;
            println!("  {}", ipa.display());
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
