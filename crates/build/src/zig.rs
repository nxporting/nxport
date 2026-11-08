// The C compiler the Windows app's lifted code is compiled with: zig cc, which players need not install (no game code
// ships, so every player compiles their own). A zig next to the builder (zig/zig.exe, a release that ships it) is used
// first; otherwise the pinned release is downloaded once into the toolchain directory, checked against its published
// size and SHA-256, and unpacked there.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The zig release lift::compile's flags are made for, and game profiles: LLVM reads a profile from its own format
/// version or older, and this zig holds LLVM 22.
pub const VERSION: &str = "0.17.1";
const NAME: &str = "zig-x86_64-windows-0.17.1";
const URL: &str = "https://ziglang.org/download/0.17.1/zig-x86_64-windows-0.17.1.zip";
/// From https://ziglang.org/download/index.json.
const SIZE: u64 = 100_343_666;
const SHA256: &str = "b03f477a1943673e02aca0f570e8030dffd536ce1f45cbc4d3326ec31159562e";

/// The C compiler command line (zig.exe cc) of the zig in `toolchains`, downloading it there first if needed;
/// `on_download(bytes, total)` follows the download.
pub fn cc(toolchains: &Path, on_download: &dyn Fn(u64, u64)) -> Result<Vec<String>, String> {
    let zig = find(toolchains).map_or_else(|| download(toolchains, on_download), Ok)?;
    Ok(vec![zig.display().to_string(), "cc".into()])
}

/// How large the builder's zig cache (`bounded_cache`) may grow before it is emptied. A game's build adds about 0.4 GB of
/// objects to it per set of flags (two sets with code mods); unchanged C is then compiled again from it in seconds, so an
/// engine update costs a lift and a link. Emptied, the next build compiles everything once (TotK: about 15 minutes).
pub const CACHE_CAP: u64 = 4 << 30;

/// The folder `dir` for zig's object cache (lift::compile sets it for each compiler it runs), emptied first when it holds
/// more than `cap` bytes. zig's own default (%LOCALAPPDATA%\zig) is never trimmed: it reached 27 GB on one PC. Emptied
/// whole: zig's manifests name objects by hash, and a cache missing some of them is not one to rely on. Returns whether
/// it was emptied.
pub fn bounded_cache(dir: &Path, cap: u64) -> std::io::Result<bool> {
    fn size(dir: &Path) -> u64 {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => size(&e.path()),
                Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
                _ => 0,
            })
            .sum()
    }
    let emptied = dir.is_dir() && size(dir) > cap;
    if emptied {
        std::fs::remove_dir_all(dir)?;
    }
    std::fs::create_dir_all(dir)?;
    Ok(emptied)
}

/// Copies the zig in `toolchains` into the app folder `app`, as `zig`: a plugin mod's code is compiled while the game runs
/// (engine live_patch.rs), on whichever PC that is. The builder has it by now: it compiled the game with it.
pub fn copy_into_app(toolchains: &Path, app: &Path) -> Result<(), String> {
    fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)?.flatten() {
            let (src, dst) = (e.path(), to.join(e.file_name()));
            if src.is_dir() {
                copy_tree(&src, &dst)?;
            } else {
                std::fs::copy(&src, &dst)?;
            }
        }
        Ok(())
    }
    let zig = find(toolchains).ok_or("no C compiler to put in the app")?;
    let from = zig.parent().ok_or("no folder")?;
    let to = app.join("zig");
    let _ = std::fs::remove_dir_all(&to);
    copy_tree(from, &to).map_err(|e| format!("{}: {e}", to.display()))
}

/// A zig shipped beside this program, else one downloaded before.
fn find(toolchains: &Path) -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.with_file_name("zig").join("zig.exe");
    [beside, toolchains.join(NAME).join("zig.exe")].into_iter().find(|p| p.is_file())
}

fn download(toolchains: &Path, on_download: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(toolchains).map_err(|e| format!("{}: {e}", toolchains.display()))?;
    let zip = toolchains.join(format!("{NAME}.zip.download"));
    println!("  Downloading zig {VERSION} from {URL}");
    // curl ships with Windows (System32) since Windows 10 1803.
    let mut child = crate::lift::command(crate::lift::CURL)
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(&zip)
        .arg(URL)
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        on_download(zip.metadata().map_or(0, |m| m.len()), SIZE);
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    if !status.success() {
        let _ = std::fs::remove_file(&zip);
        return Err(format!("downloading {URL} failed ({status})"));
    }
    let r = verify(&zip).and_then(|_| unpack(&zip, toolchains));
    let _ = std::fs::remove_file(&zip);
    r?;
    let zig = toolchains.join(NAME).join("zig.exe");
    zig.is_file().then_some(zig).ok_or_else(|| format!("{URL} holds no {NAME}/zig.exe"))
}

fn verify(zip: &Path) -> Result<(), String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let (mut hash, mut buf, mut len) = (sha2::Sha256::new(), vec![0u8; 1 << 20], 0u64);
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
        len += n as u64;
    }
    let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if len != SIZE || hex != SHA256 {
        return Err(format!("the zig download is not the published release ({len} bytes, SHA-256 {hex})"));
    }
    Ok(())
}

/// Unpacks the release into a scratch directory, then moves its top directory into place: an interrupted unpack
/// leaves nothing find() would take for a toolchain.
fn unpack(zip: &Path, toolchains: &Path) -> Result<(), String> {
    let scratch = toolchains.join(format!("{NAME}.unpack"));
    let _ = std::fs::remove_dir_all(&scratch);
    let file = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    archive.extract(&scratch).map_err(|e| format!("unpacking zig: {e}"))?;
    let dest = toolchains.join(NAME);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(scratch.join(NAME), &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let _ = std::fs::remove_dir_all(&scratch);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_is_emptied_only_past_its_cap() {
        let dir = std::env::temp_dir().join(format!("nxport-zig-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Made when missing.
        assert!(!bounded_cache(&dir, 100).unwrap());
        assert!(dir.is_dir());
        std::fs::create_dir_all(dir.join("o").join("ab")).unwrap();
        std::fs::write(dir.join("o").join("ab").join("x.o"), [0u8; 60]).unwrap();
        // Within the cap (sizes counted through subfolders): kept.
        assert!(!bounded_cache(&dir, 100).unwrap());
        assert!(dir.join("o").join("ab").join("x.o").is_file());
        std::fs::write(dir.join("h"), [0u8; 60]).unwrap();
        // Past it: emptied, and there again.
        assert!(bounded_cache(&dir, 100).unwrap());
        assert!(dir.is_dir() && std::fs::read_dir(&dir).unwrap().next().is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
