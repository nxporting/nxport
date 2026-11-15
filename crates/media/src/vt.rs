// Video through VideoToolbox, the system's video decoder hardware: the console decodes movies in hardware too, and
// decoding 1080p VP9 on the CPU took every core of an iPhone for ~12 ms a frame, beside the game. H.264: every Apple
// device. VP9: Macs (the supplemental decoder, registered on request); iOS keeps it to apps with Apple's private
// entitlement com.apple.developer.coremedia.allow-alternate-video-decoder-selection (-12906 without). Only a hardware
// decoder is used (RequireHardwareAcceleratedVideoDecoder): without one, a session fails and the caller decodes in
// software.

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
    fn CMVideoFormatDescriptionCreateFromH264ParameterSets(
        a: CFAllocatorRef,
        count: usize,
        sets: *const *const u8,
        sizes: *const usize,
        nal_header_len: i32,
        out: *mut CFTypeRef,
    ) -> OSStatus;
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
    fn VTDecompressionSessionFinishDelayedFrames(s: CFTypeRef) -> OSStatus;
    fn VTDecompressionSessionInvalidate(s: CFTypeRef);
    fn dlsym(handle: *mut c_void, symbol: *const std::ffi::c_char) -> *mut c_void;
}

/// Register the supplemental VP9 decoder so VideoToolbox will vend it. The SDK marks the call macOS-only, but iOS 26.2+
/// exports it and then allows VP9 without the private entitlement (confirmed on an iPhone 16e, iOS 27.3), so it is
/// resolved at runtime. A no-op where the symbol is absent (older iOS), leaving the caller to fall back to software.
unsafe fn register_vp9() {
    static REGISTER: std::sync::Once = std::sync::Once::new();
    REGISTER.call_once(|| {
        let rtld_default = -2isize as *mut c_void; // Darwin RTLD_DEFAULT: normal load order (VideoToolbox is loaded).
        let f = dlsym(rtld_default, c"VTRegisterSupplementalVideoDecoderIfAvailable".as_ptr());
        if !f.is_null() {
            std::mem::transmute::<_, extern "C" fn(u32)>(f)(VP09);
        }
    });
}

const VP09: u32 = u32::from_be_bytes(*b"vp09");
/// kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange ('420v'): Y, then interleaved CbCr at half size.
const NV12: i32 = i32::from_be_bytes(*b"420v");
const CF_NUMBER_SINT32: isize = 3;
const CF_STRING_UTF8: u32 = 0x0800_0100;
const LOCK_READ_ONLY: u64 = 1;
/// kVTDecodeFrame_EnableAsynchronousDecompression | kVTDecodeFrame_EnableTemporalProcessing: pictures come out in
/// presentation order (H.264 B-frames), the held ones at FinishDelayedFrames.
const TEMPORAL: u32 = 1 << 0 | 1 << 3;

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
    /// Decode flags (TEMPORAL for codecs that reorder).
    flags: u32,
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

unsafe fn dict() -> CFMutableDictionaryRef {
    CFDictionaryCreateMutable(
        std::ptr::null(),
        0,
        &kCFTypeDictionaryKeyCallBacks as *const c_void,
        &kCFTypeDictionaryValueCallBacks as *const c_void,
    )
}

impl Session {
    /// A hardware VP9 session for the stream `key` starts, or why there is none.
    pub(crate) fn new(key: &KeyFrame) -> Result<Session, String> {
        unsafe {
            register_vp9();
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
            Self::create(format, 0).map_err(|st| format!("no hardware VP9 decoder ({st})"))
        }
    }

    /// A hardware H.264 session for the stream with these parameter sets (SPS, PPS without start codes) and samples of
    /// NAL units with `nal_len`-byte length prefixes (mp4's avcC), or why there is none.
    pub(crate) fn new_h264(sets: &[&[u8]], nal_len: usize) -> Result<Session, String> {
        unsafe {
            let ptrs: Vec<*const u8> = sets.iter().map(|s| s.as_ptr()).collect();
            let sizes: Vec<usize> = sets.iter().map(|s| s.len()).collect();
            let mut format: CFTypeRef = std::ptr::null();
            let st = CMVideoFormatDescriptionCreateFromH264ParameterSets(
                std::ptr::null(),
                sets.len(),
                ptrs.as_ptr(),
                sizes.as_ptr(),
                nal_len as i32,
                &mut format,
            );
            if st != 0 {
                return Err(format!("H.264 format description: {st}"));
            }
            // A stream that never reorders (no B-frames, as our re-encoded movies are built) needs no temporal
            // buffering: each frame comes out as it is decoded. Only when the SPS allows reordering, or cannot be read,
            // is TEMPORAL used so VideoToolbox hands pictures over in presentation order.
            let sps = sets.iter().find(|s| s.first().is_some_and(|b| b & 0x1f == 7));
            let flags = if sps.and_then(|s| h264_reorder_depth(s)) == Some(0) { 0 } else { TEMPORAL };
            Self::create(format, flags).map_err(|st| format!("no hardware H.264 decoder ({st})"))
        }
    }

