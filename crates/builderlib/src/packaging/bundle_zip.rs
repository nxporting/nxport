// An Apple bundle in a zip that keeps its files' Unix modes: the .ipa (Payload/), and the macOS app when it is packed on a
// host whose file system has no modes (Windows), where the zip is what carries the executable bits to the Mac.

use std::fs;
use std::path::Path;

/// A file's mode in the zip. Hosts without modes: Mach-O files are executable.
fn mode(p: &Path) -> std::io::Result<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).map(|m| m.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let mut magic = [0u8; 4];
        let is_macho = std::io::Read::read_exact(&mut fs::File::open(p)?, &mut magic).is_ok() && u32::from_le_bytes(magic) == 0xfeed_facf;
        Ok(if is_macho { 0o755 } else { 0o644 })
    }
}

/// Writes `root` (a folder, kept as the zip's top folder) into the zip `out`. `stored(path in the zip)`: files kept
/// uncompressed (game files: compressing them gains nothing and takes long).
pub fn write(root: &Path, out: &Path, stored: &dyn Fn(&str) -> bool) -> Result<(), String> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    let err = |e: &dyn std::fmt::Display| format!("{}: {e}", out.display());
    let _ = fs::remove_file(out);
    let file = fs::File::create(out).map_err(|e| err(&e))?;
    let mut z = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let base = root.parent().ok_or_else(|| format!("{}: no parent folder", root.display()))?;
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rel = dir.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
        z.add_directory(format!("{rel}/"), SimpleFileOptions::default().system(zip::System::Unix)).map_err(|e| err(&e))?;
        let mut entries: Vec<_> = fs::read_dir(&dir).map_err(|e| err(&e))?.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/");
            // Made on Unix whatever the host: entries marked as made on DOS (zip's default on Windows) have their Unix
            // modes ignored by the readers, and the executables lose their bit.
            let opts = SimpleFileOptions::default()
                .system(zip::System::Unix)
                .compression_method(if stored(&rel) { zip::CompressionMethod::Stored } else { zip::CompressionMethod::Deflated })
                .unix_permissions(mode(&p).map_err(|e| err(&e))? & 0o777)
                .large_file(fs::metadata(&p).map(|m| m.len() >= u32::MAX as u64).unwrap_or(false));
            z.start_file(rel, opts).map_err(|e| err(&e))?;
            let mut f = fs::File::open(&p).map_err(|e| err(&e))?;
            std::io::copy(&mut f, &mut z).map_err(|e| err(&e))?;
        }
    }
    z.finish().map_err(|e| err(&e))?.flush().map_err(|e| err(&e))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn keeps_folders_and_marks_executables() {
        let dir = std::env::temp_dir().join(format!("nxport-bundle-zip-{}", std::process::id()));
        let app = dir.join("Game.app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        std::fs::create_dir_all(app.join("Contents/Resources/Dump")).unwrap();
        std::fs::write(app.join("Contents/MacOS/nxport"), [0xcf, 0xfa, 0xed, 0xfe, 0, 0, 0, 0]).unwrap();
        std::fs::write(app.join("Contents/Info.plist"), "<plist/>").unwrap();
        std::fs::write(app.join("Contents/Resources/Dump/main"), [7u8; 64]).unwrap();
        let out = dir.join("Game.app.zip");
        super::write(&app, &out, &|rel| rel.contains("/Dump/")).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut seen = Vec::new();
        for i in 0..z.len() {
            let f = z.by_index(i).unwrap();
            if !f.is_dir() {
                seen.push((f.name().unwrap().to_string(), f.unix_mode().map(|m| m & 0o777), f.compression()));
            }
        }
        seen.sort_by(|a, b| a.0.cmp(&b.0));
        let (stored, deflated) = (zip::CompressionMethod::Stored, zip::CompressionMethod::Deflated);
        let want = [
            ("Game.app/Contents/Info.plist", 0o644, deflated),
            ("Game.app/Contents/MacOS/nxport", 0o755, deflated),
            ("Game.app/Contents/Resources/Dump/main", 0o644, stored),
        ];
        // On a Unix host the files' own modes (0644 as written there).
        if cfg!(not(unix)) {
            assert_eq!(seen, want.iter().map(|(n, m, c)| (n.to_string(), Some(*m), *c)).collect::<Vec<_>>());
        }
        assert!(z.by_name("Game.app/Contents/").is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
