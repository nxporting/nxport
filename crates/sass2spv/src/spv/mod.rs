// SPIR-V module builder, ported from Ryujinx's Spv.Generator: the same id allocation, deduplication and section order,
// so a module built by the same calls is the same binary.


pub mod grammar;
pub mod spec;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

pub use spec::*;

pub const INVALID_ID: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LitKind {
    U32,
    I32,
    U64,
    I64,
    F32,
    F64,
}

/// A literal operand (LiteralInteger): the value's kind takes part in equality, like the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lit {
    kind: LitKind,
    data: u64,
    words: u16,
}

impl Lit {
    pub fn i32(v: i32) -> Lit {
        Lit { kind: LitKind::I32, data: v as i64 as u64, words: 1 }
    }
    pub fn u32(v: u32) -> Lit {
        Lit { kind: LitKind::U32, data: v as u64, words: 1 }
    }
    pub fn i64(v: i64) -> Lit {
        Lit { kind: LitKind::I64, data: v as u64, words: 2 }
    }
    pub fn u64(v: u64) -> Lit {
        Lit { kind: LitKind::U64, data: v, words: 2 }
    }
    pub fn f32(v: f32) -> Lit {
        Lit { kind: LitKind::F32, data: v.to_bits() as u64, words: 1 }
    }
    pub fn f64(v: f64) -> Lit {
        Lit { kind: LitKind::F64, data: v.to_bits(), words: 2 }
    }
    pub fn bool(v: bool) -> Lit {
        Lit { kind: LitKind::I32, data: v as u64, words: 1 }
    }
    /// An enumerant (written as a signed 32-bit integer).
    pub fn en(v: u32) -> Lit {
        Lit::i32(v as i32)
    }
}

#[derive(Clone)]
pub enum Operand {
    Inst(Inst),
    Lit(Lit),
    Str(String),
}

impl Operand {
    fn word_count(&self) -> u16 {
        match self {
            Operand::Inst(_) => 1,
            Operand::Lit(l) => l.words,
            Operand::Str(s) => (s.len() / 4 + 1) as u16,
        }
    }

    fn write(&self, w: &mut Vec<u8>) {
        match self {
            Operand::Inst(i) => i.write_operand(w),
            Operand::Lit(l) => {
                if l.words == 1 {
                    w.extend_from_slice(&(l.data as u32).to_le_bytes());
                } else {
                    w.extend_from_slice(&l.data.to_le_bytes());
                }
            }
            Operand::Str(s) => {
                w.extend_from_slice(s.as_bytes());
                w.extend(std::iter::repeat(0).take(4 - s.len() % 4));
            }
        }
    }

    fn equals(&self, o: &Operand) -> bool {
        match (self, o) {
            (Operand::Inst(a), Operand::Inst(b)) => a.equals(b),
            (Operand::Lit(a), Operand::Lit(b)) => a == b,
            (Operand::Str(a), Operand::Str(b)) => a == b,
            _ => false,
        }
    }

    fn fingerprint<H: Hasher>(&self, h: &mut H) {
        match self {
            Operand::Inst(i) => (0u8, i.id()).hash(h),
            Operand::Lit(l) => (1u8, l.data).hash(h),
            Operand::Str(s) => (2u8, s).hash(h),
        }
    }
}

impl From<Inst> for Operand {
    fn from(i: Inst) -> Self {
        Operand::Inst(i)
    }
}

impl From<&Inst> for Operand {
    fn from(i: &Inst) -> Self {
        Operand::Inst(i.clone())
    }
}

impl From<Lit> for Operand {
    fn from(l: Lit) -> Self {
        Operand::Lit(l)
    }
}

impl From<&str> for Operand {
    fn from(s: &str) -> Self {
        Operand::Str(s.to_string())
    }
}

impl From<String> for Operand {
    fn from(s: String) -> Self {
        Operand::Str(s)
    }
}

macro_rules! enum_operands {
    ($($t:ty),*) => {$(
        impl From<$t> for Operand {
            fn from(v: $t) -> Self {
                Operand::Lit(Lit::en(v.0))
            }
        }
    )*};
}

