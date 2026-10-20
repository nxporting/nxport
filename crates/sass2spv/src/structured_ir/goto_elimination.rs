use super::ast::{AstArena, AstBlockType, AstNodeKind, NodeId};
use super::goto_statement::GotoStatement;
use crate::ir::instruction::Instruction;

pub struct GotoElimination;

impl GotoElimination {
    pub fn eliminate(arena: &mut AstArena, gotos: &mut [GotoStatement]) {
        for index in (0..gotos.len()).rev() {
            let mut stmt = gotos[index].clone();

            let mut g_block = arena.parent_block(stmt.goto_node);
            let mut l_block = arena.parent_block(stmt.label_node);

            let mut g_level = arena.level(g_block);
            let mut l_level = arena.level(l_block);

            if Self::indirectly_related(arena, g_block, l_block, g_level, l_level) {
                let mut dr_block = g_block;
                let mut dr_level = g_level;

                loop {
                    dr_block = arena.nodes[dr_block.0].parent.expect("Parent must exist");
                    dr_level -= 1;
                    if Self::directly_related(arena, dr_block, l_block, dr_level, l_level) {
                        break;
                    }
                }

                Self::move_outward(arena, &stmt, g_level, dr_level);

                g_block = dr_block;
                g_level = dr_level;

                if let Some(prev) = arena.prev(stmt.goto_node) {
                    if let AstNodeKind::Block(b) = &arena.nodes[prev.0].kind {
                        if b.block_type == AstBlockType::Else {
                            l_block = arena.parent_block(stmt.label_node);
                            l_level = arena.level(l_block);

                            if !Self::indirectly_related(arena, prev, l_block, g_level + 1, l_level) {
                                stmt.is_loop = true;
                            }
                        }
                    }
                }
            }

            if Self::directly_related(arena, g_block, l_block, g_level, l_level) {
                if g_level > l_level {
                    Self::move_outward(arena, &stmt, g_level, l_level);
                } else {
                    if stmt.is_loop {
                        Self::lift(arena, &mut stmt);
                    }
                    Self::move_inward(arena, &stmt);
                }
            }

            g_block = arena.parent_block(stmt.goto_node);

            if stmt.is_loop {
                Self::enclose_do_while(arena, &stmt, g_block, stmt.label_node);
            } else {
                let cond = stmt.condition(arena);
                if let Some(next) = arena.next(stmt.goto_node) {
                    Self::enclose(arena, g_block, AstBlockType::If, cond, next, Some(stmt.label_node));
                }
            }

            arena.remove(stmt.goto_node);
            gotos[index] = stmt;
        }
    }

    fn indirectly_related(arena: &AstArena, l_block: NodeId, r_block: NodeId, l_level: usize, r_level: usize) -> bool {
        !(l_block == r_block || Self::directly_related(arena, l_block, r_block, l_level, r_level))
    }

    fn directly_related(arena: &AstArena, l_block: NodeId, r_block: NodeId, l_level: usize, r_level: usize) -> bool {
        if l_level == r_level {
            return false;
        }

        let (mut block, mut block_lvl, other, other_lvl) = if l_level > r_level {
            (l_block, l_level, r_block, r_level)
        } else {
            (r_block, r_level, l_block, l_level)
        };

        while block_lvl >= other_lvl {
            if block == other {
                return true;
            }
            if let Some(p) = arena.nodes[block.0].parent {
                block = p;
                block_lvl -= 1;
            } else {
                break;
            }
        }

        false
    }

    fn lift(arena: &mut AstArena, stmt: &mut GotoStatement) {
        let block = arena.parent_block(stmt.goto_node);
        let path = Self::backwards_path(arena, block, arena.parent_block(stmt.label_node));
        let mut loop_first_stmt = *path.last().expect("Path not empty");

        if let AstNodeKind::Block(b) = &arena.nodes[loop_first_stmt.0].kind {
            if b.block_type == AstBlockType::Else {
                loop_first_stmt = arena.prev(loop_first_stmt).expect("Matching if for else");
            }
        }

        let new_block = Self::enclose_do_while(arena, stmt, block, loop_first_stmt);
        arena.remove(stmt.goto_node);
        arena.add_first(new_block, stmt.goto_node);
        stmt.is_loop = false;
    }

