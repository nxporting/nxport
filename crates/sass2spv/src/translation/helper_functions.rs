// Functions the translator adds to a program for operations the host lacks (HelperFunctionManager).

use std::collections::HashMap;

use crate::ir::{Function, Instruction as I, IoVariable, Ir, OperandId, StorageKind};
use crate::translation::cfg::ControlFlowGraph;
use crate::translation::emitter::{Builder, Emit};
use crate::translation::properties::SupportBufferField;
use crate::types::ShaderStage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HelperFunctionName {
    Invalid,
    ConvertDoubleToFloat,
    ConvertFloatToDouble,
    SharedAtomicMaxS32,
    SharedAtomicMinS32,
    SharedStore8,
    SharedStore16,
    Shuffle,
    ShuffleDown,
    ShuffleUp,
    ShuffleXor,
    TexelFetchScale,
    TextureSizeUnscale,
}

/// The program's functions: the shader's own (filled in once translated) followed by the helpers added on the way.
pub struct HelperFunctionManager {
    pub functions: Vec<Option<Function>>,
    function_ids: HashMap<i32, i32>,
    stage: ShaderStage,
}

fn function(ir: &mut Ir, ops: &[crate::ir::NodeId], name: &str, returns_value: bool, ins: i32, outs: i32) -> Function {
    let cfg = ControlFlowGraph::create(ir, ops);
    Function { blocks: cfg.blocks, name: name.to_string(), returns_value, in_arguments_count: ins, out_arguments_count: outs }
}

impl HelperFunctionManager {
    pub fn new(count: usize, stage: ShaderStage) -> Self {
        HelperFunctionManager { functions: (0..count).map(|_| None).collect(), function_ids: HashMap::new(), stage }
    }

    pub fn add_function(&mut self, f: Function) -> i32 {
        let id = self.functions.len() as i32;
        self.functions.push(Some(f));
        id
    }

    pub fn get_or_create_function_id(&mut self, ir: &mut Ir, name: HelperFunctionName) -> Result<i32, String> {
        if let Some(&id) = self.function_ids.get(&(name as i32)) {
            return Ok(id);
        }
        let f = self.generate_function(ir, name)?;
        let id = self.add_function(f);
        self.function_ids.insert(name as i32, id);
        Ok(id)
    }

    pub fn get_or_create_function_id_with(&mut self, ir: &mut Ir, name: HelperFunctionName, id: i32) -> Result<i32, String> {
        let key = name as i32 | (id << 16);
        if let Some(&fid) = self.function_ids.get(&key) {
            return Ok(fid);
        }
        let f = match name {
            HelperFunctionName::SharedAtomicMaxS32 => generate_shared_atomic_signed(ir, id, false),
            HelperFunctionName::SharedAtomicMinS32 => generate_shared_atomic_signed(ir, id, true),
            HelperFunctionName::SharedStore8 => generate_shared_store(ir, id, 8),
            HelperFunctionName::SharedStore16 => generate_shared_store(ir, id, 16),
            n => return Err(format!("Invalid function name {n:?}")),
        };
        let fid = self.add_function(f);
        self.function_ids.insert(key, fid);
        Ok(fid)
    }

    pub fn get_or_create_shuffle_function_id(&mut self, ir: &mut Ir, name: HelperFunctionName, subgroup_size: i32) -> Result<i32, String> {
        if let Some(&id) = self.function_ids.get(&(name as i32)) {
            return Ok(id);
        }
        let f = match name {
            HelperFunctionName::Shuffle => generate_shuffle(ir, subgroup_size, ShuffleKind::Index),
            HelperFunctionName::ShuffleDown => generate_shuffle(ir, subgroup_size, ShuffleKind::Down),
            HelperFunctionName::ShuffleUp => generate_shuffle(ir, subgroup_size, ShuffleKind::Up),
            HelperFunctionName::ShuffleXor => generate_shuffle(ir, subgroup_size, ShuffleKind::Xor),
            n => return Err(format!("Invalid function name {n:?}")),
        };
        let id = self.add_function(f);
        self.function_ids.insert(name as i32, id);
        Ok(id)
    }

