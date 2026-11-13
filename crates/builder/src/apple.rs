// The macOS app's build steps: one list on every host. Only how some steps are done depends on the host:
// - the engine for macOS: a Mac builder carries it (build.rs builds it beside the builder); on Windows it is built from
//   this checkout first (nxport_build::apple_engine, as for iOS).
// - the shaders: on a Mac, the Metal shader cache from the game's shader stages (Apple's Metal compiler), the shared
//   pipeline list and its precompiled pipelines; on Windows, without that compiler, the stages the list uses translated
//   and packed as Metal source, which the app compiles the first time the game uses each (as the iOS app's,
//   windows::metal_source_steps).
// - the app (nxport_build::pack): on a Mac a folder, signed by the linker; on Windows signed with the Rust ad-hoc signer
//   (the engine with it: Apple silicon runs no unsigned code) and zipped with its file modes, so the executable reaches
//   the Mac executable (pack::zip_app).
// The iOS app's steps are in ios.rs; on a Mac they share the shader cache steps here.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nxport::settings;

use crate::app::{Output, Request, Step};

pub fn steps(r: Request, steps: &mut Vec<Step>) -> Output {
    engine(&r, steps);
    // Without the cache the app translates every shader as the game loads it.
    let cache = if r.prebuild { Some(shaders(&r, steps)) } else { None };
    app(r, cache, steps)
}

#[cfg(windows)]
fn engine(r: &Request, steps: &mut Vec<Step>) {
    engine_from_source(nxport_build::apple_engine::Os::MacOs, r, steps);
}

#[cfg(target_os = "macos")]
fn engine(_: &Request, _: &mut Vec<Step>) {}

/// Windows: the engine for `os` (the macOS app's, or the iOS app's), built from this checkout first (cargo does nothing
/// when it is up to date).
#[cfg(windows)]
pub(crate) fn engine_from_source(os: nxport_build::apple_engine::Os, r: &Request, steps: &mut Vec<Step>) {
    let (toolchains, job) = (crate::app::data_dir().join("toolchains"), r.job.clone());
    steps.push((
        format!("Building the engine for {}", os.name()),
        Box::new(move || {
            if nxport_build::apple_engine::source_root().is_some() {
                // No count: cargo does not say how many crates it will compile.
                let compiling = |name: &str| crate::app::progress(&job, format!("Compiling {name}"), 0, 0, 0.0);
                let engine = nxport_build::apple_engine::build(os, &toolchains, &compiling)?;
                println!("  {}", engine.display());
            }
            Ok(())
        }),
    ));
}

/// Mac: the Metal shader cache, and its listed pipelines precompiled.
#[cfg(target_os = "macos")]
fn shaders(r: &Request, steps: &mut Vec<Step>) -> PathBuf {
    let cache = metal_cache_steps(r, steps);
    let archive_cache = cache.clone();
    steps.push(("Precompiling pipelines".into(), Box::new(move || nxport_build::pipeline_archive::build_mac(&archive_cache))));
    cache
}

/// Windows: the stages the shared pipeline list uses, translated and packed as Metal source.
#[cfg(windows)]
fn shaders(r: &Request, steps: &mut Vec<Step>) -> PathBuf {
    crate::windows::metal_source_steps(r, steps)
}

