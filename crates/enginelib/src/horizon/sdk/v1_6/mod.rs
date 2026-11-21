// SDK 1.6 (Splatoon 2 1.0.0): what differs from older layers.

use super::{Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v1_6",
    from: SdkVersion(1, 6, 0),
    // The older forms 4.4 still has predate it too (as for 0.12 and 3.5).
    tables: &[super::v4_4::os::TABLE, super::v4_4::movie::TABLE],
    // Splatoon 2 1.0.0 main+0xe67590 (sead::GameFrameworkNx::initializeGraphicsSystem) requires major 53, minor >= 5;
    // with 0.12's 52 it never created sead::Graphics and waitStartDisplayLoop_ read a null instance.
    nvn_api: Some((53, 5)),
    thread_tail_shift: Some(8),
    nvn_procs: &[],
    audio_handle_state: None,
    command_costs: None,
    device_integers: None,
};
