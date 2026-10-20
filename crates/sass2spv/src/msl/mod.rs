// Metal Shading Language from the structured program. Each operation is written the way SPIRV-Cross 1.4.363 writes the
// SPIR-V the SPIR-V code generator (crate::codegen) makes for it, and the interface (names, bindings, struct shapes) is
// the one SPIRV-Cross gives that SPIR-V with `--msl-version 30000 --msl-decoration-binding`: the cache builder's later
// passes and the engine read it.

mod helpers;
mod ops;
mod tex;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;

use crate::codegen::HostCapabilities;
use crate::ir::{Instruction as I, IoVariable, OperandType, StorageKind};
use crate::structured::{AstBlockType, AstId, AstKind, IoDefinition, StructuredProgramInfo};
use crate::translation::definitions::{AggregateType as A, PixelImap, ShaderDefinitions};
use crate::translation::properties::{BufferDefinition, BufferLayout, ShaderProperties, TextureDefinition};
use crate::types::{SamplerType, SetBindingPair, ShaderStage};
use helpers::Helper;

pub type R<T> = Result<T, String>;

macro_rules! unsupported {
    ($($t:tt)*) => { return Err(format!("msl: unsupported {}", format!($($t)*))) };
}
pub(crate) use unsupported;

/// A resource or interface value a function body uses (helper functions get these as parameters).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Global {
    StageIn,
    StageOut,
    ConstantBuffer(i32),
    StorageBuffer(i32),
    Texture(SetBindingPair),
    Sampler(SetBindingPair),
    Image(SetBindingPair),
    LocalMemory(i32),
    SharedMemory(i32),
    Builtin(IoVariable),
}

pub struct Ctx<'a> {
    pub info: &'a StructuredProgramInfo,
    pub d: &'a ShaderDefinitions,
    pub props: &'a ShaderProperties,
    pub host: &'a HostCapabilities,
    pub stage: ShaderStage,
    pub entry: String,
    pub body: String,
    pub indent: usize,
    pub helpers: BTreeSet<Helper>,
    pub uses_atomics: bool,
    pub function_calls: bool,
    /// Any storage buffer write: SPIRV-Cross then declares all of them `device`, otherwise `const device`.
    pub buffer_writes: bool,
    pub struct_names: Vec<(String, String)>,
    pub buffer_structs: HashMap<(bool, i32), String>,
    pub image_access: HashMap<SetBindingPair, (bool, bool)>,
    pub func: usize,
    pub locals: HashMap<AstId, String>,
    pub current_block: AstId,
    pub may_have_returned: bool,
    pub uses: Vec<BTreeSet<Global>>,
    pub has_stage_out: bool,
    pub has_stage_in: bool,
    /// Statements an expression needs before the statement it is part of (SPIRV-Cross's temporaries).
    pub pre: Vec<String>,
    pub temps: usize,
    /// Where each local is declared: (block, child index) -> locals (SPIRV-Cross declares a variable in the innermost
    /// scope that holds all its uses, at its first use, with the value when that use assigns it).
    pub decl_at: HashMap<(AstId, usize), Vec<AstId>>,
    /// Where each local memory array of the entry point is declared, as decl_at.
    pub mem_at: HashMap<(AstId, usize), Vec<i32>>,
    /// Expression dependencies (SPIRV-Cross expression_dependencies): for each expression text, how many expression
    /// results it inlines. Absent: a constant or literal, not an expression.
    pub deps: HashMap<String, usize>,
}

pub fn type_name(t: A) -> R<String> {
    let base = match t.element() {
        A::BOOL => "bool",
        A::FP32 | A::FP64 => "float",
        A::S32 => "int",
        A::U32 => "uint",
        A::VOID => "void",
        e => unsupported!("type {}", e.0),
    };
    Ok(match t.count() {
        A::VECTOR2 => format!("{base}2"),
        A::VECTOR3 => format!("{base}3"),
        A::VECTOR4 => format!("{base}4"),
        _ => base.to_string(),
    })
}

/// A float constant as SPIRV-Cross prints it (the value exactly, as a decimal of the widened double).
pub fn float_literal(bits: u32) -> String {
    let v = f32::from_bits(bits);
    if v.is_nan() || v.is_infinite() {
        let comment = if v.is_nan() { "nan" } else if v < 0.0 { "-inf" } else { "inf" };
        return format!("as_type<float>(0x{bits:x}u /* {comment} */)");
    }
    let mut s = format_g32(v as f64);
    if !s.contains('.') && !s.contains('e') {
        s.push_str(".0");
    }
    s
}

/// C's `%.32g`.
fn format_g32(v: f64) -> String {
    if v == 0.0 {
        return if v.is_sign_negative() { "-0".into() } else { "0".into() };
    }
    let sci = format!("{:.31e}", v); // d.ddd...e[-]x, 32 significant digits
    let (mant, exp) = sci.split_once('e').unwrap();
    let exp: i32 = exp.parse().unwrap();
    let trim = |m: &str| -> String {
        if m.contains('.') {
            m.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            m.to_string()
        }
    };
    if exp < -4 || exp >= 32 {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mant), exp.abs())
    } else {
        let digits = (31 - exp).max(0) as usize;
        trim(&format!("{:.*}", digits, v))
    }
}

pub fn int_literal(v: i32) -> String {
    if v == i32::MIN {
        "int(0x80000000)".into()
    } else if v < 0 {
        format!("({v})")
    } else {
        v.to_string()
    }
}

/// Whether `s` must be put in parentheses to be an operand: anything with a top-level space (an infix expression or a
/// conditional) or starting with a unary operator.
pub fn needs_enclose(s: &str) -> bool {
    if s.starts_with('-') || s.starts_with('!') || s.starts_with('~') {
        return !is_wrapped(s);
    }
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ' ' if depth == 0 => return true,
            _ => {}
        }
    }
    false
}

fn is_wrapped(s: &str) -> bool {
    // "(...)" as a whole.
    if !s.starts_with('(') || !s.ends_with(')') {
        return false;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 && i != s.len() - 1 {
                    return false;
                }
            }
            _ => {}
        }
    }
    true
}

pub fn enclose(s: String) -> String {
    if needs_enclose(&s) {
        format!("({s})")
    } else {
        s
    }
}

