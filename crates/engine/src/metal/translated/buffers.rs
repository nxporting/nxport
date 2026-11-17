// Guest memory as Metal buffers: pool and guest buffers (read-only, read-write, zero-copy), the zero buffer and
// constant banks.

use super::*;

impl Gfx {
    /// A Metal buffer + offset covering `[addr, addr + len)` of guest memory without copying it.
    pub(crate) unsafe fn pool_buffer(
        &self,
        sh: &mut SharedCaches,
        addr: u64,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        let addr = crate::nvn::api::translate_virtual(addr as usize);
        // Per-thread memo by exact (address, length), valid while horizon::hle::pools_gen() is unchanged (pool registrations,
        // virtual mappings, releases and unmaps bump it, and the slow path below handles releases first): the same
        // static vertex / index ranges are looked up every frame, and each lookup took the release queue, the global
        // wrapper table and the pool registry. --no-pool-buffer-memo: off.
        thread_local! {
            static MEMO: std::cell::RefCell<(u64, HashMap<(usize, usize), (Retained<ProtocolObject<dyn MTLBuffer>>, usize)>)> =
                std::cell::RefCell::new((0, HashMap::default()));
        }
        let gen = crate::nvn::api::pools_gen();
        {
            let hit = MEMO.with(|m| {
                let m = m.borrow();
                if m.0 == gen {
                    m.1.get(&(addr, len)).cloned()
                } else {
                    None
                }
            });
            if hit.is_some() {
                return hit;
            }
        }
        let r = self.pool_buffer_slow(sh, addr, len);
        {
            if let Some(x) = &r {
                MEMO.with(|m| {
                    let mut m = m.borrow_mut();
                    if m.0 != gen || m.1.len() > 1 << 16 {
                        m.1.clear();
                        m.0 = gen;
                    }
                    m.1.insert((addr, len), x.clone());
                });
            }
        }
        r
    }

