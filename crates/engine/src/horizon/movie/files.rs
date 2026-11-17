// Movie files through crates/media: the extractor and decoder hold their file and decoder as handles and talk to them
// through these functions.

use super::*;

// ── Movie files (crates/media) ─────────────────────────────────────────────────────────────────────────────────────
// The extractor and decoder below hold their file and decoder as handles (a boxed nxport_media::Source / Decoder) and
// talk to them through these functions: sizes and statuses as ints, -1 at the end, -2 when a buffer is too small.
#[derive(Default, Clone, Copy)]
pub(crate) struct TrackInfo {
    pub(crate) ty: c_int,
    pub(crate) codec: c_int,
    pub(crate) width: c_int,
    pub(crate) height: c_int,
    pub(crate) fps: f64,
    pub(crate) sample_rate: c_int,
    pub(crate) channels: c_int,
    pub(crate) duration_us: i64,
}

pub(crate) struct Media {
    pub(crate) open: unsafe fn(*const c_char, *mut c_int) -> *mut c_void,
    pub(crate) track_info: unsafe fn(*mut c_void, c_int, *mut TrackInfo) -> c_int,
    pub(crate) peek: unsafe fn(*mut c_void, *mut c_int, *mut c_int, *mut i64, *mut c_int) -> c_int,
    pub(crate) copy: unsafe fn(*mut c_void, *mut u8, c_int) -> c_int,
    pub(crate) advance: unsafe fn(*mut c_void),
    pub(crate) seek: unsafe fn(*mut c_void, i64) -> c_int,
    pub(crate) close: unsafe fn(*mut c_void),
    pub(crate) md_open: unsafe fn(*mut c_void, c_int, Format) -> *mut c_void,
    pub(crate) md_send: unsafe fn(*mut c_void, *const u8, c_int, i64, c_int) -> c_int,
    pub(crate) md_receive: unsafe fn(*mut c_void, *mut u8, c_int, *mut i64, *mut c_int, *mut c_int, *mut c_int, *mut c_int) -> c_int,
    pub(crate) md_flush: unsafe fn(*mut c_void),
    pub(crate) md_close: unsafe fn(*mut c_void),
}

pub(crate) fn media() -> &'static Media {
    use nxport_media::{Codec, Decoder, Kind, Output, Source};
    unsafe fn src<'a>(h: *mut c_void) -> &'a mut Source {
        &mut *(h as *mut Source)
    }
    unsafe fn dec<'a>(h: *mut c_void) -> &'a mut Decoder {
        &mut *(h as *mut Decoder)
    }
    static MEDIA: Media = Media {
        open: |path, n| unsafe {
            nxport_media::set_hardware_video(crate::settings::get().graphics.hw_video);
            let p = CStr::from_ptr(path).to_string_lossy().into_owned();
            match Source::open(std::path::Path::new(&p)) {
                Ok(s) => {
                    *n = s.tracks().len() as c_int;
                    Box::into_raw(Box::new(s)) as *mut c_void
                }
                Err(e) => {
                    println!("[!] movie: {e}");
                    std::ptr::null_mut()
                }
            }
        },
        track_info: |h, i, out| unsafe {
            let Some(t) = src(h).tracks().get(i as usize) else { return -1 };
            *out = TrackInfo {
                ty: match t.kind {
                    Kind::Video => 1,
                    Kind::Audio => 2,
                    Kind::Other => 0,
                },
                codec: match t.codec {
                    Codec::Avc => 1,
                    Codec::Vp8 => 2,
                    Codec::Vp9 => 3,
                    Codec::Aac => 4,
                    Codec::Vorbis => 5,
                    Codec::Opus => 6,
                    Codec::Unknown => 0,
                },
                width: t.width as c_int,
                height: t.height as c_int,
                fps: t.fps,
                sample_rate: t.sample_rate as c_int,
                channels: t.channels as c_int,
                duration_us: t.duration_us,
            };
            0
        },
        peek: |h, track, size, pts, key| unsafe {
            let Some(s) = src(h).peek() else { return -1 };
            (*track, *size, *pts, *key) = (s.track as c_int, s.data.len() as c_int, s.pts_us, s.key as c_int);
            0
        },
        copy: |h, buf, cap| unsafe {
            let Some(s) = src(h).peek() else { return -1 };
            if s.data.len() > cap as usize {
                return -2;
            }
            std::ptr::copy_nonoverlapping(s.data.as_ptr(), buf, s.data.len());
            s.data.len() as c_int
        },
        advance: |h| unsafe { src(h).advance() },
        seek: |h, us| unsafe {
            if src(h).seek(us) {
                0
            } else {
                -1
            }
        },
        close: |h| unsafe { drop(Box::from_raw(h as *mut Source)) },
        md_open: |file, track, format| unsafe {
            let Some(t) = src(file).tracks().get(track as usize) else { return std::ptr::null_mut() };
            match Decoder::new(t, format) {
                Ok(d) => Box::into_raw(Box::new(d)) as *mut c_void,
                Err(e) => {
                    println!("[!] movie: {e}");
                    std::ptr::null_mut()
                }
            }
        },
        md_send: |h, data, size, pts, eos| unsafe {
            let data = if size > 0 { std::slice::from_raw_parts(data, size as usize) } else { &[] };
            match dec(h).send(data, pts, eos != 0) {
                Ok(()) => 0,
                Err(e) => {
                    println!("[!] movie: {e}");
                    -1
                }
            }
        },
        md_receive: |h, out, cap, pts, w, hh, stride, need| unsafe {
            let buf = std::slice::from_raw_parts_mut(out, cap.max(0) as usize);
            match dec(h).receive(buf) {
                Output::Picture { pts_us, width, height, stride: s, size } => {
                    (*pts, *w, *hh, *stride) = (pts_us, width as c_int, height as c_int, s as c_int);
                    size as c_int
                }
                Output::Audio { pts_us, frames, channels, size } => {
                    (*pts, *w, *hh, *stride) = (pts_us, frames as c_int, channels as c_int, 0);
                    size as c_int
                }
                Output::NeedInput => 0,
                Output::End => -1,
                Output::TooSmall(n) => {
                    *need = n as c_int;
                    -2
                }
            }
        },
        md_flush: |h| unsafe { dec(h).flush() },
        md_close: |h| unsafe { drop(Box::from_raw(h as *mut Decoder)) },
    };
    &MEDIA
}
