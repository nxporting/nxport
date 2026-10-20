// Operations whose sources are all constants become copies of the result (ConstantFolding).

use crate::instructions::helper::half_to_f32;
use crate::ir::{ir_consts, Instruction as I, Ir, NodeId, OperandType, StorageKind};
use crate::translation::resource_manager::ResourceManager;

fn b(v: bool) -> i32 {
    if v { ir_consts::TRUE } else { ir_consts::FALSE }
}

/// Math.Clamp, which throws when min > max.
fn clamp_i(x: i32, lo: i32, hi: i32) -> Result<i32, String> {
    if lo > hi {
        return Err(format!("System.ArgumentException: '{lo}' cannot be greater than {hi}."));
    }
    Ok(x.clamp(lo, hi))
}

fn clamp_u(x: u32, lo: u32, hi: u32) -> Result<u32, String> {
    if lo > hi {
        return Err(format!("System.ArgumentException: '{lo}' cannot be greater than {hi}."));
    }
    Ok(x.clamp(lo, hi))
}

/// Math.Clamp(float): NaN passes through; min > max throws.
fn clamp_f(x: f32, lo: f32, hi: f32) -> Result<f32, String> {
    if lo > hi {
        return Err(format!("System.ArgumentException: '{lo}' cannot be greater than {hi}."));
    }
    Ok(if x < lo {
        lo
    } else if x > hi {
        hi
    } else {
        x
    })
}

/// int.Extract(lsb, length) from Ryujinx.Common (C# shift counts are masked).
fn extract(value: i32, lsb: i32, length: i32) -> i32 {
    let v = (value as u32) >> (lsb & 31);
    let mask = u32::MAX >> ((32 - length) & 31);
    (v & mask) as i32
}

