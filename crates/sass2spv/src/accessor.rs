// What the translator asks about the GPU and the guest: the stage's code and constant bank, and fixed answers for
// state only known at draw time. The answers are the ones the engine's shader cache has always been built with.

use std::cell::Cell;

use crate::types::{AlphaTestOp, AttributeType, InputTopology, SamplerType, SetBindingPair, TessPatchType, TessSpacing, TextureFormat};

/// Binding numbers go to spirv-cross verbatim (--msl-decoration-binding), so each Metal resource class gets its own
/// range: buffer(0..17) constant buffers, buffer(18..) storage buffers, texture(0..) sampled textures,
/// texture(IMAGE_BASE..) storage images.
pub const STORAGE_BUFFER_BASE: i32 = 18;
pub const IMAGE_BASE: i32 = 96;
/// Binding 0 is the translator's support buffer; the game's driver constant buffer 0 moves to the unused 17.
pub const DRIVER_CBUF_BINDING: i32 = 17;

/// What the translation targets, where its output depends on it. The default is the engine's Metal path (MSL, and the
/// SPIR-V it was checked against).
#[derive(Clone, Debug)]
pub struct Host {
    /// The host maps NVN's -1..1 depth range itself; without it a GL depth mode is converted in the vertex stage.
    pub depth_clip_control: bool,
    pub shader_float64: bool,
    pub fragment_shader_interlock: bool,
    /// Vertex fetches convert *_UI2F / *_I2F data to float; without them the vertex stage does.
    pub scaled_vertex_formats: bool,
    pub image_gather_extended: bool,
    /// gl_BaseVertex and friends (DrawParameters): unused while draw parameters come from the driver constant buffer.
    pub draw_parameters: bool,
    /// A fragment stage that can discard writes its depth (FragCoord.z, a promise of depth greater or equal) when the
    /// pipeline has a depth target: on Apple GPUs the depth such a shader leaves is not always bit-identical to the depth
    /// a later draw of the same triangle is tested with (the Metal path's msl_specialize does the same).
    pub discard_writes_depth: bool,
    /// Constant buffer reads beyond the bound size return zero (as on the GPU) by a check in the shader, against sizes the
    /// host writes into the support buffer (properties::SUPPORT_CBUF_LIMITS_OFFSET): for hosts whose bounds checking does
    /// not cover uniform buffers (MoltenVK). SPIR-V only; the Metal path's msl_specialize does its own.
    pub cbuf_limits: bool,
    /// No NoContraction on floating-point results except those a position output depends on (transforms::
    /// position_precise), invariant positions: for hosts whose shader compiler turns NoContraction into unoptimized calls
    /// (MoltenVK's SPIRV-Cross, one call per add or multiply: about 3x the GPU time in TotK's Hateno).
    pub reduced_precision: bool,
    /// Every resource of the stage in this descriptor set, textures numbered from TEXTURE_BASE_ONE_SET (Vulkan, one set
    /// per stage). None: Ryujinx's sets (uniform buffers 0, images 1, storage buffers 2, textures 3).
    pub stage_set: Option<i32>,
}

impl Default for Host {
    fn default() -> Self {
        Host {
            depth_clip_control: true,
            shader_float64: true,
            fragment_shader_interlock: true,
            scaled_vertex_formats: true,
            image_gather_extended: true,
            draw_parameters: true,
            discard_writes_depth: false,
            cbuf_limits: false,
            reduced_precision: false,
            stage_set: None,
        }
    }
}

/// Textures' first binding when a stage has one set (Host::stage_set): above uniform (0..=17), storage (18..) and image
/// (96..) bindings.
pub const TEXTURE_BASE_ONE_SET: i32 = 128;

