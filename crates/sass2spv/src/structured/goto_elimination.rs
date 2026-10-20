// Removes the remaining gotos by moving them and wrapping code in ifs and loops (GotoElimination: a modified version of
// "Taming Control Flow: A Structured Approach to Eliminating Goto Statements").

use super::program::GotoStatement;
use super::{Ast, AstBlockType, AstId};
use crate::ir::Instruction as I;

type R<T> = Result<T, String>;

pub fn eliminate(ast: &mut Ast, mut gotos: Vec<GotoStatement>) -> R<()> {
    for stmt in gotos.iter_mut().rev() {
        let mut g_block = parent_block(ast, stmt.goto);
        let mut l_block = parent_block(ast, stmt.label);
        let mut g_level = level(ast, Some(g_block));
        let mut l_level = level(ast, Some(l_block));

        if indirectly_related(ast, g_block, l_block, g_level, l_level) {
            let mut dr_block = g_block;
            let mut dr_level = g_level;
            loop {
                dr_block = ast.parent(dr_block).ok_or(crate::translation::optimizations::utils::NRE)?;
                dr_level -= 1;
                if directly_related(ast, dr_block, l_block, dr_level, l_level) {
                    break;
                }
            }
            move_outward(ast, stmt, g_level, dr_level)?;
            g_block = dr_block;
            g_level = dr_level;
            if let Some(else_block) = ast.previous(stmt.goto)?.filter(|&p| ast.as_block(p).is_some_and(|b| b.ty == AstBlockType::Else)) {
                // The label may now be inside an else block: update its block and level, and make it a loop if the
                // label is now before the goto because of the new else block.
                l_block = parent_block(ast, stmt.label);
                l_level = level(ast, Some(l_block));
                if !indirectly_related(ast, else_block, l_block, g_level + 1, l_level) {
                    stmt.is_loop = true;
                }
            }
        }

        if directly_related(ast, g_block, l_block, g_level, l_level) {
            if g_level > l_level {
                move_outward(ast, stmt, g_level, l_level)?;
            } else {
                if stmt.is_loop {
                    lift(ast, stmt)?;
                }
                move_inward(ast, stmt)?;
            }
        }

        let g_block = parent_block(ast, stmt.goto);
        if stmt.is_loop {
            encloses_do_while(ast, stmt, g_block, Some(stmt.label))?;
        } else {
            let cond = stmt.condition(ast);
            let first = ast.next(stmt.goto)?;
            enclose(ast, g_block, AstBlockType::If, Some(cond), first, Some(stmt.label))?;
        }
        ast.remove(g_block, stmt.goto)?;
    }
    Ok(())
}

fn indirectly_related(ast: &Ast, l_block: AstId, r_block: AstId, l_level: i32, r_level: i32) -> bool {
    !(l_block == r_block || directly_related(ast, l_block, r_block, l_level, r_level))
}

fn directly_related(ast: &Ast, l_block: AstId, r_block: AstId, l_level: i32, r_level: i32) -> bool {
    // Equal levels: siblings or indirectly related.
    if l_level == r_level {
        return false;
    }
    let (mut block, mut block_lvl, other, other_lvl) =
        if l_level > r_level { (Some(l_block), l_level, r_block, r_level) } else { (Some(r_block), r_level, l_block, l_level) };
    while block_lvl >= other_lvl {
        if block == Some(other) {
            return true;
        }
        block = block.and_then(|b| ast.parent(b));
        block_lvl -= 1;
    }
    false
}

fn lift(ast: &mut Ast, stmt: &mut GotoStatement) -> R<()> {
    let block = parent_block(ast, stmt.goto);
    let path = backwards_path(ast, block, parent_block(ast, stmt.label));
    let mut loop_first_stmt = *path.last().ok_or("System.IndexOutOfRangeException")?;
    if ast.block_type(loop_first_stmt) == AstBlockType::Else {
        loop_first_stmt = ast
            .previous(loop_first_stmt)?
            .filter(|&p| ast.as_block(p).is_some_and(|b| b.ty == AstBlockType::If))
            .ok_or("System.InvalidOperationException: Found an else without a matching if.")?;
    }
    let new_block = encloses_do_while(ast, stmt, block, Some(loop_first_stmt))?.ok_or(crate::translation::optimizations::utils::NRE)?;
    ast.remove(block, stmt.goto)?;
    ast.add_first(new_block, stmt.goto)?;
    stmt.is_loop = false;
    Ok(())
}

