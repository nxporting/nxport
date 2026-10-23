// nxport-cli: shader caches, app bundles and frame-dump conversion for nxport.

use nxport_build::{extract_shaders, ppm2png, tiny_kernels};
// App bundles, signed images and Metal shader caches are built on macOS hosts.
#[cfg(target_os = "macos")]
use nxport_build::{ipa, macho, msl_specialize, pack, shader_cache_build, translation};

use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nxport-cli", version, about = "Shader caches, app bundles and frame-dump conversion for nxport")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    #[cfg(target_os = "macos")]
    /// Package a macOS .app with a game dump, its mods and its shader cache
    Pack(PackArgs),
    /// Shader stages and caches
    #[command(subcommand)]
    Shaders(Shaders),
    #[cfg(target_os = "macos")]
    /// Build an iOS app (.ipa) for a game dump
    Ipa {
        /// Game dump (ExeFS/ and RomFS/)
        #[arg(long)]
        dump: PathBuf,
        /// Mods (Ryujinx layout) to put into the app
        #[arg(long)]
        mods: Option<PathBuf>,
        /// The iOS shader cache (shaders build-ios); without it the app translates shaders as the game loads them
        #[arg(long)]
        cache: Option<PathBuf>,
        /// The settings the app starts with (a preset file)
        #[arg(long)]
        settings: Option<PathBuf>,
        /// Where the .ipa goes
        #[arg(long, short, default_value = "target")]
        out: PathBuf,
        /// Put the game files into the app (large) instead of the app's Documents/Game
        #[arg(long)]
        bundle_game: bool,
        /// Request no entitlements (for signing setups that refuse them)
        #[arg(long)]
        no_entitlements: bool,
        /// Bundle identifier (default: com.nxport.<game> for games with a profile, else com.nxport.t<program id>)
        #[arg(long)]
        bundle_id: Option<String>,
    },
    #[cfg(target_os = "macos")]
    /// The signed guest code image an iOS app runs: the game's code patched as the app patches it at startup
    GuestImage {
        /// Game dump (ExeFS/ and RomFS/)
        #[arg(long)]
        dump: PathBuf,
        /// Mods (Ryujinx layout); none without it
        #[arg(long)]
        mods: Option<PathBuf>,
        /// The app's settings (a preset file)
        #[arg(long)]
        settings: Option<PathBuf>,
        /// The dylib to write
        #[arg(long, short)]
        out: PathBuf,
        /// A macOS dylib (to test the image on the Mac) instead of the iOS framework binary
        #[arg(long)]
        macos: bool,
    },
    #[cfg(any(target_os = "macos", windows))]
    /// Run a frame capture (nxport --capture-frame) on a GPU backend and check it against its references, pass by
    /// pass; --record writes the references
    GpuReplay {
        /// The capture directory
        capture: PathBuf,
        /// Record the references instead of checking against them
        #[arg(long)]
        record: bool,
        /// Check every pass instead of stopping at the first that differs
        #[arg(long)]
        all: bool,
        /// Where diff images go (default: the capture's diff/)
        #[arg(long)]
        out: Option<PathBuf>,
        /// Where the references are (default: the capture's ref/)
        #[arg(long)]
        refs: Option<PathBuf>,
        /// Also write every target read back as a PNG (to --out)
        #[arg(long)]
        images: bool,
        /// The backend: metal or vulkan (default: metal where there is Metal, else vulkan)
        #[arg(long, default_value = "")]
        backend: String,
        /// Print the commands of these passes
        #[arg(long, value_delimiter = ',')]
        trace_pass: Vec<usize>,
        /// PASS:K: run pass PASS only up to its K-th draw and stop (bisecting where two backends part)
        #[arg(long, value_parser = parse_cut)]
        cut_draw: Option<(usize, u32)>,
        /// List the capture's GPU-only textures (storage, description, subresource, content hash) and exit
        #[arg(long)]
        list_textures: bool,
        /// Engine options (as nxport takes them, e.g. --shader-cache=DIR), after --
        #[arg(last = true)]
        engine: Vec<String>,
    },
    #[cfg(target_os = "macos")]
    /// Write a synthetic frame capture (short NVN sequences with hand-encoded shaders) for gpu-replay
    GpuScenario {
        /// Scenario name (or "list")
        name: String,
        /// The capture directory to write
        out: Option<PathBuf>,
    },
    /// Convert a frame dump (PPM) to PNG
    Ppm2png {
        input: PathBuf,
        output: PathBuf,
        /// Downscale factor
        #[arg(default_value_t = 1)]
        downscale: usize,
    },
    /// Lift a module (NSO) to C for hosts without the native runtime
    Lift {
        /// The module (e.g. ExeFS/main)
        nso: PathBuf,
        /// Output directory for the C files
        #[arg(long, short)]
        out: PathBuf,
        /// Name prefix of the generated files and tables
        #[arg(long, default_value = "main")]
        name: String,
        /// Guest address the module is loaded at (hex)
        #[arg(long, default_value = "7000100000", value_parser = parse_hex)]
        base: u64,
    },
    /// Lift a game to C as the runtime loads and patches it (every module), for hosts without the native runtime
    LiftGame {
        /// Game dump (ExeFS/ and RomFS/)
        dump: PathBuf,
        /// Output directory for the C files
        #[arg(long, short)]
        out: PathBuf,
    },
    /// Compile lifted C (lift / lift-game) into the guest library
    CompileGuest {
        /// The directory of the C files
        c_dir: PathBuf,
        /// The library to write (e.g. guest.dll)
        #[arg(long, short)]
        out: PathBuf,
        /// The C compiler command (clang, or "zig cc")
        #[arg(long, default_value = "clang")]
        cc: String,
        /// Files compiled at once (default: the CPU count)
        #[arg(short)]
        j: Option<usize>,
    },
}

