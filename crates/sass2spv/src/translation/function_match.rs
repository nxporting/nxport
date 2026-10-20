// Recognises the functions games use for fragment shader interlock (begin / end) by the shape of their data flow,
// so they can be replaced by the host's interlock (FunctionMatch).

use crate::decoders::inst_decoders::*;
use crate::decoders::register::{PREDICATE_TRUE_INDEX, REGISTER_ZERO_INDEX};
use crate::decoders::{BlockIdx, DecodedProgram, FunctionIdx, FunctionType, InstName, InstOp, InstProps};

const PREDS_COUNT: usize = 8;
const GPRS_COUNT: usize = 256;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NodeType {
    Op,
    Label,
}

#[derive(Clone, Copy)]
struct TreeNodeUse {
    node: Option<usize>,
    index: i32,
    inverted: bool,
}

impl TreeNodeUse {
    fn new(index: i32, node: Option<usize>) -> Self {
        TreeNodeUse { node, index, inverted: false }
    }

    fn flip(self) -> Self {
        TreeNodeUse { inverted: !self.inverted, ..self }
    }
}

struct TreeNode {
    op: InstOp,
    uses: Vec<TreeNodeUse>,
    ty: NodeType,
    order: u8,
}

impl TreeNode {
    fn pd(&self) -> u8 {
        ((self.op.raw_op_code >> 3) & 7) as u8
    }

    fn rd(&self) -> u8 {
        self.op.raw_op_code as u8
    }
}

/// A function's data flow: its operations in order, each with the definitions it uses.
struct Tree {
    arena: Vec<TreeNode>,
    nodes: Vec<usize>,
}

impl Tree {
    fn node(&self, i: usize) -> &TreeNode {
        &self.arena[self.nodes[i]]
    }
}

