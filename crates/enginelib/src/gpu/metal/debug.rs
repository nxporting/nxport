// Frame and texture dumps, the glitch scanner, frame and render-target history with the engine journal: what the
// investigation switches (--dump-*, --glitch-scan, --frame-history, --history-targets) produce.

use super::*;

pub(super) fn has_non_zero_pixel(tex: &Tex) -> bool {
    let (w, h) = (tex.width(), tex.height());
    let mut raw = vec![0u8; w * h * 4];
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    unsafe {
        tex.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
            w * 4,
            region,
            0,
        );
    }
    raw.chunks_exact(4)
        .any(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
}

/// Debug: writes any common color format as a PPM (floats clamped to 0..1) and returns a short value summary
/// ("empty" when every channel is zero). Private textures cannot be read back and are skipped.
pub(super) fn dump_any(tex: &Tex, path: &str) -> String {
    if objc2_metal::MTLResource::storageMode(&**tex) == MTLStorageMode::Private {
        return "private (not dumped)".into();
    }
    let (w, h) = (tex.width(), tex.height());
    let f = tex.pixelFormat();
    let bpp = match f {
        MTLPixelFormat::RGBA16Float | MTLPixelFormat::RGBA16Unorm => 8,
        MTLPixelFormat::RGBA32Float | MTLPixelFormat::RGBA32Uint | MTLPixelFormat::RGBA32Sint => 16,
        MTLPixelFormat::RG32Float | MTLPixelFormat::RG32Uint | MTLPixelFormat::RG32Sint => 8,
        MTLPixelFormat::R8Unorm => 1,
        MTLPixelFormat::RG8Unorm | MTLPixelFormat::R16Float | MTLPixelFormat::R16Unorm => 2,
        MTLPixelFormat::RG16Float | MTLPixelFormat::R32Float => 4,
        _ => 4,
    };
    let mut raw = vec![0u8; w * h * bpp];
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    unsafe {
        // Slice 0 (array and cube textures need the slice form).
        tex.getBytes_bytesPerRow_bytesPerImage_fromRegion_mipmapLevel_slice(
            std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
            w * bpp,
            w * h * bpp,
            region,
            0,
            0,
        );
    }
    let half = |b: &[u8]| half_to_f32(u16::from_le_bytes([b[0], b[1]]));
    let f32le = |b: &[u8]| f32::from_le_bytes([b[0], b[1], b[2], b[3]]);
    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    let (mut lo, mut hi, mut nonzero) = (f32::MAX, f32::MIN, 0usize);
    let mut nonfinite = 0usize;
    let dump_alpha = crate::settings::get().dump.dump_alpha;
    for px in raw.chunks_exact(bpp) {
        let rgb: [f32; 3] = match f {
            MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB => [
                px[2] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[0] as f32 / 255.0,
            ],
            MTLPixelFormat::RGBA16Float => [half(&px[0..]), half(&px[2..]), half(&px[4..])],
            MTLPixelFormat::RGBA16Unorm => [
                u16::from_le_bytes([px[0], px[1]]) as f32 / 65535.0,
                u16::from_le_bytes([px[2], px[3]]) as f32 / 65535.0,
                u16::from_le_bytes([px[4], px[5]]) as f32 / 65535.0,
            ],
            MTLPixelFormat::RGBA32Float => [f32le(&px[0..]), f32le(&px[4..]), f32le(&px[8..])],
            MTLPixelFormat::RG16Float => [half(&px[0..]), half(&px[2..]), 0.0],
            MTLPixelFormat::RG32Float => [f32le(&px[0..]), f32le(&px[4..]), 0.0],
            MTLPixelFormat::R16Float => {
                let v = half(px);
                [v, v, v]
            }
            MTLPixelFormat::R32Float => {
                let v = f32le(px);
                [v, v, v]
            }
            MTLPixelFormat::R8Unorm => {
                let v = px[0] as f32 / 255.0;
                [v, v, v]
            }
            MTLPixelFormat::RG8Unorm => [px[0] as f32 / 255.0, px[1] as f32 / 255.0, 0.0],
            MTLPixelFormat::R16Unorm => {
                let v = u16::from_le_bytes([px[0], px[1]]) as f32 / 65535.0;
                [v, v, v]
            }
            MTLPixelFormat::RG16Unorm => [
                u16::from_le_bytes([px[0], px[1]]) as f32 / 65535.0,
                u16::from_le_bytes([px[2], px[3]]) as f32 / 65535.0,
                0.0,
            ],
            MTLPixelFormat::RGB10A2Unorm => {
                let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                [
                    (v & 1023) as f32 / 1023.0,
                    ((v >> 10) & 1023) as f32 / 1023.0,
                    ((v >> 20) & 1023) as f32 / 1023.0,
                ]
            }
            MTLPixelFormat::RG11B10Float => {
                let v = u32::from_le_bytes([px[0], px[1], px[2], px[3]]);
                [
                    uf11(v & 0x7ff),
                    uf11((v >> 11) & 0x7ff),
                    uf10((v >> 22) & 0x3ff),
                ]
            }
            // --dump-alpha: 8-bit RGBA targets are dumped as their alpha channel (greyscale).
            _ if dump_alpha && bpp == 4 => {
                let a = px[3] as f32 / 255.0;
                [a, a, a]
            }
            _ => [
                px[0] as f32 / 255.0,
                px[1] as f32 / 255.0,
                px[2] as f32 / 255.0,
            ],
        };
        for c in rgb {
            if c.is_finite() {
                lo = lo.min(c);
                hi = hi.max(c);
            }
        }
        if rgb.iter().any(|c| *c != 0.0) {
            nonzero += 1;
        }
        if rgb.iter().any(|c| !c.is_finite()) {
            nonfinite += 1;
        }
        out.extend(rgb.iter().map(|c| (c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8));
    }
    let _ = std::fs::write(path, out);
    let nf = if nonfinite > 0 {
        format!(", {} non-finite px", nonfinite)
    } else {
        String::new()
    };
    if nonzero == 0 {
        "empty".into()
    } else {
        format!(
            "{} nonzero px ({:.1}%), range {:.3}..{:.3}{}",
            nonzero,
            nonzero as f64 * 100.0 / (w * h) as f64,
            lo,
            hi,
            nf
        )
    }
}

pub(super) fn half_to_f32(h: u16) -> f32 {
    let s = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let e = ((h >> 10) & 0x1f) as i32;
    let m = (h & 0x3ff) as f32;
    if e == 0 {
        s * m * 2f32.powi(-24)
    } else if e == 31 {
        if m == 0.0 {
            s * f32::INFINITY
        } else {
            f32::NAN
        }
    } else {
        s * (1.0 + m / 1024.0) * 2f32.powi(e - 15)
    }
}
pub(super) fn uf11(v: u32) -> f32 {
    let (e, m) = ((v >> 6) as i32, (v & 0x3f) as f32);
    if e == 0 {
        m / 64.0 * 2f32.powi(-14)
    } else if e == 31 {
        f32::INFINITY
    } else {
        (1.0 + m / 64.0) * 2f32.powi(e - 15)
    }
}
pub(super) fn uf10(v: u32) -> f32 {
    let (e, m) = ((v >> 5) as i32, (v & 0x1f) as f32);
    if e == 0 {
        m / 32.0 * 2f32.powi(-14)
    } else if e == 31 {
        f32::INFINITY
    } else {
        (1.0 + m / 32.0) * 2f32.powi(e - 15)
    }
}

/// Debug: write a texture as a binary PPM (BGRA/RGBA 8-bit formats only).
/// --glitch-scan=from:count[:min_pixels]: single-frame glitch detector for still-camera runs. For each presented
/// frame in range it waits for the GPU, reads the image back and compares the last five frames: pixels (on a
/// half-resolution grid) where the middle frame differs from both neighbours while the two frames on each side are
/// stable are outliers.
/// When more than min_pixels (default 200) are found, the three frames are written as
/// /tmp/nxport_glitch_<frame>_{prev,glitch,next}.ppm with the outlier bounding box logged ([glitch]). Frames are read
/// back asynchronously (no GPU wait). --exit-on-dump exits after the range.
pub(super) fn glitch_scan(g: &Gfx, pn: u64, src: &Tex) {
    let Some(crate::settings::GlitchScan {
        from,
        count,
        min_px,
    }) = crate::settings::get().debug.glitch_scan
    else {
        return;
    };
    if pn < from || pn >= from + count {
        // A frame still to dump (--dump-frame) exits there instead.
        if pn == from + count
            && crate::settings::get().dump.exit_on_dump
            && !crate::settings::get().dump.dump_frame.iter().any(|&f| f >= pn)
        {
            println!(
                "[glitch] scan of frames {}..{} done, {} events, {} tint flashes, {} sky dips",
                from,
                from + count,
                GLITCH_EVENTS.load(Ordering::Relaxed),
                GLITCH_TINTS.load(Ordering::Relaxed),
                GLITCH_SKY.load(Ordering::Relaxed)
            );
            std::process::exit(0);
        }
        return;
    }
    // Asynchronous: the frame is blitted into a staging texture and read once that copy has completed (a later present),
    // so the scan adds no GPU wait and frame timing (including --early-fence overlap) stays as in normal runs.
    use objc2_metal::MTLBlitCommandEncoder;
    type Staged = (
        u64,
        Retained<ProtocolObject<dyn MTLTexture>>,
        Retained<ProtocolObject<dyn MTLCommandBuffer>>,
    );
    struct Pending(
        std::collections::VecDeque<Staged>,
        Vec<Retained<ProtocolObject<dyn MTLTexture>>>,
    );
    unsafe impl Send for Pending {} // presenting thread only, under the mutex
    static PENDING: Mutex<Pending> =
        Mutex::new(Pending(std::collections::VecDeque::new(), Vec::new()));
    let mut pending = PENDING.lock().unwrap();
    let (w, h) = (src.width(), src.height());
    let tex = match pending.1.pop() {
        Some(t) if t.width() == w && t.height() == h && t.pixelFormat() == src.pixelFormat() => t,
        _ => {
            let desc = MTLTextureDescriptor::new();
            desc.setPixelFormat(src.pixelFormat());
            unsafe {
                desc.setWidth(w);
                desc.setHeight(h);
            }
            desc.setUsage(MTLTextureUsage::ShaderRead);
            desc.setStorageMode(MTLStorageMode::Shared);
            let Some(t) = g.device.newTextureWithDescriptor(&desc) else {
                return;
            };
            t
        }
    };
    let Some(cb) = g.queue.commandBuffer() else {
        return;
    };
    let Some(blit) = cb.blitCommandEncoder() else {
        return;
    };
    unsafe {
        blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
            src, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: w, height: h, depth: 1 }, &tex, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 });
    }
    blit.endEncoding();
    cb.commit();
    pending.0.push_back((pn, tex, cb));
    while pending.0.front().is_some_and(|f| {
        matches!(
            f.2.status(),
            MTLCommandBufferStatus::Completed | MTLCommandBufferStatus::Error
        )
    }) {
        let (f, t, _) = pending.0.pop_front().unwrap();
        let mut raw = vec![0u8; w * h * 4];
        let region = MTLRegion {
            origin: MTLOrigin { x: 0, y: 0, z: 0 },
            size: MTLSize {
                width: w,
                height: h,
                depth: 1,
            },
        };
        unsafe {
            t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                w * 4,
                region,
                0,
            )
        };
        pending.1.push(t);
        glitch_analyze(f, raw, w, h, src.pixelFormat(), min_px);
    }
}

