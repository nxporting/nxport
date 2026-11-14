// Windows: asks the player for a folder before the game window exists (an app built without its game's files), with
// SDL's message box and folder dialog. The dialog answers through a callback that may come on another thread, so the
// answer arrives through a channel while this thread pumps SDL's events.

use std::path::{Path, PathBuf};

/// Shows `message` under `title` with "Choose Folder" and "Quit", then a folder dialog, until `holds` accepts the folder
/// chosen. `rejected` is shown instead of `message` after a folder that does not hold what is needed. None: the player
/// chose Quit or closed the message.
pub fn ask_folder(title: &str, message: &str, rejected: &str, holds: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    use sdl3::messagebox::{show_message_box, ButtonData, ClickedButton, MessageBoxButtonFlag, MessageBoxFlag};
    let sdl = sdl3::init().ok()?;
    let _video = sdl.video().ok()?;
    let mut pump = sdl.event_pump().ok()?;
    let mut text = message;
    loop {
        let buttons = [
            ButtonData { flags: MessageBoxButtonFlag::RETURNKEY_DEFAULT, button_id: 1, text: "Choose Folder" },
            ButtonData { flags: MessageBoxButtonFlag::ESCAPEKEY_DEFAULT, button_id: 0, text: "Quit" },
        ];
        match show_message_box(MessageBoxFlag::INFORMATION, &buttons, title, text, None, None) {
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
        match picked {
            Some(p) if holds(&p) => return Some(p),
            // A folder without the game: say so and ask again. Cancelled: back to the message.
            Some(_) => text = rejected,
            None => text = message,
        }
    }
}
