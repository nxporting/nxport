//! --gpu-pass-profile=N: GPU time per render / compute pass of one frame every N presented frames, from Metal counter
//! samples at stage boundaries (start of vertex / end of fragment, start / end of a compute pass). Printed a few frames
//! later, grouped by the pass's targets (render) or label (compute), with the frame's GPU span and the idle time
//! between passes. Made for the iPhone, where xctrace is not at hand: the log names the passes that cost the GPU time.
//! Only the profiled frame's passes get sample buffers; the rest pay one descriptor check per pass.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::NSRange;
use objc2_metal::{
    MTLCommonCounterSetTimestamp, MTLComputePassDescriptor, MTLCounterSampleBuffer,
    MTLCounterSampleBufferDescriptor, MTLCounterSamplingPoint, MTLCounterSet, MTLDevice,
    MTLRenderPassDescriptor, MTLStorageMode, MTLTexture,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Samples in the buffer (two per pass).
const SAMPLES: usize = 4096;
/// Presents between the profiled frame and reading its samples (its command buffers are complete by then).
const RESOLVE_AFTER: u64 = 8;

struct Prof {
    buf: Retained<ProtocolObject<dyn MTLCounterSampleBuffer>>,
    /// Passes of the profiled frame: sample pair index -> key.
    passes: Vec<String>,
    frame: u64,
    /// (cpu, gpu) timestamp pairs at arming and at resolving, for the tick -> ns ratio.
    cal0: (u64, u64),
}

unsafe impl Send for Prof {}

static PROF: Mutex<Option<Prof>> = Mutex::new(None);
/// Set from the present that starts the profiled frame to the present that ends it.
static ARMED: AtomicBool = AtomicBool::new(false);

pub(super) fn interval() -> u64 {
    static N: OnceLock<u64> = OnceLock::new();
    *N.get_or_init(|| crate::settings::get().log.gpu_pass_profile)
}

fn timestamp_set(
    device: &ProtocolObject<dyn MTLDevice>,
) -> Option<Retained<ProtocolObject<dyn MTLCounterSet>>> {
    let sets = device.counterSets()?;
    let want = unsafe { MTLCommonCounterSetTimestamp };
    (0..sets.count())
        .map(|i| sets.objectAtIndex(i))
        .find(|s| s.name().isEqualToString(want))
}

fn calibrate(device: &ProtocolObject<dyn MTLDevice>) -> (u64, u64) {
    let (mut cpu, mut gpu) = (0u64, 0u64);
    unsafe {
        device.sampleTimestamps_gpuTimestamp(
            std::ptr::NonNull::from(&mut cpu),
            std::ptr::NonNull::from(&mut gpu),
        )
    };
    (cpu, gpu)
}

/// Called at every present (after the issuer synced): arms the next frame every interval() frames, disarms after it,
/// and prints a profiled frame's passes once its command buffers have completed.
pub(super) fn on_present(device: &ProtocolObject<dyn MTLDevice>, frame: u64) {
    let n = interval();
    if n == 0 {
        return;
    }
    let mut g = PROF.lock().unwrap();
    if ARMED.swap(false, Ordering::AcqRel) {
        return; // the profiled frame just ended; resolved RESOLVE_AFTER presents later
    }
    if let Some(p) = g.as_ref() {
        if !p.passes.is_empty() && frame >= p.frame + RESOLVE_AFTER {
            let p = g.as_mut().unwrap();
            report(device, p);
            p.passes.clear();
        }
    }
    if frame % n != 0 || g.as_ref().is_some_and(|p| !p.passes.is_empty()) {
        return;
    }
    if g.is_none() {
        if !device.supportsCounterSampling(MTLCounterSamplingPoint::AtStageBoundary) {
            println!("[pass-prof] this GPU has no stage-boundary counter sampling; --gpu-pass-profile ignored");
            return;
        }
        let Some(set) = timestamp_set(device) else {
            println!("[pass-prof] no timestamp counter set; --gpu-pass-profile ignored");
            return;
        };
        let d = MTLCounterSampleBufferDescriptor::new();
        d.setCounterSet(Some(&set));
        d.setStorageMode(MTLStorageMode::Shared);
        unsafe { d.setSampleCount(SAMPLES) };
        match device.newCounterSampleBufferWithDescriptor_error(&d) {
            Ok(buf) => {
                *g = Some(Prof {
                    buf,
                    passes: Vec::new(),
                    frame: 0,
                    cal0: (0, 0),
                })
            }
            Err(e) => {
                println!(
                    "[pass-prof] counter sample buffer failed: {}",
                    e.localizedDescription()
                );
                return;
            }
        }
    }
    let p = g.as_mut().unwrap();
    p.frame = frame + 1;
    p.cal0 = calibrate(device);
    ARMED.store(true, Ordering::Release);
}

fn key_of(desc: &MTLRenderPassDescriptor) -> String {
    let mut k = String::from("[");
    let ca = desc.colorAttachments();
    for i in 0..8 {
        let a = unsafe { ca.objectAtIndexedSubscript(i) };
        if let Some(t) = a.texture() {
            k.push_str(&format!(
                "c{}:{}x{}:{},",
                i,
                t.width(),
                t.height(),
                t.pixelFormat().0
            ));
        }
    }
    if let Some(t) = desc.depthAttachment().texture() {
        k.push_str(&format!(
            "d:{}x{}:{}",
            t.width(),
            t.height(),
            t.pixelFormat().0
        ));
    }
    k.push(']');
    k
}

/// Before a render encoder is created from `desc` (issuer thread): attaches the sample buffer while a frame is profiled,
/// and detaches it from descriptors reused afterwards.
pub(super) fn render_pass(desc: &MTLRenderPassDescriptor) {
    if interval() == 0 {
        return;
    }
    let att = unsafe { desc.sampleBufferAttachments().objectAtIndexedSubscript(0) };
    if ARMED.load(Ordering::Acquire) {
        let mut g = PROF.lock().unwrap();
        if let Some(p) = g.as_mut() {
            let i = p.passes.len();
            if 2 * i + 1 < SAMPLES {
                p.passes.push(key_of(desc));
                att.setSampleBuffer(Some(&p.buf));
                unsafe {
                    att.setStartOfVertexSampleIndex(2 * i);
                    att.setEndOfVertexSampleIndex(objc2_metal::MTLCounterDontSample);
                    att.setStartOfFragmentSampleIndex(objc2_metal::MTLCounterDontSample);
                    att.setEndOfFragmentSampleIndex(2 * i + 1);
                }
                return;
            }
        }
    }
    if att.sampleBuffer().is_some() {
        att.setSampleBuffer(None);
    }
}

/// A compute pass descriptor carrying the sample buffer while a frame is profiled (None: create the encoder as usual).
pub(super) fn compute_pass(label: &str) -> Option<Retained<MTLComputePassDescriptor>> {
    if interval() == 0 || !ARMED.load(Ordering::Acquire) {
        return None;
    }
    let mut g = PROF.lock().unwrap();
    let p = g.as_mut()?;
    let i = p.passes.len();
    if 2 * i + 1 >= SAMPLES {
        return None;
    }
    p.passes.push(format!(
        "compute {}",
        label.split_whitespace().last().unwrap_or(label)
    ));
    let d = MTLComputePassDescriptor::computePassDescriptor();
    let att = unsafe { d.sampleBufferAttachments().objectAtIndexedSubscript(0) };
    att.setSampleBuffer(Some(&p.buf));
    unsafe {
        att.setStartOfEncoderSampleIndex(2 * i);
        att.setEndOfEncoderSampleIndex(2 * i + 1);
    }
    Some(d)
}

fn report(device: &ProtocolObject<dyn MTLDevice>, p: &Prof) {
    let n = p.passes.len();
    let Some(data) = (unsafe { p.buf.resolveCounterRange(NSRange::new(0, 2 * n)) }) else {
        println!(
            "[pass-prof] frame {}: samples could not be resolved",
            p.frame
        );
        return;
    };
    let bytes = data.to_vec();
    let ts: Vec<u64> = bytes
        .chunks_exact(8)
        .map(|c| u64::from_le_bytes(c.try_into().unwrap()))
        .collect();
    // GPU ticks -> ns from two (cpu ns, gpu tick) pairs taken RESOLVE_AFTER frames apart.
    let cal1 = calibrate(device);
    let dg = cal1.1.saturating_sub(p.cal0.1) as f64;
    let scale = if dg > 0.0 {
        cal1.0.saturating_sub(p.cal0.0) as f64 / dg
    } else {
        1.0
    };
    let mut iv: Vec<(u64, u64, &str)> = Vec::new();
    let mut missing = 0;
    for (i, k) in p.passes.iter().enumerate() {
        let (s, e) = (
            ts.get(2 * i).copied().unwrap_or(0),
            ts.get(2 * i + 1).copied().unwrap_or(0),
        );
        // MTLCounterErrorValue (all ones) or 0: not sampled (an empty pass, or the encoder was dropped).
        if s == 0 || e == 0 || s == u64::MAX || e == u64::MAX || e < s {
            missing += 1;
            continue;
        }
        iv.push((s, e, k));
    }
    if iv.is_empty() {
        println!(
            "[pass-prof] frame {}: {} passes, no valid samples",
            p.frame, n
        );
        return;
    }
    let ms = |t: f64| t * scale / 1e6;
    iv.sort_by_key(|x| x.0);
    let (first, last) = (iv[0].0, iv.iter().map(|x| x.1).max().unwrap());
    // Busy = union of the pass intervals; the rest of the span is idle (waiting for the CPU, or between passes).
    let (mut busy, mut cur_s, mut cur_e) = (0u64, iv[0].0, iv[0].1);
    for &(s, e, _) in &iv[1..] {
        if s > cur_e {
            busy += cur_e - cur_s;
            (cur_s, cur_e) = (s, e);
        } else {
            cur_e = cur_e.max(e);
        }
    }
    busy += cur_e - cur_s;
    // Per key: passes, summed own time (start .. end; overlapping passes count fully), idle before them.
    let mut by: std::collections::HashMap<&str, (usize, u64, u64)> =
        std::collections::HashMap::new();
    let mut prev_end = first;
    for &(s, e, k) in &iv {
        let v = by.entry(k).or_default();
        v.0 += 1;
        v.1 += e - s;
        v.2 += s.saturating_sub(prev_end);
        prev_end = prev_end.max(e);
    }
    let mut rows: Vec<_> = by.into_iter().collect();
    rows.sort_by(|a, b| (b.1 .1 + b.1 .2).cmp(&(a.1 .1 + a.1 .2)));
    println!("[pass-prof] frame {}: {} passes ({} unsampled), GPU span {:.2} ms, busy {:.2} ms, idle {:.2} ms; by targets (passes, pass time, idle before):",
        p.frame, n, missing, ms((last - first) as f64), ms(busy as f64), ms((last - first - busy) as f64));
    for (k, (c, t, idle)) in rows.iter().take(24) {
        println!(
            "[pass-prof]   {:4} {:7.3} ms {:6.3} ms  {}",
            c,
            ms(*t as f64),
            ms(*idle as f64),
            k
        );
    }
}
