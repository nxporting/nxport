// A few Maxwell instructions and shader headers, enough for the synthetic capture scenarios' shaders. The encodings are
// sass2spv's decoder tables (from Ryujinx's); a shader binary is laid out as the game's are: a 0x30-byte preamble
// (magic 0x12345678), the 0x50-byte shader program header, then the code in groups of a scheduling word and three
// instructions.

const RZ: u64 = 255;
/// Predicate PT (always) in the instruction predicate field.
const PT: u64 = 7 << 16;

/// Generic attribute N's offset in the attribute space (vertex inputs and outputs, fragment inputs).
pub const fn attr(n: u64) -> u64 {
    0x80 + n * 0x10
}
/// The position output.
pub const POSITION: u64 = 0x70;

/// ALD.{32,64,96,128} Rd, a[offset]: loads `words` consecutive input attribute words into Rd..
pub fn ald(rd: u64, offset: u64, words: u64) -> u64 {
    (0x1DFB << 51) | ((words - 1) << 47) | (RZ << 39) | (offset << 20) | PT | (RZ << 8) | rd
}

/// AST.{32,64,96,128} a[offset], Rs: stores `words` consecutive registers from Rs to output attributes.
pub fn ast(offset: u64, rs: u64, words: u64) -> u64 {
    (0x1DFE << 51) | ((words - 1) << 47) | (RZ << 39) | (offset << 20) | PT | (RZ << 8) | rs
}

/// IPA.PASS Rd, a[offset]: an interpolated fragment input (as the header's imap interpolates it).
pub fn ipa(rd: u64, offset: u64) -> u64 {
    (0xE0 << 56) | (7 << 47) | (RZ << 39) | (offset << 28) | (RZ << 20) | PT | (RZ << 8) | rd
}

/// MOV32I Rd, imm.
pub fn mov32i(rd: u64, imm: u32) -> u64 {
    (0x010 << 52) | ((imm as u64) << 20) | (0xF << 12) | PT | rd
}

/// MOV32I Rd, f: a float constant.
pub fn movf(rd: u64, f: f32) -> u64 {
    mov32i(rd, f.to_bits())
}

/// FADD32I Rd, Ra, imm.
pub fn fadd32i(rd: u64, ra: u64, f: f32) -> u64 {
    (0x02 << 58) | ((f.to_bits() as u64) << 20) | PT | (ra << 8) | rd
}

/// TEXS Rd, Ra, Rb, tex[handle]: a 2D sample at (Ra, Rb), RGBA into Rd, Rd+1 and Rd2, Rd2+1. `handle` is the texture
/// handle's word offset in the bound texture bank (slot k: 8 + 2k).
pub fn texs_2d(rd: u64, rd2: u64, ra: u64, rb: u64, handle: u64) -> u64 {
    // Target 1 (2D), write mask 4 (RGBA with Rd2).
    (0b1101100 << 57) | (1 << 53) | (4 << 50) | (handle << 36) | (rd2 << 28) | (rb << 20) | PT | (ra << 8) | rd
}

/// EXIT.
pub fn exit() -> u64 {
    (0xE30 << 52) | PT | 0xF
}

/// Fragment input interpolation (header imap) per component: 2 bits each.
pub const IMAP_PERSPECTIVE: u8 = 0b10_10_10_10;

/// A vertex shader binary from its code.
pub fn vertex(code: &[u64]) -> Vec<u8> {
    let mut h = [0u32; 20];
    h[0] = 1 | (3 << 5) | (1 << 10);
    binary(&h, code)
}

/// A fragment shader binary: generic inputs `inputs` interpolated as `imap`, color targets per bit of `targets` (four
/// components each) written from R0.. at EXIT.
pub fn fragment(code: &[u64], inputs: &[(usize, u8)], targets: u32) -> Vec<u8> {
    let mut h = [0u32; 20];
    h[0] = 2 | (3 << 5) | (5 << 10) | (1 << 14);
    for &(i, imap) in inputs {
        h[6 + i / 4] |= (imap as u32) << ((i % 4) * 8);
    }
    h[18] = targets;
    binary(&h, code)
}

fn binary(header: &[u32; 20], code: &[u64]) -> Vec<u8> {
    let mut out = vec![0u8; 0x30];
    out[..4].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    for w in header {
        out.extend_from_slice(&w.to_le_bytes());
    }
    for group in code.chunks(3) {
        out.extend_from_slice(&0u64.to_le_bytes());
        for i in 0..3 {
            out.extend_from_slice(&group.get(i).copied().unwrap_or(0).to_le_bytes());
        }
    }
    // Room past the end: the decoder may read the next group.
    out.resize(out.len() + 0x80, 0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msl(bin: &[u8], hint: i32) -> String {
        let target = sass2spv::Target::Msl("main0".into());
        sass2spv::translate_stage_to(bin, hint, -1, None, &target).expect("translates").msl
    }

    #[test]
    fn passthrough_vertex() {
        let v = vertex(&[ald(0, attr(0), 4), ald(4, attr(1), 4), ast(POSITION, 0, 4), ast(attr(0), 4, 4), exit()]);
        let m = msl(&v, 2);
        println!("{m}");
        assert!(m.contains("vertex"), "{m}");
        assert!(m.contains("[[position"), "{m}");
        assert!(m.contains("[[attribute(1)]]"), "{m}");
    }

    #[test]
    fn varying_fragment() {
        let f = fragment(&[ipa(0, attr(0)), ipa(1, attr(0) + 4), ipa(2, attr(0) + 8), ipa(3, attr(0) + 12), exit()], &[(0, IMAP_PERSPECTIVE)], 0xF);
        let m = msl(&f, 1);
        println!("{m}");
        assert!(m.contains("fragment"), "{m}");
        assert!(m.contains("[[color(0)]]"), "{m}");
    }

    #[test]
    fn constant_fragment() {
        let f = fragment(&[movf(0, 1.0), movf(1, 0.5), movf(2, 0.25), movf(3, 1.0), exit()], &[], 0xF);
        let m = msl(&f, 1);
        println!("{m}");
        assert!(m.contains(&format!("{}u", 0.5f32.to_bits())), "{m}");
    }
}