enum_operands!(
    SourceLanguage, ExecutionModel, AddressingModel, MemoryModel, ExecutionMode, StorageClass, Dim,
    SamplerAddressingMode, SamplerFilterMode, ImageFormat, ImageChannelOrder, ImageChannelDataType, ImageOperandsMask,
    FPFastMathModeMask, FPRoundingMode, LinkageType, AccessQualifier, FunctionParameterAttribute, Decoration, BuiltIn,
    SelectionControlMask, LoopControlMask, FunctionControlMask, MemorySemanticsMask, MemoryAccessMask, Scope,
    GroupOperation, KernelEnqueueFlags, KernelProfilingInfoMask, Capability, RayFlagsMask, RayQueryIntersection,
    RayQueryCommittedIntersectionType, RayQueryCandidateIntersectionType, FragmentShadingRateMask
);

pub struct Instruction {
    pub opcode: Op,
    id: Cell<u32>,
    result_type: Option<Inst>,
    operands: RefCell<Vec<Operand>>,
}

pub type Inst = Rc<Instruction>;

impl Instruction {
    pub fn id(&self) -> u32 {
        self.id.get()
    }

    pub fn set_id(&self, id: u32) {
        self.id.set(id);
    }

    pub fn add_operand(&self, o: impl Into<Operand>) {
        self.operands.borrow_mut().push(o.into());
    }

    pub fn add_operands<O: Into<Operand> + Clone>(&self, os: &[O]) {
        let mut v = self.operands.borrow_mut();
        v.extend(os.iter().cloned().map(Into::into));
    }

    fn total_word_count(&self) -> u16 {
        let mut n = 1;
        if self.id() != INVALID_ID {
            n += 1;
        }
        if self.result_type.is_some() {
            n += 1;
        }
        n + self.operands.borrow().iter().map(|o| o.word_count()).sum::<u16>()
    }

    fn write(&self, w: &mut Vec<u8>) {
        w.extend_from_slice(&(self.opcode.0 as u16).to_le_bytes());
        w.extend_from_slice(&self.total_word_count().to_le_bytes());
        if let Some(rt) = &self.result_type {
            rt.write_operand(w);
        }
        if self.id() != INVALID_ID {
            w.extend_from_slice(&self.id().to_le_bytes());
        }
        for o in self.operands.borrow().iter() {
            o.write(w);
        }
    }

    fn write_operand(&self, w: &mut Vec<u8>) {
        assert!(self.id() != INVALID_ID, "SPIR-V: {:?} used as an operand before it got an id", self.opcode);
        w.extend_from_slice(&self.id().to_le_bytes());
    }

    fn equals(&self, o: &Instruction) -> bool {
        if self.id() != o.id() {
            return false;
        }
        match (&self.result_type, &o.result_type) {
            (Some(a), Some(b)) if !a.equals(b) => return false,
            (Some(_), None) | (None, Some(_)) => return false,
            _ => {}
        }
        self.equals_content(o)
    }

    fn equals_content(&self, o: &Instruction) -> bool {
        let (a, b) = (self.operands.borrow(), o.operands.borrow());
        a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x.equals(y))
    }

    fn content_fingerprint(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.opcode.hash(&mut h);
        for o in self.operands.borrow().iter() {
            o.fingerprint(&mut h);
        }
        h.finish()
    }
}

/// Instructions deduplicated by a key equality (type declarations, constants): buckets by a fingerprint consistent
/// with that equality, compared exactly.
struct Dedup {
    buckets: HashMap<u64, Vec<Inst>>,
}

impl Dedup {
    fn new() -> Self {
        Dedup { buckets: HashMap::new() }
    }

    fn find(&self, i: &Instruction, eq: impl Fn(&Instruction, &Instruction) -> bool) -> Option<&Inst> {
        self.buckets.get(&i.content_fingerprint())?.iter().find(|e| eq(e, i))
    }

