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
    // its neighbours agree; TotK's env-cubemap update frames did this in 2026-10-13 sessions (~10 levels, one frame).
    // Logged and counted only ([glitch] sky dip).
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
    // of a 2x2 block, the bytes the window shows), in memory row order. Full-size copies made the 360-frame ring of play
    // sessions ~3 GB of Metal memory (2026-10-16); captures were written at half resolution anyway.
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
