// A module's code -> C: function discovery, one C function per guest function, the address -> function table.
//
// Every address that is called, branched to from another function, named by a symbol or an .eh_frame entry, or
// stored as a code pointer becomes the start of a C function `f_<addr>`; a function runs to the next start. Branches
// inside a function are gotos, branches to another function tail calls, and anything known only at run time goes
// through rt_dispatch (runtime/cpu.h has the call / return protocol).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use crate::bits::hx;
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
    /// Computed branches that are not a recognised table: their function gets a label at every instruction.
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
    Cases(Vec<u64>),
    /// Unresolved computed branch: any instruction of the function may be the target.
    All,
}

struct Scan {
    labels: BTreeSet<u64>,
    external: BTreeSet<u64>,
    tables: BTreeMap<u64, Table>,
}

fn scan(l: &Lifted, entry: u64, end: u64) -> Scan {
    let mut s = Scan { labels: BTreeSet::from([entry]), external: BTreeSet::new(), tables: BTreeMap::new() };
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
            Insn::Br(_) => {
                if let Some(targets) = jt::resolve(l.m, entry, a) {
                    for &t in &targets {
                        if (entry..end).contains(&t) {
                            s.labels.insert(t);
                        } else {
                            s.external.insert(t);
                        }
                    }
                    s.tables.insert(a, Table::Cases(targets));
                } else if computed(l, entry, a) {
                    s.tables.insert(a, Table::All);
                }
            }
            _ => {}
        }
        a += 4;
    }
    if let Some(Table::All) = s.tables.values().find(|t| matches!(t, Table::All)) {
        s.labels.extend((entry..end).step_by(4));
    }
    s
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
            return w & 0x1f00_0000 == 0x0b00_0000 || w & 0x9f00_0000 == 0x1000_0000; // add/sub (register), adr
        }
    }
    false
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
    let all = s.tables.values().any(|t| matches!(t, Table::All));
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
                Some(Table::All) => format!("c->pc = {target}; goto L_dispatch;"),
                None => format!("c->pc = {target}; {TAIL_DISPATCH}"),
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
    if all {
        let mut cases = String::new();
        let mut t = entry;
        while t < end {
            let _ = write!(cases, "case {}: goto L_{t:x}; ", hx(t));
            t += 4;
        }
        let _ = writeln!(out, "L_dispatch:\n    switch (c->pc) {{ {cases}default: break; }}\n    {TAIL_DISPATCH}");
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
        unresolved_computed += s.tables.values().filter(|t| matches!(t, Table::All)).count();
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
}
