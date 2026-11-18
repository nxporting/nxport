// nn::pl as the game calls it: shared fonts and VR mode.


pub(crate) unsafe extern "C" fn request_shared_font_load(ty: i32) {
    shared_font(ty);
}

/// GetSharedFont(int* count, SharedFontData[] {type, address, size}, int capacity, LanguageCode): every font in type
/// order whatever the language (as Ryujinx); true when all are loaded.
pub(crate) unsafe extern "C" fn get_shared_font(out_count: *mut i32, out: *mut u8, capacity: i32, _language: u64) -> bool {
    let n = capacity.clamp(0, 6) as usize;
    let mut all = true;
    for ty in 0..n {
        let font = shared_font(ty as i32);
        let (addr, len) = if font.is_empty() {
            (DUMMY_SHARED_FONT.as_ptr(), DUMMY_SHARED_FONT.len())
        } else {
            (font.as_ptr(), font.len())
        };
        all &= len > 0;
        if !out.is_null() {
            let e = out.add(ty * 24);
            (e as *mut u32).write_unaligned(ty as u32);
            (e.add(8) as *mut usize).write_unaligned(addr as usize);
            (e.add(16) as *mut usize).write_unaligned(len);
        }
    }
    if !out_count.is_null() {
        *out_count = 6;
    }
    all
}

pub(crate) unsafe extern "C" fn get_shared_font_load_state(_ty: i32) -> i32 {
    1
}

pub(crate) unsafe extern "C" fn get_shared_font_size(ty: i32) -> usize {
    let len = shared_font(ty).len();
    if len == 0 {
        DUMMY_SHARED_FONT.len()
    } else {
        len
    }
}

pub(crate) unsafe extern "C" fn get_shared_font_address(ty: i32) -> *const u8 {
    let font = shared_font(ty);
    if font.is_empty() {
        DUMMY_SHARED_FONT.as_ptr()
    } else {
        font.as_ptr()
    }
}

