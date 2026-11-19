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
    #[cfg(target_vendor = "apple")]
    {
        let n = lock_idioms(ranges);
        println!("[+] load-exclusive rewrite: {n} spinlock loops now compare-and-swap");
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
                    crate::guest::exec::aot::protect_rx(open_page, 0x4000);
                }
                let _ = crate::platform::os::protect(page, 0x4000, crate::platform::os::Prot::ReadWrite);
                open_page = page;
            }
            *p = w | ACQUIRE;
            n += 1;
        }
        if open_page != usize::MAX {
            crate::guest::exec::aot::protect_rx(open_page, 0x4000);
        }
        crate::platform::os::flush_icache(start, len);
    }
    n
}

// Spinlock loops built from load-exclusive / store-exclusive pairs become compare-and-swap (ARMv8.1 LSE). On Apple
// cores a store-exclusive keeps failing while other cores load and store the same line: a thread that must take a lock
// two others take and drop in a tight loop waited 15 ms on average and up to 553 ms (standalone test on an M4; with
// casa 0.5 us and 2 us). Breath of the Wild's job queue (sead::FixedSizeJQ::run) reserves its first job for the main
// thread while the Workers retake the lock until it is gone: its frames took 40-260 ms in menus and while running.
// Each loop is replaced by the same number of instructions doing the same thing, only when no branch from elsewhere
// lands inside it.

const NOP: u32 = 0xd503_201f;

fn is_ldxr(w: u32) -> bool {
    w & 0xbfff_7c00 == 0x885f_7c00
}
fn is_stxr(w: u32) -> bool {
    w & 0xbfe0_7c00 == 0x8800_7c00
}
/// Size bit (1: 64-bit) of a load / store exclusive or CAS.
fn sz(w: u32) -> u32 {
    (w >> 30) & 1
}
fn rt(w: u32) -> u32 {
    w & 31
}
fn rn(w: u32) -> u32 {
    (w >> 5) & 31
}
fn rs(w: u32) -> u32 {
    (w >> 16) & 31
}
/// cbz / cbnz: (nonzero, 64-bit, register, byte offset).
fn cb(w: u32) -> Option<(bool, u32, u32, i64)> {
    (w & 0x7e00_0000 == 0x3400_0000).then(|| {
        let off = (((w >> 5) & 0x7ffff) as i64) << 45 >> 43;
        ((w >> 24) & 1 == 1, w >> 31, w & 31, off)
    })
}
fn enc_cb(nonzero: bool, x: u32, r: u32, off: i64) -> Option<u32> {
    (off % 4 == 0 && (-(1 << 20)..(1 << 20)).contains(&off))
        .then(|| x << 31 | 0x3400_0000 | (nonzero as u32) << 24 | (((off >> 2) as u32) & 0x7ffff) << 5 | r)
}
/// b.cond: (condition, byte offset).
fn bcond(w: u32) -> Option<(u32, i64)> {
    (w & 0xff00_0010 == 0x5400_0000).then(|| (w & 15, (((w >> 5) & 0x7ffff) as i64) << 45 >> 43))
}
fn enc_bcond(cond: u32, off: i64) -> Option<u32> {
    (off % 4 == 0 && (-(1 << 20)..(1 << 20)).contains(&off)).then(|| 0x5400_0000 | (((off >> 2) as u32) & 0x7ffff) << 5 | cond)
}
/// cmp Rn, #imm12 (subs zr): (64-bit, register, immediate).
fn cmp_imm(w: u32) -> Option<(u32, u32, u32)> {
    (w & 0x7fc0_001f == 0x7100_001f).then(|| (w >> 31, (w >> 5) & 31, (w >> 10) & 0xfff))
}
fn movz(x: u32, r: u32, imm: u32) -> u32 {
    0x5280_0000 | x << 31 | (imm & 0xffff) << 5 | r
}
/// cas{a}{l} Rs, Rt, [Xn]: compares Rs with memory, stores Rt when equal; Rs receives the old value.
fn cas(x: u32, acquire: bool, release: bool, s: u32, t: u32, n: u32) -> u32 {
    0x88a0_7c00 | x << 30 | (acquire as u32) << 22 | (release as u32) << 15 | s << 16 | n << 5 | t
}
/// Byte offset of a direct branch (b, bl, b.cond, cbz/cbnz, tbz/tbnz).
fn branch_offset(w: u32) -> Option<i64> {
    if w & 0x7c00_0000 == 0x1400_0000 {
        return Some(((w & 0x03ff_ffff) as i64) << 38 >> 36);
    }
    if let Some((_, o)) = bcond(w) {
        return Some(o);
    }
    if let Some((_, _, _, o)) = cb(w) {
        return Some(o);
    }
    (w & 0x7e00_0000 == 0x3600_0000).then(|| (((w >> 5) & 0x3fff) as i64) << 50 >> 48)
}