pub fn component(i: i32) -> R<&'static str> {
    Ok(match i {
        0 => "x",
        1 => "y",
        2 => "z",
        3 => "w",
        _ => unsupported!("component index {i}"),
    })
}

impl<'a> Ctx<'a> {
    pub fn line(&mut self, s: &str) {
        for _ in 0..self.indent {
            self.body.push_str("    ");
        }
        self.body.push_str(s);
        self.body.push('\n');
    }

    fn begin(&mut self) {
        self.line("{");
        self.indent += 1;
    }

    fn end(&mut self) {
        self.indent -= 1;
        self.line("}");
    }

    /// Writes the pending pre-statements.
    fn flush_pre(&mut self) {
        for l in std::mem::take(&mut self.pre) {
            self.line(&l);
        }
    }

    /// One SPIR-V result computed from `operands`: SPIRV-Cross stops forwarding when an operand carries 64 or more
    /// dependencies (should_forward), so the result becomes a temporary declared before the statement.
    pub fn inst(&mut self, ty: A, text: String, operands: &[&str]) -> R<String> {
        let ds: Vec<usize> = operands.iter().filter_map(|o| self.deps.get(*o).copied()).collect();
        self.inst_d(ty, text, &ds)
    }

    /// Ctx::inst with the operands' dependency counts (only operands that are expressions).
    pub fn inst_d(&mut self, ty: A, text: String, operand_deps: &[usize]) -> R<String> {
        if operand_deps.iter().any(|&od| od >= 64) {
            let t = self.temp();
            let tn = type_name(ty)?;
            self.pre.push(format!("{tn} {t} = {text};"));
            // A temporary is read like a variable: it starts a new expression tree.
            self.deps.insert(t.clone(), 0);
            return Ok(t);
        }
        let d: usize = operand_deps.iter().map(|od| 1 + od).sum();
        self.deps.insert(text.clone(), d);
        Ok(text)
    }

    /// A load of a variable (local, argument): an expression without dependencies.
    pub fn leaf(&mut self, text: String) -> String {
        self.deps.insert(text.clone(), 0);
        text
    }

    pub fn temp(&mut self) -> String {
        self.temps += 1;
        format!("_tmp{}", self.temps)
    }

    pub fn helper(&mut self, h: Helper) {
        self.helpers.insert(h);
    }

    /// NoContraction on floating-point results (codegen::instructions::no_contraction).
    pub fn precise(&self, force_precise: bool) -> bool {
        !self.host.reduced_precision || force_precise
    }

    pub fn single_lane_subgroups(&self) -> bool {
        crate::codegen::single_lane_subgroups(self.stage)
    }

    pub fn stage_out_name(&self) -> String {
        format!("{}_out", self.entry)
    }

    fn uses_global(&mut self, g: Global) {
        self.uses[self.func].insert(g);
    }

    pub fn buffer(&self, sk: StorageKind, binding: i32) -> R<&'a BufferDefinition> {
        let props = self.props;
        let b = if sk == StorageKind::ConstantBuffer { props.constant_buffers.get(&binding) } else { props.storage_buffers.get(&binding) };
        b.ok_or_else(|| format!("msl: no buffer {sk:?} {binding}"))
    }

    pub fn texture(&self, key: SetBindingPair) -> R<&'a TextureDefinition> {
        self.props.textures.get(&key).ok_or_else(|| format!("msl: no texture {key:?}"))
    }

    pub fn image(&self, key: SetBindingPair) -> R<&'a TextureDefinition> {
        self.props.images.get(&key).ok_or_else(|| format!("msl: no image {key:?}"))
    }

    pub fn note_texture(&mut self, key: SetBindingPair, separate_sampler: Option<SetBindingPair>) {
        self.uses_global(Global::Texture(key));
        self.uses_global(Global::Sampler(separate_sampler.unwrap_or(key)));
    }

    pub fn note_image(&mut self, key: SetBindingPair) {
        self.uses_global(Global::Image(key));
    }

    pub fn note_buffer(&mut self, sk: StorageKind, binding: i32) {
        self.uses_global(if sk == StorageKind::ConstantBuffer { Global::ConstantBuffer(binding) } else { Global::StorageBuffer(binding) });
    }

    pub fn note(&mut self, g: Global) {
        self.uses_global(g);
    }

    /// The value of `node` as type `ty`.
    pub fn expr(&mut self, ty: A, node: Option<AstId>) -> R<String> {
        let node = node.ok_or("msl: missing operand")?;
        let info = self.info;
        match info.ast.kind(node) {
            AstKind::Operation(op) => {
                let atomic = op.inst.is_atomic();
                let (t, e) = ops::operation(self, node)?;
                let mut e = e.ok_or("msl: operation without a value")?;
                if atomic {
                    // SPIRV-Cross never forwards an atomic's result.
                    let tmp = self.temp();
                    self.pre.push(format!("{} {tmp} = {e};", type_name(t)?));
                    e = self.leaf(tmp);
                }
                self.cast(ty, t, e)
            }
            AstKind::Operand(o) => match o.ty {
                OperandType::Constant => self.constant(ty, o.value),
                OperandType::LocalVariable => {
                    let n = self.locals.get(&node).cloned().ok_or("msl: unknown local")?;
                    let n = self.leaf(n);
                    self.cast(ty, o.var_type, n)
                }
                OperandType::Argument => {
                    let t = self.info.functions[self.func].argument_type(o.value as usize);
                    let n = self.leaf(format!("a{}", o.value));
                    self.cast(ty, t, n)
                }
                OperandType::Undefined => self.undefined(ty),
                t => unsupported!("operand type {t:?}"),
            },
            _ => unsupported!("node kind"),
        }
    }

    /// The value with its own type (codegen get_with_type).
    pub fn expr_typed(&mut self, node: Option<AstId>) -> R<(A, String)> {
        let node = node.ok_or("msl: missing operand")?;
        let info = self.info;
        match info.ast.kind(node) {
            AstKind::Operation(_) => {
                let (t, e) = ops::operation(self, node)?;
                Ok((t, e.ok_or("msl: operation without a value")?))
            }
            AstKind::Operand(o) if o.ty == OperandType::LocalVariable => {
                let n = self.locals.get(&node).cloned().ok_or("msl: unknown local")?;
                Ok((o.var_type, self.leaf(n)))
            }
            _ => unsupported!("typed operand"),
        }
    }

    pub fn constant(&mut self, ty: A, value: i32) -> R<String> {
        Ok(match ty {
            A::BOOL => if value != 0 { "true" } else { "false" }.into(),
            A::FP32 | A::FP64 => float_literal(value as u32),
            A::S32 => int_literal(value),
            A::U32 => format!("{}u", value as u32),
            t => unsupported!("constant type {}", t.0),
        })
    }

    fn undefined(&mut self, ty: A) -> R<String> {
        Ok(match ty {
            A::BOOL => "false".into(),
            A::FP32 | A::FP64 => "0.0".into(),
            A::S32 => "0".into(),
            A::U32 => "0u".into(),
            t => unsupported!("undefined of type {}", t.0),
        })
    }

    /// bitcast_if_needed.
    pub fn cast(&mut self, dst: A, src: A, value: String) -> R<String> {
        if dst == src {
            return Ok(value);
        }
        if dst == A::BOOL {
            let v = self.cast(A::S32, src, value)?;
            return self.inst(A::BOOL, format!("{} != 0", enclose(v.clone())), &[&v]);
        }
        if src == A::BOOL {
            let sel = self.inst(A::S32, format!("{} ? (-1) : 0", enclose(value.clone())), &[&value])?;
            return self.cast(dst, A::S32, sel);
        }
        let t = type_name(dst)?;
        // Between int and uint SPIRV-Cross writes a conversion (the same bits).
        if matches!((dst, src), (A::S32, A::U32) | (A::U32, A::S32)) {
            return self.inst(dst, format!("{t}({value})"), &[&value]);
        }
        self.inst(dst, format!("as_type<{t}>({value})"), &[&value])
    }
}

