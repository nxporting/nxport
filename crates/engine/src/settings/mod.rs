// Runtime settings. `Settings` holds the resolved values; `SettingsPatch` (all fields optional) is one layer of them:
// the platform's profile, the game's, the game's on this platform, a preset file, the command line or a front end.
// Layers are applied in that order onto the defaults, so each overrides the ones before it.

// The settings editor (Dear ImGui), for the builder and the overlay: on the hosts with a builder window.
#[cfg(any(target_os = "macos", windows))]
pub mod edit;
pub mod profiles;
pub mod values;

use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use struct_patch::Patch;

pub use values::*;

/// Declares a settings group: the resolved struct, its defaults and its patch (a clap argument group and a preset
/// table). Doc comments become the command-line help. Booleans are switches: `--name` turns one on, `--name=false`
/// off. `Option` fields stay single-level in the patch (`None`: not set by that layer).
macro_rules! group {
    ($(#[doc = $gd:expr])* $name:ident $heading:literal $([$($user:ident),* $(,)?])? { $($body:tt)* }) => {
        $crate::settings::group!(@munch [$($gd)*] $name $heading [$($($user)*)?] [] $($body)*);
    };
    (@munch $gd:tt $name:ident $heading:literal $users:tt [$($acc:tt)*]) => {
        $crate::settings::group!(@emit $gd $name $heading $users $($acc)*);
    };
    (@munch $gd:tt $name:ident $heading:literal $users:tt [$($acc:tt)*]
        $(#[doc = $d:expr])* $(#[arg($($a:tt)*)])* $f:ident : bool = $def:expr, $($rest:tt)*) => {
        $crate::settings::group!(@munch $gd $name $heading $users [$($acc)* { [$($d)*] $f bool = $def;
            [long, num_args = 0..=1, require_equals = true, default_missing_value = "true",
             value_parser = clap::builder::BoolishValueParser::new(), value_name = "true|false", hide_possible_values = true
             $(, $($a)*)*] [] }] $($rest)*);
    };
    (@munch $gd:tt $name:ident $heading:literal $users:tt [$($acc:tt)*]
        $(#[doc = $d:expr])* $(#[arg($($a:tt)*)])* $f:ident : Option<$t:ty> = $def:expr, $($rest:tt)*) => {
        $crate::settings::group!(@munch $gd $name $heading $users [$($acc)* { [$($d)*] $f Option<$t> = $def; [long $(, $($a)*)*] [skip_wrap] }] $($rest)*);
    };
    (@munch $gd:tt $name:ident $heading:literal $users:tt [$($acc:tt)*]
        $(#[doc = $d:expr])* $(#[arg($($a:tt)*)])* $f:ident : Vec<$t:ty> = $def:expr, $($rest:tt)*) => {
        $crate::settings::group!(@munch $gd $name $heading $users [$($acc)* { [$($d)*] $f Vec<$t> = $def; [long, value_delimiter = ',' $(, $($a)*)*] [] }] $($rest)*);
    };
    (@munch $gd:tt $name:ident $heading:literal $users:tt [$($acc:tt)*]
        $(#[doc = $d:expr])* $(#[arg($($a:tt)*)])* $f:ident : $t:ty = $def:expr, $($rest:tt)*) => {
        $crate::settings::group!(@munch $gd $name $heading $users [$($acc)* { [$($d)*] $f $t = $def; [long $(, $($a)*)*] [] }] $($rest)*);
    };
    (@emit [$($gd:expr)*] $name:ident $heading:literal [$($user:ident)*]
        $({ [$($d:expr)*] $f:ident $t:ty = $def:expr; [$($arg:tt)*] [$($pa:tt)*] })*) => {
        $(#[doc = $gd])*
        #[derive(Clone, Debug, PartialEq, ::serde::Serialize, ::struct_patch::Patch)]
        #[serde(rename_all = "kebab-case")]
        #[patch(attribute(derive(Clone, Debug, Default, PartialEq, ::clap::Args, ::serde::Serialize, ::serde::Deserialize)))]
        #[patch(attribute(command(next_help_heading = $heading)))]
        #[patch(attribute(serde(default, rename_all = "kebab-case", deny_unknown_fields)))]
        pub struct $name {
            $(
                $(#[doc = $d])*
                $(#[patch(attribute(doc = $d))])*
                #[patch(attribute(arg($($arg)*)))]
                #[patch(attribute(serde(skip_serializing_if = "Option::is_none")))]
                $(#[patch($pa)])*
                pub $f: $t,
            )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $($f: $def),* }
            }
        }

        #[cfg(any(target_os = "macos", windows))]
        impl $crate::settings::edit::EditGroup for $name {
            const HEADING: &'static str = $heading;
            const USER: &'static [&'static str] = &[$(stringify!($user)),*];
            const FIELDS: usize = [$(stringify!($f)),*].len();
            fn edit_fields(&mut self, ui: &::dear_imgui_rs::Ui, audience: $crate::settings::edit::Audience) -> bool {
                let mut changed = false;
                $(
                    if audience.shows(Self::USER.contains(&stringify!($f))) {
                        changed |= $crate::settings::edit::field(ui, &mut self.$f, stringify!($f), &[$($d),*]);
                    }
                )*
                changed
            }
        }
    };
}

group! {
    /// How the console behaves.
    System "System" [fps, handheld] {
        /// Frame rate pacing: Unlocked (up to 60+ fps, interval 1) or Stock (30 fps, interval 2)
        fps: FpsMode = FpsMode::Unlocked,
        /// Handheld mode (720p) instead of docked (1080p)
        handheld: bool = false,
    }
}

group! {
    /// Where the game, its mods, saves and shader cache are, and the libraries the engine loads.
    Files "Files" {
        /// Game dump: a directory with ExeFS/ and RomFS/
        game_dir: Option<PathBuf> = None,
        /// Mods in Ryujinx layout (romfs/, exefs/) (default: the game's data directory's mods/)
        mods_dir: Option<PathBuf> = None,
        /// Load no mods
        no_mods: bool = false,
        /// Apply no ExeFS code patches (IPS / pchtxt) from mods
        no_exefs_patches: bool = false,
        /// Save data directory (default: the platform's application data directory)
        save_dir: Option<PathBuf> = None,
        /// Shader cache directory (default: the app's, else the game's data directory's shader_cache/)
        shader_cache: Option<PathBuf> = None,
        /// Where shader stages the cache lacks are written, for the next cache build (default: the game's data
        /// directory's shader_dump_missing/)
        miss_dump_dir: Option<PathBuf> = None,
        /// Write no missing shader stages
        no_miss_dump: bool = false,
        /// Render pipelines to compile when the game starts drawing, and where new ones are added (default: the game's
        /// data directory's pipelines.txt)
        pipeline_list: Option<PathBuf> = None,
        /// Library that plays the game's movies (ffmpeg shim)
        movie_shim: Option<PathBuf> = None,
        /// libopus, for the hardware Opus decoder
        libopus: Option<PathBuf> = None,
        /// The game's lifted code, where guest code does not run natively (nxport-cli lift-game, then compile-guest
        /// with the same settings); its modules.txt beside it
        guest_library: Option<PathBuf> = None,
        /// System data archives from the console's firmware, one directory per archive named by its id (16 hex digits),
        /// for games that mount them (default: the data directory's system_data/)
        system_data_dir: Option<PathBuf> = None,
        /// The system's shared fonts (FontStandard.ttf, FontKorean.ttf, ...), for games that use them
        shared_fonts: Option<PathBuf> = None,
        /// Mods see a network; what they send is printed
        mod_log_socket: bool = false,
    }
}

group! {
    /// The window and presentation.
    Window "Display" [launch_aspect, stretch] {
        /// No window (frame dumps still work)
        headless: bool = false,
        /// Never wait for vsync (benchmark: the game runs faster than real time)
        no_vsync: bool = false,
        /// Launch aspect ratio (W:H): initial window aspect ratio when the application launches
        #[arg(alias = "aspect")]
        launch_aspect: Option<Aspect> = None,
        /// Fill the window edge to edge
        stretch: bool = false,
    }
}

group! {
    /// Controllers, keyboard, touch and rumble.
    Input "Input" [swap_ab, swap_xy, no_touch, no_rumble, rumble_scale] {
        /// Controls file (default: the application data directory's controls.cfg)
        controls_file: Option<PathBuf> = None,
        /// Physical controllers: A and B by position, as on a Switch controller
        swap_ab: bool = false,
        /// Physical controllers: X and Y by position, as on a Switch controller
        swap_xy: bool = false,
        /// Ignore touch input
        no_touch: bool = false,
        /// No rumble
        no_rumble: bool = false,
        /// Rumble strength factor
        rumble_scale: f32 = 1.0,
        /// Text the software keyboard returns (default: the game's initial text, else "Player"; "<cancel>" cancels)
        keyboard_text: Option<String> = None,
    }
}

group! {
    /// Audio output and its diagnostics.
    Audio "Audio" [audio] {
        /// Audio output
        audio: bool = true,
        /// Write the output as a stereo WAV file
        audio_wav: Option<PathBuf> = None,
    }
}

group! {
    /// How the game's code is loaded and run.
    Guest "Guest code" {
        /// Signed guest code image to map over the patched modules
        aot_image: Option<PathBuf> = None,
        /// Write the patched guest code as an image to this file, then exit
        aot_dump: Option<PathBuf> = None,
        /// Presented frame at which the image is written (with mods that patch code at run time)
        aot_dump_frame: u64 = 300,
        /// Check that no anonymous executable memory exists
        aot_audit: bool = false,
        /// Rewrite x18 uses into stubs (default: when the OS does not preserve x18)
        x18_trampolines: Option<bool> = None,
        /// Load the guest at the fixed base address
        fixed_guest_base: bool = false,
        /// Run one guest thread per core at a time, as the console does (game code only; engine calls release the core)
        guest_cores: bool = false,
        /// Bind main's imports to the game's own subsdk libraries and run their initialisers (experiment)
        run_game_subsdk: bool = false,
    }
}

group! {
    /// GPU execution.
    Graphics "Graphics" [async_pipelines, fetch_pipelines, share_pipelines] {
        /// Draw with the translated shader cache
        translated_shaders: bool = false,
        /// Attempt runtime in-process compilation of missing shaders
        dynamic_shaders: bool = true,
        /// Compile new shaders in the background instead of pausing the game. Smoother, but for now everything
        /// waiting on a compile is skipped until it is ready, which can cause visual glitches (missing objects or
        /// passes, flashes) the first time something is drawn.
        async_pipelines: bool = false,
        /// Microseconds per frame draws may wait for pipeline compiles
        async_wait_us: u64 = 2000,
        /// Pipeline compile threads
        pipeline_threads: usize = 4,
        /// Compile the pipeline list's pipelines when the game starts drawing
        precompile_pipelines: bool = true,
        /// Add new pipelines to the pipeline list
        record_pipelines: bool = true,
        /// Keep the shared pipeline list up to date: download it when the game starts (when it changed) and prepare
        /// the pipelines other players' games used, to avoid stutter the first time something is drawn.
        fetch_pipelines: bool = true,
        /// Create pipelines from the shader cache's binary archives (precompiled GPU code) when they have them
        pipeline_archive: bool = true,
        /// Compile the shader cache's pipeline list into binary archives in this directory, then exit
        build_pipeline_archive: Option<PathBuf> = None,
        /// With --build-pipeline-archive: archives that keep their function names plus a pipeline script
        /// (pipelines.mtlp-json) for compiling them for another GPU with metal-tt
        pipeline_archive_script: bool = false,
        /// Where the shared pipeline lists are downloaded from
        fetch_pipelines_url: Option<String> = None,
        /// Help other players avoid stutter: send the render pipelines this game used that the shared list lacks.
        /// Only shader names (hashes) and draw settings are sent, nothing about you or your files.
        share_pipelines: bool = false,
        /// Where shared pipelines are sent
        share_pipelines_url: Option<String> = None,
        /// Overlap a frame's GPU work with the next frame's CPU work
        early_fence: bool = true,
        /// Write-protect memory running GPU work reads, under the early fence
        early_fence_guard: bool = true,
        /// Wait for all GPU results before the game reads any
        early_fence_strict: bool = false,
        /// Occlusion report pages unreadable by the game until written
        report_guard: ReportGuard = ReportGuard::On,
        /// Command buffers on the GPU at once (1-4)
        frames_in_flight: usize = 2,
        /// Encode draws on a worker thread
        encode_worker: bool = true,
        /// Make the render-encoder calls on the encoding thread
        no_issuer: bool = false,
        /// Draws per render encoder before it is cut (0: never)
        split_draws: u32 = 600,
        /// From this presented frame on, every draw gets its own encoder (GPU profiling)
        split_draws_from: Option<u64> = None,
        /// Encoders per committed command buffer (0: one per submit)
        split_encoders: usize = 24,
        /// Commit between passes after this many ms of encoding (0: off)
        split_ms: f64 = 1.0,
        /// Run tiny render passes as compute kernels
        tiny_compute: bool = true,
        /// Unlocked frame rates (present interval 0): start frames on the display's refresh (F6 toggles)
        display_pacing: bool = true,
        /// Largest tiny-pass rectangle, in pixels
        tiny_max_px: u64 = 640 * 360,
        /// Draw runs that differ only in their uniforms as one instanced draw
        batch: bool = false,
        /// TIMESTAMP reports carry the GPU's times (the game's dynamic resolution then sees GPU load)
        gpu_timestamps: bool = false,
        /// Guest pages under textures that live in Metal
        tex_backing: TexBacking = TexBacking::Off,
        /// Free textures unused for 600 frames even if their memory is not shared
        tex_sweep_unshared: bool = false,
        /// Entries of the binding-view cache
        view_cache_cap: usize = 4096,
        /// Bind uniform buffers zero-copy where no update of the submit writes them
        ubo_zero_copy: bool = false,
        /// Which work the GPU executor does
        gfx_mode: GfxMode = GfxMode::All,
        /// The GPU backend
        gpu: GpuBackend = GpuBackend::Metal,
        /// Do not copy rarely drawn render targets back into guest memory (a workaround: the CPU reads of
        /// GPU-written memory are not trapped)
        no_rt_writeback: bool = false,
        /// Render targets in CPU-visible memory pools are copied into guest memory when the CPU touches them (off: the
        /// older copy of rarely drawn targets one frame later)
        rt_guard: bool = true,
    }
}

group! {
    /// Frame replay: re-executing one frame's submits with the scene held still (same-frame A/B measurements).
    FrameReplay "Frame replay" {
        /// F[:K[:M]]: at presented frame F (and every M frames after) run its submits K more times
        replay: Option<ReplaySpec> = None,
        /// Alternate side A and side B between repetitions
        replay_ab: bool = false,
        /// Compare each repetition's image with the previous one; dump those with more differing pixels
        replay_diff: Option<u64> = None,
        /// The image each side rendered last (1); also side A's repetition before (2)
        replay_dump: u8 = 0,
        /// Also dump cached textures of these sizes
        replay_dump_tex: Vec<Size> = Vec::new(),
        /// Side B: draws of these shaders dropped (or "all")
        replay_ab_skip: Vec<String> = Vec::new(),
        /// Side B: draws of these shaders get a 1x1 scissor (or "all")
        replay_ab_scissor: Vec<String> = Vec::new(),
        /// Side B: no draw calls (1); none on either side (2)
        replay_ab_null_draws: u8 = 0,
        /// Side B: render encoders cut every N draws
        replay_ab_split: Option<u32> = None,
        /// Capture presented frame N for GPU backend tests: its NVN commands, the memory they reach and the textures only
        /// the GPU holds
        capture_frame: Option<u64> = None,
        /// Where --capture-frame writes (default: the game's data directory's captures/frame_N)
        capture_dir: Option<PathBuf> = None,
        /// Exit once the capture is written
        capture_exit: bool = false,
    }
}

group! {
    /// Tracing: per-draw traces, code trace points and watchpoints.
    Trace "Tracing" {
        /// Trace every draw of these presented frames
        trace_frame: Vec<u64> = Vec::new(),
        /// Arm tracing when this frame is presented
        trace_from_frame: Option<u64> = None,
        /// Trace the frames after one using this shader
        trace_on_shader: Option<String> = None,
        /// Frames traced after a --trace-on-shader trigger
        trace_trigger_frames: u64 = 1,
        /// Floats of each uniform buffer printed in frame traces
        trace_ubo_floats: usize = 24,
        /// Guest code addresses to log registers at: 0xREL[:xN[>OFF]]
        trace: Vec<TracePoint> = Vec::new(),
        /// Also log x6..x28 at trace points
        trace_all_regs: bool = false,
        /// Trace the rest of the frame after a watch hit
        watch_trace: bool = false,
        /// Executor trace verbosity
        gfx_trace: u64 = 0,
        /// Log texel operations and render-target writes on textures of this size
        watch_size: Option<Size> = None,
        /// Follow this guest storage address through binds, copies and resolution
        watch_storage: Option<Addr> = None,
        /// Write watchpoint at this address
        watch_addr: Option<Addr> = None,
    }
}

group! {
    /// Dumps: frames, textures, memory, shader stages and the frame history.
    Dump "Dumps" {
        /// Write these presented frames as PPM files
        dump_frame: Vec<u64> = Vec::new(),
        /// Only the final presented image (no per-pass dumps)
        dump_final_only: bool = false,
        /// Exit after the last dumped frame
        exit_on_dump: bool = false,
        /// Dump N frames after a triggered (traced) frame
        dump_on_trigger: Option<u64> = None,
        /// Dump the first presented frame with a non-zero pixel
        dump_first_nonzero: bool = false,
        /// Keep alpha in frame dumps
        dump_alpha: bool = false,
        /// Dump every cached texture of these sizes with the frame
        dump_tex_size: Vec<Size> = Vec::new(),
        /// Print guest memory as floats after the frame: 0xADDR:LEN
        dump_mem: Vec<MemRange> = Vec::new(),
        /// Write guest memory to files after the frame: 0xADDR:LEN:PATH
        dump_mem_raw: Vec<MemDump> = Vec::new(),
        /// Write the main module's code as it is in memory: PATH[:FRAME]
        dump_main_image: Option<FrameFile> = None,
        /// Write shader stages the game uses into this directory
        dump_shaders: Option<PathBuf> = None,
        /// Write the stages' control sections into this directory
        dump_ctrl: Option<PathBuf> = None,
        /// Hexdump 512 bytes at this address on a crash
        dump_at_crash: Option<Addr> = None,
        /// Bytes of stack dumped on a crash
        crash_stackdump: Option<Addr> = None,
        /// Copy the color targets of these encoders when they end
        snapshot_enc: Vec<String> = Vec::new(),
        /// Snapshot only targets of this size
        snapshot_size: Option<Size> = None,
        /// Snapshot only targets of this Metal pixel format
        snapshot_fmt: Option<usize> = None,
        /// Also snapshot the depth attachment
        snapshot_depth: bool = false,
        /// Frames snapshotted after a --trace-on-shader trigger (with --snapshot-enc=trigger)
        snapshot_trigger_frames: u64 = 30,
        /// Ring of the last N presented frames, written with the engine journal on F9 / space
        frame_history: usize = 0,
        /// Write the frame history at this presented frame
        history_dump_at: Option<u64> = None,
        /// Also keep full render targets of this size in the history
        history_targets: Option<Size> = None,
        /// Frames of render targets kept
        history_targets_n: usize = 12,
    }
}

group! {
    /// Logging and verification.
    Log "Logging" {
        /// GPU time per pass of one frame every N frames (0: off)
        gpu_pass_profile: u64 = 0,
        /// Commands recorded but dropped, at this presented frame
        cmd_stats_at: Option<u64> = None,
        /// HLE call history at this presented frame
        hle_history_at: Option<u64> = None,
        /// Frames slower than this many ms: what happened in them
        log_hitch: Option<f64> = None,
        /// Draws by the game's call chain, frames N..N+60
        log_draw_callers: Option<u64> = None,
        /// Executor time per command kind, frames N..N+60
        log_cmd_time: Option<u64> = None,
        /// Call chains of render-target binds of this size
        log_rt_caller_size: Option<Size> = None,
        /// Every render-target bind in these frames
        log_rt_frames: Option<Span> = None,
        /// Vertex layouts of the first draws using these vertex shaders
        log_shader_draws: Vec<String> = Vec::new(),
        log_alias: bool = false,
        log_alias_new: bool = false,
        log_batchable: bool = false,
        log_bind_miss: bool = false,
        log_dirty_track: bool = false,
        log_draw_reuse: bool = false,
        log_early_fence: bool = false,
        log_hid: bool = false,
        log_hid_buttons: bool = false,
        log_image_mismatch: bool = false,
        log_movie: bool = false,
        log_occl: bool = false,
        log_pacing: bool = false,
        /// Per frame: start, present and on-screen times, and whether display pacing was on
        log_frame_pacing: bool = false,
        log_phases: bool = false,
        log_pool_reg: bool = false,
        log_refresh_hash: bool = false,
        log_refresh_stats: bool = false,
        log_replay: bool = false,
        log_rt_callers: bool = false,
        log_rt_writeback: bool = false,
        log_sizes: bool = false,
        log_svc: bool = false,
        log_swizzle: bool = false,
        log_sync: bool = false,
        log_sysevent_waits: bool = false,
        log_tail: bool = false,
        log_tex_init: bool = false,
        log_tex_mem: bool = false,
        log_texcopy: bool = false,
        log_texref: bool = false,
        log_threads: bool = false,
        log_timeline: bool = false,
        log_timestamps: bool = false,
        log_tiny_bufs: bool = false,
        log_ubo_content: bool = false,
        log_ubo_writers: bool = false,
        log_view_formats: bool = false,
        log_x18_jt: bool = false,
        list_hle_imports: bool = false,
        enc_stats: bool = false,
        dt_verify: bool = false,
        verify_tex: bool = false,
        alias_check: bool = false,
        tex_backing_log: bool = false,
    }
}

group! {
    /// Diagnostics that change what runs: validation, shader replacement, bisection aids, input scripts.
    Debug "Debugging" {
        /// Metal shader validation (invalid GPU memory accesses are reported instead of faulting); the Vulkan backend's
        /// validation layer
        validate: bool = false,
        /// Vulkan backend: write each translated stage's SPIR-V and manifest here (<code address>_<state>.spv / .json)
        dump_spirv: Option<PathBuf> = None,
        /// Per-draw debug groups and binding details in GPU error reports
        draw_details: bool = false,
        /// Encoder labels with attachment shapes
        enc_labels: bool = false,
        /// Draw only every EVERY-th draw starting at FIRST (bisection aid)
        gfx_draw_mask: Vec<DrawMask> = Vec::new(),
        /// Live single-frame glitch detector: FROM[:COUNT[:MIN_PX]]
        glitch_scan: Option<GlitchScan> = None,
        /// A glitch also writes the frame history
        glitch_dump_history: bool = false,
        /// Trap GPU-written guest memory under the early fence and log early accesses
        early_fence_trap: FenceTrap = FenceTrap::Off,
        /// Log why textures of this size are re-uploaded
        debug_refresh: Option<Size> = None,
        /// Clamp every sampler's maximum LOD
        debug_max_lod: Option<f32> = None,
        /// Draw these fragment shaders without depth test or write
        debug_no_depth_fs: Vec<String> = Vec::new(),
        /// Draw this fragment shader solid red
        debug_red_fs: Option<String> = None,
        /// Replace fragment functions with the ones these MSL files define
        debug_fs_file: Vec<PathBuf> = Vec::new(),
        /// Replace vertex functions with the ones these MSL files define
        debug_vs_file: Vec<PathBuf> = Vec::new(),
        /// Shared buffer at buffer index 26 for edited shaders to record values into
        debug_vpos: bool = false,
        /// Frame after which the value buffer is written
        debug_vpos_dump_at: u64 = 4800,
        /// Size of the value buffer, MiB
        debug_vpos_mb: usize = 16,
        /// Click the window's close button after this many seconds
        debug_close_after: Option<f64> = None,
        /// Hold keys in frame ranges: A-B:KEY[+KEY][,..]
        debug_input_script: Vec<HeldKeys> = Vec::new(),
        /// Drive key events through the window system and check the pad state
        debug_input_selftest: bool = false,
        /// Fill new GPU-only textures with all-ones bytes (2: also CPU-writable ones)
        poison_new_tex: u32 = 0,
        /// Drop draws using these shaders
        skip_shader: Vec<String> = Vec::new(),
        /// Sample the guest's main thread for this many seconds
        sample_main: Option<u64> = None,
        /// Seconds before the main-thread sampler starts
        sample_delay: u64 = 25,
    }
}

pub(crate) use group;

use crate::profile::totk::{Options as TotkOptions, OptionsPatch as TotkOptionsPatch};

/// Every setting.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Patch)]
#[serde(rename_all = "kebab-case")]
#[patch(attribute(derive(
    Clone,
    Debug,
    Default,
    PartialEq,
    clap::Parser,
    Serialize,
    Deserialize
)))]
#[patch(attribute(command(name = "nxport", version, about = "Runs a Nintendo Switch game")))]
#[patch(attribute(serde(default, rename_all = "kebab-case")))]
pub struct Settings {
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub system: System,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub files: Files,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub display: Window,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub input: Input,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub audio: Audio,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub guest: Guest,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub graphics: Graphics,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub replay: FrameReplay,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub trace: Trace,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub dump: Dump,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub log: Log,
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub debug: Debug,
    /// The Legend of Zelda: Tears of the Kingdom.
    #[patch(nesting)]
    #[patch(attribute(command(flatten)))]
    pub totk: TotkOptions,
}

static CURRENT: OnceLock<Settings> = OnceLock::new();

/// The settings of this run. Installed once, before the game starts.
pub fn get() -> &'static Settings {
    CURRENT
        .get()
        .expect("settings read before they were installed")
}

/// The settings of this run, if installed.
pub fn try_get() -> Option<&'static Settings> {
    CURRENT.get()
}