pub(super) fn glitch_analyze(pn: u64, raw: Vec<u8>, w: usize, h: usize, fmt: MTLPixelFormat, min_px: usize) {
    static RING: Mutex<Vec<(u64, Vec<u8>)>> = Mutex::new(Vec::new());
    let mut ring = RING.lock().unwrap();
    ring.push((pn, raw));
    if ring.len() > 5 {
        ring.remove(0);
    }
    if ring.len() < 5 || ring[0].0 + 4 != ring[4].0 {
        return;
    }
    // Middle frame f2 against two stable frames on each side: continuously animated pixels (particles, wind) fail the
    // stability test, a one-frame error on a still image passes.
    let (f0, f1, f2, f3, f4) = (&ring[0].1, &ring[1].1, &ring[2].1, &ring[3].1, &ring[4].1);
    let d =
        |a: &[u8], b: &[u8], i: usize| (0..3).map(|c| a[i + c].abs_diff(b[i + c])).max().unwrap();
    // Second test, tolerant of camera motion: pixels black in f2 while both neighbours are clearly brighter (the
    // black blobs and tiles seen in play).
    let mx = |a: &[u8], i: usize| a[i].max(a[i + 1]).max(a[i + 2]);
    let (mut n, mut nb, mut x0, mut y0, mut x1, mut y1) = (0usize, 0usize, w, h, 0, 0);
    for y in (0..h).step_by(2) {
        for x in (0..w).step_by(2) {
            let i = (y * w + x) * 4;
            let still = d(f1, f3, i) < 12
                && d(f0, f1, i) < 12
                && d(f3, f4, i) < 12
                && d(f2, f1, i) > 40
                && d(f2, f3, i) > 40;
            let black = mx(f2, i) < 6 && mx(f1, i) > 30 && mx(f3, i) > 30;
            if still || black {
                n += still as usize;
                nb += black as usize;
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
    }
    // Third test: whole-frame tint / exposure flashes (scene-wide parameters wrong for one frame). Mean colour of each
    // frame on a coarse grid; the middle one jumps away from both neighbours while they agree.
    let mean = |f: &[u8]| {
        let mut m = [0f64; 3];
        let mut k = 0f64;
        for y in (0..h).step_by(8) {
            for x in (0..w).step_by(8) {
                let i = (y * w + x) * 4;
                for c in 0..3 {
                    m[c] += f[i + c] as f64;
                }
                k += 1.0;
            }
        }
        m.map(|v| v / k)
    };
    let (m1, m2, m3) = (mean(f1), mean(f2), mean(f3));
    let tint = (0..3)
        .map(|c| {
            (
                (m2[c] - m1[c]).abs().min((m2[c] - m3[c]).abs()),
                (m1[c] - m3[c]).abs(),
            )
        })
        .filter(|&(jump, base)| jump > 4.0 && base < jump / 3.0)
        .map(|(jump, _)| jump)
        .fold(0.0, f64::max);
    if tint > 0.0 {
        let n = GLITCH_TINTS.fetch_add(1, Ordering::Relaxed);
        println!("[glitch] frame {}: tint flash, mean colour {:.1?} between {:.1?} and {:.1?} (jump {:.1})", ring[2].0, m2, m1, m3, tint);
        if n < 20 {
            let bgra = matches!(
                fmt,
                MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
            );
            for (k, name) in ["prev", "glitch", "next"].iter().enumerate() {
                write_ppm_raw(
                    &ring[k + 1].1,
                    w,
                    h,
                    bgra,
                    &format!("/tmp/nxport_glitch_{}_{}.ppm", ring[2].0, name),
                );
            }
        }
    }
    // Fourth test: sky dips. The top third of the frame (the sky in most scenes) darkens or brightens for one frame while
    // its neighbours agree (TotK's environment-cubemap update frames). Logged and counted only ([glitch] sky dip).
    let sky = |f: &[u8]| {
        let (mut m, mut k) = (0f64, 0f64);
        for y in (0..h / 3).step_by(4) {
            for x in (0..w).step_by(4) {
                let i = (y * w + x) * 4;
                m += (f[i] as f64 + f[i + 1] as f64 + f[i + 2] as f64) / 3.0;
                k += 1.0;
            }
        }
        m / k.max(1.0)
    };
    let (s1, s2, s3) = (sky(f1), sky(f2), sky(f3));
    if (s2 - s1).abs() > 4.0
        && (s2 - s3).abs() > 4.0
        && (s1 - s3).abs() < 1.5
        && (s2 - s1).signum() == (s2 - s3).signum()
    {
        GLITCH_SKY.fetch_add(1, Ordering::Relaxed);
        println!(
            "[glitch] frame {}: sky dip {:.1} (top third {:.1}, neighbours {:.1} / {:.1})",
            ring[2].0,
            s2 - (s1 + s3) / 2.0,
            s2,
            s1,
            s3
        );
    }
    // Black flashes have their own dump budget (camera motion fills the outlier one).
    static BLACK_DUMPS: AtomicU64 = AtomicU64::new(0);
    let black_dump = nb > 200 && BLACK_DUMPS.fetch_add(1, Ordering::Relaxed) < 30;
    if n <= min_px && nb <= min_px && !black_dump {
        return;
    }
    let ev = if black_dump {
        0
    } else {
        GLITCH_EVENTS.fetch_add(1, Ordering::Relaxed)
    };
    // --glitch-dump-history: an event also dumps the frame history (with targets), at most every 120 frames.
    static LAST_DUMP: AtomicU64 = AtomicU64::new(0);
    if crate::settings::get().debug.glitch_dump_history
        && ring[2].0 > LAST_DUMP.load(Ordering::Relaxed) + 120
    {
        LAST_DUMP.store(ring[2].0, Ordering::Relaxed);
        crate::gpu::history::request();
        println!("[glitch] frame {}: dumping the frame history", ring[2].0);
    }
    let mid = ring[2].0;
    println!("[glitch] frame {}: {} outlier / {} black-flash pixels (half-res grid), bbox ({},{})-({},{})", mid, n, nb, x0, y0, x1, y1);
    if ev < 30 {
        let bgra = matches!(
            fmt,
            MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
        );
        for (k, name) in ["prev", "glitch", "next"].iter().enumerate() {
            write_ppm_raw(
                &ring[k + 1].1,
                w,
                h,
                bgra,
                &format!("/tmp/nxport_glitch_{}_{}.ppm", mid, name),
            );
        }
    }
}
pub(super) static GLITCH_EVENTS: AtomicU64 = AtomicU64::new(0);
pub(super) static GLITCH_TINTS: AtomicU64 = AtomicU64::new(0);
pub(super) static GLITCH_SKY: AtomicU64 = AtomicU64::new(0);

/// --frame-history=N: every presented frame is copied on the GPU into a ring of N textures (a blit, no CPU wait).
/// F9 (request_capture) then also writes the ring, oldest first, as half-resolution images
/// /tmp/nxport_history_<press frame>/<frame>.ppm: single-frame glitches are gone before anyone can press a key.
/// The dump ranks the frames by single-frame outlier pixels (frame t differs from t-1 and t+1 while those agree) and
/// logs the top candidates ([history]); with a moving camera every frame scores, so compare scores, not absolute values.
/// --history-dump-at=<frame> dumps without a key.
pub(super) fn frame_history(g: &Gfx, pn: u64, src: &Tex) {
    let n = history_len();
    if n == 0 {
        return;
    }
    // The presented frame's events (everything since the previous present).
    journal_rotate(pn);
    struct Ring(Vec<(u64, Retained<ProtocolObject<dyn MTLTexture>>)>);
    unsafe impl Send for Ring {} // only touched on the presenting thread, under the mutex
    static RING: Mutex<Ring> = Mutex::new(Ring(Vec::new()));
    let mut guard = RING.lock().unwrap();
    let ring = &mut guard.0;
    if crate::gpu::history::due(pn) && !ring.is_empty() {
        if let Some(cb) = g.queue.commandBuffer() {
            cb.commit();
            cb.waitUntilCompleted();
        }
        let dir = crate::gpu::history::dir(pn);
        let mut frames: Vec<&(u64, Retained<ProtocolObject<dyn MTLTexture>>)> =
            ring.iter().collect();
        frames.sort_by_key(|f| f.0);
        let mut halves: Vec<(u64, usize, usize, Vec<u8>)> = Vec::new();
        for (f, t) in &frames {
            let (w, h) = (t.width(), t.height());
            let mut raw = vec![0u8; w * h * 4];
            let region = MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: w,
                    height: h,
                    depth: 1,
                },
            };
            unsafe {
                t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                    w * 4,
                    region,
                    0,
                )
            };
            let bgra = matches!(
                t.pixelFormat(),
                MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
            );
            let flip = crate::nvn::api::window_origin_mode() == 0;
            // Ring frames are stored at half resolution already (see below).
            let (hw, hh) = (w, h);
            let mut out = format!("P6\n{} {}\n255\n", hw, hh).into_bytes();
            for y in 0..hh {
                let row = if flip { h - 1 - y } else { y };
                for x in 0..hw {
                    let px = &raw[(row * w + x) * 4..][..4];
                    if bgra {
                        out.extend_from_slice(&[px[2], px[1], px[0]]);
                    } else {
                        out.extend_from_slice(&px[..3]);
                    }
                }
            }
            let hdr = out.len() - hw * hh * 3;
            halves.push((*f, hw, hh, out[hdr..].to_vec()));
        }
        crate::gpu::history::write_frames(&dir, &mut halves);
        crate::gpu::history::write_journal(&dir);
        history_targets_write(&dir);
    }
    // Frames are kept at half resolution, drawn with the present's blit pipeline (linear filtering: each pixel the average
    // of a 2x2 block, the bytes the window shows), in memory row order: a 360-frame ring of full-size frames would take
    // several GB of Metal memory, and captures are written at half resolution anyway.
    let (hw, hh) = ((src.width() / 2).max(1), (src.height() / 2).max(1));
    let slot = if ring.len() >= n {
        let i = ring
            .iter()
            .enumerate()
            .min_by_key(|(_, f)| f.0)
            .map(|(i, _)| i)
            .unwrap();
        let (_, t) = ring.remove(i);
        (t.width() == hw && t.height() == hh).then_some(t)
    } else {
        None
    };
    let dst = match slot {
        Some(t) => t,
        None => {
            let desc = MTLTextureDescriptor::new();
            desc.setPixelFormat(MTLPixelFormat::BGRA8Unorm);
            unsafe {
                desc.setWidth(hw);
                desc.setHeight(hh);
            }
            desc.setUsage(MTLTextureUsage::ShaderRead | MTLTextureUsage::RenderTarget);
            desc.setStorageMode(MTLStorageMode::Shared);
            let Some(t) = g.device.newTextureWithDescriptor(&desc) else {
                return;
            };
            t
        }
    };
    let Some(cb) = g.queue.commandBuffer() else {
        return;
    };
    let pass = MTLRenderPassDescriptor::renderPassDescriptor();
    unsafe {
        let att = pass.colorAttachments().objectAtIndexedSubscript(0);
        att.setTexture(Some(&dst));
        att.setLoadAction(MTLLoadAction::DontCare);
        att.setStoreAction(MTLStoreAction::Store);
    }
    let Some(enc) = cb.renderCommandEncoderWithDescriptor(&pass) else {
        return;
    };
    enc.setRenderPipelineState(&g.blit);
    let crop_uv: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    unsafe {
        enc.setFragmentTexture_atIndex(Some(src), 0);
        enc.setFragmentBytes_length_atIndex(
            std::ptr::NonNull::new(crop_uv.as_ptr() as *mut libc::c_void).unwrap(),
            16,
            0,
        );
        enc.drawPrimitives_vertexStart_vertexCount(MTLPrimitiveType::Triangle, 0, 3);
        // blit_vs: one full-screen triangle
    }
    enc.endEncoding();
    cb.commit();
    ring.push((pn, dst));
    history_targets_capture(g, pn);
}