    fn generate_function(&self, ir: &mut Ir, name: HelperFunctionName) -> Result<Function, String> {
        Ok(match name {
            HelperFunctionName::ConvertDoubleToFloat => generate_convert_double_to_float(ir),
            HelperFunctionName::ConvertFloatToDouble => generate_convert_float_to_double(ir),
            HelperFunctionName::TexelFetchScale => self.generate_texel_fetch_scale(ir),
            HelperFunctionName::TextureSizeUnscale => self.generate_texture_size_unscale(ir),
            n => return Err(format!("Invalid function name {n:?}")),
        })
    }

    fn generate_texel_fetch_scale(&self, ir: &mut Ir) -> Function {
        let mut c = Builder::new(ir);
        let input = c.ir.argument(0);
        let sampler_index = c.ir.argument(1);
        let index = self.scale_index(&mut c, sampler_index);
        let field = c.const_i(SupportBufferField::RenderScale as i32);
        let scale = c.load_b(StorageKind::ConstantBuffer, 0, &[field, index]);
        let one = c.const_f(1.0);
        let scale_is_one = c.fp_compare_equal(scale, one, I::FP32);
        let lbl_scale_not_one = c.label();
        c.branch_if_false(lbl_scale_not_one, scale_is_one);
        c.ret_value(input);
        c.mark_label(lbl_scale_not_one);
        let in_arguments_count;
        if self.stage == ShaderStage::Fragment {
            let zero = c.const_f(0.0);
            let scale_is_less_than_zero = c.fp_compare_less(scale, zero, I::FP32);
            let lbl = c.label();
            c.branch_if_false(lbl, scale_is_less_than_zero);
            let neg_scale = c.fp_negate(scale, I::FP32);
            let fi = c.iconvert_s32_to_fp32(input);
            let input_scaled = c.fp_multiply(fi, neg_scale, I::FP32);
            let c0 = c.const_i(0);
            let frag_coord_x = c.load_io(StorageKind::Input, IoVariable::FragmentCoord, None, Some(c0));
            let c1 = c.const_i(1);
            let frag_coord_y = c.load_io(StorageKind::Input, IoVariable::FragmentCoord, None, Some(c1));
            let a2 = c.ir.argument(2);
            let frag_coord = c.conditional_select(a2, frag_coord_y, frag_coord_x);
            let input_bias = c.fp_modulo(frag_coord, neg_scale, I::FP32);
            let input_with_bias = c.fp_add(input_scaled, input_bias, I::FP32);
            let r = c.fp32_convert_to_s32(input_with_bias);
            c.ret_value(r);
            c.mark_label(lbl);
            in_arguments_count = 3;
        } else {
            in_arguments_count = 2;
        }
        let fi = c.iconvert_s32_to_fp32(input);
        let input_scaled2 = c.fp_multiply(fi, scale, I::FP32);
        let r = c.fp32_convert_to_s32(input_scaled2);
        c.ret_value(r);
        let ops = c.operations();
        function(ir, &ops, "TexelFetchScale", true, in_arguments_count, 0)
    }

    fn generate_texture_size_unscale(&self, ir: &mut Ir) -> Function {
        let mut c = Builder::new(ir);
        let input = c.ir.argument(0);
        let sampler_index = c.ir.argument(1);
        let index = self.scale_index(&mut c, sampler_index);
        let field = c.const_i(SupportBufferField::RenderScale as i32);
        let l = c.load_b(StorageKind::ConstantBuffer, 0, &[field, index]);
        let scale = c.fp_absolute(l, I::FP32);
        let one = c.const_f(1.0);
        let scale_is_one = c.fp_compare_equal(scale, one, I::FP32);
        let lbl_scale_not_one = c.label();
        c.branch_if_false(lbl_scale_not_one, scale_is_one);
        c.ret_value(input);
        c.mark_label(lbl_scale_not_one);
        let fi = c.iconvert_s32_to_fp32(input);
        let input_unscaled = c.fp_divide(fi, scale, I::FP32);
        let r = c.fp32_convert_to_s32(input_unscaled);
        c.ret_value(r);
        let ops = c.operations();
        function(ir, &ops, "TextureSizeUnscale", true, 2, 0)
    }

