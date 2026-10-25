// The Windows target's build steps: download/find zig cc toolchain, package the Windows app folder (nxport.exe,
// settings.toml, mods, game files), lift the game's guest code into C files, compile them into guest.dll using
// zig cc, and copy modules.txt into the app folder.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::app::{pipeline_list, Output, Request, Step};

pub const PRECOMPILES_SHADERS: bool = false;

pub fn steps(r: Request, steps: &mut Vec<Step>) -> Output {
    let Request {
        target: _,
        name,
        title_id,
        work,
        dump,
        mods,
        out_dir,
        edits,
        settings: _,
        prebuild: _,
        bundle_game,
        entitlements: _,
        ios_gpu: _,
        bundle_id: _,
        job,
    } = r;

    let app_folder = out_dir.join(name.replace([':', '/'], " -"));
    let toolchains = crate::app::data_dir().join("toolchains");
    let pipeline_cache = work.join("shader_cache");
    let c_dir = work.join("lifted");
    let out: Arc<Mutex<Option<PathBuf>>> = Arc::new(Mutex::new(None));

    // 1. Toolchain
    {
        let toolchains = toolchains.clone();
        let job = job.clone();
        steps.push((
            "Checking C compiler".into(),
            Box::new(move || {
                let job_cb = job.clone();
                let _ = nxport_build::zig::cc(&toolchains, &move |done, total| {
                    let mut j = job_cb.lock().unwrap();
                    j.sub_step = Some("Downloading zig 0.13".into());
                    j.sub_done = (done / 1024 / 1024) as usize;
                    j.sub_total = (total / 1024 / 1024) as usize;
                    j.sub_frac = if total > 0 {
                        (done as f32 / total as f32).min(1.0)
                    } else {
                        0.0
                    };
                })?;
                Ok(())
            }),
        ));
    }

    // 2. Shared pipeline list
    {
        let pipeline_cache = pipeline_cache.clone();
        steps.push((
            "Getting the shared pipeline list".into(),
            Box::new(move || {
                let _ = std::fs::create_dir_all(&pipeline_cache);
                pipeline_list(title_id, &pipeline_cache, false)
            }),
        ));
    }

    // 3. Packaging the app folder
    {
        let dump = dump.clone();
        let mods = mods.clone();
        let app_folder = app_folder.clone();
        let edits = edits.clone();
        let pipeline_list_path = pipeline_cache.join("pipelines.txt");
        steps.push((
            "Packaging the app".into(),
            Box::new(move || {
                nxport_build::winapp::pack(&nxport_build::winapp::Options {
                    dump: &dump,
                    mods: mods.as_deref(),
                    app: &app_folder,
                    edits,
                    bundle_game,
                    pipeline_list: pipeline_list_path.is_file().then_some(pipeline_list_path.as_path()),
                })?;
                Ok(())
            }),
        ));
    }

    // 4. Lifting the game code
    {
        let dump = dump.clone();
        let mods = mods.clone();
        let edits = edits.clone();
        let c_dir = c_dir.clone();
        let job = job.clone();
        steps.push((
            "Lifting game code".into(),
            Box::new(move || {
                let s = nxport_build::lift::game_settings(&dump, edits, mods.as_deref());
                let job_cb = job.clone();
                let lifted = nxport_build::lift::lift_game(s, &c_dir, &move |i, n, name| {
                    let mut j = job_cb.lock().unwrap();
                    j.sub_step = Some(format!("Lifting {name}"));
                    j.sub_done = i + 1;
                    j.sub_total = n;
                    j.sub_frac = if n > 0 {
                        ((i + 1) as f32 / n as f32).min(1.0)
                    } else {
                        0.0
                    };
                })
                .map_err(|e| e.to_string())?;
                for (name, r) in &lifted {
                    nxport_build::lift::report(name, r);
                }
                Ok(())
            }),
        ));
    }

    // 5. Compiling guest code
    {
        let toolchains = toolchains.clone();
        let c_dir = c_dir.clone();
        let app_folder = app_folder.clone();
        let job = job.clone();
        let out_res = out.clone();
        steps.push((
            "Compiling guest code".into(),
            Box::new(move || {
                let cc = nxport_build::zig::cc(&toolchains, &|_, _| {})?;
                let guest_dll = app_folder.join("guest.dll");
                let cpus = std::thread::available_parallelism().map_or(6, |n| n.get());
                let jobs = cpus.saturating_sub(2).max(2);
                let job_cb = job.clone();
                nxport_build::lift::compile(&c_dir, &guest_dll, &cc, jobs, &move |done, total| {
                    let mut j = job_cb.lock().unwrap();
                    j.sub_step = Some(format!("Compiling ({done}/{total})"));
                    j.sub_done = done;
                    j.sub_total = total;
                    j.sub_frac = if total > 0 {
                        (done as f32 / total as f32).min(1.0)
                    } else {
                        0.0
                    };
                })
                .map_err(|e| e.to_string())?;

                let src_modules = c_dir.join(nxport_build::lift::MODULES_FILE);
                if src_modules.is_file() {
                    std::fs::copy(&src_modules, app_folder.join(nxport_build::lift::MODULES_FILE))
                        .map_err(|e| format!("copying {}: {e}", nxport_build::lift::MODULES_FILE))?;
                }

                *out_res.lock().unwrap() = Some(app_folder);
                Ok(())
            }),
        ));
    }

    out
}