    fn insert(&mut self, i: Inst, eq: impl Fn(&Instruction, &Instruction) -> bool) {
        let b = self.buckets.entry(i.content_fingerprint()).or_default();
        match b.iter_mut().find(|e| eq(e, &i)) {
            Some(e) => *e = i,
            None => b.push(i),
        }
    }
}

fn type_key_eq(a: &Instruction, b: &Instruction) -> bool {
    a.opcode == b.opcode && a.equals_content(b)
}

fn constant_key_eq(a: &Instruction, b: &Instruction) -> bool {
    let (ra, rb) = (a.result_type.as_ref().unwrap(), b.result_type.as_ref().unwrap());
    a.opcode == b.opcode && a.equals_content(b) && ra.opcode == rb.opcode && ra.equals_content(rb)
}

pub struct Module {
    version: u32,
    bound: u32,
    capabilities: Vec<Capability>,
    extensions: Vec<String>,
    ext_inst_imports: Vec<(String, Inst)>,
    addressing_model: AddressingModel,
    memory_model: MemoryModel,
    entry_points: Vec<Inst>,
    execution_modes: Vec<Inst>,
    debug: Vec<Inst>,
    annotations: Vec<Inst>,
    type_declarations: Dedup,
    type_declarations_list: Vec<Inst>,
    globals: Vec<Inst>,
    constants: Dedup,
    constants_list: Vec<Inst>,
    function_declarations: Vec<Inst>,
    function_definitions: Vec<Inst>,
}

impl Module {
    pub fn new(version: u32) -> Module {
        Module {
            version,
            bound: 1,
            capabilities: Vec::new(),
            extensions: Vec::new(),
            ext_inst_imports: Vec::new(),
            addressing_model: AddressingModel::LOGICAL,
            memory_model: MemoryModel::SIMPLE,
            entry_points: Vec::new(),
            execution_modes: Vec::new(),
            debug: Vec::new(),
            annotations: Vec::new(),
            type_declarations: Dedup::new(),
            type_declarations_list: Vec::new(),
            globals: Vec::new(),
            constants: Dedup::new(),
            constants_list: Vec::new(),
            function_declarations: Vec::new(),
            function_definitions: Vec::new(),
        }
    }

    fn new_id(&mut self) -> u32 {
        let id = self.bound;
        self.bound += 1;
        id
    }

    pub fn add_capability(&mut self, c: Capability) {
        self.capabilities.push(c);
    }

    pub fn add_extension(&mut self, e: &str) {
        self.extensions.push(e.to_string());
    }

    pub fn new_instruction(&self, opcode: Op, id: u32, result_type: Option<&Inst>) -> Inst {
        Rc::new(Instruction {
            opcode,
            id: Cell::new(id),
            result_type: result_type.cloned(),
            operands: RefCell::new(Vec::new()),
        })
    }

    pub fn add_ext_inst_import(&mut self, import: &str) -> Inst {
        if let Some((_, i)) = self.ext_inst_imports.iter().find(|(k, _)| k == import) {
            return i.clone();
        }
        let i = self.new_instruction(Op::EXT_INST_IMPORT, INVALID_ID, None);
        i.add_operand(import);
        i.set_id(self.new_id());
        self.ext_inst_imports.push((import.to_string(), i.clone()));
        i
    }

    fn add_type_declaration(&mut self, i: &Inst, force_id_allocation: bool) {
        if !force_id_allocation {
            if let Some(t) = self.type_declarations.find(i, type_key_eq) {
                i.set_id(t.id());
                return;
            }
        }
        i.set_id(self.new_id());
        self.type_declarations.insert(i.clone(), type_key_eq);
        self.type_declarations_list.push(i.clone());
    }

    pub fn add_entry_point(&mut self, model: ExecutionModel, function: &Inst, name: &str, interfaces: &[Inst]) {
        debug_assert!(function.opcode == Op::FUNCTION);
        let e = self.new_instruction(Op::ENTRY_POINT, INVALID_ID, None);
        e.add_operand(model);
        e.add_operand(function);
        e.add_operand(name);
        e.add_operands(interfaces);
        self.entry_points.push(e);
    }

