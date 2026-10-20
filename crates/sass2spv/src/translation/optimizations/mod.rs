// Optimisation passes over a function's SSA form (Optimizations.Optimizer and the passes it runs).

pub mod bindless_elimination;
pub mod bindless_to_array;
pub mod constant_folding;
pub mod global_to_storage;
pub mod simplification;
pub mod utils;
pub mod xmad;

use std::collections::VecDeque;

use crate::ir::{BlockId, Instruction as I, Ir, IoVariable, NodeId, OperandType};
use crate::translation::TransformContext;
use crate::types::ShaderStage;

pub fn run_pass(ctx: &mut TransformContext) -> Result<(), String> {
    for &block in ctx.blocks {
        xmad::run_pass(ctx.ir, block)?;
    }
    run_optimization_passes(ctx.ir, ctx.blocks, ctx.rm)?;

    // Not all of these are optimisations.
    global_to_storage::run_pass(ctx)?;

    // These look for specific patterns and only need to run once. The host supports doubles, so no conversion to
    // floats is needed (DoubleToFloat).
    for &block in ctx.blocks {
        bindless_to_array::run_pass(ctx.ir, block, ctx.rm, ctx.gpu);
        bindless_elimination::run_pass(ctx.ir, block, ctx.rm, ctx.gpu)?;
        // FragmentCoord only exists in fragment shaders.
        if ctx.stage == ShaderStage::Fragment {
            for node in ctx.ir.block_nodes(block) {
                if ctx.ir.is_op(node) {
                    eliminate_multiply_by_fragment_coord_w(ctx.ir, node);
                }
            }
        }
    }

    // Once more for what the passes above made optimisable.
    run_optimization_passes(ctx.ir, ctx.blocks, ctx.rm)
}

fn run_optimization_passes(ir: &mut Ir, blocks: &[BlockId], rm: &crate::translation::resource_manager::ResourceManager) -> Result<(), String> {
    loop {
        let mut modified = false;
        for &block in blocks {
            let mut node = ir.first_node(block);
            while let Some(n) = node {
                let next_node = ir.next_node(n);
                let mut is_unused = is_unused(ir, n);
                if !ir.is_op(n) || is_unused {
                    if ir.is_phi(n) && !is_unused {
                        is_unused = propagate_phi(ir, n);
                    }
                    if is_unused {
                        remove_node(ir, n);
                        modified = true;
                    }
                    node = next_node;
                    continue;
                }
                constant_folding::run_pass(ir, rm, n)?;
                simplification::run_pass(ir, n);
                if dest_is_local_var(ir, n) {
                    let inst = ir.inst(n);
                    if inst == I::COPY {
                        propagate_copy(ir, n);
                        remove_node(ir, n);
                        modified = true;
                    } else if (inst == I::PACK_HALF_2X16 && propagate_pack(ir, n)) || (inst == I::SHUFFLE_XOR && match_ddx_or_ddy(ir, n)) {
                        if dest_has_no_uses(ir, n) {
                            remove_node(ir, n);
                        }
                        modified = true;
                    }
                }
                node = next_node;
            }
            if branch_elimination(ir, block)? {
                let last = ir.last_node(block).unwrap();
                remove_node(ir, last);
                modified = true;
            }
        }
        if !modified {
            return Ok(());
        }
    }
}

/// Replaces every use of `dest` by `src`.
fn replace_uses(ir: &mut Ir, dest: crate::ir::OperandId, src: crate::ir::OperandId) {
    for use_node in ir.use_ops(dest).to_vec() {
        for index in 0..ir.sources_count(use_node) {
            if ir.get_source_opt(use_node, index) == Some(dest) {
                ir.set_source(use_node, index, Some(src));
            }
        }
    }
}

fn propagate_copy(ir: &mut Ir, copy_op: NodeId) {
    let dest = ir.dest(copy_op).unwrap();
    let src = ir.src(copy_op, 0);
    replace_uses(ir, dest, src);
}

/// A phi whose sources are all the same value is that value.
fn propagate_phi(ir: &mut Ir, phi: NodeId) -> bool {
    if !utils::are_all_sources_the_same_operand(ir, phi) {
        return false;
    }
    let first_src = ir.src(phi, 0);
    let dest = ir.dest(phi).unwrap();
    replace_uses(ir, dest, first_src);
    true
}

/// Unpacks of a pack take the packed value directly.
fn propagate_pack(ir: &mut Ir, pack_op: NodeId) -> bool {
    let mut modified = false;
    let dest = ir.dest(pack_op).unwrap();
    let (src0, src1) = (ir.src(pack_op, 0), ir.src(pack_op, 1));
    for use_node in ir.use_ops(dest).to_vec() {
        if ir.inst_of(use_node) != Some(I::UNPACK_HALF_2X16) {
            continue;
        }
        if ir.src(use_node, 0) == dest {
            let s = if ir.op(use_node).index == 1 { src1 } else { src0 };
            ir.turn_into_copy(use_node, s);
            modified = true;
        }
    }
    modified
}