    fn scale_index(&self, c: &mut Builder, index: OperandId) -> OperandId {
        match self.stage {
            ShaderStage::Vertex => {
                let field = c.const_i(SupportBufferField::FragmentRenderScaleCount as i32);
                let frag_scale_count = c.load_b(StorageKind::ConstantBuffer, 0, &[field]);
                let one = c.const_i(1);
                let s = c.iadd(index, frag_scale_count);
                c.iadd(one, s)
            }
            _ => {
                let one = c.const_i(1);
                c.iadd(one, index)
            }
        }
    }
}

fn generate_convert_double_to_float(ir: &mut Ir) -> Function {
    let mut c = Builder::new(ir);
    let value_low = c.ir.argument(0);
    let value_high = c.ir.argument(1);
    let k = c.const_i((1 << 22) - 1);
    let mantissa_low = c.bitwise_and(value_low, k);
    let k = c.const_i(22);
    let mut mantissa = c.shift_right_u32(value_low, k);
    let k = c.const_i(0xfffff);
    let t = c.bitwise_and(value_high, k);
    let k = c.const_i(10);
    let t = c.shift_left(t, k);
    mantissa = c.bitwise_or(mantissa, t);
    let (k1, k0) = (c.const_i(1), c.const_i(0));
    let t = c.conditional_select(mantissa_low, k1, k0);
    mantissa = c.bitwise_or(mantissa, t);
    let k = c.const_i(20);
    let t = c.shift_right_u32(value_high, k);
    let k = c.const_i(0x7ff);
    let mut exp = c.bitwise_and(t, k);
    let k = c.const_i(31);
    let sign = c.shift_right_s32(value_high, k);
    let k = c.const_i(31);
    let result_sign = c.shift_left(sign, k);
    let not_zero = c.bitwise_or(mantissa, exp);
    let lbl_not_zero = c.label();
    c.branch_if_true(lbl_not_zero, not_zero);
    c.ret_value(result_sign);
    c.mark_label(lbl_not_zero);
    let k = c.const_i(0x7ff);
    let not_nan_or_inf = c.icompare_not_equal(exp, k);
    let k = c.const_i(0x40000000);
    mantissa = c.bitwise_or(mantissa, k);
    let k = c.const_i(0x381);
    exp = c.isubtract(exp, k);
    // Overflow is not handled and may give wrong results.
    let k = c.const_i(0x7f);
    let round_bits = c.bitwise_and(mantissa, k);
    let k = c.const_i(0x40);
    let round_bits_xor_64 = c.bitwise_exclusive_or(round_bits, k);
    let k = c.const_i(0x40);
    let t = c.iadd(mantissa, k);
    let k = c.const_i(7);
    mantissa = c.shift_right_u32(t, k);
    let (a, b) = (c.const_i(!0), c.const_i(!1));
    let t = c.conditional_select(round_bits_xor_64, a, b);
    mantissa = c.bitwise_and(mantissa, t);
    let k = c.const_i(0);
    exp = c.conditional_select(mantissa, exp, k);
    let k = c.const_i(0xff);
    exp = c.conditional_select(not_nan_or_inf, exp, k);
    let k = c.const_i(23);
    let t = c.shift_left(exp, k);
    let t = c.iadd(mantissa, t);
    let result = c.iadd(t, result_sign);
    c.ret_value(result);
    let ops = c.operations();
    function(ir, &ops, "ConvertDoubleToFloat", true, 2, 0)
}

