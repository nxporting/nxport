// The SDK functions the engine replaces, by the mangled names the game's binaries contain, in layers per SDK version.
// A layer holds what its version adds or changes; a game whose SDK is version V looks a name up in the newest layer not
// newer than V, then in each older layer, then in `base` (everything whose first version is unknown: older than every
// version looked at). A version older than every layer starts at the oldest. The engine's implementations are named for
// what they do; a layer maps an SDK version's names and signatures onto them (`adapt!` for a different signature).
//
// Every loader resolves through here: the game's imports, the sdk module's references to its own and outside symbols,
// the sdk module's own definitions (interposed, so calls inside the SDK reach the same implementation) and mods'
// imports.

use std::collections::HashMap;
use std::sync::OnceLock;

/// A function pointer in a static table.
#[derive(Clone, Copy)]
pub struct Fp(pub *const ());
// SAFETY: the pointers are function addresses, never written.
unsafe impl Sync for Fp {}
unsafe impl Send for Fp {}

pub enum Bind {
    /// An extern "C" function, called through an HLE trampoline (or its adapter: crate::hle_abi).
    Func(crate::hle_abi::Hle),
    /// A host function bound as it is, with no adapter (a variadic one: its arguments cannot be typed); native
    /// trampolines warn about it, and a lifted runtime cannot call it.
    Raw(Fp),
    /// The address of a data symbol.
    Data(fn() -> usize),
}

/// A binding: a mangled name (a name ending in `*` matches every name with that prefix, after exact names).
pub struct Entry(pub &'static str, pub Bind);

/// What a name resolves to.
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Func(usize),
    Data(usize),
}

/// `func!(path(_, _))`: function `path` (two parameters), as `crate::hle!` takes it.
macro_rules! func {
    ($($f:ident)::+ ($($a:tt),*)) => {
        $crate::horizon::sdk::Bind::Func($crate::hle_abi::Hle::of_const(
            $($f)::+ as unsafe extern "C" fn($($a),*) -> _,
            $($f)::+ as *const (),
        ))
    };
}

/// An extern "C" adapter defined in place: an SDK version's signature onto the engine's implementation.
#[allow(unused_macros)]
macro_rules! adapt {
    (fn($($a:ident: $t:ty),*) $(-> $r:ty)? $b:block) => {{
        unsafe extern "C" fn adapter($($a: $t),*) $(-> $r)? $b
        $crate::horizon::sdk::Bind::Func($crate::hle_abi::Hle::of_const(
            adapter as unsafe extern "C" fn($($t),*) $(-> $r)?,
            adapter as *const (),
        ))
    }};
}

// The host C library's errno and stdout, which the game's `__errno_location` and `stdout` imports name (its printf /
// fflush are the host's too).
extern "C" {
    fn clock() -> libc::clock_t;
    #[cfg_attr(target_vendor = "apple", link_name = "__error")]
    #[cfg_attr(windows, link_name = "_errno")]
    fn errno_location() -> *mut libc::c_int;
    #[cfg(target_vendor = "apple")]
    static mut __stdoutp: *mut libc::FILE;
    #[cfg(windows)]
    fn __acrt_iob_func(index: u32) -> *mut libc::FILE;
}

/// The address of a variable holding the host's stdout.
fn stdout_variable() -> usize {
    #[cfg(target_vendor = "apple")]
    {
        std::ptr::addr_of_mut!(__stdoutp) as usize
    }
    // The CRT has no stdout variable, only a function: keep its result in one of ours.
    #[cfg(windows)]
    {
        static STDOUT: OnceLock<usize> = OnceLock::new();
        STDOUT.get_or_init(|| unsafe { __acrt_iob_func(1) } as usize) as *const usize as usize
    }
}

mod base;
mod v15_3;
mod v4_4;
mod v7_3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SdkVersion(pub u32, pub u32, pub u32);

impl std::fmt::Display for SdkVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

pub struct Layer {
    pub name: &'static str,
    /// The first SDK version this layer describes.
    pub from: SdkVersion,
    pub tables: &'static [&'static [Entry]],
    /// NVN_API_MAJOR / MINOR_VERSION of this SDK's NVN headers (games require that major and at least that minor).
    pub nvn_api: Option<(i32, i32)>,
    /// Bytes by which the thread object's fields after its fiber pointers (TLS values, name, lock, handle) lie further
    /// than in the newest layout, and the object is longer.
    pub thread_tail_shift: Option<usize>,
}

