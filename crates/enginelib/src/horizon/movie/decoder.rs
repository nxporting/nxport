// horizon::movie::Decoder.

use super::*;

// ── horizon::movie::Decoder ──────────────────────────────────────────────────────────────────────────────────────────────
pub(crate) const IN_SLOTS: usize = 4;
pub(crate) const OUT_SLOTS: usize = 16;
pub(crate) const MAX_PENDING: usize = 4;

/// Decoder calls come from the game's input threads (send) and its output side (acquire / release): one lock for the
/// decoders' queues and slots. A frame is decoded outside it, under its decoder's Codec lock (d_send_input): a VP9 frame
/// takes tens of milliseconds, and under this lock the game could neither take ready frames nor decode audio meanwhile. Lock order: a Codec, then this.
pub(crate) static DECODER_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// A decoder's decoding side: held while it decodes or drains, and while its frames are queued (so they keep their order).
pub(crate) struct Codec {
    scratch: Vec<u8>,
    /// The destructor closed the media decoder (d_dtor, under this lock): a send that took its handle before then and
    /// waited here must not use it.
    closed: bool,
}
pub(crate) struct OutFrame {
    data: Vec<u8>,
    pts: i64,
    flags: u32,
}
pub(crate) struct Decoder {
    out_format: Format,
    /// DecoderMode 2: output buffers name the client's NVN texture (Buffer data = NVNtexture*, with its device and
    /// sync), into which the library blits each frame on the GPU (DecoderImpl::CopyOutputBufferToClientTextureAndRelease).
    to_texture: bool,
    events: [usize; 4], // input available, output available, format changed, error
    dec: usize,         // media decoder handle
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
    codec: std::sync::Arc<std::sync::Mutex<Codec>>,
    /// --log-movie: inputs sent / outputs acquired / acquire attempts with nothing ready.
    n_in: u64,
    n_out: u64,
    n_empty: u64,
    /// Times the video clock restarted from a late frame, and the last video frame's pts.
    n_rebased: u64,
    last_pts: i64,
    /// The video's frame interval: the smallest step between frames' pts (some frames share one).
    frame_us: i64,
    /// formatChangedEvent signaled for the current output format.
    format_reported: bool,
    /// Video frames decoded since the decoder started or was flushed, their decode time in all and the longest, and how
    /// many took longer than a 30 fps frame: printed when the movie ends (report_decode).
    decodes: u64,
    decode_us: u64,
    decode_max_us: u64,
    decode_slow: u64,
    /// Video: when the first frame since the start or a flush was handed out, and its presentation time. Later frames
    /// are handed out once their time has come (d_acquire_output).
    clock: Option<(std::time::Instant, i64)>,
}
/// One line on how fast a movie decoded, so a stutter can be told apart from a decoder that cannot keep up.
/// Printed every 300 frames too (so far), so a log cut short by quitting mid-movie still has it.
pub(crate) fn report_decode(d: &mut Decoder) {
    print_decode(d, "");
    (d.decodes, d.decode_us, d.decode_max_us, d.decode_slow) = (0, 0, 0, 0);
}
pub(crate) fn print_decode(d: &Decoder, so_far: &str) {
    if d.is_video && d.decodes > 0 {
        println!(
            "[+] movie{so_far}: {} video frames ({}x{}) decoded in {:.1} ms on average, the longest {:.1} ms, {} over 33.3 ms; the clock restarted {} times",
            d.decodes,
            d.width,
            d.height,
            d.decode_us as f64 / d.decodes as f64 / 1000.0,
            d.decode_max_us as f64 / 1000.0,
            d.decode_slow,
            d.n_rebased
        );
    }
}
/// The implementation, or None after the destructor ran (methods then return Status_NullDecoder).
pub(crate) unsafe fn dec<'a>(this: *mut usize) -> Option<&'a mut Decoder> {
    (*this.add(1) as *mut Decoder).as_mut()
}
/// Whether AcquireInputBuffer would hand out a buffer (see d_acquire_input).
pub(crate) fn input_available(d: &Decoder) -> bool {
    !d.eos_in && d.pending.len() < MAX_PENDING && d.inputs.iter().any(|i| i.1)
}
/// Signals inputBufferAvailableEvent if an input buffer can be acquired.
pub(crate) fn signal_input(d: &Decoder) {
    if input_available(d) {
        signal(d, 0);
    }
}
/// Decoders alive (their boxes), so the movie clock never signals one that is gone. Taken under DECODER_LOCK.
pub(crate) static LIVE: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
/// Held-back video frames: (decoder box, when it is due), and the thread that signals "output available" then.
pub(crate) static DUE: (std::sync::Mutex<Vec<(usize, std::time::Instant)>>, std::sync::Condvar) =
    (std::sync::Mutex::new(Vec::new()), std::sync::Condvar::new());

