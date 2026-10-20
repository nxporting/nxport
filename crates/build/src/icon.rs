// App icons from a dump's Logo/<title id>.png (512x512), scaled with macOS's sips.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The game's logo in the dump, if it has one.
pub fn logo(dump: &Path, title_id: u64) -> Option<PathBuf> {
    let p = dump.join("Logo").join(format!("{title_id:016x}.png"));
    p.is_file().then_some(p)
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd.output().map_err(|e| format!("{name}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{name}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Writes `src` scaled to `px` square pixels as the PNG `out`.
pub fn scaled_png(src: &Path, px: u32, out: &Path) -> Result<(), String> {
    run(Command::new("/usr/bin/sips").args(["-s", "format", "png", "-z", &px.to_string(), &px.to_string()]).arg(src).arg("--out").arg(out))
}

/// Writes a macOS icon file (.icns) made from `src` to `out`.
pub fn icns(src: &Path, out: &Path) -> Result<(), String> {
    let set = out.with_extension("iconset");
    let _ = fs::remove_dir_all(&set);
    fs::create_dir_all(&set).map_err(|e| format!("{}: {e}", set.display()))?;
    for (px, name) in [
        (16, "icon_16x16"), (32, "icon_16x16@2x"), (32, "icon_32x32"), (64, "icon_32x32@2x"),
        (128, "icon_128x128"), (256, "icon_128x128@2x"), (256, "icon_256x256"), (512, "icon_256x256@2x"),
        (512, "icon_512x512"), (1024, "icon_512x512@2x"), // iconutil requires every size
    ] {
        scaled_png(src, px, &set.join(format!("{name}.png")))?;
    }
    let r = run(Command::new("/usr/bin/iconutil").args(["-c", "icns", "-o"]).arg(out).arg(&set));
    let _ = fs::remove_dir_all(&set);
    r
}