    fn move_outward(arena: &mut AstArena, stmt: &GotoStatement, mut g_level: usize, l_level: usize) {
        let origin = arena.parent_block(stmt.goto_node);
        let mut block = origin;
        let mut loop_block = origin;
        let mut loop_level = g_level;

        while loop_level > l_level {
            let child = loop_block;
            loop_block = arena.nodes[loop_block.0].parent.expect("Parent block");
            loop_level -= 1;

            if let AstNodeKind::Block(b) = &arena.nodes[child.0].kind {
                if b.block_type == AstBlockType::DoWhile {
                    Self::enclose_single_inst(arena, stmt, Instruction::LoopBreak);
                    arena.remove(stmt.goto_node);
                    arena.add_after(child, stmt.goto_node);
                    block = loop_block;
                    g_level = loop_level;
                }
            }
        }

        let is_uncond = stmt.is_unconditional(arena);
        let try_insert_else = is_uncond && arena.get_block(origin).block_type == AstBlockType::If;

        while g_level > l_level {
            let cond = stmt.condition(arena);
            if let Some(next) = arena.next(stmt.goto_node) {
                Self::enclose(arena, block, AstBlockType::If, cond, next, None);
            }

            arena.remove(stmt.goto_node);
            let mut child = block;

            if let Some(next_sibling) = arena.next(child) {
                if let AstNodeKind::Block(b) = &arena.nodes[next_sibling.0].kind {
                    if b.block_type == AstBlockType::Else {
                        child = next_sibling;
                    }
                }
            }

            block = arena.nodes[block.0].parent.expect("Parent block");
            arena.add_after(child, stmt.goto_node);
            g_level -= 1;

            if try_insert_else && child == origin {
                let l_block = arena.parent_block(stmt.label_node);
                let last = if block == l_block && !stmt.is_loop {
                    Some(stmt.label_node)
                } else {
                    None
                };

                if let Some(next) = arena.next(stmt.goto_node) {
                    let cond = stmt.condition(arena);
                    if let Some(new_block) = Self::enclose(arena, block, AstBlockType::Else, cond, next, last) {
                        arena.remove(stmt.goto_node);
                        arena.add_after(new_block, stmt.goto_node);
                    }
                }
            }
        }
    }

    fn move_inward(arena: &mut AstArena, stmt: &GotoStatement) {
        let mut block = arena.parent_block(stmt.goto_node);
        let path = Self::backwards_path(arena, block, arena.parent_block(stmt.label_node));

        for &child in path.iter().rev() {
            let mut last = child;

            let b_type = match &arena.nodes[child.0].kind {
                AstNodeKind::Block(b) => b.block_type,
                _ => panic!("Expected Block node in path"),
            };

            let cond = stmt.condition(arena);

            if b_type == AstBlockType::If {
                let curr_cond = arena.get_block(child).condition;
                if !Self::contains_cond_comb(arena, curr_cond, Instruction::LogicalOr, cond) {
                    arena.or_condition(child, cond);
                }
            } else if b_type == AstBlockType::Else {
                let if_block = arena.prev(child).expect("Else must have prev");
                let inv_cond = arena.inverse_cond(cond);
                let curr_cond = arena.get_block(if_block).condition;
                if !Self::contains_cond_comb(arena, curr_cond, Instruction::LogicalAnd, inv_cond) {
                    arena.and_condition(if_block, inv_cond);
                }
                last = if_block;
            }

            if let Some(next) = arena.next(stmt.goto_node) {
                Self::enclose(arena, block, AstBlockType::If, cond, next, Some(last));
            }

            arena.remove(stmt.goto_node);
            arena.add_first(child, stmt.goto_node);
            block = child;
        }
    }

