// SDK 7.3 (Super Mario Odyssey 1.4.1): what differs from older layers.

use super::{Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v7_3",
    from: SdkVersion(7, 3, 0),
    tables: &[],
    // The 7.3.2 NVN headers (SMO main+0x9414c4 requires 53, >= 311). Kirby's 11.4 SDK inherits this value unverified:
    // its code does not check the version.
    nvn_api: Some((53, 311)),
    thread_tail_shift: Some(0),
};