/// Target history (--history-targets=WxH with --frame-history): the render targets of that size drawn in each of
/// the last --history-targets-n frames (default 12), copied at the present; written as raw files with the frame
/// history (targets/<frame>_<storage>_<w>x<h>_fmt<metal format>.raw). Shows which intermediate a glitch first appears in.
pub(super) type TargetRing =
    std::collections::VecDeque<(u64, Vec<(usize, Retained<ProtocolObject<dyn MTLTexture>>)>)>;
pub(super) struct TargetHistory(TargetRing, Vec<Retained<ProtocolObject<dyn MTLTexture>>>);
unsafe impl Send for TargetHistory {}
pub(super) static TARGET_HISTORY: Mutex<TargetHistory> =
    Mutex::new(TargetHistory(std::collections::VecDeque::new(), Vec::new()));

pub(super) fn history_targets_capture(g: &Gfx, pn: u64) {
    use objc2_metal::MTLBlitCommandEncoder;
    if translated::history_target_size().is_none() {
        return;
    }
    let n: usize = crate::settings::get().dump.history_targets_n;
    let targets = std::mem::take(&mut translated::FRAME_TARGETS.lock().unwrap().0);
    let mut h = TARGET_HISTORY.lock().unwrap();
    while h.0.len() >= n {
        let (_, old) = h.0.pop_front().unwrap();
        h.1.extend(old.into_iter().map(|(_, t)| t));
    }
    let Some(cb) = g.queue.commandBuffer() else {
        return;
    };
    let Some(blit) = cb.blitCommandEncoder() else {
        return;
    };
    let mut copies = Vec::new();
    for (storage, src) in targets {
        let pos = h.1.iter().position(|t| {
            t.width() == src.width()
                && t.height() == src.height()
                && t.pixelFormat() == src.pixelFormat()
        });
        let dst = match pos {
            Some(i) => h.1.swap_remove(i),
            None => {
                let desc = MTLTextureDescriptor::new();
                desc.setPixelFormat(src.pixelFormat());
                unsafe {
                    desc.setWidth(src.width());
                    desc.setHeight(src.height());
                }
                desc.setUsage(MTLTextureUsage::ShaderRead);
                desc.setStorageMode(MTLStorageMode::Shared);
                let Some(t) = g.device.newTextureWithDescriptor(&desc) else {
                    continue;
                };
                t
            }
        };
        unsafe {
            blit.copyFromTexture_sourceSlice_sourceLevel_sourceOrigin_sourceSize_toTexture_destinationSlice_destinationLevel_destinationOrigin(
                &src, 0, 0, MTLOrigin { x: 0, y: 0, z: 0 }, MTLSize { width: src.width(), height: src.height(), depth: 1 }, &dst, 0, 0,
                MTLOrigin { x: 0, y: 0, z: 0 });
        }
        copies.push((storage, dst));
    }
    blit.endEncoding();
    cb.commit();
    h.0.push_back((pn, copies));
}

