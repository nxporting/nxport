// The cheat virtual machine, as Atmosphère's dmnt runs it (docs/features/cheats.md, and for what the document leaves
// open stratosphere/dmnt/source/cheat/impl/dmnt_cheat_vm.cpp: DecodeNextOpcode, SkipConditionalBlock, Execute). This is
// a reimplementation from that behaviour, not a copy. What it does exactly as dmnt, including dmnt's quirks:
// - the program is the master cheat then every enabled cheat, one after the other, and runs as one: registers are
//   cleared at the start of each run, not per cheat, and a cheat with an unclosed block takes the next ones into it;
// - an opcode that does not decode (0xB, 0xC5-0xCF, 0xF0-0xFFE, or words missing at the end) ends the run;
// - a width other than 1, 2, 4 or 8 makes reads and writes do nothing and immediate values 0; arithmetic then keeps all
//   64 bits;
// - a read that fails leaves its destination as it was: a comparison then sees 0, and a register load keeps the
//   register. A read narrower than 8 bytes into a register replaces only its low bytes (dmnt reads into the register's
//   memory);
// - shifts take their amount modulo 64 (the arm64 instructions dmnt's shifts compile to);
// - memory types past 4 address main;
// - static registers 0x00-0x7F are read by cheats and 0x80-0xFF written, and keep their values between runs.
// Where dmnt stops with an abort (an else with no open block) the run ends instead and the caller is told.

/// Words a program (the master and the enabled cheats) may hold.
pub const MAX_PROGRAM: usize = 0x400;
const REGISTERS: usize = 0x10;
const STATIC_REGISTERS: usize = 0x100;
const READABLE_STATICS: usize = 0x80;

/// The memory a cheat address is relative to (MemoryAccessType).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Region {
    Main,
    Heap,
    Alias,
    Aslr,
    /// An address of the process itself.
    Absolute,
}

impl Region {
    fn of(nybble: u32) -> Region {
        match nybble {
            1 => Region::Heap,
            2 => Region::Alias,
            3 => Region::Aslr,
            4 => Region::Absolute,
            _ => Region::Main,
        }
    }
}

/// An address the VM reads or writes: `offset` bytes into `region` (wrapping as dmnt's 64-bit sums do).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Address {
    pub region: Region,
    pub offset: u64,
}

/// The process cheats run against: dmnt's debug handle to it.
pub trait Process {
    /// Reads `out.len()` bytes; false (and `out` unchanged) where the process has no readable memory.
    fn read(&mut self, at: Address, out: &mut [u8]) -> bool;
    /// Writes `data`, whatever the memory's protection (a debugger's write); nothing where no memory is mapped.
    fn write(&mut self, at: Address, data: &[u8]);
    /// Opcode 0xFF0 and 0xFF1.
    fn pause(&mut self);
    fn resume(&mut self);
    /// Opcode 0xFFF: dmnt appends the value to the SD card's atmosphere/cheat_vm_logs/<id>.log.
    fn log(&mut self, id: u32, value: u64);
}

