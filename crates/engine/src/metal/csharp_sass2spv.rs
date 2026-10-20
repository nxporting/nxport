// C FFI bindings to the NativeAOT Ryujinx shader translator (sass2spv.dylib).
// Loaded dynamically at runtime via dlopen so that:
// 1. Binaries (including builder tools) do not hard-depend on dyld search paths at launch.
// 2. The guest memory space is already fully reserved and mapped before NativeAOT initializes,
//    preventing NativeAOT from consuming address space needed by guest modules and exlaunch mods.

use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::sync::OnceLock;

type TranslateFn = unsafe extern "C" fn(
    stage_ptr: *const u8,
    stage_len: i32,
    stage_hint: i32, // 0 = auto/sph, 1 = fragment, 2 = vertex, 3 = compute
    cb1_hint: i32,
    compute: *const i32, // compute stages: workgroup size x, y, z and shared memory bytes, else null
    out_spv: *mut *mut u8,
    out_spv_len: *mut i32,
    out_manifest_json: *mut *mut u8,
    out_manifest_len: *mut i32,
    out_size: *mut i32,
    out_stage: *mut i32,
) -> i32;

type FreeFn = unsafe extern "C" fn(ptr: *mut u8);

struct NativeAotFfi {
    translate: TranslateFn,
    free: FreeFn,
}

unsafe impl Send for NativeAotFfi {}
unsafe impl Sync for NativeAotFfi {}

static FFI: OnceLock<Result<NativeAotFfi, String>> = OnceLock::new();

fn get_ffi() -> Result<&'static NativeAotFfi, &'static str> {
    FFI.get_or_init(load_ffi).as_ref().map_err(|s| s.as_str())
}

/// Where the translator library may be, in search order.
fn candidates() -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(curr) = std::env::current_exe() {
        if let Some(dir) = curr.parent() {
            candidates.push(dir.join("sass2spv.dylib"));
            candidates.push(dir.join("libsass2spv.dylib"));
            // iOS app bundles: the executable sits at the top level, next to Frameworks/.
            candidates.push(dir.join("Frameworks/sass2spv.dylib"));
            candidates.push(dir.join("../Frameworks/sass2spv.dylib"));
            candidates.push(dir.join("../Frameworks/libsass2spv.dylib"));
        }
        for anc in curr.ancestors() {
            candidates.push(anc.join("target/native_sass2spv/libsass2spv.dylib"));
            candidates.push(anc.join("target/native_sass2spv/sass2spv.dylib"));
            candidates.push(anc.join("native_sass2spv/libsass2spv.dylib"));
            candidates.push(anc.join("native_sass2spv/sass2spv.dylib"));
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for anc in cwd.ancestors() {
            candidates.push(anc.join("target/native_sass2spv/libsass2spv.dylib"));
            candidates.push(anc.join("target/native_sass2spv/sass2spv.dylib"));
        }
    }

    // 2. Relative to current working directory
    candidates.push(PathBuf::from("target/native_sass2spv/sass2spv.dylib"));
    candidates.push(PathBuf::from("target/native_sass2spv/libsass2spv.dylib"));
    candidates.push(PathBuf::from("sass2spv.dylib"));
    candidates.push(PathBuf::from("libsass2spv.dylib"));
    candidates
}

/// The translator library that would be loaded (without loading it: the runtime must not start before the guest's
/// address space is reserved).
pub fn library_path() -> Option<PathBuf> {
    candidates().into_iter().find(|c| c.is_file())
}