pub(super) fn history_targets_write(dir: &str) {
    let h = TARGET_HISTORY.lock().unwrap();
    if h.0.is_empty() {
        return;
    }
    let tdir = format!("{}/targets", dir);
    let _ = std::fs::create_dir_all(&tdir);
    for (f, list) in &h.0 {
        for (storage, t) in list {
            let bpp = match t.pixelFormat().0 {
                10 => 1,
                20 | 25 | 30 => 2,
                105 | 115 => 8,
                125 => 16,
                _ => 4,
            };
            let (w, hh) = (t.width(), t.height());
            let mut raw = vec![0u8; w * hh * bpp];
            let region = MTLRegion {
                origin: MTLOrigin { x: 0, y: 0, z: 0 },
                size: MTLSize {
                    width: w,
                    height: hh,
                    depth: 1,
                },
            };
            unsafe {
                t.getBytes_bytesPerRow_fromRegion_mipmapLevel(
                    std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
                    w * bpp,
                    region,
                    0,
                )
            };
            let _ = std::fs::write(
                format!(
                    "{}/{:06}_{:x}_{}x{}_fmt{}.raw",
                    tdir,
                    f,
                    storage,
                    w,
                    hh,
                    t.pixelFormat().0
                ),
                raw,
            );
        }
    }
}

