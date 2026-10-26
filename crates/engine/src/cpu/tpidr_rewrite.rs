// Load-time rewrite of guest `mrs Xt, tpidr_el0` instructions.
//
// Horizon gives each thread a stable TPIDR_EL0 (its TLS pointer). XNU on Apple Silicon rewrites the
// user TPIDR_EL0 when it reschedules a thread (it carries CPU information, e.g. 0x1006), so a guest
// `mrs Xt, tpidr_el0` can return garbage at any time, and a pointer derived from it may be spilled to the
// stack and dereferenced much later. The engine keeps each thread's guest TLS pointer in a Darwin TSD slot of
// its own (a pthread key, cpu::tls::guest_tls_offset; set by horizon::hle::setup_horizon_tpidrro), which XNU never touches.
// Every read site is redirected to a stub in a code island next to the guest modules:
//     mrs Xt, tpidrro_el0 ; ldr Xt, [Xt, #slot] ; b <site + 4>

use crate::cpu::aot::encode_b;

const MRS_TPIDR_MASK: u32 = 0xFFFF_FFE0;
const MRS_TPIDR: u32 = 0xD53B_D040;
const MRS_TPIDRRO: u32 = 0xD53B_D060;
const BRANCH_RANGE: isize = 128 << 20;

extern "C" {
    static mach_task_self_: u32;
    fn mach_vm_allocate(task: u32, addr: *mut u64, size: u64, flags: i32) -> i32;
    fn mach_vm_deallocate(task: u32, addr: u64, size: u64) -> i32;
}

fn find_sites(start: usize, len: usize) -> Vec<usize> {
    let mut sites = Vec::new();
    for off in (0..len & !3).step_by(4) {
        let w = unsafe { *((start + off) as *const u32) };
        if w & MRS_TPIDR_MASK == MRS_TPIDR && w & 0x1f != 31 {
            sites.push(start + off);
        }
    }
    sites
}

/// Maps a code island within branch range of every text range, then rewrites each read site.
/// Returns the number of rewritten sites.
/// `ranges` are (start, len, already_executable): text that is still writable at startup is patched in
/// place; executable text is flipped to RW around each write.
pub unsafe fn rewrite(
    ranges: &[(usize, usize, bool)],
    slot: (usize, usize),
) -> Result<usize, String> {
    let sites: Vec<(usize, bool)> = ranges
        .iter()
        .flat_map(|&(s, l, x)| find_sites(s, l).into_iter().map(move |a| (a, x)))
        .collect();
    if sites.is_empty() {
        return Ok(0);
    }
    let lo = ranges.iter().map(|r| r.0).min().unwrap();
    let hi = ranges.iter().map(|r| r.0 + r.1).max().unwrap();
    let sites_only: Vec<usize> = sites.iter().map(|s| s.0).collect();
    let _ = &sites_only;
    let island_len = (sites.len() * 12 + 0x3fff) & !0x3fff;
    // Every 1 MiB-aligned address from which a branch reaches all of the guest text, nearest first, taken with a fixed
    // allocation that fails rather than moving or overwriting. A few mmap hints used to be tried instead: the kernel
    // places a hinted mapping elsewhere when the spot is taken (it varies per run), the rewrite was then skipped, and
    // guest TLS reads got XNU's CPU number: crashes reading [thread pointer + 0x10] with pointers 0..7.
    let mut island = 0usize;
    let step = 0x10_0000usize;
    let span = BRANCH_RANGE as usize - step;
    let low_end = lo.saturating_sub(island_len);
    let mut candidates: Vec<usize> = Vec::new();
    let mut a = (hi + step - 1) & !(step - 1);
    while a + island_len <= lo + span {
        candidates.push(a);
        a += step;
    }
    let mut a = low_end & !(step - 1);
    while a > 0 && a + span >= hi {
        candidates.push(a);
        a -= step;
    }
    // The slot reserved with main (guest::loader::ISLAND_SLOT_SIZE) first.
    if island_len <= slot.1
        && encode_b(lo, slot.0).is_some()
        && encode_b(hi, slot.0 + island_len).is_some()
        && encode_b(slot.0 + island_len, lo).is_some()
    {
        island = slot.0;
        candidates.clear();
    }
    for a in candidates {
        let mut addr = a as u64;
        let kr = mach_vm_allocate(
            mach_task_self_,
            &mut addr,
            island_len as u64,
            0, /* VM_FLAGS_FIXED */
        );
        if kr != 0 {
            continue;
        }
        if encode_b(lo, a).is_some()
            && encode_b(hi, a + island_len).is_some()
            && encode_b(a + island_len, lo).is_some()
        {
            island = a;
            break;
        }
        mach_vm_deallocate(mach_task_self_, addr, island_len as u64);
    }
    if island != 0 {
        println!(
            "[+] tpidr_el0 rewrite: code island at {:#x} (guest text {:#x}..{:#x})",
            island, lo, hi
        );
    }
    if island == 0 {
        return Err("no code island within branch range of the guest text".into());
    }
    let mut written = 0usize;
    for (i, &(site, executable)) in sites.iter().enumerate() {
        let rt = *(site as *const u32) & 0x1f;
        let stub = island + i * 12;
        let (Some(to_stub), Some(back)) = (encode_b(site, stub), encode_b(stub + 8, site + 4))
        else {
            continue;
        };
        let s = stub as *mut u32;
        *s = MRS_TPIDRRO | rt;
        *s.add(1) = 0xF940_0000
            | (((crate::cpu::tls::guest_tls_offset() / 8) as u32) << 10)
            | (rt << 5)
            | rt; // ldr Xt, [Xt, #guest TLS slot]
        *s.add(2) = back;
        let page = site & !0x3fff;
        if executable {
            let _ = crate::os::protect(page as usize, 0x4000, crate::os::Prot::ReadWrite);
        }
        *(site as *mut u32) = to_stub;
        if executable {
            crate::cpu::aot::protect_rx(page, 0x4000);
        }
        written += 1;
    }
    crate::cpu::aot::make_exec("tpidr.island", island, island_len);
    crate::os::flush_icache(island as usize, island_len);
    for &(s, l, _) in ranges {
        crate::os::flush_icache(s as usize, l);
    }
    println!("[+] tpidr_el0 rewrite: {} of {} guest read sites now load the TLS pointer from TSD (island at {:#x})", written, sites.len(), island);
    Ok(written)
}
