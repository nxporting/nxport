// Atmosphère cheats: each enabled mod's `cheats/<build id>.txt`, named for the first 8 bytes of main's build id in hex
// (dmnt writes the name in lower case, mods ship it in upper case, and the SD card's file system ignores case, so the
// case does not matter here either). Atmosphère reads one such file per game (atmosphere/contents/<title id>/cheats);
// here every mod may carry one, and each file runs as dmnt would run it alone: its own VM and static registers, the
// master cheat first, then its enabled cheats (vm.rs). Files run in mod priority order.
//
// As dmnt: every cheat of a file starts enabled (atmosphere!dmnt_cheats_enabled_by_default, true by default); the
// file's cheats/toggles.txt, if it has one, then turns cheats on or off, and the menu's changes are written back to
// it (dmnt saves the toggles when the game closes, and only when it loaded them); a file that does not parse, or
// whose toggles.txt does not, runs none of its cheats; a thread runs the enabled cheats twelve times a second
// (CheatProcessManager::VirtualMachineThread: run, then sleep 1/12 s), and it stops running a file whose master and
// enabled cheats hold more than 0x400 words until fewer are enabled.

pub mod parse;
mod process;
pub mod vm;

use parse::{Cheat, CheatFile};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// One mod's cheat file.
pub struct Loaded {
    pub mod_name: String,
    file_name: String,
    master: Option<Cheat>,
    pub cheats: Vec<Cheat>,
    enabled: Vec<AtomicBool>,
    /// The toggles.txt the file's toggles were read from, written again when they change.
    #[cfg_attr(target_os = "ios", allow(dead_code))] // the desktop menus
    toggles: Option<PathBuf>,
    vm: Mutex<vm::Vm>,
}

impl Loaded {
    /// The program dmnt's LoadProgram would build: the master, then the enabled cheats.
    fn program(&self) -> Vec<u32> {
        let enabled = self.cheats.iter().zip(&self.enabled).filter(|(_, on)| on.load(Ordering::Relaxed)).map(|(c, _)| c);
        self.master.iter().chain(enabled).flat_map(|c| c.opcodes.iter().copied()).collect()
    }

    #[cfg_attr(target_os = "ios", allow(dead_code))] // the desktop menus
    fn save_toggles(&self) {
        let Some(path) = &self.toggles else { return };
        let on: Vec<bool> = self.enabled.iter().map(|e| e.load(Ordering::Relaxed)).collect();
        if let Err(e) = std::fs::write(path, parse::format_toggles(&self.cheats, &on)) {
            println!("[!] Cheats: '{}' {}: {e}", self.mod_name, path.display());
        }
    }
}

/// The 16 hex digits cheat files are named by: main's build id's first 8 bytes.
fn cheat_id() -> Option<String> {
    let id = match crate::profile::main_build_id() {
        Some(id) => id.to_string(),
        None => crate::guest::nso::nso_build_id(&crate::game::exefs().join("main").display().to_string())?,
    };
    id.get(..16).map(str::to_ascii_uppercase)
}

/// The enabled mods' cheat files for this build of the game, read once.
pub fn files() -> &'static [Loaded] {
    static FILES: OnceLock<Vec<Loaded>> = OnceLock::new();
    FILES.get_or_init(|| {
        let Some(id) = cheat_id() else { return Vec::new() };
        crate::horizon::fs::mods()
            .iter()
            .filter_map(|(mod_name, dir)| {
                let cheats = crate::horizon::fs::mod_subdir(dir, "cheats");
                let mut entries: Vec<PathBuf> = std::fs::read_dir(&cheats).ok()?.flatten().map(|e| e.path()).collect();
                entries.sort();
                let stem = |p: &PathBuf| p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let is_txt = |p: &PathBuf| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt"));
                let is_id = |s: &str| s.len() == 16 && s.chars().all(|c| c.is_ascii_hexdigit());
                for other in entries.iter().filter(|p| is_txt(p) && is_id(&stem(p)) && !stem(p).eq_ignore_ascii_case(&id)) {
                    println!(
                        "[!] Cheats: '{mod_name}' cheats/{} is for another build of the game (this one's is {id}): not loaded",
                        other.file_name().unwrap_or_default().to_string_lossy()
                    );
                }
                let path = entries.into_iter().find(|p| is_txt(p) && stem(p).eq_ignore_ascii_case(&id))?;
                load(mod_name, &cheats, &path)
            })
            .collect()
    })
}

