// Atmosphere's packed mod forms: exefs.nsp (a PFS0 holding ExeFS files) and romfs.bin (a RomFS image, no IVFC levels).

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

fn u32_at(d: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(d.get(off..off + 4)?.try_into().ok()?))
}

fn u64_at(d: &[u8], off: usize) -> Option<u64> {
    Some(u64::from_le_bytes(d.get(off..off + 8)?.try_into().ok()?))
}

fn read_section(r: &mut (impl Read + Seek), off: u64, size: u64, len: u64) -> Result<Vec<u8>, String> {
    if off.checked_add(size).map_or(true, |end| end > len) {
        return Err("a table lies outside the file".into());
    }
    r.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
    let mut v = vec![0; size as usize];
    r.read_exact(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}

/// `data + off`, if `size` bytes from there lie within a file of `len` bytes.
fn inside(data: u64, off: u64, size: u64, len: u64) -> Option<u64> {
    data.checked_add(off).filter(|s| s.checked_add(size).is_some_and(|end| end <= len))
}

/// The files of a PFS0 of `len` bytes: (name, offset in the file, size).
fn pfs0_files(r: &mut (impl Read + Seek), len: u64) -> Result<Vec<(String, u64, u64)>, String> {
    const ENTRY: usize = 0x18;
    let head = read_section(r, 0, 16.min(len), len)?;
    if head.len() < 16 || &head[..4] != b"PFS0" {
        return Err("not a PFS0 file".into());
    }
    let (count, strings) = (u32_at(&head, 4).unwrap() as usize, u32_at(&head, 8).unwrap() as u64);
    let table = read_section(r, 16, count as u64 * ENTRY as u64 + strings, len)?;
    let data = 16 + table.len() as u64;
    let names = &table[count * ENTRY..];
    (0..count)
        .map(|i| {
            let e = &table[i * ENTRY..(i + 1) * ENTRY];
            let (off, size, name) = (u64_at(e, 0).unwrap(), u64_at(e, 8).unwrap(), u32_at(e, 16).unwrap() as usize);
            let name = names.get(name..).ok_or("a file name lies outside the table")?;
            let name = String::from_utf8_lossy(name.split(|b| *b == 0).next().unwrap_or(name)).into_owned();
            // A mod names these, and they become host paths: on Windows `C:x` joined to a folder leaves it.
            if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':']) {
                return Err(format!("bad file name '{name}'"));
            }
            let start = inside(data, off, size, len).ok_or_else(|| format!("{name} lies outside the file"))?;
            Ok((name, start, size))
        })
        .collect()
}

