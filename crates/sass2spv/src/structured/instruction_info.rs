// Operand types per instruction (StructuredIr.InstructionInfo).

use crate::ir::Instruction as I;
use crate::translation::definitions::AggregateType as A;

type R<T> = Result<T, String>;

/// The table entry: destination type and source types; None for instructions the original leaves out (a default
/// entry: destination Scalar, sources null).
fn info(inst: I) -> Option<(A, &'static [A])> {
    Some(match inst.base() {
        I::ATOMIC_ADD => (A::U32, &[A::S32, A::S32, A::U32]),
        I::ATOMIC_AND => (A::U32, &[A::S32, A::S32, A::U32]),
        I::ATOMIC_COMPARE_AND_SWAP => (A::U32, &[A::S32, A::S32, A::U32, A::U32]),
        I::ATOMIC_MAX_S32 => (A::S32, &[A::S32, A::S32, A::S32]),
        I::ATOMIC_MAX_U32 => (A::U32, &[A::S32, A::S32, A::U32]),
        I::ATOMIC_MIN_S32 => (A::S32, &[A::S32, A::S32, A::S32]),
        I::ATOMIC_MIN_U32 => (A::U32, &[A::S32, A::S32, A::U32]),
        I::ATOMIC_OR => (A::U32, &[A::S32, A::S32, A::U32]),
        I::ATOMIC_SWAP => (A::U32, &[A::S32, A::S32, A::U32]),
        I::ATOMIC_XOR => (A::U32, &[A::S32, A::S32, A::U32]),
        I::ABSOLUTE => (A::SCALAR, &[A::SCALAR]),
        I::ADD => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::BALLOT => (A::U32, &[A::BOOL]),
        I::BIT_COUNT => (A::S32, &[A::S32]),
        I::BITFIELD_EXTRACT_S32 => (A::S32, &[A::S32, A::S32, A::S32]),
        I::BITFIELD_EXTRACT_U32 => (A::U32, &[A::U32, A::S32, A::S32]),
        I::BITFIELD_INSERT => (A::S32, &[A::S32, A::S32, A::S32, A::S32]),
        I::BITFIELD_REVERSE => (A::S32, &[A::S32]),
        I::BITWISE_AND => (A::S32, &[A::S32, A::S32]),
        I::BITWISE_EXCLUSIVE_OR => (A::S32, &[A::S32, A::S32]),
        I::BITWISE_NOT => (A::S32, &[A::S32]),
        I::BITWISE_OR => (A::S32, &[A::S32, A::S32]),
        I::BRANCH_IF_TRUE => (A::VOID, &[A::BOOL]),
        I::BRANCH_IF_FALSE => (A::VOID, &[A::BOOL]),
        I::CALL => (A::SCALAR, &[]),
        I::CEILING => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::CLAMP => (A::SCALAR, &[A::SCALAR, A::SCALAR, A::SCALAR]),
        I::CLAMP_U32 => (A::U32, &[A::U32, A::U32, A::U32]),
        I::COMPARE_EQUAL => (A::BOOL, &[A::SCALAR, A::SCALAR]),
        I::COMPARE_GREATER => (A::BOOL, &[A::SCALAR, A::SCALAR]),
        I::COMPARE_GREATER_OR_EQUAL => (A::BOOL, &[A::SCALAR, A::SCALAR]),
        I::COMPARE_GREATER_OR_EQUAL_U32 => (A::BOOL, &[A::U32, A::U32]),
        I::COMPARE_GREATER_U32 => (A::BOOL, &[A::U32, A::U32]),
        I::COMPARE_LESS => (A::BOOL, &[A::SCALAR, A::SCALAR]),
        I::COMPARE_LESS_OR_EQUAL => (A::BOOL, &[A::SCALAR, A::SCALAR]),
        I::COMPARE_LESS_OR_EQUAL_U32 => (A::BOOL, &[A::U32, A::U32]),
        I::COMPARE_LESS_U32 => (A::BOOL, &[A::U32, A::U32]),
        I::COMPARE_NOT_EQUAL => (A::BOOL, &[A::SCALAR, A::SCALAR]),
        I::CONDITIONAL_SELECT => (A::SCALAR, &[A::BOOL, A::SCALAR, A::SCALAR]),
        I::CONVERT_FP32_TO_FP64 => (A::FP64, &[A::FP32]),
        I::CONVERT_FP64_TO_FP32 => (A::FP32, &[A::FP64]),
        I::CONVERT_FP32_TO_S32 => (A::S32, &[A::FP32]),
        I::CONVERT_FP32_TO_U32 => (A::U32, &[A::FP32]),
        I::CONVERT_FP64_TO_S32 => (A::S32, &[A::FP64]),
        I::CONVERT_FP64_TO_U32 => (A::U32, &[A::FP64]),
        I::CONVERT_S32_TO_FP32 => (A::FP32, &[A::S32]),
        I::CONVERT_S32_TO_FP64 => (A::FP64, &[A::S32]),
        I::CONVERT_U32_TO_FP32 => (A::FP32, &[A::U32]),
        I::CONVERT_U32_TO_FP64 => (A::FP64, &[A::U32]),
        I::COSINE => (A::SCALAR, &[A::SCALAR]),
        I::DDX => (A::FP32, &[A::FP32]),
        I::DDY => (A::FP32, &[A::FP32]),
        I::DIVIDE => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::EXPONENT_B2 => (A::SCALAR, &[A::SCALAR]),
        I::FIND_LSB => (A::S32, &[A::S32]),
        I::FIND_MSB_S32 => (A::S32, &[A::S32]),
        I::FIND_MSB_U32 => (A::S32, &[A::U32]),
        I::FLOOR => (A::SCALAR, &[A::SCALAR]),
        I::FUSED_MULTIPLY_ADD => (A::SCALAR, &[A::SCALAR, A::SCALAR, A::SCALAR]),
        I::IMAGE_LOAD => (A::FP32, &[]),
        I::IMAGE_STORE => (A::VOID, &[]),
        I::IMAGE_ATOMIC => (A::S32, &[]),
        I::IS_NAN => (A::BOOL, &[A::SCALAR]),
        I::LOAD => (A::FP32, &[]),
        I::LOD => (A::FP32, &[]),
        I::LOGARITHM_B2 => (A::SCALAR, &[A::SCALAR]),
        I::LOGICAL_AND => (A::BOOL, &[A::BOOL, A::BOOL]),
        I::LOGICAL_EXCLUSIVE_OR => (A::BOOL, &[A::BOOL, A::BOOL]),
        I::LOGICAL_NOT => (A::BOOL, &[A::BOOL]),
        I::LOGICAL_OR => (A::BOOL, &[A::BOOL, A::BOOL]),
        I::MAXIMUM => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::MAXIMUM_U32 => (A::U32, &[A::U32, A::U32]),
        I::MINIMUM => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::MINIMUM_U32 => (A::U32, &[A::U32, A::U32]),
        I::MODULO => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::MULTIPLY => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::MULTIPLY_HIGH_S32 => (A::S32, &[A::S32, A::S32]),
        I::MULTIPLY_HIGH_U32 => (A::U32, &[A::U32, A::U32]),
        I::NEGATE => (A::SCALAR, &[A::SCALAR]),
        I::PACK_DOUBLE_2X32 => (A::FP64, &[A::U32, A::U32]),
        I::PACK_HALF_2X16 => (A::U32, &[A::FP32, A::FP32]),
        I::RECIPROCAL_SQUARE_ROOT => (A::SCALAR, &[A::SCALAR]),
        I::RETURN => (A::VOID, &[A::U32]),
        I::ROUND => (A::SCALAR, &[A::SCALAR]),
        I::SHIFT_LEFT => (A::S32, &[A::S32, A::S32]),
        I::SHIFT_RIGHT_S32 => (A::S32, &[A::S32, A::S32]),
        I::SHIFT_RIGHT_U32 => (A::U32, &[A::U32, A::S32]),
        I::SHUFFLE => (A::FP32, &[A::FP32, A::U32]),
        I::SHUFFLE_DOWN => (A::FP32, &[A::FP32, A::U32]),
        I::SHUFFLE_UP => (A::FP32, &[A::FP32, A::U32]),
        I::SHUFFLE_XOR => (A::FP32, &[A::FP32, A::U32]),
        I::SINE => (A::SCALAR, &[A::SCALAR]),
        I::SQUARE_ROOT => (A::SCALAR, &[A::SCALAR]),
        I::STORE => (A::VOID, &[]),
        I::SUBTRACT => (A::SCALAR, &[A::SCALAR, A::SCALAR]),
        I::SWIZZLE_ADD => (A::FP32, &[A::FP32, A::FP32, A::S32]),
        I::TEXTURE_SAMPLE => (A::FP32, &[]),
        I::TEXTURE_QUERY_SAMPLES => (A::S32, &[A::S32]),
        I::TEXTURE_QUERY_SIZE => (A::S32, &[A::S32, A::S32]),
        I::TRUNCATE => (A::SCALAR, &[A::SCALAR]),
        I::UNPACK_DOUBLE_2X32 => (A::U32, &[A::FP64]),
        I::UNPACK_HALF_2X16 => (A::FP32, &[A::U32]),
        I::VECTOR_EXTRACT => (A::SCALAR, &[A::VECTOR4, A::S32]),
        I::VOTE_ALL => (A::BOOL, &[A::BOOL]),
        I::VOTE_ALL_EQUAL => (A::BOOL, &[A::BOOL]),
        I::VOTE_ANY => (A::BOOL, &[A::BOOL]),
        _ => return None,
    })
}