fn build_tree(program: &DecodedProgram, blocks: &[BlockIdx]) -> Tree {
    let mut arena: Vec<TreeNode> = Vec::new();
    let mut nodes: Vec<usize> = Vec::new();
    let mut labels: Vec<(u64, usize)> = Vec::new();
    let mut pred_defs = [TreeNodeUse::new(0, None); PREDS_COUNT];
    let mut gpr_defs = [TreeNodeUse::new(0, None); GPRS_COUNT];

    fn remove(nodes: &mut Vec<usize>, n: usize) {
        if let Some(p) = nodes.iter().position(|&x| x == n) {
            nodes.remove(p);
        }
    }

    fn use_pred(nodes: &mut Vec<usize>, pred_defs: &[TreeNodeUse], pred_index: u8, pred_inv: bool) -> TreeNodeUse {
        if pred_index as i32 != PREDICATE_TRUE_INDEX {
            let mut u = pred_defs[pred_index as usize];
            match u.node {
                Some(n) => remove(nodes, n),
                None => u = TreeNodeUse::new(-(pred_index as i32 + 2), None),
            }
            return if pred_inv { u.flip() } else { u };
        }
        TreeNodeUse::new(-1, None)
    }

    fn use_gpr(nodes: &mut Vec<usize>, gpr_defs: &[TreeNodeUse], reg_index: u8) -> TreeNodeUse {
        if reg_index as i32 != REGISTER_ZERO_INDEX {
            let mut u = gpr_defs[reg_index as usize];
            match u.node {
                Some(n) => remove(nodes, n),
                None => u = TreeNodeUse::new(-(reg_index as i32 + 2), None),
            }
            return u;
        }
        TreeNodeUse::new(-1, None)
    }

    let mut order: u8 = 0;
    for &b in blocks {
        let block = program.block(b);
        if block.predecessors.len() > 1 {
            arena.push(TreeNode { op: InstOp::default(), uses: Vec::new(), ty: NodeType::Label, order });
            order = order.wrapping_add(1);
            let label = arena.len() - 1;
            nodes.push(label);
            labels.push((block.address, label));
        }
        for &op in &block.op_codes {
            let mut uses = Vec::new();
            if !op.props.has(InstProps::NO_PRED) {
                let pred_index = ((op.raw_op_code >> 16) & 7) as u8;
                let pred_inv = op.raw_op_code & 0x80000 != 0;
                uses.push(use_pred(&mut nodes, &pred_defs, pred_index, pred_inv));
            }
            if op.props.has(InstProps::PS) {
                let pred_index = ((op.raw_op_code >> 39) & 7) as u8;
                let pred_inv = op.raw_op_code & 0x40000000000 != 0;
                uses.push(use_pred(&mut nodes, &pred_defs, pred_index, pred_inv));
            }
            if op.props.has(InstProps::RA) {
                let ra = (op.raw_op_code >> 8) as u8;
                uses.push(use_gpr(&mut nodes, &gpr_defs, ra));
            }
            if op.props.0 & (InstProps::RB.0 | InstProps::RB2.0) != 0 {
                let rb = if op.props.has(InstProps::RB2) { op.raw_op_code as u8 } else { (op.raw_op_code >> 20) as u8 };
                uses.push(use_gpr(&mut nodes, &gpr_defs, rb));
            }
            if op.props.has(InstProps::RC) {
                let rc = (op.raw_op_code >> 39) as u8;
                uses.push(use_gpr(&mut nodes, &gpr_defs, rc));
            }
            if op.name == InstName::Bra {
                let target = op.absolute_address();
                if let Some(&(_, label)) = labels.iter().find(|(a, _)| *a == target) {
                    uses.push(TreeNodeUse::new(0, Some(label)));
                }
            }
            arena.push(TreeNode { op, uses, ty: NodeType::Op, order: if is_order_dependant(op.name) { order } else { 0 } });
            let node = arena.len() - 1;

            // Definitions.
            let mut def_index = 0;
            let pd_type = InstProps(op.props.0 & InstProps::PD_MASK.0);
            if pd_type.0 != 0 {
                let bit = match pd_type {
                    InstProps::PD => 3,
                    InstProps::L_PD => 48,
                    InstProps::S_PD => 30,
                    InstProps::T_PD => 51,
                    _ => 45,
                };
                let pred_index = ((op.raw_op_code >> bit) & 7) as u8;
                if pred_index as i32 != PREDICATE_TRUE_INDEX {
                    pred_defs[pred_index as usize] = TreeNodeUse::new(def_index, Some(node));
                }
                def_index += 1;
            }
            if op.props.has(InstProps::RD) {
                let rd = op.raw_op_code as u8;
                if rd as i32 != REGISTER_ZERO_INDEX {
                    gpr_defs[rd as usize] = TreeNodeUse::new(def_index, Some(node));
                }
            }
            nodes.push(node);
        }
    }
    Tree { arena, nodes }
}

fn is_order_dependant(name: InstName) -> bool {
    matches!(
        name,
        InstName::Atom
            | InstName::AtomCas
            | InstName::Atoms
            | InstName::AtomsCas
            | InstName::Ld
            | InstName::Ldg
            | InstName::Ldl
            | InstName::Lds
            | InstName::Suatom
            | InstName::SuatomB
            | InstName::SuatomB2
            | InstName::SuatomCas
            | InstName::SuatomCasB
            | InstName::Suld
            | InstName::SuldB
            | InstName::SuldD
            | InstName::SuldDB
    )
}

#[derive(Clone, Copy)]
struct PUse {
    node: Option<usize>,
    index: i32,
    inverted: bool,
}

impl PUse {
    fn inv(self) -> Self {
        PUse { inverted: !self.inverted, ..self }
    }
}

struct PNode {
    name: InstName,
    matcher: fn(u64, &[i64]) -> bool,
    args: Vec<i64>,
    ty: NodeType,
    order: u8,
    is_imm: bool,
    uses: Vec<PUse>,
}

/// Pattern trees, built in an arena.
#[derive(Default)]
struct Patterns {
    arena: Vec<PNode>,
}

const PT: PUse = PUse { node: None, index: -1, inverted: false };
const RZ: PUse = PT;

