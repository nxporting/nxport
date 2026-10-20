// Real Opus decoding for the HLE'd hardware Opus decoder (horizon/hle.rs, nn::codec::DecodeOpusInterleavedWithHardware): TotK
// streams its music and ambience as Opus and mixes them on the CPU, so a decoder that only accounted for samples made
// every stream silent. libopus is loaded at run time (--libopus, else Homebrew's or the system's libopus.dylib), so
// the engine builds and runs without it; when it is missing, decoding falls back to silence with correct accounting.
//
// One libopus decoder per guest HardwareOpusDecoderType (keyed by its address), created on first use with the guest's
// sample rate and channel count, reset by ResetHardwareOpusDecoder and destroyed by FinalizeHardwareOpusDecoder.

use std::collections::HashMap;
use std::ffi::{c_char, c_int, c_void, CString};
use std::sync::{Mutex, OnceLock};

const OPUS_RESET_STATE: c_int = 4028;

struct Lib {
    create: unsafe extern "C" fn(i32, c_int, *mut c_int) -> *mut c_void,
    decode: unsafe extern "C" fn(*mut c_void, *const u8, i32, *mut i16, c_int, c_int) -> c_int,
    ctl: unsafe extern "C" fn(*mut c_void, c_int, ...) -> c_int,
    destroy: unsafe extern "C" fn(*mut c_void),
}

extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

fn lib() -> Option<&'static Lib> {
    static LIB: OnceLock<Option<Lib>> = OnceLock::new();
    LIB.get_or_init(|| unsafe {
        let candidates = crate::settings::get()
            .files
            .libopus
            .iter()
            .map(|p| p.display().to_string())
            .chain(
                [
                    "/opt/homebrew/lib/libopus.0.dylib",
                    "/opt/homebrew/opt/opus/lib/libopus.0.dylib",
                    "/usr/local/lib/libopus.0.dylib",
                    "libopus.0.dylib",
                ]
                .iter()
                .map(|s| s.to_string()),
            );
        for path in candidates {
            let c = CString::new(path.clone()).ok()?;
            let h = dlopen(c.as_ptr(), 2); // RTLD_NOW
            if h.is_null() {
                continue;
            }
            let sym = |name: &str| dlsym(h, CString::new(name).unwrap().as_ptr());
            let (create, decode, ctl, destroy) = (
                sym("opus_decoder_create"),
                sym("opus_decode"),
                sym("opus_decoder_ctl"),
                sym("opus_decoder_destroy"),
            );
            if [create, decode, ctl, destroy].iter().any(|p| p.is_null()) {
                continue;
            }
            println!("[+] opus: decoding with {}", path);
            return Some(Lib {
                create: std::mem::transmute::<*mut c_void, _>(create),
                decode: std::mem::transmute::<*mut c_void, _>(decode),
                ctl: std::mem::transmute::<*mut c_void, _>(ctl),
                destroy: std::mem::transmute::<*mut c_void, _>(destroy),
            });
        }
        println!("[!] opus: libopus not found (--libopus): Opus streams decode to silence");
        None
    })
    .as_ref()
}

/// Guest decoder address -> libopus decoder.
static DECODERS: Mutex<Option<HashMap<usize, usize>>> = Mutex::new(None);

/// Decodes one packet into interleaved 16-bit samples; returns samples per channel, or None (caller writes silence).
pub unsafe fn decode(
    guest: usize,
    rate: i32,
    channels: i32,
    packet: &[u8],
    out: *mut i16,
    out_frames_max: usize,
) -> Option<usize> {
    let lib = lib()?;
    let mut map = DECODERS.lock().unwrap();
    let map = map.get_or_insert_with(HashMap::new);
    let dec = match map.get(&guest) {
        Some(&d) => d as *mut c_void,
        None => {
            let mut err: c_int = 0;
            let d = (lib.create)(rate, channels, &mut err);
            if d.is_null() || err != 0 {
                println!(
                    "[!] opus: opus_decoder_create({} Hz, {} ch) failed ({})",
                    rate, channels, err
                );
                return None;
            }
            map.insert(guest, d as usize);
            d
        }
    };
    let n = (lib.decode)(
        dec,
        packet.as_ptr(),
        packet.len() as i32,
        out,
        out_frames_max as c_int,
        0,
    );
    if n < 0 {
        static REPORTED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        if REPORTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < 5 {
            println!(
                "[!] opus: opus_decode failed ({}) for a {}-byte packet",
                n,
                packet.len()
            );
        }
        return None;
    }
    Some(n as usize)
}

pub fn reset(guest: usize) {
    let Some(lib) = lib() else { return };
    if let Some(&d) = DECODERS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(&guest))
    {
        unsafe { (lib.ctl)(d as *mut c_void, OPUS_RESET_STATE) };
    }
}

pub fn destroy(guest: usize) {
    let Some(lib) = lib() else { return };
    if let Some(d) = DECODERS
        .lock()
        .unwrap()
        .as_mut()
        .and_then(|m| m.remove(&guest))
    {
        unsafe { (lib.destroy)(d as *mut c_void) };
    }
}
