// TotK's model renderer (main+0x688c68), ported (draw.rs). --verify-model-draws runs the port in predict mode before
// each call of the game's function and compares the NVN commands the game then records with the prediction; calls
// that reach a helper the port does not implement yet are counted by that helper.

mod draw;
mod mem;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const SITE: usize = 0x688c68;
const SITE_INSTRUCTION: u32 = 0xd10383ff; // sub sp, sp, #0xe0

static CALLS: AtomicU64 = AtomicU64::new(0);
static SAME: AtomicU64 = AtomicU64::new(0);
static DIFFERENT: AtomicU64 = AtomicU64::new(0);
static STOPS: Mutex<Vec<(&'static str, u64)>> = Mutex::new(Vec::new());

thread_local! {
    static EXPECTED: std::cell::RefCell<Option<Vec<String>>> = const { std::cell::RefCell::new(None) };
}

fn note_stop(what: &'static str) {
    let mut s = STOPS.lock().unwrap();
    match s.iter_mut().find(|e| e.0 == what) {
        Some(e) => e.1 += 1,
        None => s.push((what, 1)),
    }
}

#[no_mangle]
pub unsafe extern "C" fn on_model_draw(d: usize, c: usize, shape: u32, flags: u32) -> u32 {
    let n = CALLS.fetch_add(1, Ordering::Relaxed) + 1;
    if n % 200_000 == 0 {
        report();
    }
    let base = crate::guest::loader::main_base();
    if crate::settings::get().totk.diagnostics.native_model_draws {
        static METAL: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let metal = *METAL.get_or_init(|| crate::settings::get().graphics.gpu == crate::settings::GpuBackend::Metal);
        let mut port = draw::Port::new(mem::Mem::applying(), base, metal);
        let r = draw::draw(&mut port, d, c, shape, flags);
        port.flush();
        if let Err(what) = r {
            // Applying never stops (helpers it does not implement call the game's); a stop here is a port bug.
            note_stop(what);
        }
        return 1;
    }
    let mut port = draw::Port::new(mem::Mem::predicting(), base, false);
    let mut result = Ok(());
    let expected = crate::nvn::api::capture_dry(|| result = draw::draw(&mut port, d, c, shape, flags));
    match result {
        Ok(()) => {
            EXPECTED.with(|e| *e.borrow_mut() = Some(expected));
            crate::nvn::api::capture_collect();
        }
        Err(what) => note_stop(what),
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn on_model_draw_end() -> u32 {
    let Some(expected) = EXPECTED.with(|e| e.borrow_mut().take()) else { return 0 };
    let got = crate::nvn::api::capture_collected();
    if expected == got {
        SAME.fetch_add(1, Ordering::Relaxed);
        return 0;
    }
    let k = DIFFERENT.fetch_add(1, Ordering::Relaxed);
    if k < 6 {
        println!("[model-draw] mismatch #{k}:");
        for i in 0..expected.len().max(got.len()) {
            let (e, g) = (expected.get(i).map(|s| s.as_str()).unwrap_or("-"), got.get(i).map(|s| s.as_str()).unwrap_or("-"));
            println!("[model-draw]   {} port: {e}", if e == g { " " } else { "!" });
            if e != g {
                println!("[model-draw]     game: {g}");
            }
        }
    }
    0
}

fn report() {
    let (n, same, diff) = (CALLS.load(Ordering::Relaxed), SAME.load(Ordering::Relaxed), DIFFERENT.load(Ordering::Relaxed));
    let mut s = STOPS.lock().unwrap().clone();
    s.sort_by(|a, b| b.1.cmp(&a.1));
    let stops: Vec<String> = s.iter().map(|(w, k)| format!("{w} {:.1}%", *k as f64 * 100.0 / n as f64)).collect();
    let vf: Vec<String> =
        draw::UNKNOWN_VFNS.lock().unwrap().iter().map(|(slot, f)| format!("+{slot:#x}=main+{f:#x}")).collect();
    if !vf.is_empty() {
        println!("[model-draw] virtual functions met: {}", vf.join(" "));
    }
    println!(
        "[model-draw] {n} calls: {:.1}% identical, {diff} different; unported: {}",
        same as f64 * 100.0 / n as f64,
        stops.join(", ")
    );
}

extern "C" {
    fn sys_icache_invalidate(start: *mut libc::c_void, len: usize);
}

/// The entry branches to a wrapper: on_model_draw(args); unless it returned nonzero, the function itself (the displaced
/// instruction, then the rest of it) as a call; then on_model_draw_end.
pub unsafe fn install(base: usize, mem_space: &mut crate::guest::loader::GuestMemorySpace) {
    let site = (base + SITE) as *mut u32;
    let actual = std::ptr::read(site);
    if actual != SITE_INSTRUCTION {
        println!("[!] Model draw hook not installed: main+{SITE:#x} = {actual:#010x}");
        return;
    }
    let pre = mem_space.make_hle_trampoline(crate::hle!(on_model_draw(_, _, _, _))) as usize;
    let post = mem_space.make_hle_trampoline(crate::hle!(on_model_draw_end())) as usize;
    let wrap = mem_space.make_code_block(20) as usize;
    let b = |from: usize, to: usize, link: bool| -> Option<u32> {
        let d = to as i64 - from as i64;
        (d.abs() < 1 << 27).then(|| (if link { 0x9400_0000 } else { 0x1400_0000 }) | ((d >> 2) as u32 & 0x03ff_ffff))
    };
    let at = |k: usize| wrap + k * 4;
    let (Some(to_wrap), Some(call_pre), Some(call_body), Some(call_post), Some(back)) = (
        b(site as usize, wrap, false),
        b(at(6), pre, true),
        b(at(12), at(18), true),
        b(at(13), post, true),
        b(at(19), site as usize + 4, false),
    ) else {
        println!("[!] Model draw hook not installed: wrapper out of branch range");
        return;
    };
    let code: [u32; 20] = [
        0xa9ba7bfd,       // 0: stp x29, x30, [sp, #-0x60]!
        0x910003fd,       // 1: mov x29, sp
        0xa90107e0,       // 2: stp x0, x1, [sp, #0x10]
        0xa9020fe2,       // 3: stp x2, x3, [sp, #0x20]
        0xa90317e4,       // 4: stp x4, x5, [sp, #0x30]
        0xa9041fe6,       // 5: stp x6, x7, [sp, #0x40]
        call_pre,         // 6: bl on_model_draw
        0x35000120,       // 7: cbnz w0, 16 (handled)
        0xa9441fe6,       // 8: ldp x6, x7, [sp, #0x40]
        0xa94317e4,       // 9: ldp x4, x5, [sp, #0x30]
        0xa9420fe2,       // 10: ldp x2, x3, [sp, #0x20]
        0xa94107e0,       // 11: ldp x0, x1, [sp, #0x10]
        call_body,        // 12: bl 18 (the function)
        call_post,        // 13: bl on_model_draw_end
        0xa8c67bfd,       // 14: ldp x29, x30, [sp], #0x60
        0xd65f03c0,       // 15: ret
        0xa8c67bfd,       // 16: ldp x29, x30, [sp], #0x60
        0xd65f03c0,       // 17: ret
        SITE_INSTRUCTION, // 18: the displaced instruction
        back,             // 19: b site+4
    ];
    let w = wrap as *mut u32;
    for (k, c) in code.iter().enumerate() {
        std::ptr::write(w.add(k), *c);
    }
    sys_icache_invalidate(w as *mut libc::c_void, code.len() * 4);
    std::ptr::write(site, to_wrap);
    sys_icache_invalidate(site as *mut libc::c_void, 4);
    println!("[+] Model draw hook installed at main+{SITE:#x}");
}