/// Writes the files of the PFS0 at `pfs0` into the directory `into`, which it creates.
pub fn unpack_pfs0(pfs0: &Path, into: &Path) -> Result<(), String> {
    let mut f = File::open(pfs0).map_err(|e| e.to_string())?;
    let len = f.metadata().map_err(|e| e.to_string())?.len();
    let files = pfs0_files(&mut f, len)?;
    std::fs::create_dir_all(into).map_err(|e| e.to_string())?;
    for (name, off, size) in files {
        f.seek(SeekFrom::Start(off)).map_err(|e| e.to_string())?;
        let mut out = File::create(into.join(&name)).map_err(|e| e.to_string())?;
        std::io::copy(&mut (&mut f).take(size), &mut out).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// A file or folder directly in a folder of a RomFS image.
pub struct ImageDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

/// A RomFS image's index; the data stays in the file. Paths are relative to its root.
pub struct RomfsImage {
    pub path: PathBuf,
    files: HashMap<String, (u64, u64)>,
    dirs: HashMap<String, Vec<ImageDirEntry>>,
}

/// `rel` without empty components ("/a//b/" is "a/b"), as host paths treat them.
fn image_key(rel: &str) -> String {
    rel.split('/').filter(|c| !c.is_empty()).collect::<Vec<_>>().join("/")
}

impl RomfsImage {
    pub fn open(path: &Path) -> Result<RomfsImage, String> {
        let mut f = File::open(path).map_err(|e| e.to_string())?;
        let len = f.metadata().map_err(|e| e.to_string())?.len();
        Self::parse(path, &mut f, len)
    }

    fn parse(path: &Path, r: &mut (impl Read + Seek), len: u64) -> Result<RomfsImage, String> {
        const HEADER: u64 = 0x50;
        const NONE: u32 = u32::MAX;
        let head = read_section(r, 0, HEADER.min(len), len)?;
        if head.starts_with(b"IVFC") {
            return Err("it has IVFC levels: a plain RomFS image is expected".into());
        }
        let field = |i: usize| u64_at(&head, i * 8).ok_or("too short for a RomFS image".to_string());
        if field(0)? != HEADER {
            return Err("not a RomFS image".into());
        }
        let dir_meta = read_section(r, field(3)?, field(4)?, len)?;
        let file_meta = read_section(r, field(7)?, field(8)?, len)?;
        let data = field(9)?;
        let bad = || "a table entry lies outside its table".to_string();
        let name_at = |e: &[u8], off: usize, name_len: u32| -> Result<String, String> {
            let name = e.get(off..off.checked_add(name_len as usize).ok_or_else(bad)?).ok_or_else(bad)?;
            let name = String::from_utf8_lossy(name).into_owned();
            if name.is_empty() || name.contains('/') { Err(format!("bad file name '{name}'")) } else { Ok(name) }
        };
        let join = |dir: &str, name: &str| if dir.is_empty() { name.to_string() } else { format!("{dir}/{name}") };

        let mut image = RomfsImage { path: path.to_path_buf(), files: HashMap::new(), dirs: HashMap::new() };
        let (mut seen_dirs, mut seen_files) = (HashSet::new(), HashSet::new());
        let mut todo = vec![(0u32, String::new())];
        while let Some((at, dir)) = todo.pop() {
            if !seen_dirs.insert(at) {
                return Err("its folders form a loop".into());
            }
            let e = dir_meta.get(at as usize..).ok_or_else(bad)?;
            let (mut child, mut file) = (u32_at(e, 8).ok_or_else(bad)?, u32_at(e, 12).ok_or_else(bad)?);
            let mut list = Vec::new();
            while child != NONE {
                if seen_dirs.contains(&child) || list.len() > dir_meta.len() {
                    return Err("its folders form a loop".into());
                }
                let e = dir_meta.get(child as usize..).ok_or_else(bad)?;
                let name = name_at(e, 24, u32_at(e, 20).ok_or_else(bad)?)?;
                todo.push((child, join(&dir, &name)));
                list.push(ImageDirEntry { name, is_dir: true, size: 0 });
                child = u32_at(e, 4).ok_or_else(bad)?;
            }
            while file != NONE {
                if !seen_files.insert(file) {
                    return Err("its files form a loop".into());
                }
                let e = file_meta.get(file as usize..).ok_or_else(bad)?;
                let name = name_at(e, 32, u32_at(e, 28).ok_or_else(bad)?)?;
                let (off, size) = (u64_at(e, 8).ok_or_else(bad)?, u64_at(e, 16).ok_or_else(bad)?);
                let start = inside(data, off, size, len).ok_or_else(|| format!("{name} lies outside the file"))?;
                image.files.insert(join(&dir, &name), (start, size));
                list.push(ImageDirEntry { name, is_dir: false, size });
                file = u32_at(e, 4).ok_or_else(bad)?;
            }
            image.dirs.insert(dir, list);
        }
        Ok(image)
    }

    /// Where the file `rel` is in the image: (offset, size).
    pub fn file(&self, rel: &str) -> Option<(u64, u64)> {
        self.files.get(&image_key(rel)).copied()
    }

    /// What the folder `rel` holds.
    pub fn dir(&self, rel: &str) -> Option<&'_ [ImageDirEntry]> {
        self.dirs.get(&image_key(rel)).map(|v| v.as_slice())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// A PFS0 of (name, contents).
    pub(crate) fn pfs0(files: &[(&str, &[u8])]) -> Vec<u8> {
        let (mut strings, mut table, mut off) = (Vec::new(), Vec::new(), 0u64);
        for (name, data) in files {
            table.extend_from_slice(&off.to_le_bytes());
            table.extend_from_slice(&(data.len() as u64).to_le_bytes());
            table.extend_from_slice(&(strings.len() as u32).to_le_bytes());
            table.extend_from_slice(&0u32.to_le_bytes());
            strings.extend_from_slice(name.as_bytes());
            strings.push(0);
            off += data.len() as u64;
        }
        strings.resize((strings.len() + 15) & !15, 0);
        let mut f = b"PFS0".to_vec();
        f.extend_from_slice(&(files.len() as u32).to_le_bytes());
        f.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&table);
        f.extend_from_slice(&strings);
        for (_, data) in files {
            f.extend_from_slice(data);
        }
        f
    }

    /// A RomFS image of (path, contents), its paths without a leading slash. The hash tables are empty: nothing here uses them.
    pub(crate) fn romfs(files: &[(&str, &[u8])]) -> Vec<u8> {
        const NONE: u32 = u32::MAX;
        let mut dirs = vec![String::new()];
        for (p, _) in files {
            let parts: Vec<&str> = p.split('/').collect();
            for i in 1..parts.len() {
                let d = parts[..i].join("/");
                if !dirs.contains(&d) {
                    dirs.push(d);
                }
            }
        }
        let align = |n: usize| (n + 3) & !3;
        let base = |p: &str| p.rsplit('/').next().unwrap().to_string();
        let parent = |p: &str| p.rsplit_once('/').map_or(String::new(), |(d, _)| d.to_string());
        let (mut dir_off, mut at) = (HashMap::new(), 0);
        for d in &dirs {
            dir_off.insert(d.clone(), at as u32);
            at += 24 + align(base(d).len());
        }
        let (mut file_off, mut at) = (Vec::new(), 0);
        for (p, _) in files {
            file_off.push(at as u32);
            at += 32 + align(base(p).len());
        }
        // The next offset after `me` in `list`; the first with none.
        let after = |list: &[u32], me: Option<u32>| list.iter().copied().find(|o| me.map_or(true, |m| *o > m)).unwrap_or(NONE);
        let mut dir_meta = Vec::new();
        for d in &dirs {
            let dirs_in = |p: &str| dirs.iter().filter(|c| !c.is_empty() && parent(c) == p).map(|c| dir_off[c]).collect::<Vec<u32>>();
            let files_in: Vec<u32> = files.iter().zip(&file_off).filter(|((p, _), _)| parent(p) == *d).map(|(_, o)| *o).collect();
            let sibling = if d.is_empty() { NONE } else { after(&dirs_in(&parent(d)), Some(dir_off[d])) };
            let name = base(d);
            for v in [dir_off[&parent(d)], sibling, after(&dirs_in(d), None), after(&files_in, None), NONE, name.len() as u32] {
                dir_meta.extend_from_slice(&v.to_le_bytes());
            }
            dir_meta.extend_from_slice(name.as_bytes());
            dir_meta.resize(align(dir_meta.len()), 0);
        }
        let (mut file_meta, mut blob) = (Vec::new(), Vec::new());
        for ((p, data), off) in files.iter().zip(&file_off) {
            let same: Vec<u32> = files.iter().zip(&file_off).filter(|((q, _), _)| parent(q) == parent(p)).map(|(_, o)| *o).collect();
            let name = base(p);
            file_meta.extend_from_slice(&dir_off[&parent(p)].to_le_bytes());
            file_meta.extend_from_slice(&after(&same, Some(*off)).to_le_bytes());
            file_meta.extend_from_slice(&(blob.len() as u64).to_le_bytes());
            file_meta.extend_from_slice(&(data.len() as u64).to_le_bytes());
            file_meta.extend_from_slice(&NONE.to_le_bytes());
            file_meta.extend_from_slice(&(name.len() as u32).to_le_bytes());
            file_meta.extend_from_slice(name.as_bytes());
            file_meta.resize(align(file_meta.len()), 0);
            blob.extend_from_slice(data);
            blob.resize((blob.len() + 15) & !15, 0);
        }
        let dir_meta_off = 0x50u64;
        let file_meta_off = dir_meta_off + dir_meta.len() as u64;
        let data_off = (file_meta_off + file_meta.len() as u64 + 15) & !15;
        let mut f = Vec::new();
        for v in [0x50, 0x50, 0, dir_meta_off, dir_meta.len() as u64, file_meta_off, 0, file_meta_off, file_meta.len() as u64, data_off] {
            f.extend_from_slice(&u64::to_le_bytes(v));
        }
        f.extend_from_slice(&dir_meta);
        f.extend_from_slice(&file_meta);
        f.resize(data_off as usize, 0);
        f.extend_from_slice(&blob);
        f
    }

    fn parse(f: &[u8]) -> Result<RomfsImage, String> {
        RomfsImage::parse(Path::new("x"), &mut Cursor::new(f), f.len() as u64)
    }

    #[test]
    fn pfs0_names_that_would_leave_the_folder_are_refused() {
        for bad in ["../x", "a/b", "a\\b", "..", ".", "C:x", "x:stream"] {
            // One empty file: header, one entry, the name.
            let mut names = bad.as_bytes().to_vec();
            names.push(0);
            let mut f = Vec::new();
            f.extend_from_slice(b"PFS0");
            f.extend_from_slice(&1u32.to_le_bytes());
            f.extend_from_slice(&(names.len() as u32).to_le_bytes());
            f.extend_from_slice(&0u32.to_le_bytes());
            f.extend_from_slice(&0u64.to_le_bytes());
            f.extend_from_slice(&0u64.to_le_bytes());
            f.extend_from_slice(&0u32.to_le_bytes());
            f.extend_from_slice(&0u32.to_le_bytes());
            f.extend_from_slice(&names);
            let len = f.len() as u64;
            assert!(pfs0_files(&mut Cursor::new(&f), len).is_err(), "{bad} was accepted");
        }
    }

    #[test]
    fn pfs0_lists_its_files() {
        let f = pfs0(&[("main", b"code"), ("main.npdm", b"meta!")]);
        let files = pfs0_files(&mut Cursor::new(&f), f.len() as u64).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[1].0, "main.npdm");
        assert_eq!(&f[files[0].1 as usize..][..files[0].2 as usize], b"code");
        assert_eq!(&f[files[1].1 as usize..][..files[1].2 as usize], b"meta!");
    }

    #[test]
    fn pfs0_refuses_bad_files() {
        let check = |f: &[u8]| pfs0_files(&mut Cursor::new(f), f.len() as u64).is_err();
        assert!(check(b"PFS1aaaaaaaaaaaa"));
        assert!(check(b"PFS0"));
        assert!(check(&pfs0(&[("../main", b"x")])));
        let f = pfs0(&[("main", b"code")]);
        assert!(check(&f[..f.len() - 1]), "a file past the end");
        let mut many = f.clone();
        many[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(check(&many), "a table past the end");
    }

    #[test]
    fn romfs_finds_files_and_folders() {
        let f = romfs(&[("a.txt", b"root"), ("Pack/b.bin", b"bb"), ("Pack/Sub/c", b"ccc"), ("Pack/d", b""), ("Other/e", b"e")]);
        let i = parse(&f).unwrap();
        let read = |p: &str| i.file(p).map(|(o, s)| f[o as usize..][..s as usize].to_vec());
        assert_eq!(read("a.txt").unwrap(), b"root");
        assert_eq!(read("/Pack//Sub/c").unwrap(), b"ccc");
        assert_eq!(read("Pack/d").unwrap(), b"");
        assert!(i.file("Pack").is_none());
        assert!(i.file("nope").is_none());
        let names = |d: &str| {
            let mut v: Vec<(String, bool, u64)> = i.dir(d).unwrap().iter().map(|e| (e.name.clone(), e.is_dir, e.size)).collect();
            v.sort();
            v
        };
        assert_eq!(names("/"), [("Other".to_string(), true, 0), ("Pack".to_string(), true, 0), ("a.txt".to_string(), false, 4)]);
        assert_eq!(names("Pack/"), [("Sub".to_string(), true, 0), ("b.bin".to_string(), false, 2), ("d".to_string(), false, 0)]);
        assert!(i.dir("a.txt").is_none());
    }

    #[test]
    fn romfs_refuses_bad_images() {
        assert!(parse(b"IVFC\0\0\0\0").is_err());
        assert!(parse(b"short").is_err());
        assert!(parse(&[0; 0x50]).is_err());
        let f = romfs(&[("a/b", b"data")]);
        assert!(parse(&f[..f.len() - 16]).is_err(), "a file past the end");
        // The root's child folder points back at the root.
        let meta = u64_at(&f, 3 * 8).unwrap() as usize;
        let mut looped = f.clone();
        looped[meta + 8..meta + 12].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse(&looped).is_err(), "a folder inside itself");
        // A file that is its own sibling.
        let meta = u64_at(&f, 7 * 8).unwrap() as usize;
        let mut looped = f.clone();
        looped[meta + 4..meta + 8].copy_from_slice(&0u32.to_le_bytes());
        assert!(parse(&looped).is_err(), "a file after itself");
    }
}
