// Which game dump runs: --game-dir (or --dump, config.rs; an app bundle's own copy) names a dump directory with
// ExeFS/ (main, sdk, subsdk*) and RomFS/.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub fn dir() -> &'static Path {
    static D: OnceLock<PathBuf> = OnceLock::new();
    D.get_or_init(|| match &crate::settings::get().files.game_dir {
        Some(d) => d.clone(),
        None => {
            eprintln!("[-] No game dump: pass --dump <DIR> or set --game-dir to a directory with ExeFS/ and RomFS/.");
            std::process::exit(2);
        }
    })
}

pub fn exefs() -> PathBuf {
    dir().join("ExeFS")
}

pub fn exefs_file(name: &str) -> PathBuf {
    exefs().join(name)
}

pub fn romfs() -> &'static Path {
    static R: OnceLock<PathBuf> = OnceLock::new();
    R.get_or_init(|| dir().join("RomFS"))
}

/// The game's program (title) id from ExeFS/main.npdm (the ACI0 section's program id), if readable.
pub fn title_id() -> Option<u64> {
    static T: OnceLock<u64> = OnceLock::new();
    if let Some(&tid) = T.get() {
        return Some(tid);
    }
    let dir = crate::settings::try_get()?.files.game_dir.as_deref()?;
    if let Some(tid) = title_id_of(dir) {
        let _ = T.set(tid);
        Some(tid)
    } else {
        None
    }
}

/// The program id of the dump at `dir`.
pub fn title_id_of(dir: &Path) -> Option<u64> {
    let exefs = dir.join("ExeFS");
    let exefs = if exefs.is_dir() { exefs } else { dir.join("exefs") };
    npdm_program_id(&exefs.join("main.npdm"))
}

/// A game's control data (control.nacp): its title and display version.
pub struct Control {
    pub title: Option<String>,
    pub version: Option<String>,
}

/// The control data of the dump at `dir`: control.nacp beside ExeFS/ and RomFS/, or in Control/.
pub fn control_of(dir: &Path) -> Option<Control> {
    let d = ["control.nacp", "Control/control.nacp", "control/control.nacp"]
        .iter()
        .find_map(|p| std::fs::read(dir.join(p)).ok())?;
    let text = |at: usize, len: usize| {
        let b = d.get(at..at + len)?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
        let t = String::from_utf8_lossy(&b[..end]).trim().to_string();
        (!t.is_empty()).then_some(t)
    };
    // 16 language entries of 0x300 bytes (name 0x200, publisher 0x100; American English first), then the display
    // version at 0x3060.
    Some(Control {
        title: (0..16).find_map(|i| text(i * 0x300, 0x200)),
        version: text(0x3060, 0x10),
    })
}

/// Whether the dump at `dir` is a 32-bit (AArch32) program (main.npdm META flags bit 0 clear). Apple CPUs cannot run
/// 32-bit ARM code, so these games do not run.
pub fn is_32bit_of(dir: &Path) -> bool {
    let exefs = dir.join("ExeFS");
    let exefs = if exefs.is_dir() { exefs } else { dir.join("exefs") };
    std::fs::read(exefs.join("main.npdm"))
        .ok()
        .filter(|d| d.starts_with(b"META"))
        .and_then(|d| d.get(0xc).copied())
        .is_some_and(|flags| flags & 1 == 0)
}

/// The program id in an npdm file (its ACI0 section).
pub fn npdm_program_id(path: &Path) -> Option<u64> {
    let d = std::fs::read(path).ok()?;
    let i = d.windows(4).position(|w| w == b"ACI0")?;
    Some(u64::from_le_bytes(
        d.get(i + 0x10..i + 0x18)?.try_into().ok()?,
    ))
}

/// The default game (TotK 1.1.2): engine code with TotK-specific patches (frame pacer, ...) checks this; those patches
/// also verify the instructions they replace. Identified by its title id (any game without Kirby's RomFS/basil used to
/// count as TotK, so Super Mario Bros. Wonder got TotK's patches written past the end of its image).
pub fn is_totk() -> bool {
    crate::profile::is_totk()
}
