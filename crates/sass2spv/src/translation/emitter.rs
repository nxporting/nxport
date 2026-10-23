// Builds a function's operations while instructions are emitted (EmitterContext and EmitterContextInsts).

use crate::collections::NetMap;
use crate::decoders::register::RegisterType;
use crate::decoders::{BlockIdx, DecodedProgram, InstOp};
use crate::ir::{Instruction as I, IoVariable, Ir, NodeId, OperandId, StorageKind, TextureFlags};
use crate::translation::resource_manager::ResourceManager;
use crate::translation::TranslatorContext;
use crate::types::{AlphaTestOp, SamplerType, SetBindingPair, ShaderStage, TextureFormat};

#[derive(Clone, Copy, Default)]
struct BrxTarget {
    selector: Option<OperandId>,
    expected_value: i32,
    next_target_address: u64,
}

struct BlockLabel {
    label: OperandId,
    brx_target: BrxTarget,
}

pub struct EmitterContext<'a> {
    pub ir: &'a mut Ir,
    pub tc: &'a TranslatorContext,
    pub program: &'a DecodedProgram,
    pub rm: &'a mut ResourceManager,
    pub is_non_main: bool,
    pub curr_block: BlockIdx,
    pub curr_op: InstOp,
    operations: Vec<NodeId>,
    labels: NetMap<u64, BlockLabel>,
    /// Set where the original throws: the stage fails to translate.
    pub error: Option<String>,
}

/// Unwraps a helper's result inside an emitter, failing the stage on error.
#[macro_export]
macro_rules! tri {
    ($c:expr, $e:expr) => {
        match $e {
            Ok(v) => v,
            Err(msg) => {
                $c.fail(&msg);
                return;
            }
        }
    };
}

impl<'a> EmitterContext<'a> {
    pub fn new(ir: &'a mut Ir, tc: &'a TranslatorContext, program: &'a DecodedProgram, rm: &'a mut ResourceManager, is_non_main: bool) -> Self {
        let mut c = EmitterContext {
            ir,
            tc,
            program,
            rm,
            is_non_main,
            curr_block: BlockIdx(0),
            curr_op: InstOp::default(),
            operations: Vec::new(),
            labels: NetMap::new(),
            error: None,
        };
        c.emit_start();
        c
    }

    fn emit_start(&mut self) {
        // Vulkan needs the point size written whenever the topology is points.
        if self.tc.definitions.stage == ShaderStage::Vertex {
            let ps = self.const_f(self.tc.definitions.point_size());
            self.store_io(StorageKind::Output, IoVariable::PointSize, None, &[ps]);
        }
    }

    pub fn stage(&self) -> ShaderStage {
        self.tc.definitions.stage
    }

    pub fn operations_count(&self) -> usize {
        self.operations.len()
    }

    pub fn operations(&self) -> Vec<NodeId> {
        self.operations.clone()
    }

    pub fn raw_op(&self) -> u64 {
        self.curr_op.raw_op_code
    }

    pub fn fail(&mut self, msg: &str) {
        if self.error.is_none() {
            self.error = Some(msg.to_string());
        }
    }

    pub fn log(&self, msg: &str) {
        self.tc.gpu.log_msg(msg);
    }

    pub fn get_label(&mut self, address: u64) -> OperandId {
        self.ensure_block_label(address)
    }

    pub fn set_brx_target(&mut self, address: u64, selector: OperandId, target_value: i32, next_target_address: u64) {
        self.ensure_block_label(address);
        let bl = self.labels.get_mut(&address).unwrap();
        debug_assert!(bl.brx_target.selector.is_none());
        bl.brx_target = BrxTarget { selector: Some(selector), expected_value: target_value, next_target_address };
    }

    pub fn enter_block(&mut self, address: u64) {
        let label = self.ensure_block_label(address);
        self.mark_label(label);
        let t = self.labels.get(&address).unwrap().brx_target;
        if let Some(selector) = t.selector {
            let next = self.get_label(t.next_target_address);
            let expected = self.const_i(t.expected_value);
            let eq = self.icompare_equal(selector, expected);
            self.branch_if_false(next, eq);
        }
    }

    fn ensure_block_label(&mut self, address: u64) -> OperandId {
        if let Some(bl) = self.labels.get(&address) {
            return bl.label;
        }
        let label = self.label();
        self.labels.try_add(address, BlockLabel { label, brx_target: BrxTarget::default() });
        label
    }

