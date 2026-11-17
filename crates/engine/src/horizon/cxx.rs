// The C and C++ runtime pieces the engine provides: static-initialisation guards, atexit, pure / deleted virtual
// calls, RTTI vtables, the musl DSO init, and the string / memory / printf routines it replaces.

use std::os::raw::c_char;
use std::sync::{Arc, Condvar, Mutex as StdMutex, OnceLock};

// 1. C++ ABI Guards
//
// __cxa_guard_acquire/release gate C++11 thread-safe static-local
// initialization (Meyer's singletons, `static Foo& instance() { static Foo
// f; return f; }`, extremely common in sead:: manager classes). The Itanium
// ABI *requires* this to be safe under real concurrent access: if two
// threads race to initialize the same static local, exactly one must run
// the constructor while the other blocks until it's done. The previous
// implementation was a plain unsynchronized read-then-branch with no
// locking at all -- with two genuinely concurrent OS threads (main +
// VibrationThread), both could observe *guard == 0 at once, both would be
// told to proceed, and both would run the constructor: a double-init race
// that can leave a "singleton" partially or doubly constructed. That
// produces exactly the class of symptom this session kept running into --
// an object whose vtable and relocations are all individually correct, yet
// whose live state doesn't match what a single-threaded reading of the code
// would predict.
pub(crate) struct GuardState {
    in_progress: StdMutex<bool>,
    cv: Condvar,
}

pub(crate) static GUARD_TABLE: OnceLock<StdMutex<rustc_hash::FxHashMap<usize, Arc<GuardState>>>> =
    OnceLock::new();

pub(crate) fn guard_table() -> &'static StdMutex<rustc_hash::FxHashMap<usize, Arc<GuardState>>> {
    GUARD_TABLE.get_or_init(|| StdMutex::new(rustc_hash::FxHashMap::default()))
}

