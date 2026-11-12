// The macOS app's build steps: the Metal shader cache from the game's shader stages, the shared pipeline list and its
// precompiled pipelines, then the app (nxport_build::pack). The iOS app's steps are in ios.rs; they share the shader
// cache steps here.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nxport::settings;

use crate::app::{extract_stages_step, pipeline_list, Output, Request, Step};

pub fn steps(r: Request, steps: &mut Vec<Step>) -> Output {
    // Without the cache the app translates every shader as the game loads it.
    let cache = if r.prebuild {
        let cache = metal_cache_steps(&r, steps);
        let archive_cache = cache.clone();
        steps.push(("Precompiling pipelines".into(), Box::new(move || nxport_build::pipeline_archive::build_mac(&archive_cache))));
        Some(cache)
    } else {
        None
    };
    let Request { name, work, dump, mods, out_dir, edits, .. } = r;
    let app = out_dir.join(format!("{}.app", name.replace([':', '/'], " -")));
    let settings_file = work.join("settings.toml");
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
    Arc::new(Mutex::new(Some(app)))
}

/// The macOS Metal shader cache (stages extracted, translated, compiled by Apple's Metal compiler) and the shared
/// pipeline list beside it: the macOS app's cache, and what the iOS cache is compiled again from. Returns its folder.
pub(crate) fn metal_cache_steps(r: &Request, steps: &mut Vec<Step>) -> PathBuf {
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
