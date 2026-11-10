// ExeFS patches made for another build of the game (`<build id>.ips`, pchtxt): they apply to the module of that build
// only, so with this game they change nothing, which looks like a bug in the mod or in nxport. The builder says so as
// it packages mods; the engine says it again as the modules load (mod_patches::report_other_builds).

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
    }
}
