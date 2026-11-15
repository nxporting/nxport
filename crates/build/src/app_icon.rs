// Every app's icon, from the dump (the console's icon from the control data, or a logo): scaled into the Apple apps'
// PNGs and macOS .icns, or into a Windows .ico written into the app's program as its icon resource.
//
// The control data's icon_<language>.dat is the title's icon as the HOME menu shows it: a 256x256 JPEG beside
// control.nacp (control/ when the control NCA's RomFS is extracted into the dump). Logo/<title id>.png is a logo some
// dumps carry.

use std::path::{Path, PathBuf};

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


/// The version resource of an app's nxport.exe (RT_VERSION 1, VS_VERSIONINFO): the game's name as its description and
/// product, so Task Manager, Explorer and the taskbar show the game rather than the builder's own description (the engine
/// is a copy of the builder). `version`: the game's display version, if known.
pub fn version_resource(game: &str, version: Option<&str>) -> Vec<u8> {
    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()).collect()
    }
    fn pad4(v: &mut Vec<u8>) {
        while v.len() % 4 != 0 {
            v.push(0);
        }
    }
    // A node: wLength, wValueLength (words for text), wType (1 text, 0 binary), the key, padding, the value, padding,
    // the children.
    fn node(key: &str, value: &[u8], text: bool, children: &[Vec<u8>]) -> Vec<u8> {
        let mut v = vec![0u8; 6];
        v.extend(utf16z(key));
        pad4(&mut v);
        v.extend_from_slice(value);
        let value_len = if text { value.len() / 2 } else { value.len() } as u16;
        for c in children {
            pad4(&mut v);
            v.extend_from_slice(c);
        }
        let len = v.len() as u16;
        v[0..2].copy_from_slice(&len.to_le_bytes());
        v[2..4].copy_from_slice(&value_len.to_le_bytes());
        v[4..6].copy_from_slice(&(text as u16).to_le_bytes());
        v
    }
    let s = |k: &str, val: &str| node(k, &utf16z(val), true, &[]);
    let mut strings = vec![
        s("FileDescription", game),
        s("ProductName", game),
        s("CompanyName", "The nxport contributors"),
        s("OriginalFilename", "nxport.exe"),
        s("Comments", "Made with nxport, https://github.com/nxporting/nxport"),
    ];
    if let Some(ver) = version {
        strings.push(s("ProductVersion", ver));
    }
    let table = node("040904B0", &[], true, &strings);
    let string_info = node("StringFileInfo", &[], true, &[table]);
    let translation = node("Translation", &[0x09, 0x04, 0xB0, 0x04], false, &[]);
    let var_info = node("VarFileInfo", &[], true, &[translation]);
    // VS_FIXEDFILEINFO: signature, struct version, file and product versions 0, flags mask 0x3f, flags 0, OS
    // VOS_NT_WINDOWS32, type VFT_APP, subtype and date 0.
    let mut fixed = Vec::new();
    for w in [0xFEEF04BDu32, 0x0001_0000, 0, 0, 0, 0, 0x3f, 0, 0x0004_0004, 1, 0, 0, 0] {
        fixed.extend(w.to_le_bytes());
    }
    let mut root = node("VS_VERSION_INFO", &fixed, false, &[string_info, var_info]);
    // wValueLength of the root is the fixed info's size in bytes.
    root[2..4].copy_from_slice(&(fixed.len() as u16).to_le_bytes());
    root
}