pub fn run_pass(ir: &mut Ir, rm: &ResourceManager, n: NodeId) -> Result<(), String> {
    let count = ir.sources_count(n);
    if !(0..count).all(|i| ir.ty(ir.src(n, i)) == OperandType::Constant) {
        return Ok(());
    }
    let v = |ir: &Ir, i: usize| ir.value(ir.src(n, i));
    let f = |ir: &Ir, i: usize| ir.opnd(ir.src(n, i)).as_float();
    let inst = ir.inst(n);

    let copy_i = |ir: &mut Ir, x: i32| {
        let c = ir.const_i(x);
        ir.turn_into_copy(n, c);
    };
    let copy_f = |ir: &mut Ir, x: f32| {
        let c = ir.const_f(x);
        ir.turn_into_copy(n, c);
    };

    match inst {
        I::ADD => copy_i(ir, v(ir, 0).wrapping_add(v(ir, 1))),
        I::BIT_COUNT => copy_i(ir, v(ir, 0).count_ones() as i32),
        I::BITWISE_AND => copy_i(ir, v(ir, 0) & v(ir, 1)),
        I::BITWISE_EXCLUSIVE_OR => copy_i(ir, v(ir, 0) ^ v(ir, 1)),
        I::BITWISE_NOT => copy_i(ir, !v(ir, 0)),
        I::BITWISE_OR => copy_i(ir, v(ir, 0) | v(ir, 1)),
        I::BITFIELD_EXTRACT_S32 => {
            let value = extract(v(ir, 0), v(ir, 1), v(ir, 2));
            let shift = (32 - v(ir, 2)) as u32;
            copy_i(ir, value.wrapping_shl(shift).wrapping_shr(shift));
        }
        I::BITFIELD_EXTRACT_U32 => copy_i(ir, extract(v(ir, 0), v(ir, 1), v(ir, 2))),
        I::CLAMP => {
            let r = clamp_i(v(ir, 0), v(ir, 1), v(ir, 2))?;
            copy_i(ir, r);
        }
        I::CLAMP_U32 => {
            let r = clamp_u(v(ir, 0) as u32, v(ir, 1) as u32, v(ir, 2) as u32)?;
            copy_i(ir, r as i32);
        }
        I::COMPARE_EQUAL => copy_i(ir, b(v(ir, 0) == v(ir, 1))),
        I::COMPARE_GREATER => copy_i(ir, b(v(ir, 0) > v(ir, 1))),
        I::COMPARE_GREATER_OR_EQUAL => copy_i(ir, b(v(ir, 0) >= v(ir, 1))),
        I::COMPARE_GREATER_OR_EQUAL_U32 => copy_i(ir, b(v(ir, 0) as u32 >= v(ir, 1) as u32)),
        I::COMPARE_GREATER_U32 => copy_i(ir, b(v(ir, 0) as u32 > v(ir, 1) as u32)),
        I::COMPARE_LESS => copy_i(ir, b(v(ir, 0) < v(ir, 1))),
        I::COMPARE_LESS_OR_EQUAL => copy_i(ir, b(v(ir, 0) <= v(ir, 1))),
        I::COMPARE_LESS_OR_EQUAL_U32 => copy_i(ir, b(v(ir, 0) as u32 <= v(ir, 1) as u32)),
        I::COMPARE_LESS_U32 => copy_i(ir, b((v(ir, 0) as u32) < v(ir, 1) as u32)),
        I::COMPARE_NOT_EQUAL => copy_i(ir, b(v(ir, 0) != v(ir, 1))),
        I::DIVIDE => {
            let (x, y) = (v(ir, 0), v(ir, 1));
            if y == 0 {
                copy_i(ir, 0);
            } else {
                let r = x.checked_div(y).ok_or("System.OverflowException: Arithmetic operation resulted in an overflow.")?;
                copy_i(ir, r);
            }
        }
        _ if inst == I::FP32 | I::ADD => copy_f(ir, f(ir, 0) + f(ir, 1)),
        _ if inst == I::FP32 | I::CLAMP => {
            let r = clamp_f(f(ir, 0), f(ir, 1), f(ir, 2))?;
            copy_f(ir, r);
        }
        _ if inst == I::FP32 | I::COMPARE_EQUAL => copy_i(ir, b(f(ir, 0) == f(ir, 1))),
        _ if inst == I::FP32 | I::COMPARE_GREATER => copy_i(ir, b(f(ir, 0) > f(ir, 1))),
        _ if inst == I::FP32 | I::COMPARE_GREATER_OR_EQUAL => copy_i(ir, b(f(ir, 0) >= f(ir, 1))),
        _ if inst == I::FP32 | I::COMPARE_LESS => copy_i(ir, b(f(ir, 0) < f(ir, 1))),
        _ if inst == I::FP32 | I::COMPARE_LESS_OR_EQUAL => copy_i(ir, b(f(ir, 0) <= f(ir, 1))),
        _ if inst == I::FP32 | I::COMPARE_NOT_EQUAL => copy_i(ir, b(f(ir, 0) != f(ir, 1))),
        _ if inst == I::FP32 | I::DIVIDE => copy_f(ir, f(ir, 0) / f(ir, 1)),
        _ if inst == I::FP32 | I::MULTIPLY => copy_f(ir, f(ir, 0) * f(ir, 1)),
        _ if inst == I::FP32 | I::NEGATE => copy_f(ir, -f(ir, 0)),
        _ if inst == I::FP32 | I::SUBTRACT => copy_f(ir, f(ir, 0) - f(ir, 1)),
        I::IS_NAN => copy_i(ir, b(f(ir, 0).is_nan())),
        I::LOAD => {
            if ir.op(n).storage_kind == StorageKind::ConstantBuffer && count == 4 {
                let binding = v(ir, 0);
                let field_index = v(ir, 1);
                if let Some(cbuf_slot) = rm.try_get_constant_buffer_slot(binding) {
                    if field_index == 0 {
                        let cbuf_offset = v(ir, 2) * 4 + v(ir, 3);
                        let c = ir.cbuf(cbuf_slot, cbuf_offset);
                        ir.turn_into_copy(n, c);
                    }
                }
            }
        }
        I::MAXIMUM => copy_i(ir, v(ir, 0).max(v(ir, 1))),
        I::MAXIMUM_U32 => copy_i(ir, (v(ir, 0) as u32).max(v(ir, 1) as u32) as i32),
        I::MINIMUM => copy_i(ir, v(ir, 0).min(v(ir, 1))),
        I::MINIMUM_U32 => copy_i(ir, (v(ir, 0) as u32).min(v(ir, 1) as u32) as i32),
        I::MULTIPLY => copy_i(ir, v(ir, 0).wrapping_mul(v(ir, 1))),
        I::NEGATE => copy_i(ir, v(ir, 0).wrapping_neg()),
        I::SHIFT_LEFT => copy_i(ir, v(ir, 0).wrapping_shl(v(ir, 1) as u32)),
        I::SHIFT_RIGHT_S32 => copy_i(ir, v(ir, 0).wrapping_shr(v(ir, 1) as u32)),
        I::SHIFT_RIGHT_U32 => copy_i(ir, (v(ir, 0) as u32).wrapping_shr(v(ir, 1) as u32) as i32),
        I::SUBTRACT => copy_i(ir, v(ir, 0).wrapping_sub(v(ir, 1))),
        I::UNPACK_HALF_2X16 => {
            let index = ir.op(n).index;
            let value = v(ir, 0).wrapping_shr((index * 16) as u32) & 0xffff;
            copy_f(ir, half_to_f32(value as u16));
        }
        _ => {}
    }
    Ok(())
}