fn call_arg(index: i32) -> PUse {
    PUse { node: None, index: -(index + 2), inverted: false }
}

impl Patterns {
    fn add(&mut self, name: InstName, matcher: fn(u64, &[i64]) -> bool, args: Vec<i64>, ty: NodeType, order: u8, is_imm: bool) -> usize {
        self.arena.push(PNode { name, matcher, args, ty, order, is_imm, uses: Vec::new() });
        self.arena.len() - 1
    }

    fn op(&mut self, name: InstName, matcher: fn(u64, &[i64]) -> bool, args: Vec<i64>) -> usize {
        self.add(name, matcher, args, NodeType::Op, 0, false)
    }

    fn u(&mut self, n: usize, u: PUse) -> usize {
        self.arena[n].uses.push(u);
        n
    }

    fn uses(&mut self, n: usize, us: &[PUse]) -> usize {
        self.arena[n].uses.extend_from_slice(us);
        n
    }

    fn bfi(&mut self, imm: i32) -> usize {
        self.add(InstName::Bfi, |r, a| { let o = InstBfiI(r); !o.write_cc() && o.imm20() as i64 == a[0] }, vec![imm as i64], NodeType::Op, 0, true)
    }

    fn bra(&mut self) -> usize {
        self.op(InstName::Bra, |r, _| { let o = InstBra(r); o.ccc() == Ccc::T && !o.ca() }, vec![])
    }

    fn cal(&mut self) -> usize {
        self.op(InstName::Cal, |r, _| { let o = InstCal(r); !o.ca() && o.inc() }, vec![])
    }

    fn flo_u32(&mut self) -> usize {
        self.op(InstName::Flo, |r, _| { let o = InstFloR(r); !o.signed() && !o.sh() && !o.neg_b() && !o.write_cc() }, vec![])
    }

    fn iadd(&mut self, x: bool, cbuf_slot: i32, cbuf_offset: i32) -> usize {
        self.op(
            InstName::Iadd,
            |r, a| {
                let o = InstIaddC(r);
                !o.sat() && !o.write_cc() && o.x() == (a[0] != 0) && o.avg_mode() == AvgMode::NO_NEG && o.cbuf_slot() as i64 == a[1] && o.cbuf_offset() as i64 == a[2]
            },
            vec![x as i64, cbuf_slot as i64, cbuf_offset as i64],
        )
    }

    fn iadd32i(&mut self, imm: i32) -> usize {
        self.op(
            InstName::Iadd32i,
            |r, a| { let o = InstIadd32i(r); !o.sat() && !o.write_cc() && !o.x() && o.avg_mode() == AvgMode::NO_NEG && o.imm32() as i64 == a[0] },
            vec![imm as i64],
        )
    }

    fn iscadd(&mut self, cc: bool, imm: i32) -> usize {
        self.op(
            InstName::Iscadd,
            |r, a| { let o = InstIscaddR(r); o.write_cc() == (a[0] != 0) && o.avg_mode() == AvgMode::NO_NEG && o.imm5() as i64 == a[1] },
            vec![cc as i64, imm as i64],
        )
    }

    fn iscadd_c(&mut self, cc: bool, imm: i32, cbuf_slot: i32, cbuf_offset: i32) -> usize {
        self.op(
            InstName::Iscadd,
            |r, a| {
                let o = InstIscaddC(r);
                o.write_cc() == (a[0] != 0) && o.avg_mode() == AvgMode::NO_NEG && o.imm5() as i64 == a[1] && o.cbuf_slot() as i64 == a[2] && o.cbuf_offset() as i64 == a[3]
            },
            vec![cc as i64, imm as i64, cbuf_slot as i64, cbuf_offset as i64],
        )
    }

    fn isetp_u32(&mut self, comp: IComp) -> usize {
        self.op(InstName::Isetp, |r, a| { let o = InstIsetpR(r); !o.signed() && o.i_comp().0 as i64 == a[0] && o.bop() == BoolOp::AND }, vec![comp.0 as i64])
    }