/// MSL for a structured program; `entry` names the entry point.
pub fn generate(
    info: &mut StructuredProgramInfo,
    d: &ShaderDefinitions,
    props: &ShaderProperties,
    host: &HostCapabilities,
    entry: &str,
) -> R<String> {
    let stage = d.stage;
    if !matches!(stage, ShaderStage::Vertex | ShaderStage::Fragment | ShaderStage::Compute) {
        unsupported!("stage {stage}");
    }
    crate::codegen::adjust_io_definitions(info, stage);
    let info: &StructuredProgramInfo = info;
    let nfuncs = info.functions.len();
    let mut c = Ctx {
        info,
        d,
        props,
        host,
        stage,
        entry: entry.to_string(),
        body: String::new(),
        indent: 0,
        helpers: BTreeSet::new(),
        uses_atomics: false,
        function_calls: false,
        buffer_writes: false,
        struct_names: Vec::new(),
        buffer_structs: HashMap::new(),
        image_access: HashMap::new(),
        func: 0,
        locals: HashMap::new(),
        current_block: info.functions.first().map_or(AstId(0), |f| f.main_block),
        may_have_returned: false,
        uses: vec![BTreeSet::new(); nfuncs],
        has_stage_out: false,
        has_stage_in: false,
        pre: Vec::new(),
        temps: 0,
        decl_at: HashMap::new(),
        mem_at: HashMap::new(),
        deps: HashMap::new(),
    };
    scan(&mut c)?;
    let io = io_declarations(&mut c)?;

    // Function bodies: callees first (SPIRV-Cross emits a function's callees before it), the entry point last.
    let order = call_order(info)?;
    let mut functions = String::new();
    for &f in &order {
        c.body.clear();
        c.indent = 0;
        emit_function(&mut c, f, &io)?;
        functions.push_str(&c.body);
    }

    // Structs for buffers (declared on first use by the signatures above).
    let mut out = String::new();
    let mut pragmas: Vec<&str> = Vec::new();
    if c.uses_atomics {
        pragmas.push("#pragma clang diagnostic ignored \"-Wunused-variable\"");
    }
    if !c.helpers.is_empty() || c.function_calls {
        pragmas.push("#pragma clang diagnostic ignored \"-Wmissing-prototypes\"");
    }
    if c.helpers.contains(&Helper::UnsafeArray) {
        pragmas.push("#pragma clang diagnostic ignored \"-Wmissing-braces\"");
    }
    for p in &pragmas {
        out.push_str(p);
        out.push('\n');
    }
    if !pragmas.is_empty() {
        out.push('\n');
    }
    out.push_str("#include <metal_stdlib>\n#include <simd/simd.h>\n");
    if c.uses_atomics {
        out.push_str("#include <metal_atomic>\n");
    }
    out.push_str("\nusing namespace metal;\n\n");
    if c.helpers.contains(&Helper::UnsafeArray) {
        out.push_str(helpers::UNSAFE_ARRAY);
    }
    for &h in &c.helpers {
        out.push_str(&helpers::text(h));
    }
    for (name, body) in &c.struct_names {
        let _ = write!(out, "struct {name}\n{{\n{body}}};\n\n");
    }
    out.push_str(&io.structs);
    out.push_str(&functions);
    Ok(out)
}

/// Functions in emission order: every function after the functions it calls, the entry point (0) last.
fn call_order(info: &StructuredProgramInfo) -> R<Vec<usize>> {
    let n = info.functions.len();
    let mut calls: Vec<BTreeSet<usize>> = vec![BTreeSet::new(); n];
    for (i, f) in info.functions.iter().enumerate() {
        for node in nodes_of(info, f.main_block) {
            if let Some(o) = info.ast.as_operation(node) {
                if o.inst == I::CALL {
                    let id = info.ast.opnd(o.sources[0].ok_or("msl: call without target")?).value as usize;
                    calls[i].insert(id);
                }
            }
        }
    }
    let mut order = Vec::new();
    let mut state = vec![0u8; n];
    fn visit(f: usize, calls: &[BTreeSet<usize>], state: &mut [u8], order: &mut Vec<usize>) -> R<()> {
        match state[f] {
            2 => return Ok(()),
            1 => unsupported!("recursive functions"),
            _ => {}
        }
        state[f] = 1;
        for &g in &calls[f] {
            visit(g, calls, state, order)?;
        }
        state[f] = 2;
        order.push(f);
        Ok(())
    }
    visit(0, &calls, &mut state, &mut order)?;
    Ok(order)
}

