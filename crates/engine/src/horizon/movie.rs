// HLE of the game's horizon::movie:: library (subsdk0): Buffer, MediaData, Extractor and Decoder,
// backed by ffmpeg through target/movie_shim.dylib (tools/movie_shim). The native library decodes through
// multimedia sysmodules and the hardware video decoder, neither of which runs here. Without the shim, files open with no
// tracks (playback ends at once).
//
// Object layouts: Buffer is plain data, MediaData / Extractor / Decoder are a pointer
// to an implementation (plus a vtable for the last two). Vtables follow declaration order
// after the Itanium ABI's two destructor entries. The decoder follows the MediaCodec-style decoder contract:
// input / output buffer indices, BufferFlags_EndOfStream, and nn::os events signaled when buffers become available.

use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::OnceLock;

// horizon::movie::Status
const OK: u32 = 0;
const ERR_UNKNOWN: u32 = 0x1000;
const ERR_INVALID_RANGE: u32 = 0x1001;
const ERR_BUFFER_TOO_SMALL: u32 = 0x1005;
const ERR_NOT_SUPPORTED: u32 = 0x1006;
const ERR_IO: u32 = 0x1009;
const OUTPUT_NOT_AVAILABLE: u32 = 0x100e;
const INPUT_NOT_AVAILABLE: u32 = 0x1011;
const END_OF_STREAM: u32 = 0x2000;
const TRACK_NOT_FOUND: u32 = 0x3000;
const NULL_DECODER: u32 = 0x2004;
const NULL_EXTRACTOR: u32 = 0x3003;
const BUFFER_FLAG_EOS: u32 = 4; // BufferFlags_EndOfStream

// ── ffmpeg shim ─────────────────────────────────────────────────────────────────────────────────────────────────
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct TrackInfo {
    ty: c_int,
    codec: c_int,
    width: c_int,
    height: c_int,
    fps: f64,
    sample_rate: c_int,
    channels: c_int,
    duration_us: i64,
}

struct Shim {
    open: unsafe extern "C" fn(*const c_char, *mut c_int) -> *mut c_void,
    track_info: unsafe extern "C" fn(*mut c_void, c_int, *mut TrackInfo) -> c_int,
    peek: unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int, *mut i64, *mut c_int) -> c_int,
    copy: unsafe extern "C" fn(*mut c_void, *mut u8, c_int) -> c_int,
    advance: unsafe extern "C" fn(*mut c_void),
    seek: unsafe extern "C" fn(*mut c_void, i64) -> c_int,
    close: unsafe extern "C" fn(*mut c_void),
    md_open: unsafe extern "C" fn(*mut c_void, c_int, c_int) -> *mut c_void,
    md_send: unsafe extern "C" fn(*mut c_void, *const u8, c_int, i64, c_int) -> c_int,
    md_receive: unsafe extern "C" fn(
        *mut c_void,
        *mut u8,
        c_int,
        *mut i64,
        *mut c_int,
        *mut c_int,
        *mut c_int,
        *mut c_int,
    ) -> c_int,
    md_flush: unsafe extern "C" fn(*mut c_void),
    md_close: unsafe extern "C" fn(*mut c_void),
}
unsafe impl Sync for Shim {}

fn shim() -> Option<&'static Shim> {
    static S: OnceLock<Option<Shim>> = OnceLock::new();
    S.get_or_init(|| unsafe {
        let mut candidates: Vec<String> = Vec::new();
        if let Some(p) = &crate::settings::get().files.movie_shim {
            candidates.push(p.display().to_string());
        }
        if let Ok(e) = std::env::current_exe() {
            if let Some(p) = e.parent() {
                candidates.push(p.join("movie_shim.dylib").display().to_string());
                candidates.push(p.join("../Frameworks/movie_shim.dylib").display().to_string());
                candidates.push(p.join("Frameworks/movie_shim.dylib").display().to_string());
            }
        }
        for c in candidates.iter().filter(|c| !c.is_empty()) {
            let path = CString::new(c.as_str()).ok()?;
            let h = libc::dlopen(path.as_ptr(), libc::RTLD_NOW);
            if h.is_null() {
                continue;
            }
            macro_rules! sym {
                ($n:literal) => {{
                    let p = libc::dlsym(h, concat!($n, "\0").as_ptr() as *const c_char);
                    if p.is_null() {
                        println!("[!] movie: {} lacks {}", c, $n);
                        return None;
                    }
                    std::mem::transmute(p)
                }};
            }
            println!("[+] movie: playback through {}", c);
            return Some(Shim {
                open: sym!("ms_open"), track_info: sym!("ms_track_info"), peek: sym!("ms_peek"), copy: sym!("ms_copy"),
                advance: sym!("ms_advance"), seek: sym!("ms_seek"), close: sym!("ms_close"), md_open: sym!("md_open"),
                md_send: sym!("md_send"), md_receive: sym!("md_receive"), md_flush: sym!("md_flush"), md_close: sym!("md_close"),
            });
        }
        println!("[!] movie: no target/movie_shim.dylib (tools/build_movie_shim.sh): movies have no tracks");
        None
    }).as_ref()
}

// ── horizon::movie::Buffer (private members) ─────────────────────────────────────────────────────────────────────
// The NVN device and sync members came with a later library version: 4.4.0's Buffer is 0x30 bytes (its constructor
// zeroes 0x0..0x30), 7.3.2's, 11.4.0's and 15.3.1's are 0x40. Writing the newer layout into an older game's Buffer
// overwrote the 16 bytes after it on the caller's stack (Mario Kart 8 Deluxe: a saved register holding a multi-wait
// holder, so its movie player never recognised "output available").
#[repr(C)]
struct Buffer {
    range_offset: i32,
    range_length: i32,
    int32_data: i32,
    uint32_data: u32,
    owns_data: bool,
    capacity: usize,
    int64_data: i64,
    data: *mut u8,
    nvn_device: usize,
    nvn_sync: usize,
}

