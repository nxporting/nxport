// The C compiler the Windows app's lifted code is compiled with: zig cc, which players need not install (no game code
// ships, so every player compiles their own). A zig next to the builder (zig/zig.exe, a release that ships it) is used
// first; otherwise the pinned release is downloaded once into the toolchain directory, checked against its published
// size and SHA-256, and unpacked there.

use std::io::Read;
use std::path::{Path, PathBuf};

/// The zig release lift::compile's flags are made for (0.13 misreads -march=x86-64-v3, for example).
pub const VERSION: &str = "0.13.0";
const NAME: &str = "zig-windows-x86_64-0.13.0";
const URL: &str = "https://ziglang.org/download/0.13.0/zig-windows-x86_64-0.13.0.zip";
/// From https://ziglang.org/download/index.json.
const SIZE: u64 = 79_163_968;
const SHA256: &str = "d859994725ef9402381e557c60bb57497215682e355204d754ee3df75ee3c158";

/// The C compiler command line (zig.exe cc) of the zig in `toolchains`, downloading it there first if needed;
/// `on_download(bytes, total)` follows the download.
pub fn cc(toolchains: &Path, on_download: &dyn Fn(u64, u64)) -> Result<Vec<String>, String> {
    let zig = find(toolchains).map_or_else(|| download(toolchains, on_download), Ok)?;
    Ok(vec![zig.display().to_string(), "cc".into()])
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
    let mut child = crate::lift::command("curl.exe")
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