    /// Transform feedback emulation and the viewport transform are off for the engine. A GL depth mode (-1..1) on a
    /// host without depth clip control becomes 0..1 here: z = z / 2 + w / 2 (Ryujinx's PrepareForVertexReturn).
    fn prepare_for_vertex_return(&mut self) {
        if self.tc.definitions.depth_mode() && !self.tc.gpu.host_supports_depth_clip_control() {
            let (k2, k3) = (self.const_i(2), self.const_i(3));
            let z = self.load_io2(StorageKind::Output, IoVariable::Position, None, k2);
            let w = self.load_io2(StorageKind::Output, IoVariable::Position, None, k3);
            let half = self.const_f(0.5);
            let half_w = self.fp_multiply(w, half, I::FP32);
            let z = self.fp_fused_multiply_add(z, half, half_w, I::FP32);
            self.store_io(StorageKind::Output, IoVariable::Position, None, &[k2, z]);
        }
    }

    pub fn prepare_for_return(&mut self) -> bool {
        if self.is_non_main {
            return true;
        }
        let defs = &self.tc.definitions;
        if defs.last_in_vertex_pipeline && matches!(defs.stage, ShaderStage::Vertex | ShaderStage::TessellationEvaluation) {
            self.prepare_for_vertex_return();
        } else if defs.stage == ShaderStage::Geometry {
            // Passthrough without host support: write each input vertex.
            if defs.gp_passthrough && !self.tc.gpu.host_supports_geometry_shader_passthrough() {
                self.write_geometry_passthrough();
            }
        } else if defs.stage == ShaderStage::Fragment {
            self.generate_alpha_to_coverage_dither_discard();

            if defs.omap_depth {
                let src = self.gpr(self.tc.depth_register());
                self.store_io(StorageKind::Output, IoVariable::FragmentOutputDepth, None, &[src]);
            }

            let alpha_test_op = defs.alpha_test_compare();
            if alpha_test_op != AlphaTestOp::Always {
                if alpha_test_op == AlphaTestOp::Never {
                    self.discard();
                } else if defs.omap_targets & 8 != 0 {
                    let comparator = match alpha_test_op {
                        AlphaTestOp::Equal => I::COMPARE_EQUAL,
                        AlphaTestOp::Greater => I::COMPARE_GREATER,
                        AlphaTestOp::GreaterOrEqual => I::COMPARE_GREATER_OR_EQUAL,
                        AlphaTestOp::Less => I::COMPARE_LESS,
                        AlphaTestOp::LessOrEqual => I::COMPARE_LESS_OR_EQUAL,
                        AlphaTestOp::NotEqual => I::COMPARE_NOT_EQUAL,
                        _ => I(0),
                    };
                    let alpha = self.gpr(3);
                    let alpha_ref = self.const_f(defs.alpha_test_reference());
                    let alpha_pass = self.add_l(I::FP32 | comparator, &[alpha, alpha_ref]);
                    let pass_label = self.label();
                    self.branch_if_true(pass_label, alpha_pass);
                    self.discard();
                    self.mark_label(pass_label);
                }
            }

            // Nothing is output when the alpha test always fails.
            if alpha_test_op == AlphaTestOp::Never {
                return false;
            }

            // BGRA targets need no swap: the host supports them.
            let omap_targets = defs.omap_targets;
            let mut reg_index_base = 0;
            for rt_index in 0..8 {
                for component in 0..4 {
                    if omap_targets & (1 << (rt_index * 4 + component)) == 0 {
                        continue;
                    }
                    let src = self.gpr(reg_index_base + component);
                    let (rt, comp) = (self.const_i(rt_index), self.const_i(component));
                    self.store_io(StorageKind::Output, IoVariable::FragmentOutputColor, None, &[rt, comp, src]);
                }
                if omap_targets & (0xf << (rt_index * 4)) != 0 {
                    reg_index_base += 4;
                }
            }
        }
        true
    }