pub fn dest_var_type(inst: I) -> R<A> {
    let dest = info(inst).map_or(A::SCALAR, |i| i.0);
    final_var_type(dest, inst)
}

pub fn src_var_type(inst: I, index: usize) -> R<A> {
    // The type does not depend on the source index yet.
    if matches!(inst, I::IMAGE_LOAD | I::IMAGE_STORE | I::IMAGE_ATOMIC | I::LOD | I::TEXTURE_SAMPLE) {
        return Ok(A::FP32);
    } else if inst == I::CALL || inst == I::LOAD || inst == I::STORE || inst.is_atomic() {
        return Ok(A::S32);
    }
    let Some((_, srcs)) = info(inst) else { return Err(crate::translation::optimizations::utils::NRE.into()) };
    let t = *srcs.get(index).ok_or("System.IndexOutOfRangeException: Index was outside the bounds of the array.")?;
    final_var_type(t, inst)
}

fn final_var_type(ty: A, inst: I) -> R<A> {
    if ty == A::SCALAR {
        Ok(if inst.0 & I::FP32.0 != 0 {
            A::FP32
        } else if inst.0 & I::FP64.0 != 0 {
            A::FP64
        } else {
            A::S32
        })
    } else if ty == A::VOID {
        Err(format!("System.ArgumentException: Invalid operand for instruction \"{}\".", inst.name()))
    } else {
        Ok(ty)
    }
}

pub fn is_unary(inst: I) -> R<bool> {
    if inst == I::COPY {
        return Ok(true);
    } else if inst == I::TEXTURE_SAMPLE {
        return Ok(false);
    }
    let Some((_, srcs)) = info(inst) else { return Err(crate::translation::optimizations::utils::NRE.into()) };
    Ok(srcs.len() == 1)
}
