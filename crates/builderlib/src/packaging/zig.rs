// The C compiler the Windows app's lifted code is compiled with: zig cc, which players need not install (no game code
// ships, so every player compiles their own). A zig next to the builder (zig/, a release that ships it) is used first;
// otherwise the pinned release for this host is downloaded once into the toolchain directory, checked against its
// published size and SHA-256, and unpacked there. On a Mac it compiles for Windows (windows_cc); a Windows app that
// carries its compiler (plugin mods) gets the Windows release whatever the host.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The zig release lift::compile's flags are made for, and game profiles: LLVM reads a profile from its own format
/// version or older, and this zig holds LLVM 22.
pub const VERSION: &str = "0.17.1";

/// One host's build of the release, as https://ziglang.org/download/index.json lists it.
struct Release {
    /// Its top directory, and the name it is kept under in the toolchain directory.
    name: &'static str,
    url: &'static str,
    size: u64,
    sha256: &'static str,
    /// The program within `name`.
    exe: &'static str,
}

const WINDOWS: Release = Release {
    name: "zig-x86_64-windows-0.17.1",
    url: "https://ziglang.org/download/0.17.1/zig-x86_64-windows-0.17.1.zip",
    size: 100_343_666,
    sha256: "b03f477a1943673e02aca0f570e8030dffd536ce1f45cbc4d3326ec31159562e",
    exe: "zig.exe",
};

#[cfg_attr(windows, allow(dead_code))]
const MACOS: Release = Release {
    name: "zig-aarch64-macos-0.17.1",
    url: "https://ziglang.org/download/0.17.1/zig-aarch64-macos-0.17.1.tar.xz",
    size: 54_001_240,
    sha256: "75f1bc7a1e9852f17ec9a0c1895f767c4ec56d46bc8a834920eece1d0359dd8d",
    exe: "zig",
};

/// The release that runs here.
const HOST: &Release = if cfg!(windows) { &WINDOWS } else { &MACOS };

/// The C compiler command line (zig cc) of the zig in `toolchains`, downloading it there first if needed;
/// `on_download(bytes, total)` follows the download. It compiles for this host; windows_cc for the Windows app.
pub fn cc(toolchains: &Path, on_download: &dyn Fn(u64, u64)) -> Result<Vec<String>, String> {
    let zig = find(HOST, toolchains).map_or_else(|| download(HOST, toolchains, on_download), Ok)?;
    Ok(vec![zig.display().to_string(), "cc".into()])
}

/// cc, compiling for the Windows app's lifted code. On Windows that is the host, and the target is left to zig as
/// before (an explicit one would change every object, and zig's cache of them).
pub fn windows_cc(toolchains: &Path, on_download: &dyn Fn(u64, u64)) -> Result<Vec<String>, String> {
    let mut cc = cc(toolchains, on_download)?;
    if !cfg!(windows) {
        cc.extend(["-target".to_string(), "x86_64-windows-gnu".to_string()]);
    }
    Ok(cc)
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

/// Copies the Windows zig into the Windows app folder `app`, as `zig`: a plugin mod's code is compiled while the game runs
/// (engine live_patch.rs), on whichever PC that is. On Windows the builder has it by now (it compiled the game with it);
/// on a Mac it is downloaded for the app.
pub fn copy_into_app(toolchains: &Path, app: &Path, on_download: &dyn Fn(u64, u64)) -> Result<(), String> {
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
    let zig = find(&WINDOWS, toolchains).map_or_else(|| download(&WINDOWS, toolchains, on_download), Ok)?;
    let from = zig.parent().ok_or("no folder")?;
    let to = app.join("zig");
    let _ = std::fs::remove_dir_all(&to);
    copy_tree(from, &to).map_err(|e| format!("{}: {e}", to.display()))
}

/// `release` shipped beside this program (the host's only), else one downloaded before.
fn find(release: &Release, toolchains: &Path) -> Option<PathBuf> {
    let beside = std::env::current_exe().ok()?.with_file_name("zig").join(release.exe);
    let beside = std::ptr::eq(release, HOST).then_some(beside);
    beside.into_iter().chain([toolchains.join(release.name).join(release.exe)]).find(|p| p.is_file())
}

fn download(release: &Release, toolchains: &Path, on_download: &dyn Fn(u64, u64)) -> Result<PathBuf, String> {
    std::fs::create_dir_all(toolchains).map_err(|e| format!("{}: {e}", toolchains.display()))?;
    let (name, url) = (release.name, release.url);
    let archive = toolchains.join(format!("{name}.download"));
    println!("  Downloading zig {VERSION} from {url}");
    let mut child = crate::lift::command(crate::lift::CURL)
        .args(["-fsSL", "--retry", "3", "-o"])
        .arg(&archive)
        .arg(url)
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        on_download(archive.metadata().map_or(0, |m| m.len()), release.size);
        std::thread::sleep(std::time::Duration::from_millis(200));
    };
    if !status.success() {
        let _ = std::fs::remove_file(&archive);
        return Err(format!("downloading {url} failed ({status})"));
    }
    let r = verify(release, &archive).and_then(|_| unpack(release, &archive, toolchains));
    let _ = std::fs::remove_file(&archive);
    r?;
    let zig = toolchains.join(name).join(release.exe);
    zig.is_file().then_some(zig).ok_or_else(|| format!("{url} holds no {name}/{}", release.exe))
}

fn verify(release: &Release, archive: &Path) -> Result<(), String> {
    use sha2::Digest;
    let mut f = std::fs::File::open(archive).map_err(|e| e.to_string())?;
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
    if len != release.size || hex != release.sha256 {
        return Err(format!("the zig download is not the published release ({len} bytes, SHA-256 {hex})"));
    }
    Ok(())
}

/// Unpacks the release into a scratch directory, then moves its top directory into place: an interrupted unpack
/// leaves nothing find() would take for a toolchain. A .zip by the zip crate; a .tar.xz by tar (macOS's reads xz).
fn unpack(release: &Release, archive: &Path, toolchains: &Path) -> Result<(), String> {
    let name = release.name;
    let scratch = toolchains.join(format!("{name}.unpack"));
    let _ = std::fs::remove_dir_all(&scratch);
    if release.url.ends_with(".zip") {
        let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", archive.display()))?;
        zip.extract(&scratch).map_err(|e| format!("unpacking zig: {e}"))?;
    } else {
        std::fs::create_dir_all(&scratch).map_err(|e| format!("{}: {e}", scratch.display()))?;
        let o = crate::lift::command("tar")
            .arg("-xJf")
            .arg(archive)
            .arg("-C")
            .arg(&scratch)
            .output()
            .map_err(|e| format!("tar: {e}"))?;
        if !o.status.success() {
            return Err(format!("unpacking zig: {}", String::from_utf8_lossy(&o.stderr)));
        }
    }
    let dest = toolchains.join(name);
    let _ = std::fs::remove_dir_all(&dest);
    std::fs::rename(scratch.join(name), &dest).map_err(|e| format!("{}: {e}", dest.display()))?;
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
