// Intermediate representation (Ryujinx.Graphics.Shader.IntermediateRepresentation), held in arenas: operands, nodes
// (operations and phi nodes) and basic blocks are addressed by index. A block's operations form a doubly linked list
// of nodes, like the original's LinkedList<INode>.

mod instruction;

pub use instruction::Instruction;

use crate::collections::NetSet;
use crate::decoders::register::{Register, RegisterType};
use crate::types::{SamplerType, SetBindingPair, TextureFormat};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OperandId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OperandType {
    Argument,
    Constant,
    ConstantBuffer,
    Label,
    LocalVariable,
    Register,
    Undefined,
}

pub mod ir_consts {
    pub const FALSE: i32 = 0;
    pub const TRUE: i32 = -1;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum StorageKind {
    #[default]
    None,
    Input,
    InputPerPatch,
    Output,
    OutputPerPatch,
    ConstantBuffer,
    StorageBuffer,
    LocalMemory,
    SharedMemory,
    SharedMemory8,
    SharedMemory16,
    GlobalMemory,
    GlobalMemoryS8,
    GlobalMemoryS16,
    GlobalMemoryU8,
    GlobalMemoryU16,
}

impl StorageKind {
    pub fn is_input_or_output(self) -> bool {
        matches!(self, Self::Input | Self::InputPerPatch | Self::Output | Self::OutputPerPatch)
    }

    pub fn is_output(self) -> bool {
        matches!(self, Self::Output | Self::OutputPerPatch)
    }