/// Writes `res` (version_resource) into the program `exe` as its version resource.
#[cfg(windows)]
pub fn set_exe_version(exe: &Path, res: &[u8]) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    extern "system" {
        fn BeginUpdateResourceW(file: *const u16, delete_existing: i32) -> *mut c_void;
        fn UpdateResourceW(h: *mut c_void, kind: *const u16, name: *const u16, lang: u16, data: *const c_void, size: u32) -> i32;
        fn EndUpdateResourceW(h: *mut c_void, discard: i32) -> i32;
    }
    const RT_VERSION: usize = 16;
    // The builder's own resource is US English (rc.exe's default): replaced in place.
    const LANG_EN_US: u16 = 0x409;
    let path: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
    let err = |what: &str| format!("{}: {what}: {}", exe.display(), std::io::Error::last_os_error());
    unsafe {
        let h = BeginUpdateResourceW(path.as_ptr(), 0);
        if h.is_null() {
            return Err(err("BeginUpdateResource"));
        }
        if UpdateResourceW(h, RT_VERSION as *const u16, 1 as *const u16, LANG_EN_US, res.as_ptr() as *const c_void, res.len() as u32) == 0 {
            EndUpdateResourceW(h, 1);
            return Err(err("UpdateResource (version)"));
        }
        if EndUpdateResourceW(h, 0) == 0 {
            return Err(err("EndUpdateResource"));
        }
    }
    Ok(())
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
    /// The version resource as Windows reads it back from a program: its description and product are the game.
    #[cfg(windows)]
    #[test]
    fn version_resource_reads_back() {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "version")]
        extern "system" {
            fn GetFileVersionInfoSizeW(file: *const u16, handle: *mut u32) -> u32;
            fn GetFileVersionInfoW(file: *const u16, handle: u32, len: u32, data: *mut u8) -> i32;
            fn VerQueryValueW(block: *const u8, sub: *const u16, out: *mut *const u16, len: *mut u32) -> i32;
        }
        let exe = std::env::temp_dir().join(format!("nxport-ver-{}.exe", std::process::id()));
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        super::set_exe_version(&exe, &super::version_resource("Some Game", Some("1.9.0"))).unwrap();
        let wide = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<u16>>();
        let path: Vec<u16> = exe.as_os_str().encode_wide().chain([0]).collect();
        let read = |key: &str| unsafe {
            let n = GetFileVersionInfoSizeW(path.as_ptr(), std::ptr::null_mut());
            assert!(n > 0, "no version info");
            let mut data = vec![0u8; n as usize];
            assert!(GetFileVersionInfoW(path.as_ptr(), 0, n, data.as_mut_ptr()) != 0);
            let (mut p, mut len) = (std::ptr::null(), 0u32);
            if VerQueryValueW(data.as_ptr(), wide(&format!(r"\StringFileInfo\040904B0\{key}")).as_ptr(), &mut p, &mut len) == 0 {
                return None;
            }
            Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, len as usize)).trim_end_matches(char::from(0)).to_string())
        };
        let (desc, product, ver) = (read("FileDescription"), read("ProductName"), read("ProductVersion"));
        let _ = std::fs::remove_file(&exe);
        assert_eq!(desc.as_deref(), Some("Some Game"));
        assert_eq!(product.as_deref(), Some("Some Game"));
        assert_eq!(ver.as_deref(), Some("1.9.0"));
    }

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

    #[test]
    fn icns_holds_every_size_as_png() {
        let dir = std::env::temp_dir().join(format!("nxport-icns-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("icon_AmericanEnglish.dat");
        image::RgbImage::from_pixel(256, 256, image::Rgb([30, 200, 30])).save_with_format(&src, image::ImageFormat::Jpeg).unwrap();
        let out = dir.join("AppIcon.icns");
        super::icns(&src, &out).unwrap();
        let f = std::fs::read(&out).unwrap();
        assert_eq!(&f[..4], b"icns");
        assert_eq!(u32::from_be_bytes(f[4..8].try_into().unwrap()) as usize, f.len());
        let (mut at, mut found) = (8, Vec::new());
        while at < f.len() {
            let len = u32::from_be_bytes(f[at + 4..at + 8].try_into().unwrap()) as usize;
            let png = image::load_from_memory_with_format(&f[at + 8..at + len], image::ImageFormat::Png).unwrap();
            found.push((String::from_utf8_lossy(&f[at..at + 4]).into_owned(), png.width(), png.height()));
            at += len;
        }
        assert_eq!(at, f.len());
        let want = [("icp4", 16), ("ic11", 32), ("icp5", 32), ("ic12", 64), ("ic07", 128), ("ic13", 256), ("ic08", 256), ("ic14", 512), ("ic09", 512), ("ic10", 1024)];
        assert_eq!(found, want.iter().map(|(t, p)| (t.to_string(), *p, *p)).collect::<Vec<_>>());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// `img` scaled to `px` square pixels, as PNG bytes.
fn png_of(img: &image::DynamicImage, px: u32) -> Result<Vec<u8>, String> {
    let mut png = Vec::new();
    img.resize_exact(px, px, image::imageops::FilterType::Lanczos3)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png)
}

/// Writes a macOS icon file (.icns) made from `src` to `out`, on any host: the images iconutil makes from a full
/// .iconset, each a PNG under its type code (16 to 1024 pixels, the @2x sizes included), after the file's header.
pub fn icns(src: &Path, out: &Path) -> Result<(), String> {
    let bytes = std::fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("{}: {e}", src.display()))?;
    const TYPES: [(&[u8; 4], u32); 10] = [
        (b"icp4", 16),
        (b"ic11", 32),
        (b"icp5", 32),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic13", 256),
        (b"ic08", 256),
        (b"ic14", 512),
        (b"ic09", 512),
        (b"ic10", 1024),
    ];
    let mut body = Vec::new();
    for (kind, px) in TYPES {
        let png = png_of(&img, px)?;
        body.extend_from_slice(kind);
        body.extend_from_slice(&(png.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&png);
    }
    let mut file = b"icns".to_vec();
    file.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    file.extend_from_slice(&body);
    std::fs::write(out, file).map_err(|e| format!("{}: {e}", out.display()))
}
