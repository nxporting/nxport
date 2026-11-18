
// The output device on macOS and iOS (host/audio.rs): an AudioQueue (AudioToolbox) whose callback refills its buffers
// from the ring.

use std::ffi::c_void;

/// Frames per AudioQueue buffer (10 ms) and buffers in flight.
const QUEUE_FRAMES: usize = 480;
const QUEUE_BUFFERS: usize = 3;

#[repr(C)]
struct AudioStreamBasicDescription {
    sample_rate: f64,
    format_id: u32,
    format_flags: u32,
    bytes_per_packet: u32,
    frames_per_packet: u32,
    bytes_per_frame: u32,
    channels_per_frame: u32,
    bits_per_channel: u32,
    reserved: u32,
}

#[repr(C)]
struct AudioQueueBuffer {
    capacity: u32,
    data: *mut c_void,
    byte_size: u32,
    user_data: *mut c_void,
    packet_description_capacity: u32,
    packet_descriptions: *mut c_void,
    packet_description_count: u32,
}

type AudioQueueRef = *mut c_void;
type Callback = unsafe extern "C" fn(*mut c_void, AudioQueueRef, *mut AudioQueueBuffer);

#[link(name = "AudioToolbox", kind = "framework")]
extern "C" {
    fn AudioQueueNewOutput(
        format: *const AudioStreamBasicDescription,
        callback: Callback,
        user: *mut c_void,
        run_loop: *const c_void,
        run_loop_mode: *const c_void,
        flags: u32,
        out: *mut AudioQueueRef,
    ) -> i32;
    fn AudioQueueAllocateBuffer(
        aq: AudioQueueRef,
        size: u32,
        out: *mut *mut AudioQueueBuffer,
    ) -> i32;
    fn AudioQueueEnqueueBuffer(
        aq: AudioQueueRef,
        buf: *mut AudioQueueBuffer,
        n: u32,
        descs: *const c_void,
    ) -> i32;
    fn AudioQueueStart(aq: AudioQueueRef, at: *const c_void) -> i32;
}

/// Fills one queue buffer from the ring (silence for what is missing) and hands it back to the queue.
unsafe extern "C" fn refill(_user: *mut c_void, aq: AudioQueueRef, buf: *mut AudioQueueBuffer) {
    super::fill(std::slice::from_raw_parts_mut((*buf).data as *mut f32, QUEUE_FRAMES * 2));
    (*buf).byte_size = (QUEUE_FRAMES * 8) as u32;
    AudioQueueEnqueueBuffer(aq, buf, 0, std::ptr::null());
}

/// iOS: playback apps must say so, or the default (ambient) audio session category is muted by the ring / silent switch
/// and by other apps. Playback category, then activate the session, before the queue starts.
#[cfg(target_os = "ios")]
unsafe fn activate_session() {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    #[link(name = "AVFAudio", kind = "framework")]
    extern "C" {
        static AVAudioSessionCategoryPlayback: *const AnyObject;
    }
    let session: *mut AnyObject = msg_send![class!(AVAudioSession), sharedInstance];
    if session.is_null() {
        return;
    }
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let ok: bool = msg_send![session, setCategory: AVAudioSessionCategoryPlayback, error: &mut err];
    let active: bool = msg_send![session, setActive: true, error: &mut err];
    println!(
        "[+] host_audio: audio session category Playback {}, active {}",
        ok, active
    );
}

pub fn init() {}

/// Creates the queue, primes its buffers and starts it, on the thread that pushed the first frame.
pub fn start() {
    unsafe { start_queue() }
}

unsafe fn start_queue() {
    #[cfg(target_os = "ios")]
    activate_session();
    let format = AudioStreamBasicDescription {
        sample_rate: super::RATE as f64,
        format_id: u32::from_be_bytes(*b"lpcm"),
        format_flags: 1 | 8, // float, packed
        bytes_per_packet: 8,
        frames_per_packet: 1,
        bytes_per_frame: 8,
        channels_per_frame: 2,
        bits_per_channel: 32,
        reserved: 0,
    };
    let mut aq: AudioQueueRef = std::ptr::null_mut();
    // No run loop: the queue calls back on its own thread.
    let err = AudioQueueNewOutput(
        &format,
        refill,
        std::ptr::null_mut(),
        std::ptr::null(),
        std::ptr::null(),
        0,
        &mut aq,
    );
    if err != 0 {
        println!(
            "[!] host_audio: AudioQueueNewOutput failed ({}): no sound",
            err
        );
        return;
    }
    for _ in 0..QUEUE_BUFFERS {
        let mut buf: *mut AudioQueueBuffer = std::ptr::null_mut();
        if AudioQueueAllocateBuffer(aq, (QUEUE_FRAMES * 8) as u32, &mut buf) == 0 {
            refill(std::ptr::null_mut(), aq, buf);
        }
    }
    let err = AudioQueueStart(aq, std::ptr::null());
    println!(
        "[+] host_audio: AudioQueue output started (48 kHz stereo){}",
        if err != 0 {
            format!(", start failed ({})", err)
        } else {
            String::new()
        }
    );
}