pub(super) fn history_len() -> usize {
    crate::gpu::history::len()
}

#[inline]
pub(crate) fn journal_on() -> bool {
    crate::gpu::history::journal_on()
}

pub(crate) fn journal(line: impl FnOnce() -> String) {
    crate::gpu::history::journal(line)
}

pub(super) fn journal_rotate(pn: u64) {
    if let Some(g) = translated::FRAME_RT_ROOTS.lock().unwrap().as_mut() {
        *translated::PREV_RT_ROOTS.lock().unwrap() = Some(std::mem::take(&mut g.0));
        g.1.clear();
    }
    crate::gpu::history::rotate(pn);
}

pub(crate) fn tex_id(t: &Tex) -> usize {
    Retained::as_ptr(t) as *const _ as *const u8 as usize
}

pub(super) fn write_ppm_raw(raw: &[u8], w: usize, h: usize, bgra: bool, path: &str) {
    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    let flip = crate::nvn::api::window_origin_mode() == 0;
    for y in 0..h {
        let row = if flip { h - 1 - y } else { y };
        for px in raw[row * w * 4..(row + 1) * w * 4].chunks_exact(4) {
            if bgra {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            } else {
                out.extend_from_slice(&px[..3]);
            }
        }
    }
    let _ = std::fs::write(path, out);
}