    fn write_geometry_passthrough(&mut self) {
        let topology = self.tc.definitions.input_topology();
        let (start, end, step) = match topology {
            crate::types::InputTopology::LinesAdjacency => (1, 3, 1),
            crate::types::InputTopology::TrianglesAdjacency => (0, 6, 2),
            t => (0, crate::translation::definitions::input_vertex_count_no_adjacency(t), 1),
        };
        let mut prim_index = start;
        while prim_index < end {
            let v: Vec<OperandId> = (0..4)
                .map(|c| {
                    let (p, ci) = (self.const_i(prim_index), self.const_i(c));
                    self.load_io2(StorageKind::Input, IoVariable::Position, Some(p), ci)
                })
                .collect();
            for (c, value) in v.into_iter().enumerate() {
                let ci = self.const_i(c as i32);
                self.store_io(StorageKind::Output, IoVariable::Position, None, &[ci, value]);
            }
            // No passthrough attributes: a single stage has no next stage to pass them to.
            self.emit_vertex();
            prim_index += step;
        }
        self.end_primitive();
    }

    fn generate_alpha_to_coverage_dither_discard(&mut self) {
        let defs = &self.tc.definitions;
        if !defs.alpha_to_coverage_dither_enable() || defs.omap_targets & 8 == 0 {
            return;
        }
        // 11 11 11 10 10 10 10 00
        // 11 01 01 01 01 00 00 00
        let dither_mask = self.const_i(0xfbb99110u32 as i32);
        let (c0, c1) = (self.const_i(0), self.const_i(1));
        let frag_x = self.load_io(StorageKind::Input, IoVariable::FragmentCoord, None, Some(c0));
        let frag_y = self.load_io(StorageKind::Input, IoVariable::FragmentCoord, None, Some(c1));
        let one = self.const_i(1);
        let fx = self.fp32_convert_to_u32(frag_x);
        let x = self.bitwise_and(fx, one);
        let fy = self.fp32_convert_to_u32(frag_y);
        let one = self.const_i(1);
        let y = self.bitwise_and(fy, one);
        let one = self.const_i(1);
        let ys = self.shift_left(y, one);
        let xy = self.bitwise_or(x, ys);
        let alpha = self.gpr(3);
        let sat = self.fp_saturate(alpha, I::FP32);
        let eight = self.const_f(8.0);
        let scaled = self.fp_multiply(sat, eight, I::FP32);
        let su = self.fp32_convert_to_u32(scaled);
        let seven = self.const_i(7);
        let quantized = self.iminimum_u32(su, seven);
        let two = self.const_i(2);
        let qs = self.shift_left(quantized, two);
        let shift = self.bitwise_or(qs, xy);
        let shifted = self.shift_right_u32(dither_mask, shift);
        let one = self.const_i(1);
        let opaque = self.bitwise_and(shifted, one);
        let end_label = self.label();
        self.branch_if_true(end_label, opaque);
        self.discard();
        self.mark_label(end_label);
    }
}

impl Emit for EmitterContext<'_> {
    fn ir(&mut self) -> &mut Ir {
        self.ir
    }

    fn push_op(&mut self, n: NodeId) {
        self.operations.push(n);
    }
}

/// Builds operations (EmitterContextInsts): implemented by the instruction emitter and by the plain builder helper
/// functions are made with.
pub trait Emit {
    fn ir(&mut self) -> &mut Ir;
    fn push_op(&mut self, n: NodeId);

    fn add_node(&mut self, n: NodeId) {
        self.push_op(n);
    }

    // Operands.

    fn local(&mut self) -> OperandId {
        self.ir().local()
    }

    fn const_i(&mut self, v: i32) -> OperandId {
        self.ir().const_i(v)
    }

    fn const_f(&mut self, v: f32) -> OperandId {
        self.ir().const_f(v)
    }

    fn label(&mut self) -> OperandId {
        self.ir().label()
    }

    fn undef(&mut self) -> OperandId {
        self.ir().undef()
    }

    fn cbuf(&mut self, slot: i32, offset: i32) -> OperandId {
        self.ir().cbuf(slot, offset)
    }

    fn reg(&mut self, index: i32, ty: RegisterType) -> OperandId {
        self.ir().reg(index, ty)
    }

    fn gpr(&mut self, index: i32) -> OperandId {
        self.ir().reg(index, RegisterType::Gpr)
    }

    fn pred(&mut self, index: i32) -> OperandId {
        self.ir().reg(index, RegisterType::Predicate)
    }

    // Operations.

    fn add(&mut self, inst: I, dest: Option<OperandId>, sources: &[OperandId]) -> Option<OperandId> {
        let n = self.ir().operation(inst, dest, sources);
        self.push_op(n);
        dest
    }