/// The game's movie library version (its subsdk module's middleware string).
fn library_version() -> Option<(u32, u32, u32)> {
    static V: OnceLock<Option<(u32, u32, u32)>> = OnceLock::new();
    *V.get_or_init(|| {
        let dir = crate::settings::get().files.game_dir.clone()?;
        let exefs = ["ExeFS", "exefs"].iter().map(|d| dir.join(d)).find(|d| d.is_dir())?;
        let v = (0..10).find_map(|i| {
            let nso = crate::guest::nso::Nso::read(&exefs.join(format!("subsdk{i}"))).ok()?;
            crate::guest::nso::middleware_version(&nso.ro.data, b"NintendoSDK_movie-")
        });
        if let Some((a, b, c)) = v {
            println!("[+] movie: library {a}.{b}.{c}");
        }
        v
    })
}

/// Whether the game's Buffer has the NVN device and sync members (an unknown version: the newest layout).
fn buffer_has_nvn() -> bool {
    library_version().map_or(true, |v| v >= (7, 3, 0))
}

unsafe fn buf_init(b: *mut Buffer, data: *mut u8, capacity: usize, owns: bool) {
    let n = if buffer_has_nvn() { std::mem::size_of::<Buffer>() } else { std::mem::offset_of!(Buffer, nvn_device) };
    let v = Buffer {
        range_offset: 0,
        range_length: 0,
        int32_data: 0,
        uint32_data: 0,
        owns_data: owns,
        capacity,
        int64_data: 0,
        data,
        nvn_device: 0,
        nvn_sync: 0,
    };
    std::ptr::copy_nonoverlapping(&v as *const Buffer as *const u8, b as *mut u8, n);
}
unsafe fn alloc_bytes(n: usize) -> *mut u8 {
    if n == 0 {
        std::ptr::null_mut()
    } else {
        libc::calloc(1, n) as *mut u8
    }
}
unsafe extern "C" fn buffer_ctor(b: *mut Buffer) {
    buf_init(b, std::ptr::null_mut(), 0, false)
}
unsafe extern "C" fn buffer_ctor_capacity(b: *mut Buffer, cap: usize) {
    buf_init(b, alloc_bytes(cap), cap, true)
}
unsafe extern "C" fn buffer_ctor_data(b: *mut Buffer, data: *mut u8, cap: usize) {
    buf_init(b, data, cap, false)
}
unsafe extern "C" fn buffer_ctor_copy(b: *mut Buffer, o: *const Buffer) {
    buffer_ctor(b);
    buffer_assign(b, o);
}
unsafe extern "C" fn buffer_assign(b: *mut Buffer, o: *const Buffer) -> *mut Buffer {
    if b as *const Buffer == o {
        return b;
    }
    let (b_, o_) = (&mut *b, &*o);
    if b_.owns_data {
        libc::free(b_.data as *mut c_void);
    }
    // A deep copy when the source owns its data, otherwise the same (caller-owned) memory.
    if o_.owns_data {
        b_.data = alloc_bytes(o_.capacity);
        if !b_.data.is_null() && !o_.data.is_null() {
            std::ptr::copy_nonoverlapping(o_.data, b_.data, o_.capacity);
        }
        b_.owns_data = true;
    } else {
        b_.data = o_.data;
        b_.owns_data = false;
    }
    b_.capacity = o_.capacity;
    b_.range_offset = o_.range_offset;
    b_.range_length = o_.range_length;
    b_.int32_data = o_.int32_data;
    b_.uint32_data = o_.uint32_data;
    b_.int64_data = o_.int64_data;
    if buffer_has_nvn() {
        b_.nvn_device = o_.nvn_device;
        b_.nvn_sync = o_.nvn_sync;
    }
    b
}
unsafe extern "C" fn buffer_dtor(b: *mut Buffer) {
    if (*b).owns_data {
        libc::free((*b).data as *mut c_void);
        (*b).data = std::ptr::null_mut();
        (*b).owns_data = false;
    }
}
unsafe extern "C" fn buffer_data(b: *const Buffer) -> *mut u8 {
    if (*b).data.is_null() {
        std::ptr::null_mut()
    } else {
        (*b).data.add((*b).range_offset.max(0) as usize)
    }
}
unsafe extern "C" fn buffer_base(b: *const Buffer) -> *mut u8 {
    (*b).data
}
unsafe extern "C" fn buffer_capacity(b: *const Buffer) -> usize {
    (*b).capacity
}
unsafe extern "C" fn buffer_offset(b: *const Buffer) -> i32 {
    (*b).range_offset
}
unsafe extern "C" fn buffer_size(b: *const Buffer) -> i32 {
    (*b).range_length
}
unsafe extern "C" fn buffer_set_data_and_capacity(b: *mut Buffer, data: *mut u8, cap: usize) {
    buffer_dtor(b);
    (*b).data = data;
    (*b).capacity = cap;
}
unsafe extern "C" fn buffer_set_capacity(b: *mut Buffer, cap: usize) {
    (*b).capacity = cap
}
unsafe extern "C" fn buffer_set_range(b: *mut Buffer, off: i32, size: i32) {
    (*b).range_offset = off;
    (*b).range_length = size;
}
unsafe extern "C" fn buffer_set_i32(b: *mut Buffer, v: i32) {
    (*b).int32_data = v
}
unsafe extern "C" fn buffer_get_i32(b: *const Buffer) -> i32 {
    (*b).int32_data
}
unsafe extern "C" fn buffer_set_i64(b: *mut Buffer, v: i64) {
    (*b).int64_data = v
}
unsafe extern "C" fn buffer_get_i64(b: *const Buffer) -> i64 {
    (*b).int64_data
}
unsafe extern "C" fn buffer_set_u32(b: *mut Buffer, v: u32) {
    (*b).uint32_data = v
}
unsafe extern "C" fn buffer_get_u32(b: *const Buffer) -> u32 {
    (*b).uint32_data
}

// ── horizon::movie::MediaData ({ void* m_MediaDataImpl }) ───────────────────────────────────────────────────────
#[derive(Clone)]
enum Val {
    I32(i32),
    I64(i64),
    Str(CString),
    Rect([i32; 4]),
}
type Store = HashMap<String, Val>;

