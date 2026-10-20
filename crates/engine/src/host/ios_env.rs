// iOS app setup, first thing in run: the app's folders (a platform-environment settings layer) and the choices made in
// the Settings app (a layer above the game profiles).
//   Documents/Game/{ExeFS,RomFS}   the game dump (copied in with Finder or the Files app), unless the app carries one
//   <bundle>/Game/{ExeFS,RomFS}    (nxport-cli ipa --bundle-game)
//   Documents/save                 saves
//   Documents/log.txt              stdout + stderr (the engine logs a lot; send this file when something breaks)
//   Documents/shader_dump_missing  shader stages the cache lacks (they draw nothing; send them back to add them)
//   <bundle>/shader_cache          the iOS shader cache (nxport-cli shaders build-ios)
//   <bundle>/Frameworks/NxportGuest.framework/NxportGuest   the signed guest code image (cpu/aot.rs)
//   <bundle>/mods                  mods built into the app (the image holds their code patches)
//   <bundle>/settings.toml         the settings the app was built with (a layer above the game profiles)

use std::path::PathBuf;

fn documents() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join("Documents")
}

fn bundle() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_default()
}

/// Redirects the log, and returns the app's folders and the Settings app's choices as settings layers.
/// The app's environment (a layer below the game's profile) and the settings above it: what the app was built with
/// (settings.toml in the bundle), then the Settings app's choices.
pub fn setup() -> (
    crate::settings::SettingsPatch,
    Vec<crate::settings::SettingsPatch>,
) {
    let docs = documents();
    let _ = std::fs::create_dir_all(&docs);
    // Log to a file (keeps the previous one), unbuffered enough: the engine prints with println!.
    let log = docs.join("log.txt");
    let _ = std::fs::rename(&log, docs.join("log.prev.txt"));
    if let Ok(f) = std::fs::File::create(&log) {
        use std::os::fd::AsRawFd;
        unsafe {
            libc::dup2(f.as_raw_fd(), 1);
            libc::dup2(f.as_raw_fd(), 2);
        }
        std::mem::forget(f);
    }
    let b = bundle();
    let mut env = crate::settings::SettingsPatch::default();
    // A game built into the app (nxport-cli ipa --bundle-game), else the one copied into Documents/Game.
    let game = if b.join("Game/ExeFS").is_dir() {
        b.join("Game")
    } else {
        docs.join("Game")
    };
    env.files.game_dir = Some(game.clone());
    env.files.save_dir = Some(docs.join("save"));
    env.files.shader_cache = Some(b.join("shader_cache"));
    env.files.miss_dump_dir = Some(docs.join("shader_dump_missing"));
    if b.join("mods").is_dir() {
        env.files.mods_dir = Some(b.join("mods"));
    } else {
        env.files.no_mods = Some(true);
    }
    // Texture backing (metal/tex_backing.rs) stays off for now: the first phone session with it crashed on an unclaimed
    // fault in the occlusion reader (2026-10-16), with 2.4 GB of memory still available.
    let image = b.join("Frameworks/NxportGuest.framework/NxportGuest");
    if image.is_file() {
        env.guest.aot_image = Some(image.clone());
    }
    let movie_shim = b.join("Frameworks/movie_shim.dylib");
    if movie_shim.is_file() {
        env.files.movie_shim = Some(movie_shim.clone());
    }
    println!(
        "[+] iOS: bundle {}, documents {}",
        b.display(),
        docs.display()
    );
    let (footprint, avail) = crate::metal::process_memory_stats();
    if avail > 0 {
        println!(
            "[+] iOS: initial footprint {} MB, available memory limit {:.2} GB",
            footprint / 1048576,
            avail as f64 / 1073741824.0
        );
    } else if footprint > 0 {
        println!("[+] iOS: initial footprint {} MB", footprint / 1048576);
    }
    if !game.join("ExeFS").is_dir() || !game.join("RomFS").is_dir() {
        println!(
            "[!] iOS: no game at {} (needs ExeFS/ and RomFS/ from your dump)",
            game.display()
        );
    }
    if !image.is_file() {
        println!(
            "[!] iOS: no signed guest code image at {}; guest code cannot run without JIT",
            image.display()
        );
    }
    if !movie_shim.is_file() {
        println!(
            "[!] iOS: no movie shim at {}; in-game movies will have no tracks",
            movie_shim.display()
        );
    }
    let built = b.join("settings.toml");
    let built = built
        .is_file()
        .then(|| crate::settings::load_preset(&built))
        .and_then(|r| r.map_err(|e| eprintln!("[!] {e}")).ok());
    (env, built.into_iter().chain([preferences()]).collect())
}

