// The Apple targets' build steps: the Metal shader cache from the game's shader stages, the shared pipeline list and
// its precompiled pipelines, then the macOS app (nxport_build::pack) or the iOS app (nxport_build::ipa).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use nxport::settings;

use crate::app::{extract_stages_step, pipeline_list, Output, Request, Step, Target};

pub fn steps(r: Request, steps: &mut Vec<Step>) -> Output {
    let Request { target, name, title_id, work, dump, mods, out_dir, edits, settings: resolved, prebuild, bundle_game, entitlements, ios_gpu, bundle_id, job } = r;
    let (stages, cache) = (work.join("stages"), work.join("shader_cache"));
    // Without the cache the app translates every shader as the game loads it.
    if prebuild {
        steps.push(extract_stages_step(&job, &dump, &stages));
        let batch_enabled = resolved.graphics.batch;
        {
            let (stages, cache) = (stages.clone(), cache.clone());
            let job = job.clone();
            steps.push(("Building the shader cache".into(), Box::new(move || {
                let mut translation = nxport_build::translation::get();
                translation.no_batch_kernels = !batch_enabled;
                nxport_build::translation::install(translation);

                let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
                let jobs = cpus.saturating_sub(2).max(2);
                let job_cb = job.clone();
                let on_progress = Arc::new(move |phase: nxport_build::shader_cache_build::ShaderBuildPhase, done: usize, total: usize| {
                    let mut j = job_cb.lock().unwrap();
                    j.sub_step = Some(phase.to_string());
                    j.sub_done = done;
                    j.sub_total = total;
                    let phase_idx = phase.phase_index();
                    let num_phases = nxport_build::shader_cache_build::ShaderBuildPhase::NUM_PHASES;
                    let phase_frac = if total > 0 {
                        (done as f32 / total as f32).min(1.0)
                    } else {
                        0.0
                    };
                    let new_sub_frac = (phase_idx as f32 + phase_frac) / num_phases as f32;
                    j.sub_frac = j.sub_frac.max(new_sub_frac).min(1.0);
                });
                nxport_build::shader_cache_build::build(&nxport_build::shader_cache_build::Options {
                    dump_dirs: vec![stages],
                    out_dir: cache,
                    ctrl_dir: None,
                    jobs,
                    msl_out: None,
                    index_only: false,
                    on_progress: Some(on_progress),
                })
            })));
        }
        let list_cache = cache.clone();
        steps.push(("Getting the shared pipeline list".into(), Box::new(move || pipeline_list(title_id, &list_cache, true))));
        if target == Target::MacApp {
            let archive_cache = cache.clone();
            steps.push(("Precompiling pipelines".into(), Box::new(move || nxport_build::pipeline_archive::build_mac(&archive_cache))));
        }
    }
    match target {
        Target::MacApp => {
            let app = out_dir.join(format!("{}.app", name.replace([':', '/'], " -")));
            let settings_file = work.join("settings.toml");
            let app2 = app.clone();
            steps.push(("Packaging the app".into(), Box::new(move || {
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
                    shader_cache_dir: prebuild.then_some(cache.as_path()),
                    out_app: Some(&app2),
                    settings: Some(&settings_file),
                    symlink_assets: false,
                })
                .map_err(|e| e.to_string())
            })));
            Arc::new(Mutex::new(Some(app)))
        }
        Target::Ipa => {
            let ios_cache = work.join("shader_cache_ios");
            if prebuild {
                let (cache, ios_cache_dir) = (cache.clone(), ios_cache.clone());
                let job = job.clone();
                steps.push(("Building the iOS shader cache".into(), Box::new(move || {
                    let ios_cache = ios_cache_dir;
                    let job_cb = job.clone();
                    let cb = move |sub: nxport_build::shader_cache_build::IosShaderBuildPhase, done: usize, total: usize| {
                        let mut j = job_cb.lock().unwrap();
                        j.sub_step = Some(sub.to_string());
                        j.sub_done = done;
                        j.sub_total = total;
                        let phase_idx = sub.phase_index();
                        let num_phases = nxport_build::shader_cache_build::IosShaderBuildPhase::NUM_PHASES;
                        let phase_frac = if total > 0 {
                            (done as f32 / total as f32).min(1.0)
                        } else {
                            0.0
                        };
                        let new_sub_frac = (phase_idx as f32 + phase_frac) / num_phases as f32;
                        j.sub_frac = j.sub_frac.max(new_sub_frac).min(1.0);
                    };
                    let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
                    let jobs = cpus.saturating_sub(2).max(2);
                    nxport_build::shader_cache_build::build_ios(&cache, &ios_cache, jobs, false, Some(&cb))?;
                    let list = cache.join("pipelines.txt");
                    if list.exists() {
                        std::fs::copy(&list, ios_cache.join("pipelines.txt")).map_err(|e| e.to_string())?;
                    }
                    Ok(())
                })));
                if !ios_gpu.is_empty() {
                    let (ios_cache, scratch, gpu) = (ios_cache.clone(), work.join("pipeline_script"), ios_gpu.clone());
                    steps.push(("Precompiling pipelines".into(), Box::new(move || {
                        nxport_build::pipeline_archive::build_ios(&ios_cache, &scratch, &gpu)
                    })));
                }
            }
            let out: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));
            let out2 = out.clone();
            steps.push(("Building the iOS app".into(), Box::new(move || {
                std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
                let job_cb = job.clone();
                let ipa = nxport_build::ipa::build(
                    &nxport_build::ipa::Options {
                        dump: &dump,
                        mods: mods.as_deref(),
                        cache: prebuild.then_some(ios_cache.as_path()),
                        out_dir: &out_dir,
                        edits,
                        bundle_game,
                        entitlements,
                        bundle_id,
                    },
                    &move |step| {
                        println!("[+] {step}");
                        let mut j = job_cb.lock().unwrap();
                        j.sub_step = Some(step.to_string());
                        j.sub_done = 0;
                        j.sub_total = 0;
                        j.sub_frac = j.sub_frac.max(step.default_fraction());
                    },
                )?;
                *out2.lock().unwrap() = Some(ipa);
                Ok(())
            })));
            out
        }
        Target::WindowsApp => unreachable!("not a target here"),
    }
}
