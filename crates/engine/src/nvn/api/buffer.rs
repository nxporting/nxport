// NVN buffers.

use super::*;

// ---------------------------------------------------------------------------
// NVN Buffer family. NVNbuffer is 0x30 (48) bytes, NVNbufferBuilder is 0x40
// (64) bytes -- both opaque, same freedom as MemoryPool/CommandBuffer. This
// was the single highest-volume unimplemented call this session
// (nvnBufferGetAddress: 4365 calls, always returning 0) -- almost certainly
// why guest code elsewhere was dereferencing a null "buffer address" field.
#[repr(C)]
pub(crate) struct NvnBufferBuilder {
    device: usize,
    pool: usize, // *const NvnMemoryPool
    offset: i64,
    size: usize,
}

#[repr(C)]
pub(crate) struct NvnBuffer {
    address: u64, // our internal "GPU address" convention -- same host pointer value used throughout (see MemoryPoolGetBufferAddress)
    map_ptr: usize,
    size: usize,
    pool: usize,
    offset: i64,
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderSetDefaults(b: *mut NvnBufferBuilder) {
    record_hle_call("hle_nvnBufferBuilderSetDefaults");
    if !b.is_null() {
        std::ptr::write_bytes(b, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderSetDevice(b: *mut NvnBufferBuilder, device: usize) {
    record_hle_call("hle_nvnBufferBuilderSetDevice");
    if !b.is_null() {
        (*b).device = device;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderSetStorage(
    b: *mut NvnBufferBuilder,
    pool: usize,
    offset: i64,
    size: usize,
) {
    record_hle_call("hle_nvnBufferBuilderSetStorage");
    if !b.is_null() {
        (*b).pool = pool;
        (*b).offset = offset;
        (*b).size = size;
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetSize(b: *const NvnBufferBuilder) -> usize {
    record_hle_call("hle_nvnBufferBuilderGetSize");
    if b.is_null() {
        0
    } else {
        (*b).size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetMemoryOffset(b: *const NvnBufferBuilder) -> i64 {
    record_hle_call("hle_nvnBufferBuilderGetMemoryOffset");
    if b.is_null() {
        0
    } else {
        (*b).offset
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetMemoryPool(b: *const NvnBufferBuilder) -> usize {
    record_hle_call("hle_nvnBufferBuilderGetMemoryPool");
    // returns the NVNmemoryPool* passed to SetStorage (returning the struct by value made the caller read x0
    // while we wrote through x8).
    if b.is_null() {
        0
    } else {
        (*b).pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferInitialize(
    buf: *mut NvnBuffer,
    builder: *const NvnBufferBuilder,
) -> u8 {
    record_hle_call("hle_nvnBufferInitialize");
    if buf.is_null() || builder.is_null() || (*builder).pool == 0 {
        return 0;
    }
    let pool = &*((*builder).pool as *const NvnMemoryPool);
    let ptr = translate_virtual(pool.memory + (*builder).offset as usize);
    std::ptr::write(
        buf,
        NvnBuffer {
            address: ptr as u64,
            map_ptr: ptr,
            size: (*builder).size,
            pool: (*builder).pool,
            offset: (*builder).offset,
        },
    );
    1
}

/// The address nvnBufferInitialize gives a buffer with storage (`pool`, `offset`).
pub unsafe fn buffer_address(pool: usize, offset: u64) -> u64 {
    if pool == 0 {
        return 0;
    }
    translate_virtual((*(pool as *const NvnMemoryPool)).memory + offset as usize) as u64
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferFinalize(buf: *mut NvnBuffer) {
    record_hle_call("hle_nvnBufferFinalize");
    if !buf.is_null() {
        std::ptr::write_bytes(buf, 0, 1);
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetAddress(buf: *const NvnBuffer) -> u64 {
    record_hle_call("hle_nvnBufferGetAddress");
    if buf.is_null() {
        0
    } else {
        (*buf).address
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferMap(buf: *const NvnBuffer) -> usize {
    record_hle_call("hle_nvnBufferMap");
    if buf.is_null() {
        0
    } else {
        (*buf).map_ptr
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetSize(buf: *const NvnBuffer) -> usize {
    record_hle_call("hle_nvnBufferGetSize");
    if buf.is_null() {
        0
    } else {
        (*buf).size
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferFlushMappedRange(
    _buf: *const NvnBuffer,
    _offset: i64,
    _size: i64,
) {
    record_hle_call("hle_nvnBufferFlushMappedRange");
    // Unified memory architecture -- correct no-op, see MemoryPoolFlushMappedRange.
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferInvalidateMappedRange(
    _buf: *const NvnBuffer,
    _offset: i64,
    _size: i64,
) {
    record_hle_call("hle_nvnBufferInvalidateMappedRange");
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferBuilderGetDevice(b: *const NvnBufferBuilder) -> usize {
    if b.is_null() {
        0
    } else {
        (*b).device
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetMemoryPool(buf: *const NvnBuffer) -> usize {
    if buf.is_null() {
        0
    } else {
        (*buf).pool
    }
}

#[no_mangle]
pub unsafe extern "C" fn hle_nvnBufferGetMemoryOffset(buf: *const NvnBuffer) -> i64 {
    if buf.is_null() {
        0
    } else {
        (*buf).offset
    }
}
