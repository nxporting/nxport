// A module's code -> C: function discovery, one C function per guest function, the address -> function table.
//
// Every address that is called, branched to from another function, named by a symbol or an .eh_frame entry, or
// stored as a code pointer becomes the start of a C function `f_<addr>`; a function runs to the next start. Branches
// inside a function are gotos, branches to another function tail calls, and anything known only at run time goes
// through rt_dispatch (runtime/cpu.h has the call / return protocol).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::bits::{bits, hx, sext};
use crate::insn::{lift, Insn};
use crate::jt;

/// A module image as it is mapped for the guest (code already patched: relocations, ExeFS patches, game hooks).
pub struct Module<'a> {
    pub name: String,
    /// Guest address of image offset 0.
    pub base: u64,
    pub image: &'a [u8],
    /// The executable range (guest addresses).
    pub text: std::ops::Range<u64>,
    /// Known function starts (symbols, .eh_frame, code pointers in data, the entry point, HLE-bound functions).
    pub seeds: BTreeSet<u64>,
}

/// The lifted module: C sources and what was not lifted.
pub struct Output {
    /// (file name, contents): the chunks of function bodies, then `<name>_table.c` with the function table.
    pub files: Vec<(String, String)>,
    pub functions: usize,
    pub instructions: usize,
    /// Instructions lifted to traps because their encoding is not handled, by reason.
    pub unsupported: BTreeMap<String, usize>,
    pub jump_tables: usize,
    /// Computed branches that are not a recognised table: their function is split into a function per block.
    pub unresolved_computed: usize,
}

/// Options for `lift_module`.
pub struct Options {
    /// Approximate instructions per C file (compile parallelism and memory).
    pub chunk_instructions: usize,
}

impl Default for Options {
    fn default() -> Self {
        Options { chunk_instructions: 40_000 }
    }
}

struct Lifted<'m, 'a> {
    m: &'m Module<'a>,
    insns: Vec<Insn>,
    unsupported: BTreeMap<String, usize>,
}

impl jt::Memory for Module<'_> {
    fn word(&self, a: u64) -> Option<u32> {
        if !self.text.contains(&a) || a % 4 != 0 {
            return None;
        }
        let o = (a - self.base) as usize;
        Some(u32::from_le_bytes(self.image.get(o..o + 4)?.try_into().ok()?))
    }
    fn bytes(&self, a: u64, len: usize) -> Option<&[u8]> {
        let o = a.checked_sub(self.base)? as usize;
        self.image.get(o..o.checked_add(len)?)
    }
}

impl<'m, 'a> Lifted<'m, 'a> {
    fn new(m: &'m Module<'a>) -> Self {
        let mut unsupported = BTreeMap::new();
        let mut insns = Vec::with_capacity(((m.text.end - m.text.start) / 4) as usize);
        let mut a = m.text.start;
        while a + 4 <= m.text.end {
            let w = jt::Memory::word(m, a).unwrap_or(0);
            insns.push(match lift(w, a) {
                Ok(i) => i,
                Err(e) => {
                    *unsupported.entry(e.0.clone()).or_insert(0) += 1;
                    Insn::Trap(format!("rt_udf(c, {}); /* {:08x}: {} */", hx(a), w, e.0))
                }
            });
            a += 4;
        }
        Lifted { m, insns, unsupported }
    }

    fn at(&self, a: u64) -> &Insn {
        &self.insns[((a - self.m.text.start) / 4) as usize]
    }

    fn in_text(&self, a: u64) -> bool {
        self.m.text.contains(&a) && a % 4 == 0
    }
}

/// How the `br`s of a function are emitted.
enum Table {
    /// Every target known: a switch over them (anything else goes through the dispatcher).
    Cases(Vec<u64>),
    /// Unresolved computed branch: through the dispatcher, to function starts at the targets that are known, at every
    /// instruction table entries can reach, and at every block start if some definition of the register is arithmetic
    /// the matcher cannot follow.
    All { known: Vec<u64>, reach: Vec<std::ops::RangeInclusive<u64>>, blocks: bool },
}

struct Scan {
    labels: BTreeSet<u64>,
    external: BTreeSet<u64>,
    tables: BTreeMap<u64, Table>,
}