/// A FSWZADD of a quad shuffle result with the shuffled value, with the derivative masks, is DDX / DDY.
pub fn match_ddx_or_ddy(ir: &mut Ir, n: NodeId) -> bool {
    let mut modified = false;
    let (src2, src3) = (ir.src(n, 1), ir.src(n, 2));
    if ir.ty(src2) != OperandType::Constant || (ir.value(src2) != 1 && ir.value(src2) != 2) {
        return false;
    }
    if ir.ty(src3) != OperandType::Constant || ir.value(src3) != 0x1c03 {
        return false;
    }
    let is_ddy = ir.value(src2) == 2;
    let is_ddx = !is_ddy;
    let dest = ir.dest(n).unwrap();
    for use_op in ir.use_ops(dest).to_vec() {
        if ir.inst_of(use_op) != Some(I::SWIZZLE_ADD) {
            continue;
        }
        let (s1, s2, s3) = (ir.src(use_op, 0), ir.src(use_op, 1), ir.src(use_op, 2));
        if s1 != dest || s2 != ir.src(n, 0) || ir.ty(s3) != OperandType::Constant {
            continue;
        }
        let mask = ir.value(s3);
        if (is_ddx && mask != 0b10011001) || (is_ddy && mask != 0b10100101) {
            continue;
        }
        ir.turn_into(use_op, if is_ddx { I::DDX } else { I::DDY }, s2);
        modified = true;
    }
    modified
}

/// y = x * gl_FragCoord.w; v = y * (1.0 / gl_FragCoord.w) gives v = x (perspective correction).
fn eliminate_multiply_by_fragment_coord_w(ir: &mut Ir, n: NodeId) {
    let mul = I::FP32 | I::MULTIPLY;
    if ir.inst(n) != mul {
        return;
    }
    let (lhs, rhs) = (ir.src(n, 0), ir.src(n, 1));
    let Some(attr_mul_op) = ir.asg_op(lhs).filter(|&a| ir.inst_of(a) == Some(mul)) else { return };
    let (attr_mul_lhs, attr_mul_rhs) = (ir.src(attr_mul_op, 0), ir.src(attr_mul_op, 1));
    if !utils::is_input_load(ir, ir.asg_op(attr_mul_lhs)) || !utils::is_input_load_of(ir, ir.asg_op(attr_mul_rhs), IoVariable::FragmentCoord, 3) {
        return;
    }
    let Some(reciprocal_op) = ir.asg_op(rhs).filter(|&a| ir.inst_of(a) == Some(I::FP32 | I::DIVIDE)) else { return };
    let (rl, rr) = (ir.src(reciprocal_op, 0), ir.src(reciprocal_op, 1));
    if ir.ty(rl) != OperandType::Constant || ir.opnd(rl).as_float() != 1.0 {
        return;
    }
    if !utils::is_input_load_of(ir, ir.asg_op(rr), IoVariable::FragmentCoord, 3) {
        return;
    }
    ir.turn_into_copy(n, attr_mul_lhs);
}

/// Optimizer.RemoveNode: out of the block, and out of the use lists of what it uses; definitions left without uses
/// lose their own uses in turn (they stay in their blocks).
pub fn remove_node(ir: &mut Ir, n: NodeId) {
    ir.remove_node(n);
    let mut nodes = VecDeque::from([n]);
    while let Some(node) = nodes.pop_front() {
        for index in 0..ir.sources_count(node) {
            let src = ir.get_source_opt(node, index).expect("null source");
            if ir.ty(src) != OperandType::LocalVariable {
                continue;
            }
            if ir.opnd_mut(src).use_ops.remove(&node) && ir.use_ops(src).is_empty() {
                nodes.push_back(ir.asg_op(src).expect("no assignment"));
            }
        }
    }
}

fn is_unused(ir: &Ir, n: NodeId) -> bool {
    !has_side_effects(ir, n) && dest_is_local_var(ir, n) && dest_has_no_uses(ir, n)
}

fn has_side_effects(ir: &Ir, n: NodeId) -> bool {
    ir.inst_of(n).is_some_and(|i| {
        matches!(
            i.base(),
            I::ATOMIC_ADD
                | I::ATOMIC_AND
                | I::ATOMIC_COMPARE_AND_SWAP
                | I::ATOMIC_MAX_S32
                | I::ATOMIC_MAX_U32
                | I::ATOMIC_MIN_S32
                | I::ATOMIC_MIN_U32
                | I::ATOMIC_OR
                | I::ATOMIC_SWAP
                | I::ATOMIC_XOR
                | I::CALL
                | I::IMAGE_ATOMIC
        )
    })
}

fn dest_is_local_var(ir: &Ir, n: NodeId) -> bool {
    let count = ir.dests_count(n);
    count != 0 && (0..count).all(|i| ir.get_dest(n, i).is_none_or(|d| ir.ty(d) == OperandType::LocalVariable))
}

fn dest_has_no_uses(ir: &Ir, n: NodeId) -> bool {
    (0..ir.dests_count(n)).all(|i| ir.get_dest(n, i).is_none_or(|d| ir.use_ops(d).is_empty()))
}

/// BranchElimination: a branch to where the next non-empty block's unconditional branch goes anyway is dropped.
fn branch_elimination(ir: &mut Ir, block: BlockId) -> Result<bool, String> {
    if !ir.has_branch(block) {
        return Ok(false);
    }
    let current = ir.last_node(block).unwrap();
    let mut next = ir.next(block);
    while let Some(b) = next {
        if ir.node_count(b) != 0 {
            break;
        }
        if ir.has_branch(b) {
            return Err("System.InvalidOperationException: Found a bogus empty block that \"ends with a branch\".".into());
        }
        next = ir.next(b);
    }
    let Some(first) = next.and_then(|b| ir.first_node(b)) else { return Ok(false) };
    if ir.inst_of(first) != Some(I::BRANCH) {
        return Ok(false);
    }
    if ir.dest(current) == ir.dest(first) {
        ir.set_branch(block, None);
        return Ok(true);
    }
    Ok(false)
}