    fn label(&mut self) -> usize {
        self.add(InstName::Invalid, |_, _| true, vec![], NodeType::Label, 0, false)
    }

    fn lop(&mut self, neg_b: bool, logic_op: LogicOp) -> usize {
        self.op(
            InstName::Lop,
            |r, a| {
                let o = InstLopR(r);
                !o.neg_a() && o.neg_b() == (a[0] != 0) && !o.write_cc() && !o.x() && o.lop().0 as i64 == a[1] && o.predicate_op() == PredicateOp::F
            },
            vec![neg_b as i64, logic_op.0 as i64],
        )
    }

    fn lop32i(&mut self, logic_op: LogicOp, imm: i32) -> usize {
        self.op(
            InstName::Lop32i,
            |r, a| {
                let o = InstLop32i(r);
                !o.neg_a() && !o.neg_b() && !o.x() && !o.write_cc() && o.logic_op().0 as i64 == a[0] && o.imm32() as i64 == a[1]
            },
            vec![logic_op.0 as i64, imm as i64],
        )
    }

    fn membar(&mut self, membar: Membar) -> usize {
        self.op(InstName::Membar, |r, a| InstMembar(r).membar().0 as i64 == a[0], vec![membar.0 as i64])
    }

    fn popc(&mut self) -> usize {
        self.op(InstName::Popc, |r, _| !InstPopcR(r).neg_b(), vec![])
    }

    fn ret(&mut self) -> usize {
        self.op(InstName::Ret, |r, _| InstRet(r).ccc() == Ccc::T, vec![])
    }

    fn s2r(&mut self, reg: SReg) -> usize {
        self.op(InstName::S2r, |r, a| InstS2r(r).s_reg().0 as i64 == a[0], vec![reg.0 as i64])
    }

    fn shr_u32_w(&mut self, imm: i32) -> usize {
        self.add(
            InstName::Shr,
            |r, a| { let o = InstShrI(r); !o.signed() && !o.brev() && o.m() && o.x_mode().0 == 0 && o.imm20() as i64 == a[0] },
            vec![imm as i64],
            NodeType::Op,
            0,
            true,
        )
    }

    fn ldg_e(&mut self, cache_op: CacheOpLd, size: LsSize, order: u8) -> usize {
        self.add(
            InstName::Ldg,
            |r, a| { let o = InstLdg(r); o.e() && o.cache_op().0 as i64 == a[0] && o.ls_size().0 as i64 == a[1] },
            vec![cache_op.0 as i64, size.0 as i64],
            NodeType::Op,
            order,
            false,
        )
    }

    fn red_e(&mut self, red_op: RedOp, size: AtomSize) -> usize {
        self.op(
            InstName::Red,
            |r, a| { let o = InstRed(r); o.e() && o.red_op().0 as i64 == a[0] && o.red_size().0 as i64 == a[1] },
            vec![red_op.0 as i64, size.0 as i64],
        )
    }

    fn vote(&mut self, mode: VoteMode) -> usize {
        self.op(InstName::Vote, |r, a| InstVote(r).vote_mode().0 as i64 == a[0], vec![mode.0 as i64])
    }

    fn xmad(&mut self, cop: XmadCop, psl: bool, mrg: bool, hilo_a: bool, hilo_b: bool) -> usize {
        self.op(
            InstName::Xmad,
            |r, a| {
                let o = InstXmadR(r);
                o.xmad_cop().0 as i64 == a[0] && o.psl() == (a[1] != 0) && o.mrg() == (a[2] != 0) && o.hilo_a() == (a[3] != 0) && o.hilo_b() == (a[4] != 0)
            },
            vec![cop.0 as i64, psl as i64, mrg as i64, hilo_a as i64, hilo_b as i64],
        )
    }
}

fn out(n: usize) -> PUse {
    PUse { node: Some(n), index: 0, inverted: false }
}

fn out_at(n: usize, index: i32) -> PUse {
    PUse { node: Some(n), index, inverted: false }
}

