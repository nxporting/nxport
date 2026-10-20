// Metal binary archives of a shader cache's pipeline list (pipelines.txt): compiled GPU code, so the app compiles no
// listed pipeline when the game first draws it. The engine builds them (--build-pipeline-archive): for this Mac's GPU
// straight into the cache, or for an iPhone's GPU through a pipeline script that metal-tt (Xcode) compiles.

use std::path::Path;
use std::process::Command;

/// iPhone / iPad GPUs: (label, Metal GPU architecture, first iOS version for it).
pub const IOS_GPUS: &[(&str, &str, &str)] = &[
    ("A12 (iPhone XS, XR)", "applegpu_g11p", "16.0"),
    ("A13 (iPhone 11, SE 2)", "applegpu_g12p", "16.0"),
    ("A14 (iPhone 12)", "applegpu_g13p", "16.0"),
    ("A15 (iPhone 13, 14, SE 3)", "applegpu_g14p", "16.0"),
    ("A16 (iPhone 14 Pro, 15)", "applegpu_g15p", "16.0"),
    ("A17 Pro (iPhone 15 Pro)", "applegpu_g16p", "17.0"),
    ("A18 (iPhone 16, 16e)", "applegpu_g17p", "18.0"),
    ("M1 (iPad)", "applegpu_g13g", "16.0"),
    ("M2 (iPad)", "applegpu_g14g", "17.0"),
    ("M4 (iPad)", "applegpu_g16g", "18.0"),
];

fn engine() -> Result<std::path::PathBuf, String> {
    crate::pack::locate_engine_binary().map_err(|e| e.to_string())
}

fn run_engine(cache: &Path, out: &Path, script: bool) -> Result<(), String> {
    let mut c = Command::new(engine()?);
    c.arg(format!("--shader-cache={}", cache.display()))
        .arg("--translated-shaders=1")
        .arg(format!("--build-pipeline-archive={}", out.display()));
    if script {
        c.arg("--pipeline-archive-script");
    }
    let o = c.output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&o.stdout);
    for line in text.lines().filter(|l| l.contains("Pipelines:")) {
        println!("  {}", line.trim_start_matches("[+] ").trim_start_matches("Pipelines: "));
    }
    if !o.status.success() {
        return Err(format!("pipeline archives: {}", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("failed")));
    }
    Ok(())
}

fn remove_archives(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let n = e.file_name();
        let n = n.to_string_lossy();
        if n.starts_with("pipelines") && n.ends_with(".metalar") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Archives of the cache's pipeline list for this Mac, in the cache.
pub fn build_mac(cache: &Path) -> Result<(), String> {
    remove_archives(cache);
    if !cache.join("pipelines.txt").exists() {
        println!("  No pipeline list: nothing to precompile");
        return Ok(());
    }
    run_engine(cache, cache, false)
}

/// An archive of the iOS cache's pipeline list for the GPU `arch` (IOS_GPUS), as <ios cache>/pipelines.metalar.
/// `work`: a scratch directory for the engine's archives and script.
pub fn build_ios(ios_cache: &Path, work: &Path, arch: &str) -> Result<(), String> {
    remove_archives(ios_cache);
    if !ios_cache.join("pipelines.txt").exists() {
        println!("  No pipeline list: nothing to precompile");
        return Ok(());
    }
    let min = IOS_GPUS.iter().find(|g| g.1 == arch).map_or("18.0", |g| g.2);
    let _ = std::fs::remove_dir_all(work);
    run_engine(ios_cache, work, true)?;
    let out = ios_cache.join("pipelines.metalar");
    let o = Command::new("/usr/bin/xcrun")
        .args(["-sdk", "iphoneos", "metal-tt", "-arch", arch, "-ios-version-min", min, "pipelines.mtlp-json", "-o"])
        .arg(&out)
        .current_dir(work)
        .output()
        .map_err(|e| format!("metal-tt (Xcode): {e}"))?;
    if !o.status.success() || !out.exists() {
        let err = String::from_utf8_lossy(&o.stderr);
        let err = err.lines().filter(|l| !l.starts_with('+')).find(|l| l.contains("error")).unwrap_or("failed");
        return Err(format!("metal-tt: {err}"));
    }
    let _ = std::fs::remove_dir_all(work);
    println!(
        "  {} for {arch}: {} MB",
        out.display(),
        std::fs::metadata(&out).map_or(0, |m| m.len() >> 20)
    );
    Ok(())
}
