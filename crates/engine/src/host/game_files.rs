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

/// Asks for the folder with the game's files until one holds them (`title`'s if given); None when the player quits.
/// Windows: SDL's message box, then its folder dialog (before the game window exists). The dialog answers through a
/// callback that may come on another thread, so the answer arrives through a channel while this thread pumps events.
#[cfg(windows)]
pub fn ask(title: Option<u64>, name: &str) -> Option<PathBuf> {
    use sdl3::messagebox::{show_message_box, ButtonData, ClickedButton, MessageBoxButtonFlag, MessageBoxFlag};
    let sdl = sdl3::init().ok()?;
    let _video = sdl.video().ok()?;
    let mut pump = sdl.event_pump().ok()?;
    let game = if name.is_empty() { "the game".to_string() } else { name.to_string() };
    let (mut heading, mut text) = (
        "Game files needed".to_string(),
        format!("This app was built without {game}'s files. Choose the folder that holds them (the one with the ExeFS and RomFS folders). It is remembered for the next launch."),
    );
    loop {
        let buttons = [
            ButtonData { flags: MessageBoxButtonFlag::RETURNKEY_DEFAULT, button_id: 1, text: "Choose Folder…" },
            ButtonData { flags: MessageBoxButtonFlag::ESCAPEKEY_DEFAULT, button_id: 0, text: "Quit" },
        ];
        match show_message_box(MessageBoxFlag::INFORMATION, &buttons, &heading, &text, None, None) {
            Ok(ClickedButton::CustomButton(b)) if b.button_id == 1 => {}
            _ => return None,
        }
        let (tx, rx) = std::sync::mpsc::channel();
        sdl3::dialog::show_open_folder_dialog(
            None::<&Path>,
            false,
            None,
            Box::new(move |res, _| {
                let _ = tx.send(res.ok().and_then(|mut v| v.pop()));
            }),
        );
        let picked = loop {
            pump.pump_events();
            match rx.try_recv() {
                Ok(p) => break p,
                Err(std::sync::mpsc::TryRecvError::Empty) => std::thread::sleep(std::time::Duration::from_millis(10)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break None,
            }
        };
        // Cancelled: back to the first message.
        let Some(dir) = picked else { continue };
        if holds(&dir, title) {
            return Some(dir);
        }
        heading = "Not this game's files".to_string();
        text = match crate::game::title_id_of(&dir) {
            Some(t) => format!("{} holds another game ({t:016x}).", dir.display()),
            None => format!("{} has no ExeFS/main.npdm.", dir.display()),
        };
    }
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn ask(_title: Option<u64>, _name: &str) -> Option<PathBuf> {
    None
}
