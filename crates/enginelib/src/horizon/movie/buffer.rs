// horizon::movie::Buffer.

use super::*;

// ── horizon::movie::Buffer (private members) ─────────────────────────────────────────────────────────────────────
// The NVN device and sync members came with a later library version: 4.4.0's Buffer is 0x30 bytes (its constructor
// zeroes 0x0..0x30), 7.3.2's, 11.4.0's and 15.3.1's are 0x40. Writing the newer layout into an older game's Buffer
// overwrote the 16 bytes after it on the caller's stack (Mario Kart 8 Deluxe: a saved register holding a multi-wait
// holder, so its movie player never recognised "output available").
#[repr(C)]
pub(crate) struct Buffer {
    pub(crate) range_offset: i32,
    pub(crate) range_length: i32,
    pub(crate) int32_data: i32,
    pub(crate) uint32_data: u32,
    pub(crate) owns_data: bool,
    pub(crate) capacity: usize,
    pub(crate) int64_data: i64,
    pub(crate) data: *mut u8,
    pub(crate) nvn_device: usize,
    pub(crate) nvn_sync: usize,
}

/// The game's movie library version (its subsdk module's middleware string).
pub(crate) fn library_version() -> Option<(u32, u32, u32)> {
    static V: OnceLock<Option<(u32, u32, u32)>> = OnceLock::new();
    *V.get_or_init(|| {
        let dir = crate::settings::get().files.game_dir.clone()?;
        let exefs = ["ExeFS", "exefs"].iter().map(|d| dir.join(d)).find(|d| d.is_dir())?;
        let v = (0..10).find_map(|i| {
            let nso = crate::guest::nso::Nso::read(&exefs.join(format!("subsdk{i}"))).ok()?;
            crate::guest::nso::middleware_version(&nso.ro.data, b"NintendoSDK_movie-")
        });
        if let Some((a, b, c)) = v {
            println!("[+] movie: library {a}.{b}.{c}");
        }
        v
    })
}

/// Whether the game's Buffer has the NVN device and sync members (an unknown version: the newest layout).
pub(crate) fn buffer_has_nvn() -> bool {
    library_version().map_or(true, |v| v >= (7, 3, 0))
}