    /// pool_buffer without the memo (`addr` already translated).
    pub(crate) unsafe fn pool_buffer_slow(
        &self,
        sh: &mut SharedCaches,
        addr: usize,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        // One wrapper per pool for every thread: Metal tracks hazards per buffer object, so a draw reading memory through
        // another wrapper than the compute pass that wrote it would not wait for it; and take_released_ranges hands each
        // release to one caller only.
        let _ = sh;
        let released = crate::nvn::api::take_released_ranges();
        drop_released_pool_buffers(&released);
        let mut pool_buffers = POOL_BUFFERS.lock().unwrap();
        let pool_buffers = &mut pool_buffers
            .get_or_insert_with(|| PoolBuffers(HashMap::default()))
            .0;
        let (mut base, mut size) = crate::nvn::api::find_memory_pool(addr)?;
        if addr + len > base + size {
            // A pool nested inside a larger one covers the start only. Without one, the binding's size runs past its pool
            // (games bind a size covering more than the shader reaches): wrap the pool it starts in, as the GPU would only
            // reach that pool's memory. Copying instead lost every GPU write to the buffer.
            if let Some(p) = crate::nvn::api::containing_pool(addr, len) {
                (base, size) = p;
            }
        }
        let wrap_base = base & !(VM_PAGE - 1);
        let wrap_end = (base + size + VM_PAGE - 1) & !(VM_PAGE - 1);
        // Keyed by the exact wrapped range: pools come and go, and several small pools can share a page, so a buffer
        // made for an earlier pool at the same page is not valid for this one.
        if let Some(b) = pool_buffers.get(&(wrap_base, wrap_end)) {
            let off = addr - wrap_base;
            if off + len <= b.length() {
                return Some((b.clone(), off));
            }
        }
        // Metal refuses a no-copy buffer over inaccessible pages; buffers made before a page is protected keep working.
        // --tex-backing holds pages under copied textures, which share pools with buffers: open them first.
        unsafe { super::tex_backing::open_overlapping(wrap_base, wrap_end - wrap_base) };
        // Held render targets (metal/rt_guard.rs) likewise, for the moment of the wrap.
        let Some(buf) = super::rt_guard::with_open(wrap_base, wrap_end - wrap_base, || {
            self.device.newBufferWithBytesNoCopy_length_options_deallocator(
                std::ptr::NonNull::new(wrap_base as *mut libc::c_void)?,
                wrap_end - wrap_base,
                MTLResourceOptions::empty(),
                None,
            )
        }) else {
            static WARNED: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());
            let mut w = WARNED.lock().unwrap_or_else(|e| e.into_inner());
            if !w.contains(&wrap_base) {
                w.push(wrap_base);
                println!("[!] Gfx: Metal refused a zero-copy buffer over pool 0x{:x}..0x{:x} ({} MiB; device max buffer {} MiB)",
                    wrap_base, wrap_end, (wrap_end - wrap_base) >> 20, self.device.maxBufferLength() >> 20);
            }
            return None;
        };
        // Pools whose page-rounded ranges overlap (several small pools on one page) each keep their own buffer: Metal
        // allows no-copy buffers over the same memory. Evicting overlapping buffers here made two such pools re-wrap each
        // other on every bind (millions of buffer creations per session). Buffers over freed memory are dropped through
        // take_released_ranges above.
        pool_buffers.insert((wrap_base, wrap_end), buf.clone());
        if pool_buffers.len() % 256 == 1 {
            println!(
                "[+] Gfx: wrapped memory pool 0x{:x}..0x{:x} as a zero-copy buffer ({} wrapped)",
                wrap_base,
                wrap_end,
                pool_buffers.len()
            );
        }
        let off = addr - wrap_base;
        if off + len > buf.length() {
            return None;
        }
        Some((buf, off))
    }

    /// Buffer for guest memory: zero-copy from a pool when possible, else a copy in the arena.
    pub(crate) unsafe fn guest_buffer(
        &self,
        sh: &mut SharedCaches,
        cx: &mut EncodeCtx,
        addr: u64,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        // The GPU may write this memory and its writes cannot fault: stop write-tracking it.
        let host = crate::nvn::api::translate_virtual(addr as usize);
        crate::guest::dirty_track::release_for_gpu(host, len);
        super::fence_guard::note_write(host, len);
        self.guest_buffer_rw(sh, cx, addr, len, true)
    }

    /// guest_buffer for memory the GPU only reads (vertex, index and uniform buffers, copy sources): its pages may stay
    /// write-tracked. (Releasing them on every bind walked every page of every vertex buffer per draw.)
    pub(crate) unsafe fn guest_buffer_ro(
        &self,
        sh: &mut SharedCaches,
        cx: &mut EncodeCtx,
        addr: u64,
        len: usize,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        self.guest_buffer_rw(sh, cx, addr, len, false)
    }

    pub(crate) unsafe fn guest_buffer_rw(
        &self,
        sh: &mut SharedCaches,
        cx: &mut EncodeCtx,
        addr: u64,
        len: usize,
        gpu_writes: bool,
    ) -> Option<(Retained<ProtocolObject<dyn MTLBuffer>>, usize)> {
        let addr = crate::nvn::api::translate_virtual(addr as usize) as u64;
        {
            if let Some(r) = self.pool_buffer(sh, addr, len) {
                // The GPU reads guest memory itself: the early-fence guard keeps the game from overwriting it meanwhile.
                if !gpu_writes {
                    super::fence_guard::note_read(addr as usize, len);
                }
                return Some(r);
            }
        }
        let limit = ARENA_CHUNK / 2;
        if addr < 0x10000 || len > limit {
            if addr >= 0x10000 {
                println!("[!] Gfx: {} bytes at 0x{:x} are outside every memory pool and too large to copy", len, addr);
            }
            return None;
        }
        cx.arena.alloc(
            &self.device,
            std::slice::from_raw_parts(addr as *const u8, len),
        )
    }

    /// A zeroed 64KB buffer bound for storage buffers that cannot be resolved, so shaders never read through a null
    /// buffer binding (which faults the GPU).
    pub(crate) fn zero_buffer(
        &self,
        sh: &mut SharedCaches,
    ) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        if let Some(b) = &sh.zero_buffer {
            return Some(b.clone());
        }
        let b = self
            .device
            .newBufferWithLength_options(0x10000, MTLResourceOptions::empty())?;
        sh.zero_buffer = Some(b.clone());
        Some(b)
    }

    /// cbuf 1: the stage's compiler-generated constants. Their offset (from the start of the stage data) and size
    /// are in the GLSLC control section at +0x700 / +0x6fc (confirmed on every captured TotK stage); without a
    /// control section, fall back to the end of the decoded code aligned to 0x100, which is wrong whenever the
    /// decoder stops before the program's trailing instructions.
    pub(crate) unsafe fn const_bank(
        &self,
        sh: &mut SharedCaches,
        stage_data: u64,
        control: u64,
        stage: &CachedStage,
    ) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        let key = (stage_data, stage as *const CachedStage as usize); // no String clone per draw
        if let Some(b) = sh.const_banks.get(&key) {
            return Some(b.clone());
        }
        let (off, size) = match control_const_bank(control) {
            Some(x) => x,
            None => ((stage.code_end + 0xFF) & !0xFF, CONST_BANK_SIZE),
        };
        let bytes = std::slice::from_raw_parts(
            (stage_data as usize + off) as *const u8,
            size.min(CBUF_DECL_SIZE),
        );
        let mut padded = bytes.to_vec();
        padded.resize(CBUF_DECL_SIZE, 0);
        let buf = self.device.newBufferWithBytes_length_options(
            std::ptr::NonNull::new(padded.as_mut_ptr() as *mut libc::c_void)?,
            padded.len(),
            MTLResourceOptions::empty(),
        )?;
        sh.const_banks.insert(key, buf.clone());
        Some(buf)
    }
}