    fn add_sk(&mut self, inst: I, sk: StorageKind, dest: Option<OperandId>, sources: &[OperandId]) -> Option<OperandId> {
        let n = self.ir().operation_sk(inst, sk, dest, sources);
        self.push_op(n);
        dest
    }

    /// An operation with a new local destination.
    fn add_l(&mut self, inst: I, sources: &[OperandId]) -> OperandId {
        let d = self.local();
        self.add(inst, Some(d), sources);
        d
    }

    fn add_l_sk(&mut self, inst: I, sk: StorageKind, sources: &[OperandId]) -> OperandId {
        let d = self.local();
        self.add_sk(inst, sk, Some(d), sources);
        d
    }

    /// Add(inst, (dest1, dest2), sources).
    fn add_pair(&mut self, inst: I, sources: &[OperandId]) -> (OperandId, OperandId) {
        let (a, b) = (self.local(), self.local());
        let n = self.ir().operation_multi(inst, 0, &[Some(a), Some(b)], sources);
        self.push_op(n);
        (a, b)
    }


    fn mark_label(&mut self, label: OperandId) {
        self.add(I::MARK_LABEL, Some(label), &[]);
    }

    // EmitterContextInsts.

    fn atomic(&mut self, inst: I, sk: StorageKind, sources: &[OperandId]) -> OperandId {
        self.add_l_sk(inst, sk, sources)
    }

    /// Atomic{Op}(storageKind, binding, e0, e1, value) and the compare-and-swap forms: Const(binding) first.
    fn atomic_binding(&mut self, inst: I, sk: StorageKind, binding: i32, rest: &[OperandId]) -> OperandId {
        let d = self.local();
        let b = self.const_i(binding);
        let mut s = vec![b];
        s.extend_from_slice(rest);
        self.add_sk(inst, sk, Some(d), &s);
        d
    }

    fn ballot(&mut self, a: OperandId, index: i32) -> OperandId {
        let d = self.local();
        let n = self.ir().operation_idx(I::BALLOT, index, Some(d), &[a]);
        self.add_node(n);
        d
    }

    fn barrier(&mut self) {
        self.add(I::BARRIER, None, &[]);
    }

