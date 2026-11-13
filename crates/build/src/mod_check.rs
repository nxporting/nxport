// ExeFS patches made for another build of the game (`<build id>.ips`, pchtxt): they apply to the module of that build
// only, so with this game they change nothing, which looks like a bug in the mod or in nxport. The builder says so as
// it packages mods; the engine says it again as the modules load (mod_patches::report_other_builds). Cheat files
// (`cheats/<build id>.txt`) are named for main's build id alone, and checked against it.

use std::path::Path;

/// Prints a warning for each ExeFS patch in `dir` (one mod, or a folder holding mods in Atmosphere's or Ryujinx's
/// layout) that names none of the game dump's modules. `tid`: the game's title id.
pub fn report(dir: &Path, tid: u64, dump: &Path) {
    let mods = if nxport::mods::is_mod_dir(dir) {
        vec![(dir.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned()), dir.to_path_buf())]
    } else {
        nxport::mods::discover_mods(dir, Some(tid))
    };
    let ids: Vec<String> = ["ExeFS", "exefs"]
        .iter()
        .map(|d| dump.join(d))
        .filter(|d| d.is_dir())
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .filter_map(|e| nxport::nso::nso_build_id(&e.path().display().to_string()))
        .collect();
    let ids: Vec<&str> = ids.iter().map(|s| s.as_str()).collect();
    for (mod_name, path) in mods {
        let exefs = nxport::mods::mod_subdir(&path, "exefs");
        for e in std::fs::read_dir(&exefs).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let ext = e.path().extension().and_then(|x| x.to_str()).map(|x| x.to_ascii_lowercase());
            let other = match ext.as_deref() {
                Some("ips") => nxport::mods::for_other_build(&name, None, &ids),
                Some("pchtxt") => {
                    let text = String::from_utf8_lossy(&std::fs::read(e.path()).unwrap_or_default()).into_owned();
                    nxport::mods::for_other_build(&name, Some(&text), &ids)
                }
                _ => false,
            };
            if other {
                println!(
                    "  [!] Mod '{mod_name}': exefs/{name} is for another build of the game (no module of this game has \
                     that build id), so it changes nothing"
                );
            }
        }
        romfs_for_other_version(&mod_name, &nxport::mods::mod_subdir(&path, "romfs"), &dump.join("RomFS"));
        cheats_for_other_build(&mod_name, &nxport::mods::mod_subdir(&path, "cheats"), dump);
    }
}

/// The `.Product.<version>.` part of a file name (TotK names its resource size table and message archives by the game
/// version: ResourceSizeTable.Product.100.rsizetable.zs, Mals/USen.Product.100.sarc.zs), and the name around it.
fn product_version(name: &str) -> Option<(&str, &str, &str)> {
    let at = name.find(".Product.")?;
    let rest = &name[at + ".Product.".len()..];
    let end = rest.find('.').unwrap_or(rest.len());
    let version = &rest[..end];
    (!version.is_empty() && version.bytes().all(|b| b.is_ascii_digit())).then(|| (&name[..at], version, &rest[end..]))
}

/// Warns for each RomFS file of a mod named for another version of the game: one the game has no file of, beside which
/// the game has the same name with another `.Product.<version>.` (a mod merged for 1.1.2 on a 1.0.0 game: the game
/// opens its own version's names, so those files are never read and the mod works in part).
fn romfs_for_other_version(mod_name: &str, mod_romfs: &Path, game_romfs: &Path) {
    let mut stack = vec![mod_romfs.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let Some((stem, version, tail)) = product_version(&name) else { continue };
            let Ok(rel) = p.strip_prefix(mod_romfs) else { continue };
            let game_dir = game_romfs.join(rel.parent().unwrap_or(Path::new("")));
            if game_romfs.join(rel).exists() {
                continue;
            }
            let ours = std::fs::read_dir(&game_dir).into_iter().flatten().flatten().find_map(|g| {
                let n = g.file_name().to_string_lossy().into_owned();
                product_version(&n).filter(|&(s, v, t)| s == stem && t == tail && v != version).map(|(_, v, _)| v.to_string())
            });
            if let Some(game_version) = ours {
                println!(
                    "  [!] Mod '{mod_name}': romfs/{} is for version {version} of the game (this game reads \
                     .Product.{game_version}.), so it is never read",
                    rel.display().to_string().replace('\\', "/")
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn product_versions_are_read_from_names() {
        assert_eq!(super::product_version("ResourceSizeTable.Product.112.rsizetable.zs"), Some(("ResourceSizeTable", "112", ".rsizetable.zs")));
        assert_eq!(super::product_version("USen.Product.100.sarc.zs"), Some(("USen", "100", ".sarc.zs")));
        assert_eq!(super::product_version("Link.bfres.zs"), None);
        assert_eq!(super::product_version("Odd.Product.x1.zs"), None);
    }
}

/// Warns of each cheat file in `cheats` named for another build of main than the dump's (the first 8 bytes of its build
/// id, in hex): only the file named for the running main is loaded.
fn cheats_for_other_build(mod_name: &str, cheats: &Path, dump: &Path) {
    let main = ["ExeFS", "exefs"].iter().find_map(|d| nxport::nso::nso_build_id(&dump.join(d).join("main").display().to_string()));
    let Some(id) = main.as_deref().and_then(|m| m.get(..16)) else { return };
    for e in std::fs::read_dir(cheats).into_iter().flatten().flatten() {
        let path = e.path();
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let txt = path.extension().is_some_and(|x| x.eq_ignore_ascii_case("txt"));
        if txt && stem.len() == 16 && stem.chars().all(|c| c.is_ascii_hexdigit()) && !stem.eq_ignore_ascii_case(id) {
            println!(
                "  [!] Mod '{mod_name}': cheats/{} is for another build of the game (this one's main is {id}), so it is not \
                 loaded",
                e.file_name().to_string_lossy()
            );
        }
    }
}
