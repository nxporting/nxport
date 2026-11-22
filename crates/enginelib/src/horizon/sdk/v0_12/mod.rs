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
    nvn_procs: &[],
    audio_handle_state: None,
    // The 52.32 driver's command memory per command, measured as GetCommandMemoryUsed deltas between consecutive
    // calls on one command buffer under Ryujinx (Breath of the Wild 1.0.0). Its control memory stayed 0.
    command_costs: Some(&[
        ("nvnCommandBufferBindVertexAttribState", 68),
        ("nvnCommandBufferBindVertexStreamState", 68),
        ("nvnCommandBufferBindVertexBuffer", 24),
        ("nvnCommandBufferBindTexture", 20),
        ("nvnCommandBufferBindUniformBuffer", 20),
        ("nvnCommandBufferDrawElementsInstanced", 44),
        ("nvnCommandBufferDrawElements", 60),
    ]),
    // The 52.32 driver's nvnDeviceGetInteger answers where they differ from the defaults, read with breakpoints on its
    // entry and its result under Ryujinx (Breath of the Wild 1.0.0): COMMAND_BUFFER_CONTROL_ALIGNMENT 8,
    // SHADER_SCRATCH_MEMORY_ALIGNMENT 0x1000. The rest of what the game asks matches the defaults.
    device_integers: Some(&[(0x14, 8), (0x19, 0x1000)]),
};