/// The Settings app's choices (registered defaults first: Default Mode handheld, on-screen controls on, no swaps, the
/// display aspect the app was built with).
fn preferences() -> crate::settings::SettingsPatch {
    let mut prefs = crate::settings::SettingsPatch::default();
    unsafe {
        let Some(cls_defaults) = objc2::runtime::AnyClass::get(c"NSUserDefaults") else {
            return prefs;
        };
        let defaults: *mut objc2::runtime::AnyObject =
            objc2::msg_send![cls_defaults, standardUserDefaults];
        if defaults.is_null() {
            return prefs;
        }

        let Some(cls_dict) = objc2::runtime::AnyClass::get(c"NSMutableDictionary") else {
            return prefs;
        };
        let dict: *mut objc2::runtime::AnyObject = objc2::msg_send![cls_dict, new];

        let k_mode = objc2_foundation::NSString::from_str("display_mode");
        let v_mode = objc2_foundation::NSString::from_str("handheld");
        let _: () = objc2::msg_send![dict, setObject: &*v_mode, forKey: &*k_mode];

        let k_ctrl = objc2_foundation::NSString::from_str("onscreen_controls");
        let Some(cls_num) = objc2::runtime::AnyClass::get(c"NSNumber") else {
            return prefs;
        };
        let num_yes: *mut objc2::runtime::AnyObject =
            objc2::msg_send![cls_num, numberWithBool: true];
        let _: () = objc2::msg_send![dict, setObject: num_yes, forKey: &*k_ctrl];

        let num_no: *mut objc2::runtime::AnyObject =
            objc2::msg_send![cls_num, numberWithBool: false];
        for k in ["swap_ab", "swap_xy"] {
            let k = objc2_foundation::NSString::from_str(k);
            let _: () = objc2::msg_send![dict, setObject: num_no, forKey: &*k];
        }

        // Display aspect (config::display_aspect): the app's built-in default comes from Info.plist NxportDisplayAspect
        // (the aspect the app was built with, for an aspect ratio mod), else the game's own aspect.
        let k_aspect = objc2_foundation::NSString::from_str("display_aspect");
        let built_aspect = objc2::runtime::AnyClass::get(c"NSBundle").and_then(|cls| {
            let main: *mut objc2::runtime::AnyObject = objc2::msg_send![cls, mainBundle];
            if main.is_null() {
                return None;
            }
            let key = objc2_foundation::NSString::from_str("NxportDisplayAspect");
            let v: *mut objc2::runtime::AnyObject =
                objc2::msg_send![main, objectForInfoDictionaryKey: &*key];
            ns_string(v)
        });
        let v_aspect =
            objc2_foundation::NSString::from_str(built_aspect.as_deref().unwrap_or("off"));
        let _: () = objc2::msg_send![dict, setObject: &*v_aspect, forKey: &*k_aspect];

        let _: () = objc2::msg_send![defaults, registerDefaults: dict];

        let val: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults, stringForKey: &*k_aspect];
        // "off": the game's own aspect.
        prefs.display.launch_aspect = ns_string(val).and_then(|a| a.parse().ok());
        let val: *mut objc2::runtime::AnyObject =
            objc2::msg_send![defaults, stringForKey: &*k_mode];
        prefs.system.handheld =
            Some(!ns_string(val).is_some_and(|m| m.eq_ignore_ascii_case("docked")));
    }
    prefs
}

/// The contents of an NSString (None for nil).
unsafe fn ns_string(v: *mut objc2::runtime::AnyObject) -> Option<String> {
    if v.is_null() {
        return None;
    }
    let utf8: *const libc::c_char = objc2::msg_send![v, UTF8String];
    (!utf8.is_null()).then(|| {
        std::ffi::CStr::from_ptr(utf8)
            .to_string_lossy()
            .into_owned()
    })
}
