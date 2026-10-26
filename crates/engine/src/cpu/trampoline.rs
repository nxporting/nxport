// Static Out-Of-Line Trampoline Rewriter for x18.
//
// Uniformly replaces all instructions touching x18 with direct branches to
// an out-of-line trampoline island, emulating x18 safely via thread-local storage.
//
// Guest code uses x18 as an ordinary register (Kirby main ~6.6k instructions, its SDK ~12k, TotK main ~31k). Darwin
// may clear x18 on a context switch. macOS keeps it for processes linked against a macOS SDK older than 13
// (task->preserve_x18; .cargo/config.toml links with -platform_version macos,11.0,12.0), so macOS builds run guest
// code as is. iOS (and the iOS simulator) never preserve it: there every x18 use is rewritten to a stub that keeps
// the value in a TSD slot (cpu::tls::x18_slot_offset). See x18_trampolines().

use capstone::arch::arm64::{Arm64OperandType, Arm64Reg};
use capstone::prelude::*;
use std::collections::HashMap;
use std::ptr;

pub static CURRENT_X18_CTX_PTR: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Whether x18 uses must be rewritten: always on iOS; on macOS unless the kernel preserves x18 for this process (the
/// executable's LC_BUILD_VERSION names a macOS SDK older than 13). --x18-trampolines=true / false overrides.
pub fn x18_trampolines() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| {
        if let Some(on) = crate::settings::get().guest.x18_trampolines {
            return on;
        }
        if cfg!(target_os = "ios") {
            return true;
        }
        let preserved = matches!(own_build_version(), Some((1, sdk)) if (sdk >> 16) < 13);
        !preserved
    })
}

/// (platform, sdk) from the executable's LC_BUILD_VERSION (platform 1 = macOS; sdk = major << 16 | minor << 8).
fn own_build_version() -> Option<(u32, u32)> {
    extern "C" {
        fn _dyld_get_image_header(index: u32) -> *const u8;
    }
    unsafe {
        let h = _dyld_get_image_header(0);
        if h.is_null() || *(h as *const u32) != 0xfeed_facf {
            return None;
        }
        let ncmds = *(h.add(16) as *const u32);
        let mut p = h.add(32);
        for _ in 0..ncmds {
            let (cmd, size) = (*(p as *const u32), *(p.add(4) as *const u32));
            if cmd == 0x32 {
                return Some((*(p.add(8) as *const u32), *(p.add(16) as *const u32)));
            }
            p = p.add(size as usize);
        }
        None
    }
}

pub fn set_thread_x18(val: usize) {
    if x18_trampolines() {
        unsafe {
            libc::pthread_setspecific(crate::cpu::tls::x18_key(), val as *mut libc::c_void);
        }
    }
}

/// The island all modules' x18 stubs go to (set by main.rs, used again by module_loader for mods).
pub static ISLAND: std::sync::Mutex<Option<TrampolineIsland>> = std::sync::Mutex::new(None);

/// Rewrites the x18 uses of a module's text (writable at this point) into the shared island.
pub unsafe fn rewrite_module(name: &str, text: usize, len: usize) -> Result<usize, String> {
    let mut g = ISLAND.lock().unwrap();
    let island = g.as_mut().ok_or("x18 island not set up")?;
    let n = island
        .patch_text(text as *mut u32, len)
        .map_err(|e| format!("{}: {}", name, e))?;
    println!(
        "[+] x18: {} rewrote {} instructions (island {} KB used)",
        name,
        n,
        island.next_offset >> 10
    );
    Ok(n)
}

pub fn init_worker_thread_x18() {
    let ctx = CURRENT_X18_CTX_PTR.load(std::sync::atomic::Ordering::Relaxed);
    if ctx != 0 {
        set_thread_x18(ctx);
    }
}

struct AtomicSeqInsn {
    word: u32,
    pc: usize,
    writes_x18: bool,
    is_cond_branch: bool,
    target_pc: usize,
}

struct AtomicSequence {
    _start_idx: usize,
    end_idx: usize,
    insns: Vec<AtomicSeqInsn>,
    reg_used: [bool; 32],
    needs_tls_load: bool,
    needs_tls_store: bool,
}

pub struct TrampolineIsland {
    pub base_ptr: *mut u32,
    pub capacity_bytes: usize,
    pub next_offset: usize,
    pub pthread_key_offset: usize,
}

// The island is only touched with the ISLAND lock held (startup, module loading).
unsafe impl Send for TrampolineIsland {}

impl TrampolineIsland {
    /// An island in writable memory the caller reserved (guest::loader::x18_island_area: a fixed place, so the stubs are the
    /// same on every launch, see cpu/aot.rs). The x18 slot is the fixed TSD key cpu::tls::x18_key.
    pub fn new(base: usize, capacity_bytes: usize) -> Result<Self, &'static str> {
        if base == 0 {
            return Err("no memory for the x18 island");
        }
        let ptr = base as *mut libc::c_void;
        let key = crate::cpu::tls::x18_key();
        let pthread_key_offset = (key as usize) * 8;
        println!("[+] Trampoline Island allocated at {:p} (capacity: {} KB), pthread_key={} (offset: {} bytes from tpidrro_el0)",
                 ptr, capacity_bytes / 1024, key, pthread_key_offset);