    fn bit_count(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::BIT_COUNT, &[a])
    }

    fn bitfield_extract_s32(&mut self, a: OperandId, b: OperandId, c: OperandId) -> OperandId {
        self.add_l(I::BITFIELD_EXTRACT_S32, &[a, b, c])
    }

    fn bitfield_extract_u32(&mut self, a: OperandId, b: OperandId, c: OperandId) -> OperandId {
        self.add_l(I::BITFIELD_EXTRACT_U32, &[a, b, c])
    }

    fn bitfield_insert(&mut self, a: OperandId, b: OperandId, c: OperandId, d: OperandId) -> OperandId {
        self.add_l(I::BITFIELD_INSERT, &[a, b, c, d])
    }

    fn bitfield_reverse(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::BITFIELD_REVERSE, &[a])
    }

    fn bitwise_and(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::BITWISE_AND, &[a, b])
    }

    fn bitwise_exclusive_or(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::BITWISE_EXCLUSIVE_OR, &[a, b])
    }

    fn bitwise_not_if(&mut self, a: OperandId, invert: bool) -> OperandId {
        if invert { self.bitwise_not(a) } else { a }
    }

    fn bitwise_not(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::BITWISE_NOT, &[a])
    }

    fn bitwise_or(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::BITWISE_OR, &[a, b])
    }

    fn branch(&mut self, d: OperandId) {
        self.add(I::BRANCH, Some(d), &[]);
    }

    fn branch_if_false(&mut self, d: OperandId, a: OperandId) {
        self.add(I::BRANCH_IF_FALSE, Some(d), &[a]);
    }

    fn branch_if_true(&mut self, d: OperandId, a: OperandId) {
        self.add(I::BRANCH_IF_TRUE, Some(d), &[a]);
    }

    fn call(&mut self, func_id: i32, returns: bool, args: &[OperandId]) -> Option<OperandId> {
        let d = if returns { Some(self.local()) } else { None };
        let mut a = vec![self.const_i(func_id)];
        a.extend_from_slice(args);
        self.add(I::CALL, d, &a)
    }

    fn conditional_select(&mut self, a: OperandId, b: OperandId, c: OperandId) -> OperandId {
        self.add_l(I::CONDITIONAL_SELECT, &[a, b, c])
    }

    fn copy(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::COPY, &[a])
    }

    /// Copy(d, a): no-op when `d` is a constant (writes to RZ / PT).
    fn copy_to(&mut self, d: OperandId, a: OperandId) {
        if self.ir().is_const(d) {
            return;
        }
        self.add(I::COPY, Some(d), &[a]);
    }

    fn discard(&mut self) {
        self.add(I::DISCARD, None, &[]);
    }

    fn emit_vertex(&mut self) {
        self.add(I::EMIT_VERTEX, None, &[]);
    }

    fn end_primitive(&mut self) {
        self.add(I::END_PRIMITIVE, None, &[]);
    }

    fn find_lsb(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FIND_LSB, &[a])
    }

    fn find_msb_s32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FIND_MSB_S32, &[a])
    }

    fn find_msb_u32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FIND_MSB_U32, &[a])
    }

    fn fp32_convert_to_fp64(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_FP32_TO_FP64, &[a])
    }

    fn fp64_convert_to_fp32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_FP64_TO_FP32, &[a])
    }

    fn fp_abs_neg(&mut self, a: OperandId, abs: bool, neg: bool, fp: I) -> OperandId {
        let a = self.fp_absolute_if(a, abs, fp);
        self.fp_negate_if(a, neg, fp)
    }

    fn fp_absolute_if(&mut self, a: OperandId, abs: bool, fp: I) -> OperandId {
        if abs { self.fp_absolute(a, fp) } else { a }
    }

    fn fp_absolute(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::ABSOLUTE, &[a])
    }

    fn fp_add(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::ADD, &[a, b])
    }

    fn fp_ceiling(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::CEILING, &[a])
    }

    fn fp_compare_equal(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::COMPARE_EQUAL, &[a, b])
    }

    fn fp_compare_less(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::COMPARE_LESS, &[a, b])
    }

    fn fp32_convert_to_s32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_FP32_TO_S32, &[a])
    }

    fn fp32_convert_to_u32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_FP32_TO_U32, &[a])
    }

    fn fp64_convert_to_s32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_FP64_TO_S32, &[a])
    }

    fn fp64_convert_to_u32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_FP64_TO_U32, &[a])
    }

    fn fp_cosine(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FP32 | I::COSINE, &[a])
    }

    fn fp_divide(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::DIVIDE, &[a, b])
    }

    fn fp_exponent_b2(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FP32 | I::EXPONENT_B2, &[a])
    }

    fn fp_floor(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::FLOOR, &[a])
    }

    fn fp_fused_multiply_add(&mut self, a: OperandId, b: OperandId, c: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::FUSED_MULTIPLY_ADD, &[a, b, c])
    }

    fn fp_logarithm_b2(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FP32 | I::LOGARITHM_B2, &[a])
    }

    fn fp_maximum(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::MAXIMUM, &[a, b])
    }

    fn fp_minimum(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::MINIMUM, &[a, b])
    }

    fn fp_modulo(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::MODULO, &[a, b])
    }

    fn fp_multiply(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::MULTIPLY, &[a, b])
    }

    fn fp_negate_if(&mut self, a: OperandId, neg: bool, fp: I) -> OperandId {
        if neg { self.fp_negate(a, fp) } else { a }
    }

    fn fp_negate(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::NEGATE, &[a])
    }

    fn fp_reciprocal(&mut self, a: OperandId, fp: I) -> OperandId {
        let one = if fp == I::FP64 { self.pack_double_2x32_const(1.0) } else { self.const_f(1.0) };
        self.fp_divide(one, a, fp)
    }

    fn fp_reciprocal_square_root(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::RECIPROCAL_SQUARE_ROOT, &[a])
    }

    fn fp_round(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::ROUND, &[a])
    }

    fn fp_saturate_if(&mut self, a: OperandId, sat: bool, fp: I) -> OperandId {
        if sat { self.fp_saturate(a, fp) } else { a }
    }

    fn fp_saturate(&mut self, a: OperandId, fp: I) -> OperandId {
        let d = self.local();
        let (lo, hi) = if fp == I::FP64 {
            (self.pack_double_2x32_const(0.0), self.pack_double_2x32_const(1.0))
        } else {
            (self.const_f(0.0), self.const_f(1.0))
        };
        self.add(fp | I::CLAMP, Some(d), &[a, lo, hi]);
        d
    }

    fn fp_sine(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FP32 | I::SINE, &[a])
    }

    fn fp_square_root(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::FP32 | I::SQUARE_ROOT, &[a])
    }

    fn fp_subtract(&mut self, a: OperandId, b: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::SUBTRACT, &[a, b])
    }

    fn fp_truncate(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::TRUNCATE, &[a])
    }

    fn fp_swizzle_add(&mut self, a: OperandId, b: OperandId, mask: i32) -> OperandId {
        let d = self.local();
        let m = self.const_i(mask);
        self.add(I::SWIZZLE_ADD, Some(d), &[a, b, m]);
        d
    }

    fn fsi_begin(&mut self) {
        self.add(I::FSI_BEGIN, None, &[]);
    }

    fn fsi_end(&mut self) {
        self.add(I::FSI_END, None, &[]);
    }

    fn group_memory_barrier(&mut self) {
        self.add(I::GROUP_MEMORY_BARRIER, None, &[]);
    }

    fn iabs_neg(&mut self, a: OperandId, abs: bool, neg: bool) -> OperandId {
        let a = self.iabsolute_if(a, abs);
        self.inegate_if(a, neg)
    }

    fn iabsolute_if(&mut self, a: OperandId, abs: bool) -> OperandId {
        if abs { self.iabsolute(a) } else { a }
    }

    fn iabsolute(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::ABSOLUTE, &[a])
    }

    fn iadd(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::ADD, &[a, b])
    }

    fn iclamp_s32(&mut self, a: OperandId, b: OperandId, c: OperandId) -> OperandId {
        self.add_l(I::CLAMP, &[a, b, c])
    }

    fn iclamp_u32(&mut self, a: OperandId, b: OperandId, c: OperandId) -> OperandId {
        self.add_l(I::CLAMP_U32, &[a, b, c])
    }

    fn icompare_equal(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_EQUAL, &[a, b])
    }

    fn icompare_greater(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_GREATER, &[a, b])
    }

    fn icompare_greater_or_equal(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_GREATER_OR_EQUAL, &[a, b])
    }

    fn icompare_greater_or_equal_unsigned(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_GREATER_OR_EQUAL_U32, &[a, b])
    }

    fn icompare_greater_unsigned(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_GREATER_U32, &[a, b])
    }

    fn icompare_less(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_LESS, &[a, b])
    }

    fn icompare_less_or_equal(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_LESS_OR_EQUAL, &[a, b])
    }

    fn icompare_less_or_equal_unsigned(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_LESS_OR_EQUAL_U32, &[a, b])
    }

    fn icompare_less_unsigned(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_LESS_U32, &[a, b])
    }

    fn icompare_not_equal(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::COMPARE_NOT_EQUAL, &[a, b])
    }

    fn iconvert_s32_to_fp32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_S32_TO_FP32, &[a])
    }

    fn iconvert_s32_to_fp64(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_S32_TO_FP64, &[a])
    }

    fn iconvert_u32_to_fp32(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_U32_TO_FP32, &[a])
    }

    fn iconvert_u32_to_fp64(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::CONVERT_U32_TO_FP64, &[a])
    }

    fn imaximum_s32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::MAXIMUM, &[a, b])
    }

    fn imaximum_u32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::MAXIMUM_U32, &[a, b])
    }

    fn iminimum_s32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::MINIMUM, &[a, b])
    }

    fn iminimum_u32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::MINIMUM_U32, &[a, b])
    }

    fn imultiply(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::MULTIPLY, &[a, b])
    }

    fn inegate_if(&mut self, a: OperandId, neg: bool) -> OperandId {
        if neg { self.inegate(a) } else { a }
    }

    fn inegate(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::NEGATE, &[a])
    }

    fn isubtract(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::SUBTRACT, &[a, b])
    }

    #[allow(clippy::too_many_arguments)]
    fn tex_op(&mut self, inst: I, ty: SamplerType, format: TextureFormat, flags: TextureFlags, sb: SetBindingPair, comp: i32, dests: &[Option<OperandId>], sources: &[OperandId]) {
        let n = self.ir().texture_operation(inst, ty, format, flags, sb.set_index, sb.binding, comp, dests, sources);
        self.add_node(n);
    }

    fn image_atomic(&mut self, ty: SamplerType, format: TextureFormat, flags: TextureFlags, sb: SetBindingPair, sources: &[OperandId]) -> OperandId {
        let d = self.local();
        self.tex_op(I::IMAGE_ATOMIC, ty, format, flags, sb, 0, &[Some(d)], sources);
        d
    }

    #[allow(clippy::too_many_arguments)]
    fn image_load(&mut self, ty: SamplerType, format: TextureFormat, flags: TextureFlags, sb: SetBindingPair, comp_mask: i32, dests: &[Option<OperandId>], sources: &[OperandId]) {
        self.tex_op(I::IMAGE_LOAD, ty, format, flags, sb, comp_mask, dests, sources);
    }

    fn image_store(&mut self, ty: SamplerType, format: TextureFormat, flags: TextureFlags, sb: SetBindingPair, sources: &[OperandId]) {
        self.tex_op(I::IMAGE_STORE, ty, format, flags, sb, 0, &[], sources);
    }

    fn is_nan(&mut self, a: OperandId, fp: I) -> OperandId {
        self.add_l(fp | I::IS_NAN, &[a])
    }

    /// Load(storageKind, e0, e1).
    fn load(&mut self, sk: StorageKind, sources: &[OperandId]) -> OperandId {
        self.add_l_sk(I::LOAD, sk, sources)
    }

    /// Load(storageKind, binding, elements...).
    fn load_b(&mut self, sk: StorageKind, binding: i32, elems: &[OperandId]) -> OperandId {
        let d = self.local();
        let b = self.const_i(binding);
        let mut s = vec![b];
        s.extend_from_slice(elems);
        self.add_sk(I::LOAD, sk, Some(d), &s);
        d
    }

    /// Load(storageKind, ioVariable, primVertex, [elemIndex]).
    fn load_io(&mut self, sk: StorageKind, io: IoVariable, prim_vertex: Option<OperandId>, elem_index: Option<OperandId>) -> OperandId {
        let mut e: Vec<OperandId> = prim_vertex.into_iter().collect();
        e.extend(elem_index);
        self.load_b(sk, io as i32, &e)
    }

    /// Load(storageKind, ioVariable, primVertex, elemIndex) with a required element.
    fn load_io2(&mut self, sk: StorageKind, io: IoVariable, prim_vertex: Option<OperandId>, elem_index: OperandId) -> OperandId {
        self.load_io(sk, io, prim_vertex, Some(elem_index))
    }

    /// Load(storageKind, ioVariable, primVertex, arrayIndex, elemIndex).
    fn load_io3(&mut self, sk: StorageKind, io: IoVariable, prim_vertex: Option<OperandId>, array_index: OperandId, elem_index: OperandId) -> OperandId {
        let mut e: Vec<OperandId> = prim_vertex.into_iter().collect();
        e.extend([array_index, elem_index]);
        self.load_b(sk, io as i32, &e)
    }

    fn lod(&mut self, ty: SamplerType, flags: TextureFlags, sb: SetBindingPair, comp_index: i32, sources: &[OperandId]) -> OperandId {
        let d = self.local();
        self.tex_op(I::LOD, ty, TextureFormat::Unknown, flags, sb, comp_index, &[Some(d)], sources);
        d
    }

    fn memory_barrier(&mut self) {
        self.add(I::MEMORY_BARRIER, None, &[]);
    }

    fn multiply_high_s32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::MULTIPLY_HIGH_S32, &[a, b])
    }

    fn multiply_high_u32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::MULTIPLY_HIGH_U32, &[a, b])
    }

    fn pack_double_2x32_const(&mut self, value: f64) -> OperandId {
        let bits = value.to_bits() as i64;
        let d = self.local();
        let (lo, hi) = (self.const_i(bits as i32), self.const_i((bits >> 32) as i32));
        self.add(I::PACK_DOUBLE_2X32, Some(d), &[lo, hi]);
        d
    }

    fn pack_double_2x32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::PACK_DOUBLE_2X32, &[a, b])
    }

    fn pack_half_2x16(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::PACK_HALF_2X16, &[a, b])
    }

    fn ret(&mut self) {
        self.add(I::RETURN, None, &[]);
    }

    fn ret_value(&mut self, v: OperandId) {
        self.add(I::RETURN, None, &[v]);
    }

    fn shift_left(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::SHIFT_LEFT, &[a, b])
    }

    fn shift_right_s32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::SHIFT_RIGHT_S32, &[a, b])
    }

    fn shift_right_u32(&mut self, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(I::SHIFT_RIGHT_U32, &[a, b])
    }

    /// Shuffle / ShuffleDown / ShuffleUp / ShuffleXor with a value and a validity result.
    fn shuffle_pair(&mut self, inst: I, a: OperandId, b: OperandId, c: OperandId) -> (OperandId, OperandId) {
        self.add_pair(inst, &[a, b, c])
    }

    fn shuffle_single(&mut self, inst: I, a: OperandId, b: OperandId) -> OperandId {
        self.add_l(inst, &[a, b])
    }

    fn store(&mut self, sk: StorageKind, sources: &[OperandId]) {
        self.add_sk(I::STORE, sk, None, sources);
    }

    /// Store(storageKind, binding, elements..., value).
    fn store_b(&mut self, sk: StorageKind, binding: i32, rest: &[OperandId]) {
        let b = self.const_i(binding);
        let mut s = vec![b];
        s.extend_from_slice(rest);
        self.add_sk(I::STORE, sk, None, &s);
    }

    /// Store(storageKind, ioVariable, invocationId, [elements...,] value).
    fn store_io(&mut self, sk: StorageKind, io: IoVariable, invocation_id: Option<OperandId>, rest: &[OperandId]) {
        let mut s = vec![self.const_i(io as i32)];
        s.extend(invocation_id);
        s.extend_from_slice(rest);
        self.add_sk(I::STORE, sk, None, &s);
    }

    fn texture_sample(&mut self, ty: SamplerType, flags: TextureFlags, sb: SetBindingPair, comp_mask: i32, dests: &[Option<OperandId>], sources: &[OperandId]) {
        self.tex_op(I::TEXTURE_SAMPLE, ty, TextureFormat::Unknown, flags, sb, comp_mask, dests, sources);
    }

    fn texture_query_samples(&mut self, ty: SamplerType, flags: TextureFlags, sb: SetBindingPair, sources: &[OperandId]) -> OperandId {
        let d = self.local();
        self.tex_op(I::TEXTURE_QUERY_SAMPLES, ty, TextureFormat::Unknown, flags, sb, 0, &[Some(d)], sources);
        d
    }

    fn texture_query_size(&mut self, ty: SamplerType, flags: TextureFlags, sb: SetBindingPair, comp_index: i32, sources: &[OperandId]) -> OperandId {
        let d = self.local();
        self.tex_op(I::TEXTURE_QUERY_SIZE, ty, TextureFormat::Unknown, flags, sb, comp_index, &[Some(d)], sources);
        d
    }

    fn unpack_double_2x32_high(&mut self, a: OperandId) -> OperandId {
        self.unpack(I::UNPACK_DOUBLE_2X32, a, 1)
    }

    fn unpack_double_2x32_low(&mut self, a: OperandId) -> OperandId {
        self.unpack(I::UNPACK_DOUBLE_2X32, a, 0)
    }

    fn unpack_half_2x16_high(&mut self, a: OperandId) -> OperandId {
        self.unpack(I::UNPACK_HALF_2X16, a, 1)
    }

    fn unpack_half_2x16_low(&mut self, a: OperandId) -> OperandId {
        self.unpack(I::UNPACK_HALF_2X16, a, 0)
    }

    fn unpack(&mut self, inst: I, a: OperandId, index: i32) -> OperandId {
        let d = self.local();
        let n = self.ir().operation_idx(inst, index, Some(d), &[a]);
        self.add_node(n);
        d
    }

    fn vote_all(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::VOTE_ALL, &[a])
    }

    fn vote_all_equal(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::VOTE_ALL_EQUAL, &[a])
    }

    fn vote_any(&mut self, a: OperandId) -> OperandId {
        self.add_l(I::VOTE_ANY, &[a])
    }
}

/// Operations made outside any instruction (helper functions): new EmitterContext() in the original.
pub struct Builder<'a> {
    pub ir: &'a mut Ir,
    operations: Vec<NodeId>,
}

impl<'a> Builder<'a> {
    pub fn new(ir: &'a mut Ir) -> Self {
        Builder { ir, operations: Vec::new() }
    }

    pub fn operations(&self) -> Vec<NodeId> {
        self.operations.clone()
    }
}

impl Emit for Builder<'_> {
    fn ir(&mut self) -> &mut Ir {
        self.ir
    }

    fn push_op(&mut self, n: NodeId) {
        self.operations.push(n);
    }
}