/// Every node under `block` (operations and their source trees, conditions), in no particular order.
pub(super) fn nodes_of(info: &StructuredProgramInfo, block: AstId) -> Vec<AstId> {
    let ast = &info.ast;
    let mut out = Vec::new();
    let mut stack = vec![block];
    while let Some(n) = stack.pop() {
        out.push(n);
        match ast.kind(n) {
            AstKind::Block(b) => {
                if let Some(c) = b.condition {
                    stack.push(c);
                }
                stack.extend(ast.children(n));
            }
            AstKind::Operation(o) => stack.extend(o.sources.iter().flatten().copied()),
            AstKind::Assignment { destination, source } => {
                stack.push(*destination);
                if let Some(s) = source {
                    stack.push(*s);
                }
            }
            _ => {}
        }
    }
    out
}

/// What the program uses, before anything is written: storage writes (buffer address spaces), image access, and the
/// globals of every function (helper function parameters).
fn scan(c: &mut Ctx) -> R<()> {
    let info = c.info;
    for (fi, f) in info.functions.iter().enumerate() {
        for node in nodes_of(info, f.main_block) {
            let Some(o) = info.ast.as_operation(node) else { continue };
            let base = o.inst.base();
            if (base == I::STORE || o.inst.is_atomic()) && o.storage_kind == StorageKind::StorageBuffer {
                c.buffer_writes = true;
            }
            if o.inst.is_atomic() || base == I::IMAGE_ATOMIC {
                c.uses_atomics = true;
            }
            if base == I::CALL {
                c.function_calls = true;
            }
            if let Some(t) = o.tex {
                if matches!(base, I::IMAGE_LOAD | I::IMAGE_STORE | I::IMAGE_ATOMIC) {
                    let e = c.image_access.entry(t.texture_set_and_binding()).or_default();
                    if base != I::IMAGE_STORE {
                        e.0 = true;
                    }
                    if base != I::IMAGE_LOAD {
                        e.1 = true;
                    }
                }
            }
            let _ = fi;
        }
    }
    // Globals per function: a dry run of the bodies collects them (Ctx::note*), then helpers inherit their callees'.
    let saved = std::mem::take(&mut c.body);
    for fi in 0..info.functions.len() {
        c.func = fi;
        start_function(c, fi)?;
        let mb = info.functions[fi].main_block;
        emit_block_contents(c, mb)?;
    }
    c.body = saved;
    c.pre.clear();
    c.temps = 0;
    let order = call_order(info)?;
    for &f in &order {
        let mut add = BTreeSet::new();
        for node in nodes_of(info, info.functions[f].main_block) {
            if let Some(o) = info.ast.as_operation(node) {
                if o.inst == I::CALL {
                    let id = info.ast.opnd(o.sources[0].ok_or("msl: call without target")?).value as usize;
                    add.extend(c.uses[id].iter().copied());
                }
            }
        }
        c.uses[f].extend(add);
    }
    c.helpers.clear();
    Ok(())
}

/// Declarations shared by the entry point and helper functions.
pub struct IoDecls {
    pub structs: String,
    /// Entry parameters in order: (global, declaration with attribute, declaration as a helper parameter).
    pub params: Vec<(Global, String, String)>,
    /// Statements at the top of the entry point (output struct, local and shared memory).
    pub prologue: Vec<String>,
    pub ret: String,
}

fn struct_for_buffer(c: &mut Ctx, b: &BufferDefinition, is_storage: bool) -> R<String> {
    if let Some(n) = c.buffer_structs.get(&(is_storage, b.binding)) {
        return Ok(n.clone());
    }
    // Members at their SPIR-V offsets (std140: 16-byte aligned fields and array strides).
    let std140 = b.layout == BufferLayout::Std140;
    let align = if std140 { 16 } else { 4 };
    let mut body = String::new();
    let mut offset = 0i32; // SPIR-V offset of the next field
    let mut msl_offset = 0i32; // where MSL puts it
    let mut pad = 0;
    for (i, f) in b.ty.fields.iter().enumerate() {
        let elem = f.ty & !A::ARRAY;
        let is_array = f.ty.has(A::ARRAY);
        let field_size = (f.ty.size_in_bytes() + align - 1) & !(align - 1);
        // An std140 scalar array element is 16 bytes: a 4-component vector read through .x.
        let mty = if is_array && std140 && elem.count() == A::SCALAR { elem | A::VECTOR4 } else { elem };
        let msize = mty.size_in_bytes();
        let malign = match mty.count() {
            A::SCALAR => 4,
            A::VECTOR2 => 8,
            _ => 16,
        };
        let natural = (msl_offset + malign - 1) & !(malign - 1);
        if natural < offset {
            let _ = writeln!(body, "    char _pad{pad}[{}];", offset - natural);
            pad += 1;
        } else if natural > offset {
            unsupported!("buffer layout of {}", b.name);
        }
        let tn = type_name(mty)?;
        if is_array {
            let len = if f.array_length == 0 { 1 } else { f.array_length };
            if is_array && std140 && elem.count() == A::SCALAR && msize != 16 {
                unsupported!("buffer element size");
            }
            let _ = writeln!(body, "    {tn} _m{i}[{len}];");
            offset += field_size * f.array_length;
            msl_offset = offset.max(natural + msize * len);
        } else {
            let _ = writeln!(body, "    {tn} _m{i};");
            offset += field_size;
            msl_offset = natural + msize;
        }
    }
    let name = match c.struct_names.iter().find(|(_, b)| *b == body) {
        Some((n, _)) => n.clone(),
        None => {
            let n = format!("_{}", c.struct_names.len() + 1);
            c.struct_names.push((n.clone(), body));
            n
        }
    };
    c.buffer_structs.insert((is_storage, b.binding), name.clone());
    Ok(name)
}

/// Whether an std140 scalar array element of field `field` is read through `.x` (struct_for_buffer).
pub fn scalar_array_in_vec4(b: &BufferDefinition, field: usize) -> bool {
    let f = &b.ty.fields[field];
    b.layout == BufferLayout::Std140 && f.ty.has(A::ARRAY) && (f.ty & !A::ARRAY).count() == A::SCALAR
}

