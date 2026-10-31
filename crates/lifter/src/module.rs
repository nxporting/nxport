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

/// Leaves the function: the registers it keeps in locals go to the Cpu first (a `SPILL` marker: keep_registers).
/// Each dispatch probes the dispatcher's cache first (cpu.h, rt_hit), so the branch to a cached function is at this site.
const TAIL_DISPATCH: &str =
    "SPILL { GuestFn rt_f = rt_hit(c->pc); if (rt_f) { __attribute__((musttail)) return rt_f(c); } } __attribute__((musttail)) return rt_dispatch(c);";

/// One instruction's C. `next`: execution can go on at the next line.
struct Line {
    label: Option<u64>,
    code: String,
    next: bool,
}

/// The flag-setting helpers (runtime/cpu.h) and their variants that leave NZCV alone.
const FLAG_HELPERS: [(&str, &str); 6] = [
    ("subs32(c, ", "sub32_nf("),
    ("subs64(c, ", "sub64_nf("),
    ("adds32(c, ", "add32_nf("),
    ("adds64(c, ", "add64_nf("),
    ("logic32(c, ", "logic32_nf("),
    ("logic64(c, ", "logic64_nf("),
];

/// Whether a line's C reads NZCV: a condition, a carry-in (adc, sbc), or the register itself (mrs). Leaving the function
/// through a tail jump or the dispatcher counts as a read: the lifter splits functions at call targets, and code can
/// branch into the next one with flags set.
fn reads_flags(code: &str) -> bool {
    code.contains("COND_")
        || code.contains("FLAG_")
        || code.contains("adc")
        || code.contains("sbc")
        || code.contains("musttail")
        || code.match_indices("c->nzcv").any(|(i, _)| !code[i + 7..].starts_with(" ="))
}

/// Whether a line sets NZCV on every path: a flag-setting helper or an assignment, and calls (AAPCS64: the callee
/// leaves the flags undefined, and no caller reads them after a call).
fn writes_flags(code: &str) -> bool {
    FLAG_HELPERS.iter().any(|(h, _)| code.contains(h)) || code.contains("c->nzcv =") || code.contains("RT_RETURNED")
}