pub(crate) unsafe fn buf_init(b: *mut Buffer, data: *mut u8, capacity: usize, owns: bool) {
    let n = if buffer_has_nvn() { std::mem::size_of::<Buffer>() } else { std::mem::offset_of!(Buffer, nvn_device) };
    let v = Buffer {
        range_offset: 0,
        range_length: 0,
        int32_data: 0,
        uint32_data: 0,
        owns_data: owns,
        capacity,
        int64_data: 0,
        data,
        nvn_device: 0,
        nvn_sync: 0,
    };
    std::ptr::copy_nonoverlapping(&v as *const Buffer as *const u8, b as *mut u8, n);
}
pub(crate) unsafe fn alloc_bytes(n: usize) -> *mut u8 {
    if n == 0 {
        std::ptr::null_mut()
    } else {
        libc::calloc(1, n) as *mut u8
    }
}
pub(crate) unsafe extern "C" fn buffer_ctor(b: *mut Buffer) {
    buf_init(b, std::ptr::null_mut(), 0, false)
}
pub(crate) unsafe extern "C" fn buffer_ctor_capacity(b: *mut Buffer, cap: usize) {
    buf_init(b, alloc_bytes(cap), cap, true)
}
pub(crate) unsafe extern "C" fn buffer_ctor_data(b: *mut Buffer, data: *mut u8, cap: usize) {
    buf_init(b, data, cap, false)
}
pub(crate) unsafe extern "C" fn buffer_ctor_copy(b: *mut Buffer, o: *const Buffer) {
    buffer_ctor(b);
    buffer_assign(b, o);
}
pub(crate) unsafe extern "C" fn buffer_assign(b: *mut Buffer, o: *const Buffer) -> *mut Buffer {
    if b as *const Buffer == o {
        return b;
    }
    let (b_, o_) = (&mut *b, &*o);
    if b_.owns_data {
        libc::free(b_.data as *mut c_void);
    }
    // A deep copy when the source owns its data, otherwise the same (caller-owned) memory.
    if o_.owns_data {
        b_.data = alloc_bytes(o_.capacity);
        if !b_.data.is_null() && !o_.data.is_null() {
            std::ptr::copy_nonoverlapping(o_.data, b_.data, o_.capacity);
        }
        b_.owns_data = true;
    } else {
        b_.data = o_.data;
        b_.owns_data = false;
    }
    b_.capacity = o_.capacity;
    b_.range_offset = o_.range_offset;
    b_.range_length = o_.range_length;
    b_.int32_data = o_.int32_data;
    b_.uint32_data = o_.uint32_data;
    b_.int64_data = o_.int64_data;
    if buffer_has_nvn() {
        b_.nvn_device = o_.nvn_device;
        b_.nvn_sync = o_.nvn_sync;
    }
    b
}
pub(crate) unsafe extern "C" fn buffer_dtor(b: *mut Buffer) {
    if (*b).owns_data {
        libc::free((*b).data as *mut c_void);
        (*b).data = std::ptr::null_mut();
        (*b).owns_data = false;
    }
}
pub(crate) unsafe extern "C" fn buffer_data(b: *const Buffer) -> *mut u8 {
    if (*b).data.is_null() {
        std::ptr::null_mut()
    } else {
        (*b).data.add((*b).range_offset.max(0) as usize)
    }
}
pub(crate) unsafe extern "C" fn buffer_base(b: *const Buffer) -> *mut u8 {
    (*b).data
}
pub(crate) unsafe extern "C" fn buffer_capacity(b: *const Buffer) -> usize {
    (*b).capacity
}
pub(crate) unsafe extern "C" fn buffer_offset(b: *const Buffer) -> i32 {
    (*b).range_offset
}
pub(crate) unsafe extern "C" fn buffer_size(b: *const Buffer) -> i32 {
    (*b).range_length
}
pub(crate) unsafe extern "C" fn buffer_set_data_and_capacity(b: *mut Buffer, data: *mut u8, cap: usize) {
    buffer_dtor(b);
    (*b).data = data;
    (*b).capacity = cap;
}
pub(crate) unsafe extern "C" fn buffer_set_capacity(b: *mut Buffer, cap: usize) {
    (*b).capacity = cap
}
pub(crate) unsafe extern "C" fn buffer_set_range(b: *mut Buffer, off: i32, size: i32) {
    (*b).range_offset = off;
    (*b).range_length = size;
}
pub(crate) unsafe extern "C" fn buffer_set_i32(b: *mut Buffer, v: i32) {
    (*b).int32_data = v
}
pub(crate) unsafe extern "C" fn buffer_get_i32(b: *const Buffer) -> i32 {
    (*b).int32_data
}
pub(crate) unsafe extern "C" fn buffer_set_i64(b: *mut Buffer, v: i64) {
    (*b).int64_data = v
}
pub(crate) unsafe extern "C" fn buffer_get_i64(b: *const Buffer) -> i64 {
    (*b).int64_data
}
pub(crate) unsafe extern "C" fn buffer_set_u32(b: *mut Buffer, v: u32) {
    (*b).uint32_data = v
}
pub(crate) unsafe extern "C" fn buffer_get_u32(b: *const Buffer) -> u32 {
    (*b).uint32_data
}
/// SetNvnDev / SetNvnSync(void*): the NVN device and sync a hardware-decoding player hands its buffers (Super Mario
/// Party Jamboree's player calls them; nothing here decodes through them, they are kept as the library keeps them).
pub(crate) unsafe extern "C" fn buffer_set_nvn_dev(b: *mut Buffer, dev: usize) {
    if buffer_has_nvn() {
        (*b).nvn_device = dev
    }
}
pub(crate) unsafe extern "C" fn buffer_set_nvn_sync(b: *mut Buffer, sync: usize) {
    if buffer_has_nvn() {
        (*b).nvn_sync = sync
    }
}