fn scan(l: &Lifted, entry: u64, end: u64) -> Scan {
    let mut s = Scan { labels: BTreeSet::from([entry]), external: BTreeSet::new(), tables: BTreeMap::new() };
    let taken = taken_labels(l, entry, end);
    let mut preds: BTreeMap<u64, Vec<u64>> = BTreeMap::new();
    let mut a = entry;
    while a < end {
        if let Insn::B(t) | Insn::BCond(_, t) = l.at(a) {
            if (entry..end).contains(t) {
                preds.entry(*t).or_default().push(a);
            }
        }
        a += 4;
    }
    let mut a = entry;
    while a < end {
        match l.at(a) {
            Insn::B(t) | Insn::BCond(_, t) => {
                if (entry..end).contains(t) {
                    s.labels.insert(*t);
                } else if l.in_text(*t) {
                    s.external.insert(*t);
                }
            }
            Insn::Bl(t) if l.in_text(*t) => {
                s.external.insert(*t);
            }
            Insn::Br(_) => match branch_table(l, entry, &preds, &taken, a) {
                Some(Table::Cases(targets)) => {
                    for &t in &targets {
                        if (entry..end).contains(&t) {
                            s.labels.insert(t);
                        } else {
                            s.external.insert(t);
                        }
                    }
                    s.tables.insert(a, Table::Cases(targets));
                }
                Some(t) => {
                    s.tables.insert(a, t);
                }
                None => {}
            },
            _ => {}
        }
        a += 4;
    }
    // An unresolved branch's possible targets become function starts, which it reaches through the dispatcher (a
    // switch into the function would do too, but compilers take minutes over a large function entered at hundreds of
    // labels): every instruction table entries can reach, or, when even that is not known, every block start (case
    // bodies start after a block ends, unless a case falls into the next one). A target that is not a start stops as
    // code that was not lifted, naming it. The function is then split at those starts, so the targets of its switches
    // become starts too: a switch and its cases may end up in different pieces.
    let mut block_starts = false;
    for t in s.tables.values() {
        if let Table::All { known, reach, blocks } = t {
            s.external.extend(known);
            for r in reach {
                let (lo, hi) = ((*r.start()).max(entry), (*r.end()).min(end - 4));
                s.external.extend((lo.next_multiple_of(4)..=hi).step_by(4));
            }
            block_starts |= blocks;
        }
    }
    if s.tables.values().any(|t| matches!(t, Table::All { .. })) {
        for t in s.tables.values() {
            if let Table::Cases(targets) = t {
                s.external.extend(targets);
            }
        }
    }
    if block_starts {
        let mut a = entry;
        while a + 4 < end {
            if l.at(a).ends_block() {
                s.external.insert(a + 4);
            }
            a += 4;
        }
    }
    s
}

/// How the `br` at `br_pc` is emitted, from what each definition of its register that reaches it is: a recognised
/// table, a table whose entries are out of sight (`jt::reach`), an `adr` (a computed goto), arithmetic the matcher cannot
/// follow, or anything else (a function pointer: a plain dispatch). Compilers merge the `br`s of several switches into
/// one, so a definition may be in any block before it. None: a plain dispatch.
fn branch_table(
    l: &Lifted,
    entry: u64,
    preds: &BTreeMap<u64, Vec<u64>>,
    taken: &[u64],
    br_pc: u64,
) -> Option<Table> {
    let r = bits(jt::Memory::word(l.m, br_pc)?, 5, 5);
    let (mut known, mut reach, mut blocks) = (BTreeSet::new(), Vec::new(), false);
    let Some(defs) = reaching_defs(l, entry, preds, br_pc, r) else {
        // A definition is out of sight (before the function, or past a call): the instructions just before the `br`.
        return if let Some(targets) = jt::resolve(l.m, entry, br_pc, r) {
            Some(Table::Cases(targets))
        } else if let Some(range) = jt::reach(l.m, entry, br_pc, r) {
            Some(Table::All { known: Vec::new(), reach: vec![range], blocks: false })
        } else if computed(l, entry, br_pc) {
            Some(Table::All { known: Vec::new(), reach: Vec::new(), blocks: true })
        } else if !taken.is_empty() {
            Some(Table::Cases(taken.to_vec()))
        } else {
            None
        };
    };
    for d in defs {
        let w = jt::Memory::word(l.m, d).unwrap_or(0);
        if let Some(targets) = jt::resolve(l.m, entry, d + 4, r) {
            known.extend(targets);
        } else if w & 0x9f00_0000 == 0x1000_0000 {
            known.insert(d.wrapping_add(sext(((bits(w, 5, 19) << 2) | bits(w, 29, 2)) as u64, 21) as u64));
        } else if let Some(range) = jt::reach(l.m, entry, d + 4, r) {
            reach.push(range);
        } else if w & 0x1f00_0000 == 0x0b00_0000 {
            blocks = true; // add/sub (register)
        }
    }
    let known: Vec<u64> = known.into_iter().filter(|&t| l.in_text(t)).collect();
    if reach.is_empty() && !blocks {
        return (!known.is_empty()).then_some(Table::Cases(known));
    }
    Some(Table::All { known, reach, blocks })
}

