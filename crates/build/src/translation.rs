// Shader translation options of `shaders build` / `shaders specialize`. Set once per command (`install`); every one
// that changes the generated shaders is part of the cache fingerprint (`fingerprint`).


#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum Contract {
    /// No fused math.
    None,
    /// Fused math in vertex stages.
    Vertex,
    /// Fused math everywhere.
    All,
}

#[derive(Clone, Copy, Debug, clap::Args)]
#[command(next_help_heading = "Translation (part of the cache fingerprint: changing one rebuilds everything)")]
pub struct Translation {
    /// Where shader math may be fused (multiply-add contraction)
    #[arg(long, value_enum, default_value_t = Contract::Vertex)]
    pub contract_fast: Contract,
    /// Keep spirv-cross's unoptimized precise-math helpers
    #[arg(long)]
    pub keep_optnone: bool,
    /// Clamp sqrt / rsqrt / pow arguments when fusing fragment math
    #[arg(long)]
    pub fuse_guards: bool,
    /// No [[invariant]] positions
    #[arg(long)]
    pub no_invariant: bool,
    /// Constant buffer reads limited to 64 KiB instead of the bound size
    #[arg(long)]
    pub no_cbuf_limits: bool,
    /// Discarding fragment shaders do not write their depth
    #[arg(long)]
    pub no_discard_depth: bool,
    /// Early fragment tests where the control section asks for them
    #[arg(long)]
    pub early_tests: bool,
    /// No compute versions of stages for tiny passes
    #[arg(long)]
    pub no_tiny_kernels: bool,
    /// No batched variants
    #[arg(long)]
    pub no_batch_kernels: bool,
    /// Log why the kernel generators decline stages (does not change the output)
    #[arg(long)]
    pub debug_kernels: bool,
}

impl Default for Translation {
    fn default() -> Self {
        Translation {
            contract_fast: Contract::Vertex,
            keep_optnone: false,
            fuse_guards: false,
            no_invariant: false,
            no_cbuf_limits: false,
            no_discard_depth: false,
            early_tests: false,
            no_tiny_kernels: false,
            no_batch_kernels: false,
            debug_kernels: false,
        }
    }
}

static CURRENT: std::sync::RwLock<Option<Translation>> = std::sync::RwLock::new(None);

pub fn install(t: Translation) {
    *CURRENT.write().unwrap() = Some(t);
    nxport::metal::msl_specialize::set_options(nxport::metal::msl_specialize::TranslationOptions {
        contract_fast: match t.contract_fast {
            Contract::None => nxport::metal::msl_specialize::Contract::None,
            Contract::Vertex => nxport::metal::msl_specialize::Contract::Vertex,
            Contract::All => nxport::metal::msl_specialize::Contract::All,
        },
        keep_optnone: t.keep_optnone,
        fuse_guards: t.fuse_guards,
        no_invariant: t.no_invariant,
        no_cbuf_limits: t.no_cbuf_limits,
        no_discard_depth: t.no_discard_depth,
        early_tests: t.early_tests,
    });
}

/// The options of this command (the defaults if none were installed).
pub fn get() -> Translation {
    CURRENT.read().unwrap().unwrap_or_default()
}

/// The options that change the generated shaders, as text for the cache fingerprint.
pub fn fingerprint() -> String {
    let t = get();
    format!(
        "contract={:?} optnone={} guards={} invariant={} cbuf_limits={} discard_depth={} early_tests={} tiny={} batch={}",
        t.contract_fast, t.keep_optnone, t.fuse_guards, !t.no_invariant, !t.no_cbuf_limits, !t.no_discard_depth,
        t.early_tests, !t.no_tiny_kernels, !t.no_batch_kernels
    )
}
