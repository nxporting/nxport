pub mod alu;
pub mod compare;
pub mod conversion;
pub mod emit_helper;
pub mod float;
pub mod flow;
pub mod memory;
pub mod move_op;
pub mod texture;

use crate::decoders::inst_decoders::*;
use crate::decoders::inst_name::InstName;
use crate::decoders::inst_op::InstOp;
use crate::translation::emitter_context::EmitterContext;
use emit_helper::*;

pub fn emit_op(context: &mut EmitterContext, op: &InstOp, brx_targets: &[u64]) {
    match op.name {
        InstName::Ald => {
            let ald = InstAld::new(op.raw_op_code);
            let count = ald.al_size() as usize + 1;
            memory::emit_ald(context, ald.imm11(), count, ald.dest());
        }
        InstName::Ast => {
            let ast = InstAst::new(op.raw_op_code);
            let count = ast.al_size() as usize + 1;
            memory::emit_ast(context, ast.imm11(), count, ast.src_a());
        }
        InstName::Ipa => {
            let ipa = InstIpa::new(op.raw_op_code);
            memory::emit_ipa(context, ipa.imm10(), ipa.dest());
        }
        InstName::Ldc => {
            let ldc = InstLdc::new(op.raw_op_code);
            memory::emit_ldc(context, ldc.cbuf_slot(), ldc.cbuf_offset(), ldc.dest());
        }
        InstName::Ldg => {
            let ldg = InstLdg::new(op.raw_op_code);
            let src = get_src_reg(ldg.src_a());
            memory::emit_ldg(context, src, ldg.dest());
        }
        InstName::Mov => {
            let mov = InstMovR::new(op.raw_op_code);
            move_op::emit_mov_r(context, &mov);
        }
        InstName::Mov32i => {
            let mov = InstMov32i::new(op.raw_op_code);
            move_op::emit_mov32i(context, &mov);
        }
        InstName::Sel => {
            let sel = InstSelR::new(op.raw_op_code);
            move_op::emit_sel_r(context, &sel);
        }
        InstName::Iadd => {
            let iadd = InstIaddR::new(op.raw_op_code);
            let a = get_src_reg(iadd.src_a());
            let b = get_src_reg(iadd.src_b());
            alu::emit_iadd(context, a, b, iadd.dest());
        }
        InstName::Iadd32i => {
            let iadd = InstIadd32i::new(op.raw_op_code);
            let a = get_src_reg(iadd.src_a());
            let b = get_src_imm(iadd.imm32());
            alu::emit_iadd(context, a, b, iadd.dest());
        }
        InstName::Iadd3 => {
            let iadd3 = InstIadd3R::new(op.raw_op_code);
            let a = get_src_reg(iadd3.src_a());
            let b = get_src_reg(iadd3.src_b());
            let c = get_src_reg(iadd3.src_c());
            alu::emit_iadd3(context, a, b, c, iadd3.dest());
        }
        InstName::Iscadd => {
            let isc = InstIscaddR::new(op.raw_op_code);
            let a = get_src_reg(isc.src_a());
            let b = get_src_reg(isc.src_b());
            alu::emit_iscadd(context, a, b, isc.imm5(), isc.dest());
        }
        InstName::Imnmx => {
            let imnmx = InstImnmxR::new(op.raw_op_code);
            let a = get_src_reg(imnmx.src_a());
            let b = get_src_reg(imnmx.src_b());
            alu::emit_imnmx(context, a, b, imnmx.src_pred(), imnmx.src_pred_inv(), imnmx.dest(), imnmx.signed());
        }
        InstName::Xmad => {
            let xmad = InstXmadR::new(op.raw_op_code);
            let a = get_src_reg(xmad.src_a());
            let b = get_src_reg(xmad.src_b());
            let c = get_src_reg(xmad.src_c());
            alu::emit_xmad(context, a, b, c, xmad.dest(), xmad.psl(), xmad.mrg());
        }
        InstName::Lop => {
            let lop = InstLopR::new(op.raw_op_code);
            let a = get_src_reg(lop.src_a());
            let b = get_src_reg(lop.src_b());
            alu::emit_lop(context, lop.lop(), a, b, lop.dest(), lop.neg_a(), lop.neg_b());
        }
        InstName::Lop32i => {
            let lop = InstLop32i::new(op.raw_op_code);
            let a = get_src_reg(lop.src_a());
            let b = get_src_imm(lop.imm32());
            alu::emit_lop(context, lop.logic_op(), a, b, lop.dest(), false, false);
        }
        InstName::Shl => {
            let shl = InstShlR::new(op.raw_op_code);
            let a = get_src_reg(shl.src_a());
            let shift = get_src_reg(shl.src_b());
            alu::emit_shl(context, a, shift, shl.dest());
        }
        InstName::Shr => {
            let shr = InstShrR::new(op.raw_op_code);
            let a = get_src_reg(shr.src_a());
            let shift = get_src_reg(shr.src_b());
            alu::emit_shr(context, a, shift, shr.dest(), shr.signed());
        }
        InstName::Bfe => {
            let bfe = InstBfeR::new(op.raw_op_code);
            let a = get_src_reg(bfe.src_a());
            let b = get_src_reg(bfe.src_b());
            alu::emit_bfe(context, a, b, bfe.dest(), false);
        }
        InstName::Fadd | InstName::Fadd32i => {
            let fadd = InstFaddR::new(op.raw_op_code);
            let a = get_src_reg(fadd.src_a());
            let b = get_src_reg(fadd.src_b());
            float::emit_fadd(context, a, b, fadd.dest());
        }
        InstName::Fmul | InstName::Fmul32i => {
            let fmul = InstFmulR::new(op.raw_op_code);
            let a = get_src_reg(fmul.src_a());
            let b = get_src_reg(fmul.src_b());
            float::emit_fmul(context, a, b, fmul.dest());
        }
        InstName::Ffma | InstName::Ffma32i => {
            let ffma = InstFfmaR::new(op.raw_op_code);
            let a = get_src_reg(ffma.src_a());
            let b = get_src_reg(ffma.src_b());
            let c = get_src_reg(ffma.src_c());
            float::emit_ffma(context, a, b, c, ffma.dest());
        }
        InstName::Fmnmx => {
            let fmnmx = InstFmnmxR::new(op.raw_op_code);
            let a = get_src_reg(fmnmx.src_a());
            let b = get_src_reg(fmnmx.src_b());
            float::emit_fmnmx(context, a, b, fmnmx.src_pred(), fmnmx.src_pred_inv(), fmnmx.dest());
        }
        InstName::Mufu => {
            let mufu = InstMufu::new(op.raw_op_code);
            let a = get_src_reg(mufu.src_a());
            float::emit_mufu(context, mufu.mufu_op(), a, mufu.dest());
        }
        InstName::Rro => {
            let rro = InstRroR::new(op.raw_op_code);
            let b = get_src_reg(rro.src_b());
            float::emit_rro(context, b, rro.dest());
        }
        InstName::F2f => {
            let f2f = InstF2fR::new(op.raw_op_code);
            let b = get_src_reg(f2f.src_b());
            conversion::emit_f2f(context, b, f2f.dest(), f2f.abs_b(), f2f.neg_b());
        }
        InstName::F2i => {
            let f2i = InstF2iR::new(op.raw_op_code);
            let b = get_src_reg(f2i.src_b());
            conversion::emit_f2i(context, b, f2i.dest(), true);
        }
        InstName::I2f => {
            let i2f = InstI2fR::new(op.raw_op_code);
            let b = get_src_reg(i2f.src_b());
            conversion::emit_i2f(context, b, i2f.dest(), true);
        }
        InstName::I2i => {
            let i2i = InstI2iR::new(op.raw_op_code);
            let b = get_src_reg(i2i.src_b());
            conversion::emit_i2i(context, b, i2i.dest());
        }
        InstName::Iset => {
            let iset = InstIsetR::new(op.raw_op_code);
            let a = get_src_reg(iset.src_a());
            let b = get_src_reg(iset.src_b());
            compare::emit_iset(context, iset.i_comp(), iset.bop(), a, b, iset.src_pred(), iset.src_pred_inv(), iset.dest(), false, iset.signed());
        }
        InstName::Isetp => {
            let isetp = InstIsetpR::new(op.raw_op_code);
            let a = get_src_reg(isetp.src_a());
            let b = get_src_reg(isetp.src_b());
            compare::emit_isetp(context, isetp.i_comp(), isetp.bop(), a, b, isetp.src_pred(), isetp.src_pred_inv(), isetp.dest_pred(), isetp.dest_pred_inv(), isetp.signed());
        }
        InstName::Fset => {
            let fset = InstFsetR::new(op.raw_op_code);
            let a = get_src_reg(fset.src_a());
            let b = get_src_reg(fset.src_b());
            compare::emit_fset(context, fset.f_comp(), fset.bop(), a, b, fset.src_pred(), fset.src_pred_inv(), fset.dest(), false);
        }
        InstName::Fsetp => {
            let fsetp = InstFsetpR::new(op.raw_op_code);
            let a = get_src_reg(fsetp.src_a());
            let b = get_src_reg(fsetp.src_b());
            compare::emit_fsetp(context, fsetp.f_comp(), fsetp.bop(), a, b, fsetp.src_pred(), fsetp.src_pred_inv(), fsetp.dest_pred(), fsetp.dest_pred_inv());
        }
        InstName::Tex => {
            let tex = InstTex::new(op.raw_op_code);
            texture::emit_tex(context, tex.tid_b(), tex.src_a(), tex.dest());
        }
        InstName::Texs => {
            let texs = InstTexs::new(op.raw_op_code);
            texture::emit_texs(context, texs.tid_b(), texs.src_a(), texs.src_b(), texs.dest());
        }
        InstName::Bra => {
            flow::emit_bra(context, op);
        }
        InstName::Brx => {
            flow::emit_brx(context, op, brx_targets);
        }
        InstName::Brk => {
            flow::emit_brk(context);
        }
        InstName::Sync => {
            flow::emit_sync(context);
        }
        InstName::Kil => {
            flow::emit_kil(context);
        }
        InstName::Exit => {
            flow::emit_exit(context);
        }
        _ => {
            // Nop or unhandled op
        }
    }
}