/// The arguments of the call whose argument list starts `s` ("(a, b)..." -> ["a", "b"]), split at top-level commas,
/// and the text after its closing parenthesis.
fn call_args(s: &str) -> Option<(Vec<&str>, &str)> {
    let rest = s.strip_prefix('(')?;
    let (mut depth, mut start, mut args) = (0, 0, Vec::new());
    for (i, ch) in rest.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' if depth == 0 => {
                args.push(rest[start..i].trim());
                return Some((args, &rest[i + 1..]));
            }
            ')' => depth -= 1,
            ',' if depth == 0 => {
                args.push(rest[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    None
}

/// The C condition `cond` (an A64 condition name) reads from the flags of `cmp a, b` (`sub`) or `tst r` (logic) of
/// `bits` width, computed from the operands; None for the overflow conditions of a compare, left to the flags.
fn fused_cond(cond: &str, sub: bool, bits: u32, args: &[&str]) -> Option<String> {
    let (u, s) = (format!("uint{bits}_t"), format!("int{bits}_t"));
    if sub {
        let (a, b) = (format!("(({u})({}))", args[0]), format!("(({u})({}))", args[1]));
        let (sa, sb) = (format!("(({s}){a})"), format!("(({s}){b})"));
        Some(match cond {
            "EQ" => format!("{a} == {b}"),
            "NE" => format!("{a} != {b}"),
            "CS" => format!("{a} >= {b}"),
            "CC" => format!("{a} < {b}"),
            "MI" => format!("({s})({u})({a} - {b}) < 0"),
            "PL" => format!("({s})({u})({a} - {b}) >= 0"),
            "HI" => format!("{a} > {b}"),
            "LS" => format!("{a} <= {b}"),
            "GE" => format!("{sa} >= {sb}"),
            "LT" => format!("{sa} < {sb}"),
            "GT" => format!("{sa} > {sb}"),
            "LE" => format!("{sa} <= {sb}"),
            _ => return None,
        })
    } else {
        // N and Z from the result, C and V clear.
        let r = format!("(({s})({u})({}))", args[0]);
        Some(match cond {
            "EQ" => format!("{r} == 0"),
            "NE" => format!("{r} != 0"),
            "MI" | "LT" => format!("{r} < 0"),
            "PL" | "GE" => format!("{r} >= 0"),
            "GT" => format!("{r} > 0"),
            "LE" => format!("{r} <= 0"),
            "CS" | "VS" | "HI" => "0".into(),
            "CC" | "VC" | "LS" => "1".into(),
            _ => return None,
        })
    }
}

/// The next line's code with its one flag condition replaced by the condition over the operands of the compare or test
/// in `code`, when that is all that reads the flags `code` sets.
fn fuse(code: &str, next: &str) -> Option<String> {
    let code = code.trim();
    // FCMP: the condition is cc_s / cc_d over the operands (unordered as FCMP's flags give it).
    if let Some((f, rest)) = ["s", "d"].iter().find_map(|f| code.strip_prefix(&format!("c->nzcv = cmp_{f}(c, ")).map(|r| (*f, r))) {
        let call = format!("({rest}");
        let (args, tail) = call_args(&call)?;
        let uses: Vec<_> = next.match_indices("COND_").collect();
        if tail != ";" || args.len() != 2 || uses.len() != 1 || next.contains("FLAG_") || next.contains("c->nzcv") {
            return None;
        }
        let j = uses[0].0;
        let name = next.get(j + 5..j + 7)?;
        if !next[j + 7..].starts_with("(c)") || name == "AL" || name == "NV" {
            return None;
        }
        return Some(format!("{}cc_{f}(c, {}, {}, FCC_{name}){}", &next[..j], args[0], args[1], &next[j + 10..]));
    }
    let forms = [(true, 32, "(void)(subs32(c, "), (true, 64, "(void)(subs64(c, "), (false, 32, "(void)(logic32(c, "), (false, 64, "(void)(logic64(c, ")];
    let (sub, bits, f) = forms.into_iter().find(|(_, _, f)| code.starts_with(f))?;
    // "(void)(subs32(c, A, B));": the arguments after `c`.
    let (args, tail) = call_args(&format!("({}", &code[f.len()..]))
        .map(|(a, t)| (a.into_iter().map(str::to_string).collect::<Vec<_>>(), t.to_string()))?;
    if tail != ");" || args.len() != if sub { 2 } else { 1 } {
        return None;
    }
    let uses: Vec<_> = next.match_indices("COND_").collect();
    if uses.len() != 1 || next.contains("FLAG_") || next.contains("adc") || next.contains("sbc") || next.contains("c->nzcv") {
        return None;
    }
    let j = uses[0].0;
    let name = next.get(j + 5..j + 7)?;
    if !next[j + 7..].starts_with("(c)") {
        return None;
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let cond = fused_cond(name, sub, bits, &args)?;
    Some(format!("{}({cond}){}", &next[..j], &next[j + 10..]))
}

/// NZCV is computed only where something reads it: flag-setting instructions whose flags are never read compute just
/// their result, and a compare or test whose flags only the next instruction's condition reads becomes that condition
/// over its operands (cmp + b.lt: one x86 compare and branch). Flags are dead across calls and returns (AAPCS64).
fn optimize_flags(lines: &mut [Line]) {
    let n = lines.len();
    let at: BTreeMap<u64, usize> = lines.iter().enumerate().filter_map(|(i, l)| l.label.map(|a| (a, i))).collect();
    let mut succ: Vec<Vec<usize>> = Vec::with_capacity(n);
    for (i, l) in lines.iter().enumerate() {
        let mut s = Vec::new();
        let mut rest = l.code.as_str();
        while let Some(j) = rest.find("goto L_") {
            let hex: String = rest[j + 7..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if let Some(&t) = u64::from_str_radix(&hex, 16).ok().and_then(|a| at.get(&a)) {
                s.push(t);
            }
            rest = &rest[j + 7..];
        }
        if l.next && i + 1 < n {
            s.push(i + 1);
        }
        succ.push(s);
    }
    let reads: Vec<bool> = lines.iter().map(|l| reads_flags(&l.code)).collect();
    let writes: Vec<bool> = lines.iter().map(|l| writes_flags(&l.code)).collect();
    // Flags live after each line (backward, any path).
    let mut live_out = vec![false; n];
    let mut live_in = vec![false; n];
    let mut changed = true;
    while changed {
        changed = false;
        for i in (0..n).rev() {
            let out = succ[i].iter().any(|&t| live_in[t]);
            let inn = reads[i] || (out && !writes[i]);
            if out != live_out[i] || inn != live_in[i] {
                live_out[i] = out;
                live_in[i] = inn;
                changed = true;
            }
        }
    }
    for i in 0..n {
        if !writes[i] {
            continue;
        }
        // Read by the next line alone, and dead after it.
        let next_ok = i + 1 < n && lines[i].next && lines[i + 1].label.is_none() && !live_out[i + 1] && !writes[i + 1];
        if next_ok {
            if let Some(next) = fuse(&lines[i].code, &lines[i + 1].code) {
                lines[i].code = String::new();
                lines[i + 1].code = next;
                continue;
            }
        }
        // Flags never read: the result alone.
        if !live_out[i] && !reads[i] {
            // An FCMP or flag constant has nothing else to compute.
            let code = lines[i].code.trim();
            if (code.starts_with("c->nzcv = cmp_s(c, ") || code.starts_with("c->nzcv = cmp_d(c, ") || code.starts_with("c->nzcv = 0x"))
                && code.ends_with(';')
                && code.matches(';').count() == 1
            {
                lines[i].code = String::new();
                continue;
            }
            for (h, nf) in FLAG_HELPERS {
                if lines[i].code.contains(h) {
                    lines[i].code = lines[i].code.replace(h, nf);
                }
            }
        }
    }
}

/// Guest integer registers x0..x30 (bits 0..30) and sp (bit 31).
type Regs = u32;

/// Registers a function keeps in locals: its most referenced ones. More cost more than they save (clang spills them
/// to the stack, and every call stores and loads them); valentderah's TotK recompilation measured the fewest
/// instructions and memory operands at 4 to 6.
const KEPT: usize = 6;

/// x19..x29 and sp: a callee preserves them (AAPCS64), so they are not loaded again after a call.
const CALLEE_SAVED: Regs = (0x7ff << 19) | (1 << 31);

/// The guest register a "c->x[N]" or "c->sp" at the start of `s` names, and its length.
fn reg_at(s: &str) -> Option<(u32, usize)> {
    let after = s.strip_prefix("c->")?;
    if let Some(num) = after.strip_prefix("x[") {
        let digits = num.bytes().take_while(u8::is_ascii_digit).count();
        if digits > 0 && num.as_bytes().get(digits) == Some(&b']') {
            return Some((num[..digits].parse().ok()?, 3 + 2 + digits + 1));
        }
    } else if let Some(tail) = after.strip_prefix("sp") {
        if !tail.starts_with(|ch: char| ch.is_ascii_alphanumeric() || ch == '_') {
            return Some((31, 5));
        }
    }
    None
}

/// `code` with the registers in `kept` renamed to their locals (gx0..gx30, gsp).
fn localize(code: &str, kept: Regs) -> String {
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(i) = rest.find("c->") {
        out.push_str(&rest[..i]);
        match reg_at(&rest[i..]) {
            Some((r, len)) if kept & (1 << r) != 0 => {
                if r == 31 {
                    out.push_str("gsp");
                } else {
                    let _ = write!(out, "gx{r}");
                }
                rest = &rest[i + len..];
            }
            _ => {
                out.push_str("c->");
                rest = &rest[i + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The locals C code reads and writes (any use counts as a read; `r = ...` and `r op= ...` as writes).
fn uses(code: &str) -> (Regs, Regs) {
    let (mut reads, mut writes) = (0, 0);
    let b = code.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let ident = |j: usize| j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_');
        if b[i] == b'g' && (i == 0 || !ident(i - 1)) {
            let (r, len) = if code[i..].starts_with("gsp") {
                (Some(31), 3)
            } else if code[i..].starts_with("gx") {
                let d = b[i + 2..].iter().take_while(|c| c.is_ascii_digit()).count();
                (if d > 0 { code[i + 2..i + 2 + d].parse::<u32>().ok() } else { None }, 2 + d)
            } else {
                (None, 1)
            };
            if let Some(r) = r.filter(|_| !ident(i + len)) {
                let rest = code[i + len..].trim_start();
                let compound = ["+=", "-=", "|=", "&=", "^=", "<<=", ">>=", "*="].iter().any(|op| rest.starts_with(op));
                if compound {
                    reads |= 1 << r;
                    writes |= 1 << r;
                } else if rest.starts_with('=') && !rest.starts_with("==") {
                    writes |= 1 << r;
                } else {
                    reads |= 1 << r;
                }
            }
            i += len;
            continue;
        }
        i += 1;
    }
    (reads, writes)
}

/// The vector-register macros of runtime/simd.h, as the preprocessor would expand them with literal register numbers
/// ($1.. for the arguments): spelled out, their registers appear as c->v[N] and can be kept in locals (keep_vregs).
const VMACROS: [(&str, &str); 14] = [
    ("VREG", "(*($1 *)&c->v[$2])"),
    ("VQ", "(($1)(($2) ? c->v[$3] : (u128)(uint64_t)c->v[$3]))"),
    ("VW", "do { $1 _w = ($4); c->v[$3] = ($2) ? (u128)_w : (u128)(uint64_t)(u128)_w; } while (0)"),
    ("VS8", "(*(uint8_t *)&c->v[$1])"),
    ("VS16", "(*(sa16 *)&c->v[$1])"),
    ("VS32", "(*(sa32 *)&c->v[$1])"),
    ("VS64", "(*(sa64 *)&c->v[$1])"),
    ("VSF32", "(*(saf32 *)&c->v[$1])"),
    ("VSF64", "(*(saf64 *)&c->v[$1])"),
    ("VHI64", "(((sa64 *)&c->v[$1])[1])"),
    ("FP_SETS", "(c->v[$1] = fb_s($2))"),
    ("FP_SETD", "(c->v[$1] = fb_d($2))"),
    ("SV_FOP_S", "VW(vf32, $2, $3, vop_s(c, $1, $4, $5, $6))"),
    ("SV_FOP_D", "VW(vf64, $2, $3, vop_d(c, $1, $4, $5, $6))"),
];

/// Helpers of runtime/simd.h that take vector registers by number and read or write c->v themselves: the registers a
/// function keeps in locals go to the Cpu before a call to one and are loaded again after it.
const V_HELPERS: [&str; 19] = [
    "fp_fma_d", "fp_fma_s", "sl_across", "sl_addlp", "sl_gen", "sl_ldm", "sl_ldr", "sl_lds", "sl_pmull64", "sl_stm",
    "sl_sts", "sv_aes", "sv_fcvtl", "sv_fcvtn", "sv_fred_s", "sv_sha1hash", "sv_sha256hash", "sv_sha_misc", "sv_tbl",
];

/// `code` with VMACROS uses expanded (arguments first).
fn expand_vmacros(code: &str) -> String {
    let b = code.as_bytes();
    let mut out = String::with_capacity(code.len() + code.len() / 2);
    let mut i = 0;
    'scan: while i < b.len() {
        let boundary = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if boundary && b[i].is_ascii_uppercase() {
            for (name, body) in VMACROS {
                if code[i..].starts_with(name) && code[i + name.len()..].starts_with('(') {
                    if let Some((args, rest)) = call_args(&code[i + name.len()..]) {
                        let mut e = body.to_string();
                        for (k, a) in args.iter().enumerate().rev() {
                            e = e.replace(&format!("${}", k + 1), &expand_vmacros(a));
                        }
                        out.push_str(&expand_vmacros(&e));
                        i = code.len() - rest.len();
                        continue 'scan;
                    }
                }
            }
        }
        let ch = code[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// The vector register a "c->v[N]" at the start of `s` names, and its length.
fn vreg_at(s: &str) -> Option<(u32, usize)> {
    let num = s.strip_prefix("c->v[")?;
    let digits = num.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 || num.as_bytes().get(digits) != Some(&b']') {
        return None;
    }
    Some((num[..digits].parse().ok()?, 5 + digits + 1))
}

/// The vector registers `code` names as c->v[N].
fn vregs_named(code: &str) -> Regs {
    let mut set = 0;
    let mut rest = code;
    while let Some(i) = rest.find("c->v[") {
        if let Some((r, _)) = vreg_at(&rest[i..]) {
            set |= 1 << r;
        }
        rest = &rest[i + 5..];
    }
    set
}

/// The vector-register locals C code reads and writes: `gvN = ...` writes, a pointer to one (a lane or view of it)
/// reads and writes, any other use reads.
fn vuses(code: &str) -> (Regs, Regs) {
    let (mut reads, mut writes) = (0, 0);
    let b = code.as_bytes();
    let mut i = 0;
    while let Some(j) = code[i..].find("gv").map(|j| i + j) {
        let ident = |k: usize| k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_');
        let d = b[j + 2..].iter().take_while(|c| c.is_ascii_digit()).count();
        if (j > 0 && ident(j - 1)) || d == 0 || ident(j + 2 + d) {
            i = j + 2;
            continue;
        }
        let r: u32 = code[j + 2..j + 2 + d].parse().unwrap();
        let rest = code[j + 2 + d..].trim_start();
        if j > 0 && b[j - 1] == b'&' {
            // A view of it (a lane, a narrower type): written when assigned to, keeping its other bytes.
            reads |= 1 << r;
            let mut view = rest.trim_start_matches(')').trim_start();
            // A lane: "[N]".
            if let Some(lane) = view.strip_prefix('[') {
                view = lane.split_once(']').map_or("", |(_, v)| v).trim_start_matches(')').trim_start();
            }
            let compound = ["+=", "-=", "|=", "&=", "^=", "<<=", ">>=", "*="].iter().any(|op| view.starts_with(op));
            if compound || (view.starts_with('=') && !view.starts_with("==")) {
                writes |= 1 << r;
            }
        } else if rest.starts_with('=') && !rest.starts_with("==") {
            writes |= 1 << r;
        } else {
            reads |= 1 << r;
        }
        i = j + 2 + d;
    }
    (reads, writes)
}

/// Vector registers a function keeps in locals: its most referenced ones (x86 has 16 vector registers).
const KEPT_V: usize = 8;

/// keep_registers for the vector registers (u128 locals gv0..gv31), run first: it stores and loads around the SPILL and
/// RELOAD markers (leaving them to keep_registers) and around each line calling a V_HELPERS helper, and never keeps a
/// register such a line names itself. Returns the registers kept and those to load at entry.
fn keep_vregs(lines: &mut [Line]) -> (Regs, Regs) {
    for l in lines.iter_mut() {
        if l.code.contains('V') || l.code.contains("FP_SET") {
            l.code = expand_vmacros(&l.code);
        }
    }
    let sync: Vec<bool> = lines
        .iter()
        .map(|l| V_HELPERS.iter().any(|h| l.code.match_indices(h).any(|(i, _)| l.code[i + h.len()..].starts_with("(c")))
            || VMACROS.iter().any(|(m, _)| l.code.contains(&format!("{m}("))))
        .collect();
    let mut count = [0usize; 32];
    let mut banned: Regs = 0;
    for (l, &s) in lines.iter().zip(&sync) {
        let mut rest = l.code.as_str();
        while let Some(i) = rest.find("c->v[") {
            if let Some((r, _)) = vreg_at(&rest[i..]) {
                count[r as usize] += 1;
            }
            rest = &rest[i + 5..];
        }
        if s {
            banned |= vregs_named(&l.code);
        }
    }
    let mut order: Vec<u32> = (0..32).filter(|&r| count[r as usize] > 0 && banned & (1 << r) == 0).collect();
    order.sort_by_key(|&r| (std::cmp::Reverse(count[r as usize]), r));
    let kept: Regs = order.iter().take(KEPT_V).fold(0, |acc, &r| acc | 1 << r);
    if kept == 0 {
        return (0, 0);
    }
    for l in lines.iter_mut() {
        let mut out = String::with_capacity(l.code.len());
        let mut rest = l.code.as_str();
        while let Some(i) = rest.find("c->v[") {
            out.push_str(&rest[..i]);
            match vreg_at(&rest[i..]) {
                Some((r, len)) if kept & (1 << r) != 0 => {
                    let _ = write!(out, "gv{r}");
                    rest = &rest[i + len..];
                }
                _ => {
                    out.push_str("c->v[");
                    rest = &rest[i + 5..];
                }
            }
        }
        out.push_str(rest);
        l.code = out;
    }
    let n = lines.len();
    let at: BTreeMap<u64, usize> = lines.iter().enumerate().filter_map(|(i, l)| l.label.map(|a| (a, i))).collect();
    let mut succ: Vec<Vec<usize>> = Vec::with_capacity(n);
    let mut pre: Vec<(Regs, Regs)> = Vec::with_capacity(n);
    for (i, l) in lines.iter().enumerate() {
        let mut s = Vec::new();
        let mut rest = l.code.as_str();
        while let Some(j) = rest.find("goto L_") {
            let hex: String = rest[j + 7..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if let Some(&t) = u64::from_str_radix(&hex, 16).ok().and_then(|a| at.get(&a)) {
                s.push(t);
            }
            rest = &rest[j + 7..];
        }
        if l.next && i + 1 < n {
            s.push(i + 1);
        }
        succ.push(s);
        // A helper line names no kept register: its stores come before it.
        let head = if sync[i] { "" } else { l.code.find("SPILL").map_or(l.code.as_str(), |j| &l.code[..j]) };
        pre.push(vuses(head));
    }
    let spills: Vec<bool> = lines.iter().zip(&sync).map(|(l, &s)| s || l.code.contains("SPILL")).collect();
    let reloads: Vec<bool> = lines.iter().zip(&sync).map(|(l, &s)| s || l.code.contains("RELOAD")).collect();
    let mut dirty_in = vec![0 as Regs; n];
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n {
            let out = if reloads[i] { 0 } else { dirty_in[i] | pre[i].1 };
            for &t in &succ[i] {
                if dirty_in[t] | out != dirty_in[t] {
                    dirty_in[t] |= out;
                    changed = true;
                }
            }
        }
    }
    let mut live_in = vec![0 as Regs; n];
    let mut live_out = vec![0 as Regs; n];
    changed = true;
    while changed {
        changed = false;
        for i in (0..n).rev() {
            let out = succ[i].iter().fold(0, |acc, &t| acc | live_in[t]);
            let (r, w) = pre[i];
            let spilled = if spills[i] { dirty_in[i] & !w } else { 0 };
            let after = if reloads[i] { 0 } else { out & !w };
            let inn = r | spilled | after;
            if inn != live_in[i] || out != live_out[i] {
                live_in[i] = inn;
                live_out[i] = out;
                changed = true;
            }
        }
    }
    let regs = |set: Regs| (0..32).filter(move |r| set & (1 << r) != 0);
    for (i, l) in lines.iter_mut().enumerate() {
        let mut st = String::new();
        if spills[i] {
            for r in regs(dirty_in[i] | pre[i].1) {
                let _ = write!(st, "c->v[{r}] = gv{r}; ");
            }
        }
        let mut ld = String::new();
        if reloads[i] {
            for r in regs(live_out[i]) {
                let _ = write!(ld, "gv{r} = c->v[{r}]; ");
            }
        }
        if sync[i] {
            l.code = format!("{st}{} {ld}", l.code);
        } else {
            if !st.is_empty() {
                l.code = l.code.replace("SPILL ", &format!("{st}SPILL "));
            }
            if !ld.is_empty() {
                l.code = l.code.replace("RELOAD ", &format!("RELOAD {ld}"));
            }
        }
    }
    (kept, live_in.first().copied().unwrap_or(0))
}

/// Keeps the function's most referenced registers in locals, and returns them and those to load at entry. Guest memory
/// is accessed through may_alias pointers, so clang must assume every guest store may change a register in the Cpu and
/// load it again before its next use; locals stay in host registers. The Cpu holds them where code outside the function
/// sees them: `SPILL` (before calls, tail calls, dispatches, returns and runtime entries) stores those written since the
/// Cpu last held them on some path to it (dirty), `RELOAD` (after calls and svc) loads those read again before being
/// written (live), except callee-saved ones after a call. Elsewhere the Cpu holds a register's value and its local is not
/// read again. No runtime helper reads integer registers or sp from the Cpu (vector registers stay there).
fn keep_registers(lines: &mut [Line]) -> (Regs, Regs) {
    let mut count = [0usize; 32];
    for l in lines.iter() {
        let mut rest = l.code.as_str();
        while let Some(i) = rest.find("c->") {
            if let Some((r, _)) = reg_at(&rest[i..]) {
                count[r as usize] += 1;
            }
            rest = &rest[i + 3..];
        }
    }
    let mut order: Vec<u32> = (0..32).filter(|&r| count[r as usize] > 0).collect();
    order.sort_by_key(|&r| (std::cmp::Reverse(count[r as usize]), r));
    let kept = order.iter().take(KEPT).fold(0, |acc, &r| acc | 1 << r);
    for l in lines.iter_mut() {
        l.code = localize(&l.code, kept);
    }
    let n = lines.len();
    let at: BTreeMap<u64, usize> = lines.iter().enumerate().filter_map(|(i, l)| l.label.map(|a| (a, i))).collect();
    // Per line: successors, and the reads and writes before its first marker (what follows a SPILL leaves or calls).
    let mut succ: Vec<Vec<usize>> = Vec::with_capacity(n);
    let mut pre: Vec<(Regs, Regs)> = Vec::with_capacity(n);
    for (i, l) in lines.iter().enumerate() {
        let mut s = Vec::new();
        let mut rest = l.code.as_str();
        while let Some(j) = rest.find("goto L_") {
            let hex: String = rest[j + 7..].chars().take_while(|c| c.is_ascii_hexdigit()).collect();
            if let Some(&t) = u64::from_str_radix(&hex, 16).ok().and_then(|a| at.get(&a)) {
                s.push(t);
            }
            rest = &rest[j + 7..];
        }
        if l.next && i + 1 < n {
            s.push(i + 1);
        }
        succ.push(s);
        let head = l.code.find("SPILL").map_or(l.code.as_str(), |j| &l.code[..j]);
        pre.push(uses(head));
    }
    let spills: Vec<bool> = lines.iter().map(|l| l.code.contains("SPILL")).collect();
    // Registers a RELOAD leaves to the locals: callee-saved ones after a call, none after svc.
    let trusted: Vec<Option<Regs>> = lines
        .iter()
        .map(|l| l.code.contains("RELOAD").then(|| if l.code.contains("RT_RETURNED") { CALLEE_SAVED } else { 0 }))
        .collect();
    // Dirty registers on entry to each line (forward, any path).
    let mut dirty_in = vec![0 as Regs; n];
    let mut changed = true;
    while changed {
        changed = false;
        for i in 0..n {
            let out = if trusted[i].is_some() { 0 } else { dirty_in[i] | pre[i].1 };
            for &t in &succ[i] {
                if dirty_in[t] | out != dirty_in[t] {
                    dirty_in[t] |= out;
                    changed = true;
                }
            }
        }
    }
    // Live registers on entry to each line (backward): a SPILL reads what it stores, a RELOAD writes what it loads.
    let mut live_in = vec![0 as Regs; n];
    let mut live_out = vec![0 as Regs; n];
    changed = true;
    while changed {
        changed = false;
        for i in (0..n).rev() {
            let out = succ[i].iter().fold(0, |acc, &t| acc | live_in[t]);
            let (r, w) = pre[i];
            let spilled = if spills[i] { dirty_in[i] & !w } else { 0 };
            let after = match trusted[i] {
                Some(keep) => out & keep & !w,
                None => out & !w,
            };
            let inn = r | spilled | after;
            if inn != live_in[i] || out != live_out[i] {
                live_in[i] = inn;
                live_out[i] = out;
                changed = true;
            }
        }
    }
    let field = |r: u32| if r == 31 { "c->sp".to_string() } else { format!("c->x[{r}]") };
    let local = |r: u32| if r == 31 { "gsp".to_string() } else { format!("gx{r}") };
    let regs = |set: Regs| (0..32).filter(move |r| set & (1 << r) != 0);
    for (i, l) in lines.iter_mut().enumerate() {
        if spills[i] {
            let mut st = String::new();
            for r in regs(dirty_in[i] | pre[i].1) {
                let _ = write!(st, "{} = {}; ", field(r), local(r));
            }
            l.code = l.code.replace("SPILL ", &st);
        }
        if let Some(keep) = trusted[i] {
            let mut ld = String::new();
            for r in regs(live_out[i] & !keep) {
                let _ = write!(ld, "{} = {}; ", local(r), field(r));
            }
            l.code = l.code.replace("RELOAD ", &ld);
        }
    }
    (kept, live_in.first().copied().unwrap_or(0))
}

fn emit_function(l: &Lifted, entry: u64, end: u64, out: &mut String, refs: &mut BTreeSet<u64>) {
    let s = scan(l, entry, end);
    let jump = |t: u64, refs: &mut BTreeSet<u64>| -> String {
        if (entry..end).contains(&t) {
            format!("goto L_{t:x};")
        } else if l.in_text(t) {
            refs.insert(t);
            format!("SPILL __attribute__((musttail)) return {}(c);", fname(t))
        } else {
            format!("c->pc = {}; {TAIL_DISPATCH}", hx(t))
        }
    };
    let mut lines = Vec::new();
    let mut a = entry;
    let mut last_ends = false;
    while a < end {
        let ra = hx(a + 4);
        let code = match l.at(a) {
            // Runtime entries see the registers: svc returns, brk and udf do not.
            Insn::Op(code) if code.contains("rt_svc(c") => format!("SPILL {code} RELOAD "),
            Insn::Trap(code) => format!("SPILL {code}"),
            Insn::Op(code) => code.clone(),
            Insn::B(t) => jump(*t, refs),
            Insn::BCond(cond, t) => format!("if ({cond}) {{ {} }}", jump(*t, refs)),
            Insn::Bl(t) if l.in_text(*t) => {
                refs.insert(*t);
                format!("c->x[30] = {ra}; SPILL {}(c); RELOAD RT_RETURNED(c, {ra})", fname(*t))
            }
            Insn::Bl(t) => format!("c->x[30] = {ra}; c->pc = {}; SPILL rt_dispatch(c); RELOAD RT_RETURNED(c, {ra})", hx(*t)),
            Insn::Blr(target) => format!(
                "c->pc = {target}; c->x[30] = {ra}; SPILL {{ GuestFn rt_f = rt_hit(c->pc); if (rt_f) rt_f(c); else rt_dispatch(c); }} RELOAD RT_RETURNED(c, {ra})"
            ),
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
            Insn::Ret(target) => format!("c->pc = {target}; SPILL return;"),
        };
        last_ends = l.at(a).ends_block();
        lines.push(Line { label: s.labels.contains(&a).then_some(a), code, next: !last_ends });
        a += 4;
    }
    if !last_ends {
        // falls through into the next function
        let code = if end < l.m.text.end { jump(end, refs) } else { format!("SPILL rt_udf(c, {});", hx(end)) };
        lines.push(Line { label: None, code, next: false });
    }
    optimize_flags(&mut lines);
    let (vkept, ventry_live) = keep_vregs(&mut lines);
    let (kept, entry_live) = keep_registers(&mut lines);
    let _ = write!(out, "RT_FN void {}(Cpu *c) {{\n   ", fname(entry));
    for r in (0..32).filter(|r| vkept & (1 << r) != 0) {
        if ventry_live & (1 << r) != 0 {
            let _ = write!(out, " u128 gv{r} = c->v[{r}];");
        } else {
            let _ = write!(out, " u128 gv{r};");
        }
    }
    for r in (0..32).filter(|r| kept & (1 << r) != 0) {
        let (name, field) = if r == 31 { ("gsp".to_string(), "c->sp".to_string()) } else { (format!("gx{r}"), format!("c->x[{r}]")) };
        if entry_live & (1 << r) != 0 {
            let _ = write!(out, " uint64_t {name} = {field};");
        } else {
            let _ = write!(out, " uint64_t {name};");
        }
    }
    out.push('\n');
    for l in &lines {
        if let Some(a) = l.label {
            let _ = writeln!(out, "L_{a:x}:;");
        }
        if !l.code.is_empty() {
            let _ = writeln!(out, "    {}", l.code);
        }
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
        // Six registers in locals: a call stores those written since the Cpu last held them and loads those read again
        // after it, but not callee-saved ones (x19, sp).
        assert!(body.contains("gx30 = 0x10001cull; c->x[0] = gx0; c->x[19] = gx19; c->x[30] = gx30; f_100200(c); gx0 = c->x[0]; RT_RETURNED(c, 0x10001cull)"));
        assert!(body.contains("uint64_t gx0; uint64_t gx19; uint64_t gx29 = c->x[29];"));
        assert!(body.contains("case 0x100238ull: goto L_100238;"));
        assert!(body.contains("goto L_100104;")); // the loop's back edge
    }

    /// `body` with each identifier in `params` replaced by $1..
    fn subst_params(body: &str, params: &[&str]) -> String {
        let mut out = String::new();
        let mut word = String::new();
        for ch in body.chars().chain(std::iter::once(' ')) {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                word.push(ch);
                continue;
            }
            match params.iter().position(|p| *p == word) {
                Some(k) => out.push_str(&format!("${}", k + 1)),
                None => out.push_str(&word),
            }
            word.clear();
            out.push(ch);
        }
        out.pop();
        out
    }

    #[test]
    fn vector_macros_and_helpers_match_simd_h() {
        let h = include_str!("../runtime/simd.h");
        // Every macro of the runtime naming a vector register (itself or through another) is in VMACROS.
        let mut names: BTreeSet<&str> = BTreeSet::new();
        let defs: Vec<&str> = [h, include_str!("../runtime/simd_fp.h"), include_str!("../runtime/cpu.h")]
            .iter()
            .flat_map(|f| f.lines())
            .filter(|l| l.starts_with("#define "))
            .collect();
        loop {
            let before = names.len();
            for d in &defs {
                let name = d[8..].split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).next().unwrap();
                if d.contains("c->v[") || names.iter().any(|n| d[8 + name.len()..].contains(&format!("{n}("))) {
                    names.insert(name);
                }
            }
            if names.len() == before {
                break;
            }
        }
        assert_eq!(names, VMACROS.iter().map(|(n, _)| *n).collect::<BTreeSet<_>>());
        // VMACROS: each as simd.h defines it, its parameters as $1..
        for (name, body) in VMACROS {
            let def = h.lines().find(|l| l.starts_with(&format!("#define {name}("))).unwrap_or_else(|| panic!("{name}"));
            let (params, rest) = def[8 + name.len() + 1..].split_once(')').unwrap();
            let mut want = rest.trim().to_string();
            want = subst_params(&want, &params.split(',').map(str::trim).collect::<Vec<_>>());
            assert_eq!(want, body, "{name}");
        }
        // V_HELPERS: the functions taking the Cpu that read or write c->v (fp_op_s / fp_op_d are not emitted).
        let mut touch = BTreeSet::new();
        let mut cur: Option<(String, bool)> = None;
        for l in h.lines() {
            if (l.starts_with("SI ") || l.starts_with("SN ")) && l.contains("(Cpu *c") {
                let name = l[..l.find('(').unwrap()].rsplit(' ').next().unwrap().trim_start_matches('*').to_string();
                cur = Some((name, false));
            }
            if let Some((_, uses)) = cur.as_mut() {
                *uses |= ["c->v[", "VSF", "VS8(", "VS16(", "VS32(", "VS64(", "VREG(", "VQ(", "VW(", "FP_SET", "VHI64("].iter().any(|m| l.contains(m));
            }
            if l == "}" || (l.ends_with('}') && cur.is_some() && !l.starts_with(' ')) {
                if let Some((name, true)) = cur.take() {
                    touch.insert(name);
                }
                cur = None;
            }
        }
        touch.remove("fp_op_s");
        touch.remove("fp_op_d");
        assert_eq!(touch, V_HELPERS.iter().map(|s| s.to_string()).collect::<BTreeSet<_>>());
    }

    #[test]
    fn fcmp_fused() {
        // 0x1000: fcmp s0, s1; b.gt 0x100c; ret; 0x100c: ret
        let mut img = Vec::new();
        for w in [0x1e212000u32, 0x5400004c, 0xd65f03c0, 0xd65f03c0] {
            img.extend_from_slice(&w.to_le_bytes());
        }
        let m = Module { name: "t".into(), base: 0x1000, image: &img, text: 0x1000..0x1010, seeds: BTreeSet::from([0x1000]) };
        let body = &lift_module(&m, &Options::default()).files[0].1;
        assert!(body.contains("if (cc_s(c, (*(saf32 *)&gv0), (*(saf32 *)&gv1), FCC_GT))"), "{body}");
        assert!(!body.contains("nzcv"), "{body}");
    }

    #[test]
    fn vector_lane_writes() {
        assert_eq!(vuses("((sa32 *)&gv7)[2] = x;"), (1 << 7, 1 << 7));
        assert_eq!(vuses("((sa32 *)&gv7)[0] |= x;"), (1 << 7, 1 << 7));
        assert_eq!(vuses("(*(saf32 *)&gv7) = x;"), (1 << 7, 1 << 7));
        assert_eq!(vuses("y = ((sa32 *)&gv7)[3] == x;"), (1 << 7, 0));
        assert_eq!(vuses("y = (*(saf32 *)&gv7);"), (1 << 7, 0));
        assert_eq!(vuses("gv7 = gv8;"), (1 << 8, 1 << 7));
    }

    #[test]
    fn vector_registers_in_locals() {
        // 0x1000: fmul s0, s1, s2; fadd s0, s0, s3; ret
        let mut img = Vec::new();
        for w in [0x1e220820u32, 0x1e232800, 0xd65f03c0] {
            img.extend_from_slice(&w.to_le_bytes());
        }
        let m = Module { name: "t".into(), base: 0x1000, image: &img, text: 0x1000..0x100c, seeds: BTreeSet::from([0x1000]) };
        let body = &lift_module(&m, &Options::default()).files[0].1;
        assert!(body.contains("u128 gv0; u128 gv1 = c->v[1]; u128 gv2 = c->v[2]; u128 gv3 = c->v[3];"), "{body}");
        assert!(body.contains("(gv0 = fb_s(op_s(c, FOP_ADD, (*(saf32 *)&gv0), (*(saf32 *)&gv3), 0)))"), "{body}");
        assert!(body.contains("c->v[0] = gv0; return;"), "{body}");
    }

    #[test]
    fn flags_fused_and_dead() {
        // 0x1000: cmp w0, #1; b.lt 0x1010; adds x1, x1, #1 (flags never read); ret; 0x1010: ret
        let mut img = Vec::new();
        for w in [0x7100041fu32, 0x5400006b, 0xb1000421, 0xd65f03c0, 0xd65f03c0] {
            img.extend_from_slice(&w.to_le_bytes());
        }
        let m = Module { name: "t".into(), base: 0x1000, image: &img, text: 0x1000..0x1014, seeds: BTreeSet::from([0x1000]) };
        let body = &lift_module(&m, &Options::default()).files[0].1;
        assert!(body.contains("if ((((int32_t)((uint32_t)((uint32_t)gx0))) < ((int32_t)((uint32_t)(0x1ull))))) { goto L_1010; }"), "{body}");
        assert!(!body.contains("subs32(c,") && !body.contains("COND_"));
        assert!(body.contains("add64_nf("));
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