    fn contains_cond_comb(arena: &AstArena, mut node: Option<NodeId>, inst: Instruction, new_cond: NodeId) -> bool {
        while let Some(curr) = node {
            if let AstNodeKind::Operation(op) = &arena.nodes[curr.0].kind {
                if op.sources.len() == 2 {
                    if op.inst == inst && Self::is_same_cond(arena, op.sources[1], new_cond) {
                        return true;
                    }
                    node = Some(op.sources[0]);
                    continue;
                }
            }
            break;
        }
        false
    }

    fn enclose_do_while(arena: &mut AstArena, stmt: &GotoStatement, block: NodeId, first: NodeId) -> NodeId {
        let blk = arena.get_block(block);
        let cond = stmt.condition(arena);
        if blk.block_type == AstBlockType::DoWhile && blk.first == Some(first) {
            if arena.next(stmt.goto_node).is_some() || blk.condition != Some(cond) {
                Self::enclose_single_inst(arena, stmt, Instruction::LoopContinue);
            }
            let curr_cond = arena.get_block(block).condition;
            if !Self::contains_cond_comb(arena, curr_cond, Instruction::LogicalOr, cond) {
                arena.or_condition(block, cond);
            }
            return block;
        }

        Self::enclose(arena, block, AstBlockType::DoWhile, cond, first, Some(stmt.goto_node))
            .unwrap_or(block)
    }

    fn enclose_single_inst(arena: &mut AstArena, stmt: &GotoStatement, inst: Instruction) {
        let cond = stmt.condition(arena);
        let new_block = arena.new_block(AstBlockType::If, Some(cond));
        arena.add_after(stmt.goto_node, new_block);
        let op = arena.new_operation(inst, vec![]);
        arena.add_first(new_block, op);
    }

    fn enclose(
        arena: &mut AstArena,
        _block: NodeId,
        block_type: AstBlockType,
        cond: NodeId,
        mut first: NodeId,
        last: Option<NodeId>,
    ) -> Option<NodeId> {
        if Some(first) == last {
            return None;
        }

        let effective_cond = if block_type == AstBlockType::If {
            arena.inverse_cond(cond)
        } else {
            cond
        };

        let has_single_node = arena.next(first) == last;
        if has_single_node && Self::block_matches(arena, first, block_type, effective_cond) {
            return Some(first);
        }

        let new_block = arena.new_block(block_type, Some(effective_cond));
        arena.add_before(first, new_block);

        while Some(first) != last {
            let next = arena.next(first);
            arena.remove(first);
            arena.add(new_block, first);
            if let Some(n) = next {
                first = n;
            } else {
                break;
            }
        }

        Some(new_block)
    }

    fn block_matches(arena: &AstArena, node: NodeId, block_type: AstBlockType, cond: NodeId) -> bool {
        if let AstNodeKind::Block(b) = &arena.nodes[node.0].kind {
            if b.block_type == block_type {
                if let Some(c) = b.condition {
                    return Self::is_same_cond(arena, c, cond);
                }
            }
        }
        false
    }

    fn is_same_cond(arena: &AstArena, mut l_cond: NodeId, mut r_cond: NodeId) -> bool {
        if let AstNodeKind::Operation(l_op) = &arena.nodes[l_cond.0].kind {
            if l_op.inst == Instruction::LogicalNot {
                if let AstNodeKind::Operation(r_op) = &arena.nodes[r_cond.0].kind {
                    if r_op.inst == Instruction::LogicalNot {
                        l_cond = l_op.sources[0];
                        r_cond = r_op.sources[0];
                    } else {
                        return false;
                    }
                } else {
                    return false;
                }
            }
        }

        if l_cond == r_cond {
            return true;
        }

        if let (AstNodeKind::Operand(lo), AstNodeKind::Operand(ro)) =
            (&arena.nodes[l_cond.0].kind, &arena.nodes[r_cond.0].kind)
        {
            return lo.op_type == ro.op_type && lo.value == ro.value;
        }

        false
    }

    fn backwards_path(arena: &AstArena, top: NodeId, bottom: NodeId) -> Vec<NodeId> {
        let mut block = bottom;
        let mut path = Vec::new();
        while block != top {
            path.push(block);
            block = arena.nodes[block.0].parent.expect("Ancestor path in backwards_path");
        }
        path
    }
}