/// The instructions in the function that may have set register `r` last before `pc`, following its branches back
/// (`preds`: the branches to each address); None if one is out of sight: a path reaches the function's start, or a
/// call that does not preserve `r`, or the search grows too large.
fn reaching_defs(l: &Lifted, entry: u64, preds: &BTreeMap<u64, Vec<u64>>, pc: u64, r: u32) -> Option<Vec<u64>> {
    let (mut defs, mut seen, mut work) = (BTreeSet::new(), BTreeSet::new(), Vec::new());
    // Control reached `a`: from the branches to it, and from the instruction before unless that ends a block.
    let arrive = |a: u64, work: &mut Vec<u64>| -> Option<()> {
        if a == entry {
            return None;
        }
        work.extend(preds.get(&a).into_iter().flatten());
        if !l.at(a - 4).ends_block() {
            work.push(a - 4);
        }
        Some(())
    };
    arrive(pc, &mut work)?;
    while let Some(a) = work.pop() {
        if !seen.insert(a) {
            continue;
        }
        if seen.len() > 512 {
            return None;
        }
        let w = jt::Memory::word(l.m, a)?;
        if jt::writes(w, r) {
            defs.insert(a);
        } else if jt::is_call(w) && !jt::callee_saved(r) {
            return None;
        } else {
            arrive(a, &mut work)?;
        }
    }
    Some(defs.into_iter().collect())
}

/// Whether the `br` register comes from address arithmetic close before it (a table the matcher did not recognise),
/// rather than from a load (a function pointer, which goes through the dispatcher).
fn computed(l: &Lifted, entry: u64, br_pc: u64) -> bool {
    let Some(w) = jt::Memory::word(l.m, br_pc) else { return false };
    let rn = (w >> 5) & 31;
    let mut a = br_pc;
    while a > entry && br_pc - a < 64 {
        a -= 4;
        if !matches!(l.at(a), Insn::Op(_)) {
            return false;
        }
        let w = jt::Memory::word(l.m, a).unwrap_or(0);
        if w & 31 == rn {
            return w & 0x1f00_0000 == 0x0b00_0000; // add/sub (register); an adr is a computed goto (taken_labels)
        }
    }
    false
}

/// The addresses inside the function that it takes with `adr`: the labels a computed goto (`br` of a register no table
/// explains) may go to.
fn taken_labels(l: &Lifted, entry: u64, end: u64) -> Vec<u64> {
    let mut taken = BTreeSet::new();
    let mut a = entry;
    while a < end {
        let w = jt::Memory::word(l.m, a).unwrap_or(0);
        if w & 0x9f00_0000 == 0x1000_0000 {
            let imm = sext(((bits(w, 5, 19) << 2) | bits(w, 29, 2)) as u64, 21);
            let t = a.wrapping_add(imm as u64);
            if (entry..end).contains(&t) && t % 4 == 0 {
                taken.insert(t);
            }
        }
        a += 4;
    }
    taken.into_iter().collect()
}

/// Function starts: the seeds and every `bl` target, then every target branched to from another function, until no
/// new start appears.
fn discover(l: &Lifted) -> BTreeSet<u64> {
    let mut starts: BTreeSet<u64> = l.m.seeds.iter().copied().filter(|&a| l.in_text(a)).collect();
    starts.insert(l.m.text.start);
    let mut a = l.m.text.start;
    while a < l.m.text.end {
        if let Insn::Bl(t) = l.at(a) {
            if l.in_text(*t) {
                starts.insert(*t);
            }
        }
        a += 4;
    }
    loop {
        let list: Vec<u64> = starts.iter().copied().collect();
        let mut new = BTreeSet::new();
        for (i, &entry) in list.iter().enumerate() {
            let end = list.get(i + 1).copied().unwrap_or(l.m.text.end);
            for t in scan(l, entry, end).external {
                if !starts.contains(&t) {
                    new.insert(t);
                }
            }
        }
        if new.is_empty() {
            return starts;
        }
        starts.extend(new);
    }
}

