// nxport-cli: shader caches, app bundles and frame-dump conversion for nxport.

use nxport_build::{extract_shaders, ipa, macho, msl_specialize, pack, ppm2png, shader_cache_build, tiny_kernels, translation};

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "nxport-cli", version, about = "Shader caches, app bundles and frame-dump conversion for nxport")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Package a macOS .app with a game dump, its mods and its shader cache
    Pack(PackArgs),
    /// Shader stages and caches
    #[command(subcommand)]
    Shaders(Shaders),
    /// Render pipeline lists (the engine's --pipeline-list)
    #[command(subcommand)]
    Pipelines(Pipelines),
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
    /// Convert a frame dump (PPM) to PNG
    Ppm2png {
        input: PathBuf,
        output: PathBuf,
        /// Downscale factor
        #[arg(default_value_t = 1)]
        downscale: usize,
    },
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
enum Pipelines {
    /// Merge pipeline lists into one, without duplicates or entries the shader cache cannot build
    Merge {
        /// The game's shader cache (its index.txt names the stages entries may use)
        #[arg(long)]
        cache: PathBuf,
        /// Output list (may be one of the inputs)
        #[arg(long, short)]
        out: PathBuf,
        /// Lists to merge
        #[arg(required = true)]
        lists: Vec<PathBuf>,
    },
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
        Command::Pipelines(Pipelines::Merge { cache, out, lists }) => merge_pipelines(&cache, &out, &lists)?,
        Command::Shaders(Shaders::Extract { romfs, out, dirs, ctrl_out }) => {
            let dirs = if dirs.is_empty() { extract_shaders::default_dirs(&romfs) } else { dirs };
            let dirs: Vec<&str> = dirs.iter().map(String::as_str).collect();
            extract_shaders::extract_all(&romfs, &out, &dirs, ctrl_out.as_deref(), None)?;
        }
        Command::Shaders(Shaders::Build { dumps, out, ctrl_dir, msl_out, index_only, j, translation }) => {
            translation::install(translation);
            // A third of the cores by default: the build is thousands of short tool launches.
            let jobs = j.unwrap_or_else(|| (std::thread::available_parallelism().map_or(6, |n| n.get()) / 3).max(2));
            shader_cache_build::build(&shader_cache_build::Options { dump_dirs: dumps, out_dir: out, ctrl_dir, jobs, msl_out, index_only, on_progress: None })?;
        }
        Command::Shaders(Shaders::BuildIos { cache, out, sim, j }) => shader_cache_build::build_ios(&cache, &out, j, sim, None)?,
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
        Command::Ppm2png { input, output, downscale } => ppm2png::convert_ppm_to_png(&input, &output, downscale)?,
    }
    Ok(())
}

fn merge_pipelines(cache: &Path, out: &Path, lists: &[PathBuf]) -> Result<(), Box<dyn std::error::Error>> {
    use nxport::metal::pipeline_list::{header, Entry};
    let kinds = nxport::metal::shader_cache::stage_kinds(&std::fs::read_to_string(cache.join("index.txt"))?);
    let mut entries = std::collections::BTreeSet::new();
    let (mut titles, mut builds) = (std::collections::BTreeSet::new(), std::collections::BTreeSet::new());
    let (mut read, mut rejected) = (0usize, 0usize);
    for list in lists {
        for line in std::fs::read_to_string(list)?.lines() {
            if let Some(h) = line.strip_prefix("# nxport pipelines title ") {
                let mut f = h.split_whitespace();
                titles.extend(f.next().map(str::to_string));
                builds.extend(f.nth(1).map(str::to_string));
                continue;
            }
            if line.trim().is_empty() || line.starts_with('#') {
                continue;
            }
            read += 1;
            match Entry::parse(line).filter(|e| e.valid(|s| kinds.get(s).copied())) {
                Some(e) => {
                    entries.insert(e);
                }
                None => rejected += 1,
            }
        }
    }
    if titles.len() > 1 {
        return Err(format!("lists of different games: {titles:?}").into());
    }
    let title = titles.iter().next().and_then(|t| u64::from_str_radix(t, 16).ok()).unwrap_or(0);
    let build = if builds.len() == 1 { builds.iter().next().unwrap().as_str() } else { "-" };
    let mut text = header(title, build) + "\n";
    for e in &entries {
        text += &e.line();
        text.push('\n');
    }
    std::fs::write(out, text)?;
    println!("{} entries ({read} read, {rejected} rejected) -> {}", entries.len(), out.display());
    Ok(())
}