/// Version layers, oldest first.
static LAYERS: &[&Layer] = &[&v4_4::LAYER, &v7_3::LAYER, &v15_3::LAYER];

static BASE: &Layer = &base::LAYER;

static VERSION: OnceLock<Option<SdkVersion>> = OnceLock::new();

/// Records the game's SDK version (the sdk module's nnSdk middleware string); before any lookup.
pub fn set_version(v: Option<SdkVersion>) {
    let _ = VERSION.set(v);
}

pub fn version() -> Option<SdkVersion> {
    VERSION.get().copied().flatten()
}

struct Chain {
    layers: Vec<&'static Layer>,
    /// Per layer: exact names, then prefixes.
    maps: Vec<HashMap<&'static str, &'static Bind>>,
    prefixes: Vec<Vec<(&'static str, &'static Bind)>>,
}

fn chain() -> &'static Chain {
    static C: OnceLock<Chain> = OnceLock::new();
    C.get_or_init(|| {
        let v = version();
        let start = match v {
            Some(v) => LAYERS.iter().rposition(|l| l.from <= v).unwrap_or(0),
            None => LAYERS.len() - 1,
        };
        let mut layers: Vec<&'static Layer> = LAYERS[..=start].iter().rev().copied().collect();
        layers.push(BASE);
        let names: Vec<&str> = layers.iter().map(|l| l.name).collect();
        match v {
            Some(v) if LAYERS[start].from.0 == v.0 && LAYERS[start].from.1 == v.1 => {
                println!("[+] SDK {v}: {}", names.join(" > "))
            }
            Some(v) => println!(
                "[!] SDK {v}: no layer for this version; closest: {} (a function this version changed may behave as in an \
                 older one)",
                names.join(" > ")
            ),
            None => println!("[!] SDK version unknown: {}", names.join(" > ")),
        }
        let entries = |l: &&'static Layer| l.tables.iter().flat_map(|t| t.iter());
        let maps = layers
            .iter()
            .map(|l| entries(l).filter(|e| !e.0.ends_with('*')).map(|e| (e.0, &e.1)).collect())
            .collect();
        let prefixes = layers
            .iter()
            .map(|l| entries(l).filter_map(|e| Some((e.0.strip_suffix('*')?, &e.1))).collect())
            .collect();
        Chain { layers, maps, prefixes }
    })
}

fn target(b: &Bind) -> Target {
    match b {
        Bind::Func(h) => Target::Func(crate::hle_abi::record(*h) as usize),
        Bind::Raw(f) => Target::Func(f.0 as usize),
        Bind::Data(d) => Target::Data(d()),
    }
}

/// What `name` resolves to for this game's SDK version.
pub fn lookup(name: &str) -> Option<Target> {
    let c = chain();
    for (map, prefixes) in c.maps.iter().zip(&c.prefixes) {
        if let Some(b) = map.get(name) {
            return Some(target(b));
        }
        if let Some((_, b)) = prefixes.iter().find(|(p, _)| name.starts_with(p)) {
            return Some(target(b));
        }
    }
    // Libraries kept as tables of their own (the movie library ships in its own module).
    crate::horizon::movie::lookup(name)
        .or_else(|| crate::horizon::audio::out::lookup(name))
        .map(Target::Func)
}

/// Every name the layers bind.
pub fn names() -> impl Iterator<Item = &'static str> {
    let mut v: Vec<&'static str> = chain().maps.iter().flat_map(|m| m.keys().copied()).collect();
    v.sort_unstable();
    v.dedup();
    v.into_iter()
}

/// thread_tail_shift of this game's SDK (the newest layer that knows it; 0 for an unknown version).
pub fn thread_tail_shift() -> usize {
    static S: OnceLock<usize> = OnceLock::new();
    *S.get_or_init(|| chain().layers.iter().find_map(|l| l.thread_tail_shift).unwrap_or(0))
}

/// The NVN API version this game's SDK reports (the newest layer that knows it).
pub fn nvn_api_version() -> (i32, i32) {
    chain().layers.iter().find_map(|l| l.nvn_api).unwrap_or((55, 15))
}