fn generate_convert_float_to_double(ir: &mut Ir) -> Function {
    let mut c = Builder::new(ir);
    let value = c.ir.argument(0);
    let k = c.const_i(0x7fffff);
    let mut mantissa = c.bitwise_and(value, k);
    let k = c.const_i(23);
    let t = c.shift_right_u32(value, k);
    let k = c.const_i(0xff);
    let mut exp = c.bitwise_and(t, k);
    let k = c.const_i(31);
    let sign = c.shift_right_s32(value, k);
    let k = c.const_i(0xff);
    let not_nan_or_inf = c.icompare_not_equal(exp, k);
    let k = c.const_i(0);
    let exp_not_zero = c.icompare_not_equal(exp, k);
    let k = c.const_i(0);
    let t = c.icompare_equal(mantissa, k);
    let not_denorm = c.bitwise_or(exp_not_zero, t);
    let k = c.const_i(0x380);
    exp = c.iadd(exp, k);
    let k32 = c.const_i(32);
    let msb = c.find_msb_u32(mantissa);
    let shift_dist = c.isubtract(k32, msb);
    let k1 = c.const_i(1);
    let t = c.isubtract(k1, shift_dist);
    let k = c.const_i(1);
    let norm_exp = c.isubtract(t, k);
    let norm_mant = c.shift_left(mantissa, shift_dist);
    let k = c.const_i(0x7ff);
    exp = c.conditional_select(not_nan_or_inf, exp, k);
    exp = c.conditional_select(not_denorm, exp, norm_exp);
    mantissa = c.conditional_select(exp_not_zero, mantissa, norm_mant);
    let k = c.const_i(29);
    let result_low = c.shift_left(mantissa, k);
    let k = c.const_i(3);
    let mut result_high = c.shift_right_u32(mantissa, k);
    let k = c.const_i(20);
    let t = c.shift_left(exp, k);
    result_high = c.iadd(result_high, t);
    let k = c.const_i(31);
    let t = c.shift_left(sign, k);
    result_high = c.iadd(result_high, t);
    let a1 = c.ir.argument(1);
    c.copy_to(a1, result_low);
    let a2 = c.ir.argument(2);
    c.copy_to(a2, result_high);
    c.ret();
    let ops = c.operations();
    function(ir, &ops, "ConvertFloatToDouble", false, 1, 2)
}

fn generate_shared_atomic_signed(ir: &mut Ir, id: i32, is_min: bool) -> Function {
    let mut c = Builder::new(ir);
    let word_offset = c.ir.argument(0);
    let value = c.ir.argument(1);
    let result = shared_atomic_cas_loop(&mut c, word_offset, id, |c, mem| {
        if is_min { c.iminimum_s32(mem, value) } else { c.imaximum_s32(mem, value) }
    });
    c.ret_value(result);
    let ops = c.operations();
    let name = format!("SharedAtomic{}_{id}", if is_min { "Min" } else { "Max" });
    function(ir, &ops, &name, true, 2, 0)
}

