// SDK 4.4 (Mario Kart 8 Deluxe 3.0.4, Breath of the Wild): what differs from the base layer.

mod movie;
mod os;

use super::{Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v4_4",
    from: SdkVersion(4, 4, 0),
    tables: &[os::TABLE, movie::TABLE],
    // MK8D main+0xa7f4d0 and BotW main+0xaf8760 require major 53, minor >= 105.
    nvn_api: Some((53, 105)),
    // The thread object has a lock history word after its fiber pointers (BotW sdk+0x96efc, the internal lock
    // takes the owner handle from +0x1b8; newer SDKs read it at +0x1b0).
    thread_tail_shift: Some(8),
};