pub(crate) unsafe extern "C" fn begin_vr_mode() {
    VR_MODE.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) unsafe extern "C" fn end_vr_mode() {
    VR_MODE.store(false, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) unsafe extern "C" fn is_vr_mode() -> bool {
    VR_MODE.load(std::sync::atomic::Ordering::Relaxed)
}

/// nn::pl::IsRunningOnOunce: whether the console is a Switch 2. The engine presents the original Switch.
pub(crate) unsafe extern "C" fn is_running_on_ounce() -> bool {
    false
}
// ── nn::pl shared fonts ──────────────────────────────────────────────────────────────────────
// The system's shared fonts (Super Mario Odyssey renders its text with them; TotK ships its own). They are read from
// --shared-fonts (a .app's Resources/fonts), named by type: FontStandard.ttf, FontChineseSimplified.ttf,
// FontExtendedChineseSimplified.ttf, FontChineseTraditional.ttf, FontKorean.ttf, FontNintendoExtended.ttf. A missing
// font falls back to FontStandard.ttf, then to a host font with Latin glyphs.
pub(crate) const SHARED_FONT_FILES: [&str; 6] = [
    "FontStandard.ttf",
    "FontChineseSimplified.ttf",
    "FontExtendedChineseSimplified.ttf",
    "FontChineseTraditional.ttf",
    "FontKorean.ttf",
    "FontNintendoExtended.ttf",
];

pub(crate) const HOST_FONT_CANDIDATES: &[&str] = &[
    // macOS font paths
    "/System/Library/Fonts/Supplemental/Arial.ttf",
    "/System/Library/Fonts/Arial.ttf",
    "/Library/Fonts/Arial.ttf",
    "/System/Library/Fonts/Supplemental/Helvetica.ttc",
    "/System/Library/Fonts/Helvetica.ttc",
    // iOS font paths
    "/System/Library/Fonts/Core/Arial.ttf",
    "/System/Library/Fonts/CoreAddition/Arial.ttf",
    "/System/Library/Fonts/Core/Helvetica.ttc",
    "/System/Library/Fonts/CoreAddition/Helvetica.ttc",
    "/System/Library/Fonts/Core/HelveticaNeue.ttc",
    "/System/Library/Fonts/CoreAddition/HelveticaNeue.ttc",
    "/System/Library/Fonts/Core/TimesNewRoman.ttf",
    "/System/Library/Fonts/CoreAddition/TimesNewRoman.ttf",
    "/System/Library/Fonts/Core/CourierNew.ttf",
    "/System/Library/Fonts/CoreAddition/CourierNew.ttf",
    "/System/Library/Fonts/Core/PingFang.ttc",
    "/System/Library/Fonts/CoreAddition/PingFang.ttc",
    "/System/Library/Fonts/Core/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/CoreAddition/AppleSDGothicNeo.ttc",
];

pub(crate) static DUMMY_SHARED_FONT: &[u8] = include_bytes!("dummy_font.ttf");

pub(crate) static SHARED_FONTS: [std::sync::OnceLock<Vec<u8>>; 6] = [const { std::sync::OnceLock::new() }; 6];

pub(crate) fn load_shared_font(ty: usize) -> Vec<u8> {
    let dir = crate::settings::get().files.shared_fonts.clone();
    let mut font_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(d) = &dir {
        font_dirs.push(d.clone());
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            font_dirs.push(parent.join("fonts"));
            font_dirs.push(parent.join("Resources/fonts"));
            if let Some(grandparent) = parent.parent() {
                font_dirs.push(grandparent.join("Resources/fonts"));
                font_dirs.push(grandparent.join("fonts"));
            }
        }
    }
    if let Some(game_dir) = &crate::settings::get().files.game_dir {
        font_dirs.push(game_dir.join("fonts"));
        font_dirs.push(game_dir.join("RomFS/fonts"));
    }
    if let Some(app_data) = crate::profile::data_dir() {
        if let Some(nxport_dir) = app_data.parent().and_then(|p| p.parent()) {
            font_dirs.push(nxport_dir.join("fonts"));
            font_dirs.push(nxport_dir.join("system_data/0100000000000811"));
        }
    }
    if let Some(sys_font_dir) = crate::horizon::fs::system_data_dir(0x0100000000000811) {
        font_dirs.push(sys_font_dir);
    }
    if let Some(home) = std::env::var_os("HOME") {
        let p = std::path::PathBuf::from(home);
        font_dirs.push(p.join("Library/Application Support/nxport/fonts"));
        font_dirs.push(p.join("Documents/fonts"));
        font_dirs.push(p.join("Documents/Game/fonts"));
    }
    font_dirs.push(std::path::PathBuf::from("fonts"));

    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    for d in &font_dirs {
        paths.push(d.join(SHARED_FONT_FILES[ty]));
        paths.push(d.join(SHARED_FONT_FILES[0]));
    }
    for candidate in HOST_FONT_CANDIDATES {
        paths.push((*candidate).into());
    }
    #[cfg(windows)]
    {
        let windows = std::env::var_os("WINDIR").map_or_else(|| "C:\\Windows".into(), std::path::PathBuf::from);
        for name in ["arial.ttf", "segoeui.ttf", "tahoma.ttf", "verdana.ttf"] {
            paths.push(windows.join("Fonts").join(name));
        }
    }

    for p in &paths {
        if let Ok(bytes) = std::fs::read(p) {
            println!("[+] nn::pl: shared font {} from {} ({} bytes)", ty, p.display(), bytes.len());
            return bytes;
        }
    }
    eprintln!("[!] nn::pl: no shared font {} (--shared-fonts), falling back to embedded dummy font", SHARED_FONT_FILES[ty]);
    DUMMY_SHARED_FONT.to_vec()
}

pub(crate) fn shared_font(ty: i32) -> &'static [u8] {
    let ty = if (0..6).contains(&ty) { ty as usize } else { 0 };
    SHARED_FONTS[ty].get_or_init(|| load_shared_font(ty))
}

/// VR mode as the game set it (nn::pl::BeginVrMode / EndVrMode): the system only records it here; the game draws its own
/// split image.
pub(crate) static VR_MODE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
