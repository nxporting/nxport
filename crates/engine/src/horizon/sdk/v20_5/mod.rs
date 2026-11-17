// SDK 20.5 (Breath of the Wild 1.9.0): what differs from older layers.

use super::{Layer, SdkVersion};

pub(super) static LAYER: Layer = Layer {
    name: "v20_5",
    from: SdkVersion(20, 5, 0),
    tables: &[],
    nvn_api: None,
    thread_tail_shift: None,
    nvn_procs: &[],
    // BotW 1.9.0's sdk: OpenAudioRenderer (sdk+0x221580) stores the service object and a 0x30-byte state block in the
    // handle; Get/SetAudioRendererVoiceDropParameter and ...RenderingTimeLimit read the block through the second word.
    audio_handle_state: Some(true),
    command_costs: None,
};
