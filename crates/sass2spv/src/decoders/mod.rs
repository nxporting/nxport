pub mod block;
pub mod decoder;
pub mod inst_decoders;
pub mod inst_emitter;
pub mod inst_name;
pub mod inst_op;
pub mod inst_props;
pub mod inst_table;
pub mod register;

pub use block::Block;
pub use decoder::{decode, DecodedProgram, FeatureFlags};
pub use inst_emitter::InstEmitter;
pub use inst_name::InstName;
pub use inst_op::InstOp;
pub use inst_props::InstProps;
pub use inst_table::get_op;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_bra_and_nop() {
        let bra_self = 0xe2400fffff07000f;
        let op_bra = get_op(0x100, bra_self);
        assert_eq!(op_bra.name, InstName::Bra);
        assert_eq!(op_bra.emitter, Some(InstEmitter::Bra));
        assert!(op_bra.props.contains(InstProps::BRA));

        let nop = 0x50b0000000070f00;
        let op_nop = get_op(0x108, nop);
        assert_eq!(op_nop.name, InstName::Nop);

        let decoded_bra = inst_decoders::InstBra::new(bra_self);
        assert_eq!(decoded_bra.pred(), 7);
        assert!(!decoded_bra.pred_inv());
    }

    #[test]
    fn test_decode_real_shader() {
        use byteorder::{ByteOrder, LittleEndian};
        let bytes = std::fs::read("../../shader_dump_missing/3461ed5d7d63fb45_s0.code").unwrap();
        // Graphics shader: 0x30 preamble, 0x50 SPH -> code at 0x80
        assert_eq!(LittleEndian::read_u32(&bytes[0..4]), 0x12345678);
        let mut addr = 0x80;
        let mut decoded_names = Vec::new();
        while addr + 8 <= bytes.len() && decoded_names.len() < 10 {
            if (addr & 0x1f) == 0 {
                addr += 8; // skip 8-byte control code
                continue;
            }
            let raw = LittleEndian::read_u64(&bytes[addr..addr + 8]);
            let op = get_op(addr as u64, raw);
            assert_ne!(op.name, InstName::Invalid, "Decoded invalid op at 0x{:x}", addr);
            decoded_names.push(op.name);
            addr += 8;
        }
        assert_eq!(decoded_names.len(), 10);
        println!("First 10 decoded instructions: {:?}", decoded_names);
    }

    #[test]
    fn test_decode_full_program() {
        use crate::gpu_accessor::MemoryGpuAccessor;
        let bytes = std::fs::read("../../shader_dump_missing/3461ed5d7d63fb45_s0.code").unwrap();
        let sass = &bytes[0x30..]; // SPH at 0x0, code at 0x50
        let cb1 = MemoryGpuAccessor::find_const_bank(&bytes);
        let accessor = MemoryGpuAccessor::new(sass, false, Some(&bytes), cb1);
        let program = decode(&accessor, 0x50, false);
        assert!(!program.blocks.is_empty());
        println!("Decoded {} basic blocks, max end address: 0x{:x}", program.blocks.len(), program.max_end_address);
        for (i, b) in program.blocks.iter().enumerate() {
            println!("  Block {}: 0x{:x} - 0x{:x} ({} ops)", i, b.address, b.end_address, b.op_codes.len());
        }
    }
}
