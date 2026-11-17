// Shader libraries: the cache's compiled libraries, ones compiled at run time, and the stage denylist.

use super::*;

/// The translated shader libraries (shaders_NNNN.metallib) with an entry-point -> library map.
pub(crate) struct ShaderLibs {
    libs: Vec<Retained<ProtocolObject<dyn MTLLibrary>>>,
    by_entry: HashMap<String, usize>,
    dynamic_libs: Mutex<HashMap<String, Retained<ProtocolObject<dyn MTLLibrary>>>>,
}

impl ShaderLibs {
    /// A kernel the cache builder appended to `base`'s stage (same library), with function constants.
    pub(crate) fn kernel(
        &self,
        base: &str,
        name: &str,
        constants: &MTLFunctionConstantValues,
    ) -> Result<Retained<ProtocolObject<dyn MTLFunction>>, String> {
        if let Some(&i) = self.by_entry.get(base) {
            return super::pipeline_archive::function(&self.libs[i], name, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = self.dynamic_libs.lock().unwrap().get(base) {
            return super::pipeline_archive::function(&lib, name, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = dynamic_libs().lock().unwrap().get(base) {
            return super::pipeline_archive::function(&lib, name, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        Err(format!("{} is not in any shader library", base))
    }

    pub(crate) fn function(
        &self,
        entry: &str,
        constants: &MTLFunctionConstantValues,
    ) -> Result<Retained<ProtocolObject<dyn MTLFunction>>, String> {
        if let Some(&i) = self.by_entry.get(entry) {
            return super::pipeline_archive::function(&self.libs[i], entry, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = self.dynamic_libs.lock().unwrap().get(entry) {
            return super::pipeline_archive::function(&lib, entry, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        if let Some(lib) = dynamic_libs().lock().unwrap().get(entry) {
            return super::pipeline_archive::function(&lib, entry, constants)
                .map_err(|e| e.localizedDescription().to_string());
        }
        Err(format!("{} is not in any shader library", entry))
    }

    pub(crate) fn register_dynamic(
        &self,
        entry: String,
        lib: Retained<ProtocolObject<dyn MTLLibrary>>,
    ) {
        self.dynamic_libs.lock().unwrap().insert(entry, lib);
    }
}

pub(crate) static DYNAMIC_LIBS: OnceLock<Mutex<HashMap<String, Retained<ProtocolObject<dyn MTLLibrary>>>>> =
    OnceLock::new();

pub(crate) fn dynamic_libs() -> &'static Mutex<HashMap<String, Retained<ProtocolObject<dyn MTLLibrary>>>> {
    DYNAMIC_LIBS.get_or_init(Default::default)
}

pub(crate) fn register_dynamic_library(entry: &str, lib: Retained<ProtocolObject<dyn MTLLibrary>>) {
    dynamic_libs().lock().unwrap().insert(entry.to_string(), lib.clone());
    if let Some(gfx) = super::GFX.get() {
        if let Some(shader_lib) = &gfx.shader_lib {
            shader_lib.register_dynamic(entry.to_string(), lib);
        }
    }
}

pub(crate) fn load_shader_lib(device: &Device) -> Option<ShaderLibs> {
    if !enabled() {
        return None;
    }
    let dir = crate::metal::shader_cache::cache_dir();
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.file_name().and_then(|n| n.to_str()).map_or(false, |n| {
                n.starts_with("shaders_") && n.ends_with(".metallib")
            })
        })
        .collect();
    paths.sort();
    let mut libs = Vec::new();
    let mut by_entry = HashMap::default();
    for path in &paths {
        let path = path.canonicalize().unwrap_or_else(|_| path.clone());
        let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str()?));
        match device.newLibraryWithURL_error(&url) {
            Ok(lib) => {
                for name in lib.functionNames().iter() {
                    by_entry.insert(name.to_string(), libs.len());
                }
                libs.push(lib);
            }
            Err(e) => println!(
                "[!] Gfx: cannot load {}: {}",
                path.display(),
                e.localizedDescription()
            ),
        }
    }
    if libs.is_empty() {
        if !crate::settings::get().graphics.dynamic_shaders {
            println!("[!] Gfx: no shader libraries in {} and --dynamic-shaders off: nothing is drawn", dir.display());
            return None;
        }
        println!("[+] Gfx: no shader libraries in {}: shaders are translated as the game loads them", dir.display());
    }
    println!(
        "[+] Gfx: {} translated shader libraries, {} entry points",
        libs.len(),
        by_entry.len()
    );
    let initial_dynamic = dynamic_libs().lock().unwrap().clone();
    Some(ShaderLibs {
        libs,
        by_entry,
        dynamic_libs: Mutex::new(initial_dynamic),
    })
}

/// Shaders that hung the GPU on an earlier run. A hang makes macOS reset the GPU and can freeze the whole machine for
/// many seconds, so after the first one the offender is never run again: the engine writes the entries of the encoder
/// that did not complete to <cache>/hang_denylist.txt and skips every draw that uses them.
pub(crate) fn denylist() -> &'static Mutex<std::collections::HashSet<String>> {
    static D: OnceLock<Mutex<std::collections::HashSet<String>>> = OnceLock::new();
    D.get_or_init(|| {
        let path = crate::metal::shader_cache::cache_dir().join("hang_denylist.txt");
        let set: std::collections::HashSet<String> = std::fs::read_to_string(&path)
            .map(|t| {
                t.lines()
                    .map(|l| l.trim().to_string())
                    .filter(|l| l.starts_with("s_"))
                    .collect()
            })
            .unwrap_or_default();
        if !set.is_empty() {
            println!(
                "[!] Gfx: {} shader(s) are on the GPU-hang denylist ({}) and will not run",
                set.len(),
                path.display()
            );
        }
        Mutex::new(set)
    })
}

/// Entries in the hang denylist (checked per draw: nearly always zero, so no lock).
pub(crate) static DENIED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);

pub(crate) fn is_denied(entry: &str) -> bool {
    let n = DENIED.load(std::sync::atomic::Ordering::Relaxed);
    if n == 0 {
        return false;
    }
    let d = denylist().lock().unwrap();
    DENIED.store(d.len(), std::sync::atomic::Ordering::Relaxed);
    d.contains(entry)
}
