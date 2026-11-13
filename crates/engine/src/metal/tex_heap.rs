// Descriptor heaps for stages built with --texture-heap (index line `th`). Such a stage reads each sampled texture of
// the bound texture table from a buffer of gpuResourceIDs at the index in the low 20 bits of its handle, and the
// sampler from the sampler heap at the bits above; the heap addresses and the handles are in the driver cbuf
// (sass2spv TEXTURE_HEAP_*). One texture heap per MTLTextureType value (the shader declares the type it reads).
//
// Each Metal texture or sampler state gets a slot the first time a draw uses it and keeps it while it is in use, so an
// entry is written once and never changes under GPU work that reads it. The heap retains what its slots name (a pointer
// cannot be reused for another object while it has a slot); slots unused for IDLE_FRAMES are freed. Growing a heap makes
// a larger copy; the old buffer stays alive for draws already encoded with its address.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_metal::{MTLBuffer, MTLDevice, MTLResourceOptions, MTLSamplerState, MTLTexture};
use rustc_hash::FxHashMap;

use super::Tex;

/// Heap index of the sampler heap (after the texture heaps 0..=8).
pub(super) const SAMPLERS: usize = sass2spv::accessor::TEXTURE_HEAP_SAMPLERS as usize;
pub(super) const HEAPS: usize = SAMPLERS + 1;
/// Driver cbuf byte offsets (sass2spv TEXTURE_HEAP_ADDRESSES / TEXTURE_HEAP_HANDLES).
pub(super) const ADDRESSES: usize = sass2spv::accessor::TEXTURE_HEAP_ADDRESSES as usize;
pub(super) const HANDLES: usize = sass2spv::accessor::TEXTURE_HEAP_HANDLES as usize;
/// Slots a handle can address: 20 bits of texture, 12 of sampler.
const LIMIT: [u32; 2] = [1 << 20, 1 << 12];
/// Frames after its last use before a slot (or a replaced heap buffer) is released: well past any GPU work using it.
const IDLE_FRAMES: u64 = 16;

enum Obj {
    /// The texture and the texture it is a view of (itself if none): residency and hazard tracking of resources reached
    /// only through useResource follow the resource given, so views are made resident through their root.
    Tex(#[allow(dead_code)] Tex, Tex),
    Sampler(#[allow(dead_code)] Retained<ProtocolObject<dyn MTLSamplerState>>),
}

#[derive(Default)]
struct Heap {
    buf: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
    /// What each slot names (None: free), the frame it was last used in.
    objs: Vec<Option<Obj>>,
    used: Vec<u64>,
    free: Vec<u32>,
    by_ptr: FxHashMap<usize, u32>,
}

#[derive(Default)]
pub(crate) struct TexHeaps {
    heaps: [Heap; HEAPS],
    /// Replaced heap buffers, with the frame they were replaced in.
    retired: Vec<(u64, Retained<ProtocolObject<dyn MTLBuffer>>)>,
    swept: u64,
}

impl TexHeaps {
    /// The slot of texture `t` in the heap of type `ty` (MTLTextureType value), and the texture to make resident.
    pub(super) unsafe fn texture(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        ty: usize,
        t: &Tex,
        frame: u64,
    ) -> Option<(u32, Tex)> {
        let ptr = &**t as *const ProtocolObject<dyn MTLTexture> as *const u8 as usize;
        let h = ty.min(SAMPLERS - 1);
        let s = self.slot(device, h, ptr, frame, || {
            let mut root = t.clone();
            while let Some(p) = root.parentTexture() {
                root = p;
            }
            (std::mem::transmute::<_, u64>(t.gpuResourceID()), Obj::Tex(t.clone(), root))
        })?;
        match &self.heaps[h].objs[s as usize] {
            Some(Obj::Tex(_, root)) => Some((s, root.clone())),
            _ => None,
        }
    }

    pub(super) unsafe fn sampler(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        s: &Retained<ProtocolObject<dyn MTLSamplerState>>,
        frame: u64,
    ) -> Option<u32> {
        let ptr = &**s as *const ProtocolObject<dyn MTLSamplerState> as *const u8 as usize;
        self.slot(device, SAMPLERS, ptr, frame, || {
            (std::mem::transmute::<_, u64>(s.gpuResourceID()), Obj::Sampler(s.clone()))
        })
    }

    unsafe fn slot(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
        h: usize,
        ptr: usize,
        frame: u64,
        make: impl FnOnce() -> (u64, Obj),
    ) -> Option<u32> {
        let heap = &mut self.heaps[h];
        if let Some(&s) = heap.by_ptr.get(&ptr) {
            heap.used[s as usize] = frame;
            return Some(s);
        }
        let s = match heap.free.pop() {
            Some(s) => s,
            None => {
                let s = heap.objs.len() as u32;
                if s >= LIMIT[(h == SAMPLERS) as usize] {
                    return None;
                }
                heap.objs.push(None);
                heap.used.push(0);
                s
            }
        };
        let cap = heap.buf.as_ref().map_or(0, |b| b.length() / 8);
        if s as usize >= cap {
            let new_cap = (cap * 2).max(1024);
            let nb = device.newBufferWithLength_options(new_cap * 8, MTLResourceOptions::empty())?;
            if let Some(old) = heap.buf.take() {
                std::ptr::copy_nonoverlapping(
                    old.contents().as_ptr() as *const u8,
                    nb.contents().as_ptr() as *mut u8,
                    cap * 8,
                );
                self.retired.push((frame, old));
            }
            heap.buf = Some(nb);
        }
        let (id, obj) = make();
        let buf = heap.buf.as_ref().unwrap();
        *(buf.contents().as_ptr() as *mut u64).add(s as usize) = id;
        heap.objs[s as usize] = Some(obj);
        heap.used[s as usize] = frame;
        heap.by_ptr.insert(ptr, s);
        Some(s)
    }

    /// GPU address of heap `h` (0 before its first slot).
    pub(super) fn address(&self, h: usize) -> u64 {
        self.heaps[h].buf.as_ref().map_or(0, |b| b.gpuAddress())
    }

    pub(super) fn buffer(&self, h: usize) -> Option<&Retained<ProtocolObject<dyn MTLBuffer>>> {
        self.heaps[h].buf.as_ref()
    }

    /// Releases slots and replaced buffers unused for IDLE_FRAMES (checked every IDLE_FRAMES frames).
    pub(super) fn sweep(&mut self, frame: u64) {
        if frame < self.swept + IDLE_FRAMES {
            return;
        }
        self.swept = frame;
        let old = frame.saturating_sub(IDLE_FRAMES);
        self.retired.retain(|(f, _)| *f >= old);
        for heap in &mut self.heaps {
            let Heap { objs, used, free, by_ptr, .. } = heap;
            by_ptr.retain(|_, s| {
                let s = *s as usize;
                if used[s] < old {
                    objs[s] = None;
                    free.push(s as u32);
                    false
                } else {
                    true
                }
            });
        }
    }
}
