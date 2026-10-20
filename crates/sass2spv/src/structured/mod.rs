// Structured form of a program: nested blocks (if / else / loops) of assignments and operations, made from the
// control flow graph (Ryujinx.Graphics.Shader.StructuredIr). Nodes live in an arena; a block's children form a doubly
// linked list, like the original's LinkedList<IAstNode>.

pub mod goto_elimination;
pub mod instruction_info;
pub mod optimizer;
pub mod program;

use crate::collections::NetSet;
use crate::ir::{Instruction, IoVariable, OperandType, StorageKind, TextureFlags};
use crate::translation::definitions::AggregateType;
use crate::types::{SamplerType, SetBindingPair, TextureFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AstId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AstBlockType {
    DoWhile,
    If,
    Else,
    ElseIf,
    Main,
    While,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AstTexture {
    pub ty: SamplerType,
    pub format: TextureFormat,
    pub flags: TextureFlags,
    pub set: i32,
    pub binding: i32,
    pub sampler_set: i32,
    pub sampler_binding: i32,
}

impl AstTexture {
    pub fn is_separate(&self) -> bool {
        self.sampler_binding >= 0
    }

    pub fn texture_set_and_binding(&self) -> SetBindingPair {
        SetBindingPair::new(self.set, self.binding)
    }

    pub fn sampler_set_and_binding(&self) -> SetBindingPair {
        SetBindingPair::new(self.sampler_set, self.sampler_binding)
    }
}

pub struct AstOperand {
    pub defs: NetSet<AstId>,
    pub uses: NetSet<AstId>,
    pub ty: OperandType,
    pub var_type: AggregateType,
    pub value: i32,
}

pub struct AstOperation {
    pub inst: Instruction,
    pub storage_kind: StorageKind,
    pub force_precise: bool,
    pub index: i32,
    pub sources: Vec<Option<AstId>>,
    pub tex: Option<AstTexture>,
}

impl AstOperation {
    /// The type of a vector result with as many components as the index's mask has bits.
    pub fn vector_type(&self, scalar_type: AggregateType) -> AggregateType {
        match (self.index as u32).count_ones() {
            2 => scalar_type | AggregateType::VECTOR2,
            3 => scalar_type | AggregateType::VECTOR3,
            4 => scalar_type | AggregateType::VECTOR4,
            _ => scalar_type,
        }
    }
}

pub struct AstBlock {
    pub ty: AstBlockType,
    pub condition: Option<AstId>,
    first: Option<AstId>,
    last: Option<AstId>,
    count: usize,
}

pub enum AstKind {
    Operand(AstOperand),
    Operation(AstOperation),
    Assignment { destination: AstId, source: Option<AstId> },
    Comment(String),
    Block(AstBlock),
}

pub struct AstNode {
    pub kind: AstKind,
    pub parent: Option<AstId>,
    prev: Option<AstId>,
    next: Option<AstId>,
    /// Whether the node is in a block's list (LLNode != null).
    linked: bool,
}

#[derive(Default)]
pub struct Ast {
    pub nodes: Vec<AstNode>,
}

impl Ast {
    fn push(&mut self, kind: AstKind) -> AstId {
        self.nodes.push(AstNode { kind, parent: None, prev: None, next: None, linked: false });
        AstId(self.nodes.len() as u32 - 1)
    }

    pub fn node(&self, id: AstId) -> &AstNode {
        &self.nodes[id.0 as usize]
    }

    pub fn kind(&self, id: AstId) -> &AstKind {
        &self.node(id).kind
    }

    fn kind_mut(&mut self, id: AstId) -> &mut AstKind {
        &mut self.nodes[id.0 as usize].kind
    }

    pub fn parent(&self, id: AstId) -> Option<AstId> {
        self.node(id).parent
    }

    // Operands.

    pub fn operand(&mut self, ty: OperandType, value: i32) -> AstId {
        self.push(AstKind::Operand(AstOperand { defs: NetSet::new(), uses: NetSet::new(), ty, var_type: AggregateType::S32, value }))
    }

    pub fn as_operand(&self, id: AstId) -> Option<&AstOperand> {
        match self.kind(id) {
            AstKind::Operand(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_operand_mut(&mut self, id: AstId) -> Option<&mut AstOperand> {
        match self.kind_mut(id) {
            AstKind::Operand(o) => Some(o),
            _ => None,
        }
    }

    pub fn opnd(&self, id: AstId) -> &AstOperand {
        self.as_operand(id).expect("not an operand")
    }

    pub fn opnd_mut(&mut self, id: AstId) -> &mut AstOperand {
        self.as_operand_mut(id).expect("not an operand")
    }

    /// AstHelper.Const.
    pub fn konst(&mut self, value: i32) -> AstId {
        self.operand(OperandType::Constant, value)
    }

    /// AstHelper.Local.
    pub fn local(&mut self, ty: AggregateType) -> AstId {
        let l = self.operand(OperandType::LocalVariable, 0);
        self.opnd_mut(l).var_type = ty;
        l
    }

    fn local_operand_mut(&mut self, id: Option<AstId>) -> Option<&mut AstOperand> {
        let id = id?;
        match self.kind_mut(id) {
            AstKind::Operand(o) if o.ty == OperandType::LocalVariable => Some(o),
            _ => None,
        }
    }

    pub fn add_use(&mut self, node: Option<AstId>, parent: AstId) {
        if let Some(o) = self.local_operand_mut(node) {
            o.uses.insert(parent);
        }
    }

    pub fn add_def(&mut self, node: Option<AstId>, parent: AstId) {
        if let Some(o) = self.local_operand_mut(node) {
            o.defs.insert(parent);
        }
    }

    pub fn remove_use(&mut self, node: Option<AstId>, parent: AstId) {
        if let Some(o) = self.local_operand_mut(node) {
            o.uses.remove(&parent);
        }
    }

    // Operations.

    /// new AstOperation(inst, storageKind, forcePrecise, index, sources, sourcesCount): the first `sources_count`
    /// sources are uses, the rest (output arguments) definitions.
    pub fn operation_full(
        &mut self,
        inst: Instruction,
        storage_kind: StorageKind,
        force_precise: bool,
        index: i32,
        sources: Vec<Option<AstId>>,
        sources_count: usize,
        tex: Option<AstTexture>,
    ) -> AstId {
        let srcs = sources.clone();
        let id = self.push(AstKind::Operation(AstOperation { inst, storage_kind, force_precise, index, sources, tex }));
        for (i, s) in srcs.into_iter().enumerate() {
            if i < sources_count {
                self.add_use(s, id);
            } else {
                self.add_def(s, id);
            }
        }
        id
    }

    /// new AstOperation(inst, sources...).
    pub fn operation(&mut self, inst: Instruction, sources: &[AstId]) -> AstId {
        let s: Vec<Option<AstId>> = sources.iter().map(|&x| Some(x)).collect();
        let n = s.len();
        self.operation_full(inst, StorageKind::None, false, 0, s, n, None)
    }

    pub fn as_operation(&self, id: AstId) -> Option<&AstOperation> {
        match self.kind(id) {
            AstKind::Operation(o) => Some(o),
            _ => None,
        }
    }

    pub fn op(&self, id: AstId) -> &AstOperation {
        self.as_operation(id).expect("not an operation")
    }

    pub fn set_source(&mut self, id: AstId, index: usize, source: Option<AstId>) {
        let old = self.op(id).sources[index];
        self.remove_use(old, id);
        self.add_use(source, id);
        match self.kind_mut(id) {
            AstKind::Operation(o) => o.sources[index] = source,
            _ => unreachable!(),
        }
    }

    // Assignments.

    /// new AstAssignment(destination, source).
    pub fn assign(&mut self, destination: AstId, source: Option<AstId>) -> AstId {
        let id = self.push(AstKind::Assignment { destination, source: None });
        self.set_assignment_source(id, source);
        self.add_def(Some(destination), id);
        id
    }

    pub fn assignment(&self, id: AstId) -> Option<(AstId, Option<AstId>)> {
        match self.kind(id) {
            AstKind::Assignment { destination, source } => Some((*destination, *source)),
            _ => None,
        }
    }

    pub fn set_assignment_source(&mut self, id: AstId, source: Option<AstId>) {
        let old = self.assignment(id).unwrap().1;
        self.remove_use(old, id);
        self.add_use(source, id);
        if let AstKind::Assignment { source: s, .. } = self.kind_mut(id) {
            *s = source;
        }
    }

    pub fn comment(&mut self, text: String) -> AstId {
        self.push(AstKind::Comment(text))
    }

    // Blocks.

    pub fn block(&mut self, ty: AstBlockType, condition: Option<AstId>) -> AstId {
        let id = self.push(AstKind::Block(AstBlock { ty, condition: None, first: None, last: None, count: 0 }));
        self.set_condition(id, condition);
        id
    }

    pub fn as_block(&self, id: AstId) -> Option<&AstBlock> {
        match self.kind(id) {
            AstKind::Block(b) => Some(b),
            _ => None,
        }
    }

    pub fn blk(&self, id: AstId) -> &AstBlock {
        self.as_block(id).expect("not a block")
    }

    fn blk_mut(&mut self, id: AstId) -> &mut AstBlock {
        match self.kind_mut(id) {
            AstKind::Block(b) => b,
            _ => panic!("not a block"),
        }
    }

    pub fn is_block(&self, id: AstId) -> bool {
        self.as_block(id).is_some()
    }

    pub fn block_type(&self, id: AstId) -> AstBlockType {
        self.blk(id).ty
    }

    pub fn condition(&self, id: AstId) -> Option<AstId> {
        self.blk(id).condition
    }

    pub fn set_condition(&mut self, id: AstId, condition: Option<AstId>) {
        let old = self.blk(id).condition;
        self.remove_use(old, id);
        self.add_use(condition, id);
        self.blk_mut(id).condition = condition;
    }

    pub fn first(&self, block: AstId) -> Option<AstId> {
        self.blk(block).first
    }

    pub fn last(&self, block: AstId) -> Option<AstId> {
        self.blk(block).last
    }

    pub fn count(&self, block: AstId) -> usize {
        self.blk(block).count
    }

    pub fn children(&self, block: AstId) -> Vec<AstId> {
        let mut v = Vec::new();
        let mut n = self.first(block);
        while let Some(x) = n {
            v.push(x);
            n = self.node(x).next;
        }
        v
    }

    fn link(&mut self, block: AstId, node: AstId, prev: Option<AstId>, next: Option<AstId>) -> Result<(), String> {
        if self.node(node).parent.is_some() {
            return Err("System.ArgumentException: Node already belongs to a block.".into());
        }
        {
            let n = &mut self.nodes[node.0 as usize];
            n.prev = prev;
            n.next = next;
            n.parent = Some(block);
            n.linked = true;
        }
        match prev {
            Some(p) => self.nodes[p.0 as usize].next = Some(node),
            None => self.blk_mut(block).first = Some(node),
        }
        match next {
            Some(x) => self.nodes[x.0 as usize].prev = Some(node),
            None => self.blk_mut(block).last = Some(node),
        }
        self.blk_mut(block).count += 1;
        Ok(())
    }

    /// AstBlock.Add.
    pub fn add(&mut self, block: AstId, node: AstId) -> Result<(), String> {
        let last = self.last(block);
        self.link(block, node, last, None)
    }

    pub fn add_first(&mut self, block: AstId, node: AstId) -> Result<(), String> {
        let first = self.first(block);
        self.link(block, node, None, first)
    }

    pub fn add_before(&mut self, block: AstId, next: AstId, node: AstId) -> Result<(), String> {
        self.check_in(block, next)?;
        let prev = self.node(next).prev;
        self.link(block, node, prev, Some(next))
    }

    pub fn add_after(&mut self, block: AstId, prev: AstId, node: AstId) -> Result<(), String> {
        self.check_in(block, prev)?;
        let next = self.node(prev).next;
        self.link(block, node, Some(prev), next)
    }

    /// LinkedList operations on a node of another list throw.
    fn check_in(&self, block: AstId, node: AstId) -> Result<(), String> {
        if self.node(node).parent != Some(block) || !self.node(node).linked {
            return Err("System.InvalidOperationException: The LinkedList node does not belong to current LinkedList.".into());
        }
        Ok(())
    }

    /// AstBlock.Remove.
    pub fn remove(&mut self, block: AstId, node: AstId) -> Result<(), String> {
        if !self.node(node).linked {
            return Err("System.ArgumentNullException: Value cannot be null. (Parameter 'node')".into());
        }
        self.check_in(block, node)?;
        let (prev, next) = (self.node(node).prev, self.node(node).next);
        match prev {
            Some(p) => self.nodes[p.0 as usize].next = next,
            None => self.blk_mut(block).first = next,
        }
        match next {
            Some(x) => self.nodes[x.0 as usize].prev = prev,
            None => self.blk_mut(block).last = prev,
        }
        self.blk_mut(block).count -= 1;
        let n = &mut self.nodes[node.0 as usize];
        n.prev = None;
        n.next = None;
        n.parent = None;
        n.linked = false;
        Ok(())
    }

    pub fn and_condition(&mut self, block: AstId, cond: AstId) {
        let c = self.condition(block);
        let op = self.operation_opt(Instruction::LOGICAL_AND, &[c, Some(cond)]);
        self.set_condition(block, Some(op));
    }

    pub fn or_condition(&mut self, block: AstId, cond: AstId) {
        let c = self.condition(block);
        let op = self.operation_opt(Instruction::LOGICAL_OR, &[c, Some(cond)]);
        self.set_condition(block, Some(op));
    }

    fn operation_opt(&mut self, inst: Instruction, sources: &[Option<AstId>]) -> AstId {
        self.operation_full(inst, StorageKind::None, false, 0, sources.to_vec(), sources.len(), None)
    }

    pub fn turn_into_if(&mut self, block: AstId, cond: Option<AstId>) {
        self.set_condition(block, cond);
        self.blk_mut(block).ty = AstBlockType::If;
    }

    /// AstHelper.InverseCond.
    pub fn inverse_cond(&mut self, cond: Option<AstId>) -> AstId {
        self.operation_opt(Instruction::LOGICAL_NOT, &[cond])
    }

    /// AstHelper.Next: the following node in the node's block (the node must be in one).
    pub fn next(&self, node: AstId) -> Result<Option<AstId>, String> {
        if !self.node(node).linked {
            return Err(crate::translation::optimizations::utils::NRE.into());
        }
        Ok(self.node(node).next)
    }

    pub fn previous(&self, node: AstId) -> Result<Option<AstId>, String> {
        if !self.node(node).linked {
            return Err(crate::translation::optimizations::utils::NRE.into());
        }
        Ok(self.node(node).prev)
    }
}

/// What the visitor reports, in the original's order: entering a block (BlockEntered), a node, leaving a block
/// (BlockLeft).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VisitEvent {
    Enter(AstId),
    Node(AstId),
    Left(AstId),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitPhase {
    Start,
    OuterCheck,
    Inner,
    GoUp,
}

/// Iterates a block's nodes depth first, entering child blocks (AstBlockVisitor). `block` is the block of the node
/// last returned. Each step runs where the original's generator resumes, so the caller sees the tree as the original
/// does.
pub struct AstBlockVisitor {
    pub block: AstId,
    node: Option<AstId>,
    phase: VisitPhase,
}

impl AstBlockVisitor {
    pub fn new(main_block: AstId) -> Self {
        AstBlockVisitor { block: main_block, node: None, phase: VisitPhase::Start }
    }

    /// The next node (block events skipped).
    pub fn next(&mut self, ast: &Ast) -> Option<AstId> {
        loop {
            match self.next_event(ast)? {
                VisitEvent::Node(n) => return Some(n),
                _ => continue,
            }
        }
    }

    pub fn next_event(&mut self, ast: &Ast) -> Option<VisitEvent> {
        loop {
            match self.phase {
                VisitPhase::Start => {
                    self.node = ast.first(self.block);
                    self.phase = VisitPhase::OuterCheck;
                }
                VisitPhase::OuterCheck => {
                    self.node?;
                    self.phase = VisitPhase::Inner;
                }
                VisitPhase::Inner => match self.node {
                    // A child block: visit its nodes.
                    Some(n) if ast.is_block(n) => {
                        self.block = n;
                        self.node = ast.first(n);
                        return Some(VisitEvent::Enter(n));
                    }
                    Some(n) => {
                        self.node = ast.node(n).next;
                        self.phase = VisitPhase::GoUp;
                        return Some(VisitEvent::Node(n));
                    }
                    // An empty block.
                    None => self.phase = VisitPhase::GoUp,
                },
                VisitPhase::GoUp => {
                    // At the end of a list: back up to the parent blocks.
                    if self.node.is_none() && ast.block_type(self.block) != AstBlockType::Main {
                        let left = self.block;
                        self.node = ast.node(left).next;
                        self.block = ast.parent(left).unwrap();
                        return Some(VisitEvent::Left(left));
                    }
                    self.phase = VisitPhase::OuterCheck;
                }
            }
        }
    }
}

/// A function's structured form (StructuredFunction).
pub struct StructuredFunction {
    pub main_block: AstId,
    pub name: String,
    pub return_type: AggregateType,
    pub in_arguments: Vec<AggregateType>,
    pub out_arguments: Vec<AggregateType>,
    pub locals: NetSet<AstId>,
}

impl StructuredFunction {
    pub fn argument_type(&self, index: usize) -> AggregateType {
        if index >= self.in_arguments.len() {
            self.out_arguments[index - self.in_arguments.len()]
        } else {
            self.in_arguments[index]
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct IoDefinition {
    pub storage_kind: StorageKind,
    pub io_variable: IoVariable,
    pub location: i32,
    pub component: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct HelperFunctionsMask(pub u32);

impl HelperFunctionsMask {
    pub const MULTIPLY_HIGH_S32: u32 = 1 << 2;
    pub const MULTIPLY_HIGH_U32: u32 = 1 << 3;
    pub const SWIZZLE_ADD: u32 = 1 << 10;
    pub const FSI: u32 = 1 << 11;
}

/// The structured program (StructuredProgramInfo).
pub struct StructuredProgramInfo {
    pub ast: Ast,
    pub functions: Vec<StructuredFunction>,
    pub io_definitions: NetSet<IoDefinition>,
    pub helper_functions_mask: HelperFunctionsMask,
}

/// OperandInfo.GetVarType.
pub fn operand_var_type(ast: &Ast, operand: AstId) -> Result<AggregateType, String> {
    let o = ast.opnd(operand);
    if o.ty == OperandType::LocalVariable {
        Ok(o.var_type)
    } else {
        var_type_of(o.ty)
    }
}

pub fn var_type_of(ty: OperandType) -> Result<AggregateType, String> {
    match ty {
        OperandType::Argument | OperandType::Constant | OperandType::Undefined => Ok(AggregateType::S32),
        t => Err(format!("System.ArgumentException: Invalid operand type \"{t:?}\".")),
    }
}