pub(crate) unsafe fn guard_state_for(guard: *mut u8) -> Arc<GuardState> {
    let key = guard as usize;
    let mut table = guard_table().lock().unwrap();
    table
        .entry(key)
        .or_insert_with(|| {
            Arc::new(GuardState {
                in_progress: StdMutex::new(false),
                cv: Condvar::new(),
            })
        })
        .clone()
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_cxa_guard_acquire(guard: *mut u8) -> i32 {
    if guard.is_null() {
        return 0;
    }
    if *guard != 0 {
        return 0; // fast path: already fully initialized
    }
    let state = guard_state_for(guard);
    let mut in_progress = state.in_progress.lock().unwrap();
    loop {
        if *guard != 0 {
            return 0; // finished by whoever held in_progress while we waited for the lock
        }
        if !*in_progress {
            *in_progress = true;
            return 1; // this thread does the initialization
        }
        // another thread is currently constructing this static local --
        // block until it calls release(), then re-check rather than racing.
        in_progress = state.cv.wait(in_progress).unwrap();
    }
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_cxa_guard_release(guard: *mut u8) {
    if guard.is_null() {
        return;
    }
    *guard = 1;
    let state = guard_state_for(guard);
    {
        let mut in_progress = state.in_progress.lock().unwrap();
        *in_progress = false;
    }
    state.cv.notify_all();
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_cxa_atexit(_func: *const (), _arg: *const (), _dso: *const ()) -> i32 {
    0
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_nnmusl_init_dso(
    base: usize,
    _a1: usize,
    tdata_start: usize,
    tdata_end: usize,
    align_sym: usize,
    _a5: usize,
    tbss_start: usize,
    tbss_end: usize,
) -> i32 {
    // The module's DT_INIT passes its TLS segment: x2..x3 the initialized part (.tdata), x4 its alignment (an absolute
    // linker symbol that arrives relocated: module base + alignment; x0 is the base), x6..x7 the
    // zero-filled part (.tbss) (TotK: no .tdata, 8 bytes of .tbss; Super Mario Bros. Wonder: 16 bytes of .tdata, an
    // owner sentinel of -1 its allocator checks before using a per-thread arena). Every guest thread's block starts
    // with this template (guest/exec/tls.rs), as nnmusl gives each thread a copy.
    let align = align_sym.wrapping_sub(base);
    let align = if align.is_power_of_two() && align <= 0x1000 {
        align
    } else {
        16
    };
    let n = tdata_end.saturating_sub(tdata_start);
    // The block holds at most 0x1f0 bytes of it (guest/tls.rs reports a larger segment), so no more is read.
    let template = if n > 0 && n <= 0x1f0 && tdata_start >= 0x10000 {
        std::slice::from_raw_parts(tdata_start as *const u8, n).to_vec()
    } else {
        Vec::new()
    };
    println!(
        "[+] HLE: __nnmusl_init_dso: TLS template {} bytes initialized + {} zeroed, alignment {}",
        template.len(),
        tbss_end.saturating_sub(tbss_start),
        align
    );
    crate::guest::tls::set_module_tls_template(template, align, n + tbss_end.saturating_sub(tbss_start));
    1
}

// 3. Libc / standard math pass-throughs
#[no_mangle]
pub(crate) unsafe extern "C" fn hle_memset(dest: *mut u8, c: i32, n: usize) -> *mut u8 {
    #[cfg(nx_native)]
    let _copy = crate::gpu::metal::rt_guard::guest_copy_scope();
    if (dest as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memset(dest as *mut libc::c_void, c, n) as *mut u8
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    #[cfg(nx_native)]
    let _copy = crate::gpu::metal::rt_guard::guest_copy_scope();
    if (dest as usize) < 0x10000 || (src as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memcpy(dest as *mut libc::c_void, src as *const libc::c_void, n) as *mut u8
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8 {
    #[cfg(nx_native)]
    let _copy = crate::gpu::metal::rt_guard::guest_copy_scope();
    if (dest as usize) < 0x10000 || (src as usize) < 0x10000 || n == 0 {
        return dest;
    }
    libc::memmove(dest as *mut libc::c_void, src as *const libc::c_void, n) as *mut u8
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_strlen(s: *const c_char) -> usize {
    if (s as usize) < 0x10000 {
        return 0;
    }
    libc::strlen(s)
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_strcmp(s1: *const c_char, s2: *const c_char) -> i32 {
    libc::strcmp(s1, s2)
}

/// printf, formatted here: the guest passes variadic arguments in registers (AAPCS64), where neither the host's C
/// library on Apple (variadics on the stack) nor a call through an adapter (hle_abi) would find them. Integer, string
/// and pointer conversions take x1..x7 in turn; floating-point ones (passed in v registers) print as "?".
#[no_mangle]
pub(crate) unsafe extern "C" fn hle_printf(fmt: *const c_char, a1: u64, a2: u64, a3: u64, a4: u64, a5: u64, a6: u64, a7: u64) -> i32 {
    if fmt.is_null() {
        return 0;
    }
    let out = format_printf(std::ffi::CStr::from_ptr(fmt).to_bytes(), &[a1, a2, a3, a4, a5, a6, a7]);
    print!("{}", String::from_utf8_lossy(&out));
    out.len() as i32
}

pub(crate) fn format_printf(fmt: &[u8], args: &[u64]) -> Vec<u8> {
    let mut args = args.iter().copied();
    let mut out = Vec::new();
    let mut i = 0;
    while i < fmt.len() {
        if fmt[i] != b'%' {
            out.push(fmt[i]);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        // Flags, width, precision and length are skipped (a '*' takes an argument).
        let mut long = false;
        while i < fmt.len() && b"-+ #0123456789.*hlLqjzt".contains(&fmt[i]) {
            match fmt[i] {
                b'*' => drop(args.next()),
                b'l' | b'L' | b'q' | b'j' | b'z' | b't' => long = true,
                _ => {}
            }
            i += 1;
        }
        let Some(&c) = fmt.get(i) else { break };
        i += 1;
        let v = |a: Option<u64>| a.unwrap_or(0);
        let s = match c {
            b'%' => "%".to_string(),
            b'd' | b'i' => if long { (v(args.next()) as i64).to_string() } else { (v(args.next()) as i32).to_string() },
            b'u' => if long { v(args.next()).to_string() } else { (v(args.next()) as u32).to_string() },
            b'x' => if long { format!("{:x}", v(args.next())) } else { format!("{:x}", v(args.next()) as u32) },
            b'X' => if long { format!("{:X}", v(args.next())) } else { format!("{:X}", v(args.next()) as u32) },
            b'o' => if long { format!("{:o}", v(args.next())) } else { format!("{:o}", v(args.next()) as u32) },
            b'p' => format!("{:#x}", v(args.next())),
            b'c' => char::from(v(args.next()) as u8).to_string(),
            b's' => match v(args.next()) {
                0 => "(null)".to_string(),
                p => String::from_utf8_lossy(unsafe { std::ffi::CStr::from_ptr(p as *const c_char) }.to_bytes()).into_owned(),
            },
            b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => "?".to_string(),
            _ => String::from_utf8_lossy(&fmt[start..i]).into_owned(),
        };
        out.extend_from_slice(s.as_bytes());
    }
    out
}

// 12. Symbol-based (R_AARCH64_ABS64 / R_AARCH64_GLOB_DAT) relocation targets.
//
// The loader previously only applied R_AARCH64_RELATIVE relocations. The other
// 1,444 entries in the binary's .rela.dyn are symbol-based, resolving against
// *external* symbols the linker expected some other module to provide -- on a
// real Switch, the C++ runtime (libc++abi-equivalent, linked in via `sdk`).
// Two symbols alone account for 1,344 of those (95%): `__cxa_pure_virtual` and
// `__cxa_deleted_virtual`, the Itanium C++ ABI's standard vtable-slot fillers
// for any pure-virtual or deleted method a derived class doesn't override --
// completely routine in any real C++ binary. Leaving them unresolved meant
// those vtable slots sat at raw zero instead of a safe handler, so calling an
// unoverridden method (an expected, frequent occurrence, not a bug in the
// guest code) crashed the whole engine on a null function pointer.
//
// These are called with whatever "this"/args the original virtual call used;
// since we don't know the real signature at the call site, they take no
// declared params and just report and return, matching the rest of this
// file's "log the unimplemented case, then let execution continue" approach.
#[no_mangle]
pub(crate) unsafe extern "C" fn hle_cxa_pure_virtual() {
    println!("[!] __cxa_pure_virtual called (unoverridden pure virtual method)");
}

#[no_mangle]
pub(crate) unsafe extern "C" fn hle_cxa_deleted_virtual() {
    println!("[!] __cxa_deleted_virtual called (deleted virtual method)");
}

// Minimal RTTI vtable stand-ins for __cxxabiv1::__class_type_info /
// __si_class_type_info. A real implementation of typeid/dynamic_cast needs
// these to behave correctly, but the common failure mode we're guarding
// against is simply "something calls through this vtable" -- so this is a
// small block of safe no-op slots (bigger than either class actually needs)
// rather than a faithful RTTI implementation.
#[no_mangle]
pub(crate) unsafe extern "C" fn hle_rtti_stub_slot() -> i64 {
    0
}

pub(crate) const RTTI_VTABLE_LEN: usize = 12;

pub(crate) static RTTI_CLASS_TYPE_INFO_VTABLE: [unsafe extern "C" fn() -> i64; RTTI_VTABLE_LEN] =
    [hle_rtti_stub_slot; RTTI_VTABLE_LEN];

pub(crate) static RTTI_SI_CLASS_TYPE_INFO_VTABLE: [unsafe extern "C" fn() -> i64; RTTI_VTABLE_LEN] =
    [hle_rtti_stub_slot; RTTI_VTABLE_LEN];

pub(crate) fn rtti_class_type_info_vtable_addr() -> usize {
    RTTI_CLASS_TYPE_INFO_VTABLE.as_ptr() as usize
}

pub(crate) fn rtti_si_class_type_info_vtable_addr() -> usize {
    RTTI_SI_CLASS_TYPE_INFO_VTABLE.as_ptr() as usize
}

pub(crate) static STD_NOTHROW: u8 = 0;

#[cfg(test)]
mod printf_tests {
    #[test]
    fn formats_integers_strings_and_unknowns() {
        let s = b"str\0";
        let out = super::format_printf(b"%d %5u %lx %s %c %% %f %y", &[(-3i32) as u32 as u64, 7, 0xabc, s.as_ptr() as u64, b'z' as u64]);
        assert_eq!(String::from_utf8(out).unwrap(), "-3 7 abc str z % ? %y");
    }
}
