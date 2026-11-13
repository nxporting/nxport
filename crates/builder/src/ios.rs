// The iOS app's build steps: one list on every host. Only how some steps are done depends on the host:
// - the engine for iOS: a Mac builder carries it (build.rs builds it beside the builder); on Windows it is built from
//   this checkout first (nxport_build::apple_engine).
// - the shaders: on a Mac, Apple's Metal compiler compiles the shader cache for iOS (and, for a chosen GPU, its listed
//   pipelines); on Windows, without that compiler, the stages are translated and packed as Metal source, which the app
//   compiles the first time the game uses each (metal/source_pack.rs), as the macOS app built on Windows does.
// - the app (nxport_build::ipa): on a Mac in this process; on Windows in a worker process, as every Windows step that
//   sets up the guest image (windows.rs).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::app::{Output, Request, Step};

/// What `shaders` leaves for the app: the folder it puts in the .ipa as its shader cache.
type Cache = Option<PathBuf>;

pub fn steps(r: Request, steps: &mut Vec<Step>) -> Output {
    engine(&r, steps);
    let cache = if r.prebuild { Some(shaders(&r, steps)) } else { None };
    app(r, cache, steps)
}

/// Each step of nxport_build::ipa::build on the progress bar, with its share of the step (a Windows worker reports
/// the same through windows::report).
#[cfg(target_os = "macos")]
fn ipa_progress(job: &Mutex<crate::app::Job>, step: nxport_build::ipa::IpaBuildPhase) {
    println!("[+] {step}");
    crate::app::progress(job, step.to_string(), 0, 0, step.default_fraction());
}

/// The engine for iOS, built from this checkout (as the macOS app's). A Mac builder carries it.
#[cfg(windows)]
fn engine(r: &Request, steps: &mut Vec<Step>) {
    crate::apple::engine_from_source(nxport_build::apple_engine::Os::Ios, r, steps);
}

#[cfg(target_os = "macos")]
fn engine(_: &Request, _: &mut Vec<Step>) {}

/// Mac: the macOS shader cache (shared with the macOS app's steps), compiled again for iOS, and with a GPU chosen its
/// listed pipelines precompiled for that GPU.
#[cfg(target_os = "macos")]
fn shaders(r: &Request, steps: &mut Vec<Step>) -> PathBuf {
    let cache = crate::apple::metal_cache_steps(r, steps);
    let ios_cache = r.work.join("shader_cache_ios");
    {
        let (cache, ios_cache, job) = (cache.clone(), ios_cache.clone(), r.job.clone());
        steps.push((
            "Building the iOS shader cache".into(),
            Box::new(move || {
                use nxport_build::shader_cache_build::IosShaderBuildPhase as Phase;
                let cb = move |phase: Phase, done: usize, total: usize| {
                    crate::app::phase_progress(&job, phase.to_string(), phase.phase_index(), Phase::NUM_PHASES, done, total);
                };
                nxport_build::shader_cache_build::build_ios(&cache, &ios_cache, crate::app::shader_jobs(), false, Some(&cb))?;
                let list = cache.join("pipelines.txt");
                if list.exists() {
                    std::fs::copy(&list, ios_cache.join("pipelines.txt")).map_err(|e| e.to_string())?;
                }
                Ok(())
            }),
        ));
    }
    if !r.ios_gpu.is_empty() {
        let (ios_cache, scratch, gpu) = (ios_cache.clone(), r.work.join("pipeline_script"), r.ios_gpu.clone());
        steps.push(("Precompiling pipelines".into(), Box::new(move || nxport_build::pipeline_archive::build_ios(&ios_cache, &scratch, &gpu))));
    }
    ios_cache
}

/// Windows: the stages the shared pipeline list uses, translated and packed as Metal source.
#[cfg(windows)]
fn shaders(r: &Request, steps: &mut Vec<Step>) -> PathBuf {
    crate::windows::metal_source_steps(r, steps)
}

/// The .ipa itself.
fn app(r: Request, cache: Cache, steps: &mut Vec<Step>) -> Output {
    let out: Output = Arc::new(Mutex::new(None));
    let result = out.clone();
    let Request { name, work, dump, mods, out_dir, edits, bundle_game, entitlements, bundle_id, job, .. } = r;
    steps.push((
        "Building the iOS app".into(),
        Box::new(move || {
            std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
            #[cfg(windows)]
            let ipa = {
                let _ = &name;
                crate::windows::ipa_in_worker(&job, &work, dump, mods, out_dir.clone(), edits, bundle_game, entitlements, bundle_id, cache)?;
                nxport_build::ipa::path_for(&out_dir, &name)
            };
            #[cfg(target_os = "macos")]
            let ipa = {
                let _ = (&name, &work);
                nxport_build::ipa::build(
                    &nxport_build::ipa::Options {
                        dump: &dump,
                        mods: mods.as_deref(),
                        cache: cache.as_deref(),
                        out_dir: &out_dir,
                        edits,
                        bundle_game,
                        entitlements,
                        bundle_id,
                    },
                    &|step| ipa_progress(&job, step),
                )?
            };
            *out.lock().unwrap() = Some(ipa);
            Ok(())
        }),
    ));
    result
}