fn fname(a: u64) -> String {
    format!("f_{a:x}")
}

const TAIL_DISPATCH: &str = "__attribute__((musttail)) return rt_dispatch(c);";

fn emit_function(l: &Lifted, entry: u64, end: u64, out: &mut String, refs: &mut BTreeSet<u64>) {
    let s = scan(l, entry, end);
    let jump = |t: u64, refs: &mut BTreeSet<u64>| -> String {
        if (entry..end).contains(&t) {
            format!("goto L_{t:x};")
        } else if l.in_text(t) {
            refs.insert(t);
            format!("__attribute__((musttail)) return {}(c);", fname(t))
        } else {
            format!("c->pc = {}; {TAIL_DISPATCH}", hx(t))
        }
    };
    let _ = writeln!(out, "RT_FN void {}(Cpu *c) {{", fname(entry));
    let mut a = entry;
    let mut last_ends = false;
    while a < end {
        if s.labels.contains(&a) {
            let _ = writeln!(out, "L_{a:x}:;");
        }
        let ra = hx(a + 4);
        let line = match l.at(a) {
            Insn::Op(code) | Insn::Trap(code) => code.clone(),
            Insn::B(t) => jump(*t, refs),
            Insn::BCond(cond, t) => format!("if ({cond}) {{ {} }}", jump(*t, refs)),
            Insn::Bl(t) if l.in_text(*t) => {
                refs.insert(*t);
                format!("c->x[30] = {ra}; {}(c); RT_RETURNED(c, {ra})", fname(*t))
            }
            Insn::Bl(t) => format!("c->x[30] = {ra}; c->pc = {}; rt_dispatch(c); RT_RETURNED(c, {ra})", hx(*t)),
            Insn::Blr(target) => format!("c->pc = {target}; c->x[30] = {ra}; rt_dispatch(c); RT_RETURNED(c, {ra})"),
            Insn::Br(target) => match s.tables.get(&a) {
                Some(Table::Cases(targets)) => {
                    let mut cases = String::new();
                    for &t in targets {
                        let _ = write!(cases, "case {}: {} ", hx(t), jump(t, refs));
                    }
                    format!("{{ uint64_t t = {target}; switch (t) {{ {cases}default: c->pc = t; {TAIL_DISPATCH} }} }}")
                }
                Some(Table::All { .. }) | None => format!("c->pc = {target}; {TAIL_DISPATCH}"),
            },
            Insn::Ret(target) => format!("c->pc = {target}; return;"),
        };
        let _ = writeln!(out, "    {line}");
        last_ends = l.at(a).ends_block();
        a += 4;
    }
    if !last_ends {
        // falls through into the next function
        let line = if end < l.m.text.end { jump(end, refs) } else { format!("rt_udf(c, {});", hx(end)) };
        let _ = writeln!(out, "    {line}");
    }
    out.push_str("}\n\n");
}

