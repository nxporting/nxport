// horizon::movie::Extractor.

use super::*;

// ── horizon::movie::Extractor ────────────────────────────────────────────────────────────────────────────────────────────
/// One lock for all extractor calls: Kirby shares one Extractor between its audio and video input threads (each takes
/// the samples of its own track), and the demuxer behind it is not thread-safe.
pub(crate) static EXTRACTOR_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) struct Extractor {
    file: usize, // media file handle
    tracks: Vec<TrackInfo>,
    selected: Vec<bool>,
}
/// The implementation, or None after the destructor ran (methods then return Status_NullExtractor).
pub(crate) unsafe fn ext<'a>(this: *mut usize) -> Option<&'a mut Extractor> {
    (*this.add(1) as *mut Extractor).as_mut()
}
pub(crate) unsafe extern "C" fn x_dtor(this: *mut usize) {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if *this.add(1) != 0 {
        let e = Box::from_raw(*this.add(1) as *mut Extractor);
        if e.file != 0 {
            (media().close)(e.file as *mut c_void);
        }
        *this.add(1) = 0;
    }
}
pub(crate) unsafe fn x_open(this: *mut usize, uri: *const c_char) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let u = key(uri);
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    let s = media();
    let Some(path) = crate::horizon::fs::resolve_to_file(&u) else {
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
pub(crate) unsafe extern "C" fn x_set_source_uri(this: *mut usize, uri: *const c_char) -> u32 {
    x_open(this, uri)
}
pub(crate) unsafe extern "C" fn x_set_source_reader(
    this: *mut usize,
    _reader: usize,
    uri: *const c_char,
) -> u32 {
    x_open(this, uri)
}
pub(crate) unsafe extern "C" fn x_set_source_buffer(_this: *mut usize, _buf: usize, _len: i32) -> u32 {
    ERR_NOT_SUPPORTED
}
pub(crate) unsafe extern "C" fn x_track_count(this: *mut usize, out: *mut i32) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    *out = e.tracks.len() as i32;
    OK
}
pub(crate) fn mime(t: &TrackInfo) -> &'static str {
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
pub(crate) unsafe extern "C" fn x_track_config(this: *mut usize, idx: i32, md: *mut usize) -> u32 {
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
pub(crate) unsafe extern "C" fn x_select(this: *mut usize, idx: i32) -> u32 {
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
pub(crate) unsafe extern "C" fn x_unselect(this: *mut usize, idx: i32) -> u32 {
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
pub(crate) unsafe extern "C" fn x_seek(this: *mut usize, us: i64) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    if e.file != 0 {
        (media().seek)(e.file as *mut c_void, us);
    }
    OK
}
/// The current sample of a selected track (skipping the others): (track, size, pts, key).
pub(crate) unsafe fn x_current(e: &mut Extractor) -> Option<(i32, i32, i64, i32)> {
    let s = media();
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
pub(crate) unsafe extern "C" fn x_track_for_data(this: *mut usize, out: *mut usize) -> u32 {
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
pub(crate) unsafe extern "C" fn x_read(this: *mut usize, b: *mut Buffer) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    let Some((_t, size, pts, _k)) = x_current(e) else {
        return END_OF_STREAM;
    };
    let s = media();
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
pub(crate) unsafe extern "C" fn x_advance(this: *mut usize) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    if e.file == 0 || x_current(e).is_none() {
        return END_OF_STREAM;
    }
    (media().advance)(e.file as *mut c_void);
    OK
}
pub(crate) unsafe extern "C" fn x_sample_time(this: *mut usize, out: *mut i64) -> u32 {
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
pub(crate) unsafe extern "C" fn x_sample_meta(this: *mut usize, md: *mut usize) -> u32 {
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
pub(crate) unsafe extern "C" fn x_buffered_range(this: *mut usize, start: *mut i64, end: *mut i64) -> u32 {
    let _guard = EXTRACTOR_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(e) = ext(this) else {
        return NULL_EXTRACTOR;
    };
    let d = e.tracks.iter().map(|t| t.duration_us).max().unwrap_or(0);
    *start = 0;
    *end = d;
    OK
}
guest_vtable!(EXTRACTOR_VTABLE, register_extractor_hle, [
    x_dtor(_),
    x_dtor(_), // ~Extractor (complete / deleting: the object's storage is the caller's)
    x_set_source_uri(_, _),
    x_set_source_buffer(_, _, _),
    x_track_count(_, _),
    x_track_config(_, _, _),
    x_select(_, _),
    x_unselect(_, _),
    x_seek(_, _),
    x_track_for_data(_, _),
    x_read(_, _),
    x_advance(_),
    x_sample_time(_, _),
    x_sample_meta(_, _),
    x_buffered_range(_, _, _),
    x_set_source_reader(_, _, _), // Extractor's own SetDataSource(ClientStreamReader*, const char*)
]);
/// The vtables as the guest calls them: entries are HLE trampolines once `set_trampolines` ran.
pub(crate) static GUEST_VTABLES: std::sync::OnceLock<(Vec<usize>, Vec<usize>)> = std::sync::OnceLock::new();

/// Routes the extractor's and decoder's vtables through HLE trampolines (`make` wraps a function), like every other
/// guest-to-engine call.
pub fn set_trampolines(make: &mut dyn FnMut(*const ()) -> usize) {
    register_extractor_hle();
    register_decoder_hle();
    let mut wrap = |t: &[AnyFn]| t.iter().map(|&f| make(f as *const ())).collect::<Vec<_>>();
    let tables = (wrap(&EXTRACTOR_VTABLE), wrap(&DECODER_VTABLE));
    let _ = GUEST_VTABLES.set(tables);
}

pub(crate) fn vtable(engine: &'static [AnyFn], decoder: bool) -> usize {
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
pub(crate) fn log_ms() -> u128 {
    static T0: OnceLock<std::time::Instant> = OnceLock::new();
    T0.get_or_init(std::time::Instant::now)
        .elapsed()
        .as_millis()
}

pub(crate) fn log_enabled() -> bool {
    static L: OnceLock<bool> = OnceLock::new();
    *L.get_or_init(|| crate::settings::get().log.log_movie)
}