unsafe fn store<'a>(md: *mut usize) -> &'a mut Store {
    if *md == 0 {
        *md = Box::into_raw(Box::new(Store::new())) as usize;
    }
    &mut *(*md as *mut Store)
}
unsafe fn key(name: *const c_char) -> String {
    if name.is_null() {
        String::new()
    } else {
        CStr::from_ptr(name).to_string_lossy().into_owned()
    }
}
unsafe extern "C" fn md_ctor(md: *mut usize) {
    *md = 0
}
unsafe extern "C" fn md_dtor(md: *mut usize) {
    if *md != 0 {
        drop(Box::from_raw(*md as *mut Store));
        *md = 0;
    }
}
unsafe extern "C" fn md_copy_ctor(md: *mut usize, o: *const usize) {
    *md = 0;
    md_assign(md, o);
}
unsafe extern "C" fn md_assign(md: *mut usize, o: *const usize) -> *mut usize {
    if md as *const usize != o {
        let copy = if *o == 0 {
            Store::new()
        } else {
            (*(*o as *const Store)).clone()
        };
        *store(md) = copy;
    }
    md
}
unsafe extern "C" fn md_clear(md: *mut usize) {
    store(md).clear()
}
fn md_set(md: *mut usize, k: &str, v: Val) {
    unsafe { store(md).insert(k.to_string(), v) };
}
unsafe extern "C" fn md_set_i32(md: *mut usize, name: *const c_char, v: i32) {
    md_set(md, &key(name), Val::I32(v))
}
unsafe extern "C" fn md_set_i64(md: *mut usize, name: *const c_char, v: i64) {
    md_set(md, &key(name), Val::I64(v))
}
unsafe extern "C" fn md_set_string(md: *mut usize, name: *const c_char, v: *const c_char) {
    md_set(md, &key(name), Val::Str(CStr::from_ptr(v).to_owned()))
}
unsafe extern "C" fn md_set_rect(
    md: *mut usize,
    name: *const c_char,
    l: i32,
    t: i32,
    r: i32,
    b: i32,
) {
    md_set(md, &key(name), Val::Rect([l, t, r, b]))
}
unsafe extern "C" fn md_find_rect(
    md: *mut usize,
    name: *const c_char,
    l: *mut i32,
    t: *mut i32,
    r: *mut i32,
    b: *mut i32,
) -> bool {
    if *md == 0 {
        return false;
    }
    match store(md).get(&key(name)) {
        Some(Val::Rect(v)) => {
            for (p, x) in [l, t, r, b].into_iter().zip(v) {
                if !p.is_null() {
                    *p = *x;
                }
            }
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn md_contains(md: *mut usize, name: *const c_char) -> bool {
    *md != 0 && store(md).contains_key(&key(name))
}
unsafe extern "C" fn md_find_i32(md: *mut usize, name: *const c_char, out: *mut i32) -> bool {
    if log_enabled() {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let k = key(name);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&k) {
            println!(
                "[movie] MediaData::md_find_i32('{}') present={}",
                k,
                *md != 0 && store(md).contains_key(&k)
            );
            seen.push(k);
        }
    }
    match (*md != 0)
        .then(|| store(md).get(&key(name)).cloned())
        .flatten()
    {
        Some(Val::I32(v)) => {
            *out = v;
            true
        }
        Some(Val::I64(v)) => {
            *out = v as i32;
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn md_find_i64(md: *mut usize, name: *const c_char, out: *mut i64) -> bool {
    if log_enabled() {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let k = key(name);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&k) {
            println!(
                "[movie] MediaData::md_find_i64('{}') present={}",
                k,
                *md != 0 && store(md).contains_key(&k)
            );
            seen.push(k);
        }
    }
    match (*md != 0)
        .then(|| store(md).get(&key(name)).cloned())
        .flatten()
    {
        Some(Val::I64(v)) => {
            *out = v;
            true
        }
        Some(Val::I32(v)) => {
            *out = v as i64;
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn md_find_string(
    md: *mut usize,
    name: *const c_char,
    out: *mut *const c_char,
) -> bool {
    if log_enabled() {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let k = key(name);
        let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
        if !seen.contains(&k) {
            println!(
                "[movie] MediaData::md_find_string('{}') present={}",
                k,
                *md != 0 && store(md).contains_key(&k)
            );
            seen.push(k);
        }
    }
    if *md == 0 {
        return false;
    }
    match store(md).get(&key(name)) {
        Some(Val::Str(s)) => {
            *out = s.as_ptr();
            true
        }
        _ => false,
    }
}
unsafe extern "C" fn md_count(md: *mut usize) -> usize {
    if *md == 0 {
        0
    } else {
        store(md).len()
    }
}

// ── horizon::movie::Extractor ────────────────────────────────────────────────────────────────────────────────────────────
/// One lock for all extractor calls: Kirby shares one Extractor between its audio and video input threads (each takes
/// the samples of its own track), and the demuxer behind it is not thread-safe.
static EXTRACTOR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct Extractor {
    file: usize, // shim handle
    tracks: Vec<TrackInfo>,
    selected: Vec<bool>,
}
type AnyFn = unsafe extern "C" fn();
macro_rules! f {
    ($x:expr) => {
        unsafe { std::mem::transmute::<*const (), AnyFn>($x as *const ()) }
    };
}
/// The implementation, or None after the destructor ran (methods then return Status_NullExtractor).
unsafe fn ext<'a>(this: *mut usize) -> Option<&'a mut Extractor> {
    (*this.add(1) as *mut Extractor).as_mut()
}
unsafe extern "C" fn x_dtor(this: *mut usize) {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if *this.add(1) != 0 {
        let e = Box::from_raw(*this.add(1) as *mut Extractor);
        if e.file != 0 {
            if let Some(s) = shim() {
                (s.close)(e.file as *mut c_void);
            }
        }
        *this.add(1) = 0;
    }
}
unsafe fn x_open(this: *mut usize, uri: *const c_char) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let u = key(uri);
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    let Some(s) = shim() else {
        println!("[movie] {}: no decoder library, no tracks", u);
        return OK;
    };
    let Some(path) = crate::horizon::fs::resolve_path(&u) else {
        return ERR_IO;
    };
    let Ok(cpath) = CString::new(path.display().to_string()) else {
        return ERR_IO;
    };
    let mut n: c_int = 0;
    let h = (s.open)(cpath.as_ptr(), &mut n);
    if h.is_null() {
        println!("[!] movie: cannot open {}", path.display());
        return ERR_IO;
    }
    e.file = h as usize;
    e.tracks = (0..n)
        .map(|i| {
            let mut t = TrackInfo::default();
            (s.track_info)(h, i, &mut t);
            t
        })
        .collect();
    e.selected = vec![false; n as usize];
    println!(
        "[movie] opened {}: {} tracks {:?}",
        u,
        n,
        e.tracks
            .iter()
            .map(|t| (t.ty, t.codec, t.width, t.height, t.sample_rate, t.channels))
            .collect::<Vec<_>>()
    );
    OK
}
unsafe extern "C" fn x_set_source_uri(this: *mut usize, uri: *const c_char) -> u32 {
    x_open(this, uri)
}
unsafe extern "C" fn x_set_source_reader(
    this: *mut usize,
    _reader: usize,
    uri: *const c_char,
) -> u32 {
    x_open(this, uri)
}
unsafe extern "C" fn x_set_source_buffer(_this: *mut usize, _buf: usize, _len: i32) -> u32 {
    ERR_NOT_SUPPORTED
}
unsafe extern "C" fn x_track_count(this: *mut usize, out: *mut i32) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    *out = e.tracks.len() as i32;
    OK
}
fn mime(t: &TrackInfo) -> &'static str {
    match t.codec {
        1 => "video/avc",
        2 => "video/x-vnd.on2.vp8",
        3 => "video/x-vnd.on2.vp9",
        4 => "audio/mp4a-latm",
        5 => "audio/vorbis",
        6 => "audio/opus",
        _ => {
            if t.ty == 1 {
                "video/unknown"
            } else if t.ty == 2 {
                "audio/unknown"
            } else {
                "application/unknown"
            }
        }
    }
}
unsafe extern "C" fn x_track_config(this: *mut usize, idx: i32, md: *mut usize) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    let Some(t) = e.tracks.get(idx as usize).copied() else {
        return TRACK_NOT_FOUND;
    };
    md_set(md, "mime", Val::Str(CString::new(mime(&t)).unwrap()));
    md_set(md, "durationUs", Val::I64(t.duration_us));
    if t.ty == 1 {
        md_set(md, "width", Val::I32(t.width));
        md_set(md, "height", Val::I32(t.height));
        md_set(md, "frame-rate", Val::I32(t.fps.round() as i32));
    } else if t.ty == 2 {
        md_set(md, "sample-rate", Val::I32(t.sample_rate));
        md_set(md, "channel-count", Val::I32(t.channels));
    }
    // Private keys: which file and track a decoder configured with this MediaData decodes.
    md_set(md, "nx-movie-file", Val::I64(e.file as i64));
    md_set(md, "nx-movie-track", Val::I32(idx));
    OK
}
unsafe extern "C" fn x_select(this: *mut usize, idx: i32) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    match e.selected.get_mut(idx as usize) {
        Some(s) => {
            *s = true;
            OK
        }
        None => TRACK_NOT_FOUND,
    }
}
unsafe extern "C" fn x_unselect(this: *mut usize, idx: i32) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    match e.selected.get_mut(idx as usize) {
        Some(s) => {
            *s = false;
            OK
        }
        None => TRACK_NOT_FOUND,
    }
}
unsafe extern "C" fn x_seek(this: *mut usize, us: i64) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    match shim() {
        Some(s) if e.file != 0 => {
            (s.seek)(e.file as *mut c_void, us);
            OK
        }
        _ => OK,
    }
}
/// The current sample of a selected track (skipping the others): (track, size, pts, key).
unsafe fn x_current(e: &mut Extractor) -> Option<(i32, i32, i64, i32)> {
    let s = shim()?;
    if e.file == 0 {
        return None;
    }
    loop {
        let (mut t, mut size, mut pts, mut k) = (0, 0, 0i64, 0);
        if (s.peek)(e.file as *mut c_void, &mut t, &mut size, &mut pts, &mut k) < 0 {
            return None;
        }
        if e.selected.get(t as usize).copied().unwrap_or(false) {
            return Some((t, size, pts, k));
        }
        (s.advance)(e.file as *mut c_void);
    }
}
unsafe extern "C" fn x_track_for_data(this: *mut usize, out: *mut usize) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    match x_current(e) {
        Some((t, ..)) => {
            *out = t as usize;
            OK
        }
        None => END_OF_STREAM,
    }
}
unsafe extern "C" fn x_read(this: *mut usize, b: *mut Buffer) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    let Some((_t, size, pts, _k)) = x_current(e) else {
        return END_OF_STREAM;
    };
    let Some(s) = shim() else {
        return END_OF_STREAM;
    };
    if (*b).data.is_null() || ((*b).capacity as i64) < size as i64 {
        return ERR_BUFFER_TOO_SMALL;
    }
    let n = (s.copy)(
        e.file as *mut c_void,
        (*b).data,
        (*b).capacity.min(i32::MAX as usize) as c_int,
    );
    if n < 0 {
        return if n == -2 {
            ERR_BUFFER_TOO_SMALL
        } else {
            END_OF_STREAM
        };
    }
    (*b).range_offset = 0;
    (*b).range_length = n;
    (*b).int64_data = pts;
    OK
}
unsafe extern "C" fn x_advance(this: *mut usize) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    match shim() {
        Some(s) if e.file != 0 => {
            if x_current(e).is_none() {
                return END_OF_STREAM;
            }
            (s.advance)(e.file as *mut c_void);
            OK
        }
        _ => END_OF_STREAM,
    }
}
unsafe extern "C" fn x_sample_time(this: *mut usize, out: *mut i64) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    match x_current(e) {
        Some((_, _, pts, _)) => {
            *out = pts;
            OK
        }
        None => END_OF_STREAM,
    }
}
unsafe extern "C" fn x_sample_meta(this: *mut usize, md: *mut usize) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    match x_current(e) {
        Some((t, _, pts, k)) => {
            md_set(md, "timeUs", Val::I64(pts));
            md_set(md, "is-sync-frame", Val::I32(k));
            md_set(md, "track-index", Val::I32(t));
            OK
        }
        None => END_OF_STREAM,
    }
}
unsafe extern "C" fn x_buffered_range(this: *mut usize, start: *mut i64, end: *mut i64) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    let d = e.tracks.iter().map(|t| t.duration_us).max().unwrap_or(0);
    *start = 0;
    *end = d;
    OK
}
static EXTRACTOR_VTABLE: [AnyFn; 16] = [
    f!(x_dtor),
    f!(x_dtor), // ~Extractor (complete / deleting: the object's storage is the caller's)
    f!(x_set_source_uri),
    f!(x_set_source_buffer),
    f!(x_track_count),
    f!(x_track_config),
    f!(x_select),
    f!(x_unselect),
    f!(x_seek),
    f!(x_track_for_data),
    f!(x_read),
    f!(x_advance),
    f!(x_sample_time),
    f!(x_sample_meta),
    f!(x_buffered_range),
    f!(x_set_source_reader), // Extractor's own SetDataSource(ClientStreamReader*, const char*)
];
/// The vtables as the guest calls them: entries are HLE trampolines once `set_trampolines` ran.
static GUEST_VTABLES: std::sync::OnceLock<(Vec<usize>, Vec<usize>)> = std::sync::OnceLock::new();

