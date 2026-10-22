// SDK 4.4 (Mario Kart 8 Deluxe 3.0.4, Breath of the Wild): what differs from the base layer.

use super::{Entry, Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v4_4",
    from: SdkVersion(4, 4, 0),
    tables: &[TABLE],
    // MK8D main+0xa7f4d0 and BotW main+0xaf8760 require major 53, minor >= 105.
    nvn_api: Some((53, 105)),
    // The thread object has a lock history word after its fiber pointers (BotW sdk+0x96efc, the internal lock
    // takes the owner handle from +0x1b8; newer SDKs read it at +0x1b0).
    thread_tail_shift: Some(8),
};

static TABLE: &[Entry] = &[
    // ReleaseSemaphore without a count releases one (MK8D's sdk module has only this form; 7.3 has the counted one).
    Entry(
        "_ZN2nn2os16ReleaseSemaphoreEPNS0_13SemaphoreTypeE",
        adapt!(fn(sem: *mut ()) {
            crate::horizon::hle::hle_nn_os_ReleaseSemaphore(sem, 1)
        }),
    ),
    // The movie library's Decoder constructor before it took a trailing int (MK8D's subsdk0).
    Entry(
        "_ZN5movie7DecoderC1ENS_11DecoderTypeENS_19DecoderOutputFormatENS_11DecoderModeE",
        adapt!(fn(this: *mut usize, ty: i32, format: i32, mode: i32) {
            crate::horizon::movie::decoder_ctor(this, ty, format, mode, 0)
        }),
    ),
];
