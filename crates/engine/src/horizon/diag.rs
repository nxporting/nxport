// nn::diag as the game calls it: logging and asserts.

use super::hle::*;

pub(crate) unsafe extern "C" fn is_debugger_attached() -> bool {
    false
}

pub(crate) unsafe extern "C" fn detail_abort(res: *const u32) -> ! {
    sanitize_darwin_tpidrro();
    let code = if res.is_null() { 0 } else { *res };
    let module = code & 0x1ff;
    let desc = (code >> 9) & 0x1fff;
    eprintln!("[!] ========================================================");
    eprintln!("[!] CRITICAL ABORT: nn::diag::detail::Abort called!");
    eprintln!(
        "[!] Result code: 0x{:08X} (Module: {}, Description: {})",
        code, module, desc
    );
    let mut fp = crate::guest_reg!(x29);
    eprintln!("[!] Call trace for Abort:");
    for frame_idx in 0..15 {
        if fp < 0x10000 || (fp & 7) != 0 {
            break;
        }
        let next_fp = *(fp as *const usize);
        let lr = *((fp + 8) as *const usize);
        let (lib, sym) = crate::platform::os::address_owner(lr);
        let (sname, dname) = (sym.unwrap_or_else(|| "?".into()), lib.unwrap_or_else(|| "?".into()));
        eprintln!(
            "    Frame #{:02}: FP={:#x} LR={:#x} <{} in {}>",
            frame_idx, fp, lr, sname, dname
        );
        if next_fp <= fp {
            break;
        }
        fp = next_fp;
    }
    eprintln!("[!] ========================================================");
    eprintln!("[!] Abort called — terminating process with exit code 1.");
    std::process::exit(1);
}

// ── nn::diag ─────────────────────────────────────────────────────────────────────────────────
pub(crate) unsafe extern "C" fn get_symbol_name(buf: *mut u8, size: usize, _addr: usize) -> usize {
    if !buf.is_null() && size > 0 {
        *buf = 0;
    }
    0
}

pub(crate) unsafe extern "C" fn get_backtrace(_out: *mut usize, _count: i32) -> i32 {
    0
}

/// nn::diag::detail::PutImpl(const LogMetaData&, const char* msg, size_t len): the game's log sink.
pub(crate) unsafe extern "C" fn put_impl(_meta: *const u8, msg: *const u8, len: usize) {
    if !msg.is_null() && len > 0 && len < 0x10000 {
        let s = String::from_utf8_lossy(std::slice::from_raw_parts(msg, len));
        println!("[nn::diag] {}", s.trim_end());
    }
}

pub(crate) unsafe extern "C" fn abort_impl_noargs() {
    // The HLE trampoline leaves the guest's return address in x17; x29 still holds the guest frame chain.
    let (lr, fp) = (crate::guest_reg!(x17), crate::guest_reg!(x29));
    let base = crate::guest::loader::main_base();
    let rel = |a: usize| {
        if a >= base && a < base + (256 << 20) {
            format!("main+{:#x}", a - base)
        } else {
            format!("{:#x}", a)
        }
    };
    let mut chain = vec![rel(lr)];
    let mut f = fp;
    for _ in 0..12 {
        if f < 0x10000 || f & 7 != 0 {
            break;
        }
        let ret = *((f + 8) as *const usize);
        chain.push(rel(ret));
        f = *(f as *const usize);
    }
    eprintln!(
        "[!] CRITICAL ABORT: nn::diag::detail::AbortImpl() called from {}",
        chain.join(" <- ")
    );
    std::process::abort();
}