/// Draw-time state the translator assumes (GpuGraphicsState).
#[derive(Clone, Debug)]
pub struct GraphicsState {
    pub early_z_force: bool,
    pub topology: InputTopology,
    pub tess_cw: bool,
    pub tess_patch_type: TessPatchType,
    pub tess_spacing: TessSpacing,
    pub alpha_to_coverage_enable: bool,
    pub alpha_to_coverage_dither_enable: bool,
    pub viewport_transform_disable: bool,
    pub depth_mode: bool,
    pub program_point_size_enable: bool,
    pub point_size: f32,
    pub alpha_test_compare: AlphaTestOp,
    pub alpha_test_reference: f32,
    pub attribute_types: [AttributeType; 32],
    pub has_constant_buffer_draw_parameters: bool,
    pub fragment_output_types: [AttributeType; 8],
    pub dual_source_blend_enable: bool,
    pub y_negate_enabled: bool,
    pub origin_upper_left: bool,
    pub halve_primitive_id: bool,
}

impl GraphicsState {
    /// The engine's state: registers are untyped on the Switch GPU, so inputs and outputs are raw 32-bit unsigned
    /// values (msl_specialize retypes them per draw); NVN has no alpha test.
    pub fn engine() -> GraphicsState {
        GraphicsState {
            early_z_force: false,
            topology: InputTopology::Points,
            tess_cw: false,
            tess_patch_type: TessPatchType::Triangles,
            tess_spacing: TessSpacing::EqualSpacing,
            alpha_to_coverage_enable: false,
            alpha_to_coverage_dither_enable: false,
            viewport_transform_disable: false,
            depth_mode: false,
            program_point_size_enable: false,
            point_size: 1.0,
            alpha_test_compare: AlphaTestOp::Always,
            alpha_test_reference: 0.0,
            attribute_types: [AttributeType::Uint; 32],
            has_constant_buffer_draw_parameters: true,
            fragment_output_types: [AttributeType::Uint; 8],
            dual_source_blend_enable: false,
            y_negate_enabled: false,
            origin_upper_left: false,
            halve_primitive_id: false,
        }
    }

    /// default(GpuGraphicsState), what compute definitions hold.
    pub fn zeroed() -> GraphicsState {
        GraphicsState {
            point_size: 0.0,
            alpha_test_compare: AlphaTestOp::Always,
            attribute_types: [AttributeType::Float; 32],
            has_constant_buffer_draw_parameters: false,
            fragment_output_types: [AttributeType::Float; 8],
            ..GraphicsState::engine()
        }
    }
}

pub struct GpuAccessor {
    code: Vec<u64>,
    is_compute: bool,
    stage: Vec<u8>,
    cb1_offset: i32,
    next_texture: Cell<i32>,
    next_image: Cell<i32>,
    next_compute_buffer: Cell<i32>,
    pub local_size: [i32; 3],
    pub shared_memory_size: i32,
    pub local_memory_size: i32,
    pub log: bool,
    /// Draw-time state to translate for (a backend that specializes per pipeline); None: GraphicsState::engine().
    pub state: Option<GraphicsState>,
    pub host: Host,
}

impl GpuAccessor {
    /// `sass`: the code (from the shader program header for graphics stages); `stage`: the whole NVN stage data the
    /// constant bank offset refers to.
    pub fn new(sass: &[u8], is_compute: bool, stage: &[u8], cb1_offset: i32) -> GpuAccessor {
        let code = sass.chunks_exact(8).map(|c| u64::from_le_bytes(c.try_into().unwrap())).collect();
        GpuAccessor {
            code,
            is_compute,
            stage: stage.to_vec(),
            cb1_offset,
            next_texture: Cell::new(0),
            next_image: Cell::new(0),
            next_compute_buffer: Cell::new(1),
            local_size: [1, 1, 1],
            shared_memory_size: 0,
            local_memory_size: 0,
            log: false,
            state: None,
            host: Host::default(),
        }
    }

