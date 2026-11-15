// VP9 through VideoToolbox: the system's video decoder hardware (Apple GPUs decode VP9; macOS registers the decoder on
// request). The console decodes movies in hardware too; decoding 1080p VP9 on the CPU took every core of an iPhone for
// ~12 ms a frame, beside the game. Only a hardware decoder is used (RequireHardwareAcceleratedVideoDecoder): without
// one, Session::new fails and the caller decodes in software.

use std::collections::VecDeque;
use std::ffi::c_void;

use crate::Decoded;

type CFTypeRef = *const c_void;
type CFAllocatorRef = *const c_void;
type CFStringRef = *const c_void;
type CFDictionaryRef = *const c_void;
type CFMutableDictionaryRef = *mut c_void;
type OSStatus = i32;

#[repr(C)]
#[derive(Clone, Copy)]
struct CMTime {
    value: i64,
    timescale: i32,
    flags: u32,
    epoch: i64,
}
const CM_TIME_VALID: u32 = 1;
const INVALID_TIME: CMTime = CMTime { value: 0, timescale: 0, flags: 0, epoch: 0 };

#[repr(C)]
struct CMSampleTimingInfo {
    duration: CMTime,
    presentation: CMTime,
    decode: CMTime,
}

type OutputCallback = unsafe extern "C" fn(*mut c_void, *mut c_void, OSStatus, u32, CFTypeRef, CMTime, CMTime);

#[repr(C)]
struct OutputCallbackRecord {
    callback: OutputCallback,
    refcon: *mut c_void,
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    static kCFBooleanTrue: CFTypeRef;
    fn CFDictionaryCreateMutable(a: CFAllocatorRef, cap: isize, k: *const c_void, v: *const c_void) -> CFMutableDictionaryRef;
    fn CFDictionarySetValue(d: CFMutableDictionaryRef, k: CFTypeRef, v: CFTypeRef);
    fn CFNumberCreate(a: CFAllocatorRef, ty: isize, value: *const c_void) -> CFTypeRef;
    fn CFDataCreate(a: CFAllocatorRef, bytes: *const u8, len: isize) -> CFTypeRef;
    fn CFStringCreateWithCString(a: CFAllocatorRef, s: *const i8, encoding: u32) -> CFStringRef;
    fn CFRelease(o: CFTypeRef);
}

#[link(name = "CoreMedia", kind = "framework")]
extern "C" {
    static kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms: CFStringRef;
    fn CMVideoFormatDescriptionCreate(a: CFAllocatorRef, codec: u32, w: i32, h: i32, ext: CFDictionaryRef, out: *mut CFTypeRef) -> OSStatus;
    fn CMBlockBufferCreateWithMemoryBlock(
        a: CFAllocatorRef,
        block: *mut c_void,
        len: usize,
        block_allocator: CFAllocatorRef,
        source: *const c_void,
        offset: usize,
        data_len: usize,
        flags: u32,
        out: *mut CFTypeRef,
    ) -> OSStatus;
    fn CMBlockBufferReplaceDataBytes(src: *const c_void, dst: CFTypeRef, offset: usize, len: usize) -> OSStatus;
    fn CMSampleBufferCreateReady(
        a: CFAllocatorRef,
        data: CFTypeRef,
        format: CFTypeRef,
        samples: isize,
        timings: isize,
        timing: *const CMSampleTimingInfo,
        sizes: isize,
        size: *const usize,
        out: *mut CFTypeRef,
    ) -> OSStatus;
}

#[link(name = "CoreVideo", kind = "framework")]
extern "C" {
    static kCVPixelBufferPixelFormatTypeKey: CFStringRef;
    fn CVPixelBufferLockBaseAddress(b: CFTypeRef, flags: u64) -> i32;
    fn CVPixelBufferUnlockBaseAddress(b: CFTypeRef, flags: u64) -> i32;
    fn CVPixelBufferGetBaseAddressOfPlane(b: CFTypeRef, plane: usize) -> *const u8;
    fn CVPixelBufferGetBytesPerRowOfPlane(b: CFTypeRef, plane: usize) -> usize;
    fn CVPixelBufferGetWidthOfPlane(b: CFTypeRef, plane: usize) -> usize;
    fn CVPixelBufferGetHeightOfPlane(b: CFTypeRef, plane: usize) -> usize;
}

#[link(name = "VideoToolbox", kind = "framework")]
extern "C" {
    static kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder: CFStringRef;
    fn VTDecompressionSessionCreate(
        a: CFAllocatorRef,
        format: CFTypeRef,
        spec: CFDictionaryRef,
        attrs: CFDictionaryRef,
        callback: *const OutputCallbackRecord,
        out: *mut CFTypeRef,
    ) -> OSStatus;
    fn VTDecompressionSessionDecodeFrame(s: CFTypeRef, sample: CFTypeRef, flags: u32, refcon: *mut c_void, info: *mut u32) -> OSStatus;
    fn VTDecompressionSessionWaitForAsynchronousFrames(s: CFTypeRef) -> OSStatus;
    fn VTDecompressionSessionInvalidate(s: CFTypeRef);
    #[cfg(target_os = "macos")]
    fn VTRegisterSupplementalVideoDecoderIfAvailable(codec: u32);
}