pub fn texture_type(ty: SamplerType, component: &str, access: Option<&str>) -> R<String> {
    let base = match ty.base() {
        SamplerType::TEXTURE_1D => {
            if ty.has(SamplerType::ARRAY) {
                "texture1d_array"
            } else {
                "texture1d"
            }
        }
        SamplerType::TEXTURE_2D => match (ty.has(SamplerType::ARRAY), ty.has(SamplerType::MULTISAMPLE)) {
            (false, false) => "texture2d",
            (true, false) => "texture2d_array",
            (false, true) => "texture2d_ms",
            (true, true) => "texture2d_ms_array",
        },
        SamplerType::TEXTURE_3D => "texture3d",
        SamplerType::TEXTURE_CUBE => {
            if ty.has(SamplerType::ARRAY) {
                "texturecube_array"
            } else {
                "texturecube"
            }
        }
        t => unsupported!("texture type {}", t.0),
    };
    Ok(match access {
        Some(a) => format!("{base}<{component}, access::{a}>"),
        None => format!("{base}<{component}>"),
    })
}

fn builtin_param(v: IoVariable) -> R<(&'static str, &'static str, &'static str)> {
    use IoVariable as V;
    Ok(match v {
        V::FragmentCoord => ("float4", "gl_FragCoord", "position"),
        V::FrontFacing => ("bool", "gl_FrontFacing", "front_facing"),
        V::PointCoord => ("float2", "gl_PointCoord", "point_coord"),
        V::VertexIndex => ("uint", "gl_VertexIndex", "vertex_id"),
        V::InstanceIndex => ("uint", "gl_InstanceIndex", "instance_id"),
        V::BaseInstance => ("uint", "gl_BaseInstance", "base_instance"),
        V::BaseVertex => ("uint", "gl_BaseVertex", "base_vertex"),
        V::ThreadId => ("uint3", "gl_LocalInvocationID", "thread_position_in_threadgroup"),
        V::CtaId => ("uint3", "gl_WorkGroupID", "threadgroup_position_in_grid"),
        V::GlobalId => ("uint3", "gl_GlobalInvocationID", "thread_position_in_grid"),
        V::SubgroupLaneId => ("uint", "gl_SubgroupInvocationID", "thread_index_in_simdgroup"),
        v => unsupported!("input builtin {v:?}"),
    })
}

/// The MSL name of an input builtin and its MSL type.
pub fn builtin_input(c: &mut Ctx, v: IoVariable) -> R<(String, A)> {
    use IoVariable as V;
    c.note(Global::Builtin(v));
    Ok(match v {
        V::FragmentCoord => ("gl_FragCoord".into(), A::VECTOR4 | A::FP32),
        V::FrontFacing => ("gl_FrontFacing".into(), A::BOOL),
        V::PointCoord => ("gl_PointCoord".into(), A::VECTOR2 | A::FP32),
        V::VertexIndex => ("gl_VertexIndex".into(), A::U32),
        V::InstanceIndex => ("gl_InstanceIndex".into(), A::U32),
        V::BaseInstance => ("gl_BaseInstance".into(), A::U32),
        V::BaseVertex => ("gl_BaseVertex".into(), A::U32),
        V::ThreadId => ("gl_LocalInvocationID".into(), A::VECTOR3 | A::U32),
        V::CtaId => ("gl_WorkGroupID".into(), A::VECTOR3 | A::U32),
        V::GlobalId => ("gl_GlobalInvocationID".into(), A::VECTOR3 | A::U32),
        V::SubgroupLaneId => ("gl_SubgroupInvocationID".into(), A::U32),
        V::SubgroupLtMask => ("gl_SubgroupLtMask".into(), A::VECTOR4 | A::U32),
        V::ThreadKill => ("gl_HelperInvocation".into(), A::BOOL),
        v => unsupported!("input builtin {v:?}"),
    })
}