fn parse_cut(s: &str) -> Result<(usize, u32), String> {
    let (p, k) = s.split_once(':').ok_or("PASS:K")?;
    Ok((p.parse().map_err(|_| "PASS:K")?, k.parse().map_err(|_| "PASS:K")?))
}

fn parse_hex(s: &str) -> Result<u64, String> {
    u64::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| e.to_string())
}

#[derive(clap::Args)]
struct PackArgs {
    /// Game dump (ExeFS/ and RomFS/)
    #[arg(short, long)]
    dump: PathBuf,
    /// Mods directory
    #[arg(short, long)]
    mods: Option<PathBuf>,
    /// Shader cache directory
    #[arg(short, long, default_value = "target/shader_cache")]
    cache: PathBuf,
    /// The .app to write (default: target/<game title>.app)
    #[arg(short, long)]
    out: Option<PathBuf>,
    /// Settings the app starts with (a preset file)
    #[arg(long)]
    settings: Option<PathBuf>,
    /// Link the game files instead of copying them
    #[arg(long)]
    symlink: bool,
}

#[derive(Subcommand)]
enum Shaders {
    /// Extract every shader stage embedded in a RomFS
    Extract {
        /// The game's RomFS directory
        #[arg(long)]
        romfs: PathBuf,
        /// Output directory
        #[arg(long, default_value = "shader_dump_romfs")]
        out: PathBuf,
        /// RomFS directories to scan (default: the game profile's, else all of RomFS)
        #[arg(long, value_delimiter = ',')]
        dirs: Vec<String>,
        /// Also write each stage's control section by weak hash here (for builds of runtime dumps without them)
        #[arg(long)]
        ctrl_out: Option<PathBuf>,
    },
    #[cfg(target_os = "macos")]
    /// Build the Metal shader cache from stage dumps (incremental: stages translated before are reused)
    Build {
        /// Stage dump directories (*.code)
        #[arg(required_unless_present = "index_only")]
        dumps: Vec<PathBuf>,
        /// Output cache directory
        #[arg(long, short)]
        out: PathBuf,
        /// Shared control sections by weak hash (the engine's --dump-ctrl), for stages without a .ctrl file
        #[arg(long)]
        ctrl_dir: Option<PathBuf>,
        /// Only write each stage's specialized MSL there (s_<hash>.metal) and stop
        #[arg(long)]
        msl_out: Option<PathBuf>,
        /// Only rewrite the index from the last build's state (constant-buffer extents recomputed from the kept MSL)
        #[arg(long)]
        index_only: bool,
        /// Parallel jobs (default: a third of the cores)
        #[arg(short)]
        j: Option<usize>,
        #[command(flatten)]
        translation: translation::Translation,
    },
    #[cfg(target_os = "macos")]
    /// Compile a macOS cache's specialized MSL for iOS 16+
    BuildIos {
        /// The macOS cache (its work/ directory holds the MSL)
        cache: PathBuf,
        /// Output directory
        out: PathBuf,
        /// For the simulator
        #[arg(long)]
        sim: bool,
        /// Parallel jobs
        #[arg(short, default_value_t = 6)]
        j: usize,
    },
    #[cfg(target_os = "macos")]
    /// Specialize one translated MSL file for Metal
    Specialize {
        input: PathBuf,
        output: PathBuf,
        /// Print the index metadata as one JSON line instead of a summary
        #[arg(long)]
        meta: bool,
        #[command(flatten)]
        translation: translation::Translation,
    },
    /// Append the tiny-pass compute kernels to specialized stages
    TinyKernels {
        /// Output directory
        out: PathBuf,
        /// Specialized stages (.metal)
        #[arg(required = true)]
        stages: Vec<PathBuf>,
    },
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match Cli::parse().command {
        #[cfg(target_os = "macos")]
        Command::Pack(a) => {
            let mods = a.mods.or_else(|| {
                let dump_mods = a.dump.join("mods");
                dump_mods.is_dir().then_some(dump_mods)
            });
            pack::pack_app(&pack::PackOptions {
                dump_dir: &a.dump,
                mods_dir: mods.as_deref().filter(|p| p.is_dir()),
                shader_cache_dir: a.cache.is_dir().then_some(a.cache.as_path()),
                out_app: a.out.as_deref(),
                settings: a.settings.as_deref(),
                symlink_assets: a.symlink,
            })?;
        }
        Command::Shaders(Shaders::Extract { romfs, out, dirs, ctrl_out }) => {
            let dirs = if dirs.is_empty() { extract_shaders::default_dirs(&romfs) } else { dirs };
            let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
            extract_shaders::extract_all(&romfs, &out, &dirs, ctrl_out.as_deref(), None)?;
        }
        #[cfg(target_os = "macos")]
        Command::Shaders(Shaders::Build { dumps, out, ctrl_dir, msl_out, index_only, j, translation }) => {
            translation::install(translation);
            // A third of the cores by default: the build is thousands of short tool launches.
            let jobs = j.unwrap_or_else(|| (std::thread::available_parallelism().map_or(6, |n| n.get()) / 3).max(2));
            shader_cache_build::build(&shader_cache_build::Options { dump_dirs: dumps, out_dir: out, ctrl_dir, jobs, msl_out, index_only, on_progress: None })?;
        }
        #[cfg(target_os = "macos")]
        Command::Shaders(Shaders::BuildIos { cache, out, sim, j }) => shader_cache_build::build_ios(&cache, &out, j, sim, None)?,
        #[cfg(target_os = "macos")]
        Command::Shaders(Shaders::Specialize { input, output, meta, translation }) => {
            translation::install(translation);
            let text = std::fs::read_to_string(&input)?;
            let (specialized, attrs, colors, varyings) = msl_specialize::specialize(&text);
            std::fs::write(&output, &specialized)?;
            if meta {
                println!("{}", serde_json::json!({
                    "attrs": attrs, "colors": colors, "varyings": varyings,
                    "cl": msl_specialize::uses_cbuf_limits(&specialized),
                    "cbext": shader_cache_build::cbuf_extents(&specialized),
                    "sbro": shader_cache_build::readonly_sbuffers(&specialized),
                }));
            } else {
                println!("[+] Specialized {}: attrs={:?}, colors={:?}, varyings={:?}", input.display(), attrs, colors, varyings);
            }
        }
        Command::Shaders(Shaders::TinyKernels { out, stages }) => {
            std::fs::create_dir_all(&out)?;
            let (mut f_ok, mut v_ok) = (0, 0);
            for f in &stages {
                let text = std::fs::read_to_string(f)?;
                let k = if text.contains("\nfragment ") {
                    tiny_kernels::fragment_kernel(&text).inspect(|_| f_ok += 1)
                } else if text.contains("\nvertex ") {
                    tiny_kernels::vertex_kernel(&text).inspect(|_| v_ok += 1)
                } else {
                    None
                };
                if let (Some(k), Some(name)) = (k, f.file_name()) {
                    std::fs::write(out.join(name), format!("{}\n{}", text, k))?;
                }
            }
            println!("[+] {} stages: {} fragment kernels, {} vertex kernels", stages.len(), f_ok, v_ok);
        }
        #[cfg(target_os = "macos")]
        Command::Ipa { dump, mods, cache, settings, out, bundle_game, no_entitlements, bundle_id } => {
            let edits = settings.as_deref().map(nxport::settings::load_preset).transpose()?.unwrap_or_default();
            std::fs::create_dir_all(&out)?;
            let ipa = ipa::build(
                &ipa::Options {
                    dump: &dump,
                    mods: mods.as_deref(),
                    cache: cache.as_deref(),
                    out_dir: &out,
                    edits,
                    bundle_game,
                    entitlements: !no_entitlements,
                    bundle_id,
                },
                &|step| println!("[+] {step}"),
            )?;
            println!("[+] Wrote {}", ipa.display());
        }
        #[cfg(target_os = "macos")]
        Command::GuestImage { dump, mods, settings, out, macos } => {
            let edits = settings.as_deref().map(nxport::settings::load_preset).transpose()?.unwrap_or_default();
            let img = nxport::guest_image(ipa::app_settings(&dump, mods.as_deref(), edits))?;
            let (platform, name) = if macos {
                (macho::Platform::MacOS, "@rpath/libnxport_guest.dylib")
            } else {
                (macho::Platform::IOS, ipa::GUEST_INSTALL_NAME)
            };
            macho::write_guest_dylib(&img, &out, platform, name)?;
        }
        #[cfg(target_os = "macos")]
        Command::GpuScenario { name, out } => match out {
            Some(out) if name != "list" => nxport::write_scenario(&name, &out)?,
            _ => println!("{}", nxport::capture::scenarios::NAMES.join("\n")),
        },
        #[cfg(any(target_os = "macos", windows))]
        Command::GpuReplay { capture, list_textures: true, .. } => nxport::capture::replay::list_gpu_textures(&capture)?,
        #[cfg(any(target_os = "macos", windows))]
        Command::GpuReplay { capture, record, all, out, refs, images, backend, trace_pass, cut_draw, engine, .. } => {
            let layers = nxport::settings::from_args(std::iter::once("nxport".into()).chain(engine.into_iter().map(Into::into)))
                .map_err(|e| e.to_string())?;
            let opts = nxport::capture::replay::Options { record, out, all, refs, images, backend, trace_passes: trace_pass, cut_draw };
            let report = nxport::replay_capture(&capture, &opts, layers)?;
            if report.first_failure.is_some() {
                std::process::exit(1);
            }
        }
        Command::Ppm2png { input, output, downscale } => ppm2png::convert_ppm_to_png(&input, &output, downscale)?,
        Command::Lift { nso, out, name, base } => {
            let r = nxport_build::lift::lift_nso(&nso, &name, base, &out)?;
            report_lift(&nso.display().to_string(), &r);
        }
        Command::LiftGame { dump, out } => {
            let s = nxport_build::lift::game_settings(&dump, Default::default());
            for (name, r) in nxport_build::lift::lift_game(s, &out)? {
                report_lift(&name, &r);
            }
        }
        Command::CompileGuest { c_dir, out, cc, j } => {
            let cc: Vec<String> = cc.split_whitespace().map(String::from).collect();
            let jobs = j.unwrap_or_else(|| std::thread::available_parallelism().map_or(4, |n| n.get()));
            nxport_build::lift::compile(&c_dir, &out, &cc, jobs)?;
            println!("[+] {}", out.display());
        }
    }
    Ok(())
}

fn report_lift(what: &str, r: &nxport_build::lift::Output) {
    println!(
        "[+] {}: {} functions, {} instructions, {} jump tables, {} unresolved computed branches, {} C files",
        what,
        r.functions,
        r.instructions,
        r.jump_tables,
        r.unresolved_computed,
        r.files.len()
    );
    let mut unsupported: Vec<_> = r.unsupported.iter().collect();
    unsupported.sort_by(|a, b| b.1.cmp(a.1));
    for (reason, n) in unsupported.iter().take(20) {
        println!("    {n:>9} not lifted: {reason}");
    }
}

