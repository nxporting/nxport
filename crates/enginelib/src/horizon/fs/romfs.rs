// Resolving a game path to a file: RomFS images and directories with the mods layered over them, merged listings.

use super::*;

/// What a RomFS path finds in a romfs.bin.
pub(crate) enum ImageEntry<'a> {
    Dir,
    /// The image, where the file lies in it, its size, and whether the path found its `.zs` (OpenFile's fallback).
    File(&'a RomfsImage, u64, u64, bool),
}

/// `rel` in the first of `layers` that has it, if that is an image. Otherwise the host paths below apply.
pub(crate) fn image_entry_in<'a>(layers: &'a [RomfsLayer], rel: &str) -> Option<ImageEntry<'a>> {
    let rel = rel.trim_start_matches('/');
    let zs = format!("{rel}.zs");
    for layer in layers {
        match layer {
            RomfsLayer::Dir(root) => {
                if root.join(rel).exists() || root.join(&zs).exists() {
                    return None;
                }
            }
            RomfsLayer::Image(i) => {
                if i.dir(rel).is_some() {
                    return Some(ImageEntry::Dir);
                }
                if let Some((off, size)) = i.file(rel) {
                    return Some(ImageEntry::File(i, off, size, false));
                }
                if let Some((off, size)) = i.file(&zs) {
                    return Some(ImageEntry::File(i, off, size, true));
                }
            }
        }
    }
    None
}

/// What a guest path finds in a mod's romfs.bin. None without such files, whatever the path.
pub(crate) fn image_entry(guest_path: &str) -> Option<ImageEntry<'static>> {
    let layers = mod_romfs_layers();
    if !layers.iter().any(|l| matches!(l, RomfsLayer::Image(_))) {
        return None;
    }
    image_entry_in(layers, romfs_rel(guest_path)?)
}

/// A RomFS-relative path resolved through the mods first (the game's own .zs fallback applies to mod files too). Host
/// paths only: what a romfs.bin holds is found by image_entry first. (The movie player opens files by host path, so it
/// does not see a movie in a romfs.bin.)
pub(crate) fn romfs_path(rel: &str) -> PathBuf {
    let rel = rel.trim_start_matches('/');
    for layer in mod_romfs_layers() {
        let RomfsLayer::Dir(root) = layer else { continue };
        let p = root.join(rel);
        let zs = PathBuf::from(format!("{}.zs", p.display()));
        if p.exists() || zs.exists() {
            return p;
        }
    }
    romfs_root().join(rel)
}

/// A folder to list, in a RomFS merge or on its own.
pub(crate) enum ListedDir<'a> {
    Host(PathBuf),
    /// A mod's romfs folder: its patches of other files (romfs_patch) are not files of the game.
    ModHost(PathBuf),
    Image(&'a [ImageDirEntry]),
}

/// Everywhere a RomFS-relative directory path exists in `layers` (highest priority first), then `game`'s.
pub(crate) fn romfs_dirs_in<'a>(layers: &'a [RomfsLayer], game: &Path, rel: &str) -> Vec<ListedDir<'a>> {
    let rel = rel.trim_start_matches('/');
    let mut v: Vec<ListedDir> = layers
        .iter()
        .filter_map(|l| match l {
            RomfsLayer::Dir(r) => Some(r.join(rel)).filter(|p| p.is_dir()).map(ListedDir::ModHost),
            RomfsLayer::Image(i) => i.dir(rel).map(ListedDir::Image),
        })
        .collect();
    v.push(ListedDir::Host(game.join(rel)));
    v
}

