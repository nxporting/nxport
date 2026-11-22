// RomFS patches in mods: `romfs/<path>.ips` (IPS or IPS32) changes the file `<path>` as the game reads it, whichever
// layer the file itself comes from (the game or a lower-priority mod). A mod changes a few bytes of a game file
// without shipping the file. Patches of several mods apply from the lowest priority up, so the highest wins where they
// overlap. Records past the file's end are ignored: the file keeps its size. Listings leave the .ips files out.

use super::*;
use std::collections::HashMap;
use std::sync::Arc;

/// One file's patch records (file offset, bytes), lowest-priority mod first.
pub(crate) struct FilePatch {
    records: Vec<(u64, Vec<u8>)>,
}

impl FilePatch {
    /// Applies the records to `data`, read at `offset` of the file.
    pub(crate) fn apply(&self, offset: u64, data: &mut [u8]) {
        let end = offset + data.len() as u64;
        for (at, bytes) in &self.records {
            let (start, stop) = ((*at).max(offset), (*at + bytes.len() as u64).min(end));
            if start < stop {
                let (d, s, n) = ((start - offset) as usize, (start - at) as usize, (stop - start) as usize);
                data[d..d + n].copy_from_slice(&bytes[s..s + n]);
            }
        }
    }
}

/// Whether a file in a mod's romfs is a patch of another file, not a file of the game's.
pub(crate) fn is_patch_file(name: &str) -> bool {
    name.len() > 4 && name[name.len() - 4..].eq_ignore_ascii_case(".ips")
}

/// The patches of the RomFS file `rel` (RomFS-relative), if any mod has one. Looked up once per path.
pub(crate) fn for_file(rel: &str) -> Option<Arc<FilePatch>> {
    static CACHE: Mutex<Option<HashMap<String, Option<Arc<FilePatch>>>>> = Mutex::new(None);
    let rel = rel.trim_start_matches('/');
    if mods().is_empty() {
        return None;
    }
    let mut cache = CACHE.lock().unwrap();
    let cache = cache.get_or_insert_with(HashMap::new);
    if let Some(p) = cache.get(rel) {
        return p.clone();
    }
    let mut records = Vec::new();
    for (name, dir) in mods().iter().rev() {
        let path = mod_subdir(dir, "romfs").join(format!("{rel}.ips"));
        let Ok(data) = std::fs::read(&path) else { continue };
        match crate::guest::mod_patches::ips_records(&data) {
            Some(r) => {
                println!("[+] Mods: '{name}' patches {rel} ({} records)", r.len());
                records.extend(r.into_iter().map(|(o, b)| (o as u64, b)));
            }
            None => println!("[!] Mods: '{name}': {} is not a valid IPS / IPS32 file", path.display()),
        }
    }
    let p = (!records.is_empty()).then(|| Arc::new(FilePatch { records }));
    cache.insert(rel.to_string(), p.clone());
    p
}

/// Applies the patches of the file the game opened as `guest_path`, if it is in the RomFS, to `data` read at `offset`.
pub(crate) fn patch_read(guest_path: &str, offset: u64, data: &mut [u8]) {
    if let Some(p) = romfs_rel(guest_path).and_then(for_file) {
        p.apply(offset, data);
    }
}

#[cfg(test)]
mod tests {
    use super::FilePatch;

    #[test]
    fn records_apply_within_each_read() {
        let p = FilePatch { records: vec![(2, vec![9, 9, 9]), (10, vec![7])] };
        let mut a = [0u8; 4];
        p.apply(0, &mut a);
        assert_eq!(a, [0, 0, 9, 9]);
        let mut b = [0u8; 8];
        p.apply(4, &mut b);
        assert_eq!(b, [9, 0, 0, 0, 0, 0, 7, 0]);
    }

    #[test]
    fn patch_file_names() {
        assert!(super::is_patch_file("globalgamemanagers.IPS"));
        assert!(!super::is_patch_file("ips"));
    }
}