        Ok(Self {
            base_ptr: ptr as *mut u32,
            capacity_bytes,
            next_offset: 0,
            pthread_key_offset,
        })
    }

    /// Sets the emulated x18 value for the current thread
    pub fn set_current_thread_x18(&self, val: usize) {
        CURRENT_X18_CTX_PTR.store(val, std::sync::atomic::Ordering::Relaxed);
        set_thread_x18(val);
    }

    /// Rewrite all x18 instructions in text_slice into branches to trampoline stubs.
    pub fn patch_text(
        &mut self,
        text_ptr: *mut u32,
        text_size: usize,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let cs = Capstone::new()
            .arm64()
            .mode(arch::arm64::ArchMode::Arm)
            .detail(true)
            .build()?;

        let reg_x18 = cs.reg_name(RegId(Arm64Reg::ARM64_REG_X18 as u16)).unwrap();
        let reg_w18 = cs.reg_name(RegId(Arm64Reg::ARM64_REG_W18 as u16)).unwrap();

        let num_instructions = text_size / 4;
        let mut patched_count = 0;

        // Pass 1: Identify exclusive/atomic instructions (ldxr/stxr) and x18 references
        let mut is_ldxr = vec![false; num_instructions];
        let mut is_stxr = vec![false; num_instructions];
        let mut touches_x18_vec = vec![false; num_instructions];

        unsafe {
            for i in 0..num_instructions {
                let pc = text_ptr.add(i) as usize;
                let fw = ptr::read(text_ptr.add(i));
                let chunk = fw.to_le_bytes();

                let insns = match cs.disasm_count(&chunk, pc as u64, 1) {
                    Ok(ins) => ins,
                    Err(_) => continue,
                };
                let Some(insn) = insns.first() else {
                    continue;
                };
                let mnem = insn.mnemonic().unwrap_or("");
                if mnem.starts_with("ldxr") || mnem.starts_with("ldaxr") {
                    is_ldxr[i] = true;
                } else if mnem.starts_with("stxr") || mnem.starts_with("stlxr") {
                    is_stxr[i] = true;
                }

                if let Ok(detail) = cs.insn_detail(insn) {
                    let arch_detail = detail.arch_detail();
                    if let Some(arm64_detail) = arch_detail.arm64() {
                        for r in detail.regs_read().iter().chain(detail.regs_write().iter()) {
                            if let Some(name) = cs.reg_name(*r) {
                                if name == reg_x18 || name == reg_w18 {
                                    touches_x18_vec[i] = true;
                                }
                            }
                        }
                        for op in arm64_detail.operands() {
                            match op.op_type {
                                Arm64OperandType::Reg(r) => {
                                    if let Some(name) = cs.reg_name(r) {
                                        if name == reg_x18 || name == reg_w18 {
                                            touches_x18_vec[i] = true;
                                        }
                                    }
                                }
                                Arm64OperandType::Mem(m) => {
                                    if let Some(name) = cs.reg_name(m.base()) {
                                        if name == reg_x18 || name == reg_w18 {
                                            touches_x18_vec[i] = true;
                                        }
                                    }
                                    if let Some(name) = cs.reg_name(m.index()) {
                                        if name == reg_x18 || name == reg_w18 {
                                            touches_x18_vec[i] = true;
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        let mut atomic_seq_map: HashMap<usize, AtomicSequence> = HashMap::new();
        let mut in_atomic_seq = vec![false; num_instructions];

        unsafe {
            for i in 0..num_instructions {
                if is_ldxr[i] {
                    for j in (i + 1)..=(i + 15).min(num_instructions - 1) {
                        if is_stxr[j] {
                            let touches = (i..=j).any(|k| touches_x18_vec[k]);
                            if touches {
                                let mut seq_insns = Vec::new();
                                let mut reg_used = [false; 32];
                                let mut needs_tls_load = false;
                                let mut needs_tls_store = false;
                                let mut x18_written = false;

                                for k in i..=j {
                                    let pc = text_ptr.add(k) as usize;
                                    let fw = ptr::read(text_ptr.add(k));
                                    let chunk = fw.to_le_bytes();
                                    let insns = cs.disasm_count(&chunk, pc as u64, 1)?;
                                    let insn = &insns[0];
                                    let mnem = insn.mnemonic().unwrap_or("");
                                    let detail = cs.insn_detail(insn)?;
                                    let arch_detail = detail.arch_detail();
                                    let arm64_detail = arch_detail.arm64().unwrap();

                                    let is_stxr_insn =
                                        mnem.starts_with("stxr") || mnem.starts_with("stlxr");
                                    let is_store = mnem.starts_with("str")
                                        || mnem.starts_with("stp")
                                        || mnem.starts_with("stur");
                                    let is_cmp = mnem.starts_with("cmp")
                                        || mnem.starts_with("cmn")
                                        || mnem.starts_with("tst");
                                    let is_cond_branch = k < j && mnem.starts_with("b.");

                                    let mut target_pc = 0usize;
                                    if is_cond_branch {
                                        for op in arm64_detail.operands() {
                                            if let Arm64OperandType::Imm(target) = op.op_type {
                                                target_pc = target as usize;
                                            }
                                        }
                                    }

                                    let mut written_here = false;
                                    let mut read_here = false;

                                    for r in
                                        detail.regs_read().iter().chain(detail.regs_write().iter())
                                    {
                                        if let Some(name) = cs.reg_name(*r) {
                                            if let Some(idx) = parse_gpr_index(&name) {
                                                reg_used[idx] = true;
                                            }
                                        }
                                    }

                                    for (op_idx, op) in arm64_detail.operands().enumerate() {
                                        match op.op_type {
                                            Arm64OperandType::Reg(r) => {
                                                if let Some(name) = cs.reg_name(r) {
                                                    if let Some(idx) = parse_gpr_index(&name) {
                                                        reg_used[idx] = true;
                                                    }
                                                    if name == reg_x18 || name == reg_w18 {
                                                        if (op_idx == 0 && !is_store && !is_cmp)
                                                            || (op_idx == 0 && is_stxr_insn)
                                                            || (op_idx == 1
                                                                && (mnem.starts_with("ldp")
                                                                    || mnem.starts_with("ldpsw")))
                                                        {
                                                            written_here = true;
                                                        } else {
                                                            read_here = true;
                                                        }
                                                    }
                                                }
                                            }
                                            Arm64OperandType::Mem(m) => {
                                                if let Some(name) = cs.reg_name(m.base()) {
                                                    if let Some(idx) = parse_gpr_index(&name) {
                                                        reg_used[idx] = true;
                                                    }
                                                    if name == reg_x18 || name == reg_w18 {
                                                        read_here = true;
                                                        let op_str = insn.op_str().unwrap_or("");
                                                        if op_str.contains("!")
                                                            || op_str.contains("], #")
                                                        {
                                                            written_here = true;
                                                        }
                                                    }
                                                }
                                                if let Some(name) = cs.reg_name(m.index()) {
                                                    if let Some(idx) = parse_gpr_index(&name) {
                                                        reg_used[idx] = true;
                                                    }
                                                    if name == reg_x18 || name == reg_w18 {
                                                        read_here = true;
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }

                                    if read_here && !x18_written {
                                        needs_tls_load = true;
                                    }
                                    if written_here {
                                        x18_written = true;
                                        needs_tls_store = true;
                                    }

                                    seq_insns.push(AtomicSeqInsn {
                                        word: fw,
                                        pc,
                                        writes_x18: written_here,
                                        is_cond_branch,
                                        target_pc,
                                    });
                                }

                                for k in (i + 1)..=j {
                                    in_atomic_seq[k] = true;
                                }

                                atomic_seq_map.insert(
                                    i,
                                    AtomicSequence {
                                        _start_idx: i,
                                        end_idx: j,
                                        insns: seq_insns,
                                        reg_used,
                                        needs_tls_load,
                                        needs_tls_store,
                                    },
                                );
                            }
                            break;
                        }
                        if is_ldxr[j] {
                            break;
                        }
                    }
                }
            }
        }

        // Jump tables through x18 (`adr x18, BASE; ldrb/ldrh wO, [..]; add x18, x18, xO, lsl #2; br x18`), found in the
        // original text before pass 2 rewrites the instructions leading up to each `br x18` (see emit_stub).
        let mut jump_tables: std::collections::HashMap<usize, (usize, usize)> =
            std::collections::HashMap::new();
        unsafe {
            for i in 0..num_instructions {
                if ptr::read(text_ptr.add(i)) == 0xD61F_0240 {
                    match jump_table_of(text_ptr, i) {
                        Some(jt) => {
                            if crate::settings::get().log.log_x18_jt {
                                println!("[x18-jt] br x18 at +{:#x}: base +{:#x}, {} offsets", i * 4, jt.0 - text_ptr as usize, jt.1);
                            }
                            jump_tables.insert(i, jt);
                        }
                        None => eprintln!("[!] x18: br x18 at {:#x} is not a recognised jump table; its stub clobbers x17", text_ptr.add(i) as usize),
                    }
                }
            }
        }

        // Pass 2: Patch text
        let mut atomic_seqs_count = 0;
        unsafe {
            for i in 0..num_instructions {
                if let Some(seq) = atomic_seq_map.remove(&i) {
                    let pc = text_ptr.add(i) as usize;
                    let repl_idx = (9..=28)
                        .chain(0..=8)
                        .find(|&r| r != 18 && !seq.reg_used[r])
                        .unwrap_or(9) as u32;
                    let scratch_idx = (9..=28)
                        .chain(0..=8)
                        .find(|&r| r != 18 && r != repl_idx as usize && !seq.reg_used[r])
                        .unwrap_or(16) as u32;

                    let stub_addr = (self.base_ptr as usize) + self.next_offset;
                    let diff = (stub_addr as i64) - (pc as i64);
                    if diff < -0x0800_0000 || diff >= 0x0800_0000 {
                        return Err(
                            format!("atomic stub out of branch range: diff = {}", diff).into()
                        );
                    }
                    if self.next_offset + 256 > self.capacity_bytes {
                        return Err("x18 island full".into());
                    }

                    let stub_words =
                        self.emit_atomic_sequence_stub(&seq, stub_addr, repl_idx, scratch_idx)?;
                    self.next_offset += stub_words * 4;

                    let imm26 = ((diff >> 2) as u32) & 0x03FF_FFFF;
                    let branch_insn = 0x1400_0000u32 | imm26;
                    ptr::write(text_ptr.add(i), branch_insn);

                    for k in (i + 1)..=seq.end_idx {
                        ptr::write(text_ptr.add(k), 0xD503201Fu32); // nop
                    }
                    patched_count += seq.end_idx - i + 1;
                    atomic_seqs_count += 1;
                    continue;
                }

                if in_atomic_seq[i] || !touches_x18_vec[i] {
                    continue;
                }

                let pc = text_ptr.add(i) as usize;
                let fw = ptr::read(text_ptr.add(i));
                let chunk = fw.to_le_bytes();

                let insns = match cs.disasm_count(&chunk, pc as u64, 1) {
                    Ok(ins) => ins,
                    Err(_) => continue,
                };

                let Some(insn) = insns.first() else {
                    continue;
                };
                let detail = cs.insn_detail(insn)?;
                let arch_detail = detail.arch_detail();
                let arm64_detail = arch_detail.arm64().unwrap();

                let mut touches_sp = false;
                let mut writes_x18 = false;
                let mut reg_used = [false; 32];

                for r in detail.regs_read().iter().chain(detail.regs_write().iter()) {
                    if let Some(name) = cs.reg_name(*r) {
                        if name == "sp" {
                            touches_sp = true;
                        }
                        if let Some(idx) = parse_gpr_index(&name) {
                            reg_used[idx] = true;
                        }
                    }
                }

                for r in detail.regs_write().iter() {
                    if let Some(name) = cs.reg_name(*r) {
                        if name == reg_x18 || name == reg_w18 {
                            writes_x18 = true;
                        }
                    }
                }

                let mnem = insn.mnemonic().unwrap_or("");
                let is_store =
                    mnem.starts_with("str") || mnem.starts_with("stp") || mnem.starts_with("stur");
                let is_cmp =
                    mnem.starts_with("cmp") || mnem.starts_with("cmn") || mnem.starts_with("tst");
                let is_cbz_cbnz = mnem == "cbz" || mnem == "cbnz";
                let is_tbz_tbnz = mnem == "tbz" || mnem == "tbnz";
                let is_br = mnem == "br";
                let is_blr = mnem == "blr";
                let is_adr = mnem == "adr";
                let is_adrp = mnem == "adrp";
                let is_adr_adrp = is_adr || is_adrp;

                let mut target_val = 0usize;
                if is_adr {
                    let immhi = ((fw >> 5) & 0x7FFFF) as i32;
                    let immlo = ((fw >> 29) & 0x3) as i32;
                    let imm21 = (immhi << 2) | immlo;
                    let signed_imm21 = (imm21 << 11) >> 11;
                    target_val = ((pc as i64) + (signed_imm21 as i64)) as usize;
                } else if is_adrp {
                    let immhi = ((fw >> 5) & 0x7FFFF) as i64;
                    let immlo = ((fw >> 29) & 0x3) as i64;
                    let imm21 = (immhi << 2) | immlo;
                    let signed_imm21 = (imm21 << 43) >> 43;
                    let pc_page = (pc as i64) & !0xFFFi64;
                    target_val = (pc_page + (signed_imm21 << 12)) as usize;
                }

                let mut fp_simd_x18_shift: Option<usize> = None;
                if (fw & 0x0E000000) == 0x0E000000 {
                    for (op_idx, op) in arm64_detail.operands().enumerate() {
                        if let Arm64OperandType::Reg(r) = op.op_type {
                            if let Some(name) = cs.reg_name(r) {
                                if name == reg_x18 || name == reg_w18 {
                                    fp_simd_x18_shift = Some(if op_idx == 0 { 0 } else { 5 });
                                }
                            }
                        }
                    }
                }

                for (idx, op) in arm64_detail.operands().enumerate() {
                    match op.op_type {
                        Arm64OperandType::Reg(r) => {
                            if let Some(name) = cs.reg_name(r) {
                                if (idx == 0
                                    && (name == reg_x18 || name == reg_w18)
                                    && !is_store
                                    && !is_cmp
                                    && !is_cbz_cbnz
                                    && !is_tbz_tbnz
                                    && !is_br
                                    && !is_blr)
                                    || (idx == 1
                                        && (name == reg_x18 || name == reg_w18)
                                        && (mnem.starts_with("ldp") || mnem.starts_with("ldpsw")))
                                {
                                    writes_x18 = true;
                                }
                                if name == "sp" {
                                    touches_sp = true;
                                }
                                if let Some(gpr) = parse_gpr_index(&name) {
                                    reg_used[gpr] = true;
                                }
                            }
                        }
                        Arm64OperandType::Mem(m) => {
                            if let Some(name) = cs.reg_name(m.base()) {
                                if name == "sp" {
                                    touches_sp = true;
                                }
                                if let Some(gpr) = parse_gpr_index(&name) {
                                    reg_used[gpr] = true;
                                }
                                if name == reg_x18 || name == reg_w18 {
                                    let op_str = insn.op_str().unwrap_or("");
                                    if op_str.contains("!") || op_str.contains("], #") {
                                        writes_x18 = true;
                                    }
                                }
                            }
                            if let Some(name) = cs.reg_name(m.index()) {
                                if name == "sp" {
                                    touches_sp = true;
                                }
                                if let Some(gpr) = parse_gpr_index(&name) {
                                    reg_used[gpr] = true;
                                }
                            }
                        }
                        _ => {}
                    }
                }

                if fp_simd_x18_shift == Some(0) {
                    writes_x18 = true;
                }

                let repl_idx = (9..=28)
                    .chain(0..=8)
                    .find(|&r| r != 18 && !reg_used[r])
                    .unwrap_or(9) as u32;
                let scratch_idx = (9..=28)
                    .chain(0..=8)
                    .find(|&r| r != 18 && r != repl_idx as usize && !reg_used[r])
                    .unwrap_or(16) as u32;

                let stub_addr = (self.base_ptr as usize) + self.next_offset;
                let diff = (stub_addr as i64) - (pc as i64);
                if diff < -0x0800_0000 || diff >= 0x0800_0000 {
                    return Err(format!("stub out of branch range: diff = {}", diff).into());
                }
                if self.next_offset + 64 > self.capacity_bytes {
                    return Err("x18 island full".into());
                }

                let stub_words = self.emit_stub(
                    fw,
                    pc,
                    stub_addr,
                    writes_x18,
                    touches_sp,
                    is_cbz_cbnz,
                    is_tbz_tbnz,
                    is_adr_adrp,
                    target_val,
                    is_br,
                    is_blr,
                    fp_simd_x18_shift,
                    repl_idx,
                    scratch_idx,
                    jump_tables.get(&i).copied(),
                )?;
                self.next_offset += stub_words * 4;

                let imm26 = ((diff >> 2) as u32) & 0x03FF_FFFF;
                let branch_insn = 0x1400_0000u32 | imm26;
                ptr::write(text_ptr.add(i), branch_insn);
                patched_count += 1;
            }
        }

        let misses = RENAME_MISSES.with(|m| std::mem::take(&mut *m.borrow_mut()));
        if !misses.is_empty() {
            for (fw, nw) in misses.iter().take(10) {
                eprintln!(
                    "[!!] x18 rename left an x18 operand: {:#010x} -> {:#010x}",
                    fw, nw
                );
            }
            return Err(format!(
                "{} instructions with x18 operands the decoder does not rename",
                misses.len()
            )
            .into());
        }
        println!(
            "[+] Static Trampoline Rewriter: patched {} atomic sequences (zero-store FP stashing)",
            atomic_seqs_count
        );
        println!(
            "[+] Static Trampoline Rewriter: successfully patched {} instructions touching x18",
            patched_count
        );
        Ok(patched_count)
    }

    /// Emits a dynamically register-allocated trampoline stub:
    /// Handles non-branching instructions, PC-relative branches, indirect branches, and ADR/ADRP.
    unsafe fn emit_stub(
        &mut self,
        original_word: u32,
        original_pc: usize,
        stub_addr: usize,
        writes_x18: bool,
        touches_sp: bool,
        is_cbz_cbnz: bool,
        is_tbz_tbnz: bool,
        is_adr_adrp: bool,
        target_val: usize,
        is_br: bool,
        is_blr: bool,
        fp_simd_x18_shift: Option<usize>,
        repl_idx: u32,
        scratch_idx: u32,
        jump_table: Option<(usize, usize)>,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let stub_ptr = stub_addr as *mut u32;
        let mut words: Vec<u32> = Vec::with_capacity(16);
        let imm_scaled = (self.pthread_key_offset / 8) as u32;

        // 0. A jump table through x18: the dispatch must preserve every register. Guest code keeps live values in x16 /
        // x17 inside a function (TotK main+0x1415a18 kept a 12-byte stride in w17 across a switch; the old stub's
        // `br x17` replaced it with the target, and the loop computed a pointer far outside its array: SIGBUS on the
        // iPhone, 2026-10-16). x16 / x17 are saved on the stack, the target's offset from BASE picks a landing that
        // restores them and branches on. Offsets come from a byte or halfword table: 256 or 65536 landings. No flags
        // are changed (sub / add, no compare).
        if let (true, Some((base, landings))) = (is_br, jump_table) {
            const DISPATCH: usize = 10; // instructions before L0
            let need = (DISPATCH + 2 * landings) * 4;
            if self.next_offset + need > self.capacity_bytes {
                return Err("x18 island full (jump table landings)".into());
            }
            let adrp_add = |words: &mut Vec<u32>, rd: u32, target: usize| {
                let here = stub_addr + words.len() * 4;
                let pages = ((target & !0xfff) as i64 - (here & !0xfff) as i64) >> 12;
                words.push(
                    0x9000_0000u32
                        | (((pages & 3) as u32) << 29)
                        | ((((pages >> 2) & 0x7ffff) as u32) << 5)
                        | rd,
                );
                words.push(0x9100_0000u32 | (((target & 0xfff) as u32) << 10) | (rd << 5) | rd);
            };
            words.push(0xA9BF_47F0); // stp x16, x17, [sp, #-16]!
            words.push(0xD53BD060u32 | 17); // mrs x17, tpidrro_el0
            words.push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (17 << 5) | 17); // ldr x17, [x17, #x18 slot]
            adrp_add(&mut words, 16, base); // x16 = BASE
            words.push(0xCB10_0231); // sub x17, x17, x16      (4 * offset)
            let l0 = stub_addr + DISPATCH * 4;
            adrp_add(&mut words, 16, l0); // x16 = L0
            words.push(0x8B11_0611); // add x17, x16, x17, lsl #1  (L0 + 8 * offset)
            words.push(0xD61F_0220); // br x17
                                     // L0 must follow the dispatch exactly (a miscount by one landed every jump on a landing's branch, skipping
                                     // the restore: SP off by 16, a null pointer in the SDK's CFF interpreter at startup).
            if words.len() != DISPATCH {
                return Err(format!(
                    "jump table dispatch is {} instructions, expected {}",
                    words.len(),
                    DISPATCH
                )
                .into());
            }
            for j in 0..landings {
                words.push(0xA8C1_47F0); // L_j: ldp x16, x17, [sp], #16
                let here = stub_addr + words.len() * 4;
                let diff = (base + 4 * j) as i64 - here as i64;
                if diff < -0x0800_0000 || diff >= 0x0800_0000 {
                    return Err("jump table landing out of branch range".into());
                }
                words.push(0x1400_0000u32 | (((diff >> 2) as u32) & 0x03FF_FFFF));
                // b BASE + 4j
            }
            for (i, &w) in words.iter().enumerate() {
                ptr::write(stub_ptr.add(i), w);
            }
            return Ok(words.len());
        }

        // 1. Indirect branch: BR x18. The target goes through x17 (IP1, which AAPCS lets veneers clobber at any branch
        // to another function); this used to borrow d31, which can be live in guest code.
        if is_br {
            words.push(0xD53BD060u32 | 17); // mrs x17, tpidrro_el0
            words.push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (17 << 5) | 17); // ldr x17, [x17, #x18 slot]
            words.push(0xD61F0220u32); // br x17

            for (i, &w) in words.iter().enumerate() {
                ptr::write(stub_ptr.add(i), w);
            }
            return Ok(words.len());
        }

        // 2. Indirect call: BLR x18
        if is_blr {
            words.push(0xD53BD060u32 | 17); // mrs x17, tpidrro_el0 (x17: free at a call, see BR above)
            words.push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (17 << 5) | 17); // ldr x17, [x17, #x18 slot]
                                                                                       // Call; the callee returns here and we branch back. (The return address used to be an absolute literal,
                                                                                       // which made the stub differ per launch; see cpu/aot.rs.)
            words.push(0xD63F0220u32); // blr x17
            let current_stub_pc = stub_addr + (words.len() * 4);
            let return_diff = ((original_pc + 4) as i64) - (current_stub_pc as i64);
            words.push(0x1400_0000u32 | (((return_diff >> 2) as u32) & 0x03FF_FFFF)); // b original_pc + 4

            for (i, &w) in words.iter().enumerate() {
                ptr::write(stub_ptr.add(i), w);
            }
            return Ok(words.len());
        }

        // 3. PC-relative addressing: ADR / ADRP into x18
        if is_adr_adrp {
            // stp scratch, repl, [sp, #-16]!
            words.push(0xA9BF03E0u32 | (repl_idx << 10) | scratch_idx);
            // repl = target_val, computed from this stub's own address (adrp + add): no absolute literal, so the
            // stub is the same on every launch (cpu/aot.rs).
            let here = stub_addr + words.len() * 4;
            let pages = ((target_val & !0xfff) as i64 - (here & !0xfff) as i64) >> 12;
            words.push(
                0x9000_0000u32
                    | (((pages & 3) as u32) << 29)
                    | ((((pages >> 2) & 0x7ffff) as u32) << 5)
                    | repl_idx,
            );
            words.push(
                0x9100_0000u32 | (((target_val & 0xfff) as u32) << 10) | (repl_idx << 5) | repl_idx,
            ); // add repl, repl, #lo12
               // mrs scratch, tpidrro_el0
            words.push(0xD53BD060u32 | scratch_idx);
            // str repl, [scratch, #imm_scaled]
            words
                .push(0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx);
            // ldp scratch, repl, [sp], #16
            words.push(0xA8C103E0u32 | (repl_idx << 10) | scratch_idx);
            // b original_pc + 4
            let return_target = original_pc + 4;
            let current_stub_pc = stub_addr + (words.len() * 4);
            let return_diff = (return_target as i64) - (current_stub_pc as i64);
            let imm26_ret = ((return_diff >> 2) as u32) & 0x03FF_FFFF;
            words.push(0x1400_0000u32 | imm26_ret);

            for (i, &w) in words.iter().enumerate() {
                ptr::write(stub_ptr.add(i), w);
            }
            return Ok(words.len());
        }

        // 3b. SP writeback instructions (LDP/STP/LDR/STR with pre-index or post-index on SP)
        let sp_wb = detect_sp_writeback(original_word);
        match sp_wb {
            SpWriteback::PreIndexPair { scale, imm, is_load, is_64, x18_is_rt2, other_reg } => {
                if !is_load {
                    // Pre-indexed store pair: stp x18, xN, [sp, #imm]!
                    let sp_delta = imm - 16;
                    if sp_delta >= -512 && sp_delta <= 504 && (sp_delta % 8 == 0) {
                        let imm7 = (((sp_delta / 8) as u32) & 0x7F) << 15;
                        words.push(0xA98003E0u32 | imm7 | (repl_idx << 10) | scratch_idx);
                    } else {
                        let abs_delta = (-sp_delta) as u32;
                        words.push(0xD10003FFu32 | ((abs_delta & 0xFFF) << 10)); // sub sp, sp, #abs_delta
                        words.push(0xA90003E0u32 | (repl_idx << 10) | scratch_idx); // stp scratch, repl, [sp]
                    }
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // ldr repl, [scratch, #imm_scaled]
                    let opc = if is_64 { 2u32 } else { 0u32 };
                    let (r1, r2) = if other_reg == 18 {
                        (repl_idx, repl_idx)
                    } else if x18_is_rt2 {
                        (other_reg, repl_idx)
                    } else {
                        (repl_idx, other_reg)
                    };
                    let imm7_pair = (((16 / scale) as u32) & 0x7F) << 15;
                    words.push((opc << 30) | 0x290003E0u32 | imm7_pair | (r2 << 10) | r1); // stp r1, r2, [sp, #16]
                    words.push(0xA8C103E0u32 | (repl_idx << 10) | scratch_idx); // ldp scratch, repl, [sp], #16
                } else {
                    // Pre-indexed load pair: ldp x18, xN, [sp, #imm]!
                    words.push(0xA9BF03E0u32 | (repl_idx << 10) | scratch_idx); // stp scratch, repl, [sp, #-16]!
                    let load_offset = 16 + imm;
                    let opc = if is_64 { 2u32 } else { 0u32 };
                    let (r1, r2) = if other_reg == 18 {
                        (repl_idx, scratch_idx)
                    } else if x18_is_rt2 {
                        (other_reg, repl_idx)
                    } else {
                        (repl_idx, other_reg)
                    };
                    if load_offset >= -512 && load_offset <= 504 && (load_offset % scale == 0) {
                        let imm7_load = (((load_offset / scale) as u32) & 0x7F) << 15;
                        words.push((opc << 30) | 0x294003E0u32 | imm7_load | (r2 << 10) | r1); // ldp r1, r2, [sp, #load_offset]
                    } else {
                        words.push(0x91000000u32 | (((load_offset as u32) & 0xFFF) << 10) | (31 << 5) | scratch_idx); // add scratch, sp, #load_offset
                        words.push((opc << 30) | 0x29400000u32 | (scratch_idx << 5) | (r2 << 10) | r1); // ldp r1, r2, [scratch]
                    }
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // str repl, [scratch, #imm_scaled]
                    words.push(0xA8C103E0u32 | (repl_idx << 10) | scratch_idx); // ldp scratch, repl, [sp], #16
                    if imm > 0 {
                        words.push(0x910003FFu32 | (((imm as u32) & 0xFFF) << 10)); // add sp, sp, #imm
                    } else if imm < 0 {
                        words.push(0xD10003FFu32 | ((((-imm) as u32) & 0xFFF) << 10)); // sub sp, sp, #(-imm)
                    }
                }
                let current_stub_pc = stub_addr + (words.len() * 4);
                let return_diff = ((original_pc + 4) as i64) - (current_stub_pc as i64);
                words.push(0x1400_0000u32 | (((return_diff >> 2) as u32) & 0x03FF_FFFF));
                for (i, &w) in words.iter().enumerate() {
                    ptr::write(stub_ptr.add(i), w);
                }
                return Ok(words.len());
            }
            SpWriteback::PostIndexPair { scale, imm, is_load, is_64, x18_is_rt2, other_reg } => {
                words.push(0xA9BF03E0u32 | (repl_idx << 10) | scratch_idx); // stp scratch, repl, [sp, #-16]!
                let opc = if is_64 { 2u32 } else { 0u32 };
                let imm7_pair = (((16 / scale) as u32) & 0x7F) << 15;
                if is_load {
                    // Post-indexed load pair: ldp x18, xN, [sp], #imm
                    let (r1, r2) = if other_reg == 18 {
                        (repl_idx, scratch_idx)
                    } else if x18_is_rt2 {
                        (other_reg, repl_idx)
                    } else {
                        (repl_idx, other_reg)
                    };
                    words.push((opc << 30) | 0x294003E0u32 | imm7_pair | (r2 << 10) | r1); // ldp r1, r2, [sp, #16]
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // str repl, [scratch, #imm_scaled]
                } else {
                    // Post-indexed store pair: stp x18, xN, [sp], #imm
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // ldr repl, [scratch, #imm_scaled]
                    let (r1, r2) = if other_reg == 18 {
                        (repl_idx, repl_idx)
                    } else if x18_is_rt2 {
                        (other_reg, repl_idx)
                    } else {
                        (repl_idx, other_reg)
                    };
                    words.push((opc << 30) | 0x290003E0u32 | imm7_pair | (r2 << 10) | r1); // stp r1, r2, [sp, #16]
                }
                words.push(0xA8C103E0u32 | (repl_idx << 10) | scratch_idx); // ldp scratch, repl, [sp], #16
                if imm > 0 {
                    words.push(0x910003FFu32 | (((imm as u32) & 0xFFF) << 10)); // add sp, sp, #imm
                } else if imm < 0 {
                    words.push(0xD10003FFu32 | ((((-imm) as u32) & 0xFFF) << 10)); // sub sp, sp, #(-imm)
                }
                let current_stub_pc = stub_addr + (words.len() * 4);
                let return_diff = ((original_pc + 4) as i64) - (current_stub_pc as i64);
                words.push(0x1400_0000u32 | (((return_diff >> 2) as u32) & 0x03FF_FFFF));
                for (i, &w) in words.iter().enumerate() {
                    ptr::write(stub_ptr.add(i), w);
                }
                return Ok(words.len());
            }
            SpWriteback::PreIndexSingle { scale, size, imm, is_load } => {
                if !is_load {
                    // Pre-indexed single store: str x18, [sp, #imm]!
                    let sp_delta = imm - 16;
                    if sp_delta >= -512 && sp_delta <= 504 && (sp_delta % 8 == 0) {
                        let imm7 = (((sp_delta / 8) as u32) & 0x7F) << 15;
                        words.push(0xA98003E0u32 | imm7 | (repl_idx << 10) | scratch_idx);
                    } else {
                        let abs_delta = (-sp_delta) as u32;
                        words.push(0xD10003FFu32 | ((abs_delta & 0xFFF) << 10)); // sub sp, sp, #abs_delta
                        words.push(0xA90003E0u32 | (repl_idx << 10) | scratch_idx); // stp scratch, repl, [sp]
                    }
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // ldr repl, [scratch, #imm_scaled]
                    let imm_scaled_16 = 16 / scale;
                    let store_op = 0x39000000u32 | ((size as u32) << 30) | (((imm_scaled_16 as u32) & 0xFFF) << 10) | (31 << 5) | repl_idx;
                    words.push(store_op); // str repl, [sp, #16]
                    words.push(0xA8C103E0u32 | (repl_idx << 10) | scratch_idx); // ldp scratch, repl, [sp], #16
                } else {
                    // Pre-indexed single load: ldr x18, [sp, #imm]!
                    words.push(0xA9BF03E0u32 | (repl_idx << 10) | scratch_idx); // stp scratch, repl, [sp, #-16]!
                    let load_offset = 16 + imm;
                    let imm_scaled_load = load_offset / scale;
                    if imm_scaled_load >= 0 && imm_scaled_load <= 0xFFF && (load_offset % scale == 0) {
                        let load_op = 0x39400000u32 | ((size as u32) << 30) | (((imm_scaled_load as u32) & 0xFFF) << 10) | (31 << 5) | repl_idx;
                        words.push(load_op);
                    } else {
                        words.push(0x91000000u32 | (((load_offset as u32) & 0xFFF) << 10) | (31 << 5) | scratch_idx); // add scratch, sp, #load_offset
                        let load_op = 0x39400000u32 | ((size as u32) << 30) | (scratch_idx << 5) | repl_idx;
                        words.push(load_op);
                    }
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // str repl, [scratch, #imm_scaled]
                    words.push(0xA8C103E0u32 | (repl_idx << 10) | scratch_idx); // ldp scratch, repl, [sp], #16
                    if imm > 0 {
                        words.push(0x910003FFu32 | (((imm as u32) & 0xFFF) << 10)); // add sp, sp, #imm
                    } else if imm < 0 {
                        words.push(0xD10003FFu32 | ((((-imm) as u32) & 0xFFF) << 10)); // sub sp, sp, #(-imm)
                    }
                }
                let current_stub_pc = stub_addr + (words.len() * 4);
                let return_diff = ((original_pc + 4) as i64) - (current_stub_pc as i64);
                words.push(0x1400_0000u32 | (((return_diff >> 2) as u32) & 0x03FF_FFFF));
                for (i, &w) in words.iter().enumerate() {
                    ptr::write(stub_ptr.add(i), w);
                }
                return Ok(words.len());
            }
            SpWriteback::PostIndexSingle { scale, size, imm, is_load } => {
                words.push(0xA9BF03E0u32 | (repl_idx << 10) | scratch_idx); // stp scratch, repl, [sp, #-16]!
                let imm_scaled_16 = 16 / scale;
                if is_load {
                    // Post-indexed single load: ldr x18, [sp], #imm
                    let load_op = 0x39400000u32 | ((size as u32) << 30) | (((imm_scaled_16 as u32) & 0xFFF) << 10) | (31 << 5) | repl_idx;
                    words.push(load_op); // ldr repl, [sp, #16]
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // str repl, [scratch, #imm_scaled]
                } else {
                    // Post-indexed single store: str x18, [sp], #imm
                    words.push(0xD53BD060u32 | scratch_idx); // mrs scratch, tpidrro_el0
                    words.push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx); // ldr repl, [scratch, #imm_scaled]
                    let store_op = 0x39000000u32 | ((size as u32) << 30) | (((imm_scaled_16 as u32) & 0xFFF) << 10) | (31 << 5) | repl_idx;
                    words.push(store_op); // str repl, [sp, #16]
                }
                words.push(0xA8C103E0u32 | (repl_idx << 10) | scratch_idx); // ldp scratch, repl, [sp], #16
                if imm > 0 {
                    words.push(0x910003FFu32 | (((imm as u32) & 0xFFF) << 10)); // add sp, sp, #imm
                } else if imm < 0 {
                    words.push(0xD10003FFu32 | ((((-imm) as u32) & 0xFFF) << 10)); // sub sp, sp, #(-imm)
                }
                let current_stub_pc = stub_addr + (words.len() * 4);
                let return_diff = ((original_pc + 4) as i64) - (current_stub_pc as i64);
                words.push(0x1400_0000u32 | (((return_diff >> 2) as u32) & 0x03FF_FFFF));
                for (i, &w) in words.iter().enumerate() {
                    ptr::write(stub_ptr.add(i), w);
                }
                return Ok(words.len());
            }
            SpWriteback::None => {}
        }

        // 4. stp scratch, repl, [sp, #-16]! (Pre-index store pair, 16-byte aligned)
        let stp_insn = 0xA9BF03E0u32 | (repl_idx << 10) | scratch_idx;
        words.push(stp_insn);

        // 5. mrs scratch, tpidrro_el0 -> 0xD53BD060 | scratch_idx
        let mrs_tpidrro = 0xD53BD060u32 | scratch_idx;
        words.push(mrs_tpidrro);

        // 6. ldr repl, [scratch, #pthread_key_offset]
        let ldr_repl = 0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx;
        words.push(ldr_repl);

        let ldp_restore = 0xA8C103E0u32 | (repl_idx << 10) | scratch_idx;

        // Special handling for PC-relative conditional branches (CBZ, CBNZ, TBZ, TBNZ)
        if is_cbz_cbnz || is_tbz_tbnz {
            let target_pc = if is_cbz_cbnz {
                // CBZ/CBNZ: imm19 is at bits 23:5
                let imm19 = (original_word >> 5) & 0x7FFFF;
                let signed_imm19 = ((imm19 as i32) << 13) >> 13;
                ((original_pc as i64) + ((signed_imm19 as i64) << 2)) as usize
            } else {
                // TBZ/TBNZ: imm14 is at bits 18:5
                let imm14 = (original_word >> 5) & 0x3FFF;
                let signed_imm14 = ((imm14 as i32) << 18) >> 18;
                ((original_pc as i64) + ((signed_imm14 as i64) << 2)) as usize
            };

            // In the stub:
            // 4. Branch to taken_label (+3 instructions = +12 bytes = offset #3):
            // Replace register 18 with repl_idx, and set relative branch to +3 words (+12 bytes)
            let mut branch_in_stub = if let Some(shift) = fp_simd_x18_shift {
                (original_word & !(0x1F << shift)) | (repl_idx << shift)
            } else {
                rename_checked(original_word, repl_idx)
            };
            if is_cbz_cbnz {
                branch_in_stub = (branch_in_stub & !(0x7FFFF << 5)) | (3u32 << 5);
            } else {
                branch_in_stub = (branch_in_stub & !(0x3FFF << 5)) | (3u32 << 5);
            }
            words.push(branch_in_stub);

            // 5. Fall-through path (condition NOT taken):
            words.push(ldp_restore); // Restore scratch, repl
            let return_target = original_pc + 4;
            let current_pc = stub_addr + (words.len() * 4);
            let diff_not_taken = (return_target as i64) - (current_pc as i64);
            let imm26_not_taken = ((diff_not_taken >> 2) as u32) & 0x03FF_FFFF;
            words.push(0x1400_0000u32 | imm26_not_taken); // b (original_pc + 4)

            // 6. Taken path (condition TAKEN):
            // (Target of branch_in_stub)
            words.push(ldp_restore); // Restore scratch, repl
            let current_pc_taken = stub_addr + (words.len() * 4);
            let diff_taken = (target_pc as i64) - (current_pc_taken as i64);
            let imm26_taken = ((diff_taken >> 2) as u32) & 0x03FF_FFFF;
            words.push(0x1400_0000u32 | imm26_taken); // b target_pc

            for (i, &w) in words.iter().enumerate() {
                ptr::write(stub_ptr.add(i), w);
            }
            return Ok(words.len());
        }

        // `mrs x18, tpidr_el0`: the guest's thread pointer (cpu/tpidr_rewrite.rs handles every other register; it only scans
        // module text, so moved here the instruction would read Darwin's TPIDR_EL0).
        if original_word == 0xD53B_D040 | 18 {
            words.push(0xD53B_D060 | repl_idx); // mrs repl, tpidrro_el0
            words.push(
                0xF940_0000
                    | (((crate::cpu::tls::guest_tls_offset() / 8) as u32) << 10)
                    | (repl_idx << 5)
                    | repl_idx,
            );
            words.push(mrs_tpidrro);
            words
                .push(0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx);
            words.push(ldp_restore);
            let current_stub_pc = stub_addr + (words.len() * 4);
            let return_diff = ((original_pc + 4) as i64) - (current_stub_pc as i64);
            words.push(0x1400_0000u32 | (((return_diff >> 2) as u32) & 0x03FF_FFFF));
            for (i, &w) in words.iter().enumerate() {
                ptr::write(stub_ptr.add(i), w);
            }
            return Ok(words.len());
        }

        // 4. Normal non-branching instruction
        let modified_insn = if let Some(shift) = fp_simd_x18_shift {
            (original_word & !(0x1F << shift)) | (repl_idx << shift)
        } else {
            rename_checked(original_word, repl_idx)
        };

        // If instruction accesses SP, adjust immediate offset by +16 because SP was pushed by 16 bytes!
        if touches_sp {
            let adjusted = adjust_sp_offset(modified_insn);
            if adjusted != modified_insn {
                words.push(adjusted);
            } else if (original_word & 0x3B800000) == 0x29000000 {
                // LDP/STP signed immediate overflow when adding 16:
                // Pre-compute effective SP address in scratch_idx
                let opc = (original_word >> 30) & 3;
                let scale = if opc == 2 { 8 } else { 4 };
                let imm7 = (original_word >> 15) & 0x7F;
                let signed_imm7 = ((imm7 as i32) << 25) >> 25;
                let byte_offset = (signed_imm7 * scale) + 16;
                // add scratch_idx, sp, #byte_offset
                words.push(
                    0x91000000u32
                        | (((byte_offset as u32) & 0xFFF) << 10)
                        | (31 << 5)
                        | scratch_idx,
                );
                // change base register in modified_insn from SP (31) to scratch_idx, and imm7 to 0
                let mut base_scratch = (modified_insn & !(0x1F << 5)) | (scratch_idx << 5);
                base_scratch &= !(0x7F << 15); // imm7 = 0
                words.push(base_scratch);
            } else {
                words.push(modified_insn);
            }
        } else {
            words.push(modified_insn);
        }

        // 5. If original instruction wrote to x18, save repl back to TLS
        if writes_x18 {
            words.push(mrs_tpidrro);
            let str_repl =
                0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx;
            words.push(str_repl);
        }

        // 6. ldp scratch, repl, [sp], #16 (Post-index load pair)
        words.push(ldp_restore);

        // 7. Branch back to original_pc + 4
        let return_target = original_pc + 4;
        let current_stub_pc = stub_addr + (words.len() * 4);
        let return_diff = (return_target as i64) - (current_stub_pc as i64);
        let imm26_ret = ((return_diff >> 2) as u32) & 0x03FF_FFFF;
        let b_ret = 0x1400_0000u32 | imm26_ret;
        words.push(b_ret);

        for (i, &w) in words.iter().enumerate() {
            ptr::write(stub_ptr.add(i), w);
        }

        Ok(words.len())
    }

    /// Emits a zero-store atomic sequence stub (ldxr .. stxr):
    /// Preserves caller-saved scratch registers in FP/SIMD register d31 with zero memory stores,
    /// ensuring the hardware exclusive monitor is never reset during the atomic window.
    unsafe fn emit_atomic_sequence_stub(
        &mut self,
        seq: &AtomicSequence,
        stub_addr: usize,
        repl_idx: u32,
        scratch_idx: u32,
    ) -> Result<usize, Box<dyn std::error::Error>> {
        let stub_ptr = stub_addr as *mut u32;
        let mut words: Vec<u32> = Vec::with_capacity(32);
        let imm_scaled = (self.pthread_key_offset / 8) as u32;

        // 0. Preserve repl_idx in FP register d30 (scratch register preserved with zero memory stores)
        words.push(0x9E670000u32 | (repl_idx << 5) | 30); // fmov d30, repl_idx

        // 1. Entry: Load x18 from TLS into repl_idx if needed (BEFORE ldxr)
        if seq.needs_tls_load {
            // fmov d31, scratch_idx
            words.push(0x9E670000u32 | (scratch_idx << 5) | 31);
            // mrs scratch_idx, tpidrro_el0
            words.push(0xD53BD060u32 | scratch_idx);
            // ldr repl_idx, [scratch_idx, #pthread_key_offset]
            words
                .push(0xF9400000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx);
            // fmov scratch_idx, d31
            words.push(0x9E660000u32 | (31 << 5) | scratch_idx);
        }

        struct ExitBranch {
            branch_word_idx: usize,
            target_pc: usize,
            written_before: bool,
        }
        let mut exit_branches: Vec<ExitBranch> = Vec::new();
        let mut written_so_far = false;

        // 2. Sequence body: straight-line execution between ldxr and stxr
        for insn in &seq.insns {
            if insn.writes_x18 {
                written_so_far = true;
            }

            if insn.is_cond_branch {
                let branch_word_idx = words.len();
                // Placeholder word, will be fixed up with relative offset to exit block
                words.push(insn.word);
                exit_branches.push(ExitBranch {
                    branch_word_idx,
                    target_pc: insn.target_pc,
                    written_before: written_so_far,
                });
            } else {
                let modified = rename_checked(insn.word, repl_idx);
                words.push(modified);
            }
        }

        // 3. Normal completion exit (after stxr)
        if seq.needs_tls_store {
            // fmov d31, scratch_idx
            words.push(0x9E670000u32 | (scratch_idx << 5) | 31);
            // mrs scratch_idx, tpidrro_el0
            words.push(0xD53BD060u32 | scratch_idx);
            // str repl_idx, [scratch_idx, #pthread_key_offset]
            words
                .push(0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx);
            // fmov scratch_idx, d31
            words.push(0x9E660000u32 | (31 << 5) | scratch_idx);
        }
        // Restore caller's original repl_idx register from d30
        words.push(0x9E660000u32 | (30 << 5) | repl_idx); // fmov repl_idx, d30

        let end_pc = seq.insns.last().unwrap().pc;
        let normal_return_pc = end_pc + 4;
        let current_stub_pc = stub_addr + (words.len() * 4);
        let diff = (normal_return_pc as i64) - (current_stub_pc as i64);
        let imm26 = ((diff >> 2) as u32) & 0x03FF_FFFF;
        words.push(0x1400_0000u32 | imm26);

        // 4. Exit blocks for conditional branches
        for br in exit_branches {
            let exit_label_word_idx = words.len();
            let word_diff = (exit_label_word_idx as i32) - (br.branch_word_idx as i32);
            let cond = words[br.branch_word_idx] & 0x1F;
            let imm19 = (word_diff as u32) & 0x7FFFF;
            words[br.branch_word_idx] = 0x54000000u32 | (imm19 << 5) | cond;

            if br.written_before {
                // fmov d31, scratch_idx
                words.push(0x9E670000u32 | (scratch_idx << 5) | 31);
                // mrs scratch_idx, tpidrro_el0
                words.push(0xD53BD060u32 | scratch_idx);
                // str repl_idx, [scratch_idx, #pthread_key_offset]
                words.push(
                    0xF9000000u32 | ((imm_scaled & 0xFFF) << 10) | (scratch_idx << 5) | repl_idx,
                );
                // fmov scratch_idx, d31
                words.push(0x9E660000u32 | (31 << 5) | scratch_idx);
            }
            // Restore caller's original repl_idx register from d30
            words.push(0x9E660000u32 | (30 << 5) | repl_idx); // fmov repl_idx, d30

            let exit_stub_pc = stub_addr + (words.len() * 4);
            let return_diff = (br.target_pc as i64) - (exit_stub_pc as i64);
            let imm26 = ((return_diff >> 2) as u32) & 0x03FF_FFFF;
            words.push(0x1400_0000u32 | imm26);
        }

        for (i, &w) in words.iter().enumerate() {
            ptr::write(stub_ptr.add(i), w);
        }

        Ok(words.len())
    }

    /// Mark trampoline memory PROT_READ | PROT_EXEC and flush icache.
    pub fn finalize(&self) -> Result<(), &'static str> {
        // Only the used part becomes code (and goes into AOT images).
        let used = (self.next_offset.max(4) + 0x3fff) & !0x3fff;
        let res = unsafe { crate::cpu::aot::make_exec("x18.island", self.base_ptr as usize, used) };
        if res != 0 {
            return Err("Failed to protect trampoline island as executable");
        }

        crate::os::flush_icache(self.base_ptr as usize, self.next_offset);

        println!(
            "[+] Trampoline Island finalized: {} KB of executable stubs ready ({} bytes)",
            (self.next_offset + 1023) / 1024,
            self.next_offset
        );
        Ok(())
    }
}

/// Helper to parse GPR index (0..30) from register name "xN" or "wN".
fn parse_gpr_index(name: &str) -> Option<usize> {
    if name.starts_with('x') || name.starts_with('w') {
        name[1..].parse::<usize>().ok().filter(|&i| i <= 30)
    } else {
        None
    }
}

/// Precise AArch64 instruction bitmask decoder for register field offsets.
fn get_arm64_gpr_offsets(fw: u32) -> ([usize; 4], usize) {
    let mut offs = [0usize; 4];
    let mut n = 0;

    // Data Processing -- Immediate (bits 28:26 == 0b100)
    if (fw & 0x1C000000) == 0x10000000 {
        let op0 = (fw >> 23) & 0x7;
        if op0 == 0 || op0 == 1 {
            // PC-rel (ADR/ADRP)
            offs[n] = 0;
            n += 1;
        } else if op0 == 2 || op0 == 3 {
            // Add/sub imm
            offs[n] = 0;
            n += 1;
            offs[n] = 5;
            n += 1;
        } else if op0 == 4 {
            // Logical imm
            offs[n] = 0;
            n += 1;
            offs[n] = 5;
            n += 1;
        } else if op0 == 5 {
            // Move wide (MOVZ/MOVN/MOVK)
            offs[n] = 0;
            n += 1;
        } else if op0 == 6 {
            // Bitfield
            offs[n] = 0;
            n += 1;
            offs[n] = 5;
            n += 1;
        } else if op0 == 7 {
            // Extract
            offs[n] = 0;
            n += 1;
            offs[n] = 5;
            n += 1;
            offs[n] = 16;
            n += 1;
        }
        return (offs, n);
    }

    // Branches & System (bits 28:26 == 0b101)
    if (fw & 0x1C000000) == 0x14000000 {
        let op0 = (fw >> 29) & 0x7;
        let op1 = (fw >> 22) & 0x3F;
        if op0 == 1 || op0 == 5 {
            // CBZ/CBNZ
            offs[n] = 0;
            n += 1;
        } else if op0 == 3 || op0 == 7 {
            // TBZ/TBNZ
            offs[n] = 0;
            n += 1;
        } else if (op1 & 0x38) == 0x30 {
            // Branch reg (BR/BLR/RET)
            offs[n] = 5;
            n += 1;
        }
        return (offs, n);
    }

    // Load/store exclusive, load-acquire/store-release, CAS (bits 29:24 == 0b001000): Rs at 16, Rt2 at 10.
    if (fw & 0x3F000000) == 0x08000000 {
        offs[n] = 0;
        n += 1;
        offs[n] = 5;
        n += 1;
        offs[n] = 16;
        n += 1;
        offs[n] = 10;
        n += 1;
        return (offs, n);
    }
    // LSE atomic memory operations (LDADD / SWP / LDCLR ..., bits 29:24 == 0b111000, bit 21, bits 11:10 == 0): Rs at 16.
    if (fw & 0x3F200C00) == 0x38200000 {
        offs[n] = 0;
        n += 1;
        offs[n] = 5;
        n += 1;
        offs[n] = 16;
        n += 1;
        return (offs, n);
    }

    // Loads & Stores (bit 27 == 0b1, bit 25 == 0b0)
    if (fw & 0x0A000000) == 0x08000000 {
        let op2 = (fw >> 23) & 0x3;
        let is_simd = (fw & 0x04000000) != 0;

        if is_simd {
            offs[n] = 5;
            n += 1;
            if (fw & 0x00200C00) == 0x00200800 || (fw & 0x3E000000) == 0x0C000000 {
                offs[n] = 16;
                n += 1;
            }
            return (offs, n);
        }

        // Load/store pair (LDP/STP/LDPSW, every addressing form: bits 29:27 == 0b101; the old op2 == 2 test only
        // covered the signed-offset form, so a pre/post-indexed pair kept x18 as its second register).
        let _ = op2;
        if (fw & 0x38000000) == 0x28000000 {
            offs[n] = 0;
            n += 1;
            offs[n] = 10;
            n += 1;
            offs[n] = 5;
            n += 1;
        } else {
            // Single load/store
            offs[n] = 0;
            n += 1;
            offs[n] = 5;
            n += 1;
            if (fw & 0x00200C00) == 0x00200800 {
                // register offset
                offs[n] = 16;
                n += 1;
            }
        }
        return (offs, n);
    }

    // Data Processing -- Register (bit 27 == 0b1, bit 26 == 0b0, bit 25 == 0b1)
    if (fw & 0x0E000000) == 0x0A000000 {
        offs[n] = 0;
        n += 1;
        offs[n] = 5;
        n += 1;
        offs[n] = 16;
        n += 1;
        // 3-source (MADD / MSUB / SMADDL ...): bits 28:24 == 0b11011, Ra at 10. (The old test, mask 0x11000000, could
        // never match, so Ra stayed x18: the stub then read the real register.)
        if (fw & 0x1F000000) == 0x1B000000 {
            offs[n] = 10;
            n += 1;
        }
        return (offs, n);
    }

    (offs, n)
}

thread_local! {
    /// Instructions whose rename left an x18 operand (a field the decoder does not know); patch_text fails on them.
    static RENAME_MISSES: std::cell::RefCell<Vec<(u32, u32)>> = const { std::cell::RefCell::new(Vec::new()) };
}

thread_local! {
    static CS: Capstone = Capstone::new().arm64().mode(arch::arm64::ArchMode::Arm).build().expect("capstone");
}

/// rename_register_precise(fw, 18, to), checked: the result must not name x18 / w18 any more (capstone decides).
fn rename_checked(fw: u32, to: u32) -> u32 {
    let nw = rename_register_precise(fw, 18, to);
    CS.with(|cs| check_renamed(cs, fw, nw));
    nw
}

fn check_renamed(cs: &Capstone, fw: u32, nw: u32) {
    if let Ok(insns) = cs.disasm_count(&nw.to_le_bytes(), 0, 1) {
        if let Some(i) = insns.first() {
            let ops = i.op_str().unwrap_or("");
            if ops
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|t| t == "x18" || t == "w18")
            {
                RENAME_MISSES.with(|m| m.borrow_mut().push((fw, nw)));
            }
        }
    }
}

/// Precise register renamer that touches ONLY genuine GPR fields.
fn rename_register_precise(fw: u32, from: u32, to: u32) -> u32 {
    let (offsets, count) = get_arm64_gpr_offsets(fw);
    let mut new_w = fw;
    for i in 0..count {
        let shift = offsets[i];
        if ((new_w >> shift) & 0x1F) == from {
            new_w = (new_w & !(0x1F << shift)) | (to << shift);
        }
    }
    new_w
}

/// SP writeback categorization for x18 instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpWriteback {
    None,
    PreIndexPair {
        scale: i32,
        imm: i32,
        is_load: bool,
        is_64: bool,
        x18_is_rt2: bool,
        other_reg: u32,
    },
    PostIndexPair {
        scale: i32,
        imm: i32,
        is_load: bool,
        is_64: bool,
        x18_is_rt2: bool,
        other_reg: u32,
    },
    PreIndexSingle {
        scale: i32,
        size: usize,
        imm: i32,
        is_load: bool,
    },
    PostIndexSingle {
        scale: i32,
        size: usize,
        imm: i32,
        is_load: bool,
    },
}

/// Identifies whether an instruction modifies SP via pre-index or post-index writeback while referencing x18.
pub fn detect_sp_writeback(fw: u32) -> SpWriteback {
    let rn = (fw >> 5) & 0x1F;
    if rn != 31 {
        // Base register is not SP
        return SpWriteback::None;
    }

    // 1. Load/Store Register Pair (bits 29:27 == 0b101, bit 26 == 0 for GPR)
    if (fw & 0x3E000000) == 0x28000000 {
        let addr_mode = (fw >> 23) & 3;
        // addr_mode: 0b01 = Post-index, 0b11 = Pre-index
        if addr_mode == 0b01 || addr_mode == 0b11 {
            let opc = (fw >> 30) & 3;
            let is_64 = opc == 2;
            let scale = if is_64 { 8 } else { 4 };
            let imm7 = (fw >> 15) & 0x7F;
            let signed_imm7 = ((imm7 as i32) << 25) >> 25;
            let imm = signed_imm7 * scale;
            let is_load = (fw & 0x00400000) != 0;
            let rt = fw & 0x1F;
            let rt2 = (fw >> 10) & 0x1F;

            let touches_x18 = rt == 18 || rt2 == 18;
            let x18_is_rt2 = rt2 == 18;
            let other_reg = if x18_is_rt2 { rt } else { rt2 };

            if touches_x18 {
                if addr_mode == 0b11 {
                    return SpWriteback::PreIndexPair {
                        scale,
                        imm,
                        is_load,
                        is_64,
                        x18_is_rt2,
                        other_reg,
                    };
                } else {
                    return SpWriteback::PostIndexPair {
                        scale,
                        imm,
                        is_load,
                        is_64,
                        x18_is_rt2,
                        other_reg,
                    };
                }
            }
        }
    }

    // 2. Load/Store Register (immediate unscaled / pre/post-indexed)
    // Bits 29:24 == 0b111000 (0x38000000), bit 26 == 0 (GPR)
    if (fw & 0x3B000000) == 0x38000000 {
        let addr_mode = (fw >> 10) & 3;
        // addr_mode: 0b01 = Post-index, 0b11 = Pre-index
        if addr_mode == 0b01 || addr_mode == 0b11 {
            let rt = fw & 0x1F;
            if rt == 18 {
                let size = ((fw >> 30) & 3) as usize;
                let scale = 1 << size;
                let imm9 = (fw >> 12) & 0x1FF;
                let signed_imm9 = ((imm9 as i32) << 23) >> 23;
                let imm = signed_imm9;
                let is_load = (fw & 0x00400000) != 0;

                if addr_mode == 0b11 {
                    return SpWriteback::PreIndexSingle {
                        scale,
                        size,
                        imm,
                        is_load,
                    };
                } else {
                    return SpWriteback::PostIndexSingle {
                        scale,
                        size,
                        imm,
                        is_load,
                    };
                }
            }
        }
    }

    SpWriteback::None
}

/// Helper to adjust SP-relative immediate offset by +16 when SP has been decremented by 16.
/// Note: writeback instructions (pre/post-indexed) are handled separately by detect_sp_writeback.
fn adjust_sp_offset(fw: u32) -> u32 {
    // 1. LDR/STR unsigned immediate: bits 29:24 == 0b111001 (0x39000000)
    if (fw & 0x3B000000) == 0x39000000 {
        let size = (fw >> 30) & 3;
        let scale = 1 << size;
        let imm12 = (fw >> 10) & 0xFFF;
        let byte_offset = imm12 * scale;
        let new_byte_offset = byte_offset + 16;
        let new_imm12 = new_byte_offset / scale;
        if new_imm12 <= 0xFFF {
            return (fw & !(0xFFF << 10)) | ((new_imm12 & 0xFFF) << 10);
        } else {
            return fw;
        }
    }

    // 2. LDP/STP signed immediate: ONLY signed offset without writeback (addr_mode == 0b10)
    // Bits 29:27 == 0b101, bits 24:23 == 0b10 -> mask 0x3B800000 == 0x29000000
    if (fw & 0x3B800000) == 0x29000000 {
        let opc = (fw >> 30) & 3;
        let scale = if opc == 2 { 8 } else { 4 };
        let imm7 = (fw >> 15) & 0x7F;
        let signed_imm7 = ((imm7 as i32) << 25) >> 25;
        let byte_offset = signed_imm7 * scale;
        let new_byte_offset = byte_offset + 16;
        let new_imm7_val = new_byte_offset / scale;
        if new_imm7_val >= -64 && new_imm7_val <= 63 {
            let new_imm7 = (new_imm7_val as u32) & 0x7F;
            return (fw & !(0x7F << 15)) | (new_imm7 << 15);
        } else {
            return fw; // Signal overflow to caller
        }
    }

    // 3. LDUR/STUR unscaled signed offset without writeback (addr_mode == 0b00)
    // Bits 29:24 == 0b111000 (0x38000000), bits 11:10 == 0b00 -> mask 0x3B200C00 == 0x38000000
    if (fw & 0x3B200C00) == 0x38000000 {
        let imm9 = (fw >> 12) & 0x1FF;
        let signed_imm9 = ((imm9 as i32) << 23) >> 23;
        let new_imm9_val = signed_imm9 + 16;
        if new_imm9_val >= -256 && new_imm9_val <= 255 {
            let new_imm9 = (new_imm9_val as u32) & 0x1FF;
            return (fw & !(0x1FF << 12)) | (new_imm9 << 12);
        } else {
            return fw;
        }
    }

    // 4. ADD/SUB immediate: bits 28:24 == 0b10001 (0x11000000)
    if (fw & 0x1F000000) == 0x11000000 {
        let is_sub = (fw & 0x40000000) != 0;
        let shift = (fw >> 22) & 1;
        if shift == 0 {
            let imm12 = (fw >> 10) & 0xFFF;
            if is_sub {
                if imm12 >= 16 {
                    let new_imm12 = imm12 - 16;
                    return (fw & !(0xFFF << 10)) | ((new_imm12 & 0xFFF) << 10);
                }
            } else {
                let new_imm12 = imm12 + 16;
                if new_imm12 <= 0xFFF {
                    return (fw & !(0xFFF << 10)) | ((new_imm12 & 0xFFF) << 10);
                }
            }
        }
    }

    fw
}

/// The jump table a `br x18` at instruction `i` dispatches through: (BASE, number of possible offsets). Scans the
/// original instructions before it for `add x18, x18, xO, lsl #2`, the byte / halfword load into wO before that, and
/// `adr x18, BASE`. None for anything else.
unsafe fn jump_table_of(text: *const u32, i: usize) -> Option<(usize, usize)> {
    let (mut off_reg, mut width, mut base) = (None, None, None);
    for k in 1..=i.min(128) {
        let w = ptr::read(text.add(i - k));
        let pc = text.add(i - k) as usize;
        if off_reg.is_none() {
            if w & 0xFFE0_FFFF == 0x8B00_0A52 {
                off_reg = Some((w >> 16) & 31); // add x18, x18, xO, lsl #2
            }
            continue;
        }
        if width.is_none() && w & 31 == off_reg.unwrap() {
            if w & 0xFFE0_0C00 == 0x3860_0800 || w & 0xFFC0_0000 == 0x3940_0000 {
                width = Some(256); // ldrb wO, [xN, xM] / [xN, #imm]
            } else if w & 0xFFE0_0C00 == 0x7860_0800 || w & 0xFFC0_0000 == 0x7940_0000 {
                width = Some(65536); // ldrh
            }
        }
        if w & 0x9F00_001F == 0x1000_0012 {
            // adr x18, BASE
            let imm = ((((w >> 5) & 0x7FFFF) << 2) | ((w >> 29) & 3)) as i64;
            base = Some((pc as i64 + ((imm << 43) >> 43)) as usize);
            break;
        }
    }
    Some((base?, width?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_sp_writeback_pre_index_pair() {
        let cs = Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build()
            .unwrap();

        // stp x18, x19, [sp, #-16]!
        let stp_pre = 0xA9BF4FF2u32;
        let insns = cs.disasm_count(&stp_pre.to_le_bytes(), 0x1000, 1).unwrap();
        let insn = insns.first().unwrap();
        assert_eq!(insn.mnemonic().unwrap(), "stp");
        assert!(insn.op_str().unwrap().contains("x18"));
        assert!(insn.op_str().unwrap().contains("[sp, #-0x10]!"));

        let wb = detect_sp_writeback(stp_pre);
        assert_eq!(
            wb,
            SpWriteback::PreIndexPair {
                scale: 8,
                imm: -16,
                is_load: false,
                is_64: true,
                x18_is_rt2: false,
                other_reg: 19,
            }
        );
    }

    #[test]
    fn test_detect_sp_writeback_post_index_pair() {
        let cs = Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build()
            .unwrap();

        // ldp x18, x19, [sp], #16
        let ldp_post = 0xA8C14FF2u32;
        let insns = cs.disasm_count(&ldp_post.to_le_bytes(), 0x1000, 1).unwrap();
        let insn = insns.first().unwrap();
        assert_eq!(insn.mnemonic().unwrap(), "ldp");
        assert!(insn.op_str().unwrap().contains("[sp], #0x10"));

        let wb = detect_sp_writeback(ldp_post);
        assert_eq!(
            wb,
            SpWriteback::PostIndexPair {
                scale: 8,
                imm: 16,
                is_load: true,
                is_64: true,
                x18_is_rt2: false,
                other_reg: 19,
            }
        );
    }

    #[test]
    fn test_detect_sp_writeback_single() {
        // str x18, [sp, #-16]!
        let str_pre = 0xF81F0FF2u32;
        let wb = detect_sp_writeback(str_pre);
        assert_eq!(
            wb,
            SpWriteback::PreIndexSingle {
                scale: 8,
                size: 3,
                imm: -16,
                is_load: false,
            }
        );

        // ldr x18, [sp], #16
        let ldr_post = 0xF84107F2u32;
        let wb_post = detect_sp_writeback(ldr_post);
        assert_eq!(
            wb_post,
            SpWriteback::PostIndexSingle {
                scale: 8,
                size: 3,
                imm: 16,
                is_load: true,
            }
        );
    }

    #[test]
    fn test_non_writeback_sp_offset_adjust() {
        // stp x18, x19, [sp, #16] (signed offset, NO writeback)
        let stp_offset = 0xA9014FF2u32;
        assert_eq!(detect_sp_writeback(stp_offset), SpWriteback::None);

        // adjust_sp_offset should add 16 to the offset (16 -> 32)
        let adjusted = adjust_sp_offset(stp_offset);
        assert_ne!(adjusted, stp_offset);

        let cs = Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build()
            .unwrap();
        let insns = cs.disasm_count(&adjusted.to_le_bytes(), 0x1000, 1).unwrap();
        let insn = insns.first().unwrap();
        assert_eq!(insn.mnemonic().unwrap(), "stp");
        assert!(insn.op_str().unwrap().contains("[sp, #0x20]"));
    }

    #[test]
    fn test_pre_index_stub_disassembly() {
        // Test that emitting a stub for `stp x18, x19, [sp, #-16]!` generates valid instructions
        let mut pool = TrampolineIsland {
            base_ptr: 0x1000_0000 as *mut u32,
            capacity_bytes: 0x10000,
            next_offset: 0,
            pthread_key_offset: 0xc90,
        };
        let mut buffer = [0u32; 16];
        let original_word = 0xA9BF4FF2u32; // stp x18, x19, [sp, #-16]!
        let original_pc = 0x10e2cce50usize;
        let stub_addr = buffer.as_mut_ptr() as usize;

        let words_count = unsafe {
            pool.emit_stub(
                original_word,
                original_pc,
                stub_addr,
                false, // writes_x18
                true,  // touches_sp
                false, // is_cbz_cbnz
                false, // is_tbz_tbnz
                false, // is_adr_adrp
                0,
                false, // is_br
                false, // is_blr
                None,  // fp_simd_x18_shift
                9,     // repl_idx (x9)
                16,    // scratch_idx (x16)
                None,
            )
            .unwrap()
        };

        let cs = Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build()
            .unwrap();

        let mut disasm_lines = Vec::new();
        for i in 0..words_count {
            let chunk = buffer[i].to_le_bytes();
            let insns = cs.disasm_count(&chunk, (stub_addr + i * 4) as u64, 1).unwrap();
            let insn = insns.first().unwrap();
            disasm_lines.push(format!("{} {}", insn.mnemonic().unwrap(), insn.op_str().unwrap()));
        }

        // Verify the emitted sequence:
        // 0: stp x16, x9, [sp, #-0x20]!
        // 1: mrs x16, tpidrro_el0
        // 2: ldr x9, [x16, #0xc90]
        // 3: stp x9, x19, [sp, #0x10]
        // 4: ldp x16, x9, [sp], #0x10
        // 5: b original_pc + 4
        assert!(disasm_lines[0].starts_with("stp") && disasm_lines[0].contains("[sp, #-0x20]!"));
        assert!(disasm_lines[1].starts_with("mrs") && disasm_lines[1].contains("tpidrro_el0"));
        assert!(disasm_lines[2].starts_with("ldr") && disasm_lines[2].contains("0xc90"));
        assert!(disasm_lines[3].starts_with("stp") && disasm_lines[3].contains("[sp, #0x10]"));
        assert!(disasm_lines[4].starts_with("ldp") && disasm_lines[4].contains("[sp], #0x10"));
        assert!(disasm_lines[5].starts_with("b"));
    }

    #[test]
    fn test_post_index_stub_disassembly() {
        // Test that emitting a stub for `ldp x18, x19, [sp], #16` generates valid instructions
        let mut pool = TrampolineIsland {
            base_ptr: 0x1000_0000 as *mut u32,
            capacity_bytes: 0x10000,
            next_offset: 0,
            pthread_key_offset: 0xc90,
        };
        let mut buffer = [0u32; 16];
        let original_word = 0xA8C14FF2u32; // ldp x18, x19, [sp], #16
        let original_pc = 0x10e2cce50usize;
        let stub_addr = buffer.as_mut_ptr() as usize;

        let words_count = unsafe {
            pool.emit_stub(
                original_word,
                original_pc,
                stub_addr,
                true,  // writes_x18
                true,  // touches_sp
                false, // is_cbz_cbnz
                false, // is_tbz_tbnz
                false, // is_adr_adrp
                0,
                false, // is_br
                false, // is_blr
                None,  // fp_simd_x18_shift
                9,     // repl_idx (x9)
                16,    // scratch_idx (x16)
                None,
            )
            .unwrap()
        };

        let cs = Capstone::new()
            .arm64()
            .mode(capstone::arch::arm64::ArchMode::Arm)
            .detail(true)
            .build()
            .unwrap();

        let mut disasm_lines = Vec::new();
        for i in 0..words_count {
            let chunk = buffer[i].to_le_bytes();
            let insns = cs.disasm_count(&chunk, (stub_addr + i * 4) as u64, 1).unwrap();
            let insn = insns.first().unwrap();
            disasm_lines.push(format!("{} {}", insn.mnemonic().unwrap(), insn.op_str().unwrap()));
        }

        // Verify the emitted sequence:
        // 0: stp x16, x9, [sp, #-0x10]!
        // 1: ldp x9, x19, [sp, #0x10]
        // 2: mrs x16, tpidrro_el0
        // 3: str x9, [x16, #0xc90]
        // 4: ldp x16, x9, [sp], #0x10
        // 5: add sp, sp, #0x10
        // 6: b original_pc + 4
        assert!(disasm_lines[0].starts_with("stp") && disasm_lines[0].contains("[sp, #-0x10]!"));
        assert!(disasm_lines[1].starts_with("ldp") && disasm_lines[1].contains("[sp, #0x10]"));
        assert!(disasm_lines[2].starts_with("mrs") && disasm_lines[2].contains("tpidrro_el0"));
        assert!(disasm_lines[3].starts_with("str") && disasm_lines[3].contains("0xc90"));
        assert!(disasm_lines[4].starts_with("ldp") && disasm_lines[4].contains("[sp], #0x10"));
        assert!(disasm_lines[5].starts_with("add") && disasm_lines[5].contains("sp, sp, #0x10"));
        assert!(disasm_lines[6].starts_with("b"));
    }
}