/// A run ended early where dmnt would abort.
#[derive(Debug, PartialEq)]
pub struct Abort(pub &'static str);

#[derive(Clone, Copy, Debug)]
enum Op {
    StoreStatic { width: u32, region: u32, ofs_reg: usize, rel: u64, value: u64 },
    BeginCond { width: u32, region: u32, cond: u32, use_ofs: bool, ofs_reg: usize, rel: u64, value: u64 },
    EndCond { is_else: bool },
    Loop { start: bool, reg: usize, iters: u32 },
    LoadStatic { reg: usize, value: u64 },
    LoadMemory { width: u32, region: u32, reg: usize, from: u32, ofs_reg: usize, rel: u64 },
    StoreStaticToAddress { width: u32, reg: usize, increment: bool, add_ofs: bool, ofs_reg: usize, value: u64 },
    MathStatic { width: u32, reg: usize, op: u32, value: u32 },
    Keypress { mask: u64 },
    ExtKeypress { mask: u64, auto_repeat: bool },
    MathRegister { width: u32, op: u32, dst: usize, src1: usize, src2: usize, imm: Option<u64> },
    StoreRegister { width: u32, src: usize, addr_reg: usize, increment: bool, ofs_type: u32, region: u32, ofs_reg: usize, rel: u64 },
    RegisterCond { width: u32, cond: u32, val_reg: usize, comp: u32, region: u32, addr_reg: usize, other_reg: usize, ofs_reg: usize, rel: u64, value: u64 },
    SaveRestore { dst: usize, src: usize, op: u32 },
    SaveRestoreMask { op: u32, mask: u16 },
    StaticRegister { index: usize, reg: usize },
    Pause,
    Resume,
    DebugLog { width: u32, id: u32, val_type: u32, region: u32, addr_reg: usize, val_reg: usize, ofs_reg: usize, rel: u64 },
}

impl Op {
    fn begins_block(&self) -> bool {
        matches!(self, Op::BeginCond { .. } | Op::Keypress { .. } | Op::ExtKeypress { .. } | Op::RegisterCond { .. })
    }
}

fn valid_width(width: u32) -> bool {
    matches!(width, 1 | 2 | 4 | 8)
}

/// `v` cut to `width` bytes; 0 for a width no value has (GetVmInt).
fn vm_int(v: u64, width: u32) -> u64 {
    match width {
        1 => v as u8 as u64,
        2 => v as u16 as u64,
        4 => v as u32 as u64,
        8 => v,
        _ => 0,
    }
}

/// `v` cut to `width` bytes; all of it for any other width (the "apply bit width" steps).
fn truncate(v: u64, width: u32) -> u64 {
    match width {
        1 | 2 | 4 => vm_int(v, width),
        _ => v,
    }
}

fn compare(cond: u32, a: u64, b: u64) -> bool {
    match cond {
        1 => a > b,
        2 => a >= b,
        3 => a < b,
        4 => a <= b,
        5 => a == b,
        6 => a != b,
        _ => false,
    }
}

fn nyb(w: u32, shift: u32) -> u32 {
    (w >> shift) & 0xF
}

/// One cheat process's VM: its static registers and the keys held at its last run outlive a run.
pub struct Vm {
    registers: [u64; REGISTERS],
    saved: [u64; REGISTERS],
    statics: [u64; STATIC_REGISTERS],
    loop_tops: [usize; REGISTERS],
    ip: usize,
    depth: usize,
    decode_ok: bool,
    keys_old: u64,
}

impl Default for Vm {
    fn default() -> Self {
        Vm {
            registers: [0; REGISTERS],
            saved: [0; REGISTERS],
            statics: [0; STATIC_REGISTERS],
            loop_tops: [0; REGISTERS],
            ip: 0,
            depth: 0,
            decode_ok: true,
            keys_old: 0,
        }
    }
}

impl Vm {
    /// The next word, or 0 past the end, which fails the decode.
    fn word(&mut self, program: &[u32], valid: &mut bool) -> u32 {
        match program.get(self.ip) {
            Some(&w) => {
                self.ip += 1;
                w
            }
            None => {
                *valid = false;
                0
            }
        }
    }

    /// An immediate of `width` bytes: one word, two for 8 bytes.
    fn vm_int_words(&mut self, program: &[u32], width: u32, valid: &mut bool) -> u64 {
        let first = self.word(program, valid) as u64;
        if width == 8 {
            first << 32 | self.word(program, valid) as u64
        } else {
            vm_int(first, width)
        }
    }

    /// The opcode at the instruction pointer, which moves past it. Once a decode fails every later one does, until the
    /// next run.
    fn decode(&mut self, program: &[u32]) -> Option<Op> {
        if !self.decode_ok {
            return None;
        }
        let mut valid = true;
        let first = self.word(program, &mut valid);
        if !valid {
            self.decode_ok = false;
            return None;
        }
        let mut kind = first >> 28;
        if kind >= 0xC {
            kind = kind << 4 | nyb(first, 24);
        }
        if kind >= 0xF0 {
            kind = kind << 4 | nyb(first, 20);
        }
        let rel40 = |second: u32| ((first & 0xFF) as u64) << 32 | second as u64;
        let rel36 = |second: u32| ((first & 0xF) as u64) << 32 | second as u64;
        let op = match kind {
            0x0 => {
                let second = self.word(program, &mut valid);
                let width = nyb(first, 24);
                Op::StoreStatic {
                    width,
                    region: nyb(first, 20),
                    ofs_reg: nyb(first, 16) as usize,
                    rel: rel40(second),
                    value: self.vm_int_words(program, width, &mut valid),
                }
            }
            0x1 => {
                let second = self.word(program, &mut valid);
                let width = nyb(first, 24);
                Op::BeginCond {
                    width,
                    region: nyb(first, 20),
                    cond: nyb(first, 16),
                    use_ofs: nyb(first, 12) != 0,
                    ofs_reg: nyb(first, 8) as usize,
                    rel: rel40(second),
                    value: self.vm_int_words(program, width, &mut valid),
                }
            }
            0x2 => Op::EndCond { is_else: nyb(first, 24) == 1 },
            0x3 => {
                let start = nyb(first, 24) == 0;
                Op::Loop { start, reg: nyb(first, 16) as usize, iters: if start { self.word(program, &mut valid) } else { 0 } }
            }
            0x4 => {
                let hi = self.word(program, &mut valid) as u64;
                Op::LoadStatic { reg: nyb(first, 16) as usize, value: hi << 32 | self.word(program, &mut valid) as u64 }
            }
            0x5 => {
                let second = self.word(program, &mut valid);
                Op::LoadMemory {
                    width: nyb(first, 24),
                    region: nyb(first, 20),
                    reg: nyb(first, 16) as usize,
                    from: nyb(first, 12),
                    ofs_reg: nyb(first, 8) as usize,
                    rel: rel40(second),
                }
            }
            0x6 => {
                let hi = self.word(program, &mut valid) as u64;
                Op::StoreStaticToAddress {
                    width: nyb(first, 24),
                    reg: nyb(first, 16) as usize,
                    increment: nyb(first, 12) != 0,
                    add_ofs: nyb(first, 8) != 0,
                    ofs_reg: nyb(first, 4) as usize,
                    value: hi << 32 | self.word(program, &mut valid) as u64,
                }
            }
            0x7 => Op::MathStatic {
                width: nyb(first, 24),
                reg: nyb(first, 16) as usize,
                op: nyb(first, 12),
                value: self.word(program, &mut valid),
            },
            0x8 => Op::Keypress { mask: (first & 0x0FFF_FFFF) as u64 },
            0x9 => {
                let width = nyb(first, 24);
                let imm = (nyb(first, 8) != 0).then(|| self.vm_int_words(program, width, &mut valid));
                Op::MathRegister {
                    width,
                    op: nyb(first, 20),
                    dst: nyb(first, 16) as usize,
                    src1: nyb(first, 12) as usize,
                    src2: if imm.is_some() { 0 } else { nyb(first, 4) as usize },
                    imm,
                }
            }
            0xA => {
                let mut ofs_type = nyb(first, 8);
                let (mut region, mut rel) = (0, 0);
                match ofs_type {
                    0 | 1 => {}
                    2 => rel = rel36(self.word(program, &mut valid)),
                    3 => region = nyb(first, 4),
                    4 | 5 => {
                        region = nyb(first, 4);
                        rel = rel36(self.word(program, &mut valid));
                    }
                    _ => ofs_type = 0,
                }
                Op::StoreRegister {
                    width: nyb(first, 24),
                    src: nyb(first, 20) as usize,
                    addr_reg: nyb(first, 16) as usize,
                    increment: nyb(first, 12) != 0,
                    ofs_type,
                    region,
                    ofs_reg: nyb(first, 4) as usize,
                    rel,
                }
            }
            0xC0 => {
                let width = nyb(first, 20);
                let comp = nyb(first, 8);
                let (mut region, mut addr_reg, mut other_reg, mut ofs_reg, mut rel, mut value) = (0, 0, 0, 0, 0, 0);
                match comp {
                    0 => {
                        region = nyb(first, 4);
                        rel = rel36(self.word(program, &mut valid));
                    }
                    1 => {
                        region = nyb(first, 4);
                        ofs_reg = nyb(first, 0) as usize;
                    }
                    2 => {
                        addr_reg = nyb(first, 4) as usize;
                        rel = rel36(self.word(program, &mut valid));
                    }
                    3 => {
                        addr_reg = nyb(first, 4) as usize;
                        ofs_reg = nyb(first, 0) as usize;
                    }
                    4 => value = self.vm_int_words(program, width, &mut valid),
                    5 => other_reg = nyb(first, 4) as usize,
                    _ => {}
                }
                Op::RegisterCond { width, cond: nyb(first, 16), val_reg: nyb(first, 12) as usize, comp, region, addr_reg, other_reg, ofs_reg, rel, value }
            }
            0xC1 => Op::SaveRestore { dst: nyb(first, 16) as usize, src: nyb(first, 8) as usize, op: nyb(first, 4) },
            0xC2 => Op::SaveRestoreMask { op: nyb(first, 20), mask: first as u16 },
            0xC3 => Op::StaticRegister { index: ((first >> 4) & 0xFF) as usize, reg: nyb(first, 0) as usize },
            0xC4 => {
                let hi = self.word(program, &mut valid) as u64;
                Op::ExtKeypress { mask: hi << 32 | self.word(program, &mut valid) as u64, auto_repeat: nyb(first, 20) != 0 }
            }
            0xFF0 => Op::Pause,
            0xFF1 => Op::Resume,
            0xFFF => {
                let val_type = nyb(first, 8);
                let (mut region, mut addr_reg, mut val_reg, mut ofs_reg, mut rel) = (0, 0, 0, 0, 0);
                match val_type {
                    0 => {
                        region = nyb(first, 4);
                        rel = rel36(self.word(program, &mut valid));
                    }
                    1 => {
                        region = nyb(first, 4);
                        ofs_reg = nyb(first, 0) as usize;
                    }
                    2 => {
                        addr_reg = nyb(first, 4) as usize;
                        rel = rel36(self.word(program, &mut valid));
                    }
                    3 => {
                        addr_reg = nyb(first, 4) as usize;
                        ofs_reg = nyb(first, 0) as usize;
                    }
                    4 => val_reg = nyb(first, 4) as usize,
                    _ => {}
                }
                Op::DebugLog { width: nyb(first, 16), id: nyb(first, 12), val_type, region, addr_reg, val_reg, ofs_reg, rel }
            }
            _ => {
                valid = false;
                Op::Pause
            }
        };
        self.decode_ok = valid;
        valid.then_some(op)
    }

    /// Skips to the end of the open block, or for a failed condition (`is_if`) to its else.
    fn skip_block(&mut self, program: &[u32], is_if: bool) -> Result<(), Abort> {
        if self.depth == 0 {
            return Err(Abort("an else or a skipped block with no condition open"));
        }
        let desired = self.depth - 1;
        while self.depth > desired {
            let Some(op) = self.decode(program) else { break };
            if op.begins_block() {
                self.depth += 1;
            } else if let Op::EndCond { is_else } = op {
                if !is_else {
                    self.depth -= 1;
                } else if is_if && self.depth - 1 == desired {
                    break;
                }
            }
        }
        Ok(())
    }

    fn read(p: &mut impl Process, at: Address, width: u32, into: &mut u64) {
        if valid_width(width) {
            let mut bytes = into.to_le_bytes();
            if p.read(at, &mut bytes[..width as usize]) {
                *into = u64::from_le_bytes(bytes);
            }
        }
    }

    fn write(p: &mut impl Process, at: Address, width: u32, value: u64) {
        if valid_width(width) {
            p.write(at, &value.to_le_bytes()[..width as usize]);
        }
    }

    fn reg_value(&self, reg: usize, width: u32) -> u64 {
        vm_int(self.registers[reg], width)
    }

    /// One run of `program` with `keys` held (HidNpadButton bits).
    pub fn execute(&mut self, program: &[u32], p: &mut impl Process, keys: u64) -> Result<(), Abort> {
        self.registers = [0; REGISTERS];
        self.saved = [0; REGISTERS];
        self.loop_tops = [0; REGISTERS];
        self.ip = 0;
        self.depth = 0;
        self.decode_ok = true;
        let at = |region: u32, offset: u64| Address { region: Region::of(region), offset };
        let absolute = |offset: u64| Address { region: Region::Absolute, offset };
        while let Some(op) = self.decode(program) {
            if op.begins_block() {
                self.depth += 1;
            }
            let r = &mut self.registers;
            match op {
                Op::StoreStatic { width, region, ofs_reg, rel, value } => {
                    Self::write(p, at(region, rel.wrapping_add(r[ofs_reg])), width, value);
                }
                Op::BeginCond { width, region, cond, use_ofs, ofs_reg, rel, value } => {
                    let offset = if use_ofs { r[ofs_reg].wrapping_add(rel) } else { rel };
                    let mut v = 0;
                    Self::read(p, at(region, offset), width, &mut v);
                    if !compare(cond, v, value) {
                        self.skip_block(program, true)?;
                    }
                }
                Op::EndCond { is_else: true } => self.skip_block(program, false)?,
                Op::EndCond { is_else: false } => self.depth = self.depth.saturating_sub(1),
                Op::Loop { start: true, reg, iters } => {
                    r[reg] = iters as u64;
                    self.loop_tops[reg] = self.ip;
                }
                Op::Loop { start: false, reg, .. } => {
                    r[reg] = r[reg].wrapping_sub(1);
                    if r[reg] != 0 {
                        self.ip = self.loop_tops[reg];
                    }
                }
                Op::LoadStatic { reg, value } => r[reg] = value,
                Op::LoadMemory { width, region, reg, from, ofs_reg, rel } => {
                    let src = match from {
                        1 => absolute(r[reg].wrapping_add(rel)),
                        2 => absolute(r[ofs_reg].wrapping_add(rel)),
                        3 => at(region, r[ofs_reg].wrapping_add(rel)),
                        _ => at(region, rel),
                    };
                    Self::read(p, src, width, &mut r[reg]);
                }
                Op::StoreStaticToAddress { width, reg, increment, add_ofs, ofs_reg, value } => {
                    let dst = if add_ofs { r[reg].wrapping_add(r[ofs_reg]) } else { r[reg] };
                    Self::write(p, absolute(dst), width, value);
                    if increment {
                        r[reg] = r[reg].wrapping_add(width as u64);
                    }
                }
                Op::MathStatic { width, reg, op, value } => {
                    let (a, b) = (r[reg], value as u64);
                    r[reg] = match op {
                        0 => a.wrapping_add(b),
                        1 => a.wrapping_sub(b),
                        2 => a.wrapping_mul(b),
                        3 => a.wrapping_shl(b as u32),
                        4 => a.wrapping_shr(b as u32),
                        _ => a,
                    };
                    r[reg] = truncate(r[reg], width);
                }
                Op::Keypress { mask } => {
                    if mask & keys != mask {
                        self.skip_block(program, true)?;
                    }
                }
                Op::ExtKeypress { mask, auto_repeat } => {
                    // Without auto-repeat the block runs once per press: not while the keys were already held last run.
                    if mask & keys != mask || (!auto_repeat && mask & self.keys_old == mask) {
                        self.skip_block(program, true)?;
                    }
                }
                Op::MathRegister { width, op, dst, src1, src2, imm } => {
                    let a = r[src1];
                    let b = imm.unwrap_or(r[src2]);
                    let f32_op = |f: fn(f32, f32) -> f32| f(f32::from_bits(a as u32), f32::from_bits(b as u32)).to_bits() as u64;
                    let f64_op = |f: fn(f64, f64) -> f64| f(f64::from_bits(a), f64::from_bits(b)).to_bits();
                    let float = |f32f: fn(f32, f32) -> f32, f64f: fn(f64, f64) -> f64| match width {
                        4 => f32_op(f32f),
                        8 => f64_op(f64f),
                        _ => 0,
                    };
                    let result = match op {
                        0 => a.wrapping_add(b),
                        1 => a.wrapping_sub(b),
                        2 => a.wrapping_mul(b),
                        3 => a.wrapping_shl(b as u32),
                        4 => a.wrapping_shr(b as u32),
                        5 => a & b,
                        6 => a | b,
                        7 => !a,
                        8 => a ^ b,
                        9 => a,
                        10 => float(|x, y| x + y, |x, y| x + y),
                        11 => float(|x, y| x - y, |x, y| x - y),
                        12 => float(|x, y| x * y, |x, y| x * y),
                        13 => float(|x, y| x / y, |x, y| x / y),
                        _ => 0,
                    };
                    r[dst] = truncate(result, width);
                }
                Op::StoreRegister { width, src, addr_reg, increment, ofs_type, region, ofs_reg, rel } => {
                    let value = r[src];
                    let base = r[addr_reg];
                    let dst = match ofs_type {
                        1 => absolute(base.wrapping_add(r[ofs_reg])),
                        2 => absolute(base.wrapping_add(rel)),
                        3 => at(region, base),
                        4 => at(region, rel),
                        5 => at(region, base.wrapping_add(rel)),
                        _ => absolute(base),
                    };
                    Self::write(p, dst, width, value);
                    if increment {
                        r[addr_reg] = r[addr_reg].wrapping_add(width as u64);
                    }
                }
                Op::RegisterCond { width, cond, val_reg, comp, region, addr_reg, other_reg, ofs_reg, rel, value } => {
                    let src = self.reg_value(val_reg, width);
                    let r = &self.registers;
                    let operand = match comp {
                        4 => value,
                        5 => vm_int(r[other_reg], width),
                        _ => {
                            let addr = match comp {
                                0 => at(region, rel),
                                1 => at(region, r[ofs_reg]),
                                2 => absolute(r[addr_reg].wrapping_add(rel)),
                                3 => absolute(r[addr_reg].wrapping_add(r[ofs_reg])),
                                _ => absolute(0),
                            };
                            let mut v = 0;
                            Self::read(p, addr, width, &mut v);
                            v
                        }
                    };
                    if !compare(cond, src, operand) {
                        self.skip_block(program, true)?;
                    }
                }
                Op::SaveRestore { dst, src, op } => match op {
                    3 => r[dst] = 0,
                    2 => self.saved[dst] = 0,
                    1 => self.saved[dst] = r[src],
                    _ => r[dst] = self.saved[src],
                },
                Op::SaveRestoreMask { op, mask } => {
                    for i in (0..REGISTERS).filter(|i| mask >> i & 1 != 0) {
                        match op {
                            2 => self.saved[i] = 0,
                            3 => r[i] = 0,
                            1 => self.saved[i] = r[i],
                            _ => r[i] = self.saved[i],
                        }
                    }
                }
                Op::StaticRegister { index, reg } => {
                    if index < READABLE_STATICS {
                        r[reg] = self.statics[index];
                    } else {
                        self.statics[index] = r[reg];
                    }
                }
                Op::Pause => p.pause(),
                Op::Resume => p.resume(),
                Op::DebugLog { width, id, val_type, region, addr_reg, val_reg, ofs_reg, rel } => {
                    let value = if val_type == 4 {
                        vm_int(r[val_reg], width)
                    } else {
                        let addr = match val_type {
                            0 => at(region, rel),
                            1 => at(region, r[ofs_reg]),
                            2 => absolute(r[addr_reg].wrapping_add(rel)),
                            3 => absolute(r[addr_reg].wrapping_add(r[ofs_reg])),
                            _ => absolute(0),
                        };
                        let mut v = 0;
                        Self::read(p, addr, width, &mut v);
                        v
                    };
                    p.log(id, value);
                }
            }
        }
        self.keys_old = keys;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A process with four regions of 0x100 bytes, each also reachable absolutely at its base below.
    struct TestProcess {
        mem: [[u8; 0x100]; 4],
        pauses: i32,
        logs: Vec<(u32, u64)>,
    }

    const BASES: [u64; 4] = [0x1000, 0x2000, 0x3000, 0x4000];

    impl TestProcess {
        fn new() -> Self {
            TestProcess { mem: [[0; 0x100]; 4], pauses: 0, logs: Vec::new() }
        }
        fn locate(&mut self, at: Address, len: usize) -> Option<&mut [u8]> {
            let (i, off) = match at.region {
                Region::Main => (0, at.offset),
                Region::Heap => (1, at.offset),
                Region::Alias => (2, at.offset),
                Region::Aslr => (3, at.offset),
                Region::Absolute => {
                    let i = BASES.iter().position(|&b| at.offset >= b && at.offset < b + 0x100)?;
                    (i, at.offset - BASES[i])
                }
            };
            let off = off as usize;
            self.mem[i].get_mut(off..off.checked_add(len)?)
        }
        fn get(&self, region: usize, off: usize, len: usize) -> u64 {
            let mut b = [0u8; 8];
            b[..len].copy_from_slice(&self.mem[region][off..off + len]);
            u64::from_le_bytes(b)
        }
        fn put(&mut self, region: usize, off: usize, bytes: &[u8]) {
            self.mem[region][off..off + bytes.len()].copy_from_slice(bytes);
        }
    }

    impl Process for TestProcess {
        fn read(&mut self, at: Address, out: &mut [u8]) -> bool {
            let len = out.len();
            match self.locate(at, len) {
                Some(m) => {
                    out.copy_from_slice(m);
                    true
                }
                None => false,
            }
        }
        fn write(&mut self, at: Address, data: &[u8]) {
            if let Some(m) = self.locate(at, data.len()) {
                m.copy_from_slice(data);
            }
        }
        fn pause(&mut self) {
            self.pauses += 1;
        }
        fn resume(&mut self) {
            self.pauses -= 1;
        }
        fn log(&mut self, id: u32, value: u64) {
            self.logs.push((id, value));
        }
    }

    fn run(program: &[u32], p: &mut TestProcess) -> Vm {
        run_keys(program, p, 0)
    }

    fn run_keys(program: &[u32], p: &mut TestProcess, keys: u64) -> Vm {
        let mut vm = Vm::default();
        vm.execute(program, p, keys).unwrap();
        vm
    }

    #[test]
    fn op0_store_static() {
        let mut p = TestProcess::new();
        // Main + 0x10, 4 bytes; heap + 0x20 + r0 (0), 8 bytes; 1 and 2 bytes; a bad width writes nothing.
        run(
            &[
                0x0400_0000, 0x10, 0xDEAD_BEEF,
                0x0810_0000, 0x20, 0x1122_3344, 0x5566_7788,
                0x0120_0000, 0x30, 0xFFFF_FFAB,
                0x0230_0000, 0x40, 0x1234_ABCD,
                0x0300_0000, 0x50, 0xFFFF_FFFF,
            ],
            &mut p,
        );
        assert_eq!(p.get(0, 0x10, 4), 0xDEAD_BEEF);
        assert_eq!(p.get(1, 0x20, 8), 0x1122_3344_5566_7788);
        assert_eq!(p.get(2, 0x30, 2), 0xAB);
        assert_eq!(p.get(3, 0x40, 4), 0xABCD);
        assert_eq!(p.get(0, 0x50, 8), 0);
        // With an offset register (r3 = 8), and memory type 4: an absolute address.
        run(&[0x4003_0000, 0, 8, 0x0103_0000, 0x10, 0x77, 0x0140_0000, 0x1020, 0x66], &mut p);
        assert_eq!(p.get(0, 0x18, 1), 0x77);
        assert_eq!(p.get(0, 0x20, 1), 0x66);
    }

    #[test]
    fn op1_conditions_and_else() {
        let mut p = TestProcess::new();
        p.put(1, 0x10, &[5]);
        // if heap[0x10] == 5 { main[0] = 1 } else { main[1] = 1 }; if heap[0x10] > 5 { main[2] = 1 } else { main[3] = 1 }
        run(
            &[
                0x1115_0000, 0x10, 5, 0x0100_0000, 0, 1, 0x2100_0000, 0x0100_0000, 1, 1, 0x2000_0000,
                0x1111_0000, 0x10, 5, 0x0100_0000, 2, 1, 0x2100_0000, 0x0100_0000, 3, 1, 0x2000_0000,
            ],
            &mut p,
        );
        assert_eq!(&p.mem[0][..4], &[1, 0, 0, 1]);
        // Every comparison, with an offset register: r1 = 0x10, heap[0x10 + 0] (offset register form).
        for (cond, expect) in [(1, false), (2, true), (3, false), (4, true), (5, true), (6, false), (7, false)] {
            let mut p = TestProcess::new();
            p.put(1, 0x10, &[5]);
            run(&[0x4001_0000, 0, 0x10, 0x1110_1100 | cond << 16, 0, 5, 0x0100_0000, 0, 1, 0x2000_0000], &mut p);
            assert_eq!(p.mem[0][0] == 1, expect, "condition {cond}");
        }
        // An unreadable address compares as 0; nested blocks are skipped whole, and an operand word that reads as
        // 20000000 ends nothing.
        let mut p = TestProcess::new();
        run(
            &[
                0x1415_0000, 0x900, 1, 0x1415_0000, 0x900, 0, 0x0400_0000, 0, 0x2000_0000, 0x2000_0000, 0x2000_0000,
                0x0100_0000, 1, 1,
            ],
            &mut p,
        );
        assert_eq!(&p.mem[0][..2], &[0, 1]);
    }

    #[test]
    fn op3_loops() {
        let mut p = TestProcess::new();
        // r1 = main + 0; loop 4 times (r0): store 1 byte at [r1], r1 += 1.
        run(&[0x4001_0000, 0, 0x1000, 0x3000_0000, 4, 0x6101_1000, 0, 0xAA, 0x3100_0000], &mut p);
        assert_eq!(&p.mem[0][..5], &[0xAA, 0xAA, 0xAA, 0xAA, 0]);
    }

    #[test]
    fn op4_op5_loads() {
        let mut p = TestProcess::new();
        p.put(0, 0x10, &0x2000u64.to_le_bytes()); // main[0x10] -> heap
        p.put(1, 0x08, &0x1122_3344u32.to_le_bytes());
        p.put(1, 0x20, &0x55u64.to_le_bytes());
        let vm = run(
            &[
                0x5800_0000, 0x10, // r0 = main[0x10]
                0x5400_1000, 0x08, // r0 = [r0 + 8] (4 bytes): only the low 4 bytes are replaced
                0x4002_0000, 0, 0x2000, // r2 = heap address
                0x5803_2200, 0x20, // r3 = [r2 + 0x20]
                0x4004_0000, 0, 0x18, // r4 = 0x18
                0x5815_3400, 0x08, // r5 = heap[r4 + 8]
                0x5806_0000, 0x900, // unreadable: r6 stays
            ],
            &mut p,
        );
        assert_eq!(vm.registers[0], 0x1122_3344);
        assert_eq!(vm.registers[3], 0x55);
        assert_eq!(vm.registers[5], 0x55);
        assert_eq!(vm.registers[6], 0);
        // The partial replacement: a pointer's high half survives a 4-byte load.
        let vm = run(&[0x4000_0000, 0xAB, 0xFFFF_FFFF, 0x4001_0000, 0, 0x1000, 0x5400_2100, 0x10], &mut p);
        assert_eq!(vm.registers[0], 0xAB_0000_2000);
    }

    #[test]
    fn op6_store_to_register_address() {
        let mut p = TestProcess::new();
        // r0 = heap base, r1 = 4: store 2 bytes at r0 + r1, then at r0 with increment, twice.
        run(
            &[
                0x4000_0000, 0, 0x2000, 0x4001_0000, 0, 4, 0x6200_0110, 0, 0xBEEF, 0x6200_1000, 0, 0x1111,
                0x6200_1000, 0x1, 0x2222_2222,
            ],
            &mut p,
        );
        assert_eq!(p.get(1, 4, 2), 0xBEEF);
        assert_eq!(p.get(1, 0, 4), 0x2222_1111);
    }

    #[test]
    fn op7_legacy_arithmetic() {
        let mut p = TestProcess::new();
        let vm = run(
            &[
                0x4000_0000, 0, 10, 0x7400_0000, 5, // r0 += 5
                0x4001_0000, 0, 3, 0x7401_1000, 5, // r1 -= 5, cut to 4 bytes
                0x4002_0000, 0, 7, 0x7802_2000, 3, // r2 *= 3
                0x4003_0000, 0, 1, 0x7803_3000, 68, // r3 <<= 68 (68 mod 64 = 4)
                0x4004_0000, 0, 0x100, 0x7204_4000, 4, // r4 >>= 4, cut to 2 bytes
                0x4005_0000, 0, 9, 0x7805_5000, 1, // op 5: not legacy, r5 unchanged
            ],
            &mut p,
        );
        assert_eq!(vm.registers[..6], [15, 0xFFFF_FFFE, 21, 16, 0x10, 9]);
    }

    #[test]
    fn op8_c4_keypress() {
        let program = [0x8000_0041, 0x0100_0000, 0, 1, 0x2000_0000]; // A + L
        let mut p = TestProcess::new();
        run_keys(&program, &mut p, 0x41 | 0x2);
        assert_eq!(p.mem[0][0], 1);
        let mut p = TestProcess::new();
        run_keys(&program, &mut p, 0x1);
        assert_eq!(p.mem[0][0], 0);
        // C4 without auto-repeat runs once per press; with it, every run.
        let once = [0xC400_0000, 0x1, 0, 0x7400_0000, 1, 0xC300_0800, 0x2000_0000];
        let repeat = [0xC410_0000, 0x1, 0, 0x7400_0000, 1, 0xC300_0800, 0x2000_0000];
        for (program, expect) in [(once, [1, 1, 1, 2]), (repeat, [1, 2, 2, 3])] {
            let (mut vm, mut p, mut seen) = (Vm::default(), TestProcess::new(), Vec::new());
            for keys in [1u64 << 32, 1 << 32, 0, 1 << 32] {
                // Static register 0x80 written by the cheat, read back as statics[0x80].
                let program: Vec<u32> = [&[0xC300_0000u32][..], &program[..]].concat();
                vm.statics[0] = vm.statics[0x80];
                vm.execute(&program, &mut p, keys).unwrap();
                seen.push(vm.statics[0x80]);
            }
            assert_eq!(seen, expect);
        }
    }

    #[test]
    fn op9_arithmetic() {
        let mut p = TestProcess::new();
        let f = |x: f32| x.to_bits();
        let vm = run(
            &[
                0x4001_0000, 0, 6, 0x4002_0000, 0, 4,
                0x9820_1020, // r0 = r1 * r2
                0x9833_1100, 0, 2, // r3 = r1 << 2 (8-byte immediate)
                0x9464_1100, 0xFFFF_FFF0, // r4 = r1 | 0xFFFFFFF0, 4 bytes
                0x9875_1000, // r5 = !r1
                0x9189_1100, 3, // r9 = (r1 ^ 3) cut to a byte
                0x4006_0000, 0, f(1.5), 0x94A7_6100, f(2.25), // r7 = 1.5f + 2.25f
                0x98D8_6100, 0x4000_0000, 0, // r8 = r6 / 2.0 (doubles: r6's bits as a tiny double)
                0x98EA_1100, 0, 1, // op 14: 0
            ],
            &mut p,
        );
        assert_eq!(vm.registers[0], 24);
        assert_eq!(vm.registers[3], 24);
        assert_eq!(vm.registers[4], 0xFFFF_FFF6);
        assert_eq!(vm.registers[5], !6);
        assert_eq!(vm.registers[9], 5);
        assert_eq!(f32::from_bits(vm.registers[7] as u32), 3.75);
        assert_eq!(f64::from_bits(vm.registers[8]), f64::from_bits(f(1.5) as u64) / 2.0);
        assert_eq!(vm.registers[10], 0);
    }

    #[test]
    fn op_a_store_register() {
        let mut p = TestProcess::new();
        run(
            &[
                0x4001_0000, 0, 0x1000, // r1 = main base
                0x4002_0000, 0, 0x11, // r2 = 0x11 (value; also an offset)
                0x4003_0000, 0, 0x20, // r3 = 0x20
                0xA121_0000, // [r1] = r2
                0xA121_0120, // [r1 + r2]
                0xA121_0200, 0x30, // [r1 + 0x30]
                0xA123_0310, // heap + r3
                0xA120_0410, 0x40, // heap + 0x40
                0xA123_0510, 0x08, // heap + r3 + 8
                0xA121_1000, // [r1] with increment, twice
                0xA121_1000,
            ],
            &mut p,
        );
        assert_eq!(p.mem[0][0x00], 0x11);
        assert_eq!(p.mem[0][0x01], 0x11);
        assert_eq!(p.mem[0][0x11], 0x11);
        assert_eq!(p.mem[0][0x30], 0x11);
        assert_eq!(p.mem[1][0x20], 0x11);
        assert_eq!(p.mem[1][0x40], 0x11);
        assert_eq!(p.mem[1][0x28], 0x11);
    }

    #[test]
    fn op_c0_register_conditions() {
        // Each block stores 1 at main + target + r15 (0).
        let mut p = TestProcess::new();
        p.put(1, 0x10, &[7]);
        p.put(0, 0x18, &[7]);
        let block = |cond: u32, extra: &[u32], target: u32| -> Vec<u32> {
            [&[cond][..], extra, &[0x010F_0000, target, 1, 0x2000_0000][..]].concat()
        };
        let program = [
            &[0x4000_0000, 0, 7, 0x4001_0000, 0, 0x10, 0x4002_0000, 0, 0x1008, 0x4003_0000, 0, 0x7][..],
            &block(0xC015_0010, &[0x10], 0)[..],     // r0 == heap[0x10]
            &block(0xC015_0111, &[], 1)[..],         // r0 == heap[r1]
            &block(0xC015_0220, &[0x10], 2)[..],     // r0 == [r2 + 0x10]
            &block(0xC015_0321, &[], 3)[..],         // r0 == [r2 + r1]
            &block(0xC015_0400, &[7], 4)[..],        // r0 == 7
            &block(0xC015_0530, &[], 5)[..],         // r0 == r3
            &block(0xC016_0530, &[], 6)[..],         // r0 != r3: skipped
            &block(0xC085_0400, &[0, 7], 7)[..],     // 8 bytes
        ]
        .concat();
        run(&program, &mut p);
        assert_eq!(&p.mem[0][..8], &[1, 1, 1, 1, 1, 1, 0, 1]);
    }

    #[test]
    fn op_c1_c2_c3_registers() {
        let mut p = TestProcess::new();
        let mut vm = Vm::default();
        let program = [
            0x4000_0000, 0, 0xA, 0x4001_0000, 0, 0xB,
            0xC105_0010, // saved[5] = r0
            0xC106_0030, // r6 = 0 (clear register)
            0xC107_0500, // r7 = saved[5]
            0xC210_0003, // saved[0] = r0, saved[1] = r1
            0xC230_0003, // r0 = r1 = 0
            0xC200_0002, // r1 = saved[1]
            0xC300_0810, // statics[0x81] = r0 (0)
            0xC300_0821, // statics[0x82] = r1
            0xC300_0012, // r2 = statics[0x01]
        ];
        vm.statics[1] = 0x99;
        vm.execute(&program, &mut p, 0).unwrap();
        assert_eq!(vm.registers[..8], [0, 0xB, 0x99, 0, 0, 0, 0, 0xA]);
        assert_eq!((vm.statics[0x81], vm.statics[0x82]), (0, 0xB));
        // Saved values are cleared each run (C1 op 2 too); static registers are not.
        vm.execute(&[0xC107_0500, 0xC300_0013], &mut p, 0).unwrap();
        assert_eq!((vm.registers[7], vm.registers[3]), (0, 0x99));
        vm.execute(&[0x4000_0000, 0, 1, 0xC105_0010, 0xC105_0020, 0xC107_0500], &mut p, 0).unwrap();
        assert_eq!(vm.registers[7], 0);
    }

    #[test]
    fn op_ff_pause_resume_log() {
        let mut p = TestProcess::new();
        p.put(1, 0x10, &0x1234u16.to_le_bytes());
        run(
            &[
                0xFF00_0000, 0xFF10_0000,
                0x4000_0000, 0, 0xFF, // r0
                0xFFF8_1400, // log 1: r0, 8 bytes
                0xFFF1_2400, // log 2: r0, 1 byte
                0xFFF2_3010, 0x10, // log 3: heap[0x10], 2 bytes
                0xFFF3_4400, // log 4: r0 at a width no value has: 0
            ],
            &mut p,
        );
        assert_eq!(p.pauses, 0);
        assert_eq!(p.logs, vec![(1, 0xFF), (2, 0xFF), (3, 0x1234), (4, 0)]);
    }

    #[test]
    fn bad_opcodes_end_the_run() {
        let mut p = TestProcess::new();
        // 0xB, then a store that must not run; a cut-off opcode at the end.
        run(&[0x0100_0000, 0, 1, 0xB000_0000, 0x0100_0000, 1, 1], &mut p);
        assert_eq!(&p.mem[0][..2], &[1, 0]);
        run(&[0x0100_0000, 2, 1, 0x0400_0000, 3], &mut p);
        assert_eq!(&p.mem[0][..4], &[1, 0, 1, 0]);
        // An else with nothing open: dmnt aborts; here the run ends.
        let mut vm = Vm::default();
        assert!(vm.execute(&[0x2100_0000, 0x0100_0000, 5, 1], &mut p, 0).is_err());
        assert_eq!(p.mem[0][5], 0);
        // A lone end is a no-op.
        run(&[0x2000_0000, 0x0100_0000, 6, 1], &mut p);
        assert_eq!(p.mem[0][6], 1);
    }
}
