// SDK 3.5 (Super Mario Odyssey 1.0.0): what differs from older layers.

use super::{Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v3_5",
    from: SdkVersion(3, 5, 0),
    // The older forms 4.4 still has predate it too (as for 0.12).
    tables: &[super::v4_4::os::TABLE, super::v4_4::movie::TABLE],
    // SMO 1.0.0 main+0x731158 requires major 53, minor >= 13. (Until the 0.12 layer it took 4.4's 53.105, which also
    // passes; 0.12's 52 does not.)
    nvn_api: Some((53, 13)),
    // The 4.4 thread layout, which it ran with.
    thread_tail_shift: Some(8),
};
