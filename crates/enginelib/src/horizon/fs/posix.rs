// The C library's file functions over the engine's file system: stat, the FILE functions, directories, mkdir / rmdir /
// unlink. The SDK's own versions resolve paths through its mount table, which the engine's mount calls do not fill,
// so they fail for every mount name (Unity reads boot.config with fopen and checks its Addressables settings with
// stat). Paths resolve as nn::fs paths do (resolve_path). A FILE the engine did not open is a console stream (stdout,
// stderr): what is written to it goes to the log, a line at a time.

use super::resolve_path;
use std::collections::HashMap;
use std::ffi::{c_char, CStr};
use std::io::{Read, Seek, SeekFrom, Write};
use std::sync::Mutex;

struct HostFile {
    file: std::fs::File,
    eof: bool,
}

struct HostDir {
    entries: Vec<(Vec<u8>, bool)>,
    next: usize,
    /// The struct dirent readdir returns: d_ino u64, d_off i64, d_reclen u16 at 0x10, d_type u8 at 0x12, d_name at 0x13.
    dirent: Box<[u8; 0x118]>,
}

static FILES: Mutex<Option<HashMap<usize, HostFile>>> = Mutex::new(None);
static DIRS: Mutex<Option<HashMap<usize, HostDir>>> = Mutex::new(None);
static CONSOLE: Mutex<Vec<u8>> = Mutex::new(Vec::new());

unsafe fn guest_str(p: *const c_char) -> Option<String> {
    (!p.is_null()).then(|| CStr::from_ptr(p).to_string_lossy().into_owned())
}

fn with_file<R>(f: usize, op: impl FnOnce(&mut HostFile) -> R) -> Option<R> {
    FILES.lock().unwrap().get_or_insert_with(HashMap::new).get_mut(&f).map(op)
}

/// A handle the guest holds for an object kept here: a unique address of its own.
fn new_handle() -> usize {
    Box::into_raw(Box::new(0u64)) as usize
}

fn free_handle(h: usize) {
    unsafe { drop(Box::from_raw(h as *mut u64)) }
}

/// Text written to a console stream, logged by line.
fn console_write(bytes: &[u8]) {
    let mut buf = CONSOLE.lock().unwrap();
    buf.extend_from_slice(bytes);
    while let Some(n) = buf.iter().position(|&b| b == b'\n') {
        let line: Vec<u8> = buf.drain(..=n).collect();
        println!("[console] {}", String::from_utf8_lossy(&line[..n]).trim_end());
    }
}

fn write_to(f: usize, bytes: &[u8]) -> bool {
    match with_file(f, |h| h.file.write_all(bytes).is_ok()) {
        Some(ok) => ok,
        None => {
            console_write(bytes);
            true
        }
    }
}

/// stat(const char*, struct stat*): mode (0666 with S_IFREG or S_IFDIR) at 0x10, nlink 0x14, size 0x30.
pub(crate) unsafe extern "C" fn stat(path: *const c_char, st: *mut u8) -> i32 {
    let Some(path) = guest_str(path) else { return -1 };
    let Some(host) = resolve_path(&path) else { return -1 };
    let Ok(meta) = std::fs::metadata(&host) else { return -1 };
    if !st.is_null() {
        std::ptr::write_bytes(st, 0, 0x80);
        let mode: u32 = 0o666 | if meta.is_dir() { 0x4000 } else { 0x8000 };
        std::ptr::write_unaligned(st.add(0x10) as *mut u32, mode);
        std::ptr::write_unaligned(st.add(0x14) as *mut u32, 1);
        std::ptr::write_unaligned(st.add(0x30) as *mut u64, if meta.is_dir() { 0 } else { meta.len() });
    }
    0
}

