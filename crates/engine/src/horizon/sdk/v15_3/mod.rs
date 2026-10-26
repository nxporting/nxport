// SDK 15.3 (Tears of the Kingdom 1.1.2; Super Mario Bros. Wonder's 16.2 inherits it): what differs from older layers.

use super::{Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v15_3",
    from: SdkVersion(15, 3, 0),
    tables: &[],
    // The 15.3.2 NVN headers (TotK main+0xeac0e4 and Wonder main+0x5a48c8 require 55, >= 15).
    nvn_api: Some((55, 15)),
    thread_tail_shift: Some(0),
};
