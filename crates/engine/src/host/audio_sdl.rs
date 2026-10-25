// The output device where guest code is lifted (host/audio.rs): an SDL3 audio stream on the default playback device,
// whose callback asks for samples on SDL's audio thread and gets them from the ring. Only the audio subsystem is
// initialised here; the window and input layer initialises its own subsystems on the same SDL instance.

use sdl3::audio::{AudioCallback, AudioFormat, AudioSpec, AudioStream};
use std::sync::atomic::{AtomicPtr, Ordering};

/// The open stream, paused until the first pushed frame (start), so the device plays from where the renderer begins as
/// the AudioQueue does.
static STREAM: AtomicPtr<sdl3::sys::audio::SDL_AudioStream> = AtomicPtr::new(std::ptr::null_mut());

struct Source {
    buf: Vec<f32>,
}

impl AudioCallback<f32> for Source {
    /// `requested` is in samples (both channels); SDL asks for what its device buffer needs beyond what the stream
    /// already holds.
    fn callback(&mut self, stream: &mut AudioStream, requested: i32) {
        let n = (requested.max(0) as usize) & !1;
        if n == 0 {
            return;
        }
        self.buf.resize(n, 0.0);
        super::fill(&mut self.buf);
        let _ = stream.put_data_f32(&self.buf);
    }
}

/// Initialises SDL's audio subsystem and opens the stream (48 kHz stereo float; SDL converts to the device's format).
/// Runs on the main thread: the sdl3 crate takes the thread of its first `init` as SDL's main thread, which the window
/// has to be created on.
pub fn init() {
    // 10 ms device buffers, the AudioQueue's size.
    sdl3::hint::set("SDL_AUDIO_DEVICE_SAMPLE_FRAMES", "480");
    let opened = sdl3::init().and_then(|sdl| {
        let audio = sdl.audio()?;
        let spec = AudioSpec {
            freq: Some(super::RATE as i32),
            channels: Some(2),
            format: Some(AudioFormat::f32_sys()),
        };
        let stream = audio.open_playback_stream(&spec, Source { buf: Vec::new() })?;
        let found = (stream.device_name().unwrap_or_default(), audio.current_audio_driver());
        STREAM.store(stream.raw(), Ordering::Release);
        // The stream plays for the rest of the process: neither it nor the subsystem it holds is ever closed.
        std::mem::forget(stream);
        std::mem::forget(audio);
        std::mem::forget(sdl);
        Ok(found)
    });
    match opened {
        Ok((device, driver)) => println!("[+] host_audio: SDL audio on {device} ({driver})"),
        Err(e) => println!("[!] host_audio: SDL audio failed ({e}): no sound"),
    }
}

/// Starts the device the stream is bound to. SDL's audio functions may be called from any thread.
pub fn start() {
    let stream = STREAM.load(Ordering::Acquire);
    if stream.is_null() {
        return;
    }
    let ok = unsafe { sdl3::sys::audio::SDL_ResumeAudioStreamDevice(stream) };
    println!(
        "[+] host_audio: SDL output started (48 kHz stereo){}",
        if ok { String::new() } else { format!(", start failed ({})", sdl3::get_error()) }
    );
}
