// Load-time rewrite of guest load-exclusives (ldxr / ldxrb / ldxrh / ldxp) into their acquire forms (ldaxr ...).
//
// Game spinlocks take the lock with a plain ldxr / stxr pair and read the protected data right after, without an acquire
// barrier. The architecture lets those reads be satisfied before the lock is held; the console's cores do not reorder
// them in practice, Apple's do: Breath of the Wild's job queue then handed one chunk of jobs to two threads (both read
// the old position under the "lock"). A standalone test on an M4 loses updates with that lock and none with ldaxr.
// Acquire is strictly stronger, so code that was correct stays correct.

/// Load/store exclusive class with o2 = 0, L = 1, o0 = 0: ldxr, ldxrb, ldxrh, ldxp. Setting o0 (bit 15) makes the
/// acquire form.
const MASK: u32 = 0x3fc0_8000;
const LOAD_EXCLUSIVE: u32 = 0x0840_0000;
const ACQUIRE: u32 = 1 << 15;

/// Rewrites every load-exclusive in the given text ranges (start, len, already executable). Returns the count.
pub unsafe fn rewrite(ranges: &[(usize, usize, bool)]) -> usize {
    extern "C" {
        fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
    }
    let mut n = 0;
    for &(start, len, executable) in ranges {
        let mut open_page = usize::MAX;
        for off in (0..len & !3).step_by(4) {
            let p = (start + off) as *mut u32;
            let w = *p;
            if w & MASK != LOAD_EXCLUSIVE {
                continue;
            }
            let page = (p as usize) & !0x3fff;
            if executable && page != open_page {
                if open_page != usize::MAX {
                    crate::cpu::aot::protect_rx(open_page, 0x4000);
                }
                libc::mprotect(page as *mut libc::c_void, 0x4000, libc::PROT_READ | libc::PROT_WRITE);
                open_page = page;
            }
            *p = w | ACQUIRE;
            n += 1;
        }
        if open_page != usize::MAX {
            crate::cpu::aot::protect_rx(open_page, 0x4000);
        }
        sys_icache_invalidate(start as *mut libc::c_void, len);
    }
    n
}