struct PatternSet {
    p: Patterns,
    fsi_get_address: Vec<usize>,
    fsi_get_address_v2: Vec<usize>,
    fsi_is_last_warp_thread: Vec<usize>,
    fsi_begin: Vec<usize>,
    fsi_end: Vec<usize>,
}

fn build_patterns() -> PatternSet {
    let mut p = Patterns::default();

    let fsi_get_address = {
        let n = p.s2r(SReg::AFFINITY);
        let affinity_value = out(p.u(n, PT));
        let n = p.s2r(SReg::ORDERING_TICKET);
        let ordering_ticket_value = out(p.u(n, PT));
        let l1 = p.lop32i(LogicOp::AND, 0xff);
        let l1 = out(p.uses(l1, &[PT, affinity_value]));
        let l2 = p.lop32i(LogicOp::AND, 0xff);
        let l2 = out(p.uses(l2, &[PT, ordering_ticket_value]));
        let inner = p.iscadd(false, 8);
        let inner = out(p.uses(inner, &[PT, l1, l2]));
        let a = p.iscadd_c(true, 2, 0, 404);
        p.uses(a, &[PT, inner]);
        let b = p.shr_u32_w(16);
        p.uses(b, &[PT, ordering_ticket_value]);
        let l3 = p.lop32i(LogicOp::AND, 0xfe00);
        let l3 = out(p.uses(l3, &[PT, ordering_ticket_value]));
        let c = p.iadd32i(0x200);
        p.uses(c, &[PT, l3]);
        let d = p.iadd(true, 0, 405);
        p.uses(d, &[PT, RZ]);
        let e = p.ret();
        p.u(e, PT);
        vec![a, b, c, d, e]
    };

    let fsi_get_address_v2 = {
        let n = p.s2r(SReg::AFFINITY);
        let affinity_value = out(p.u(n, PT));
        let n = p.s2r(SReg::ORDERING_TICKET);
        let ordering_ticket_value = out(p.u(n, PT));
        let a = p.shr_u32_w(16);
        p.uses(a, &[PT, ordering_ticket_value]);
        let l = p.lop32i(LogicOp::AND, 0xfe00);
        let l = out(p.uses(l, &[PT, ordering_ticket_value]));
        let b = p.iadd32i(0x200);
        p.uses(b, &[PT, l]);
        let l2 = p.lop32i(LogicOp::AND, 0xff);
        let l2 = out(p.uses(l2, &[PT, ordering_ticket_value]));
        let bfi = p.bfi(0x808);
        let bfi = out(p.uses(bfi, &[PT, affinity_value, l2]));
        let c = p.iscadd_c(true, 2, 0, 404);
        p.uses(c, &[PT, bfi]);
        let d = p.iadd(true, 0, 405);
        p.uses(d, &[PT, RZ]);
        let e = p.ret();
        p.u(e, PT);
        vec![a, b, c, d, e]
    };

    let fsi_is_last_warp_thread = {
        let n = p.s2r(SReg::THREAD_KILL);
        let thread_kill_value = out(p.u(n, PT));
        let n = p.s2r(SReg::LANE_ID);
        let lane_id_value = out(p.u(n, PT));
        let lop = p.lop(true, LogicOp::PASS_B);
        let lop = out_at(p.uses(lop, &[PT, RZ, thread_kill_value]), 1);
        let ne = p.isetp_u32(IComp::NE);
        let ne = out(p.uses(ne, &[PT, PT, lop, RZ]));
        let vote = p.vote(VoteMode::ANY);
        let vote = out_at(p.uses(vote, &[PT, ne]), 1);
        let flo = p.flo_u32();
        let flo = out(p.uses(flo, &[PT, vote]));
        let a = p.isetp_u32(IComp::EQ);
        p.uses(a, &[PT, PT, flo, lane_id_value]);
        let b = p.ret();
        p.u(b, PT);
        vec![a, b]
    };

    let fsi_begin = {
        let address_low_value = call_arg(1);
        let high_u16_equals = |p: &mut Patterns, x: PUse| -> PUse {
            let expected_value = call_arg(3);
            let shr = p.shr_u32_w(16);
            let shr = out(p.uses(shr, &[PT, x]));
            let eq = p.isetp_u32(IComp::EQ);
            out(p.uses(eq, &[PT, PT, shr, expected_value]))
        };
        let a = p.cal();
        let b = p.ret();
        p.u(b, call_arg(0).inv());
        let ldg = p.ldg_e(CacheOpLd::CG, LsSize::B32, 0);
        let ldg = out(p.uses(ldg, &[PT, address_low_value]));
        let eq = high_u16_equals(&mut p, ldg);
        let c = p.ret();
        p.u(c, eq);
        let label = p.label();
        let ldg = p.ldg_e(CacheOpLd::CG, LsSize::B32, 1);
        let ldg = out(p.uses(ldg, &[PT, address_low_value]));
        let eq = high_u16_equals(&mut p, ldg).inv();
        let e = p.bra();
        p.uses(e, &[eq, out(label)]);
        let f = p.ret();
        p.u(f, PT);
        vec![a, b, c, label, e, f]
    };

    let fsi_end = {
        let vote = p.vote(VoteMode::ALL);
        let vote_result = out_at(p.uses(vote, &[PT, PT]), 1);
        let popc = p.popc();
        let popc_result = out(p.uses(popc, &[PT, vote_result]));
        let n = p.s2r(SReg::THREAD_KILL);
        let thread_kill_value = out(p.u(n, PT));
        let n = p.s2r(SReg::LANE_ID);
        let lane_id_value = out(p.u(n, PT));
        let address_low_value = call_arg(1);
        let increment_value = call_arg(2);

        let a = p.cal();
        let b = p.ret();
        p.u(b, call_arg(0).inv());
        let c = p.membar(Membar::VC);
        p.u(c, PT);
        let ne = p.isetp_u32(IComp::NE);
        let ne = out(p.uses(ne, &[PT, PT, thread_kill_value, RZ]));
        let d = p.ret();
        p.u(d, ne);
        let flo = p.flo_u32();
        let flo = out(p.uses(flo, &[PT, vote_result]));
        let eq = p.isetp_u32(IComp::EQ);
        let eq = out(p.uses(eq, &[PT, PT, flo, lane_id_value]));
        let x2 = p.xmad(XmadCop::CFULL, false, true, false, true);
        let x2 = out(p.uses(x2, &[PT, increment_value, popc_result, RZ]));
        let x3 = p.xmad(XmadCop::CFULL, false, false, false, false);
        let x3 = out(p.uses(x3, &[PT, increment_value, popc_result, RZ]));
        let x1 = p.xmad(XmadCop::CBCC, true, false, true, true);
        let x1 = out(p.uses(x1, &[PT, increment_value, x2, x3]));
        let e = p.red_e(RedOp::ADD, AtomSize::U32);
        p.uses(e, &[eq, address_low_value, x1]);
        let f = p.ret();
        p.u(f, PT);
        vec![a, b, c, d, e, f]
    };

    PatternSet { p, fsi_get_address, fsi_get_address_v2, fsi_is_last_warp_thread, fsi_begin, fsi_end }
}