/// Routes the extractor's and decoder's vtables through HLE trampolines (`make` wraps a function), like every other
/// guest-to-engine call.
pub fn set_trampolines(make: &mut dyn FnMut(*const ()) -> usize) {
    let mut wrap = |t: &[AnyFn]| t.iter().map(|&f| make(f as *const ())).collect::<Vec<_>>();
    let tables = (wrap(&EXTRACTOR_VTABLE), wrap(&DECODER_VTABLE));
    let _ = GUEST_VTABLES.set(tables);
}

fn vtable(engine: &'static [AnyFn], decoder: bool) -> usize {
    match GUEST_VTABLES.get() {
        Some((x, d)) => (if decoder { d } else { x }).as_ptr() as usize,
        None => engine.as_ptr() as usize,
    }
}

pub unsafe extern "C" fn extractor_ctor(this: *mut usize) {
    *this = vtable(&EXTRACTOR_VTABLE, false);
    *this.add(1) = Box::into_raw(Box::new(Extractor {
        file: 0,
        tracks: Vec::new(),
        selected: Vec::new(),
    })) as usize;
}

/// Milliseconds since the first movie event (--log-movie timestamps).
fn log_ms() -> u128 {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
}

fn log_enabled() -> bool {
    static L: OnceLock<bool> = OnceLock::new();
    *L.get_or_init(|| crate::settings::get().log.log_movie)
}