#[cfg(target_vendor = "apple")]
unsafe fn lock_idioms(ranges: &[(usize, usize, bool)]) -> usize {
    let mut n = 0;
    for &(start, len, executable) in ranges {
        let words = len / 4;
        let code = std::slice::from_raw_parts(start as *const u32, words);
        // How many direct branches land on each instruction.
        let mut targets: rustc_hash::FxHashMap<usize, u32> = Default::default();
        for (i, &w) in code.iter().enumerate() {
            if let Some(o) = branch_offset(w) {
                let t = i as i64 + o / 4;
                if (0..words as i64).contains(&t) {
                    *targets.entry(t as usize).or_insert(0) += 1;
                }
            }
        }
        let landed = |i: usize| targets.get(&i).copied().unwrap_or(0);
        let mut patches: Vec<(usize, Vec<u32>)> = Vec::new();
        let mut i = 0;
        while i + 6 <= words {
            let l = code[i];
            if !is_ldxr(l) {
                i += 1;
                continue;
            }
            let (x, t_reg, base) = (sz(l), rt(l), rn(l));
            if t_reg == 31 {
                i += 1;
                continue;
            }
            // The status register may be the loaded one (BotW's loops reuse it: the loaded value is dead once tested);
            // the replacement leaves it 0, as a successful store-exclusive does.
            let store_ok = |w: u32| is_stxr(w) && sz(w) == x && rn(w) == base && rs(w) != base && rt(w) != t_reg;
            let rel = |w: u32| (w >> 15) & 1 == 1;
            // Acquire, long form: ldxr t; cbz t, S(+12); clrex; b L(=i); S: stxr s, v; cbnz s, T.
            if let (Some((false, cx, cr, 12)), 0xd503_3f5f, true) = (cb(code[i + 1]), code[i + 2], code[i + 3] == (0x1400_0000 | ((-3i32 as u32) & 0x03ff_ffff))) {
                let (st, last) = (code[i + 4], code[i + 5]);
                if cx == x && cr == t_reg && store_ok(st) {
                    if let Some((true, lx, lr, lo)) = cb(last) {
                        if lx == 0 && lr == rs(st) && landed(i + 1) == 0 && landed(i + 2) == 0 && landed(i + 3) == 0 && landed(i + 4) == 1 && landed(i + 5) == 0 {
                            // The retry target, from the new cbnz two instructions earlier.
                            if let Some(retry) = enc_cb(true, x, t_reg, lo + 12) {
                                patches.push((i, vec![movz(x, t_reg, 0), cas(x, true, rel(st), t_reg, rt(st), base), retry, movz(0, rs(st), 0), NOP, NOP]));
                                i += 6;
                                continue;
                            }
                        }
                    }
                }
            }
            // Acquire, short form: ldxr t; cbnz t, T; stxr s, v; cbnz s, U.
            if let (Some((true, cx, cr, to)), st, Some((true, 0, lr, _))) = (cb(code[i + 1]), code[i + 2], cb(code[i + 3])) {
                if cx == x && cr == t_reg && store_ok(st) && lr == rs(st) && landed(i + 1) == 0 && landed(i + 2) == 0 && landed(i + 3) == 0 {
                    if let Some(retry) = enc_cb(true, x, t_reg, to - 4) {
                        patches.push((i, vec![movz(x, t_reg, 0), cas(x, true, rel(st), t_reg, rt(st), base), retry, movz(0, rs(st), 0)]));
                        i += 4;
                        continue;
                    }
                }
            }
            // Release when the value is k: ldxr t; cmp t, #k; b.ne X; stxr s, v; cbnz s, L.
            if let (Some((cx, cr, k)), Some((1, bo)), st, Some((true, 0, lr, _))) = (cmp_imm(code[i + 1]), bcond(code[i + 2]), code[i + 3], cb(code[i + 4])) {
                if cx == x && cr == t_reg && k <= 0xffff && store_ok(st) && lr == rs(st) && (1..=4).all(|d| landed(i + d) == 0) {
                    if let Some(bne) = enc_bcond(1, bo - 4) {
                        patches.push((i, vec![movz(x, t_reg, k), cas(x, true, rel(st), t_reg, rt(st), base), code[i + 1], bne, movz(0, rs(st), 0)]));
                        i += 5;
                        continue;
                    }
                }
            }
            i += 1;
        }
        let mut open_page = usize::MAX;
        for (i, words) in &patches {
            for (k, &w) in words.iter().enumerate() {
                let p = (start + (i + k) * 4) as *mut u32;
                let page = (p as usize) & !0x3fff;
                if executable && page != open_page {
                    if open_page != usize::MAX {
                        crate::guest::exec::aot::protect_rx(open_page, 0x4000);
                    }
                    let _ = crate::platform::os::protect(page, 0x4000, crate::platform::os::Prot::ReadWrite);
                    open_page = page;
                }
                *p = w;
            }
        }
        if open_page != usize::MAX {
            crate::guest::exec::aot::protect_rx(open_page, 0x4000);
        }
        n += patches.len();
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodings() {
        // cas w14, w10, [x9] family, from the architecture's encoding tables.
        assert_eq!(cas(0, false, false, 14, 10, 9), 0x88ae_7d2a);
        assert_eq!(cas(0, true, false, 14, 10, 9), 0x88ee_7d2a);
        assert_eq!(cas(0, true, true, 14, 10, 9), 0x88ee_fd2a);
        assert_eq!(movz(0, 14, 0), 0x5280_000e);
        assert!(is_ldxr(0x885f_fd2e)); // ldaxr w14, [x9]
        assert!(is_stxr(0x880e_7d2a)); // stxr w14, w10, [x9]
        assert_eq!(cb(0x3400_006e), Some((false, 0, 14, 12))); // cbz w14, +12
        assert_eq!(cmp_imm(0x7100_05ff), Some((0, 15, 1))); // cmp w15, #1
    }
}