fn matches_tree(p: &Patterns, ptree: &[usize], c: &Tree, external_regs: &[u8; 4]) -> bool {
    if ptree.len() != c.nodes.len() {
        return false;
    }
    ptree.iter().zip(&c.nodes).all(|(&pn, &cn)| matches_node(p, pn, c, cn, external_regs))
}

fn matches_node(p: &Patterns, pn: usize, c: &Tree, cn: usize, external_regs: &[u8; 4]) -> bool {
    let pnode = &p.arena[pn];
    let cnode = &c.arena[cn];
    let op_matches = cnode.op.name == pnode.name && (pnode.matcher)(cnode.op.raw_op_code, &pnode.args);
    if !op_matches || pnode.ty != cnode.ty || pnode.order != cnode.order || pnode.is_imm != cnode.op.props.has(InstProps::IB) {
        return false;
    }
    if pnode.ty == NodeType::Op {
        if pnode.uses.len() != cnode.uses.len() {
            return false;
        }
        for (pu, cu) in pnode.uses.iter().zip(&cnode.uses) {
            if pu.index <= -2 {
                if external_regs[(-pu.index - 2) as usize] as i32 != -cu.index - 2 {
                    return false;
                }
            } else if pu.index != cu.index {
                return false;
            }
            if pu.inverted != cu.inverted || pu.node.is_none() != cu.node.is_none() {
                return false;
            }
            if let (Some(pn2), Some(cn2)) = (pu.node, cu.node) {
                if !matches_node(p, pn2, c, cn2, external_regs) {
                    return false;
                }
            }
        }
    }
    true
}