fn load(mod_name: &str, dir: &std::path::Path, path: &std::path::Path) -> Option<Loaded> {
    let file_name = format!("cheats/{}", path.file_name().unwrap_or_default().to_string_lossy());
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            println!("[!] Cheats: '{mod_name}' {file_name}: {e}");
            return None;
        }
    };
    let CheatFile { master, cheats } = match parse::parse_cheats(&data) {
        Ok(f) => f,
        Err(e) => {
            println!("[!] Cheats: '{mod_name}' {file_name}, {e}: none of its cheats are loaded (as Atmosphère)");
            return None;
        }
    };
    let mut on = vec![true; cheats.len()];
    let toggles = std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        p.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case("toggles.txt"))
    });
    if let Some(t) = &toggles {
        let read = std::fs::read(t).map_err(|e| e.to_string());
        if let Err(e) = read.and_then(|d| parse::parse_toggles(&d, &cheats, &mut on).map_err(|e| e.to_string())) {
            println!("[!] Cheats: '{mod_name}' cheats/toggles.txt, {e}: none of {file_name}'s cheats run (as Atmosphère)");
            return None;
        }
    }
    let names: Vec<String> =
        cheats.iter().zip(&on).map(|(c, &e)| if e { c.name.clone() } else { format!("{} (off)", c.name) }).collect();
    println!(
        "[+] Cheats: '{mod_name}' {file_name}: {} cheats{}{}: {}",
        cheats.len(),
        master.as_ref().map_or(String::new(), |m| format!(", master '{}'", m.name)),
        if toggles.is_some() { ", toggles.txt" } else { "" },
        names.join(", ")
    );
    Some(Loaded {
        mod_name: mod_name.to_string(),
        file_name,
        master,
        enabled: on.into_iter().map(AtomicBool::new).collect(),
        cheats,
        toggles,
        vm: Mutex::new(vm::Vm::default()),
    })
}

/// Every cheat listed (not the masters, which cannot be turned off): (file index, cheat index), in menu order.
#[cfg_attr(target_os = "ios", allow(dead_code))] // the desktop menus
pub fn listed() -> Vec<(usize, usize)> {
    files().iter().enumerate().flat_map(|(f, l)| (0..l.cheats.len()).map(move |c| (f, c))).collect()
}

#[cfg_attr(target_os = "ios", allow(dead_code))] // the desktop menus
pub fn is_enabled(file: usize, cheat: usize) -> bool {
    files()[file].enabled[cheat].load(Ordering::Relaxed)
}

/// Turns a cheat on or off. What it wrote stays written, as on a Switch.
#[cfg_attr(target_os = "ios", allow(dead_code))] // the desktop menus
pub fn set_enabled(file: usize, cheat: usize, on: bool) {
    let f = &files()[file];
    f.enabled[cheat].store(on, Ordering::Relaxed);
    println!("[+] Cheats: '{}' {}: {}", f.mod_name, f.cheats[cheat].name, if on { "on" } else { "off" });
    f.save_toggles();
}

/// Starts running the cheats, if there are any, against main's image at `main` (`main_size` bytes, the first
/// `text_size` of them code). Before guest code runs: dmnt attaches before the game starts.
pub fn start(main: usize, main_size: usize, text_size: usize) {
    if files().is_empty() {
        return;
    }
    let spawned = std::thread::Builder::new().name("cheats".into()).spawn(move || {
        let mut process = process::GuestProcess::new(main, main_size, text_size);
        let mut too_long = vec![false; files().len()];
        loop {
            // hid::GetKeysHeld: the buttons of every controller, handheld included.
            let keys = (0..crate::host::input::MAX_PLAYERS).fold(0, |k, p| k | crate::host::input::player_npad_state(p).0);
            for (f, long) in files().iter().zip(&mut too_long) {
                let program = f.program();
                if program.len() > vm::MAX_PROGRAM {
                    if !*long {
                        println!(
                            "[!] Cheats: '{}' {}: the enabled cheats hold {} words, more than {}: none run until fewer are on",
                            f.mod_name,
                            f.file_name,
                            program.len(),
                            vm::MAX_PROGRAM
                        );
                    }
                    *long = true;
                    continue;
                }
                *long = false;
                if program.is_empty() {
                    continue;
                }
                if let Err(vm::Abort(why)) = f.vm.lock().unwrap().execute(&program, &mut process, keys) {
                    process.say_once(format!("[!] Cheats: '{}' {}: {why} (Atmosphère's dmnt aborts there): the run ends", f.mod_name, f.file_name));
                }
            }
            std::thread::sleep(std::time::Duration::from_nanos(1_000_000_000 / 12));
        }
    });
    if let Err(e) = spawned {
        println!("[!] Cheats: no thread to run them: {e}");
    }
}
