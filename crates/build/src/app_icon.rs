// Every app's icon, from the dump (the console's icon from the control data, or a logo): scaled into the Apple apps'
// PNGs and macOS .icns, or into a Windows .ico written into the app's program as its icon resource.
//
// The control data's icon_<language>.dat is the title's icon as the HOME menu shows it: a 256x256 JPEG beside
// control.nacp (control/ when the control NCA's RomFS is extracted into the dump). Logo/<title id>.png is a logo some
// dumps carry.

#[cfg(target_os = "macos")]
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

/// Where the dump's icon is, if it has one: the American English control icon first, then any language's, then a logo.
pub fn source(dump: &Path, title_id: u64) -> Option<PathBuf> {
    for dir in [dump.join("control"), dump.join("Control"), dump.to_path_buf()] {
        let first = dir.join("icon_AmericanEnglish.dat");
        if first.is_file() {
            return Some(first);
        }
        let mut any: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("icon_") && n.to_string_lossy().ends_with(".dat")))
            .collect();
        any.sort();
        if let Some(p) = any.into_iter().next() {
            return Some(p);
        }
    }
    let logo = dump.join("Logo").join(format!("{title_id:016x}.png"));
    logo.is_file().then_some(logo)
}

/// The .ico for `src` (JPEG or PNG): PNG images of 256, 64, 48, 32 and 16 pixels, as Explorer and the taskbar pick.
pub fn ico(src: &Path) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("{}: {e}", src.display()))?.into_rgba8();
    const SIZES: [u32; 5] = [256, 64, 48, 32, 16];
    let mut pngs = Vec::new();
    for px in SIZES {
        let scaled = image::imageops::resize(&img, px, px, image::imageops::FilterType::Lanczos3);
        let mut png = Vec::new();
        scaled
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        pngs.push((px, png));
    }
    // ICONDIR, then an ICONDIRENTRY per image (a size of 256 is written as 0), then the images.
    let mut out = Vec::new();
    out.extend_from_slice(&[0, 0, 1, 0]);
    out.extend_from_slice(&(pngs.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * pngs.len() as u32;
    for (px, png) in &pngs {
        let side = if *px >= 256 { 0 } else { *px as u8 };
        out.extend_from_slice(&[side, side, 0, 0]);
        out.extend_from_slice(&1u16.to_le_bytes()); // planes
        out.extend_from_slice(&32u16.to_le_bytes()); // bits per pixel
        out.extend_from_slice(&(png.len() as u32).to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in &pngs {
        out.extend_from_slice(png);
    }
    Ok(out)
}

/// Writes the .ico `ico` into the program `exe` as its icon (RT_GROUP_ICON 1, its images RT_ICON 1..): the first icon
/// group, which Explorer shows, and the one the engine's window takes (SDL_HINT_WINDOWS_INTRESOURCE_ICON).
#[cfg(windows)]
pub fn set_exe_icon(exe: &Path, ico: &[u8]) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn BeginUpdateResourceW(file: *const u16, delete_existing: i32) -> *mut c_void;
        fn UpdateResourceW(h: *mut c_void, kind: *const u16, name: *const u16, lang: u16, data: *const c_void, size: u32) -> i32;
        fn EndUpdateResourceW(h: *mut c_void, discard: i32) -> i32;
    }
    const RT_ICON: usize = 3;
    const RT_GROUP_ICON: usize = 14;
    const LANG_NEUTRAL: u16 = 0;
    let u16_at = |at: usize| u16::from_le_bytes([ico[at], ico[at + 1]]);
    let u32_at = |at: usize| u32::from_le_bytes(ico[at..at + 4].try_into().unwrap());
    if ico.len() < 6 {
        return Err("not an .ico".into());
    }
    let count = u16_at(4) as usize;
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let err = |what: &str| format!("{}: {what}: {}", exe.display(), std::io::Error::last_os_error());
    unsafe {
        let h = BeginUpdateResourceW(path.as_ptr(), 0);
        if h.is_null() {
            return Err(err("BeginUpdateResource"));
        }
        // GRPICONDIR: the ICONDIR with each entry's file offset replaced by its RT_ICON id (a 2-byte field).
        let mut group = ico[..6].to_vec();
        for i in 0..count {
            let e = 6 + 16 * i;
            let (size, offset) = (u32_at(e + 8) as usize, u32_at(e + 12) as usize);
            let id = (i + 1) as u16;
            let image = &ico[offset..offset + size];
            if UpdateResourceW(h, RT_ICON as *const u16, id as usize as *const u16, LANG_NEUTRAL, image.as_ptr() as *const c_void, size as u32) == 0 {
                EndUpdateResourceW(h, 1);
                return Err(err("UpdateResource (icon image)"));
            }
            group.extend_from_slice(&ico[e..e + 12]);
            group.extend_from_slice(&id.to_le_bytes());
        }
        if UpdateResourceW(h, RT_GROUP_ICON as *const u16, 1 as *const u16, LANG_NEUTRAL, group.as_ptr() as *const c_void, group.len() as u32) == 0 {
            EndUpdateResourceW(h, 1);
            return Err(err("UpdateResource (icon group)"));
        }
        if EndUpdateResourceW(h, 0) == 0 {
            return Err(err("EndUpdateResource"));
        }
    }
    Ok(())
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