fn generate_shared_store(ir: &mut Ir, id: i32, bit_size: i32) -> Function {
    let mut c = Builder::new(ir);
    let offset = c.ir.argument(0);
    let value = c.ir.argument(1);
    let k = c.const_i(2);
    let word_offset = c.shift_right_u32(offset, k);
    let bit_offset = bit_offset(&mut c, offset);
    shared_atomic_cas_loop(&mut c, word_offset, id, |c, mem| {
        let size = c.const_i(bit_size);
        c.bitfield_insert(mem, value, bit_offset, size)
    });
    c.ret();
    let ops = c.operations();
    function(ir, &ops, &format!("SharedStore{bit_size}_{id}"), false, 2, 0)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ShuffleKind {
    Index,
    Down,
    Up,
    Xor,
}

fn generate_shuffle(ir: &mut Ir, subgroup_size: i32, kind: ShuffleKind) -> Function {
    let mut c = Builder::new(ir);
    let value = c.ir.argument(0);
    let index = c.ir.argument(1);
    let mask = c.ir.argument(2);
    let clamp = if kind != ShuffleKind::Up {
        let k = c.const_i(0x1f);
        Some(c.bitwise_and(mask, k))
    } else {
        None
    };
    let k = c.const_i(8);
    let t = c.shift_right_u32(mask, k);
    let k = c.const_i(0x1f);
    let seg_mask = c.bitwise_and(t, k);
    let (valid, src_thread_id) = match kind {
        ShuffleKind::Index => {
            let lane = load_subgroup_lane_id(&mut c, subgroup_size);
            let min_thread_id = c.bitwise_and(lane, seg_mask);
            let ns = c.bitwise_not(seg_mask);
            let t = c.bitwise_and(clamp.unwrap(), ns);
            let max_thread_id = c.bitwise_or(t, min_thread_id);
            let ns = c.bitwise_not(seg_mask);
            let t = c.bitwise_and(index, ns);
            let src_thread_id = c.bitwise_or(t, min_thread_id);
            (c.icompare_less_or_equal_unsigned(src_thread_id, max_thread_id), src_thread_id)
        }
        ShuffleKind::Down | ShuffleKind::Xor => {
            let lane_id = load_subgroup_lane_id(&mut c, subgroup_size);
            let min_thread_id = c.bitwise_and(lane_id, seg_mask);
            let ns = c.bitwise_not(seg_mask);
            let t = c.bitwise_and(clamp.unwrap(), ns);
            let max_thread_id = c.bitwise_or(t, min_thread_id);
            let src_thread_id = if kind == ShuffleKind::Down { c.iadd(lane_id, index) } else { c.bitwise_exclusive_or(lane_id, index) };
            (c.icompare_less_or_equal_unsigned(src_thread_id, max_thread_id), src_thread_id)
        }
        ShuffleKind::Up => {
            let lane_id = load_subgroup_lane_id(&mut c, subgroup_size);
            let min_thread_id = c.bitwise_and(lane_id, seg_mask);
            let src_thread_id = c.isubtract(lane_id, index);
            (c.icompare_greater_or_equal(src_thread_id, min_thread_id), src_thread_id)
        }
    };
    let a3 = c.ir.argument(3);
    c.copy_to(a3, valid);
    let shuffle_index = subgroup_shuffle_index(&mut c, src_thread_id, subgroup_size);
    let result = c.shuffle_single(I::SHUFFLE, value, shuffle_index);
    let r = c.conditional_select(valid, result, value);
    c.ret_value(r);
    let ops = c.operations();
    let name = match kind {
        ShuffleKind::Index => "Shuffle",
        ShuffleKind::Down => "ShuffleDown",
        ShuffleKind::Up => "ShuffleUp",
        ShuffleKind::Xor => "ShuffleXor",
    };
    function(ir, &ops, name, true, 3, 1)
}

fn load_subgroup_lane_id(c: &mut Builder, subgroup_size: i32) -> OperandId {
    if subgroup_size <= 32 {
        return c.load_io(StorageKind::Input, IoVariable::SubgroupLaneId, None, None);
    }
    let l = c.load_io(StorageKind::Input, IoVariable::SubgroupLaneId, None, None);
    let k = c.const_i(0x1f);
    c.bitwise_and(l, k)
}

fn subgroup_shuffle_index(c: &mut Builder, src_thread_id: OperandId, subgroup_size: i32) -> OperandId {
    if subgroup_size <= 32 {
        return src_thread_id;
    }
    let l = c.load_io(StorageKind::Input, IoVariable::SubgroupLaneId, None, None);
    let k = c.const_i(0x60);
    let t = c.bitwise_and(l, k);
    c.bitwise_or(t, src_thread_id)
}

/// HelperFunctionManager.GetBitOffset: the bit position of a byte offset in its word.
pub fn bit_offset<E: Emit>(c: &mut E, offset: OperandId) -> OperandId {
    let k = c.const_i(3);
    let t = c.bitwise_and(offset, k);
    let k = c.const_i(3);
    c.shift_left(t, k)
}

fn shared_atomic_cas_loop(c: &mut Builder, word_offset: OperandId, id: i32, op: impl FnOnce(&mut Builder, OperandId) -> OperandId) -> OperandId {
    let lbl_loop_head = c.label();
    c.mark_label(lbl_loop_head);
    let old_value = c.load_b(StorageKind::SharedMemory, id, &[word_offset]);
    let new_value = op(c, old_value);
    let cas_result = c.atomic_binding(I::ATOMIC_COMPARE_AND_SWAP, StorageKind::SharedMemory, id, &[word_offset, old_value, new_value]);
    let cas_fail = c.icompare_not_equal(cas_result, old_value);
    c.branch_if_true(lbl_loop_head, cas_fail);
    old_value
}