    /// The constant bank's offset in NVN stage data when the control section is not known: the program ends with a
    /// branch to itself and NOP padding to a 32-byte group; the bank starts at the next 0x100 boundary after a zero
    /// gap. -1 when that shape is not found.
    pub fn find_const_bank(stage: &[u8]) -> i32 {
        const BRA_SELF: u64 = 0xe2400fffff07000f;
        const NOP: u64 = 0x50b0000000070f00;
        const CODE_START: usize = 0x80;
        const MASK: u64 = 0xffffffffff00ffff;
        let word = |at: usize| u64::from_le_bytes(stage[at..at + 8].try_into().unwrap());
        let mut q = CODE_START;
        while q + 8 <= stage.len() {
            if word(q) & MASK == BRA_SELF & MASK {
                let mut g = CODE_START + (q + 8 - CODE_START).div_ceil(0x20) * 0x20;
                while g + 0x20 <= stage.len() {
                    let w: Vec<u64> = (0..4).map(|i| word(g + i * 8)).collect();
                    let all_zero = w.iter().all(|&v| v == 0);
                    let padding = w[1..].iter().all(|&v| v == NOP || v == 0);
                    if all_zero || !padding {
                        break;
                    }
                    g += 0x20;
                }
                let bank = (g + 0xff) & !0xff;
                let gap_zero = stage[g.min(stage.len())..bank.min(stage.len())].iter().all(|&b| b == 0);
                if gap_zero && bank < stage.len() {
                    return bank as i32;
                }
            }
            q += 8;
        }
        -1
    }

    /// The code from byte `address` on (GetCode).
    pub fn code(&self, address: u64) -> &[u64] {
        let index = (address / 8) as usize;
        self.code.get(index..).unwrap_or(&[])
    }

    /// Reads constant buffer 1 (indirect-branch jump tables).
    pub fn constant_buffer1_read(&self, offset: i32) -> u32 {
        let at = self.cb1_offset as i64 + offset as i64;
        if self.cb1_offset < 0 || offset < 0 || at + 4 > self.stage.len() as i64 {
            return 0;
        }
        u32::from_le_bytes(self.stage[at as usize..at as usize + 4].try_into().unwrap())
    }

    fn next(c: &Cell<i32>, n: i32) -> i32 {
        let v = c.get();
        c.set(v + n);
        v
    }

    /// The descriptor set for resources of Ryujinx's set `set`.
    pub fn set_for(&self, set: i32) -> i32 {
        self.host.stage_set.unwrap_or(set)
    }

    pub fn create_constant_buffer_binding(&self, index: i32) -> SetBindingPair {
        if self.is_compute {
            SetBindingPair::new(self.set_for(0), Self::next(&self.next_compute_buffer, 1))
        } else {
            SetBindingPair::new(self.set_for(0), if index == 0 { DRIVER_CBUF_BINDING } else { index })
        }
    }

    pub fn create_image_binding(&self, count: i32, _is_buffer: bool) -> SetBindingPair {
        SetBindingPair::new(self.set_for(1), IMAGE_BASE + Self::next(&self.next_image, count))
    }

    /// Compute kernels have no vertex buffers and may use up to 16 storage buffers: their constant and storage
    /// buffers are numbered compactly from 1 (0 is the support buffer) so they fit Metal's 31 buffer slots.
    pub fn create_storage_buffer_binding(&self, index: i32) -> SetBindingPair {
        if self.is_compute {
            SetBindingPair::new(self.set_for(2), Self::next(&self.next_compute_buffer, 1))
        } else {
            SetBindingPair::new(self.set_for(2), STORAGE_BUFFER_BASE + index)
        }
    }

    pub fn create_texture_binding(&self, count: i32, _is_buffer: bool) -> SetBindingPair {
        let base = if self.host.stage_set.is_some() { TEXTURE_BASE_ONE_SET } else { 0 };
        SetBindingPair::new(self.set_for(3), base + Self::next(&self.next_texture, count))
    }

    pub fn create_extra_set(&self) -> i32 {
        4
    }

