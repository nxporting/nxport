// Generated from Ryujinx.Graphics.Shader/Decoders/InstTable.cs - do not edit

use std::sync::OnceLock;
use super::inst_emitter::InstEmitter;
use super::inst_name::InstName;
use super::inst_op::InstOp;
use super::inst_props::InstProps;

const ENCODING_BITS: usize = 14;
const TABLE_SIZE: usize = 1 << ENCODING_BITS;

#[derive(Debug, Clone, Copy)]
pub struct TableEntry {
    pub name: InstName,
    pub emitter: Option<InstEmitter>,
    pub props: InstProps,
    pub x_bits: usize,
}

impl Default for TableEntry {
    fn default() -> Self {
        Self {
            name: InstName::Invalid,
            emitter: None,
            props: InstProps::NONE,
            x_bits: usize::MAX,
        }
    }
}

fn build_table() -> Box<[TableEntry; TABLE_SIZE]> {
    let mut table = Box::new([TableEntry::default(); TABLE_SIZE]);

    let mut add = |encoding: &str, name: InstName, emitter: InstEmitter, props: InstProps| {
        let enc_part = &encoding[..ENCODING_BITS];
        let mut bit = enc_part.len() - 1;
        let mut value = 0usize;
        let mut x_mask = 0usize;
        let mut x_bits = 0usize;
        let mut x_pos = [0usize; ENCODING_BITS];

        for chr in enc_part.chars() {
            if chr == '1' {
                value |= 1 << bit;
            } else if chr == 'x' {
                x_mask |= 1 << bit;
                x_pos[x_bits] = bit;
                x_bits += 1;
            }
            bit = bit.wrapping_sub(1);
        }

        x_mask = !x_mask;
        let entry = TableEntry {
            name,
            emitter: Some(emitter),
            props,
            x_bits,
        };

        for index in 0..(1 << x_bits) {
            let mut val = value & x_mask;
            for x in 0..x_bits {
                val |= ((index >> x) & 1) << x_pos[x];
            }
            if table[val].emitter.is_none() || table[val].x_bits > x_bits {
                table[val] = entry;
            }
        }
    };

    add("1110111110100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Al2p, InstEmitter::Al2p, InstProps::RD.union(InstProps::RA));
    add("1110111111011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ald, InstEmitter::Ald, InstProps::RD.union(InstProps::RA));
    add("1110111111110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ast, InstEmitter::Ast, InstProps::RA.union(InstProps::RB2.union(InstProps::RC)));
    add("11101101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Atom, InstEmitter::Atom, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("111011101111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::AtomCas, InstEmitter::AtomCas, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("11101100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Atoms, InstEmitter::Atoms, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("111011100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::AtomsCas, InstEmitter::AtomsCas, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("1111000010111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::B2r, InstEmitter::B2r, InstProps::RD.union(InstProps::RA.union(InstProps::VPD)));
    add("1111000010101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bar, InstEmitter::Bar, InstProps::RA.union(InstProps::PS));
    add("0101110000000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bfe, InstEmitter::BfeR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x00000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bfe, InstEmitter::BfeI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110000000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bfe, InstEmitter::BfeC, InstProps::RD.union(InstProps::RA));
    add("0101101111110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bfi, InstEmitter::BfiR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011011x11110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bfi, InstEmitter::BfiI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("0100101111110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bfi, InstEmitter::BfiC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("0101001111110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bfi, InstEmitter::BfiRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("111000111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bpt, InstEmitter::Bpt, InstProps::NO_PRED);
    add("111000100100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Bra, InstEmitter::Bra, InstProps::BRA);
    add("111000110100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Brk, InstEmitter::Brk, InstProps::BRA);
    add("111000100101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Brx, InstEmitter::Brx, InstProps::RA.union(InstProps::BRA));
    add("111000100110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cal, InstEmitter::Cal, InstProps::BRA.union(InstProps::NO_PRED));
    add("11101111011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cctl, InstEmitter::Cctl, InstProps::RA);
    add("1110111110000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cctll, InstEmitter::Cctll, InstProps::RA);
    add("1110101111110xx0000000000000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cctlt, InstEmitter::Cctlt, InstProps::NONE);
    add("1110101111101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cctlt, InstEmitter::Cctlt, InstProps::RC);
    add("111000110101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cont, InstEmitter::Cont, InstProps::BRA);
    add("0101000010011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cset, InstEmitter::Cset, InstProps::RD.union(InstProps::PS));
    add("0101000010100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Csetp, InstEmitter::Csetp, InstProps::PD.union(InstProps::PDN.union(InstProps::PS)));
    add("0101000011001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Cs2r, InstEmitter::Cs2r, InstProps::RD);
    add("0101110001110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dadd, InstEmitter::DaddR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x01110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dadd, InstEmitter::DaddI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110001110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dadd, InstEmitter::DaddC, InstProps::RD.union(InstProps::RA));
    add("1111000011110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Depbar, InstEmitter::Depbar, InstProps::NONE);
    add("010110110111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dfma, InstEmitter::DfmaR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011011x0111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dfma, InstEmitter::DfmaI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010010110111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dfma, InstEmitter::DfmaC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100110111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dfma, InstEmitter::DfmaRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("0101110001010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dmnmx, InstEmitter::DmnmxR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0011100x01010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dmnmx, InstEmitter::DmnmxI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("0100110001010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dmnmx, InstEmitter::DmnmxC, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("0101110010000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dmul, InstEmitter::DmulR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x10000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dmul, InstEmitter::DmulI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110010000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dmul, InstEmitter::DmulC, InstProps::RD.union(InstProps::RA));
    add("010110010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dset, InstEmitter::DsetR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0011001x0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dset, InstEmitter::DsetI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("010010010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dset, InstEmitter::DsetC, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("010110111000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dsetp, InstEmitter::DsetpR, InstProps::RA.union(InstProps::RB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("0011011x1000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dsetp, InstEmitter::DsetpI, InstProps::RA.union(InstProps::IB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("010010111000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Dsetp, InstEmitter::DsetpC, InstProps::RA.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS))));
    add("111000110000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Exit, InstEmitter::Exit, InstProps::BRA);
    add("0101110010101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::F2f, InstEmitter::F2fR, InstProps::RD.union(InstProps::RB));
    add("0011100x10101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::F2f, InstEmitter::F2fI, InstProps::RD.union(InstProps::IB));
    add("0100110010101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::F2f, InstEmitter::F2fC, InstProps::RD);
    add("0101110010110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::F2i, InstEmitter::F2iR, InstProps::RD.union(InstProps::RB));
    add("0011100x10110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::F2i, InstEmitter::F2iI, InstProps::RD.union(InstProps::IB));
    add("0100110010110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::F2i, InstEmitter::F2iC, InstProps::RD);
    add("0101110001011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fadd, InstEmitter::FaddR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x01011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fadd, InstEmitter::FaddI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110001011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fadd, InstEmitter::FaddC, InstProps::RD.union(InstProps::RA));
    add("000010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fadd32i, InstEmitter::Fadd32i, InstProps::RD.union(InstProps::RA));
    add("0101110010001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fchk, InstEmitter::FchkR, InstProps::RA.union(InstProps::RB.union(InstProps::PD)));
    add("0011100x10001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fchk, InstEmitter::FchkI, InstProps::RA.union(InstProps::IB.union(InstProps::PD)));
    add("0100110010001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fchk, InstEmitter::FchkC, InstProps::RA.union(InstProps::PD));
    add("010110111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fcmp, InstEmitter::FcmpR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011011x1010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fcmp, InstEmitter::FcmpI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010010111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fcmp, InstEmitter::FcmpC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fcmp, InstEmitter::FcmpRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010110011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ffma, InstEmitter::FfmaR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011001x1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ffma, InstEmitter::FfmaI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010010011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ffma, InstEmitter::FfmaC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ffma, InstEmitter::FfmaRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("000011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ffma32i, InstEmitter::Ffma32i, InstProps::RD.union(InstProps::RA));
    add("0101110000110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Flo, InstEmitter::FloR, InstProps::RD.union(InstProps::RB));
    add("0011100x00110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Flo, InstEmitter::FloI, InstProps::RD.union(InstProps::IB));
    add("0100110000110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Flo, InstEmitter::FloC, InstProps::RD);
    add("0101110001100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fmnmx, InstEmitter::FmnmxR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0011100x01100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fmnmx, InstEmitter::FmnmxI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("0100110001100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fmnmx, InstEmitter::FmnmxC, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("0101110001101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fmul, InstEmitter::FmulR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x01101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fmul, InstEmitter::FmulI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110001101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fmul, InstEmitter::FmulC, InstProps::RD.union(InstProps::RA));
    add("00011110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fmul32i, InstEmitter::Fmul32i, InstProps::RD.union(InstProps::RA));
    add("01011000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fset, InstEmitter::FsetR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0011000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fset, InstEmitter::FsetI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("01001000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fset, InstEmitter::FsetC, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("010110111011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fsetp, InstEmitter::FsetpR, InstProps::RA.union(InstProps::RB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("0011011x1011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fsetp, InstEmitter::FsetpI, InstProps::RA.union(InstProps::IB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("010010111011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fsetp, InstEmitter::FsetpC, InstProps::RA.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS))));
    add("0101000011111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Fswzadd, InstEmitter::Fswzadd, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("111000101100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Getcrsptr, InstEmitter::Getcrsptr, InstProps::RD.union(InstProps::NO_PRED));
    add("111000101101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Getlmembase, InstEmitter::Getlmembase, InstProps::RD.union(InstProps::NO_PRED));
    add("0101110100010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hadd2, InstEmitter::Hadd2R, InstProps::RD.union(InstProps::RA));
    add("0111101x0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hadd2, InstEmitter::Hadd2I, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0111101x1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hadd2, InstEmitter::Hadd2C, InstProps::RD.union(InstProps::RA));
    add("0010110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hadd232i, InstEmitter::Hadd232i, InstProps::RD.union(InstProps::RA));
    add("0101110100000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hfma2, InstEmitter::Hfma2R, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("01110xxx0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hfma2, InstEmitter::Hfma2I, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("01110xxx1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hfma2, InstEmitter::Hfma2C, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("01100xxx1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hfma2, InstEmitter::Hfma2Rc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("0010100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hfma2, InstEmitter::Hfma232i, InstProps::RD.union(InstProps::RA));
    add("0101110100001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hmul2, InstEmitter::Hmul2R, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0111100x0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hmul2, InstEmitter::Hmul2I, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0111100x1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hmul2, InstEmitter::Hmul2C, InstProps::RD.union(InstProps::RA));
    add("0010101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hmul232i, InstEmitter::Hmul232i, InstProps::RD.union(InstProps::RA));
    add("0101110100011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hset2, InstEmitter::Hset2R, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0111110x0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hset2, InstEmitter::Hset2I, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("0111110x1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hset2, InstEmitter::Hset2C, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("0101110100100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hsetp2, InstEmitter::Hsetp2R, InstProps::RA.union(InstProps::RB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("0111111x0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hsetp2, InstEmitter::Hsetp2I, InstProps::RA.union(InstProps::IB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("0111111x1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Hsetp2, InstEmitter::Hsetp2C, InstProps::RA.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS))));
    add("0101110010111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::I2f, InstEmitter::I2fR, InstProps::RD.union(InstProps::RB));
    add("0011100x10111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::I2f, InstEmitter::I2fI, InstProps::RD.union(InstProps::IB));
    add("0100110010111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::I2f, InstEmitter::I2fC, InstProps::RD);
    add("0101110011100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::I2i, InstEmitter::I2iR, InstProps::RD.union(InstProps::RB));
    add("0011100x11100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::I2i, InstEmitter::I2iI, InstProps::RD.union(InstProps::IB));
    add("0100110011100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::I2i, InstEmitter::I2iC, InstProps::RD);
    add("0101110000010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iadd, InstEmitter::IaddR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x00010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iadd, InstEmitter::IaddI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110000010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iadd, InstEmitter::IaddC, InstProps::RD.union(InstProps::RA));
    add("0001110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iadd32i, InstEmitter::Iadd32i, InstProps::RD.union(InstProps::RA));
    add("010111001100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iadd3, InstEmitter::Iadd3R, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011100x1100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iadd3, InstEmitter::Iadd3I, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010011001100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iadd3, InstEmitter::Iadd3C, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010110110100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Icmp, InstEmitter::IcmpR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011011x0100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Icmp, InstEmitter::IcmpI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010010110100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Icmp, InstEmitter::IcmpC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100110100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Icmp, InstEmitter::IcmpRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("111000111001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ide, InstEmitter::Ide, InstProps::NO_PRED);
    add("0101001111111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Idp, InstEmitter::IdpR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0101001111011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Idp, InstEmitter::IdpC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010110100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imad, InstEmitter::ImadR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011010x0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imad, InstEmitter::ImadI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010010100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imad, InstEmitter::ImadC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imad, InstEmitter::ImadRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("000100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imad32i, InstEmitter::Imad32i, InstProps::RD.union(InstProps::RA));
    add("010110101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imadsp, InstEmitter::ImadspR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011010x1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imadsp, InstEmitter::ImadspI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010010101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imadsp, InstEmitter::ImadspC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imadsp, InstEmitter::ImadspRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("0101110000100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imnmx, InstEmitter::ImnmxR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0011100x00100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imnmx, InstEmitter::ImnmxI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("0100110000100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imnmx, InstEmitter::ImnmxC, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("0101110000111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imul, InstEmitter::ImulR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x00111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imul, InstEmitter::ImulI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110000111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imul, InstEmitter::ImulC, InstProps::RD.union(InstProps::RA));
    add("00011111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Imul32i, InstEmitter::Imul32i, InstProps::RD.union(InstProps::RA));
    add("11100000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ipa, InstEmitter::Ipa, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("1110111111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Isberd, InstEmitter::Isberd, InstProps::RD.union(InstProps::RA));
    add("0101110000011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iscadd, InstEmitter::IscaddR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x00011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iscadd, InstEmitter::IscaddI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110000011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iscadd, InstEmitter::IscaddC, InstProps::RD.union(InstProps::RA));
    add("000101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iscadd32i, InstEmitter::Iscadd32i, InstProps::RD.union(InstProps::RA));
    add("010110110101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iset, InstEmitter::IsetR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0011011x0101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iset, InstEmitter::IsetI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("010010110101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Iset, InstEmitter::IsetC, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("010110110110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Isetp, InstEmitter::IsetpR, InstProps::RA.union(InstProps::RB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("0011011x0110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Isetp, InstEmitter::IsetpI, InstProps::RA.union(InstProps::IB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("010010110110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Isetp, InstEmitter::IsetpC, InstProps::RA.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS))));
    add("111000100010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Jcal, InstEmitter::Jcal, InstProps::BRA);
    add("111000100001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Jmp, InstEmitter::Jmp, InstProps::RA.union(InstProps::BRA));
    add("111000100000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Jmx, InstEmitter::Jmx, InstProps::RA.union(InstProps::BRA));
    add("111000110011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Kil, InstEmitter::Kil, InstProps::BRA);
    add("100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ld, InstEmitter::Ld, InstProps::RD.union(InstProps::RA));
    add("1110111110010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ldc, InstEmitter::Ldc, InstProps::RD.union(InstProps::RA));
    add("1110111011010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ldg, InstEmitter::Ldg, InstProps::RD.union(InstProps::RA));
    add("1110111101000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ldl, InstEmitter::Ldl, InstProps::RD.union(InstProps::RA));
    add("1110111101001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lds, InstEmitter::Lds, InstProps::RD.union(InstProps::RA));
    add("0101101111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lea, InstEmitter::LeaR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::LPD))));
    add("0011011x11010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lea, InstEmitter::LeaI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::LPD))));
    add("0100101111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lea, InstEmitter::LeaC, InstProps::RD.union(InstProps::RA.union(InstProps::LPD)));
    add("0101101111011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::LeaHi, InstEmitter::LeaHiR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC.union(InstProps::LPD)))));
    add("000110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::LeaHi, InstEmitter::LeaHiC, InstProps::RD.union(InstProps::RA.union(InstProps::RC.union(InstProps::LPD))));
    add("0101000011010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lepc, InstEmitter::Lepc, InstProps::NONE);
    add("111000110001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Longjmp, InstEmitter::Longjmp, InstProps::BRA);
    add("0101110001000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lop, InstEmitter::LopR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::LPD))));
    add("0011100x01000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lop, InstEmitter::LopI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::LPD))));
    add("0100110001000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lop, InstEmitter::LopC, InstProps::RD.union(InstProps::RA.union(InstProps::LPD)));
    add("0101101111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lop3, InstEmitter::Lop3R, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC.union(InstProps::LPD)))));
    add("001111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lop3, InstEmitter::Lop3I, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("0000001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lop3, InstEmitter::Lop3C, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("000001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Lop32i, InstEmitter::Lop32i, InstProps::RD.union(InstProps::RA));
    add("1110111110011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Membar, InstEmitter::Membar, InstProps::NONE);
    add("0101110010011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Mov, InstEmitter::MovR, InstProps::RD.union(InstProps::RA));
    add("0011100x10011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Mov, InstEmitter::MovI, InstProps::RD.union(InstProps::IB));
    add("0100110010011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Mov, InstEmitter::MovC, InstProps::RD);
    add("000000010000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Mov32i, InstEmitter::Mov32i, InstProps::RD);
    add("0101000010000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Mufu, InstEmitter::Mufu, InstProps::RD.union(InstProps::RA));
    add("0101000010110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Nop, InstEmitter::Nop, InstProps::NONE);
    add("1111101111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Out, InstEmitter::OutR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("1111011x11100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Out, InstEmitter::OutI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("1110101111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Out, InstEmitter::OutC, InstProps::RD.union(InstProps::RA));
    add("0101110011101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::P2r, InstEmitter::P2rR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x11101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::P2r, InstEmitter::P2rI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110011101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::P2r, InstEmitter::P2rC, InstProps::RD.union(InstProps::RA));
    add("111000101010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Pbk, InstEmitter::Pbk, InstProps::NO_PRED);
    add("111000101011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Pcnt, InstEmitter::Pcnt, InstProps::NO_PRED);
    add("111000100011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Pexit, InstEmitter::Pexit, InstProps::NONE);
    add("1110111111101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Pixld, InstEmitter::Pixld, InstProps::RD.union(InstProps::RA.union(InstProps::VPD)));
    add("111000101000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Plongjmp, InstEmitter::Plongjmp, InstProps::BRA.union(InstProps::NO_PRED));
    add("0101110000001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Popc, InstEmitter::PopcR, InstProps::RD.union(InstProps::RB));
    add("0011100x00001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Popc, InstEmitter::PopcI, InstProps::RD.union(InstProps::IB));
    add("0100110000001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Popc, InstEmitter::PopcC, InstProps::RD);
    add("111000100111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Pret, InstEmitter::Pret, InstProps::NO_PRED);
    add("010110111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Prmt, InstEmitter::PrmtR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011011x1100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Prmt, InstEmitter::PrmtI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("010010111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Prmt, InstEmitter::PrmtC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Prmt, InstEmitter::PrmtRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("0101000010001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Pset, InstEmitter::Pset, InstProps::RD.union(InstProps::PS));
    add("0101000010010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Psetp, InstEmitter::Psetp, InstProps::PD.union(InstProps::PDN.union(InstProps::PS)));
    add("1111000011000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::R2b, InstEmitter::R2b, InstProps::RB);
    add("0101110011110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::R2p, InstEmitter::R2pR, InstProps::RA.union(InstProps::RB));
    add("0011100x11110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::R2p, InstEmitter::R2pI, InstProps::RA.union(InstProps::IB));
    add("0100110011110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::R2p, InstEmitter::R2pC, InstProps::RA);
    add("111000111000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ram, InstEmitter::Ram, InstProps::NO_PRED);
    add("1110101111111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Red, InstEmitter::Red, InstProps::RA.union(InstProps::RB2));
    add("111000110010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ret, InstEmitter::Ret, InstProps::BRA);
    add("0101110010010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Rro, InstEmitter::RroR, InstProps::RD.union(InstProps::RB));
    add("0011100x10010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Rro, InstEmitter::RroI, InstProps::RD.union(InstProps::IB));
    add("0100110010010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Rro, InstEmitter::RroC, InstProps::RD);
    add("111000110110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Rtt, InstEmitter::Rtt, InstProps::NO_PRED);
    add("1111000011001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::S2r, InstEmitter::S2r, InstProps::RD);
    add("111000110111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sam, InstEmitter::Sam, InstProps::NO_PRED);
    add("0101110010100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sel, InstEmitter::SelR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::PS))));
    add("0011100x10100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sel, InstEmitter::SelI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::PS))));
    add("0100110010100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sel, InstEmitter::SelC, InstProps::RD.union(InstProps::RA.union(InstProps::PS)));
    add("111000101110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Setcrsptr, InstEmitter::Setcrsptr, InstProps::RA.union(InstProps::NO_PRED));
    add("111000101111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Setlmembase, InstEmitter::Setlmembase, InstProps::RA.union(InstProps::NO_PRED));
    add("0101101111111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shf, InstEmitter::ShfLR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0101110011111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shf, InstEmitter::ShfRR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011011x11111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shf, InstEmitter::ShfLI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("0011100x11111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shf, InstEmitter::ShfRI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("1110111100010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shfl, InstEmitter::Shfl, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC.union(InstProps::LPD)))));
    add("0101110001001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shl, InstEmitter::ShlR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x01001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shl, InstEmitter::ShlI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110001001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shl, InstEmitter::ShlC, InstProps::RD.union(InstProps::RA));
    add("0101110000101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shr, InstEmitter::ShrR, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("0011100x00101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shr, InstEmitter::ShrI, InstProps::RD.union(InstProps::RA.union(InstProps::IB)));
    add("0100110000101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Shr, InstEmitter::ShrC, InstProps::RD.union(InstProps::RA));
    add("111000101001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Ssy, InstEmitter::Ssy, InstProps::NO_PRED);
    add("101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::St, InstEmitter::St, InstProps::RD.union(InstProps::RA));
    add("1110111011011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Stg, InstEmitter::Stg, InstProps::RD.union(InstProps::RA));
    add("1110111101010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Stl, InstEmitter::Stl, InstProps::RD.union(InstProps::RA));
    add("1110111010100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Stp, InstEmitter::Stp, InstProps::NO_PRED);
    add("1110111101011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sts, InstEmitter::Sts, InstProps::RD.union(InstProps::RA));
    add("1110101001110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuatomB, InstEmitter::SuatomB, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("11101010x0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Suatom, InstEmitter::Suatom, InstProps::RD.union(InstProps::RA.union(InstProps::RB)));
    add("1110101110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuatomB2, InstEmitter::SuatomB2, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("1110101011010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuatomCasB, InstEmitter::SuatomCasB, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC.union(InstProps::SPD)))));
    add("1110101x1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuatomCas, InstEmitter::SuatomCas, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::SPD))));
    add("1110101100010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuldDB, InstEmitter::SuldDB, InstProps::RD.union(InstProps::RA.union(InstProps::RC.union(InstProps::SPD.union(InstProps::TEX_B)))));
    add("1110101100011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuldD, InstEmitter::SuldD, InstProps::RD.union(InstProps::RA.union(InstProps::SPD.union(InstProps::TEX))));
    add("1110101100000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuldB, InstEmitter::SuldB, InstProps::RD.union(InstProps::RA.union(InstProps::RC.union(InstProps::SPD.union(InstProps::TEX_B)))));
    add("1110101100001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Suld, InstEmitter::Suld, InstProps::RD.union(InstProps::RA.union(InstProps::SPD.union(InstProps::TEX))));
    add("1110101101010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SuredB, InstEmitter::SuredB, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("1110101101011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sured, InstEmitter::Sured, InstProps::RD.union(InstProps::RA));
    add("1110101100110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SustDB, InstEmitter::SustDB, InstProps::RD.union(InstProps::RA.union(InstProps::RC.union(InstProps::TEX_B))));
    add("1110101100111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SustD, InstEmitter::SustD, InstProps::RD.union(InstProps::RA.union(InstProps::TEX)));
    add("1110101100100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::SustB, InstEmitter::SustB, InstProps::RD.union(InstProps::RA.union(InstProps::RC.union(InstProps::TEX_B))));
    add("1110101100101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sust, InstEmitter::Sust, InstProps::RD.union(InstProps::RA.union(InstProps::TEX)));
    add("1111000011111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Sync, InstEmitter::Sync, InstProps::BRA);
    add("11000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tex, InstEmitter::Tex, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX)))));
    add("1101111010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::TexB, InstEmitter::TexB, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX_B)))));
    add("1101100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Texs, InstEmitter::Texs, InstProps::RD.union(InstProps::RD2.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX)))));
    add("1101000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::TexsF16, InstEmitter::TexsF16, InstProps::RD.union(InstProps::RD2.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX)))));
    add("11011100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tld, InstEmitter::Tld, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX)))));
    add("11011101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::TldB, InstEmitter::TldB, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX_B)))));
    add("1101101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tlds, InstEmitter::Tlds, InstProps::RD.union(InstProps::RD2.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX)))));
    add("1101001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::TldsF16, InstEmitter::TldsF16, InstProps::RD.union(InstProps::RD2.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX)))));
    add("110010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tld4, InstEmitter::Tld4, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX)))));
    add("1101111011xxxxxxxxxxxxx0xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tld4B, InstEmitter::Tld4B, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX_B)))));
    add("1101111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tld4s, InstEmitter::Tld4s, InstProps::RD.union(InstProps::RD2.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX)))));
    add("1101111110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tld4sF16, InstEmitter::Tld4sF16, InstProps::RD.union(InstProps::RD2.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX)))));
    add("1101111101011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Tmml, InstEmitter::Tmml, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX))));
    add("1101111101100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::TmmlB, InstEmitter::TmmlB, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TEX_B))));
    add("1101111101000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Txa, InstEmitter::Txa, InstProps::RD.union(InstProps::RA.union(InstProps::TEX)));
    add("110111100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Txd, InstEmitter::Txd, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX)))));
    add("1101111001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::TxdB, InstEmitter::TxdB, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::TPD.union(InstProps::TEX_B)))));
    add("1101111101001xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Txq, InstEmitter::Txq, InstProps::RD.union(InstProps::RA.union(InstProps::TEX)));
    add("1101111101010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::TxqB, InstEmitter::TxqB, InstProps::RD.union(InstProps::RA.union(InstProps::TEX_B)));
    add("01010100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vabsdiff, InstEmitter::Vabsdiff, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("010100000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vabsdiff4, InstEmitter::Vabsdiff4, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("001000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vadd, InstEmitter::Vadd, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("01011111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vmad, InstEmitter::Vmad, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011101xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vmnmx, InstEmitter::Vmnmx, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0101000011011xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vote, InstEmitter::Vote, InstProps::RD.union(InstProps::VPD.union(InstProps::PS)));
    add("0101000011100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Votevtg, InstEmitter::Votevtg, InstProps::NONE);
    add("0100000xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vset, InstEmitter::Vset, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0101000011110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vsetp, InstEmitter::Vsetp, InstProps::RA.union(InstProps::RB.union(InstProps::PD.union(InstProps::PDN.union(InstProps::PS)))));
    add("01010111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vshl, InstEmitter::Vshl, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("01010110xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Vshr, InstEmitter::Vshr, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0101101100xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Xmad, InstEmitter::XmadR, InstProps::RD.union(InstProps::RA.union(InstProps::RB.union(InstProps::RC))));
    add("0011011x00xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Xmad, InstEmitter::XmadI, InstProps::RD.union(InstProps::RA.union(InstProps::IB.union(InstProps::RC))));
    add("0100111xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Xmad, InstEmitter::XmadC, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));
    add("010100010xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx", InstName::Xmad, InstEmitter::XmadRc, InstProps::RD.union(InstProps::RA.union(InstProps::RC)));

    table
}

static OP_CODES: OnceLock<Box<[TableEntry; TABLE_SIZE]>> = OnceLock::new();

pub fn get_op(address: u64, op_code: u64) -> InstOp {
    let table = OP_CODES.get_or_init(build_table);
    let entry = &table[(op_code >> (64 - ENCODING_BITS)) as usize];
    if let Some(emitter) = entry.emitter {
        InstOp::new(address, op_code, entry.name, Some(emitter), entry.props)
    } else {
        InstOp::new(address, op_code, InstName::Invalid, None, InstProps::NONE)
    }
}