fn load_ffi() -> Result<NativeAotFfi, String> {
    let candidates = candidates();

    let mut handle: *mut c_void = std::ptr::null_mut();
    let mut loaded_path = None;

    for candidate in &candidates {
        if candidate.is_file()
            || candidate.as_os_str() == "sass2spv.dylib"
            || candidate.as_os_str() == "libsass2spv.dylib"
        {
            let path_str = candidate.to_string_lossy();
            let cstr = match CString::new(path_str.as_bytes()) {
                Ok(s) => s,
                Err(_) => continue,
            };
            let h = unsafe { libc::dlopen(cstr.as_ptr(), libc::RTLD_LAZY | libc::RTLD_LOCAL) };
            if !h.is_null() {
                handle = h;
                loaded_path = Some(candidate.display().to_string());
                break;
            }
        }
    }

    if handle.is_null() {
        let err_msg = unsafe {
            let err_ptr = libc::dlerror();
            if !err_ptr.is_null() {
                std::ffi::CStr::from_ptr(err_ptr)
                    .to_string_lossy()
                    .into_owned()
            } else {
                "file not found".to_string()
            }
        };
        return Err(format!("Could not load sass2spv.dylib: {err_msg}"));
    }

    unsafe {
        let sym_translate = CString::new("sass2spv_csharp_translate").unwrap();
        let sym_free = CString::new("sass2spv_csharp_free").unwrap();

        let translate_ptr = libc::dlsym(handle, sym_translate.as_ptr());
        if translate_ptr.is_null() {
            return Err("dlsym failed for sass2spv_csharp_translate".into());
        }

        let free_ptr = libc::dlsym(handle, sym_free.as_ptr());
        if free_ptr.is_null() {
            return Err("dlsym failed for sass2spv_csharp_free".into());
        }

        let translate: TranslateFn = std::mem::transmute(translate_ptr);
        let free: FreeFn = std::mem::transmute(free_ptr);

        if let Some(path) = loaded_path {
            println!("[+] Loaded native C# shader translator from {path}");
        }

        Ok(NativeAotFfi { translate, free })
    }
}

pub struct NativeTranslationResult {
    pub spirv: Vec<u8>,
    pub manifest_json: String,
    pub size: usize,
    pub stage: i32,
}

/// `compute`: a compute stage's workgroup size (x, y, z) and shared memory bytes from its control section.
pub fn translate_stage_native(
    stage_data: &[u8],
    stage_hint: i32,
    cb1_hint: i32,
    compute: Option<[i32; 4]>,
) -> Result<NativeTranslationResult, String> {
    let ffi = get_ffi().map_err(|e| e.to_string())?;

    unsafe {
        let mut out_spv: *mut u8 = std::ptr::null_mut();
        let mut out_spv_len: i32 = 0;
        let mut out_manifest: *mut u8 = std::ptr::null_mut();
        let mut out_manifest_len: i32 = 0;
        let mut out_size: i32 = 0;
        let mut out_stage: i32 = 0;

        let ret = (ffi.translate)(
            stage_data.as_ptr(),
            stage_data.len() as i32,
            stage_hint,
            cb1_hint,
            compute.as_ref().map_or(std::ptr::null(), |c| c.as_ptr()),
            &mut out_spv,
            &mut out_spv_len,
            &mut out_manifest,
            &mut out_manifest_len,
            &mut out_size,
            &mut out_stage,
        );

        if ret != 0 {
            return Err(format!(
                "Native C# sass2spv translation failed with code {ret}"
            ));
        }

        let spirv = if !out_spv.is_null() && out_spv_len > 0 {
            let slice = std::slice::from_raw_parts(out_spv, out_spv_len as usize);
            let vec = slice.to_vec();
            (ffi.free)(out_spv);
            vec
        } else {
            return Err("Empty SPIR-V emitted by native C# sass2spv".to_string());
        };

        let manifest_json = if !out_manifest.is_null() && out_manifest_len > 0 {
            let slice = std::slice::from_raw_parts(out_manifest, out_manifest_len as usize);
            let s = String::from_utf8_lossy(slice).to_string();
            (ffi.free)(out_manifest);
            s
        } else {
            String::new()
        };

        Ok(NativeTranslationResult {
            spirv,
            manifest_json,
            size: out_size as usize,
            stage: out_stage,
        })
    }
}