pub(super) fn dump_ppm(tex: &Tex, path: &str) {
    let (w, h) = (tex.width(), tex.height());
    let mut raw = vec![0u8; w * h * 4];
    let region = MTLRegion {
        origin: MTLOrigin { x: 0, y: 0, z: 0 },
        size: MTLSize {
            width: w,
            height: h,
            depth: 1,
        },
    };
    unsafe {
        tex.getBytes_bytesPerRow_fromRegion_mipmapLevel(
            std::ptr::NonNull::new(raw.as_mut_ptr().cast()).unwrap(),
            w * 4,
            region,
            0,
        );
    }
    let bgra = matches!(
        tex.pixelFormat(),
        MTLPixelFormat::BGRA8Unorm | MTLPixelFormat::BGRA8Unorm_sRGB
    );
    let mut out = format!("P6\n{} {}\n255\n", w, h).into_bytes();
    // Rows as the screen shows them: bottom-up in memory for NVN's LOWER_LEFT window origin (see present).
    let flip = crate::nvn::api::window_origin_mode() == 0;
    for y in 0..h {
        let row = if flip { h - 1 - y } else { y };
        for px in raw[row * w * 4..(row + 1) * w * 4].chunks_exact(4) {
            if bgra {
                out.extend_from_slice(&[px[2], px[1], px[0]]);
            } else {
                out.extend_from_slice(&px[..3]);
            }
        }
    }
    let _ = std::fs::write(path, out);
    println!("[+] Gfx: dumped {}x{} frame to {}", w, h, path);
}

/// --dump-main-image=path[:frame] (default frame 300): writes the main module's .text + .rodata as they are in memory
/// at that frame (plus .data and .bss), to diff against a run without mods and list what mods (exlaunch hooks) patched.
pub(super) fn dump_main_image() {
    let Some(crate::settings::FrameFile { path, frame }) =
        &crate::settings::get().dump.dump_main_image
    else {
        return;
    };
    if presented_frames() != *frame {
        return;
    }
    let base = crate::guest::exec::crash_handler::get_guest_base();
    // .text at 0, .rodata at 0x2B18000, .data at 0x41A7000, .bss at 0x466D000 (guest/loader.rs).
    let bytes = unsafe { std::slice::from_raw_parts(base as *const u8, 0x0466_D000 + 761_888) };
    match std::fs::write(path, bytes) {
        Ok(()) => println!(
            "[+] main image (.text to .bss, {} bytes) written to {}",
            bytes.len(),
            path.display()
        ),
        Err(e) => println!("[!] main image dump to {} failed: {}", path.display(), e),
    }
}
/// --log-tex-mem: every 600 frames, where texture memory goes. Metal textures of the cache by kind (rendered or
/// cleared by the GPU vs copies of guest data), and how much guest memory under the copies is resident (what the guest
/// side of the duplication costs), plus Metal's total allocation.
pub(super) fn tex_mem_report(g: &Gfx, pn: u64) {
    static ON: OnceLock<bool> = OnceLock::new();
    if pn % 600 != 0 || !*ON.get_or_init(|| crate::settings::get().log.log_tex_mem) {
        return;
    }
    let st = g.state.lock().unwrap();
    let mut seen = rustc_hash::FxHashSet::default();
    let (mut rt, mut rt_n, mut copy, mut copy_n, mut resident) = (0u64, 0u64, 0u64, 0u64, 0u64);
    let mut vec = vec![0u8; 1 << 16];
    for (desc, (tex, _)) in st.tc.tex_by_desc.iter() {
        let id = Retained::as_ptr(tex) as *const () as usize;
        if !seen.insert(id) {
            continue;
        }
        let bytes = objc2_metal::MTLAllocation::allocatedSize(&**tex) as u64;
        if st.tc.gpu_written.contains(&storage_id(tex)) {
            rt += bytes;
            rt_n += 1;
            continue;
        }
        copy += bytes;
        copy_n += 1;
        // Resident 16 KiB pages of the guest storage under it (mincore).
        let start = desc.storage & !0x3fff;
        let end = (desc.storage + bytes as usize + 0x3fff) & !0x3fff;
        let pages = (end - start) / 0x4000;
        if desc.storage >= 0x10000 && pages > 0 && pages <= vec.len() {
            if unsafe {
                libc::mincore(
                    start as *mut libc::c_void,
                    end - start,
                    vec.as_mut_ptr() as *mut libc::c_char,
                )
            } == 0
            {
                resident += vec[..pages].iter().filter(|&&b| b & 1 != 0).count() as u64 * 0x4000;
            }
        }
    }
    let metal = g.device.currentAllocatedSize() as u64;
    println!("[tex-mem] f{}: Metal total {} MB; cached textures: GPU-written {} ({} MB), copies of guest data {} ({} MB) over {} MB of resident guest memory",
        pn, metal >> 20, rt_n, rt >> 20, copy_n, copy >> 20, resident >> 20);
}