/// fopen(const char*, const char* mode).
pub(crate) unsafe extern "C" fn fopen(path: *const c_char, mode: *const c_char) -> usize {
    let (Some(path), Some(mode)) = (guest_str(path), guest_str(mode)) else { return 0 };
    let Some(host) = resolve_path(&path) else { return 0 };
    let plus = mode.contains('+');
    let mut o = std::fs::OpenOptions::new();
    match mode.chars().next() {
        Some('r') => o.read(true).write(plus),
        Some('w') => o.write(true).create(true).truncate(true).read(plus),
        Some('a') => o.append(true).create(true).read(plus),
        _ => return 0,
    };
    match o.open(&host) {
        Ok(file) => {
            let h = new_handle();
            FILES.lock().unwrap().get_or_insert_with(HashMap::new).insert(h, HostFile { file, eof: false });
            h
        }
        Err(_) => 0,
    }
}

pub(crate) unsafe extern "C" fn fclose(f: usize) -> i32 {
    match FILES.lock().unwrap().get_or_insert_with(HashMap::new).remove(&f) {
        Some(_) => {
            free_handle(f);
            0
        }
        None => 0,
    }
}

/// fread(void*, size, count, FILE*): whole items read.
pub(crate) unsafe extern "C" fn fread(buf: *mut u8, size: usize, count: usize, f: usize) -> usize {
    let want = size.saturating_mul(count);
    if buf.is_null() || want == 0 {
        return 0;
    }
    let out = std::slice::from_raw_parts_mut(buf, want);
    with_file(f, |h| {
        let mut got = 0;
        while got < want {
            match h.file.read(&mut out[got..]) {
                Ok(0) => {
                    h.eof = true;
                    break;
                }
                Ok(n) => got += n,
                Err(_) => break,
            }
        }
        got / size
    })
    .unwrap_or(0)
}

pub(crate) unsafe extern "C" fn fwrite(buf: *const u8, size: usize, count: usize, f: usize) -> usize {
    let n = size.saturating_mul(count);
    if buf.is_null() || n == 0 {
        return 0;
    }
    if write_to(f, std::slice::from_raw_parts(buf, n)) {
        count
    } else {
        0
    }
}

/// fseek(FILE*, long, whence): SEEK_SET 0, SEEK_CUR 1, SEEK_END 2.
pub(crate) unsafe extern "C" fn fseek(f: usize, off: i64, whence: i32) -> i32 {
    let pos = match whence {
        0 => SeekFrom::Start(off.max(0) as u64),
        1 => SeekFrom::Current(off),
        2 => SeekFrom::End(off),
        _ => return -1,
    };
    with_file(f, |h| {
        h.eof = false;
        h.file.seek(pos).map_or(-1, |_| 0)
    })
    .unwrap_or(-1)
}

pub(crate) unsafe extern "C" fn ftell(f: usize) -> i64 {
    with_file(f, |h| h.file.stream_position().map_or(-1, |p| p as i64)).unwrap_or(-1)
}

pub(crate) unsafe extern "C" fn fflush(f: usize) -> i32 {
    with_file(f, |h| h.file.flush().map_or(-1, |_| 0)).unwrap_or(0)
}

/// fgets(char*, int, FILE*): a line, at most n - 1 bytes, NUL-terminated; null at end of file.
pub(crate) unsafe extern "C" fn fgets(buf: *mut c_char, n: i32, f: usize) -> *mut c_char {
    if buf.is_null() || n < 1 {
        return std::ptr::null_mut();
    }
    let got = with_file(f, |h| {
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        while line.len() + 1 < n as usize {
            match h.file.read(&mut b) {
                Ok(1) => {
                    line.push(b[0]);
                    if b[0] == b'\n' {
                        break;
                    }
                }
                _ => {
                    h.eof = true;
                    break;
                }
            }
        }
        line
    });
    match got {
        Some(line) if !line.is_empty() => {
            std::ptr::copy_nonoverlapping(line.as_ptr(), buf as *mut u8, line.len());
            *buf.add(line.len()) = 0;
            buf
        }
        _ => std::ptr::null_mut(),
    }
}