fn io_declarations(c: &mut Ctx) -> R<IoDecls> {
    let info = c.info;
    let d = c.d;
    let stage = c.stage;
    let mut structs = String::new();
    let mut params: Vec<(Global, String, String)> = Vec::new();
    let mut prologue = Vec::new();
    let main_uses = c.uses[0].clone();

    // Stage inputs (vertex attributes, fragment varyings).
    let mut in_members: BTreeMap<i32, String> = BTreeMap::new();
    let mut out_members: BTreeMap<i32, String> = BTreeMap::new();
    let mut out_builtins: BTreeMap<u32, String> = BTreeMap::new();
    let mut input_builtins: Vec<IoVariable> = Vec::new();
    for io in info.io_definitions.iter() {
        let is_output = io.storage_kind.is_output();
        if io.storage_kind.is_per_patch() {
            unsupported!("per-patch I/O");
        }
        match io.io_variable {
            IoVariable::UserDefined => {
                let t = d.user_defined_type(io.location, is_output);
                if t.has(A::ARRAY) {
                    unsupported!("indexed user attributes");
                }
                let tn = type_name(t)?;
                if stage == ShaderStage::Vertex && !is_output {
                    in_members.insert(io.location, format!("    {tn} in_attr{0} [[attribute({0})]];\n", io.location));
                } else if is_output {
                    out_members.insert(io.location, format!("    {tn} loc{0} [[user(locn{0})]];\n", io.location));
                } else {
                    let interp = match d.imap_types.as_ref().ok_or("msl: no pixel imap")?[io.location as usize].first_used_type() {
                        PixelImap::Constant => ", flat",
                        PixelImap::ScreenLinear => ", center_no_perspective",
                        _ => "",
                    };
                    in_members.insert(io.location, format!("    {tn} loc{0} [[user(locn{0}){interp}]];\n", io.location));
                }
            }
            IoVariable::FragmentOutputColor => {
                if d.dual_source_blend() {
                    unsupported!("dual-source blending");
                }
                let tn = type_name(d.fragment_output_color_type(io.location))?;
                out_members.insert(io.location, format!("    {tn} color{0} [[color({0})]];\n", io.location));
            }
            IoVariable::Position if is_output => {
                out_builtins.insert(0, "    float4 gl_Position [[position]];\n".into());
            }
            IoVariable::PointSize if is_output => {
                out_builtins.insert(1, "    float gl_PointSize [[point_size]];\n".into());
            }
            IoVariable::FragmentOutputDepth => {
                out_builtins.insert(22, "    float gl_FragDepth [[depth(any)]];\n".into());
            }
            IoVariable::Layer if is_output => {
                out_builtins.insert(9, "    uint gl_Layer [[render_target_array_index]];\n".into());
            }
            IoVariable::ViewportIndex if is_output => {
                out_builtins.insert(10, "    uint gl_ViewportIndex [[viewport_array_index]];\n".into());
            }
            v if !is_output => {
                if !matches!(v, IoVariable::SubgroupLtMask | IoVariable::ThreadKill) {
                    builtin_param(v)?;
                }
                input_builtins.push(v);
            }
            v => unsupported!("output {v:?}"),
        }
    }
    c.has_stage_in = !in_members.is_empty();
    c.has_stage_out = stage != ShaderStage::Compute && (!out_members.is_empty() || !out_builtins.is_empty());
    let e = c.entry.clone();
    if c.has_stage_out {
        let _ = write!(structs, "struct {e}_out\n{{\n");
        for m in out_members.values() {
            structs.push_str(m);
        }
        for m in out_builtins.values() {
            structs.push_str(m);
        }
        structs.push_str("};\n\n");
    }
    if c.has_stage_in {
        let _ = write!(structs, "struct {e}_in\n{{\n");
        for m in in_members.values() {
            structs.push_str(m);
        }
        structs.push_str("};\n\n");
        params.push((Global::StageIn, format!("{e}_in in [[stage_in]]"), format!("thread {e}_in& in")));
    }

    // Buffers: Metal buffer index = binding.
    let mut buffers: Vec<(i32, Global, String, String)> = Vec::new();
    for b in c.props.constant_buffers.values() {
        let g = Global::ConstantBuffer(b.binding);
        if !main_uses.contains(&g) {
            continue;
        }
        let s = struct_for_buffer(c, b, false)?;
        buffers.push((b.binding, g, format!("constant {s}& {} [[buffer({})]]", b.name, b.binding), format!("constant {s}& {}", b.name)));
    }
    let space = if c.buffer_writes { "device" } else { "const device" };
    for b in c.props.storage_buffers.values() {
        let g = Global::StorageBuffer(b.binding);
        if !main_uses.contains(&g) {
            continue;
        }
        let s = struct_for_buffer(c, b, true)?;
        buffers.push((b.binding, g, format!("{space} {s}& {} [[buffer({})]]", b.name, b.binding), format!("{space} {s}& {}", b.name)));
    }
    buffers.sort_by_key(|b| b.0);
    for (_, g, a, h) in buffers {
        params.push((g, a, h));
    }

    // Textures, images, then samplers (texture and sampler indices = bindings).
    let mut textures: Vec<(i32, Global, String, String)> = Vec::new();
    let mut samplers: Vec<(i32, Global, String, String)> = Vec::new();
    for t in c.props.textures.values() {
        let key = SetBindingPair::new(t.set, t.binding);
        if t.array_length != 1 {
            if main_uses.contains(&Global::Texture(key)) || main_uses.contains(&Global::Sampler(key)) {
                unsupported!("texture arrays");
            }
            continue;
        }
        if t.ty != SamplerType::NONE && main_uses.contains(&Global::Texture(key)) {
            if t.ty.base() == SamplerType::TEXTURE_BUFFER {
                unsupported!("texture buffers");
            }
            let tt = texture_type(t.ty, "float", None)?;
            textures.push((t.binding, Global::Texture(key), format!("{tt} {} [[texture({})]]", t.name, t.binding), format!("{tt} {}", t.name)));
        }
        if main_uses.contains(&Global::Sampler(key)) {
            let n = if t.separate { t.name.clone() } else { format!("{}Smplr", t.name) };
            samplers.push((t.binding, Global::Sampler(key), format!("sampler {n} [[sampler({})]]", t.binding), format!("sampler {n}")));
        }
    }
    for t in c.props.images.values() {
        let key = SetBindingPair::new(t.set, t.binding);
        if !main_uses.contains(&Global::Image(key)) {
            continue;
        }
        if t.array_length != 1 {
            unsupported!("image arrays");
        }
        if t.ty.base() == SamplerType::TEXTURE_BUFFER {
            unsupported!("image buffers");
        }
        let (r, w) = c.image_access.get(&key).copied().unwrap_or((true, true));
        let access = match (r, w) {
            (true, true) => "read_write",
            (false, true) => "write",
            _ => "read",
        };
        if access == "read_write" {
            unsupported!("read-write images");
        }
        let ct = type_name(t.format.component_type())?;
        // Read-only images keep the default (sample) access, which allows read() (SPIRV-Cross does the same).
        let tt = texture_type(t.ty, &ct, (access != "read").then_some(access))?;
        textures.push((t.binding, Global::Image(key), format!("{tt} {} [[texture({})]]", t.name, t.binding), format!("{tt} {}", t.name)));
    }
    textures.sort_by_key(|t| t.0);
    samplers.sort_by_key(|t| t.0);
    for (_, g, a, h) in textures.into_iter().chain(samplers) {
        params.push((g, a, h));
    }

    // Input builtins.
    // In declaration order (SPIRV-Cross lists them by variable id).
    let builtins: Vec<IoVariable> = input_builtins.iter().copied().filter(|v| main_uses.contains(&Global::Builtin(*v))).collect();
    for v in builtins {
        match v {
            IoVariable::SubgroupLtMask | IoVariable::ThreadKill => {}
            _ => {
                let (t, n, a) = builtin_param(v)?;
                params.push((Global::Builtin(v), format!("{t} {n} [[{a}]]"), format!("thread {t}& {n}")));
            }
        }
    }

    if c.has_stage_out {
        prologue.push(format!("{e}_out out = {{}};"));
    }
    // Memories: local in each invocation, shared per threadgroup.
    // Local memory is declared at its first use (place_memories).
    if c.props.local_memories.keys().any(|id| main_uses.contains(&Global::LocalMemory(*id))) {
        c.helper(Helper::UnsafeArray);
    }
    for (&id, m) in c.props.shared_memories.iter() {
        if main_uses.contains(&Global::SharedMemory(id)) {
            c.helper(Helper::UnsafeArray);
            prologue.push(format!("threadgroup spvUnsafeArray<{}, {}> smem{id};", type_name(m.ty.element())?, m.array_length));
        }
    }
    let ret = match stage {
        ShaderStage::Compute => "void".to_string(),
        _ if c.has_stage_out => format!("{e}_out"),
        _ => "void".to_string(),
    };
    Ok(IoDecls { structs, params, prologue, ret })
}