/// --tex-backing (metal/tex_backing.rs): every 60 frames, the storage of textures whose content is a copy of guest data.
pub(super) fn tex_backing_frame(g: &Gfx, pn: u64) {
    if tex_backing::mode() == crate::settings::TexBacking::Off {
        return;
    }
    tex_backing::frame_end(pn);
    if pn % 60 != 0 {
        return;
    }
    let texs: Vec<tex_backing::TexRec> = {
        let st = g.state.lock().unwrap();
        st.tc
            .tex_by_desc
            .iter()
            .filter(|(_, (tex, _))| !st.tc.gpu_written.contains(&storage_id(tex)))
            .filter_map(|(d, (tex, _))| {
                let p = crate::nvn::layout::TextureParams {
                    target: d.target,
                    width: d.width,
                    height: d.height,
                    depth: d.depth,
                    levels: d.levels,
                    format: d.format,
                    flags: d.flags,
                    samples: d.samples,
                    stride: d.stride,
                };
                // Only textures whose guest bytes can be rebuilt from Metal (others keep their pages out of the ranges).
                tex_backing::supported(tex, &p).then(|| tex_backing::TexRec {
                    storage: d.storage,
                    len: crate::nvn::layout::storage_size(&p),
                    params: p,
                    tex: tex.clone(),
                })
            })
            .collect()
    };
    tex_backing::rebuild(pn, texs);
}

/// --watch-storage=0x...: log render-target binds and copies into textures stored at that address.
/// Debug: --watch-size=WxH logs render-target binds, copies and clears into textures of that size (any storage):
/// finds the writer of a texture whose storage address changes from run to run.
pub(crate) fn watch_size() -> Option<(u32, u32)> {
    crate::settings::get().trace.watch_size.map(|s| (s.w, s.h))
}

/// Guest memory of the textures --watch-size has seen; watch_log reports everything else that touches it (holds,
/// write-backs, ended holds, engine reads of held pages, cache evictions), so a texture's whole history is in the log.
pub(crate) static WATCH_MEM: Mutex<Vec<(usize, usize)>> = Mutex::new(Vec::new());

/// Remembers `t`'s memory when it has the --watch-size size; true if it does.
pub(crate) fn watch_texture(t: &NvnTexture) -> bool {
    let Some((w, h)) = watch_size() else {
        return false;
    };
    if t.width as u32 != w || t.height as u32 != h || t.storage <= 0x10000 {
        return false;
    }
    let range = (t.storage, t.storage + layout_of(t).total as usize);
    let mut m = WATCH_MEM.lock().unwrap();
    if !m.contains(&range) {
        if m.len() >= 8192 {
            m.remove(0);
        }
        m.push(range);
    }
    true
}

pub(crate) fn watch_log(start: usize, end: usize, what: impl FnOnce() -> String) {
    if watch_size().is_none() || !WATCH_MEM.lock().unwrap().iter().any(|&(s, e)| s < end && e > start) {
        return;
    }
    println!("[watch-size] f{} {}", presented_frames() + 1, what());
}

/// Non-zero bytes of [addr, addr+len) read without disturbing holds (through their second mappings).
pub(crate) unsafe fn watch_nonzero(addr: usize, len: usize) -> usize {
    match rt_guard::guest_copy::<u8>(addr, len) {
        Some(v) => v.iter().filter(|&&b| b != 0).count(),
        None => std::slice::from_raw_parts(addr as *const u8, len).iter().filter(|&&b| b != 0).count(),
    }
}

pub(crate) fn watch_storage() -> Option<usize> {
    crate::settings::get().trace.watch_storage.map(|a| a.0)
}

/// Debug: --verify-tex records, per Metal texture, the guest storage, content hash and frame of its last upload;
/// the frame trace prints it next to each bound texture's current guest hash (stale or foreign uploads show up there).
pub(crate) fn verify_tex_enabled() -> bool {
    static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *V.get_or_init(|| crate::settings::get().log.verify_tex)
}

/// Frames presented so far (for deferring work until the GPU is done with a frame).
pub(crate) fn draw_reuse_log() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_draw_reuse)
}

pub(crate) fn cmd_time_from() -> Option<u64> {
    static F: OnceLock<Option<u64>> = OnceLock::new();
    *F.get_or_init(|| crate::settings::get().log.log_cmd_time)
}

