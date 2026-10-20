use super::basic_block::BasicBlock;
use super::instruction::Instruction;
use super::operand::Operand;
use super::operation::Operation;
use rustc_hash::FxHashMap;

#[derive(Debug, Clone)]
pub struct ControlFlowGraph {
    pub blocks: Vec<BasicBlock>,
}

impl ControlFlowGraph {
    pub fn create(operations: &[Operation]) -> Self {
        if operations.is_empty() {
            return Self { blocks: Vec::new() };
        }

        let mut labels: FxHashMap<Operand, usize> = FxHashMap::default();
        let mut blocks: Vec<BasicBlock> = Vec::new();
        let mut current_block_idx: Option<usize> = None;

        let new_next_block = |blocks: &mut Vec<BasicBlock>, curr_idx: &mut Option<usize>| -> usize {
            let idx = blocks.len();
            blocks.push(BasicBlock::new(idx));
            if let Some(prev_idx) = *curr_idx {
                if !Self::ends_with_unconditional_inst(blocks[prev_idx].get_last_op()) {
                    blocks[prev_idx].next = Some(idx);
                }
            }
            *curr_idx = Some(idx);
            idx
        };

        let next_block = |blocks: &mut Vec<BasicBlock>, curr_idx: &mut Option<usize>, target_idx: usize| {
            if let Some(prev_idx) = *curr_idx {
                if !Self::ends_with_unconditional_inst(blocks[prev_idx].get_last_op()) {
                    blocks[prev_idx].next = Some(target_idx);
                }
            }
            *curr_idx = Some(target_idx);
        };

        let mut needs_new_block = true;

        for op in operations {
            if op.inst == Instruction::MarkLabel {
                let label = op.dest.or_else(|| op.sources.first().copied()).unwrap();

                if let Some(&target_idx) = labels.get(&label) {
                    blocks[target_idx].index = blocks.len();
                    let moved_idx = blocks.len();
                    // If target was a forward reference placeholder
                    blocks.push(BasicBlock::new(moved_idx));
                    next_block(&mut blocks, &mut current_block_idx, moved_idx);
                } else {
                    let new_idx = new_next_block(&mut blocks, &mut current_block_idx);
                    labels.insert(label, new_idx);
                }
            } else {
                if needs_new_block {
                    new_next_block(&mut blocks, &mut current_block_idx);
                }
                if let Some(curr) = current_block_idx {
                    blocks[curr].operations.push(op.clone());
                }
            }

            needs_new_block = matches!(
                op.inst,
                Instruction::Branch | Instruction::BranchIfTrue | Instruction::BranchIfFalse
            );

            if needs_new_block {
                let label = op.dest.or_else(|| op.sources.first().copied()).unwrap();
                let branch_target_idx = if let Some(&idx) = labels.get(&label) {
                    idx
                } else {
                    // Forward branch: allocate placeholder block
                    let idx = blocks.len();
                    blocks.push(BasicBlock::new(idx));
                    labels.insert(label, idx);
                    idx
                };

                if let Some(curr) = current_block_idx {
                    blocks[curr].branch = Some(branch_target_idx);
                }
            }
        }

        // Recompute predecessors
        Self::rebuild_predecessors(&mut blocks);

        // Remove unreachable blocks
        let mut has_unreachable = true;
        while has_unreachable {
            has_unreachable = false;
            let mut i = 1;
            while i < blocks.len() {
                if blocks[i].predecessors.is_empty() {
                    let dead_idx = i;
                    // Unlink edges
                    blocks.remove(dead_idx);
                    // Shift indices in next/branch/predecessors
                    for blk in &mut blocks {
                        if let Some(n) = blk.next {
                            if n == dead_idx {
                                blk.next = None;
                            } else if n > dead_idx {
                                blk.next = Some(n - 1);
                            }
                        }
                        if let Some(b) = blk.branch {
                            if b == dead_idx {
                                blk.branch = None;
                            } else if b > dead_idx {
                                blk.branch = Some(b - 1);
                            }
                        }
                    }
                    Self::rebuild_predecessors(&mut blocks);
                    has_unreachable = true;
                } else {
                    blocks[i].index = i;
                    i += 1;
                }
            }
        }

        for (idx, blk) in blocks.iter_mut().enumerate() {
            blk.index = idx;
        }

        Self { blocks }
    }

    fn rebuild_predecessors(blocks: &mut [BasicBlock]) {
        for blk in blocks.iter_mut() {
            blk.predecessors.clear();
        }
        let edges: Vec<(usize, Option<usize>, Option<usize>)> = blocks
            .iter()
            .enumerate()
            .map(|(i, b)| (i, b.next, b.branch))
            .collect();

        for (src, next, branch) in edges {
            if let Some(n) = next {
                if n < blocks.len() && !blocks[n].predecessors.contains(&src) {
                    blocks[n].predecessors.push(src);
                }
            }
            if let Some(b) = branch {
                if b < blocks.len() && !blocks[b].predecessors.contains(&src) {
                    blocks[b].predecessors.push(src);
                }
            }
        }
    }

    fn ends_with_unconditional_inst(op: Option<&Operation>) -> bool {
        if let Some(op) = op {
            matches!(
                op.inst,
                Instruction::Branch | Instruction::Discard | Instruction::Return
            )
        } else {
            false
        }
    }
}