/// Signals output available for decoder `d` at `at` (the game's player waits for that signal, then asks again).
pub(crate) fn signal_output_at(d: &Decoder, at: std::time::Instant) {
    static THREAD: std::sync::Once = std::sync::Once::new();
    THREAD.call_once(|| {
        std::thread::Builder::new().name("movie-clock".into()).spawn(movie_clock).expect("spawn movie clock thread");
    });
    let mut due = DUE.0.lock().unwrap();
    let key = d as *const Decoder as usize;
    if !due.iter().any(|&(k, t)| k == key && t <= at) {
        due.push((key, at));
        DUE.1.notify_one();
    }
}

pub(crate) fn movie_clock() {
    let mut due = DUE.0.lock().unwrap();
    loop {
        let now = std::time::Instant::now();
        let ready: Vec<usize> = due.iter().filter(|&&(_, t)| t <= now).map(|&(k, _)| k).collect();
        due.retain(|&(_, t)| t > now);
        if !ready.is_empty() {
            drop(due);
            let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let live = LIVE.lock().unwrap();
            for k in ready {
                if live.contains(&k) {
                    signal(unsafe { &*(k as *const Decoder) }, 1);
                }
            }
            drop(live);
            drop(_guard);
            due = DUE.0.lock().unwrap();
            continue;
        }
        due = match due.iter().map(|&(_, t)| t).min() {
            Some(t) => DUE.1.wait_timeout(due, t.saturating_duration_since(now)).unwrap().0,
            None => DUE.1.wait(due).unwrap(),
        };
    }
}