    /// The session for `format` (consumed), hardware only, decoding to NV12.
    unsafe fn create(format: CFTypeRef, flags: u32) -> Result<Session, OSStatus> {
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
            return Err(st);
        }
        Ok(Session { session, format, flags, out })
    }

    /// Decodes one sample; the picture it shows (if any) goes to `out`.
    pub(crate) fn decode(&mut self, data: &[u8], pts_us: i64, out: &mut VecDeque<Decoded>) -> Result<(), String> {
        unsafe {
            let mut block: CFTypeRef = std::ptr::null();
            // A buffer of its own (the sample is copied in): VideoToolbox may read it after this returns.
            let st = CMBlockBufferCreateWithMemoryBlock(std::ptr::null(), std::ptr::null_mut(), data.len(), std::ptr::null(), std::ptr::null(), 0, data.len(), 0, &mut block);
            if st != 0 {
                return Err(format!("video block buffer: {st}"));
            }
            let st = CMBlockBufferReplaceDataBytes(data.as_ptr() as *const c_void, block, 0, data.len());
            if st != 0 {
                CFRelease(block);
                return Err(format!("video block buffer: {st}"));
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
                return Err(format!("video sample buffer: {st}"));
            }
            // Synchronous: the callback runs before this returns.
            let st = VTDecompressionSessionDecodeFrame(self.session, sample, self.flags, std::ptr::null_mut(), std::ptr::null_mut());
            VTDecompressionSessionWaitForAsynchronousFrames(self.session);
            CFRelease(sample);
            self.take(out)?;
            if st != 0 {
                return Err(format!("hardware video decode: {st}"));
            }
            Ok(())
        }
    }

    /// End of stream: the pictures held back for presentation order.
    pub(crate) fn drain(&mut self, out: &mut VecDeque<Decoded>) {
        unsafe {
            VTDecompressionSessionFinishDelayedFrames(self.session);
            VTDecompressionSessionWaitForAsynchronousFrames(self.session);
        }
        let _ = self.take(out);
    }

    fn take(&mut self, out: &mut VecDeque<Decoded>) -> Result<(), String> {
        for r in self.out.drain(..) {
            match r {
                Ok(d) => out.push_back(d),
                Err(e) => return Err(format!("hardware video decode: {e}")),
            }
        }
        Ok(())
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

/// How many pictures an H.264 stream may decode ahead of the next one shown, from its SPS (NAL unit, header byte
/// first): VUI max_num_reorder_frames, else the default the spec gives (0 for intra-only profiles with
/// constraint_set3, else MaxDpbFrames for the level and picture size: A.3.1 / E.2.1). None when the SPS cannot be read.
fn h264_reorder_depth(sps: &[u8]) -> Option<usize> {
    // Emulation prevention: 00 00 03 -> 00 00.
    let mut rbsp = Vec::with_capacity(sps.len());
    let mut zeros = 0;
    for &b in sps.get(1..)? {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        rbsp.push(b);
    }
    let mut pos = 0usize;
    let mut bit = || -> Option<u32> {
        let b = (*rbsp.get(pos / 8)? >> (7 - pos % 8)) & 1;
        pos += 1;
        Some(b as u32)
    };
    fn bits(bit: &mut dyn FnMut() -> Option<u32>, n: u32) -> Option<u32> {
        let mut v = 0;
        for _ in 0..n {
            v = v << 1 | bit()?;
        }
        Some(v)
    }
    fn ue(bit: &mut dyn FnMut() -> Option<u32>) -> Option<u32> {
        let mut lz = 0;
        while bit()? == 0 {
            lz += 1;
            if lz > 31 {
                return None;
            }
        }
        Some((1u32 << lz) - 1 + bits(bit, lz)?)
    }
    let b = &mut bit;
    let profile = bits(b, 8)?;
    let constraints = bits(b, 8)?;
    let level = bits(b, 8)?;
    ue(b)?; // seq_parameter_set_id
    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&profile) {
        let chroma = ue(b)?;
        if chroma == 3 {
            bits(b, 1)?;
        }
        ue(b)?;
        ue(b)?;
        bits(b, 1)?;
        if bits(b, 1)? == 1 {
            for i in 0..if chroma == 3 { 12 } else { 8 } {
                if bits(b, 1)? == 1 {
                    // scaling_list: delta_scale until next_scale is 0.
                    let (mut last, mut next) = (8i32, 8i32);
                    for _ in 0..if i < 6 { 16 } else { 64 } {
                        if next != 0 {
                            let v = ue(b)?;
                            let delta = if v & 1 == 1 { (v as i32 + 1) / 2 } else { -(v as i32 / 2) };
                            next = (last + delta + 256) % 256;
                        }
                        last = if next == 0 { last } else { next };
                    }
                }
            }
        }
    }
    ue(b)?; // log2_max_frame_num_minus4
    match ue(b)? {
        0 => {
            ue(b)?;
        }
        1 => {
            bits(b, 1)?;
            ue(b)?;
            ue(b)?;
            for _ in 0..ue(b)? {
                ue(b)?;
            }
        }
        _ => {}
    }
    ue(b)?; // max_num_ref_frames
    bits(b, 1)?;
    let width_mbs = ue(b)? + 1;
    let height_units = ue(b)? + 1;
    let frame_mbs_only = bits(b, 1)?;
    if frame_mbs_only == 0 {
        bits(b, 1)?;
    }
    bits(b, 1)?;
    if bits(b, 1)? == 1 {
        for _ in 0..4 {
            ue(b)?;
        }
    }
    let height_mbs = (2 - frame_mbs_only) * height_units;
    let default = || -> usize {
        if [44, 86, 100, 110, 122, 244].contains(&profile) && constraints & 0x10 != 0 {
            return 0;
        }
        let max_dpb_mbs = match level {
            9 => 396,
            10 => 396,
            11 if constraints & 0x10 != 0 && profile != 100 => 396,
            11 => 900,
            12 | 13 | 20 => 2376,
            21 => 4752,
            22 | 30 => 8100,
            31 => 18000,
            32 => 20480,
            40 | 41 => 32768,
            42 => 34816,
            50 => 110400,
            51 | 52 => 184320,
            _ => 696320,
        };
        (max_dpb_mbs / (width_mbs * height_mbs).max(1)).min(16) as usize
    };
    if bits(b, 1)? == 0 {
        return Some(default()); // no VUI
    }
    if bits(b, 1)? == 1 && bits(b, 8)? == 255 {
        bits(b, 32)?;
    }
    if bits(b, 1)? == 1 {
        bits(b, 1)?;
    }
    if bits(b, 1)? == 1 {
        bits(b, 4)?;
        if bits(b, 1)? == 1 {
            bits(b, 24)?;
        }
    }
    if bits(b, 1)? == 1 {
        ue(b)?;
        ue(b)?;
    }
    if bits(b, 1)? == 1 {
        bits(b, 32)?;
        bits(b, 32)?;
        bits(b, 1)?;
    }
    let hrd = |b: &mut dyn FnMut() -> Option<u32>| -> Option<()> {
        let cpb = ue(b)? + 1;
        bits(b, 8)?;
        for _ in 0..cpb {
            ue(b)?;
            ue(b)?;
            bits(b, 1)?;
        }
        bits(b, 20)?;
        Some(())
    };
    let nal = bits(b, 1)? == 1;
    if nal {
        hrd(b)?;
    }
    let vcl = bits(b, 1)? == 1;
    if vcl {
        hrd(b)?;
    }
    if nal || vcl {
        bits(b, 1)?;
    }
    bits(b, 1)?; // pic_struct_present_flag
    if bits(b, 1)? == 0 {
        return Some(default()); // no bitstream_restriction
    }
    bits(b, 1)?;
    for _ in 0..4 {
        ue(b)?;
    }
    Some(ue(b)? as usize) // max_num_reorder_frames
}