fn matches_fsi(
    ps: &PatternSet,
    pattern: &[usize],
    program: &mut DecodedProgram,
    function: FunctionIdx,
    function_tree: &Tree,
    external_regs: &mut [u8; 4],
) -> bool {
    let blocks = &program.function(function).blocks;
    if blocks.is_empty() {
        return false;
    }
    let call_op = program.block(blocks[0]).last_op();
    if call_op.name != InstName::Cal {
        return false;
    }
    let Some(call_target) = program.function_by_address(call_op.absolute_address()) else { return false };
    let call_target_tree = build_tree(program, &program.function(call_target).blocks);
    if !matches_tree(&ps.p, &ps.fsi_is_last_warp_thread, &call_target_tree, external_regs) {
        return false;
    }
    external_regs[0] = call_target_tree.node(0).pd();
    if matches_tree(&ps.p, pattern, function_tree, external_regs) {
        program.remove_caller(call_target, function);
        return true;
    }
    false
}

pub fn run_pass(program: &mut DecodedProgram) {
    let ps = build_patterns();
    let mut external_regs = [0u8; 4];
    let mut has_get_address = false;
    let functions: Vec<FunctionIdx> = program.function_indices().collect();

    for &function in &functions {
        if function == program.main_function {
            continue;
        }
        let mut external_reg4 = 0u8;
        let tree = build_tree(program, &program.function(function).blocks);
        if matches_tree(&ps.p, &ps.fsi_get_address, &tree, &external_regs) {
            external_regs[1] = tree.node(0).rd();
            external_regs[2] = tree.node(2).rd();
            external_regs[3] = tree.node(1).rd();
            external_reg4 = tree.node(3).rd();
        } else if matches_tree(&ps.p, &ps.fsi_get_address_v2, &tree, &external_regs) {
            external_regs[1] = tree.node(2).rd();
            external_regs[2] = tree.node(1).rd();
            external_regs[3] = tree.node(0).rd();
            external_reg4 = tree.node(3).rd();
        }
        // A valid register allocation is a match.
        let r = &external_regs;
        if r[1] != r[2]
            && r[2] != r[3]
            && r[1] != r[3]
            && r[1] as i32 + 1 != r[2] as i32
            && r[1] as i32 + 1 != r[3] as i32
            && r[1] as i32 + 1 == external_reg4 as i32
            && r[2] as i32 != REGISTER_ZERO_INDEX
            && r[3] as i32 != REGISTER_ZERO_INDEX
            && external_reg4 as i32 != REGISTER_ZERO_INDEX
        {
            has_get_address = true;
            program.function_mut(function).ty = FunctionType::Unused;
            break;
        }
    }

    for &function in &functions {
        if program.function(function).is_compiler_generated() || function == program.main_function {
            continue;
        }
        if has_get_address {
            let tree = build_tree(program, &program.function(function).blocks);
            if matches_fsi(&ps, &ps.fsi_begin, program, function, &tree, &mut external_regs) {
                program.function_mut(function).ty = FunctionType::BuiltInFsiBegin;
                continue;
            } else if matches_fsi(&ps, &ps.fsi_end, program, function, &tree, &mut external_regs) {
                program.function_mut(function).ty = FunctionType::BuiltInFsiEnd;
                continue;
            }
        }
    }
}