/// Signals one of the game's events. A stopped decoder signals nothing: the game tears down the threads that handle
/// them after Stop.
pub(crate) fn signal(d: &Decoder, which: usize) {
    if d.started && d.events[which] != 0 {
        crate::horizon::os::objects::signal_event_at(d.events[which]);
    }
}
/// The decoder's Codec lock (taken before DECODER_LOCK by those that touch the media decoder), or None once it is gone.
pub(crate) unsafe fn codec_of(this: *mut usize) -> Option<std::sync::Arc<std::sync::Mutex<Codec>>> {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    dec(this).map(|d| d.codec.clone())
}
pub(crate) unsafe extern "C" fn d_dtor(this: *mut usize) {
    // Freed only once no frame is decoding: its Codec lock first (lock order, see DECODER_LOCK).
    let codec = codec_of(this);
    let mut codec = codec.as_ref().map(|c| c.lock().unwrap_or_else(|e| e.into_inner()));
    // A send waiting for this lock with the handle in hand sees this and leaves it alone.
    if let Some(c) = codec.as_mut() {
        c.closed = true;
    }
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    if *this.add(1) != 0 {
        LIVE.lock().unwrap().retain(|&k| k != *this.add(1));
        let mut d = Box::from_raw(*this.add(1) as *mut Decoder);
        report_decode(&mut d);
        if d.dec != 0 {
            (media().md_close)(d.dec as *mut c_void);
        }
        *this.add(1) = 0;
    }
}
pub(crate) unsafe extern "C" fn d_configure(this: *mut usize, md: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let s = media();
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
    let fmt = match (d.is_video, d.out_format) {
        (false, _) => Format::Pcm16,
        (true, Format::Pcm16) => Format::Rgba,
        (true, f) => f,
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
        "[movie] Decoder configured: track {} {} {}x{} {} Hz x{} -> output format {:?}",
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
pub(crate) unsafe extern "C" fn d_register_events(this: *mut usize, ev: *const usize) -> u32 {
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
pub(crate) fn fill_format(d: &Decoder, md: *mut usize) {
    if d.is_video {
        md_set(md, "width", Val::I32(d.width));
        md_set(md, "height", Val::I32(d.height));
        md_set(
            md,
            "stride",
            Val::I32(if d.out_format == Format::Nv12 {
                d.width
            } else {
                d.width * 4
            }),
        );
        md_set(md, "slice-height", Val::I32(d.height));
        if d.out_format == Format::Nv12 {
            // Plane offsets inside an NV12 output buffer (the decoder writes tightly packed planes).
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
pub(crate) unsafe extern "C" fn d_output_format(this: *mut usize, md: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    fill_format(d, md);
    OK
}
pub(crate) unsafe extern "C" fn d_start(this: *mut usize) -> u32 {
    if log_enabled() {
        println!("[movie] t={}ms decoder start", log_ms());
    }
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    d.started = true;
    d.clock = None;
    signal_input(d);
    OK
}
pub(crate) unsafe extern "C" fn d_stop(this: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    d.started = false;
    d.clock = None;
    OK
}
pub(crate) unsafe extern "C" fn d_flush(this: *mut usize) -> u32 {
    // The decoder is reset only once no frame is decoding: its Codec lock first (lock order, see DECODER_LOCK).
    let Some(codec) = codec_of(this) else {
        return NULL_DECODER;
    };
    let _codec = codec.lock().unwrap_or_else(|e| e.into_inner());
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    report_decode(d);
    if d.dec != 0 {
        (media().md_flush)(d.dec as *mut c_void);
    }
    for i in d.inputs.iter_mut() {
        i.1 = true;
    }
    d.outputs.iter_mut().for_each(|o| *o = None);
    d.ready.clear();
    d.pending.clear();
    d.clock = None;
    d.eos_in = false;
    d.eos_out = false;
    signal_input(d);
    OK
}
/// Moves decoded frames into free output slots and signals the game.
pub(crate) fn distribute(d: &mut Decoder) {
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
/// A decoded frame or audio packet as the decoder handed it out: its bytes, presentation time, and picture size.
pub(crate) struct Received {
    data: Vec<u8>,
    pts: i64,
    width: i32,
    height: i32,
}

/// Everything the media decoder `handle` has ready, under its Codec lock only; and whether it then had nothing more
/// (its last answer negative: at the end of the stream, the end).
pub(crate) unsafe fn receive_all(handle: usize, codec: &mut Codec) -> (Vec<Received>, bool) {
    let s = media();
    let mut out = Vec::new();
    loop {
        let (mut pts, mut w, mut h, mut stride, mut need) = (0i64, 0, 0, 0, 0);
        let cap = codec.scratch.len() as c_int;
        let n = (s.md_receive)(
            handle as *mut c_void,
            codec.scratch.as_mut_ptr(),
            cap,
            &mut pts,
            &mut w,
            &mut h,
            &mut stride,
            &mut need,
        );
        if n == -2 {
            codec.scratch.resize(need as usize, 0);
            continue;
        }
        if n > 0 {
            out.push(Received { data: codec.scratch[..n as usize].to_vec(), pts, width: w, height: h });
            continue;
        }
        return (out, n < 0);
    }
}

/// Queues what receive_all took (under DECODER_LOCK), and the end-of-stream output once the decoder is done after the
/// last input.
pub(crate) fn deliver(d: &mut Decoder, received: Vec<Received>, done: bool) {
    for r in received {
        // formatChangedEvent before the first output (and on size changes), as MediaCodec-style decoders report the
        // output format first: Kirby's guide movies (no audio track) wait for it before starting playback.
        if d.is_video && (r.width != d.width || r.height != d.height || !d.format_reported) {
            d.width = r.width;
            d.height = r.height;
            d.format_reported = true;
            signal(d, 2);
        } else if !d.is_video && !d.format_reported {
            d.format_reported = true;
            signal(d, 2);
        }
        d.pending.push_back(OutFrame { data: r.data, pts: r.pts, flags: 0 });
    }
    if done && d.eos_in && !d.eos_out {
        d.eos_out = true;
        d.pending.push_back(OutFrame { data: Vec::new(), pts: 0, flags: BUFFER_FLAG_EOS });
    }
    distribute(d);
}
pub(crate) unsafe extern "C" fn d_acquire_input(this: *mut usize, idx: *mut i32, remaining: *mut i32) -> u32 {
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
pub(crate) unsafe extern "C" fn d_get_input(this: *mut usize, idx: i32, b: *mut Buffer) -> u32 {
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
pub(crate) unsafe extern "C" fn d_send_input(
    this: *mut usize,
    idx: i32,
    off: i32,
    size: i32,
    pts: i64,
    flags: u32,
) -> u32 {
    // Taken from the input slot under DECODER_LOCK (the slot is free again at once: the game may refill it while this
    // frame decodes, so the bytes are copied), then decoded under the decoder's Codec lock alone (see DECODER_LOCK).
    let (handle, codec, data, size, eos) = {
        let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(d) = dec(this) else {
            return NULL_DECODER;
        };
        let Some(slot) = d.inputs.get_mut(idx as usize) else {
            return ERR_INVALID_RANGE;
        };
        let eos = flags & BUFFER_FLAG_EOS != 0;
        let start = (off.max(0) as usize).min(slot.0.len());
        let size = size.max(0).min(slot.0.len() as i32 - start as i32);
        let data = slot.0[start..start + size as usize].to_vec();
        slot.1 = true;
        if eos {
            d.eos_in = true;
        }
        (d.dec, d.codec.clone(), data, size, eos)
    };
    let mut codec = codec.lock().unwrap_or_else(|e| e.into_inner());
    if codec.closed {
        return NULL_DECODER;
    }
    let t0 = std::time::Instant::now();
    let (failed, received, done) = if handle != 0 {
        let failed = (media().md_send)(handle as *mut c_void, data.as_ptr(), size, pts, eos as c_int) < 0;
        let (received, done) = receive_all(handle, &mut codec);
        (failed, received, done)
    } else {
        (false, Vec::new(), false)
    };
    let us = t0.elapsed().as_micros() as u64;
    // Still holding the Codec lock: frames of two sends are queued in the order they were decoded.
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    if handle != 0 {
        if failed {
            signal(d, 3);
        }
        deliver(d, received, done);
        if d.is_video {
            d.decodes += 1;
            d.decode_us += us;
            d.decode_max_us = d.decode_max_us.max(us);
            d.decode_slow += (us > 33_333) as u64;
            if d.decodes % 300 == 0 {
                print_decode(d, " (so far)");
            }
        }
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
pub(crate) unsafe extern "C" fn d_acquire_output(
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
    // A video frame is handed out once its presentation time has come, counted from the first one. The game's player
    // shows the newest frame it has each time it draws (TotK takes one per draw: frames handed out as decoded would
    // play at the draw rate, twice as fast at 60 fps); the console draws them at the video's 30. Without a new frame the
    // player keeps showing the last one. A frame that comes out a whole frame interval (frame_us) or more after its time
    // (the decoder fell behind, the game stalled, or it took the first frame long before playing: TotK decodes a
    // movie's first frame while its event loads) restarts the clock from it: the console shows the next frame one draw
    // later and never catches up, and handing out every late frame would play them at the draw rate.
    if d.is_video {
        if let Some(&slot) = d.ready.front() {
            let pts = d.outputs[slot].as_ref().map_or(0, |f| f.pts);
            let now = std::time::Instant::now();
            // A frame due more than a second from now follows a seek (Mario Kart 8 Deluxe's menu jumps between previews
            // in one movie): the clock restarts from it, as from a late frame. Timed from the old start it would wait out
            // the whole jump while the menu waits for its preview. A contiguous stream cannot be that far
            // ahead: the decoder holds only a few output slots of frames.
            let jumped = d.clock.is_some_and(|(t0, p0)| (pts - p0) - (now - t0).as_micros() as i64 > 1_000_000);
            match d.clock {
                None => d.clock = Some((now, pts)),
                Some(_) if jumped => {
                    d.clock = Some((now, pts));
                    d.n_rebased += 1;
                }
                Some((t0, p0)) => {
                    let late = (now - t0).as_micros() as i64 - (pts - p0);
                    if late < 0 {
                        d.n_empty += 1;
                        signal_output_at(d, t0 + std::time::Duration::from_micros((pts - p0) as u64));
                        return OUTPUT_NOT_AVAILABLE;
                    }
                    if pts > d.last_pts {
                        let step = pts - d.last_pts;
                        d.frame_us = if d.frame_us == 0 { step } else { d.frame_us.min(step) };
                    }
                    if d.frame_us > 0 && late >= d.frame_us {
                        d.clock = Some((now, pts));
                        d.n_rebased += 1;
                    }
                }
            }
            d.last_pts = pts;
        }
    }
    let Some(slot) = d.ready.pop_front() else {
        d.n_empty += 1;
        return OUTPUT_NOT_AVAILABLE;
    };
    let f = d.outputs[slot].as_ref().unwrap();
    d.n_out += 1;
    if log_enabled() && (d.n_out <= 5 || d.n_out % 60 == 0 || f.flags != 0) {
        println!("[movie] t={}ms {} output #{}: slot {} pts {} us, {} bytes, flags {:#x} (empty polls {}, clock restarts {})",
            log_ms(), if d.is_video { "video" } else { "audio" }, d.n_out, slot, f.pts, f.data.len(), f.flags, d.n_empty,
            d.n_rebased);
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
pub(crate) unsafe extern "C" fn d_get_output(this: *mut usize, idx: i32, b: *mut Buffer) -> u32 {
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
    if d.to_texture && d.is_video {
        // The library copies the frame into the texture and releases the slot in one step
        // (CopyOutputBufferToClientTextureAndRelease): the game does not release it.
        let r = copy_to_client_texture(d.width, d.height, d.out_format, &f.data, f.pts, b);
        return if r == OK { release_output(d, idx) } else { r };
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
/// A frame into the client's texture (DecoderMode 2): the Buffer's data is the NVNtexture, its sync is signaled when
/// the copy is done (here at once: the texels are in the texture's memory when this returns). The game sizes these
/// textures for 1080 rows (nvn::layout::storage_target).
unsafe fn copy_to_client_texture(w: i32, h: i32, format: Format, data: &[u8], pts: i64, b: *mut Buffer) -> u32 {
    use crate::nvn::api::{texture_write_texels, NvnCopyRegion, NvnSync, NvnTexture};
    let tex = (*b).data as *const NvnTexture;
    if tex.is_null() || format != Format::Rgba || data.len() < (w * h * 4) as usize {
        return ERR_INVALID_RANGE;
    }
    let region = NvnCopyRegion { x: 0, y: 0, z: 0, w: w.min((*tex).width), h: h.min((*tex).height), d: 1 };
    if region.w == w {
        texture_write_texels(tex, std::ptr::null(), &region, data.as_ptr(), true);
    } else {
        let rows: Vec<u8> = data.chunks(w as usize * 4).take(region.h as usize).flat_map(|r| &r[..region.w as usize * 4]).copied().collect();
        texture_write_texels(tex, std::ptr::null(), &region, rows.as_ptr(), true);
    }
    if (*b).nvn_sync != 0 {
        (*((*b).nvn_sync as *mut NvnSync)).serial = 0;
    }
    (*b).range_offset = 0;
    (*b).range_length = 0;
    (*b).int64_data = pts;
    OK
}
pub(crate) unsafe extern "C" fn d_get_output_with_format(
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
pub(crate) unsafe extern "C" fn d_output_buffer_format(this: *mut usize, _idx: i32, md: *mut usize) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    fill_format(d, md);
    OK
}
pub(crate) unsafe extern "C" fn d_output_size(this: *mut usize, size: *mut i32, align: *mut i32) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    let s = if d.is_video {
        if d.out_format == Format::Nv12 {
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
pub(crate) unsafe extern "C" fn d_release_output(this: *mut usize, idx: i32) -> u32 {
    let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(d) = dec(this) else {
        return NULL_DECODER;
    };
    release_output(d, idx)
}
/// Frees an output slot (the decoder lock held).
unsafe fn release_output(d: &mut Decoder, idx: i32) -> u32 {
    match d.outputs.get_mut(idx as usize) {
        Some(o) => *o = None,
        None => return ERR_INVALID_RANGE,
    }
    // Input becomes available here only if the back-pressure on pending frames held it (the event reports a change,
    // not a level: signalling it on every release would wake the game's input handler after it shut down).
    let held = !input_available(d);
    distribute(d);
    if held {
        signal_input(d);
    }
    OK
}
pub(crate) unsafe extern "C" fn d_last_error(_this: *mut usize) -> u32 {
    OK
}
guest_vtable!(DECODER_VTABLE, register_decoder_hle, [
    d_dtor(_),
    d_dtor(_),
    d_configure(_, _),
    d_register_events(_, _),
    d_output_format(_, _),
    d_start(_),
    d_stop(_),
    d_flush(_),
    d_flush(_),
    d_acquire_input(_, _, _),
    d_get_input(_, _, _),
    d_send_input(_, _, _, _, _, _),
    d_acquire_output(_, _, _, _, _),
    d_get_output(_, _, _),
    d_output_size(_, _, _),
    d_release_output(_, _),
    d_last_error(_),
    d_get_output_with_format(_, _, _, _),
    d_output_buffer_format(_, _, _),
]);
/// movie::DecoderOutputFormat where the Decoder constructor takes a mode: 1 PCM16, 3 bytes R, G, B, A, 4 NV12.
pub(crate) fn output_format(raw: i32) -> Format {
    match raw {
        1 => Format::Pcm16,
        4 => Format::Nv12,
        _ => Format::Rgba,
    }
}
/// movie::DecoderOutputFormat where it does not (library 3.5.1's DecoderImpl::GetOutputBufferSize and ConvertToArgb):
/// 1 PCM16, 2 bytes A, R, G, B, 3 NV12.
pub(crate) fn output_format_before_modes(raw: i32) -> Format {
    match raw {
        1 => Format::Pcm16,
        3 => Format::Nv12,
        _ => Format::Argb,
    }
}
pub unsafe extern "C" fn decoder_ctor(this: *mut usize, ty: i32, format: i32, mode: i32, _n: i32) {
    new_decoder(this, ty, output_format(format));
    if mode == 2 {
        if let Some(d) = dec(this) {
            d.to_texture = true;
            println!("[movie] decoder outputs into the client's NVN textures");
        }
    }
}
/// The constructor before it took a mode.
pub(crate) unsafe extern "C" fn decoder_ctor_before_modes(this: *mut usize, ty: i32, format: i32) {
    new_decoder(this, ty, output_format_before_modes(format));
}
pub(crate) unsafe fn new_decoder(this: *mut usize, ty: i32, format: Format) {
    println!("[movie] Decoder(type={ty}, output format {format:?})");
    *this = vtable(&DECODER_VTABLE, true);
    let d = Decoder {
        out_format: format,
        to_texture: false,
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
        codec: std::sync::Arc::new(std::sync::Mutex::new(Codec { scratch: vec![0u8; 1 << 20], closed: false })),
        n_in: 0,
        n_out: 0,
        n_empty: 0,
        n_rebased: 0,
        last_pts: 0,
        frame_us: 0,
        format_reported: false,
        decodes: 0,
        decode_us: 0,
        decode_max_us: 0,
        decode_slow: 0,
        clock: None,
    };
    let ptr = Box::into_raw(Box::new(d)) as usize;
    {
        let _guard = DECODER_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        LIVE.lock().unwrap().push(ptr);
    }
    *this.add(1) = ptr;
}
