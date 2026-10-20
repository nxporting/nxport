// SPIRV-Cross's C API, built from source for the target by spirv-cross-sys.
use spirv_cross_sys as _;

use libc::c_char;
use std::ffi::{CStr, CString};

type SpvcContext = *mut std::ffi::c_void;
type SpvcParsedIr = *mut std::ffi::c_void;
type SpvcCompiler = *mut std::ffi::c_void;
type SpvcCompilerOptions = *mut std::ffi::c_void;

const SPVC_SUCCESS: i32 = 0;
const SPVC_BACKEND_MSL: i32 = 3;
const SPVC_CAPTURE_MODE_TAKE_OWNERSHIP: i32 = 1;

const SPVC_COMPILER_OPTION_MSL_VERSION: u32 = 17 | 0x8000000;
const SPVC_COMPILER_OPTION_MSL_ENABLE_DECORATION_BINDING: u32 = 49 | 0x8000000;

const SPV_EXECUTION_MODEL_VERTEX: i32 = 0;
const SPV_EXECUTION_MODEL_FRAGMENT: i32 = 4;
const SPV_EXECUTION_MODEL_GLCOMPUTE: i32 = 5;

extern "C" {
    fn spvc_context_create(context: *mut SpvcContext) -> i32;
    fn spvc_context_destroy(context: SpvcContext);
    fn spvc_context_parse_spirv(
        context: SpvcContext,
        spirv: *const u32,
        word_count: usize,
        parsed_ir: *mut SpvcParsedIr,
    ) -> i32;
    fn spvc_context_create_compiler(
        context: SpvcContext,
        backend: i32,
        parsed_ir: SpvcParsedIr,
        mode: i32,
        compiler: *mut SpvcCompiler,
    ) -> i32;
    fn spvc_compiler_create_compiler_options(
        compiler: SpvcCompiler,
        options: *mut SpvcCompilerOptions,
    ) -> i32;
    fn spvc_compiler_options_set_uint(options: SpvcCompilerOptions, option: u32, value: u32)
        -> i32;
    fn spvc_compiler_options_set_bool(options: SpvcCompilerOptions, option: u32, value: u8) -> i32;
    fn spvc_compiler_install_compiler_options(
        compiler: SpvcCompiler,
        options: SpvcCompilerOptions,
    ) -> i32;
    fn spvc_compiler_rename_entry_point(
        compiler: SpvcCompiler,
        old_name: *const c_char,
        new_name: *const c_char,
        model: i32,
    ) -> i32;
    fn spvc_compiler_compile(compiler: SpvcCompiler, source: *mut *const c_char) -> i32;
    fn spvc_context_get_last_error_string(context: SpvcContext) -> *const c_char;
}

/// Converts a raw SPIR-V byte slice into Metal Shading Language (MSL) directly in-process via linked libspirv-cross.
pub fn compile_spirv_to_msl(
    spv_bytes: &[u8],
    entry: &str,
    stage: sass2spv::ShaderStage,
) -> Result<String, String> {
    if spv_bytes.len() < 4 || spv_bytes.len() % 4 != 0 {
        return Err("Invalid SPIR-V byte length (must be multiple of 4)".into());
    }
    let word_count = spv_bytes.len() / 4;
    let words = spv_bytes.as_ptr() as *const u32;

    unsafe {
        let mut ctx: SpvcContext = std::ptr::null_mut();
        if spvc_context_create(&mut ctx) != SPVC_SUCCESS {
            return Err("spvc_context_create failed".into());
        }

        struct ContextGuard(SpvcContext);
        impl Drop for ContextGuard {
            fn drop(&mut self) {
                unsafe {
                    spvc_context_destroy(self.0);
                }
            }
        }
        let _guard = ContextGuard(ctx);

        let err_str = || {
            let p = spvc_context_get_last_error_string(ctx);
            if p.is_null() {
                "unknown error".to_string()
            } else {
                CStr::from_ptr(p).to_string_lossy().into_owned()
            }
        };

        let mut parsed_ir: SpvcParsedIr = std::ptr::null_mut();
        if spvc_context_parse_spirv(ctx, words, word_count, &mut parsed_ir) != SPVC_SUCCESS {
            return Err(format!("spvc_context_parse_spirv: {}", err_str()));
        }

        let mut compiler: SpvcCompiler = std::ptr::null_mut();
        if spvc_context_create_compiler(
            ctx,
            SPVC_BACKEND_MSL,
            parsed_ir,
            SPVC_CAPTURE_MODE_TAKE_OWNERSHIP,
            &mut compiler,
        ) != SPVC_SUCCESS
        {
            return Err(format!("spvc_context_create_compiler: {}", err_str()));
        }

        let mut opts: SpvcCompilerOptions = std::ptr::null_mut();
        if spvc_compiler_create_compiler_options(compiler, &mut opts) != SPVC_SUCCESS {
            return Err(format!(
                "spvc_compiler_create_compiler_options: {}",
                err_str()
            ));
        }

        spvc_compiler_options_set_uint(opts, SPVC_COMPILER_OPTION_MSL_VERSION, 30000);
        spvc_compiler_options_set_bool(opts, SPVC_COMPILER_OPTION_MSL_ENABLE_DECORATION_BINDING, 1);
        spvc_compiler_install_compiler_options(compiler, opts);

        let model = match stage {
            sass2spv::ShaderStage::Vertex => SPV_EXECUTION_MODEL_VERTEX,
            sass2spv::ShaderStage::Fragment => SPV_EXECUTION_MODEL_FRAGMENT,
            sass2spv::ShaderStage::Compute => SPV_EXECUTION_MODEL_GLCOMPUTE,
            _ => SPV_EXECUTION_MODEL_VERTEX,
        };

        let old_name = CString::new("main").unwrap();
        let new_name = CString::new(entry).map_err(|e| e.to_string())?;
        spvc_compiler_rename_entry_point(compiler, old_name.as_ptr(), new_name.as_ptr(), model);

        let mut source_ptr: *const c_char = std::ptr::null();
        if spvc_compiler_compile(compiler, &mut source_ptr) != SPVC_SUCCESS || source_ptr.is_null()
        {
            return Err(format!("spvc_compiler_compile: {}", err_str()));
        }

        let msl = CStr::from_ptr(source_ptr).to_string_lossy().into_owned();
        Ok(msl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compile_spirv_to_msl_in_process() {
        let path = std::path::Path::new("../../shader_dump_missing/010c4f884a07a980_s1.code");
        if !path.exists() {
            return;
        }
        let sass_bytes = std::fs::read(path).expect("read shader code");
        let (spv_bytes, _) = sass2spv::translate_stage(
            &sass_bytes,
            sass2spv::ShaderStage::Fragment,
            sass2spv::TargetApi::Metal,
        )
        .expect("translate_stage");

        let msl = compile_spirv_to_msl(
            &spv_bytes,
            "s_test_in_process",
            sass2spv::ShaderStage::Fragment,
        )
        .expect("compile_spirv_to_msl");

        println!("Generated MSL:\n{}", &msl[..500.min(msl.len())]);
        assert!(msl.contains("s_test_in_process"));
        assert!(msl.contains("#include <metal_stdlib>"));
    }
}