/// The helper-function parameter for a global that has no entry parameter (stage in/out, memories, computed builtins).
fn helper_param(c: &Ctx, g: Global, io: &IoDecls) -> R<String> {
    if let Some((_, _, h)) = io.params.iter().find(|p| p.0 == g) {
        return Ok(h.clone());
    }
    let e = &c.entry;
    Ok(match g {
        Global::StageOut => format!("thread {e}_out& out"),
        Global::LocalMemory(id) => {
            let m = c.props.local_memories.get(&id).ok_or("msl: local memory")?;
            format!("thread spvUnsafeArray<{}, {}>& lmem{id}", type_name(m.ty.element())?, m.array_length)
        }
        Global::SharedMemory(id) => {
            let m = c.props.shared_memories.get(&id).ok_or("msl: shared memory")?;
            format!("threadgroup spvUnsafeArray<{}, {}>& smem{id}", type_name(m.ty.element())?, m.array_length)
        }
        Global::Builtin(IoVariable::SubgroupLtMask) => "thread uint4& gl_SubgroupLtMask".into(),
        Global::Builtin(IoVariable::ThreadKill) => "thread bool& gl_HelperInvocation".into(),
        g => unsupported!("helper parameter {g:?}"),
    })
}

/// The argument passed for a global in a call (its name).
pub fn global_arg(c: &Ctx, g: Global) -> R<String> {
    Ok(match g {
        Global::StageIn => "in".into(),
        Global::StageOut => "out".into(),
        Global::ConstantBuffer(b) => c.buffer(StorageKind::ConstantBuffer, b)?.name.clone(),
        Global::StorageBuffer(b) => c.buffer(StorageKind::StorageBuffer, b)?.name.clone(),
        Global::Texture(k) => c.texture(k)?.name.clone(),
        Global::Sampler(k) => {
            let t = c.texture(k)?;
            if t.separate { t.name.clone() } else { format!("{}Smplr", t.name) }
        }
        Global::Image(k) => c.image(k)?.name.clone(),
        Global::LocalMemory(id) => format!("lmem{id}"),
        Global::SharedMemory(id) => format!("smem{id}"),
        Global::Builtin(v) => match v {
            IoVariable::SubgroupLtMask => "gl_SubgroupLtMask".into(),
            IoVariable::ThreadKill => "gl_HelperInvocation".into(),
            v => builtin_param(v)?.1.into(),
        },
    })
}

fn start_function(c: &mut Ctx, f: usize) -> R<()> {
    c.func = f;
    c.locals.clear();
    c.may_have_returned = false;
    let info = c.info;
    for (i, &l) in info.functions[f].locals.iter().enumerate() {
        c.locals.insert(l, format!("t{i}"));
    }
    c.current_block = info.functions[f].main_block;
    c.decl_at.clear();
    c.deps.clear();
    let all: Vec<AstId> = info.functions[f].locals.iter().copied().collect();
    place_declarations(c, info.functions[f].main_block, &all);
    c.mem_at.clear();
    if f == 0 {
        let ids: Vec<i32> = c.props.local_memories.keys().copied().collect();
        place_memories(c, info.functions[f].main_block, &ids);
    }
    Ok(())
}

/// The locals referenced anywhere in `node` (its operations, assignments, nested blocks and their conditions).
fn local_refs(c: &Ctx, node: AstId) -> std::collections::HashSet<AstId> {
    nodes_of(c.info, node).into_iter().filter(|n| c.locals.contains_key(n)).collect()
}

/// The local memory arrays an AST subtree accesses.
fn memory_refs(c: &Ctx, node: AstId) -> std::collections::HashSet<i32> {
    let ast = &c.info.ast;
    nodes_of(c.info, node)
        .into_iter()
        .filter_map(|n| ast.as_operation(n))
        .filter(|o| o.storage_kind == StorageKind::LocalMemory)
        .filter_map(|o| o.sources.first().copied().flatten().and_then(|s| ast.as_operand(s)).map(|x| x.value))
        .collect()
}

/// Where the entry point declares its local memory arrays: SPIRV-Cross declares them in the first basic block,
/// at their first use there or at its end (before the first control flow).
fn place_memories(c: &mut Ctx, block: AstId, ids: &[i32]) {
    let ast = &c.info.ast;
    let children = ast.children(block);
    let first_block = children.iter().position(|&n| ast.is_block(n)).unwrap_or(children.len());
    for &id in ids {
        let first_use = children.iter().position(|&n| memory_refs(c, n).contains(&id));
        let Some(u) = first_use else { continue };
        c.mem_at.entry((block, u.min(first_block))).or_default().push(id);
    }
}

fn place_declarations(c: &mut Ctx, block: AstId, locals: &[AstId]) {
    let ast = &c.info.ast;
    let children = ast.children(block);
    let refs: Vec<std::collections::HashSet<AstId>> = children.iter().map(|&n| local_refs(c, n)).collect();
    let mut inner: HashMap<usize, Vec<AstId>> = HashMap::new();
    for &l in locals {
        let users: Vec<usize> = (0..children.len()).filter(|&i| refs[i].contains(&l)).collect();
        let Some(&first) = users.first() else { continue };
        let child = children[first];
        let in_condition = ast.as_block(child).and_then(|b| b.condition).is_some_and(|cnd| local_refs(c, cnd).contains(&l));
        if users.len() == 1 && ast.is_block(child) && !in_condition {
            inner.entry(first).or_default().push(l);
        } else {
            c.decl_at.entry((block, first)).or_default().push(l);
        }
    }
    let mut keys: Vec<usize> = inner.keys().copied().collect();
    keys.sort();
    for i in keys {
        let ls = inner.remove(&i).unwrap();
        place_declarations(c, children[i], &ls);
    }
}