/// Lifts a module to C. The files include runtime/cpu.h (`CPU_H`), which the caller writes next to them.
pub fn lift_module(m: &Module, opts: &Options) -> Output {
    let l = Lifted::new(m);
    let starts: Vec<u64> = discover(&l).into_iter().collect();
    let mut files = Vec::new();
    let mut body = String::new();
    let mut refs = BTreeSet::new();
    let mut defined = BTreeSet::new();
    let mut in_chunk = 0usize;
    let (mut jump_tables, mut unresolved_computed) = (0, 0);
    let flush = |body: &mut String, refs: &mut BTreeSet<u64>, defined: &mut BTreeSet<u64>, files: &mut Vec<_>| {
        let mut text = String::from("// Generated by nxport-lifter. Do not edit.\n#include \"cpu.h\"\n\n");
        for r in refs.iter().chain(defined.iter()) {
            let _ = writeln!(text, "RT_FN void {}(Cpu *c);", fname(*r));
        }
        text.push('\n');
        text.push_str(body);
        files.push((format!("{}_{:03}.c", m.name, files.len()), text));
        body.clear();
        refs.clear();
        defined.clear();
    };
    for (i, &entry) in starts.iter().enumerate() {
        let end = starts.get(i + 1).copied().unwrap_or(m.text.end);
        let s = scan(&l, entry, end);
        jump_tables += s.tables.values().filter(|t| matches!(t, Table::Cases(_))).count();
        unresolved_computed += s.tables.values().filter(|t| matches!(t, Table::All { .. })).count();
        emit_function(&l, entry, end, &mut body, &mut refs);
        defined.insert(entry);
        in_chunk += ((end - entry) / 4) as usize;
        if in_chunk >= opts.chunk_instructions {
            flush(&mut body, &mut refs, &mut defined, &mut files);
            in_chunk = 0;
        }
    }
    if !body.is_empty() {
        flush(&mut body, &mut refs, &mut defined, &mut files);
    }
    let mut table = String::from("// Generated by nxport-lifter. Do not edit.\n#include \"cpu.h\"\n\n");
    for &a in &starts {
        let _ = writeln!(table, "RT_FN void {}(Cpu *c);", fname(a));
    }
    let _ = writeln!(table, "\nRT_EXPORT const uint64_t {}_fn_count = {};", m.name, starts.len());
    let _ = writeln!(table, "RT_EXPORT const uint64_t {}_fn_addrs[] = {{", m.name);
    for &a in &starts {
        let _ = writeln!(table, "    {},", hx(a));
    }
    let _ = writeln!(table, "}};\nRT_EXPORT const GuestFn {}_fn_ptrs[] = {{", m.name);
    for &a in &starts {
        let _ = writeln!(table, "    {},", fname(a));
    }
    table.push_str("};\n");
    files.push((format!("{}_table.c", m.name), table));
    Output {
        files,
        functions: starts.len(),
        instructions: l.insns.len(),
        unsupported: l.unsupported,
        jump_tables,
        unresolved_computed,
    }
}