    pub fn graphics_state(&self) -> GraphicsState {
        self.state.clone().unwrap_or_else(GraphicsState::engine)
    }

    pub fn log_msg(&self, message: &str) {
        if self.log {
            eprintln!("[log] {message}");
        }
    }

    // Fixed answers (IGpuAccessor queries).
    pub fn constant_buffer_use(&self) -> u32 {
        u32::MAX
    }
    pub fn has_unaligned_storage_buffer(&self) -> bool {
        false
    }
    pub fn host_gather_bias_precision(&self) -> i32 {
        0
    }
    pub fn host_reduced_precision(&self) -> bool {
        self.host.reduced_precision
    }
    pub fn host_has_front_facing_bug(&self) -> bool {
        false
    }
    pub fn host_has_vector_indexing_bug(&self) -> bool {
        false
    }
    pub fn host_storage_buffer_offset_alignment(&self) -> i32 {
        16
    }
    /// Apple Silicon SIMD width.
    pub fn host_subgroup_size(&self) -> i32 {
        32
    }
    pub fn host_supports_bgra_format(&self) -> bool {
        true
    }
    pub fn host_supports_fragment_shader_interlock(&self) -> bool {
        self.host.fragment_shader_interlock
    }
    pub fn host_supports_fragment_shader_ordering_intel(&self) -> bool {
        false
    }
    pub fn host_supports_geometry_shader(&self) -> bool {
        false
    }
    pub fn host_supports_geometry_shader_passthrough(&self) -> bool {
        false
    }
    pub fn host_supports_image_load_formatted(&self) -> bool {
        true
    }
    pub fn host_supports_layer_vertex_tessellation(&self) -> bool {
        true
    }
    pub fn host_supports_non_constant_texture_offset(&self) -> bool {
        true
    }
    pub fn host_supports_scaled_vertex_formats(&self) -> bool {
        self.host.scaled_vertex_formats
    }
    pub fn host_supports_separate_sampler(&self) -> bool {
        true
    }
    pub fn host_supports_shader_ballot(&self) -> bool {
        true
    }
    pub fn host_supports_shader_barrier_divergence(&self) -> bool {
        true
    }
    pub fn host_supports_shader_float64(&self) -> bool {
        self.host.shader_float64
    }
    /// The interface's default (the engine's accessor does not override it).
    pub fn host_supports_shader_non_uniform_indexing(&self) -> bool {
        false
    }
    pub fn host_supports_snorm_buffer_texture_format(&self) -> bool {
        true
    }
    pub fn host_supports_texture_gather_offsets(&self) -> bool {
        true
    }
    pub fn host_supports_texture_shadow_lod(&self) -> bool {
        true
    }
    pub fn host_supports_transform_feedback(&self) -> bool {
        false
    }
    pub fn host_supports_viewport_index_vertex_tessellation(&self) -> bool {
        true
    }
    pub fn host_supports_viewport_mask(&self) -> bool {
        false
    }
    pub fn host_supports_depth_clip_control(&self) -> bool {
        self.host.depth_clip_control
    }
    pub fn sampler_array_length_from_pool(&self) -> i32 {
        0
    }
    pub fn sampler_type(&self, _handle: i32, _cbuf_slot: i32) -> SamplerType {
        SamplerType::TEXTURE_2D
    }
    pub fn texture_array_length_from_buffer(&self, _slot: i32) -> i32 {
        0
    }
    pub fn texture_array_length_from_pool(&self) -> i32 {
        0
    }
    pub fn texture_coord_normalized(&self, _handle: i32, _cbuf_slot: i32) -> bool {
        true
    }
    pub fn texture_format(&self, _handle: i32, _cbuf_slot: i32) -> TextureFormat {
        TextureFormat::R8G8B8A8Unorm
    }
    pub fn transform_feedback_enabled(&self) -> bool {
        false
    }
}