const VP09: u32 = u32::from_be_bytes(*b"vp09");
/// kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange ('420v'): Y, then interleaved CbCr at half size.
const NV12: i32 = i32::from_be_bytes(*b"420v");
const CF_NUMBER_SINT32: isize = 3;
const CF_STRING_UTF8: u32 = 0x0800_0100;
const LOCK_READ_ONLY: u64 = 1;

/// What a VP9 key frame's uncompressed header says about the stream (VP9 bitstream spec 6.2).
pub(crate) struct KeyFrame {
    pub profile: u8,
    pub bit_depth: u8,
    pub width: u32,
    pub height: u32,
}

/// The first coded frame of `data` (a superframe's frames are in order; its index is at the end) if it is a key frame.
pub(crate) fn key_frame(data: &[u8]) -> Option<KeyFrame> {
    let mut pos = 0usize;
    let mut bit = |n: u32| -> Option<u32> {
        let mut v = 0;
        for _ in 0..n {
            let b = (*data.get(pos / 8)? >> (7 - pos % 8)) & 1;
            v = v << 1 | b as u32;
            pos += 1;
        }
        Some(v)
    };
    if bit(2)? != 2 {
        return None;
    }
    let lo = bit(1)?;
    let profile = (bit(1)? << 1 | lo) as u8;
    if profile == 3 {
        bit(1)?;
    }
    if bit(1)? != 0 || bit(1)? != 0 {
        return None; // show_existing_frame, or not a key frame
    }
    bit(2)?; // show_frame, error_resilient_mode
    if bit(24)? != 0x49_83_42 {
        return None;
    }
    let bit_depth = if profile >= 2 { if bit(1)? == 1 { 12 } else { 10 } } else { 8 };
    if bit(3)? != 7 {
        bit(1)?; // color_range
        if profile == 1 || profile == 3 {
            bit(3)?;
        }
    } else if profile == 1 || profile == 3 {
        bit(1)?;
    }
    let width = bit(16)? + 1;
    let height = bit(16)? + 1;
    Some(KeyFrame { profile, bit_depth, width, height })
}

pub(crate) struct Session {
    session: CFTypeRef,
    format: CFTypeRef,
    /// Pictures the output callback handed over during the last decode (the callback's refcon).
    out: Box<Vec<Result<Decoded, OSStatus>>>,
}

// SAFETY: a decompression session may be used from any thread, one call at a time (the decoder is behind a lock).
unsafe impl Send for Session {}

unsafe extern "C" fn on_output(refcon: *mut c_void, _frame: *mut c_void, status: OSStatus, _info: u32, image: CFTypeRef, pts: CMTime, _dur: CMTime) {
    let out = &mut *(refcon as *mut Vec<Result<Decoded, OSStatus>>);
    if status != 0 {
        out.push(Err(status));
        return;
    }
    if image.is_null() {
        return; // a frame that is not shown
    }
    if CVPixelBufferLockBaseAddress(image, LOCK_READ_ONLY) != 0 {
        out.push(Err(-1));
        return;
    }
    let plane = |p: usize| {
        (
            CVPixelBufferGetBaseAddressOfPlane(image, p),
            CVPixelBufferGetBytesPerRowOfPlane(image, p),
            CVPixelBufferGetWidthOfPlane(image, p),
            CVPixelBufferGetHeightOfPlane(image, p),
        )
    };
    let ((yp, ys, w, h), (cp, cs, cw, ch)) = (plane(0), plane(1));
    let mut y = Vec::with_capacity(w * h);
    for r in 0..h {
        y.extend_from_slice(std::slice::from_raw_parts(yp.add(r * ys), w));
    }
    let (mut u, mut v) = (Vec::with_capacity(cw * ch), Vec::with_capacity(cw * ch));
    for r in 0..ch {
        for uv in std::slice::from_raw_parts(cp.add(r * cs), cw * 2).chunks_exact(2) {
            u.push(uv[0]);
            v.push(uv[1]);
        }
    }
    CVPixelBufferUnlockBaseAddress(image, LOCK_READ_ONLY);
    let pts_us = if pts.flags & CM_TIME_VALID != 0 && pts.timescale > 0 {
        (pts.value as i128 * 1_000_000 / pts.timescale as i128) as i64
    } else {
        0
    };
    out.push(Ok(Decoded::Picture { pts_us, width: w as u32, height: h as u32, y, u, v }));
}