/// A small hand-assembled module for tests: main calls an iterative factorial and a function that dispatches through
/// a byte jump table; main returns fact(5) + case 2 = 150.
#[doc(hidden)]
pub fn sample_image() -> (u64, Vec<u8>, std::ops::Range<u64>) {
    const BASE: u64 = 0x10_0000;
    let mut img = vec![0u8; 0x400];
    let mut put = |addr: u64, words: &[u32]| {
        for (i, w) in words.iter().enumerate() {
            let o = (addr - BASE) as usize + i * 4;
            img[o..o + 4].copy_from_slice(&w.to_le_bytes());
        }
    };
    // 0x100000 main: stp x29, x30, [sp, #-16]!; mov x29, sp; mov x0, #5; bl fact; mov x19, x0; mov w0, #2;
    //               bl switchy; add x0, x0, x19; ldp x29, x30, [sp], #16; ret
    put(0x10_0000, &[
        0xa9bf7bfd, 0x910003fd, 0xd28000a0, 0x9400003d, 0xaa0003f3, 0x52800040, 0x9400007a, 0x8b130000, 0xa8c17bfd,
        0xd65f03c0,
    ]);
    // 0x100100 fact: mov x1, #1; loop: cbz x0, done; mul x1, x1, x0; sub x0, x0, #1; b loop; done: mov x0, x1; ret
    put(0x10_0100, &[0xd2800021, 0xb4000080, 0x9b007c21, 0xd1000400, 0x17fffffd, 0xaa0103e0, 0xd65f03c0]);
    // 0x100200 switchy: cmp w0, #3; b.hi default; adrp x9, .; add x9, x9, #0x300; ldrb w10, [x9, x0];
    //                   adr x11, cases; add x11, x11, x10, lsl #2; br x11
    put(0x10_0200, &[0x71000c1f, 0x54000168, 0x90000009, 0x910c0129, 0x3860692a, 0x1000006b, 0x8b0a096b, 0xd61f0160]);
    // cases: 0x100220 x0 = 10, 0x100228 x0 = 20, 0x100230 (default) x0 = 99, 0x100238 x0 = 30; each then ret
    put(0x10_0220, &[0xd2800140, 0xd65f03c0, 0xd2800280, 0xd65f03c0, 0xd2800c60, 0xd65f03c0, 0xd28003c0, 0xd65f03c0]);
    img[0x300..0x304].copy_from_slice(&[0, 2, 6, 4]);
    (BASE, img, BASE..BASE + 0x300)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_module() {
        let (base, img, text) = sample_image();
        let m = Module { name: "t".into(), base, image: &img, text, seeds: BTreeSet::from([base]) };
        let out = lift_module(&m, &Options::default());
        assert_eq!(out.functions, 3); // main, fact, switchy
        assert_eq!(out.jump_tables, 1);
        assert_eq!(out.unresolved_computed, 0);
        let body = &out.files[0].1;
        assert!(body.contains("c->x[30] = 0x100010ull; f_100100(c); RT_RETURNED(c, 0x100010ull)"));
        assert!(body.contains("case 0x100238ull: goto L_100238;"));
        assert!(body.contains("goto L_100104;")); // the loop's back edge
    }

    #[test]
    fn computed_goto() {
        // 0x1000: adr x19, 0x1010; cbz x0, 0x100c; adr x19, 0x1014; br x19; 0x1010: ret; 0x1014: ret
        let mut img = Vec::new();
        for w in [0x10000093u32, 0xb4000040, 0x10000073, 0xd61f0260, 0xd65f03c0, 0xd65f03c0] {
            img.extend_from_slice(&w.to_le_bytes());
        }
        let m = Module { name: "t".into(), base: 0x1000, image: &img, text: 0x1000..0x1018, seeds: BTreeSet::from([0x1000]) };
        let out = lift_module(&m, &Options::default());
        assert_eq!((out.functions, out.unresolved_computed), (1, 0));
        let body = &out.files[0].1;
        assert!(body.contains("case 0x1010ull: goto L_1010; case 0x1014ull: goto L_1014;"));
        assert!(!body.contains("L_1004:")); // no label at every instruction
    }

    #[test]
    fn unresolved_table() {
        // 0x1000: adr x9, 0x1010; add x9, x9, x0, lsl #2; br x9; 0x100c, 0x1010, 0x1014: ret
        let mut img = Vec::new();
        for w in [0x10000089u32, 0x8b000929, 0xd61f0120, 0xd65f03c0, 0xd65f03c0, 0xd65f03c0] {
            img.extend_from_slice(&w.to_le_bytes());
        }
        let m = Module { name: "t".into(), base: 0x1000, image: &img, text: 0x1000..0x1018, seeds: BTreeSet::from([0x1000]) };
        let out = lift_module(&m, &Options::default());
        assert_eq!((out.functions, out.unresolved_computed), (4, 1)); // a function per block
        let body = &out.files[0].1;
        assert!(body.contains("RT_FN void f_1010(Cpu *c) {") && !body.contains("switch"));
    }

    #[test]
    fn table_out_of_sight() {
        // The table's address is an argument: 0x1000: adr x9, 0x1010; ldrb w10, [x1, x0]; add x9, x9, x10, lsl #2;
        // br x9; 0x1010: mov x0, #1 (falls into the next case); 0x1014: mov x0, #2; ret
        let mut img = Vec::new();
        for w in [0x10000089u32, 0x3860682a, 0x8b0a0929, 0xd61f0120, 0xd2800020, 0xd2800040, 0xd65f03c0] {
            img.extend_from_slice(&w.to_le_bytes());
        }
        let m = Module { name: "t".into(), base: 0x1000, image: &img, text: 0x1000..0x101c, seeds: BTreeSet::from([0x1000]) };
        let out = lift_module(&m, &Options::default());
        assert_eq!((out.functions, out.unresolved_computed), (4, 1)); // a start at each instruction a byte reaches
        assert!(out.files[0].1.contains("RT_FN void f_1014(Cpu *c) {"));
    }

    #[test]
    fn merged_branch() {
        // Two paths set x9 and share one `br`: 0x1000: cbz x1, 0x1010; adr x9, 0x1020; b 0x1018; adr x10, 0x101c;
        // 0x1010: adr x9, 0x1024; mov x2, #0; 0x1018: br x9; ret; 0x1020: ret; 0x1024: ret
        let mut img = Vec::new();
        for w in [
            0xb4000081u32, 0x100000e9, 0x14000004, 0x1000008a, 0x100000a9, 0xd2800002, 0xd61f0120, 0xd65f03c0, 0xd65f03c0,
            0xd65f03c0,
        ] {
            img.extend_from_slice(&w.to_le_bytes());
        }
        let m = Module { name: "t".into(), base: 0x1000, image: &img, text: 0x1000..0x1028, seeds: BTreeSet::from([0x1000]) };
        let out = lift_module(&m, &Options::default());
        assert_eq!((out.functions, out.unresolved_computed), (1, 0));
        let body = &out.files[0].1;
        // the cases are what reaches the `br`, not every address the function takes (0x101c)
        assert!(body.contains("switch (t) { case 0x1020ull: goto L_1020; case 0x1024ull: goto L_1024; default:"));
    }
}