pub(crate) unsafe extern "C" fn fputs(s: *const c_char, f: usize) -> i32 {
    if s.is_null() {
        return -1;
    }
    if write_to(f, CStr::from_ptr(s).to_bytes()) {
        0
    } else {
        -1
    }
}

pub(crate) unsafe extern "C" fn fputc(c: i32, f: usize) -> i32 {
    if write_to(f, &[c as u8]) {
        c & 0xff
    } else {
        -1
    }
}

/// fprintf(FILE*, const char*, ...): integer and pointer arguments (registers x2-x7), as printf here.
pub(crate) unsafe extern "C" fn fprintf(f: usize, fmt: *const c_char, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64, a7: u64) -> i32 {
    if fmt.is_null() {
        return -1;
    }
    let out = crate::horizon::cxx::format_printf(CStr::from_ptr(fmt).to_bytes(), &[a2, a3, a4, a5, a6, a7]);
    if write_to(f, &out) {
        out.len() as i32
    } else {
        -1
    }
}

pub(crate) unsafe extern "C" fn opendir(path: *const c_char) -> usize {
    let Some(path) = guest_str(path) else { return 0 };
    let Some(host) = resolve_path(&path) else { return 0 };
    let Ok(rd) = std::fs::read_dir(&host) else { return 0 };
    let mut entries: Vec<(Vec<u8>, bool)> = rd
        .flatten()
        .map(|e| (e.file_name().to_string_lossy().as_bytes().to_vec(), e.file_type().is_ok_and(|t| t.is_dir())))
        .collect();
    entries.sort();
    let h = new_handle();
    DIRS.lock().unwrap().get_or_insert_with(HashMap::new).insert(h, HostDir { entries, next: 0, dirent: Box::new([0; 0x118]) });
    h
}

/// readdir(DIR*): the next entry (DT_DIR 4, DT_REG 8), or null at the end.
pub(crate) unsafe extern "C" fn readdir(d: usize) -> *mut u8 {
    let mut g = DIRS.lock().unwrap();
    let Some(dir) = g.get_or_insert_with(HashMap::new).get_mut(&d) else { return std::ptr::null_mut() };
    let Some((name, is_dir)) = dir.entries.get(dir.next).cloned() else { return std::ptr::null_mut() };
    dir.next += 1;
    let e = &mut dir.dirent;
    e.fill(0);
    e[0..8].copy_from_slice(&(dir.next as u64).to_le_bytes());
    e[8..16].copy_from_slice(&(dir.next as i64).to_le_bytes());
    e[0x10..0x12].copy_from_slice(&0x118u16.to_le_bytes());
    e[0x12] = if is_dir { 4 } else { 8 };
    let n = name.len().min(255);
    e[0x13..0x13 + n].copy_from_slice(&name[..n]);
    e.as_mut_ptr()
}

pub(crate) unsafe extern "C" fn closedir(d: usize) -> i32 {
    match DIRS.lock().unwrap().get_or_insert_with(HashMap::new).remove(&d) {
        Some(_) => {
            free_handle(d);
            0
        }
        None => -1,
    }
}

pub(crate) unsafe extern "C" fn mkdir(path: *const c_char, _mode: u32) -> i32 {
    let Some(host) = guest_str(path).and_then(|p| resolve_path(&p)) else { return -1 };
    std::fs::create_dir(&host).map_or(-1, |_| 0)
}

pub(crate) unsafe extern "C" fn rmdir(path: *const c_char) -> i32 {
    let Some(host) = guest_str(path).and_then(|p| resolve_path(&p)) else { return -1 };
    std::fs::remove_dir(&host).map_or(-1, |_| 0)
}

pub(crate) unsafe extern "C" fn unlink(path: *const c_char) -> i32 {
    let Some(host) = guest_str(path).and_then(|p| resolve_path(&p)) else { return -1 };
    std::fs::remove_file(&host).map_or(-1, |_| 0)
}
