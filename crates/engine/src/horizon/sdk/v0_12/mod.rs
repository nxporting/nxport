// SDK 0.12 (Breath of the Wild 1.0.0, built before the Switch's launch): what differs from the base layer.

use super::{Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v0_12",
    from: SdkVersion(0, 12, 0),
    // The older forms 4.4 still has (ReleaseSemaphore without a count, the movie Decoder constructor without its
    // trailing int) predate it, so this SDK has them too.
    tables: &[super::v4_4::os::TABLE, super::v4_4::movie::TABLE],
    // BotW 1.0.0 main+0xafc450 creates its graphics system only when the device reports major 52 and minor >= 32
    // (the 0.12 headers); otherwise it skips it, and main+0xafeb38 then reads a null object.
    nvn_api: Some((52, 32)),
    // Thread names read at the 4.4 layout ('Prepare Thread' comes out whole).
    thread_tail_shift: Some(8),
    audio_handle_state: None,
};