fn emit_function(c: &mut Ctx, f: usize, io: &IoDecls) -> R<()> {
    start_function(c, f)?;
    let info = c.info;
    let func = &info.functions[f];
    let is_main = f == 0;
    if is_main {
        let kind = match c.stage {
            ShaderStage::Vertex => "vertex",
            ShaderStage::Fragment => "fragment",
            _ => "kernel",
        };
        let params: Vec<&str> = io.params.iter().map(|p| p.1.as_str()).collect();
        let sig = format!("{kind} {} {}({})", io.ret, c.entry, params.join(", "));
        c.line(&sig);
        c.begin();
        for p in &io.prologue {
            c.line(p);
        }
        if c.uses[0].contains(&Global::Builtin(IoVariable::ThreadKill)) {
            c.line("bool gl_HelperInvocation = {};");
            c.line("gl_HelperInvocation = simd_is_helper_thread();");
        }
        if c.uses[0].contains(&Global::Builtin(IoVariable::SubgroupLtMask)) {
            ops::subgroup_lt_mask_prologue(c)?;
        }
    } else {
        let ret = type_name(func.return_type)?;
        let mut params = Vec::new();
        // The function's own parameters, then the globals it uses (SPIRV-Cross appends those).
        for i in 0..func.in_arguments.len() + func.out_arguments.len() {
            params.push(format!("thread {}& a{i}", type_name(func.argument_type(i))?));
        }
        for g in c.uses[f].clone() {
            params.push(helper_param(c, g, io)?);
        }
        let sig = format!("static inline __attribute__((always_inline))\n{ret} {}({})", function_name(f), params.join(", "));
        c.line(&sig);
        c.begin();
    }
    let mb = func.main_block;
    emit_block_contents(c, mb)?;
    let last = info.ast.last(mb).and_then(|l| info.ast.as_operation(l)).map(|o| o.inst);
    // SPIRV-Cross gives the entry point's last block the return value, also after a discard.
    if is_main && last == Some(I::DISCARD) && c.has_stage_out {
        c.line("return out;");
    }
    let ends = last == Some(I::RETURN) || last == Some(I::DISCARD);
    if !ends {
        if is_main {
            if c.has_stage_out {
                c.line("return out;");
            }
        } else if func.return_type != A::VOID {
            unsupported!("value function without a return");
        }
    }
    c.end();
    c.line("");
    Ok(())
}

fn emit_block_contents(c: &mut Ctx, block: AstId) -> R<()> {
    let info = c.info;
    let ast = &info.ast;
    let saved = c.current_block;
    c.current_block = block;
    for (index, n) in ast.children(block).into_iter().enumerate() {
        // Declarations placed here (place_declarations); one that this statement assigns takes its value.
        let mut inline_decl = None;
        if let Some(ids) = c.mem_at.get(&(block, index)).cloned() {
            for id in ids {
                let m = c.props.local_memories.get(&id).ok_or("msl: local memory")?;
                c.line(&format!("spvUnsafeArray<{}, {}> lmem{id};", type_name(m.ty.element())?, m.array_length));
            }
        }
        if let Some(ls) = c.decl_at.get(&(block, index)).cloned() {
            for l in ls {
                let assigned_here = match ast.kind(n) {
                    AstKind::Assignment { destination, source } => *destination == l && !source.is_some_and(|s| local_refs(c, s).contains(&l)),
                    _ => false,
                };
                let t = type_name(ast.opnd(l).var_type)?;
                if assigned_here {
                    inline_decl = Some(t);
                } else {
                    let name = c.locals[&l].clone();
                    c.line(&format!("{t} {name};"));
                }
            }
        }
        match ast.kind(n) {
            AstKind::Block(b) => {
                let cond = b.condition;
                match b.ty {
                    AstBlockType::If => {
                        let e = c.expr(A::BOOL, cond)?;
                        c.flush_pre();
                        c.line(&format!("if ({e})"));
                        c.begin();
                        emit_block_contents(c, n)?;
                        c.end();
                    }
                    AstBlockType::Else => {
                        c.line("else");
                        c.begin();
                        emit_block_contents(c, n)?;
                        c.end();
                    }
                    AstBlockType::DoWhile => {
                        c.line("do");
                        c.begin();
                        emit_block_contents(c, n)?;
                        // The condition is evaluated in the loop's own scope (its continue target).
                        let saved_inner = c.current_block;
                        c.current_block = n;
                        let e = c.expr(A::BOOL, cond)?;
                        c.current_block = saved_inner;
                        c.flush_pre();
                        c.end();
                        c.line(&format!("while ({e});"));
                    }
                    t => unsupported!("block type {t:?}"),
                }
            }
            AstKind::Assignment { destination, source } => {
                let (dest, source) = (*destination, *source);
                let d = ast.opnd(dest);
                let name = match d.ty {
                    OperandType::LocalVariable => c.locals.get(&dest).cloned().ok_or("msl: unknown local")?,
                    OperandType::Argument => format!("a{}", d.value),
                    t => unsupported!("assignment to {t:?}"),
                };
                let ty = match d.ty {
                    OperandType::Argument => info.functions[c.func].argument_type(d.value as usize),
                    _ => d.var_type,
                };
                let v = c.expr(ty, source)?;
                c.flush_pre();
                match inline_decl {
                    Some(t) => c.line(&format!("{t} {name} = {v};")),
                    None => c.line(&format!("{name} = {v};")),
                }
            }
            AstKind::Operation(_) => {
                let (_, e) = ops::operation(c, n)?;
                c.flush_pre();
                if let Some(e) = e {
                    c.line(&format!("{e};"));
                }
            }
            AstKind::Comment(_) => {}
            AstKind::Operand(_) => unsupported!("operand statement"),
        }
    }
    c.current_block = saved;
    Ok(())
}

/// A helper function's name: `_N` as SPIRV-Cross names them by id (clear of the struct names).
pub fn function_name(index: usize) -> String {
    format!("_{}", 1000 + index)
}

pub fn is_main(c: &Ctx) -> bool {
    c.func == 0
}

pub fn io_key(c: &Ctx, sk: StorageKind, v: IoVariable, location: i32) -> IoDefinition {
    let _ = c;
    IoDefinition { storage_kind: sk, io_variable: v, location, component: 0 }
}

#[cfg(test)]
mod tests {
    use super::float_literal;

    #[test]
    fn float_literals_as_c_prints_them() {
        let cases: [(f32, &str); 8] = [
            (7.345489978790283203125, "7.345489978790283203125"),
            (1.4012984643248170709237295832899e-45, "1.4012984643248170709237295832899e-45"),
            (0.0319039933383464813232421875, "0.0319039933383464813232421875"),
            (256.0, "256.0"),
            (1e30, "1000000015047466219876688855040.0"),
            (f32::MAX, "3.4028234663852885981170418348452e+38"),
            (-0.0, "-0.0"),
            (0.5, "0.5"),
        ];
        for (v, s) in cases {
            assert_eq!(float_literal(v.to_bits()), s);
        }
    }
}