pub(crate) fn cmd_time_on() -> bool {
    let on = cmd_time_from().is_some_and(|f| (f..f + 60).contains(&presented_frames()));
    if on {
        let _ = CMD_TIME_GEN0.compare_exchange(
            0,
            crate::nvn::api::pools_gen(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
        let _ = CMD_TIME_SGEN0.compare_exchange(
            0,
            crate::nvn::api::samplers_gen(),
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }
    on
}

/// nvn::api::pools_gen() when the --log-cmd-time window began (how often pool-derived memos are invalidated).
pub(crate) static CMD_TIME_GEN0: AtomicU64 = AtomicU64::new(0);

pub(crate) static CMD_TIME_SGEN0: AtomicU64 = AtomicU64::new(0);

pub(crate) static CMD_TIME: Mutex<Option<std::collections::HashMap<&'static str, (u64, u64)>>> =
    Mutex::new(None);

pub(crate) fn note_cmd_time(name: &'static str, ns: u64) {
    let mut g = CMD_TIME.lock().unwrap();
    let e = g
        .get_or_insert_with(Default::default)
        .entry(name)
        .or_default();
    e.0 += ns;
    e.1 += 1;
}

/// Prints the --log-cmd-time table once its 60 frames are over (called at the present).
pub(crate) fn report_cmd_time() {
    let Some(f) = cmd_time_from() else { return };
    if presented_frames() != f + 60 {
        return;
    }
    let g = CMD_TIME.lock().unwrap();
    let mut rows: Vec<_> = g
        .as_ref()
        .map(|m| m.iter().map(|(k, v)| (*k, *v)).collect())
        .unwrap_or_default();
    rows.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let total: u64 = rows
        .iter()
        .filter(|r| r.0 != "nvnCommandBufferCallCommands")
        .map(|r| r.1 .0)
        .sum();
    println!("[cmd-time] frames {}..{}: {:.2} ms per frame in commands (CallCommands excluded; it includes its list); pool generation changes {:.1}, sampler registrations {:.1} per frame",
        f, f + 60, total as f64 / 60.0 / 1e6, crate::nvn::api::pools_gen().saturating_sub(CMD_TIME_GEN0.load(Ordering::Relaxed)) as f64 / 60.0,
        crate::nvn::api::samplers_gen().saturating_sub(CMD_TIME_SGEN0.load(Ordering::Relaxed)) as f64 / 60.0);
    for (n, (ns, c)) in rows.iter().take(30) {
        println!(
            "[cmd-time] {:7.3} ms {:8.1} per frame ({:6.2} us each) {}",
            *ns as f64 / 60.0 / 1e6,
            *c as f64 / 60.0,
            *ns as f64 / (*c).max(1) as f64 / 1e3,
            n
        );
    }
}

/// Query the current process memory footprint (phys_footprint) and available memory limit on Darwin/Apple Silicon.
pub fn process_memory_stats() -> (u64, u64) {
    #[cfg(target_vendor = "apple")]
    unsafe {
        #[repr(C)]
        struct TaskVMInfo {
            virtual_size: u64,
            region_count: i32,
            page_size: i32,
            resident_size: u64,
            resident_size_peak: u64,
            device: u64,
            device_peak: u64,
            internal: u64,
            internal_peak: u64,
            external: u64,
            external_peak: u64,
            reusable: u64,
            reusable_peak: u64,
            purgeable_volatile_pmap: u64,
            purgeable_volatile_resident: u64,
            purgeable_volatile_virtual: u64,
            compressed: u64,
            compressed_peak: u64,
            compressed_lifetime: u64,
            phys_footprint: u64,
        }
        extern "C" {
            fn mach_task_self() -> u32;
            fn task_info(
                task: u32,
                flavor: u32,
                task_info_out: *mut TaskVMInfo,
                task_info_outCnt: *mut u32,
            ) -> i32;
            fn os_proc_available_memory() -> usize;
        }
        let mut info: TaskVMInfo = std::mem::zeroed();
        let mut count = (std::mem::size_of::<TaskVMInfo>() / 4) as u32;
        let footprint = if task_info(mach_task_self(), 22, &mut info, &mut count) == 0 {
            info.phys_footprint
        } else {
            0
        };
        let avail = os_proc_available_memory() as u64;
        (footprint, avail)
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        (0, 0)
    }
}

/// --trace-on-shader=<entry>: the frame after the first draw using that shader is traced (and dumped with
/// --dump-on-trigger), for content that appears at a different frame number in every run.
pub(crate) static TRACE_TRIGGERED_FRAME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Set by request_capture: the triggered frame is also dumped (as with --dump-on-trigger).
pub(crate) static CAPTURE_REQUESTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// F9 in the game window: trace the next frame's draws into the log and dump it to /tmp/nxport_frame_<n>.ppm.
/// Can be repeated; each press captures a new frame.
#[cfg(target_os = "macos")]
pub fn request_capture() {
    use std::sync::atomic::Ordering::Relaxed;
    let f = PRESENTED.load(Relaxed) + 2;
    CAPTURE_REQUESTED.store(true, Relaxed);
    TRACE_TRIGGERED_FRAME.store(f, Relaxed);
    crate::gpu::history::request();
    println!(
        "[+] Capture requested: frame {} (trace in the log, image /tmp/nxport_frame_{}.ppm)",
        f, f
    );
}

pub(crate) fn trace_trigger_check(entries: &[&str]) {
    let Some(want) = &crate::settings::get().trace.trace_on_shader else {
        return;
    };
    if TRACE_TRIGGERED_FRAME.load(std::sync::atomic::Ordering::Relaxed) == 0
        && entries.iter().any(|e| e == want)
    {
        let f = PRESENTED.load(std::sync::atomic::Ordering::Relaxed) + 2;
        TRACE_TRIGGERED_FRAME.store(f, std::sync::atomic::Ordering::Relaxed);
        println!("[+] Gfx: {} seen; tracing frame {}", want, f);
    }
}

pub(crate) fn trace_triggered_frame() -> u64 {
    TRACE_TRIGGERED_FRAME.load(std::sync::atomic::Ordering::Relaxed)
}