/// What `dirs` hold, the first of a name winning: (name, whether a folder and size, if known). None if none could be read.
pub(crate) fn merged_listing(dirs: &[ListedDir]) -> Option<Vec<(String, Option<(bool, u64)>)>> {
    let (mut found, mut seen, mut out) = (false, std::collections::HashSet::new(), Vec::new());
    for d in dirs {
        match d {
            ListedDir::Host(p) | ListedDir::ModHost(p) => {
                let Ok(rd) = std::fs::read_dir(p) else { continue };
                found = true;
                let of_mod = matches!(d, ListedDir::ModHost(_));
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if of_mod && super::romfs_patch::is_patch_file(&name) {
                        continue;
                    }
                    if seen.insert(name.clone()) {
                        out.push((name, e.metadata().ok().map(|m| (m.is_dir(), m.len()))));
                    }
                }
            }
            ListedDir::Image(entries) => {
                found = true;
                for e in *entries {
                    if seen.insert(e.name.clone()) {
                        out.push((e.name.clone(), Some((e.is_dir, e.size))));
                    }
                }
            }
        }
    }
    found.then_some(out)
}

/// The RomFS-relative part of a guest path, if it names the game's RomFS.
pub(crate) fn romfs_rel(guest_path: &str) -> Option<&str> {
    let clean_path = guest_path.trim_start_matches('/');
    if let Some(rel) = rom_mount_rel(clean_path) {
        return Some(rel);
    }
    if clean_path.starts_with("data:")
        || clean_path.starts_with("save:")
        || clean_path.starts_with("sd:")
        || extra_mount_of(clean_path).is_some()
        || read_only_mount_of(clean_path).is_some()
    {
        return None;
    }
    Some(clean_path)
}

/// resolve_path for a reader that needs a real file (the movie decoder): a file in a romfs.bin is copied out once, to the system's/// temporary folder, as nothing but this one reader needs it there.
pub(crate) fn resolve_to_file(guest_path: &str) -> Option<PathBuf> {
    let Some(ImageEntry::File(image, off, size, zs)) = image_entry(guest_path) else {
        return resolve_path(guest_path);
    };
    use std::hash::{Hash, Hasher};
    use std::io::{Read, Seek};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let changed = std::fs::metadata(&image.path).and_then(|m| m.modified()).ok();
    (&image.path, off, size, changed).hash(&mut h);
    let name = Path::new(guest_path).file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let out = std::env::temp_dir().join(format!("nxport-{:016x}-{name}", h.finish()));
    if !out.is_file() {
        let copied = (|| -> std::io::Result<()> {
            let mut f = File::open(&image.path)?;
            f.seek(std::io::SeekFrom::Start(off))?;
            let tmp = out.with_extension("part");
            let mut to = File::create(&tmp)?;
            if zs {
                zstd::stream::copy_decode(f.take(size), &mut to)?;
            } else {
                std::io::copy(&mut f.take(size), &mut to)?;
            }
            drop(to);
            std::fs::rename(&tmp, &out)
        })();
        if let Err(e) = copied {
            println!("[!] Mods: {guest_path} from a romfs.bin: {e}");
            return None;
        }
    }
    Some(out)
}

pub(crate) fn resolve_path(guest_path: &str) -> Option<PathBuf> {
    // Paths like "content://Sound/x" (double slash) occur; Path::join with a leading "/" would replace
    // the root entirely, so strip every leading slash after the mount name.
    let clean_path = guest_path.trim_start_matches('/');
    if let Some(rel) = rom_mount_rel(clean_path) {
        Some(romfs_path(rel))
    } else if let Some(rel) = clean_path
        .strip_prefix("data:")
        .or_else(|| clean_path.strip_prefix("save:"))
    {
        Some(Path::new(save_root()).join(rel.trim_start_matches('/')))
    } else if let Some(rel) = clean_path.strip_prefix("sd:") {
        Some(
            Path::new(save_root())
                .join("mount_sd")
                .join(rel.trim_start_matches('/')),
        )
    } else if let Some((dir, rel)) = read_only_mount_of(clean_path) {
        Some(dir.join(rel.trim_start_matches('/')))
    } else if let Some((mount, rel)) = extra_mount_of(clean_path) {
        Some(
            Path::new(save_root())
                .join(format!("mount_{}", mount))
                .join(rel.trim_start_matches('/')),
        )
    } else {
        Some(romfs_path(clean_path))
    }
}