fn move_outward(ast: &mut Ast, stmt: &mut GotoStatement, mut g_level: i32, l_level: i32) -> R<()> {
    let origin = parent_block(ast, stmt.goto);
    let mut block = origin;
    // A loop around the goto whose block directly related to the label is above it: a break gets out of it.
    let mut loop_block = origin;
    let mut loop_level = g_level;
    while loop_level > l_level {
        let child = loop_block;
        loop_block = ast.parent(loop_block).ok_or(crate::translation::optimizations::utils::NRE)?;
        loop_level -= 1;
        if ast.block_type(child) == AstBlockType::DoWhile {
            enclose_single_inst(ast, stmt, I::LOOP_BREAK)?;
            ast.remove(block, stmt.goto)?;
            ast.add_after(loop_block, child, stmt.goto)?;
            block = loop_block;
            g_level = loop_level;
        }
    }

    // Ifs skip what the goto jumps over.
    let try_insert_else = stmt.is_unconditional(ast) && ast.block_type(origin) == AstBlockType::If;
    while g_level > l_level {
        let cond = stmt.condition(ast);
        let first = ast.next(stmt.goto)?;
        enclose(ast, block, AstBlockType::If, Some(cond), first, None)?;
        ast.remove(block, stmt.goto)?;
        let mut child = block;
        // The goto cannot go between an if and its else: it goes after the else.
        if let Some(else_block) = ast.next(child)?.filter(|&n| ast.as_block(n).is_some_and(|b| b.ty == AstBlockType::Else)) {
            child = else_block;
        }
        block = ast.parent(block).ok_or(crate::translation::optimizations::utils::NRE)?;
        ast.add_after(block, child, stmt.goto)?;
        g_level -= 1;
        if try_insert_else && child == origin {
            let l_block = parent_block(ast, stmt.label);
            let last = if block == l_block && !stmt.is_loop { Some(stmt.label) } else { None };
            let first = ast.next(stmt.goto)?;
            if let Some(new_block) = enclose(ast, block, AstBlockType::Else, None, first, last)? {
                ast.remove(block, stmt.goto)?;
                ast.add_after(block, new_block, stmt.goto)?;
            }
        }
    }
    Ok(())
}

fn move_inward(ast: &mut Ast, stmt: &mut GotoStatement) -> R<()> {
    let mut block = parent_block(ast, stmt.goto);
    let path = backwards_path(ast, block, parent_block(ast, stmt.label));
    for &child in path.iter().rev() {
        let mut last = child;
        match ast.block_type(child) {
            AstBlockType::If => {
                // The goto may enter the if.
                let cond = stmt.condition(ast);
                if !contains_cond_comb(ast, ast.condition(child), I::LOGICAL_OR, Some(cond)) {
                    ast.or_condition(child, cond);
                }
            }
            AstBlockType::Else => {
                // The goto forces the matching if's else.
                let if_block = ast
                    .previous(child)?
                    .filter(|&p| ast.as_block(p).is_some_and(|b| b.ty == AstBlockType::If))
                    .ok_or("System.InvalidOperationException: Found an else without a matching if.")?;
                let c = stmt.condition(ast);
                let cond = ast.inverse_cond(Some(c));
                if !contains_cond_comb(ast, ast.condition(if_block), I::LOGICAL_AND, Some(cond)) {
                    ast.and_condition(if_block, cond);
                }
                last = if_block;
            }
            _ => {}
        }
        let cond = stmt.condition(ast);
        let first = ast.next(stmt.goto)?;
        enclose(ast, block, AstBlockType::If, Some(cond), first, Some(last))?;
        ast.remove(block, stmt.goto)?;
        ast.add_first(child, stmt.goto)?;
        block = child;
    }
    Ok(())
}

fn contains_cond_comb(ast: &Ast, mut node: Option<AstId>, inst: I, new_cond: Option<AstId>) -> bool {
    while let Some(op) = node.and_then(|n| ast.as_operation(n)).filter(|o| o.sources.len() == 2) {
        if op.inst == inst && is_same_cond(ast, op.sources[1], new_cond) {
            return true;
        }
        node = op.sources[0];
    }
    false
}

