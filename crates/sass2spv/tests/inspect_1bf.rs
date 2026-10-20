
use sass2spv::decoders::decoder::decode;
use sass2spv::gpu_accessor::MemoryGpuAccessor;
use std::fs;

#[test]
fn test_inspect_1bf() {
    let bytes = fs::read("../../shader_dump_missing/1bfb79406e3a1096_s1.code").unwrap();
    let sass = &bytes[0x30..];
    let cb1 = MemoryGpuAccessor::find_const_bank(&bytes);
    let accessor = MemoryGpuAccessor::new(sass, false, Some(&bytes), cb1);
    let program = decode(&accessor, 0x50, false);
    for (i, b) in program.blocks.iter().enumerate() {
        println!("Block {}: 0x{:x} - 0x{:x}, ops={}, succ={:?}, pred={:?}", i, b.address, b.end_address, b.op_codes.len(), b.successors, b.predecessors);
        for op in &b.op_codes {
            println!("   {:?} (addr=0x{:x})", op.name, op.address);
        }
    }
}