/// The app itself: what the host hands over (the .app on a Mac, its zip on Windows).
fn app(r: Request, cache: Option<PathBuf>, steps: &mut Vec<Step>) -> Output {
    let Request { name, work, dump, mods, out_dir, edits, .. } = r;
    let app = out_dir.join(format!("{}.app", name.replace([':', '/'], " -")));
    let settings_file = work.join("settings.toml");
    let out: Output = Arc::new(Mutex::new(Some(handed_over(&app))));
    let app2 = app.clone();
    steps.push((
        "Packaging the app".into(),
        Box::new(move || {
            let mut edits = edits.clone();
            if mods.is_none() {
                edits.files.no_mods = Some(true);
            }
            if settings_file.exists() || settings_file.is_symlink() {
                let _ = nxport_build::pack::ensure_writable(&settings_file);
                let _ = std::fs::remove_file(&settings_file);
            }
            settings::save_preset(&settings_file, &edits)?;
            if let Some(parent) = app2.parent() {
                let _ = nxport_build::pack::ensure_writable(parent);
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            nxport_build::pack::pack_app(&nxport_build::pack::PackOptions {
                dump_dir: &dump,
                mods_dir: mods.as_deref(),
                shader_cache_dir: cache.as_deref(),
                out_app: Some(&app2),
                settings: Some(&settings_file),
                symlink_assets: false,
            })
            .map_err(|e| e.to_string())
        }),
    ));
    finish(&app, steps);
    out
}

/// Mac: the .app as it is.
#[cfg(target_os = "macos")]
fn handed_over(app: &std::path::Path) -> PathBuf {
    app.to_path_buf()
}

#[cfg(target_os = "macos")]
fn finish(_: &std::path::Path, _: &mut Vec<Step>) {}

/// Windows: <app>.zip (pack::zip_app).
#[cfg(windows)]
fn handed_over(app: &std::path::Path) -> PathBuf {
    let mut zip = app.as_os_str().to_owned();
    zip.push(".zip");
    PathBuf::from(zip)
}

/// Windows: the app signed (its engine with it), zipped, and the folder removed: the zip is what goes to the Mac.
#[cfg(windows)]
fn finish(app: &std::path::Path, steps: &mut Vec<Step>) {
    let (app, signed) = (app.to_path_buf(), app.to_path_buf());
    steps.push(("Signing the app".into(), Box::new(move || nxport_build::macho::sign(&signed, None))));
    steps.push((
        "Zipping the app".into(),
        Box::new(move || {
            let zip = nxport_build::pack::zip_app(&app)?;
            debug_assert_eq!(zip, handed_over(&app));
            nxport_build::pack::clean_path(&app).map_err(|e| format!("{}: {e}", app.display()))?;
            println!("  {}", zip.display());
            Ok(())
        }),
    ));
}

/// The macOS Metal shader cache (stages extracted, translated, compiled by Apple's Metal compiler) and the shared
/// pipeline list beside it: the macOS app's cache, and what the iOS cache is compiled again from. Returns its folder.
#[cfg(target_os = "macos")]
pub(crate) fn metal_cache_steps(r: &Request, steps: &mut Vec<Step>) -> PathBuf {
    use crate::app::{extract_stages_step, pipeline_list};
    let (stages, cache) = (r.work.join("stages"), r.work.join("shader_cache"));
    steps.push(extract_stages_step(&r.job, &r.dump, &stages));
    let batch_enabled = r.settings.graphics.batch;
    {
        let (cache, job) = (cache.clone(), r.job.clone());
        steps.push((
            "Building the shader cache".into(),
            Box::new(move || {
                use nxport_build::shader_cache_build::ShaderBuildPhase as Phase;
                let mut translation = nxport_build::translation::get();
                translation.no_batch_kernels = !batch_enabled;
                nxport_build::translation::install(translation);
                let on_progress = Arc::new(move |phase: Phase, done: usize, total: usize| {
                    crate::app::phase_progress(&job, phase.to_string(), phase.phase_index(), Phase::NUM_PHASES, done, total);
                });
                nxport_build::shader_cache_build::build(&nxport_build::shader_cache_build::Options {
                    dump_dirs: vec![stages],
                    out_dir: cache,
                    ctrl_dir: None,
                    jobs: crate::app::shader_jobs(),
                    msl_out: None,
                    index_only: false,
                    on_progress: Some(on_progress),
                })
            }),
        ));
    }
    let (list_cache, title_id) = (cache.clone(), r.title_id);
    steps.push(("Getting the shared pipeline list".into(), Box::new(move || pipeline_list(title_id, &list_cache, true))));
    cache
}
