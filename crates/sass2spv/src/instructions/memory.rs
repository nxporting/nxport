// Global, local, shared and constant memory, and atomics (InstEmitMemory).

use super::helper::*;
use crate::decoders::inst_decoders::*;
use crate::decoders::register::{Register, RegisterType, REGISTER_ZERO_INDEX};
use crate::ir::{Instruction as I, OperandId, OperandType, StorageKind};
use crate::tri;
use crate::translation::emitter::{Emit, EmitterContext as Ctx};
use crate::types::ShaderStage;

pub fn atom(c: &mut Ctx) {
    let op = InstAtom(c.raw_op());
    let s_offset = (op.imm20() << 12) >> 12;
    let (addr_low, addr_high) = get_40_bits_address(c, Register::new(op.src_a(), RegisterType::Gpr), op.e(), s_offset);
    let value = get_src_reg(c, op.src_b(), false);
    let res = emit_atomic_op(c, StorageKind::GlobalMemory, op.op(), op.size(), addr_low, addr_high, value);
    let d = get_dest(c, op.dest());
    c.copy_to(d, res);
}

pub fn atoms(c: &mut Ctx) {
    if c.stage() != ShaderStage::Compute {
        c.log(&format!("Atoms instruction is not valid on \"{}\" stage.", c.stage()));
        return;
    }
    let op = InstAtoms(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    let k2 = c.const_i(2);
    let offset = c.shift_right_u32(a, k2);
    let s_offset = (op.imm22() << 10) >> 10;
    let k = c.const_i(s_offset);
    let offset = c.iadd(offset, k);
    let value = get_src_reg(c, op.src_b(), false);
    let size = match op.atoms_size() {
        AtomsSize::S32 => AtomSize::S32,
        AtomsSize::U64 => AtomSize::U64,
        AtomsSize::S64 => AtomSize::S64,
        _ => AtomSize::U32,
    };
    let id = c.const_i(c.rm.shared_memory_id);
    let res = emit_atomic_op(c, StorageKind::SharedMemory, op.atom_op(), size, id, offset, value);
    let d = get_dest(c, op.dest());
    c.copy_to(d, res);
}

pub fn ldc(c: &mut Ctx) {
    let op = InstLdc(c.raw_op());
    if op.ls_size().0 > LsSize2::B64.0 {
        c.log(&format!("Invalid LDC size: {}.", op.ls_size().0));
        return;
    }
    let small = op.ls_size().0 < LsSize2::B32.0;
    let count = if op.ls_size() == LsSize2::B64 { 2 } else { 1 };
    let mut slot = c.const_i(op.cbuf_slot());
    let mut a = get_src_reg(c, op.src_a(), false);
    if op.address_mode() == AddressMode::IS || op.address_mode() == AddressMode::ISL {
        let (k16, k16b) = (c.const_i(16), c.const_i(16));
        let hi = c.bitfield_extract_u32(a, k16, k16b);
        slot = c.iadd(slot, hi);
        let m = c.const_i(0xffff);
        a = c.bitwise_and(a, m);
    }
    let k = c.const_i(imm16_to_sint(op.cbuf_offset()));
    let addr = c.iadd(a, k);
    let k2 = c.const_i(2);
    let word_offset = c.shift_right_u32(addr, k2);
    for index in 0..count {
        let dest = Register::new(op.dest() + index, RegisterType::Gpr);
        if dest.is_rz() {
            break;
        }
        let ki = c.const_i(index);
        let offset = c.iadd(word_offset, ki);
        let mut value = tri!(c, emit_load_constant(c, slot, offset));
        if small {
            let bit_offset = get_bit_offset(c, addr);
            value = extract_small_int(c, LsSize(op.ls_size().0), bit_offset, value);
        }
        let d = c.ir.register(dest);
        c.copy_to(d, value);
    }
}

pub fn ldg(c: &mut Ctx) {
    let op = InstLdg(c.raw_op());
    emit_ldg(c, op.ls_size(), op.src_a(), op.dest(), imm24_to_sint(op.imm24()), op.e());
}

pub fn ldl(c: &mut Ctx) {
    let op = InstLdl(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    emit_load(c, StorageKind::LocalMemory, op.ls_size(), a, op.dest(), imm24_to_sint(op.imm24()));
}

pub fn lds(c: &mut Ctx) {
    if c.stage() != ShaderStage::Compute {
        c.log(&format!("Lds instruction is not valid on \"{}\" stage.", c.stage()));
        return;
    }
    let op = InstLds(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    emit_load(c, StorageKind::SharedMemory, op.ls_size(), a, op.dest(), imm24_to_sint(op.imm24()));
}

pub fn red(c: &mut Ctx) {
    let op = InstRed(c.raw_op());
    let (addr_low, addr_high) = get_40_bits_address(c, Register::new(op.src_a(), RegisterType::Gpr), op.e(), op.imm20());
    let v = get_dest(c, op.src_b());
    emit_atomic_op(c, StorageKind::GlobalMemory, AtomOp(op.red_op().0), op.red_size(), addr_low, addr_high, v);
}

pub fn stg(c: &mut Ctx) {
    let op = InstStg(c.raw_op());
    emit_stg(c, op.ls_size(), op.src_a(), op.dest(), imm24_to_sint(op.imm24()), op.e());
}

pub fn stl(c: &mut Ctx) {
    let op = InstStl(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    emit_store(c, StorageKind::LocalMemory, op.ls_size(), a, op.dest(), imm24_to_sint(op.imm24()));
}

pub fn sts(c: &mut Ctx) {
    if c.stage() != ShaderStage::Compute {
        c.log(&format!("Sts instruction is not valid on \"{}\" stage.", c.stage()));
        return;
    }
    let op = InstSts(c.raw_op());
    let a = get_src_reg(c, op.src_a(), false);
    emit_store(c, StorageKind::SharedMemory, op.ls_size(), a, op.dest(), imm24_to_sint(op.imm24()));
}

fn emit_load_constant(c: &mut Ctx, slot: OperandId, offset: OperandId) -> Result<OperandId, String> {
    let k2 = c.const_i(2);
    let vec_index = c.shift_right_u32(offset, k2);
    let k3 = c.const_i(3);
    let elem_index = c.bitwise_and(offset, k3);
    if c.ir.ty(slot) == OperandType::Constant {
        let binding = c.rm.constant_buffer_binding(&c.tc.gpu, c.ir.value(slot))?;
        let z = c.const_i(0);
        return Ok(c.load_b(StorageKind::ConstantBuffer, binding, &[z, vec_index, elem_index]));
    }
    let mut value = c.const_i(0);
    let mut cb_use_mask = c.tc.gpu.constant_buffer_use();
    while cb_use_mask != 0 {
        let cb_index = cb_use_mask.trailing_zeros() as i32;
        let binding = c.rm.constant_buffer_binding(&c.tc.gpu, cb_index)?;
        let k = c.const_i(cb_index);
        let is_current = c.icompare_equal(slot, k);
        let z = c.const_i(0);
        let current = c.load_b(StorageKind::ConstantBuffer, binding, &[z, vec_index, elem_index]);
        value = c.conditional_select(is_current, current, value);
        cb_use_mask &= !(1u32 << cb_index);
    }
    Ok(value)
}

#[allow(clippy::too_many_arguments)]
fn emit_atomic_op(c: &mut Ctx, sk: StorageKind, op: AtomOp, ty: AtomSize, e0: OperandId, e1: OperandId, value: OperandId) -> OperandId {
    let mut res = c.const_i(0);
    let int32 = ty == AtomSize::S32 || ty == AtomSize::U32;
    let invalid = |c: &mut Ctx| c.log(&format!("Invalid reduction type: {}.", ty.0));
    match op {
        AtomOp::ADD => {
            if int32 { res = c.atomic(I::ATOMIC_ADD, sk, &[e0, e1, value]) } else { invalid(c) }
        }
        AtomOp::MIN => {
            if ty == AtomSize::S32 {
                res = c.atomic(I::ATOMIC_MIN_S32, sk, &[e0, e1, value]);
            } else if ty == AtomSize::U32 {
                res = c.atomic(I::ATOMIC_MIN_U32, sk, &[e0, e1, value]);
            } else {
                invalid(c);
            }
        }
        AtomOp::MAX => {
            if ty == AtomSize::S32 {
                res = c.atomic(I::ATOMIC_MAX_S32, sk, &[e0, e1, value]);
            } else if ty == AtomSize::U32 {
                res = c.atomic(I::ATOMIC_MAX_U32, sk, &[e0, e1, value]);
            } else {
                invalid(c);
            }
        }
        AtomOp::AND => {
            if int32 { res = c.atomic(I::ATOMIC_AND, sk, &[e0, e1, value]) } else { invalid(c) }
        }
        AtomOp::OR => {
            if int32 { res = c.atomic(I::ATOMIC_OR, sk, &[e0, e1, value]) } else { invalid(c) }
        }
        AtomOp::XOR => {
            if int32 { res = c.atomic(I::ATOMIC_XOR, sk, &[e0, e1, value]) } else { invalid(c) }
        }
        AtomOp::EXCH => {
            if int32 { res = c.atomic(I::ATOMIC_SWAP, sk, &[e0, e1, value]) } else { invalid(c) }
        }
        _ => c.log(&format!("Invalid atomic operation: {}.", op.0)),
    }
    res
}

fn emit_load(c: &mut Ctx, sk: StorageKind, size: LsSize2, a: OperandId, rd: i32, offset: i32) {
    if size.0 > LsSize2::B128.0 {
        c.log(&format!("Invalid load size: {}.", size.0));
        return;
    }
    let id = if sk == StorageKind::LocalMemory { c.rm.local_memory_id } else { c.rm.shared_memory_id };
    let small = size.0 < LsSize2::B32.0;
    let count = match size {
        LsSize2::B64 => 2,
        LsSize2::B128 => 4,
        _ => 1,
    };
    let base_offset = c.copy(a);
    for index in 0..count {
        let dest = Register::new(rd + index, RegisterType::Gpr);
        if dest.is_rz() {
            break;
        }
        let k = c.const_i(offset + index * 4);
        let byte_offset = c.iadd(base_offset, k);
        // Word offset = byte offset / 4.
        let k2 = c.const_i(2);
        let word_offset = c.shift_right_u32(byte_offset, k2);
        let bit_offset = get_bit_offset(c, byte_offset);
        let mut value = c.load_b(sk, id, &[word_offset]);
        if small {
            value = extract_small_int(c, LsSize(size.0), bit_offset, value);
        }
        let d = c.ir.register(dest);
        c.copy_to(d, value);
    }
}

fn emit_ldg(c: &mut Ctx, size: LsSize, ra: i32, rd: i32, offset: i32, extended: bool) {
    let count = vector_count(size);
    let sk = storage_kind(size);
    let (_, addr_high) = get_40_bits_address(c, Register::new(ra, RegisterType::Gpr), extended, offset);
    let reg = c.ir.register_raw(Register::new(ra, RegisterType::Gpr));
    let a = c.copy(reg);
    for index in 0..count {
        let dest = Register::new(rd + index, RegisterType::Gpr);
        if dest.is_rz() {
            break;
        }
        let k = c.const_i(offset + index * 4);
        let low = c.iadd(a, k);
        let value = c.load(sk, &[low, addr_high]);
        let d = c.ir.register(dest);
        c.copy_to(d, value);
    }
}

fn emit_store(c: &mut Ctx, sk: StorageKind, size: LsSize2, a: OperandId, rd: i32, offset: i32) {
    if size.0 > LsSize2::B128.0 {
        c.log(&format!("Invalid store size: {}.", size.0));
        return;
    }
    let id = if sk == StorageKind::LocalMemory { c.rm.local_memory_id } else { c.rm.shared_memory_id };
    let small = size.0 < LsSize2::B32.0;
    let count = match size {
        LsSize2::B64 => 2,
        LsSize2::B128 => 4,
        _ => 1,
    };
    let base_offset = c.copy(a);
    for index in 0..count {
        let is_rz = rd + index >= REGISTER_ZERO_INDEX;
        let mut value = c.gpr(if is_rz { rd } else { rd + index });
        let k = c.const_i(offset + index * 4);
        let byte_offset = c.iadd(base_offset, k);
        let k2 = c.const_i(2);
        let word_offset = c.shift_right_u32(byte_offset, k2);
        let bit_offset = get_bit_offset(c, byte_offset);
        if small && sk == StorageKind::LocalMemory {
            let word = c.load_b(sk, id, &[word_offset]);
            value = insert_small_int(c, LsSize(size.0), bit_offset, word, value);
        }
        if sk == StorageKind::LocalMemory {
            c.store_b(sk, id, &[word_offset, value]);
        } else if sk == StorageKind::SharedMemory {
            match size {
                LsSize2::U8 | LsSize2::S8 => c.store_b(StorageKind::SharedMemory8, id, &[byte_offset, value]),
                LsSize2::U16 | LsSize2::S16 => c.store_b(StorageKind::SharedMemory16, id, &[byte_offset, value]),
                _ => c.store_b(sk, id, &[word_offset, value]),
            }
        }
    }
}

fn emit_stg(c: &mut Ctx, size: LsSize2, ra: i32, rd: i32, offset: i32, extended: bool) {
    if size.0 > LsSize2::B128.0 {
        c.log(&format!("Invalid store size: {}.", size.0));
        return;
    }
    let count = vector_count(LsSize(size.0));
    let sk = storage_kind(LsSize(size.0));
    let (_, addr_high) = get_40_bits_address(c, Register::new(ra, RegisterType::Gpr), extended, offset);
    let reg = c.ir.register_raw(Register::new(ra, RegisterType::Gpr));
    let a = c.copy(reg);
    for index in 0..count {
        let is_rz = rd + index >= REGISTER_ZERO_INDEX;
        let value = c.gpr(if is_rz { rd } else { rd + index });
        let k = c.const_i(offset + index * 4);
        let low = c.iadd(a, k);
        c.store(sk, &[low, addr_high, value]);
    }
}

fn storage_kind(size: LsSize) -> StorageKind {
    match size {
        LsSize::U8 => StorageKind::GlobalMemoryU8,
        LsSize::S8 => StorageKind::GlobalMemoryS8,
        LsSize::U16 => StorageKind::GlobalMemoryU16,
        LsSize::S16 => StorageKind::GlobalMemoryS16,
        _ => StorageKind::GlobalMemory,
    }
}

fn vector_count(size: LsSize) -> i32 {
    match size {
        LsSize::B64 => 2,
        LsSize::B128 | LsSize::UB128 => 4,
        _ => 1,
    }
}

fn get_40_bits_address(c: &mut Ctx, ra: Register, extended: bool, offset: i32) -> (OperandId, OperandId) {
    let mut addr_low = c.ir.register(ra);
    let mut addr_high = if extended && !ra.is_rz() { c.gpr(ra.index + 1) } else { c.const_i(0) };
    let offs = c.const_i(offset);
    addr_low = c.iadd(addr_low, offs);
    if extended {
        let carry = c.icompare_less_unsigned(addr_low, offs);
        let (one, zero) = (c.const_i(1), c.const_i(0));
        let sel = c.conditional_select(carry, one, zero);
        addr_high = c.iadd(addr_high, sel);
    }
    (addr_low, addr_high)
}

/// (offset & 3) * 8: addresses are aligned to the type, so unaligned ones need no care.
fn get_bit_offset(c: &mut Ctx, base_offset: OperandId) -> OperandId {
    let k3 = c.const_i(3);
    let a = c.bitwise_and(base_offset, k3);
    let k3b = c.const_i(3);
    c.shift_left(a, k3b)
}

fn extract_small_int(c: &mut Ctx, size: LsSize, bit_offset: OperandId, value: OperandId) -> OperandId {
    let value = c.shift_right_u32(value, bit_offset);
    match size {
        LsSize::U8 => zero_extend_to_32(c, value, 8),
        LsSize::U16 => zero_extend_to_32(c, value, 16),
        LsSize::S8 => sign_extend_to_32(c, value, 8),
        LsSize::S16 => sign_extend_to_32(c, value, 16),
        _ => value,
    }
}

fn insert_small_int(c: &mut Ctx, size: LsSize, bit_offset: OperandId, word: OperandId, value: OperandId) -> OperandId {
    match size {
        LsSize::U8 | LsSize::S8 => {
            let m = c.const_i(0xff);
            let v = c.bitwise_and(value, m);
            let k = c.const_i(8);
            c.bitfield_insert(word, v, bit_offset, k)
        }
        LsSize::U16 | LsSize::S16 => {
            let m = c.const_i(0xffff);
            let v = c.bitwise_and(value, m);
            let k = c.const_i(16);
            c.bitfield_insert(word, v, bit_offset, k)
        }
        _ => value,
    }
}
