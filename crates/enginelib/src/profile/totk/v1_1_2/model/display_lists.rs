// main+0xbccb6c(set, w, rebuild): a set of the game's recorded display lists called in order (+0x7c8, count +0x7c0;
// each list's handle at +0x48), then the set's total (+0x24 of each) at +0x790 and its maximum at +0x798. A rebuild
// first heap-sorts the source lists (+0x7a8, count +0x7a0) by +0x60 and copies the enabled ones (+0x50 bit 0) into
// the set's records (+0x7b8, 0x78 bytes, count +0x7b0) and call array. Direct calls (+0x50 bit 4) and the per-call
// GPU timing (+0x749 bits 5 / 6) are left to the game.

use super::draw::{Port, R};

pub(super) unsafe fn call(p: &mut Port, set: usize, w: usize, rebuild: u64) -> R<()> {
    if rebuild & 1 != 0 {
        rebuild_calls(p, set);
    }
    if (p.m.r8(set + 0x749) >> 5) & 3 != 0 {
        return Err("display list timing");
    }
    let n = p.m.i32(set + 0x7c0) as isize as usize;
    let lists = p.m.r64(set + 0x7c8);
    for k in 0..n {
        if (p.m.r8(p.m.r64(lists + k * 8) + 0x50) >> 4) & 1 != 0 {
            return Err("display list direct call");
        }
    }
    let cmd = p.m.r64(w + 0xb8);
    let mut total = 0u64;
    for k in 0..n {
        let l = p.m.r64(lists + k * 8);
        // A copy: the handle may come from this call's rebuild, which a prediction keeps out of guest memory.
        let handle = [p.m.r64(l + 0x48) as u64];
        p.call_unit(cmd, handle.as_ptr() as usize);
        total += p.m.r32(l + 0x24) as u64;
    }
    p.m.w::<u64>(set + 0x790, total);
    if n != 0 && (p.m.r64(set + 0x798) as u64) < total {
        p.m.w::<u64>(set + 0x798, total);
    }
    Ok(())
}

/// The game's heap sort (1-based sift-down, larger child on ties to the right) of the source lists by +0x60.
unsafe fn sort(p: &mut Port, a: usize, n: i32) {
    let get = |p: &Port, i: i32| p.m.r64(a + (i as isize as usize) * 8);
    let key = |p: &Port, v: usize| p.m.i32(v + 0x60);
    let child = |p: &Port, c: i32, last: i32| {
        if c < last { c | (key(p, get(p, c - 1)) < key(p, get(p, c))) as i32 } else { c }
    };
    for i in (1..=n / 2).rev() {
        let v = get(p, i - 1);
        let mut j = i;
        let mut c = 2 * i;
        while c <= n {
            let cc = child(p, c, n);
            if key(p, get(p, cc - 1)) <= key(p, v) {
                break;
            }
            p.m.w::<u64>(a + (j - 1) as usize * 8, get(p, cc - 1) as u64);
            j = cc;
            c = cc * 2;
        }
        p.m.w::<u64>(a + (j - 1) as usize * 8, v as u64);
    }
    let mut m = n - 1;
    let mut v = get(p, m);
    p.m.w::<u64>(a + m as usize * 8, get(p, 0) as u64);
    let mut k = n;
    if n > 2 {
        loop {
            let mut j = 1;
            let mut c = 2;
            loop {
                let cc = child(p, c, m);
                if key(p, get(p, cc - 1)) <= key(p, v) {
                    break;
                }
                p.m.w::<u64>(a + (j - 1) as usize * 8, get(p, cc - 1) as u64);
                j = cc;
                c = cc << 1;
                if c >= k {
                    break;
                }
            }
            p.m.w::<u64>(a + (j - 1) as usize * 8, v as u64);
            m -= 1;
            v = get(p, m);
            p.m.w::<u64>(a + m as usize * 8, get(p, 0) as u64);
            let more = k > 3;
            k -= 1;
            if !more {
                break;
            }
        }
    }
    p.m.w::<u64>(a, v as u64);
}

unsafe fn rebuild_calls(p: &mut Port, set: usize) {
    p.m.w::<u32>(set + 0x7c0, 0);
    let n = p.m.i32(set + 0x7a0);
    let a = p.m.r64(set + 0x7a8);
    if n > 1 {
        sort(p, a, n);
    }
    for k in 0..p.m.i32(set + 0x7a0).max(0) as usize {
        let src = p.m.r64(a + k * 8);
        if p.m.r8(src + 0x50) & 1 == 0 {
            continue;
        }
        let used = p.m.r32(set + 0x7c0);
        let i = if p.m.r32(set + 0x7b0) <= used { 0 } else { used };
        let d = p.m.r64(set + 0x7b8).wrapping_add((i as i32 as isize as usize).wrapping_mul(0x78));
        p.m.w::<u32>(d + 8, p.m.r32(src + 8));
        let flags = p.m.r8(d + 0x50);
        p.m.w::<u32>(d + 0xc, p.m.r32(src + 0xc));
        p.m.w::<u64>(d + 0x10, p.m.r64(src + 0x10) as u64);
        p.m.w::<u64>(d + 0x18, p.m.r64(src + 0x18) as u64);
        p.m.w::<u32>(d + 0x20, p.m.r32(src + 0x20));
        p.m.w::<u8>(d + 0x50, (flags & 0xec) | (p.m.r8(src + 0x50) & 0x13));
        p.m.w::<u32>(d + 0x24, p.m.r32(src + 0x24));
        for o in [0x48, 0x68, 0x70] {
            p.m.w::<u64>(d + o, p.m.r64(src + o) as u64);
        }
        p.m.w::<u32>(d + 0x60, p.m.r32(src + 0x60));
        p.m.w::<u8>(d + 0x64, p.m.r8(src + 0x64));
        p.m.w::<u64>(d + 0x58, p.m.r64(src + 0x58) as u64);
        p.m.w::<u32>(set + 0x7c0, used.wrapping_add(1));
        if (used as i32) < p.m.i32(set + 0x7c4) {
            p.m.w::<u64>(p.m.r64(set + 0x7c8).wrapping_add((used as i32 as isize as usize).wrapping_mul(8)), d as u64);
        }
    }
}