    pub fn add_execution_mode(&mut self, function: &Inst, mode: ExecutionMode, parameters: &[Operand]) {
        debug_assert!(function.opcode == Op::FUNCTION);
        let e = self.new_instruction(Op::EXECUTION_MODE, INVALID_ID, None);
        e.add_operand(function);
        e.add_operand(mode);
        e.add_operands(parameters);
        self.execution_modes.push(e);
    }

    pub fn add_function(&mut self, function: &Inst) {
        self.add_to_function_definitions(function);
    }

    fn add_to_function_definitions(&mut self, i: &Inst) {
        self.function_definitions.push(i.clone());
    }

    fn add_annotation(&mut self, i: &Inst) {
        self.annotations.push(i.clone());
    }

    fn add_debug(&mut self, i: &Inst) {
        self.debug.push(i.clone());
    }

    pub fn add_label(&mut self, label: &Inst) {
        debug_assert!(label.opcode == Op::LABEL);
        label.set_id(self.new_id());
        self.add_to_function_definitions(label);
    }

    pub fn add_local_variable(&mut self, v: &Inst) {
        debug_assert!(v.opcode == Op::VARIABLE);
        v.set_id(self.new_id());
        self.add_to_function_definitions(v);
    }

    pub fn add_global_variable(&mut self, v: &Inst) {
        debug_assert!(v.opcode == Op::VARIABLE);
        v.set_id(self.new_id());
        self.globals.push(v.clone());
    }

    fn add_constant(&mut self, c: &Inst) {
        if let Some(g) = self.constants.find(c, constant_key_eq) {
            c.set_id(g.id());
            return;
        }
        c.set_id(self.new_id());
        self.constants.insert(c.clone(), constant_key_eq);
        self.constants_list.push(c.clone());
    }

    pub fn ext_inst(&mut self, result_type: &Inst, set: &Inst, instruction: i32, parameters: &[Operand]) -> Inst {
        let id = self.new_id();
        let r = self.new_instruction(Op::EXT_INST, id, Some(result_type));
        r.add_operand(set);
        r.add_operand(Lit::i32(instruction));
        r.add_operands(parameters);
        self.add_to_function_definitions(&r);
        r
    }

    pub fn set_memory_model(&mut self, addressing: AddressingModel, memory: MemoryModel) {
        self.addressing_model = addressing;
        self.memory_model = memory;
    }

    pub fn generate(&self) -> Vec<u8> {
        let mut w = Vec::with_capacity(1024 + self.function_definitions.len() * 32);
        for v in [MAGIC_NUMBER, self.version, 0, self.bound, 0] {
            w.extend_from_slice(&v.to_le_bytes());
        }
        for c in &self.capabilities {
            let i = self.new_instruction(Op::CAPABILITY, INVALID_ID, None);
            i.add_operand(*c);
            i.write(&mut w);
        }
        for e in &self.extensions {
            let i = self.new_instruction(Op::EXTENSION, INVALID_ID, None);
            i.add_operand(e.as_str());
            i.write(&mut w);
        }
        for (_, i) in &self.ext_inst_imports {
            i.write(&mut w);
        }
        let mm = self.new_instruction(Op::MEMORY_MODEL, INVALID_ID, None);
        mm.add_operand(self.addressing_model);
        mm.add_operand(self.memory_model);
        mm.write(&mut w);
        for i in self.entry_points.iter().chain(&self.execution_modes).chain(&self.debug).chain(&self.annotations) {
            i.write(&mut w);
        }
        let mut declarations: Vec<&Inst> =
            self.type_declarations_list.iter().chain(&self.globals).chain(&self.constants_list).collect();
        declarations.sort_by_key(|i| i.id());
        for i in declarations {
            i.write(&mut w);
        }
        for i in self.function_declarations.iter().chain(&self.function_definitions) {
            i.write(&mut w);
        }
        w
    }
}