    pub fn is_per_patch(self) -> bool {
        matches!(self, Self::InputPerPatch | Self::OutputPerPatch)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum IoVariable {
    Invalid,
    BackColorDiffuse,
    BackColorSpecular,
    BaseInstance,
    BaseVertex,
    ClipDistance,
    CtaId,
    DrawIndex,
    FogCoord,
    FragmentCoord,
    FragmentOutputColor,
    FragmentOutputDepth,
    FrontColorDiffuse,
    FrontColorSpecular,
    FrontFacing,
    GlobalId,
    InstanceId,
    InstanceIndex,
    InvocationId,
    Layer,
    PatchVertices,
    PointCoord,
    PointSize,
    Position,
    PrimitiveId,
    SubgroupEqMask,
    SubgroupGeMask,
    SubgroupGtMask,
    SubgroupLaneId,
    SubgroupLeMask,
    SubgroupLtMask,
    TessellationCoord,
    TessellationLevelInner,
    TessellationLevelOuter,
    TextureCoord,
    ThreadId,
    ThreadKill,
    UserDefined,
    VertexId,
    VertexIndex,
    ViewportIndex,
    ViewportMask,
}

impl IoVariable {
    const ALL: [IoVariable; 42] = [
        Self::Invalid, Self::BackColorDiffuse, Self::BackColorSpecular, Self::BaseInstance, Self::BaseVertex,
        Self::ClipDistance, Self::CtaId, Self::DrawIndex, Self::FogCoord, Self::FragmentCoord, Self::FragmentOutputColor,
        Self::FragmentOutputDepth, Self::FrontColorDiffuse, Self::FrontColorSpecular, Self::FrontFacing, Self::GlobalId,
        Self::InstanceId, Self::InstanceIndex, Self::InvocationId, Self::Layer, Self::PatchVertices, Self::PointCoord,
        Self::PointSize, Self::Position, Self::PrimitiveId, Self::SubgroupEqMask, Self::SubgroupGeMask,
        Self::SubgroupGtMask, Self::SubgroupLaneId, Self::SubgroupLeMask, Self::SubgroupLtMask, Self::TessellationCoord,
        Self::TessellationLevelInner, Self::TessellationLevelOuter, Self::TextureCoord, Self::ThreadId, Self::ThreadKill,
        Self::UserDefined, Self::VertexId, Self::VertexIndex, Self::ViewportIndex, Self::ViewportMask,
    ];

    /// The variable an operand's value names (operations carry it as a constant source).
    pub fn from_value(v: i32) -> IoVariable {
        Self::ALL.get(v as usize).copied().unwrap_or(Self::Invalid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct TextureFlags(pub u32);

impl TextureFlags {
    pub const NONE: Self = Self(0);
    pub const BINDLESS: Self = Self(1 << 0);
    pub const GATHER: Self = Self(1 << 1);
    pub const DERIVATIVES: Self = Self(1 << 2);
    pub const INT_COORDS: Self = Self(1 << 3);
    pub const LOD_BIAS: Self = Self(1 << 4);
    pub const LOD_LEVEL: Self = Self(1 << 5);
    pub const OFFSET: Self = Self(1 << 6);
    pub const OFFSETS: Self = Self(1 << 7);
    pub const COHERENT: Self = Self(1 << 8);
    pub const ATOMIC_MASK: Self = Self(15 << 16);
    pub const ADD: Self = Self(0);
    pub const MINIMUM: Self = Self(1 << 16);
    pub const MAXIMUM: Self = Self(2 << 16);
    pub const INCREMENT: Self = Self(3 << 16);
    pub const DECREMENT: Self = Self(4 << 16);
    pub const BITWISE_AND: Self = Self(5 << 16);
    pub const BITWISE_OR: Self = Self(6 << 16);
    pub const BITWISE_XOR: Self = Self(7 << 16);
    pub const SWAP: Self = Self(8 << 16);
    pub const CAS: Self = Self(9 << 16);

    pub fn has(self, f: Self) -> bool {
        self.0 & f.0 != 0
    }
}

impl std::ops::BitOr for TextureFlags {
    type Output = Self;
    fn bitor(self, o: Self) -> Self {
        Self(self.0 | o.0)
    }
}

impl std::ops::BitOrAssign for TextureFlags {
    fn bitor_assign(&mut self, o: Self) {
        self.0 |= o.0;
    }
}

impl std::ops::BitAnd for TextureFlags {
    type Output = Self;
    fn bitand(self, o: Self) -> Self {
        Self(self.0 & o.0)
    }
}

impl std::ops::Not for TextureFlags {
    type Output = Self;
    fn not(self) -> Self {
        Self(!self.0)
    }
}

pub struct OperandData {
    pub ty: OperandType,
    pub value: i32,
    pub asg_op: Option<NodeId>,
    pub use_ops: NetSet<NodeId>,
}

const CBUF_SLOT_BITS: i32 = 5;
const CBUF_SLOT_LSB: i32 = 32 - CBUF_SLOT_BITS;
const CBUF_SLOT_MASK: i32 = (1 << CBUF_SLOT_BITS) - 1;

impl OperandData {
    pub fn cbuf_slot(&self) -> i32 {
        (self.value >> CBUF_SLOT_LSB) & CBUF_SLOT_MASK
    }

    pub fn cbuf_offset(&self) -> i32 {
        self.value & !(CBUF_SLOT_MASK << CBUF_SLOT_LSB)
    }

    pub fn register(&self) -> Register {
        Register::new(self.value & 0xffffff, RegisterType::from_i32(self.value >> 24))
    }

    pub fn as_float(&self) -> f32 {
        f32::from_bits(self.value as u32)
    }
}

/// TextureOperation's fields.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureInfo {
    pub ty: SamplerType,
    pub format: TextureFormat,
    pub flags: TextureFlags,
    pub set: i32,
    pub binding: i32,
    pub sampler_set: i32,
    pub sampler_binding: i32,
}

pub const DEFAULT_CBUF_SLOT: i32 = -1;

pub struct Operation {
    pub inst: Instruction,
    pub storage_kind: StorageKind,
    pub force_precise: bool,
    pub dests: Vec<Option<OperandId>>,
    pub sources: Vec<Option<OperandId>>,
    pub index: i32,
    pub comment: Option<String>,
    pub tex: Option<TextureInfo>,
}

pub struct PhiNode {
    pub dest: Option<OperandId>,
    blocks: NetSet<BlockId>,
    pub sources: Vec<(BlockId, OperandId)>,
}

pub enum NodeKind {
    Op(Operation),
    Phi(PhiNode),
}

pub struct Node {
    pub kind: NodeKind,
    prev: Option<NodeId>,
    next: Option<NodeId>,
    list: Option<BlockId>,
}

#[derive(Default)]
pub struct BasicBlock {
    pub index: i32,
    first: Option<NodeId>,
    last: Option<NodeId>,
    count: usize,
    next: Option<BlockId>,
    branch: Option<BlockId>,
    pub predecessors: Vec<BlockId>,
    pub dominance_frontiers: NetSet<BlockId>,
    pub immediate_dominator: Option<BlockId>,
}

pub struct Function {
    pub blocks: Vec<BlockId>,
    pub name: String,
    pub returns_value: bool,
    pub in_arguments_count: i32,
    pub out_arguments_count: i32,
}

#[derive(Default)]
pub struct Ir {
    pub operands: Vec<OperandData>,
    pub nodes: Vec<Node>,
    pub blocks: Vec<BasicBlock>,
}

impl Ir {
    pub fn new() -> Ir {
        Ir::default()
    }

    // Operands (OperandHelper).

    pub fn new_operand(&mut self, ty: OperandType, value: i32) -> OperandId {
        self.operands.push(OperandData { ty, value, asg_op: None, use_ops: NetSet::new() });
        OperandId(self.operands.len() as u32 - 1)
    }

    pub fn argument(&mut self, value: i32) -> OperandId {
        self.new_operand(OperandType::Argument, value)
    }

    pub fn cbuf(&mut self, slot: i32, offset: i32) -> OperandId {
        self.new_operand(OperandType::ConstantBuffer, (slot << CBUF_SLOT_LSB) | offset)
    }

    pub fn const_i(&mut self, value: i32) -> OperandId {
        self.new_operand(OperandType::Constant, value)
    }

    pub fn const_f(&mut self, value: f32) -> OperandId {
        self.new_operand(OperandType::Constant, value.to_bits() as i32)
    }

    pub fn label(&mut self) -> OperandId {
        self.new_operand(OperandType::Label, 0)
    }

    pub fn local(&mut self) -> OperandId {
        self.new_operand(OperandType::LocalVariable, 0)
    }

    pub fn undef(&mut self) -> OperandId {
        self.new_operand(OperandType::Undefined, 0)
    }

    pub fn register(&mut self, reg: Register) -> OperandId {
        if reg.is_rz() {
            self.const_i(0)
        } else if reg.is_pt() {
            self.const_i(ir_consts::TRUE)
        } else {
            self.new_operand(OperandType::Register, ((reg.ty as i32) << 24) | reg.index)
        }
    }

    /// new Operand(Register): a register operand even for RZ / PT.
    pub fn register_raw(&mut self, reg: Register) -> OperandId {
        self.new_operand(OperandType::Register, ((reg.ty as i32) << 24) | reg.index)
    }

    pub fn reg(&mut self, index: i32, ty: RegisterType) -> OperandId {
        self.register(Register::new(index, ty))
    }

    pub fn opnd(&self, o: OperandId) -> &OperandData {
        &self.operands[o.0 as usize]
    }

    pub fn opnd_mut(&mut self, o: OperandId) -> &mut OperandData {
        &mut self.operands[o.0 as usize]
    }

    pub fn ty(&self, o: OperandId) -> OperandType {
        self.opnd(o).ty
    }

    pub fn value(&self, o: OperandId) -> i32 {
        self.opnd(o).value
    }

    pub fn is_local(&self, o: OperandId) -> bool {
        self.ty(o) == OperandType::LocalVariable
    }

    pub fn is_const(&self, o: OperandId) -> bool {
        self.ty(o) == OperandType::Constant
    }

    pub fn asg_op(&self, o: OperandId) -> Option<NodeId> {
        self.opnd(o).asg_op
    }

    pub fn use_ops(&self, o: OperandId) -> &NetSet<NodeId> {
        &self.opnd(o).use_ops
    }

    fn add_use(&mut self, o: Option<OperandId>, n: NodeId) {
        if let Some(o) = o {
            if self.is_local(o) {
                self.opnd_mut(o).use_ops.insert(n);
            }
        }
    }

    fn remove_use(&mut self, o: Option<OperandId>, n: NodeId) {
        if let Some(o) = o {
            if self.is_local(o) {
                self.opnd_mut(o).use_ops.remove(&n);
            }
        }
    }

    fn set_asg(&mut self, o: Option<OperandId>, n: Option<NodeId>) {
        if let Some(o) = o {
            if self.is_local(o) {
                self.opnd_mut(o).asg_op = n;
            }
        }
    }

    // Nodes.

    fn push_node(&mut self, kind: NodeKind) -> NodeId {
        self.nodes.push(Node { kind, prev: None, next: None, list: None });
        NodeId(self.nodes.len() as u32 - 1)
    }

    /// new Operation(inst, index, dests, sources): every local destination's assignment is the new operation.
    pub fn operation_multi(&mut self, inst: Instruction, index: i32, dests: &[Option<OperandId>], sources: &[OperandId]) -> NodeId {
        let n = self.push_node(NodeKind::Op(Operation {
            inst,
            storage_kind: StorageKind::None,
            force_precise: false,
            dests: dests.to_vec(),
            sources: sources.iter().map(|&s| Some(s)).collect(),
            index,
            comment: None,
            tex: None,
        }));
        for &s in sources {
            self.add_use(Some(s), n);
        }
        for &d in dests {
            self.set_asg(d, Some(n));
        }
        n
    }

    /// new Operation(inst, storageKind, dest, sources): the destination's assignment is the new operation whatever
    /// its type (as in the original).
    pub fn operation_sk(&mut self, inst: Instruction, storage_kind: StorageKind, dest: Option<OperandId>, sources: &[OperandId]) -> NodeId {
        let n = self.push_node(NodeKind::Op(Operation {
            inst,
            storage_kind,
            force_precise: false,
            dests: dest.into_iter().map(Some).collect(),
            sources: sources.iter().map(|&s| Some(s)).collect(),
            index: 0,
            comment: None,
            tex: None,
        }));
        for &s in sources {
            self.add_use(Some(s), n);
        }
        if let Some(d) = dest {
            self.opnd_mut(d).asg_op = Some(n);
        }
        n
    }

    pub fn operation(&mut self, inst: Instruction, dest: Option<OperandId>, sources: &[OperandId]) -> NodeId {
        self.operation_sk(inst, StorageKind::None, dest, sources)
    }

    /// new Operation(inst, index, dest, sources).
    pub fn operation_idx(&mut self, inst: Instruction, index: i32, dest: Option<OperandId>, sources: &[OperandId]) -> NodeId {
        let n = self.operation(inst, dest, sources);
        self.op_mut(n).index = index;
        n
    }

    pub fn comment(&mut self, text: String) -> NodeId {
        let n = self.operation(Instruction::COMMENT, None, &[]);
        self.op_mut(n).comment = Some(text);
        n
    }

    #[allow(clippy::too_many_arguments)]
    pub fn texture_operation(
        &mut self,
        inst: Instruction,
        ty: SamplerType,
        format: TextureFormat,
        flags: TextureFlags,
        set: i32,
        binding: i32,
        comp_index: i32,
        dests: &[Option<OperandId>],
        sources: &[OperandId],
    ) -> NodeId {
        let n = self.operation_multi(inst, comp_index, dests, sources);
        self.op_mut(n).tex = Some(TextureInfo { ty, format, flags, set, binding, sampler_set: -1, sampler_binding: -1 });
        n
    }

    pub fn phi(&mut self, dest: OperandId) -> NodeId {
        let n = self.push_node(NodeKind::Phi(PhiNode { dest: Some(dest), blocks: NetSet::new(), sources: Vec::new() }));
        self.opnd_mut(dest).asg_op = Some(n);
        n
    }

    pub fn node(&self, n: NodeId) -> &Node {
        &self.nodes[n.0 as usize]
    }

    pub fn is_op(&self, n: NodeId) -> bool {
        matches!(self.node(n).kind, NodeKind::Op(_))
    }

    pub fn is_phi(&self, n: NodeId) -> bool {
        matches!(self.node(n).kind, NodeKind::Phi(_))
    }

    pub fn as_op(&self, n: NodeId) -> Option<&Operation> {
        match &self.node(n).kind {
            NodeKind::Op(o) => Some(o),
            NodeKind::Phi(_) => None,
        }
    }

    pub fn op(&self, n: NodeId) -> &Operation {
        self.as_op(n).expect("not an operation")
    }

    pub fn op_mut(&mut self, n: NodeId) -> &mut Operation {
        match &mut self.nodes[n.0 as usize].kind {
            NodeKind::Op(o) => o,
            NodeKind::Phi(_) => panic!("not an operation"),
        }
    }

    pub fn phi_node(&self, n: NodeId) -> &PhiNode {
        match &self.node(n).kind {
            NodeKind::Phi(p) => p,
            NodeKind::Op(_) => panic!("not a phi node"),
        }
    }

    fn phi_mut(&mut self, n: NodeId) -> &mut PhiNode {
        match &mut self.nodes[n.0 as usize].kind {
            NodeKind::Phi(p) => p,
            NodeKind::Op(_) => panic!("not a phi node"),
        }
    }

    pub fn inst(&self, n: NodeId) -> Instruction {
        self.op(n).inst
    }

    /// The operation's instruction, or None for phi nodes ("node is Operation operation && ...").
    pub fn inst_of(&self, n: NodeId) -> Option<Instruction> {
        self.as_op(n).map(|o| o.inst)
    }

    pub fn tex(&self, n: NodeId) -> Option<&TextureInfo> {
        self.as_op(n).and_then(|o| o.tex.as_ref())
    }

    pub fn tex_mut(&mut self, n: NodeId) -> &mut TextureInfo {
        self.op_mut(n).tex.as_mut().expect("not a texture operation")
    }

    // INode.

    pub fn dest(&self, n: NodeId) -> Option<OperandId> {
        match &self.node(n).kind {
            NodeKind::Op(o) => o.dests.first().copied().flatten(),
            NodeKind::Phi(p) => p.dest,
        }
    }

    pub fn dests_count(&self, n: NodeId) -> usize {
        match &self.node(n).kind {
            NodeKind::Op(o) => o.dests.len(),
            NodeKind::Phi(p) => p.dest.is_some() as usize,
        }
    }

    pub fn sources_count(&self, n: NodeId) -> usize {
        match &self.node(n).kind {
            NodeKind::Op(o) => o.sources.len(),
            NodeKind::Phi(p) => p.sources.len(),
        }
    }

    pub fn get_dest(&self, n: NodeId, i: usize) -> Option<OperandId> {
        match &self.node(n).kind {
            NodeKind::Op(o) => o.dests[i],
            NodeKind::Phi(p) => {
                assert_eq!(i, 0);
                p.dest
            }
        }
    }

    /// A source that may have been cleared (Utils.RemoveNode leaves nulls behind).
    pub fn get_source_opt(&self, n: NodeId, i: usize) -> Option<OperandId> {
        match &self.node(n).kind {
            NodeKind::Op(o) => o.sources[i],
            NodeKind::Phi(p) => Some(p.sources[i].1),
        }
    }

    pub fn src(&self, n: NodeId, i: usize) -> OperandId {
        self.get_source_opt(n, i).expect("null source")
    }

    pub fn sources(&self, n: NodeId) -> Vec<OperandId> {
        (0..self.sources_count(n)).map(|i| self.src(n, i)).collect()
    }

    /// Operation.Dest = value.
    pub fn set_dest_single(&mut self, n: NodeId, dest: Option<OperandId>) {
        match dest {
            Some(d) => {
                self.set_asg(Some(d), Some(n));
                self.op_mut(n).dests = vec![Some(d)];
            }
            None => self.op_mut(n).dests = Vec::new(),
        }
    }

    /// PhiNode.Dest = value.
    pub fn set_phi_dest(&mut self, n: NodeId, dest: Option<OperandId>) {
        self.set_asg(dest, Some(n));
        self.phi_mut(n).dest = dest;
    }

    pub fn set_dest(&mut self, n: NodeId, i: usize, dest: Option<OperandId>) {
        let old = self.op(n).dests[i];
        self.set_asg(old, None);
        self.set_asg(dest, Some(n));
        self.op_mut(n).dests[i] = dest;
    }

    pub fn set_source(&mut self, n: NodeId, i: usize, src: Option<OperandId>) {
        let old = self.get_source_opt(n, i);
        self.remove_use(old, n);
        self.add_use(src, n);
        match &mut self.nodes[n.0 as usize].kind {
            NodeKind::Op(o) => o.sources[i] = src,
            NodeKind::Phi(p) => p.sources[i].1 = src.expect("phi sources cannot be null"),
        }
    }

    pub fn append_dests(&mut self, n: NodeId, dests: &[Option<OperandId>]) {
        for &d in dests {
            self.set_asg(d, Some(n));
            self.op_mut(n).dests.push(d);
        }
    }

    pub fn append_sources(&mut self, n: NodeId, sources: &[OperandId]) {
        for &s in sources {
            self.add_use(Some(s), n);
            self.op_mut(n).sources.push(Some(s));
        }
    }

    pub fn insert_source(&mut self, n: NodeId, i: usize, src: OperandId) {
        self.add_use(Some(src), n);
        self.op_mut(n).sources.insert(i, Some(src));
    }

    pub fn remove_source(&mut self, n: NodeId, i: usize) {
        self.set_source(n, i, None);
        self.op_mut(n).sources.remove(i);
    }

    pub fn turn_into_copy(&mut self, n: NodeId, src: OperandId) {
        self.turn_into(n, Instruction::COPY, src);
    }

    pub fn turn_into(&mut self, n: NodeId, inst: Instruction, src: OperandId) {
        self.op_mut(n).inst = inst;
        let old = std::mem::take(&mut self.op_mut(n).sources);
        for o in old {
            self.remove_use(o, n);
        }
        self.add_use(Some(src), n);
        self.op_mut(n).sources = vec![Some(src)];
    }

    pub fn turn_double_into_float(&mut self, n: NodeId) {
        let inst = self.inst(n);
        let new = if inst.0 & !Instruction::MASK.0 == Instruction::FP64.0 {
            Instruction(inst.0 & Instruction::MASK.0) | Instruction::FP32
        } else {
            match inst {
                Instruction::CONVERT_FP32_TO_FP64 | Instruction::CONVERT_FP64_TO_FP32 => Instruction::COPY,
                Instruction::CONVERT_FP64_TO_S32 => Instruction::CONVERT_FP32_TO_S32,
                Instruction::CONVERT_FP64_TO_U32 => Instruction::CONVERT_FP32_TO_U32,
                Instruction::CONVERT_S32_TO_FP64 => Instruction::CONVERT_S32_TO_FP32,
                Instruction::CONVERT_U32_TO_FP64 => Instruction::CONVERT_U32_TO_FP32,
                i => i,
            }
        };
        self.op_mut(n).inst = new;
    }

    // TextureOperation.

    pub fn turn_into_array(&mut self, n: NodeId, sb: SetBindingPair) {
        let t = self.tex_mut(n);
        t.flags = t.flags & !TextureFlags::BINDLESS;
        t.set = sb.set_index;
        t.binding = sb.binding;
    }

    pub fn turn_into_array_with_sampler(&mut self, n: NodeId, tsb: SetBindingPair, ssb: SetBindingPair) {
        self.turn_into_array(n, tsb);
        let t = self.tex_mut(n);
        t.sampler_set = ssb.set_index;
        t.sampler_binding = ssb.binding;
    }

    pub fn set_binding(&mut self, n: NodeId, sb: SetBindingPair) {
        if self.tex_mut(n).flags.has(TextureFlags::BINDLESS) {
            let t = self.tex_mut(n);
            t.flags = t.flags & !TextureFlags::BINDLESS;
            self.remove_source(n, 0);
        }
        let t = self.tex_mut(n);
        t.set = sb.set_index;
        t.binding = sb.binding;
    }

    pub fn set_lod_level_flag(&mut self, n: NodeId) {
        self.tex_mut(n).flags |= TextureFlags::LOD_LEVEL;
    }

    // PhiNode.

    pub fn phi_add_source(&mut self, n: NodeId, block: BlockId, src: OperandId) {
        if self.phi_mut(n).blocks.insert(block) {
            self.add_use(Some(src), n);
            self.phi_mut(n).sources.push((block, src));
        }
    }

    pub fn phi_block(&self, n: NodeId, i: usize) -> BlockId {
        self.phi_node(n).sources[i].0
    }

    // Basic blocks.

    pub fn new_block(&mut self, index: i32) -> BlockId {
        self.blocks.push(BasicBlock { index, ..Default::default() });
        BlockId(self.blocks.len() as u32 - 1)
    }

    pub fn block(&self, b: BlockId) -> &BasicBlock {
        &self.blocks[b.0 as usize]
    }

    pub fn block_mut(&mut self, b: BlockId) -> &mut BasicBlock {
        &mut self.blocks[b.0 as usize]
    }

    pub fn next(&self, b: BlockId) -> Option<BlockId> {
        self.block(b).next
    }

    pub fn branch(&self, b: BlockId) -> Option<BlockId> {
        self.block(b).branch
    }

    pub fn has_branch(&self, b: BlockId) -> bool {
        self.block(b).branch.is_some()
    }

    pub fn reachable(&self, b: BlockId) -> bool {
        self.block(b).index == 0 || !self.block(b).predecessors.is_empty()
    }

    fn replace_successor(&mut self, b: BlockId, old: Option<BlockId>, new: Option<BlockId>) {
        if let Some(o) = old {
            let preds = &mut self.block_mut(o).predecessors;
            if let Some(p) = preds.iter().position(|&x| x == b) {
                preds.remove(p);
            }
        }
        if let Some(n) = new {
            self.block_mut(n).predecessors.push(b);
        }
    }

    pub fn set_next(&mut self, b: BlockId, next: Option<BlockId>) {
        let old = self.block(b).next;
        self.replace_successor(b, old, next);
        self.block_mut(b).next = next;
    }

    pub fn set_branch(&mut self, b: BlockId, branch: Option<BlockId>) {
        let old = self.block(b).branch;
        self.replace_successor(b, old, branch);
        self.block_mut(b).branch = branch;
    }

    pub fn first_node(&self, b: BlockId) -> Option<NodeId> {
        self.block(b).first
    }

    pub fn last_node(&self, b: BlockId) -> Option<NodeId> {
        self.block(b).last
    }

    pub fn node_count(&self, b: BlockId) -> usize {
        self.block(b).count
    }

    pub fn next_node(&self, n: NodeId) -> Option<NodeId> {
        self.node(n).next
    }

    pub fn prev_node(&self, n: NodeId) -> Option<NodeId> {
        self.node(n).prev
    }

    /// The block whose list holds `n` (LinkedListNode.List), if any.
    pub fn list_of(&self, n: NodeId) -> Option<BlockId> {
        self.node(n).list
    }

    pub fn block_nodes(&self, b: BlockId) -> Vec<NodeId> {
        let mut v = Vec::with_capacity(self.block(b).count);
        let mut n = self.block(b).first;
        while let Some(x) = n {
            v.push(x);
            n = self.node(x).next;
        }
        v
    }

    fn link(&mut self, b: BlockId, n: NodeId, prev: Option<NodeId>, next: Option<NodeId>) {
        assert!(self.node(n).list.is_none(), "node already in a list");
        {
            let node = &mut self.nodes[n.0 as usize];
            node.prev = prev;
            node.next = next;
            node.list = Some(b);
        }
        match prev {
            Some(p) => self.nodes[p.0 as usize].next = Some(n),
            None => self.block_mut(b).first = Some(n),
        }
        match next {
            Some(x) => self.nodes[x.0 as usize].prev = Some(n),
            None => self.block_mut(b).last = Some(n),
        }
        self.block_mut(b).count += 1;
    }

    pub fn add_last(&mut self, b: BlockId, n: NodeId) {
        let last = self.block(b).last;
        self.link(b, n, last, None);
    }

    pub fn add_first(&mut self, b: BlockId, n: NodeId) {
        let first = self.block(b).first;
        self.link(b, n, None, first);
    }

    /// node.List.AddBefore(node, n): returns `n`.
    pub fn add_before(&mut self, at: NodeId, n: NodeId) -> NodeId {
        let b = self.node(at).list.expect("node not in a list");
        let prev = self.node(at).prev;
        self.link(b, n, prev, Some(at));
        n
    }

    pub fn add_after(&mut self, at: NodeId, n: NodeId) -> NodeId {
        let b = self.node(at).list.expect("node not in a list");
        let next = self.node(at).next;
        self.link(b, n, Some(at), next);
        n
    }

    pub fn remove_node(&mut self, n: NodeId) {
        let b = self.node(n).list.expect("node not in a list");
        let (prev, next) = (self.node(n).prev, self.node(n).next);
        match prev {
            Some(p) => self.nodes[p.0 as usize].next = next,
            None => self.block_mut(b).first = next,
        }
        match next {
            Some(x) => self.nodes[x.0 as usize].prev = prev,
            None => self.block_mut(b).last = prev,
        }
        let node = &mut self.nodes[n.0 as usize];
        node.prev = None;
        node.next = None;
        node.list = None;
        self.block_mut(b).count -= 1;
    }

    pub fn get_last_op(&self, b: BlockId) -> Option<NodeId> {
        self.block(b).last
    }

    /// BasicBlock.Append: before a final control flow instruction.
    pub fn append(&mut self, b: BlockId, n: NodeId) {
        match self.block(b).last {
            Some(last) if self.inst_of(last).is_some_and(is_control_flow_inst) => {
                self.add_before(last, n);
            }
            _ => self.add_last(b, n),
        }
    }
}

fn is_control_flow_inst(inst: Instruction) -> bool {
    matches!(
        inst,
        Instruction::BRANCH | Instruction::BRANCH_IF_FALSE | Instruction::BRANCH_IF_TRUE | Instruction::DISCARD | Instruction::RETURN
    )
}