impl Session {
    /// A hardware VP9 session for the stream `key` starts, or why there is none.
    pub(crate) fn new(key: &KeyFrame) -> Result<Session, String> {
        unsafe {
            #[cfg(target_os = "macos")]
            {
                static REGISTER: std::sync::Once = std::sync::Once::new();
                REGISTER.call_once(|| VTRegisterSupplementalVideoDecoderIfAvailable(VP09));
            }
            let dict = || {
                CFDictionaryCreateMutable(
                    std::ptr::null(),
                    0,
                    &kCFTypeDictionaryKeyCallBacks as *const c_void,
                    &kCFTypeDictionaryValueCallBacks as *const c_void,
                )
            };
            // vpcC (VP Codec ISO Media File Format Binding 2.2): version 1, flags 0, profile, level, bit depth << 4 |
            // chroma subsampling (1: 4:2:0 co-located) << 1 | full range, primaries / transfer / matrix (2: unspecified),
            // no initialization data.
            let level: u8 = if key.width * key.height <= 1920 * 1088 { 40 } else { 50 };
            let vpcc = [1, 0, 0, 0, key.profile, level, key.bit_depth << 4 | 1 << 1, 2, 2, 2, 0, 0];
            let data = CFDataCreate(std::ptr::null(), vpcc.as_ptr(), vpcc.len() as isize);
            let name = CFStringCreateWithCString(std::ptr::null(), c"vpcC".as_ptr(), CF_STRING_UTF8);
            let atoms = dict();
            CFDictionarySetValue(atoms, name, data);
            let ext = dict();
            CFDictionarySetValue(ext, kCMFormatDescriptionExtension_SampleDescriptionExtensionAtoms, atoms);
            let mut format: CFTypeRef = std::ptr::null();
            let st = CMVideoFormatDescriptionCreate(std::ptr::null(), VP09, key.width as i32, key.height as i32, ext, &mut format);
            for o in [data, name, atoms as CFTypeRef, ext as CFTypeRef] {
                CFRelease(o);
            }
            if st != 0 {
                return Err(format!("VP9 format description: {st}"));
            }
            let spec = dict();
            CFDictionarySetValue(spec, kVTVideoDecoderSpecification_RequireHardwareAcceleratedVideoDecoder, kCFBooleanTrue);
            let attrs = dict();
            let pf = CFNumberCreate(std::ptr::null(), CF_NUMBER_SINT32, &NV12 as *const i32 as *const c_void);
            CFDictionarySetValue(attrs, kCVPixelBufferPixelFormatTypeKey, pf);
            CFRelease(pf);
            let mut out: Box<Vec<Result<Decoded, OSStatus>>> = Box::new(Vec::new());
            let record = OutputCallbackRecord { callback: on_output, refcon: &mut *out as *mut _ as *mut c_void };
            let mut session: CFTypeRef = std::ptr::null();
            let st = VTDecompressionSessionCreate(std::ptr::null(), format, spec, attrs, &record, &mut session);
            CFRelease(spec as CFTypeRef);
            CFRelease(attrs as CFTypeRef);
            if st != 0 {
                CFRelease(format);
                return Err(format!("no hardware VP9 decoder ({st})"));
            }
            Ok(Session { session, format, out })
        }
    }

    /// Decodes one sample; the picture it shows (if any) goes to `out`.
    pub(crate) fn decode(&mut self, data: &[u8], pts_us: i64, out: &mut VecDeque<Decoded>) -> Result<(), String> {
        unsafe {
            let mut block: CFTypeRef = std::ptr::null();
            // A buffer of its own (the sample is copied in): VideoToolbox may read it after this returns.
            let st = CMBlockBufferCreateWithMemoryBlock(std::ptr::null(), std::ptr::null_mut(), data.len(), std::ptr::null(), std::ptr::null(), 0, data.len(), 0, &mut block);
            if st != 0 {
                return Err(format!("VP9 block buffer: {st}"));
            }
            let st = CMBlockBufferReplaceDataBytes(data.as_ptr() as *const c_void, block, 0, data.len());
            if st != 0 {
                CFRelease(block);
                return Err(format!("VP9 block buffer: {st}"));
            }
            let timing = CMSampleTimingInfo {
                duration: INVALID_TIME,
                presentation: CMTime { value: pts_us, timescale: 1_000_000, flags: CM_TIME_VALID, epoch: 0 },
                decode: INVALID_TIME,
            };
            let size = data.len();
            let mut sample: CFTypeRef = std::ptr::null();
            let st = CMSampleBufferCreateReady(std::ptr::null(), block, self.format, 1, 1, &timing, 1, &size, &mut sample);
            CFRelease(block);
            if st != 0 {
                return Err(format!("VP9 sample buffer: {st}"));
            }
            // Synchronous: the callback runs before this returns.
            let st = VTDecompressionSessionDecodeFrame(self.session, sample, 0, std::ptr::null_mut(), std::ptr::null_mut());
            VTDecompressionSessionWaitForAsynchronousFrames(self.session);
            CFRelease(sample);
            for r in self.out.drain(..) {
                match r {
                    Ok(d) => out.push_back(d),
                    Err(e) => return Err(format!("VP9 hardware decode: {e}")),
                }
            }
            if st != 0 {
                return Err(format!("VP9 hardware decode: {st}"));
            }
            Ok(())
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            VTDecompressionSessionWaitForAsynchronousFrames(self.session);
            VTDecompressionSessionInvalidate(self.session);
            CFRelease(self.session);
            CFRelease(self.format);
        }
    }
}
