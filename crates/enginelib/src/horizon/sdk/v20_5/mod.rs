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
    // The 55.15 driver's command memory per command, measured as in v0_12 (BotW 1.9.0, 413 calls): the same sizes as
    // 52.32. Its uniform buffer binds gave no steady size yet and draws without instancing were not seen; those stay
    // at the default.
    command_costs: Some(&[
        ("nvnCommandBufferBindVertexAttribState", 68),
        ("nvnCommandBufferBindVertexStreamState", 68),
        ("nvnCommandBufferBindVertexBuffer", 24),
        ("nvnCommandBufferBindTexture", 20),
        ("nvnCommandBufferDrawElementsInstanced", 44),
    ]),
    device_integers: None,
};