// ── horizon::movie::Decoder ──────────────────────────────────────────────────────────────────────────────────────────────
const IN_SLOTS: usize = 4;
const OUT_SLOTS: usize = 16;
const MAX_PENDING: usize = 4;

/// Decoder calls come from the game's input threads (send) and its output side (acquire / release): one lock.
static DECODER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct OutFrame {
    data: Vec<u8>,
    pts: i64,
    flags: u32,
}
struct Decoder {
    out_format: i32,
    events: [usize; 4], // input available, output available, format changed, error
    dec: usize,         // shim decoder
    is_video: bool,
    width: i32,
    height: i32,
    sample_rate: i32,
    channels: i32,
    inputs: Vec<(Vec<u8>, bool)>,   // (memory, free)
    outputs: Vec<Option<OutFrame>>, // slot -> frame handed out (None = free)
    ready: VecDeque<usize>,         // filled output slots not yet acquired
    pending: VecDeque<OutFrame>,    // decoded, waiting for a free output slot
    eos_in: bool,
    eos_out: bool,
    started: bool,
    scratch: Vec<u8>,
    /// --log-movie: inputs sent / outputs acquired / acquire attempts with nothing ready.
    n_in: u64,
    n_out: u64,
    n_empty: u64,
    /// formatChangedEvent signaled for the current output format.
    format_reported: bool,
}
/// The implementation, or None after the destructor ran (methods then return Status_NullDecoder).
unsafe fn dec<'a>(this: *mut usize) -> Option<&'a mut Decoder> {
    (*this.add(1) as *mut Decoder).as_mut()
}
/// Whether AcquireInputBuffer would hand out a buffer (see d_acquire_input).
fn input_available(d: &Decoder) -> bool {
    !d.eos_in && d.pending.len() < MAX_PENDING && d.inputs.iter().any(|i| i.1)
}
/// Signals inputBufferAvailableEvent if an input buffer can be acquired.
fn signal_input(d: &Decoder) {
    if input_available(d) {
        signal(d, 0);
    }
}
/// Signals one of the game's events. A stopped decoder signals nothing: the game tears down the threads that handle
/// them after Stop.
fn signal(d: &Decoder, which: usize) {
    if d.started && d.events[which] != 0 {
        crate::horizon::nn::signal_event(d.events[which]);
    }
}
unsafe extern "C" fn d_dtor(this: *mut usize) {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if *this.add(1) != 0 {
        let d = Box::from_raw(*this.add(1) as *mut Decoder);
        if d.dec != 0 {
            if let Some(s) = shim() {
                (s.md_close)(d.dec as *mut c_void);
            }
        }
        *this.add(1) = 0;
    }
}
unsafe extern "C" fn d_configure(this: *mut usize, md: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let Some(s) = shim() else {
        return ERR_NOT_SUPPORTED;
    };
    let (mut file, mut track) = (0i64, 0i32);
    if !md_find_i64(md, b"nx-movie-file\0".as_ptr() as _, &mut file)
        || !md_find_i32(md, b"nx-movie-track\0".as_ptr() as _, &mut track)
        || file == 0
    {
        println!(
            "[!] movie: Decoder::Configure with a MediaData that did not come from the extractor"
        );
        return ERR_NOT_SUPPORTED;
    }
    let mut t = TrackInfo::default();
    (s.track_info)(file as *mut c_void, track, &mut t);
    d.is_video = t.ty == 1;
    d.width = t.width;
    d.height = t.height;
    d.sample_rate = t.sample_rate;
    d.channels = t.channels;
    let fmt = if d.is_video {
        if d.out_format == 4 {
            4
        } else {
            3
        }
    } else {
        1
    };
    let h = (s.md_open)(file as *mut c_void, track, fmt);
    if h.is_null() {
        println!("[!] movie: no decoder for track {} ({})", track, mime(&t));
        return ERR_NOT_SUPPORTED;
    }
    d.dec = h as usize;
    let in_size = if d.is_video { 4 << 20 } else { 256 << 10 };
    d.inputs = (0..IN_SLOTS).map(|_| (vec![0u8; in_size], true)).collect();
    d.outputs = (0..OUT_SLOTS).map(|_| None).collect();
    println!(
        "[movie] Decoder configured: track {} {} {}x{} {} Hz x{} -> output format {}",
        track,
        mime(&t),
        t.width,
        t.height,
        t.sample_rate,
        t.channels,
        fmt
    );
    OK
}
unsafe extern "C" fn d_register_events(this: *mut usize, ev: *const usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    if !ev.is_null() {
        for i in 0..4 {
            d.events[i] = *ev.add(i);
        }
    }
    OK
}
fn fill_format(d: &Decoder, md: *mut usize) {
    if d.is_video {
        md_set(md, "width", Val::I32(d.width));
        md_set(md, "height", Val::I32(d.height));
        md_set(
            md,
            "stride",
            Val::I32(if d.out_format == 4 {
                d.width
            } else {
                d.width * 4
            }),
        );
        md_set(md, "slice-height", Val::I32(d.height));
        if d.out_format == 4 {
            // Plane offsets inside an NV12 output buffer (the shim writes tightly packed planes).
            md_set(md, "nv12-y-offset", Val::I32(0));
            md_set(md, "nv12-uv-offset", Val::I32(d.width * d.height));
        }
        md_set(md, "crop-left", Val::I32(0));
        md_set(md, "crop-top", Val::I32(0));
        md_set(md, "crop-right", Val::I32(d.width - 1));
        md_set(md, "crop-bottom", Val::I32(d.height - 1));
        md_set(md, "crop", Val::Rect([0, 0, d.width - 1, d.height - 1]));
    } else {
        md_set(md, "sample-rate", Val::I32(d.sample_rate));
        md_set(md, "channel-count", Val::I32(d.channels));
    }
}
unsafe extern "C" fn d_output_format(this: *mut usize, md: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    fill_format(d, md);
    OK
}
unsafe extern "C" fn d_start(this: *mut usize) -> u32 {
    if log_enabled() {
        println!("[movie] t={}ms decoder start", log_ms());
    }
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    d.started = true;
    signal_input(d);
    OK
}
unsafe extern "C" fn d_stop(this: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    d.started = false;
    OK
}
unsafe extern "C" fn d_flush(this: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    if let Some(s) = shim() {
        if d.dec != 0 {
            (s.md_flush)(d.dec as *mut c_void);
        }
    }
    for i in d.inputs.iter_mut() {
        i.1 = true;
    }
    d.outputs.iter_mut().for_each(|o| *o = None);
    d.ready.clear();
    d.pending.clear();
    d.eos_in = false;
    d.eos_out = false;
    signal_input(d);
    OK
}
/// Moves decoded frames into free output slots and signals the game.
fn distribute(d: &mut Decoder) {
    let mut any = false;
    while let Some(slot) = d.outputs.iter().position(|o| o.is_none()) {
        let Some(f) = d.pending.pop_front() else {
            break;
        };
        d.outputs[slot] = Some(f);
        d.ready.push_back(slot);
        any = true;
    }
    if any {
        signal(d, 1);
    }
}
unsafe fn drain(d: &mut Decoder) {
    let Some(s) = shim() else { return };
    loop {
        let (mut pts, mut w, mut h, mut stride, mut need) = (0i64, 0, 0, 0, 0);
        let cap = d.scratch.len() as c_int;
        let n = (s.md_receive)(
            d.dec as *mut c_void,
            d.scratch.as_mut_ptr(),
            cap,
            &mut pts,
            &mut w,
            &mut h,
            &mut stride,
            &mut need,
        );
        if n == -2 {
            d.scratch.resize(need as usize, 0);
            continue;
        }
        if n > 0 {
            // formatChangedEvent before the first output (and on size changes), as MediaCodec-style decoders report the
            // output format first: Kirby's guide movies (no audio track) waited ~5 s for it before starting playback.
            if d.is_video && (w != d.width || h != d.height || !d.format_reported) {
                d.width = w;
                d.height = h;
                d.format_reported = true;
                signal(d, 2);
            } else if !d.is_video && !d.format_reported {
                d.format_reported = true;
                signal(d, 2);
            }
            d.pending.push_back(OutFrame {
                data: d.scratch[..n as usize].to_vec(),
                pts,
                flags: 0,
            });
            continue;
        }
        if n < 0 && d.eos_in && !d.eos_out {
            d.eos_out = true;
            d.pending.push_back(OutFrame {
                data: Vec::new(),
                pts: 0,
                flags: BUFFER_FLAG_EOS,
            });
        }
        break;
    }
    distribute(d);
}
unsafe extern "C" fn d_acquire_input(this: *mut usize, idx: *mut i32, remaining: *mut i32) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    // Back-pressure: hold input while decoded frames wait for output slots. After an end-of-stream input no input
    // buffers are offered until Flush, as with any MediaCodec-style decoder (the game's input threads idle on that).
    let free: Vec<usize> = if d.eos_in || d.pending.len() >= MAX_PENDING {
        Vec::new()
    } else {
        d.inputs
            .iter()
            .enumerate()
            .filter(|(_, i)| i.1)
            .map(|(n, _)| n)
            .collect()
    };
    if !remaining.is_null() {
        *remaining = free.len().saturating_sub(1) as i32;
    }
    match free.first() {
        Some(&i) => {
            *idx = i as i32;
            OK
        }
        None => INPUT_NOT_AVAILABLE,
    }
}
unsafe extern "C" fn d_get_input(this: *mut usize, idx: i32, b: *mut Buffer) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let Some(slot) = d.inputs.get_mut(idx as usize) else {
        return ERR_INVALID_RANGE;
    };
    slot.1 = false;
    buffer_set_data_and_capacity(b, slot.0.as_mut_ptr(), slot.0.len());
    (*b).range_offset = 0;
    (*b).range_length = 0;
    OK
}
unsafe extern "C" fn d_send_input(
    this: *mut usize,
    idx: i32,
    off: i32,
    size: i32,
    pts: i64,
    flags: u32,
) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let Some(s) = shim() else { return ERR_UNKNOWN };
    let Some(slot) = d.inputs.get_mut(idx as usize) else {
        return ERR_INVALID_RANGE;
    };
    let eos = flags & BUFFER_FLAG_EOS != 0;
    let data = slot.0.as_ptr().add(off.max(0) as usize);
    let size = size.max(0).min(slot.0.len() as i32 - off.max(0));
    slot.1 = true;
    if eos {
        d.eos_in = true;
    }
    if d.dec != 0 {
        if (s.md_send)(d.dec as *mut c_void, data, size, pts, eos as c_int) < 0 {
            signal(d, 3);
        }
        drain(d);
    }
    d.n_in += 1;
    if log_enabled() && (d.n_in <= 5 || d.n_in % 100 == 0 || eos) {
        println!(
            "[movie] t={}ms {} input #{} pts {} size {} eos {} -> pending {} ready {}",
            log_ms(),
            if d.is_video { "video" } else { "audio" },
            d.n_in,
            pts,
            size,
            eos,
            d.pending.len(),
            d.ready.len()
        );
    }
    signal_input(d);
    OK
}
unsafe extern "C" fn d_acquire_output(
    this: *mut usize,
    idx: *mut i32,
    pts: *mut i64,
    flags: *mut u32,
    remaining: *mut i32,
) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let Some(slot) = d.ready.pop_front() else {
        d.n_empty += 1;
        return OUTPUT_NOT_AVAILABLE;
    };
    let f = d.outputs[slot].as_ref().unwrap();
    d.n_out += 1;
    if log_enabled() && (d.n_out <= 5 || d.n_out % 60 == 0 || f.flags != 0) {
        println!("[movie] t={}ms {} output #{}: slot {} pts {} us, {} bytes, flags {:#x} (empty polls {})", log_ms(),
            if d.is_video { "video" } else { "audio" }, d.n_out, slot, f.pts, f.data.len(), f.flags, d.n_empty);
    }
    *idx = slot as i32;
    if !pts.is_null() {
        *pts = f.pts;
    }
    if !flags.is_null() {
        *flags = f.flags;
    }
    if !remaining.is_null() {
        *remaining = d.ready.len() as i32;
    }
    OK
}
unsafe extern "C" fn d_get_output(this: *mut usize, idx: i32, b: *mut Buffer) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let Some(Some(f)) = d.outputs.get_mut(idx as usize) else {
        return ERR_INVALID_RANGE;
    };
    // "Client provided buffer to copy output data": the output is copied into the client's memory. A buffer
    // without memory gets a view of the decoder's own copy instead.
    if log_enabled() && d.is_video {
        println!(
            "[movie] GetOutputBuffer({}) into client {:p} cap {:#x} ({} bytes)",
            idx,
            (*b).data,
            (*b).capacity,
            f.data.len()
        );
    }
    if !(*b).data.is_null() {
        if (*b).capacity < f.data.len() {
            return ERR_BUFFER_TOO_SMALL;
        }
        std::ptr::copy_nonoverlapping(f.data.as_ptr(), (*b).data, f.data.len());
    } else {
        buffer_set_data_and_capacity(b, f.data.as_mut_ptr(), f.data.len());
    }
    (*b).range_offset = 0;
    (*b).range_length = f.data.len() as i32;
    (*b).int64_data = f.pts;
    OK
}
unsafe extern "C" fn d_get_output_with_format(
    this: *mut usize,
    idx: i32,
    b: *mut Buffer,
    md: *mut usize,
) -> u32 {
    let r = d_get_output(this, idx, b); // takes DECODER_LOCK itself
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if r == OK && !md.is_null() {
        if let Some(d) = dec(this) {
            fill_format(d, md);
        }
    }
    r
}
unsafe extern "C" fn d_output_buffer_format(this: *mut usize, _idx: i32, md: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    fill_format(d, md);
    OK
}
unsafe extern "C" fn d_output_size(this: *mut usize, size: *mut i32, align: *mut i32) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let s = if d.is_video {
        if d.out_format == 4 {
            d.width * d.height * 3 / 2
        } else {
            d.width * d.height * 4
        }
    } else {
        64 << 10
    };
    if !size.is_null() {
        *size = s;
    }
    if !align.is_null() {
        *align = 256;
    }
    OK
}
unsafe extern "C" fn d_release_output(this: *mut usize, idx: i32) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    match d.outputs.get_mut(idx as usize) {
        Some(o) => *o = None,
        None => return ERR_INVALID_RANGE,
    }
    // Input becomes available here only if the back-pressure on pending frames held it (the event reports a change,
    // not a level: signalling it on every release woke the game's input handler after it had shut down).
    let held = !input_available(d);
    distribute(d);
    if held {
        signal_input(d);
    }
    OK
}
unsafe extern "C" fn d_last_error(_this: *mut usize) -> u32 {
    OK
}
static DECODER_VTABLE: [AnyFn; 19] = [
    f!(d_dtor),
    f!(d_dtor),
    f!(d_configure),
    f!(d_register_events),
    f!(d_output_format),
    f!(d_start),
    f!(d_stop),
    f!(d_flush),
    f!(d_flush),
    f!(d_acquire_input),
    f!(d_get_input),
    f!(d_send_input),
    f!(d_acquire_output),
    f!(d_get_output),
    f!(d_output_size),
    f!(d_release_output),
    f!(d_last_error),
    f!(d_get_output_with_format),
    f!(d_output_buffer_format),
];
pub unsafe extern "C" fn decoder_ctor(this: *mut usize, ty: i32, format: i32, mode: i32, _n: i32) {
    println!(
        "[movie] Decoder(type={}, output format={}, mode={})",
        ty, format, mode
    );
    *this = vtable(&DECODER_VTABLE, true);
    let d = Decoder {
        out_format: format,
        events: [0; 4],
        dec: 0,
        is_video: false,
        width: 0,
        height: 0,
        sample_rate: 0,
        channels: 0,
        inputs: Vec::new(),
        outputs: Vec::new(),
        ready: VecDeque::new(),
        pending: VecDeque::new(),
        eos_in: false,
        eos_out: false,
        started: false,
        scratch: vec![0u8; 1 << 20],
        n_in: 0,
        n_out: 0,
        n_empty: 0,
        format_reported: false,
    };
    *this.add(1) = Box::into_raw(Box::new(d)) as usize;
}

