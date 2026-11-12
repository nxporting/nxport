// App icons for the Apple apps from the dump's icon (app_icon::source: the control data's icon_<language>.dat, a JPEG,
// else Logo/<title id>.png), scaled with the image crate on every host.

#[cfg(target_os = "macos")]
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

/// The game's icon in the dump, if it has one.
pub fn logo(dump: &Path, title_id: u64) -> Option<PathBuf> {
    crate::app_icon::source(dump, title_id)
}

#[cfg(target_os = "macos")]
fn run(cmd: &mut Command) -> Result<(), String> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let out = cmd.output().map_err(|e| format!("{name}: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("{name}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Writes `src` (JPEG or PNG, told apart by its bytes: the control icon's name ends in .dat) scaled to `px` square
/// pixels as the PNG `out`.
pub fn scaled_png(src: &Path, px: u32, out: &Path) -> Result<(), String> {
    let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("{}: {e}", src.display()))?;
    img.resize_exact(px, px, image::imageops::FilterType::Lanczos3)
        .save_with_format(out, image::ImageFormat::Png)
        .map_err(|e| format!("{}: {e}", out.display()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn scales_a_jpeg_named_dat_into_a_png() {
        let dir = std::env::temp_dir().join(format!("nxport-icon-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("icon_AmericanEnglish.dat");
        image::RgbImage::from_pixel(256, 256, image::Rgb([200, 30, 30])).save_with_format(&src, image::ImageFormat::Jpeg).unwrap();
        let out = dir.join("AppIcon60x60@2x.png");
        super::scaled_png(&src, 120, &out).unwrap();
        let png = image::open(&out).unwrap();
        assert_eq!((png.width(), png.height()), (120, 120));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// Writes a macOS icon file (.icns) made from `src` to `out`.
#[cfg(target_os = "macos")]
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
