// An app built without the game's files, on a machine where the folder it names is missing (built on another computer,
// or the folder moved): it asks for them before anything loads, and remembers the answer for the game.

use std::path::{Path, PathBuf};

/// Whether `dir` holds a game's files, and if `title` is given, that game's.
pub fn holds(dir: &Path, title: Option<u64>) -> bool {
    match crate::game::title_id_of(dir) {
        Some(t) => title.is_none_or(|want| want == t),
        None => false,
    }
}

/// Asks for the folder with the game's files until one holds them (`title`'s if given); None when the player quits.
#[cfg(target_os = "macos")]
pub fn ask(title: Option<u64>, name: &str) -> Option<PathBuf> {
    use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSApplication, NSApplicationActivationPolicy, NSModalResponseOK, NSOpenPanel};
    use objc2_foundation::{MainThreadMarker, NSString};
    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    let alert = |text: &str, info: &str, buttons: &[&str]| {
        let a = NSAlert::new(mtm);
        a.setMessageText(&NSString::from_str(text));
        a.setInformativeText(&NSString::from_str(info));
        for b in buttons {
            a.addButtonWithTitle(&NSString::from_str(b));
        }
        a.runModal() == NSAlertFirstButtonReturn
    };
    let game = if name.is_empty() { "the game".to_string() } else { name.to_string() };
    if !alert(
        "Game files needed",
        &format!("This app was built without {game}'s files. Choose the folder that holds them (the one with the ExeFS and RomFS folders). It is remembered for the next launch."),
        &["Choose Folder…", "Quit"],
    ) {
        return None;
    }
    loop {
        let panel = NSOpenPanel::openPanel(mtm);
        panel.setCanChooseDirectories(true);
        panel.setCanChooseFiles(false);
        panel.setAllowsMultipleSelection(false);
        panel.setMessage(Some(&NSString::from_str(&format!("The folder with {game}'s ExeFS and RomFS"))));
        if panel.runModal() != NSModalResponseOK {
            return None;
        }
        let dir = panel.URL().and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()))?;
        if holds(&dir, title) {
            return Some(dir);
        }
        let why = match crate::game::title_id_of(&dir) {
            Some(t) => format!("{} holds another game ({t:016x}).", dir.display()),
            None => format!("{} has no ExeFS/main.npdm.", dir.display()),
        };
        if !alert("Not this game's files", &why, &["Choose Again…", "Quit"]) {
            return None;
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn ask(_title: Option<u64>, _name: &str) -> Option<PathBuf> {
    None
}