unsafe extern "C" fn thread_priority() -> i32 {
    16
} // nn::os default priority
unsafe extern "C" fn noop() {}

/// Mangled name -> HLE function for the horizon::movie:: symbols (chained by horizon::sdk::lookup).
pub fn lookup(name: &str) -> Option<usize> {
    macro_rules! a {
        ($x:expr) => {
            Some($x as *const () as usize)
        };
    }
    match name {
        "_ZN5movie9ExtractorC1ENS_13ContainerTypeE"
        | "_ZN5movie9ExtractorC1ENS_13ContainerTypeENS_9CacheSizeE" => a!(extractor_ctor),
        "_ZN5movie7DecoderC1ENS_11DecoderTypeENS_19DecoderOutputFormatENS_11DecoderModeEi" => a!(decoder_ctor),
        "_ZN5movie9MediaDataC1Ev" => a!(md_ctor),
        "_ZN5movie9MediaDataD1Ev" => a!(md_dtor),
        "_ZN5movie9MediaDataC1ERKS0_" => a!(md_copy_ctor),
        "_ZN5movie9MediaDataaSERKS0_" => a!(md_assign),
        "_ZN5movie9MediaData5ClearEv" => a!(md_clear),
        "_ZN5movie9MediaData8SetInt32EPKci" => a!(md_set_i32),
        "_ZN5movie9MediaData8SetInt64EPKcl" => a!(md_set_i64),
        "_ZN5movie9MediaData9SetStringEPKcS2_" => a!(md_set_string),
        "_ZNK5movie9MediaData8ContainsEPKc" => a!(md_contains),
        "_ZN5movie9MediaData7SetRectEPKciiii" => a!(md_set_rect),
        "_ZNK5movie9MediaData8FindRectEPKcPiS3_S3_S3_" => a!(md_find_rect),
        "_ZNK5movie9MediaData9FindInt32EPKcPi" => a!(md_find_i32),
        "_ZNK5movie9MediaData9FindInt64EPKcPl" => a!(md_find_i64),
        "_ZNK5movie9MediaData10FindStringEPKcPS2_" => a!(md_find_string),
        "_ZNK5movie9MediaData12CountEntriesEv" => a!(md_count),
        "_ZN5movie6BufferC1Ev" => a!(buffer_ctor),
        "_ZN5movie6BufferC1Em" => a!(buffer_ctor_capacity),
        "_ZN5movie6BufferC1EPvm" => a!(buffer_ctor_data),
        "_ZN5movie6BufferC1ERKS0_" => a!(buffer_ctor_copy),
        "_ZN5movie6BufferaSERKS0_" => a!(buffer_assign),
        "_ZN5movie6BufferD1Ev" => a!(buffer_dtor),
        "_ZNK5movie6Buffer4DataEv" => a!(buffer_data),
        "_ZNK5movie6Buffer4BaseEv" => a!(buffer_base),
        "_ZNK5movie6Buffer8CapacityEv" => a!(buffer_capacity),
        "_ZNK5movie6Buffer6OffsetEv" => a!(buffer_offset),
        "_ZNK5movie6Buffer4SizeEv" => a!(buffer_size),
        "_ZN5movie6Buffer18SetDataAndCapacityEPvm" => a!(buffer_set_data_and_capacity),
        "_ZN5movie6Buffer11SetCapacityEm" => a!(buffer_set_capacity),
        "_ZN5movie6Buffer8SetRangeEii" => a!(buffer_set_range),
        "_ZN5movie6Buffer12SetInt32DataEi" => a!(buffer_set_i32),
        "_ZNK5movie6Buffer12GetInt32DataEv" => a!(buffer_get_i32),
        "_ZN5movie6Buffer12SetInt64DataEl" => a!(buffer_set_i64),
        "_ZNK5movie6Buffer12GetInt64DataEv" => a!(buffer_get_i64),
        "_ZN5movie6Buffer13SetUint32DataEj" => a!(buffer_set_u32),
        "_ZNK5movie6Buffer13GetUint32DataEv" => a!(buffer_get_u32),
        "_ZN5movie17GetThreadPriorityEv" => a!(thread_priority),
        "_ZN5movie12SetAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_"
        | "_ZN5movie11SetCoreMaskEm" => a!(noop),
        _ => None,
    }
}