/// EncloseDoWhile.
fn encloses_do_while(ast: &mut Ast, stmt: &mut GotoStatement, block: AstId, first: Option<AstId>) -> R<Option<AstId>> {
    if ast.block_type(block) == AstBlockType::DoWhile && first == ast.first(block) {
        // A continue only when not at the end of the loop, or with a condition other than the loop's.
        let cond = stmt.condition(ast);
        if ast.next(stmt.goto)?.is_some() || ast.condition(block) != Some(cond) {
            enclose_single_inst(ast, stmt, I::LOOP_CONTINUE)?;
        }
        // The loop condition must allow it to continue.
        if !contains_cond_comb(ast, ast.condition(block), I::LOGICAL_OR, Some(cond)) {
            ast.or_condition(block, cond);
        }
        return Ok(Some(block));
    }
    let cond = stmt.condition(ast);
    enclose(ast, block, AstBlockType::DoWhile, Some(cond), first, Some(stmt.goto))
}

fn enclose_single_inst(ast: &mut Ast, stmt: &mut GotoStatement, inst: I) -> R<()> {
    let block = parent_block(ast, stmt.goto);
    let cond = stmt.condition(ast);
    let new_block = ast.block(AstBlockType::If, Some(cond));
    ast.add_after(block, stmt.goto, new_block)?;
    let op = ast.operation(inst, &[]);
    ast.add_first(new_block, op)
}

fn enclose(ast: &mut Ast, block: AstId, ty: AstBlockType, cond: Option<AstId>, first: Option<AstId>, last: Option<AstId>) -> R<Option<AstId>> {
    if first == last {
        return Ok(None);
    }
    let cond = if ty == AstBlockType::If { Some(ast.inverse_cond(cond)) } else { cond };
    let first_node = first.ok_or(crate::translation::optimizations::utils::NRE)?;
    // A single block of the same type and condition needs no new one.
    let has_single_node = ast.next(first_node)? == last;
    if has_single_node && block_matches(ast, first_node, ty, cond) {
        return Ok(Some(first_node));
    }
    let new_block = ast.block(ty, cond);
    ast.add_before(block, first_node, new_block)?;
    let mut f = first;
    while f != last {
        let node = f.ok_or(crate::translation::optimizations::utils::NRE)?;
        let next = ast.next(node)?;
        ast.remove(block, node)?;
        ast.add(new_block, node)?;
        f = next;
    }
    Ok(Some(new_block))
}

fn block_matches(ast: &Ast, node: AstId, ty: AstBlockType, cond: Option<AstId>) -> bool {
    ast.as_block(node).is_some_and(|b| b.ty == ty && is_same_cond(ast, b.condition, cond))
}

fn is_same_cond(ast: &Ast, mut l: Option<AstId>, mut r: Option<AstId>) -> bool {
    if let Some(lop) = l.and_then(|x| ast.as_operation(x)).filter(|o| o.inst == I::LOGICAL_NOT) {
        let Some(rop) = r.and_then(|x| ast.as_operation(x)).filter(|o| o.inst == lop.inst) else { return false };
        l = lop.sources[0];
        r = rop.sources[0];
    }
    l == r
}

/// The block holding a node (a block's own parent).
fn parent_block(ast: &Ast, node: AstId) -> AstId {
    if ast.is_block(node) {
        return ast.parent(node).expect("block without a parent");
    }
    let mut n = node;
    while !ast.is_block(n) {
        n = ast.parent(n).expect("node outside any block");
    }
    n
}

fn backwards_path(ast: &Ast, top: AstId, bottom: AstId) -> Vec<AstId> {
    let mut path = Vec::new();
    let mut block = Some(bottom);
    while block != Some(top) {
        let b = block.expect("path does not reach the top block");
        path.push(b);
        block = ast.parent(b);
    }
    path
}

fn level(ast: &Ast, mut node: Option<AstId>) -> i32 {
    let mut level = 0;
    while let Some(n) = node {
        level += 1;
        node = ast.parent(n);
    }
    level
}
