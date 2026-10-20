use byteorder::{ByteOrder, LittleEndian};

pub struct SetBindingPair {
    pub set: u32,
    pub binding: u32,
}

pub struct MemoryGpuAccessor {
    code: Vec<u64>,
    is_compute: bool,
    stage: Vec<u8>,
    cb1_offset: i32,

    pub local_size_x: u32,
    pub local_size_y: u32,
    pub local_size_z: u32,
    pub shared_memory_size: u32,
    pub local_memory_size: u32,

    next_texture: u32,
    next_image: u32,
    next_compute_buffer: u32,
}

impl MemoryGpuAccessor {
    pub const STORAGE_BUFFER_BASE: u32 = 18;
    pub const IMAGE_BASE: u32 = 96;
    pub const DRIVER_CBUF_BINDING: u32 = 17;

    pub fn new(sass_bytes: &[u8], is_compute: bool, stage: Option<&[u8]>, cb1_offset: i32) -> Self {
        let u64_count = sass_bytes.len() / 8;
        let mut code = Vec::with_capacity(u64_count);
        for chunk in sass_bytes[..u64_count * 8].chunks_exact(8) {
            code.push(LittleEndian::read_u64(chunk));
        }

        Self {
            code,
            is_compute,
            stage: stage.map(|s| s.to_vec()).unwrap_or_default(),
            cb1_offset,
            local_size_x: 1,
            local_size_y: 1,
            local_size_z: 1,
            shared_memory_size: 0,
            local_memory_size: 0,
            next_texture: 0,
            next_image: 0,
            next_compute_buffer: 1,
        }
    }

    /// Offset of the constant bank in NVN stage data when the GLSLC control section is not available: the program ends
    /// with a branch-to-self followed by NOP padding to a 32-byte group; the bank starts at the next 0x100 boundary,
    /// after a zero gap. Returns -1 when that shape is not found.
    pub fn find_const_bank(stage: &[u8]) -> i32 {
        const BRA_SELF: u64 = 0xe2400fffff07000f;
        const NOP: u64 = 0x50b0000000070f00;
        const CODE_START: usize = 0x80;

        let mut q = CODE_START;
        while q + 8 <= stage.len() {
            let val = LittleEndian::read_u64(&stage[q..q + 8]);
            if (val & 0xffffffffff00ffff) != (BRA_SELF & 0xffffffffff00ffff) {
                q += 8;
                continue;
            }

            let mut g = CODE_START + ((q + 8 - CODE_START + 0x1f) / 0x20) * 0x20;
            while g + 0x20 <= stage.len() {
                let mut all_zero = true;
                let mut padding = true;
                for w in 0..4 {
                    let v = LittleEndian::read_u64(&stage[g + w * 8..g + (w + 1) * 8]);
                    if v != 0 {
                        all_zero = false;
                    }
                    if w > 0 && v != NOP && v != 0 {
                        padding = false;
                    }
                }
                if all_zero || !padding {
                    break;
                }
                g += 0x20;
            }

            let bank = (g + 0xff) & !0xff;
            let mut gap_zero = true;
            for i in g..bank.min(stage.len()) {
                if stage[i] != 0 {
                    gap_zero = false;
                    break;
                }
            }
            if gap_zero && bank < stage.len() {
                return bank as i32;
            }
            q += 8;
        }
        -1
    }

    pub fn get_code(&self, address: u64) -> &[u64] {
        let index = (address / 8) as usize;
        if index >= self.code.len() {
            &[]
        } else {
            &self.code[index..]
        }
    }

    pub fn constant_buffer1_read(&self, offset: i32) -> u32 {
        let at = (self.cb1_offset + offset) as usize;
        if self.cb1_offset < 0 || offset < 0 || at + 4 > self.stage.len() {
            return 0;
        }
        LittleEndian::read_u32(&self.stage[at..at + 4])
    }

    pub fn create_constant_buffer_binding(&mut self, index: i32) -> SetBindingPair {
        if self.is_compute {
            let binding = self.next_compute_buffer;
            self.next_compute_buffer += 1;
            SetBindingPair { set: 0, binding }
        } else {
            let binding = if index == 0 {
                Self::DRIVER_CBUF_BINDING
            } else {
                index as u32
            };
            SetBindingPair { set: 0, binding }
        }
    }

    pub fn create_image_binding(&mut self, count: u32) -> SetBindingPair {
        let binding = Self::IMAGE_BASE + self.next_image;
        self.next_image += count;
        SetBindingPair { set: 1, binding }
    }

    pub fn create_storage_buffer_binding(&mut self, index: i32) -> SetBindingPair {
        if self.is_compute {
            let binding = self.next_compute_buffer;
            self.next_compute_buffer += 1;
            SetBindingPair { set: 2, binding }
        } else {
            SetBindingPair {
                set: 2,
                binding: Self::STORAGE_BUFFER_BASE + index as u32,
            }
        }
    }

    pub fn create_texture_binding(&mut self, count: u32) -> SetBindingPair {
        let binding = self.next_texture;
        self.next_texture += count;
        SetBindingPair { set: 3, binding }
    }
}