/// Installs the resolved settings (first call only).
pub fn install(s: Settings) -> &'static Settings {
    try_install(s).expect("settings");
    get()
}

/// Installs the resolved settings; an error if this process has them already.
pub fn try_install(s: Settings) -> Result<(), String> {
    CURRENT
        .set(s)
        .map_err(|_| "settings are already installed in this process".to_string())
}

/// Settings from `layers`, applied in order onto the defaults.
pub fn resolve(layers: impl IntoIterator<Item = SettingsPatch>) -> Settings {
    let mut s = Settings::default();
    for l in layers {
        s.apply(l);
    }
    s
}

/// A preset file: one layer, in TOML (`[graphics]` / `async-wait-us = 4000`, ...).
pub fn load_preset(path: &std::path::Path) -> Result<SettingsPatch, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_preset(path: &std::path::Path, p: &SettingsPatch) -> Result<(), String> {
    let mut v = toml::Value::try_from(p).map_err(|e| e.to_string())?;
    prune_empty(&mut v);
    let text = toml::to_string(&v).map_err(|e| e.to_string())?;
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

/// Removes tables left empty (groups without a set value), nested ones included.
pub fn prune_empty(v: &mut toml::Value) {
    if let toml::Value::Table(t) = v {
        for (_, g) in t.iter_mut() {
            prune_empty(g);
        }
        t.retain(|_, g| !g.as_table().is_some_and(|g| g.is_empty()));
    }
}

/// The command line: `--preset FILE` (a layer below the other arguments) and the settings.
#[derive(clap::Parser)]
#[command(name = "nxport", version, about = "Runs a Nintendo Switch game")]
struct Cli {
    /// Settings from a preset file (TOML), below the other arguments
    #[arg(long, value_name = "FILE")]
    preset: Option<PathBuf>,
    /// Game dump (same as --game-dir)
    #[arg(value_name = "GAME_DIR")]
    game: Option<PathBuf>,
    #[command(flatten)]
    settings: SettingsPatch,
}

/// The layers a command line gives: a preset file (if any), then the arguments. Errors (and --help / --version) are
/// clap's, ready to print.
pub fn from_args(
    args: impl IntoIterator<Item = std::ffi::OsString>,
) -> Result<Vec<SettingsPatch>, clap::Error> {
    use clap::{CommandFactory, FromArgMatches};
    let mut cmd = with_defaults_in_help(Cli::command());
    let matches = cmd.try_get_matches_from_mut(args)?;
    let mut cli = Cli::from_arg_matches(&matches)?;
    if cli.settings.files.game_dir.is_none() {
        cli.settings.files.game_dir = cli.game;
    }
    let mut layers = Vec::new();
    if let Some(p) = &cli.preset {
        layers.push(load_preset(p).map_err(|e| cmd.error(clap::error::ErrorKind::Io, e))?);
    }
    layers.push(cli.settings);
    Ok(layers)
}

/// Appends each setting's default to its help line (taken from `Settings::default()`).
fn with_defaults_in_help(mut cmd: clap::Command) -> clap::Command {
    let Ok(toml::Value::Table(groups)) = toml::Value::try_from(Settings::default()) else {
        return cmd;
    };
    let defaults: Vec<(String, String)> = groups
        .values()
        .filter_map(|g| g.as_table())
        .flat_map(|t| t.iter())
        .map(|(k, v)| {
            (
                k.clone(),
                match v {
                    toml::Value::String(s) => s.clone(),
                    v => v.to_string(),
                },
            )
        })
        .collect();
    for (key, value) in defaults {
        if value.is_empty() || value == "[]" || value == "false" {
            continue;
        }
        let id = key.replace('-', "_");
        if cmd.get_arguments().any(|a| a.get_id() == id.as_str()) {
            cmd = cmd.mut_arg(id, |a| {
                let help = a.get_help().map(|h| h.to_string()).unwrap_or_default();
                a.help(format!("{help} [default: {value}]"))
            });
        }
    }
    cmd
}
