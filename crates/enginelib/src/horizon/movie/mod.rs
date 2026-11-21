// HLE of the game's horizon::movie:: library (subsdk0): Buffer, MediaData, Extractor and Decoder,
// backed by crates/medialib (its demuxers and decoders are built into the program). The native library decodes through
// multimedia sysmodules and the hardware video decoder, neither of which runs here.
//
// Object layouts: Buffer is plain data, MediaData / Extractor / Decoder are a pointer
// to an implementation (plus a vtable for the last two). Vtables follow declaration order
// after the Itanium ABI's two destructor entries. The decoder follows the MediaCodec-style decoder contract:
// input / output buffer indices, BufferFlags_EndOfStream, and nn::os events signaled when buffers become available.

use nxport_medialib::Format;
use std::collections::{HashMap, VecDeque};
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::sync::OnceLock;

type AnyFn = unsafe extern "C" fn();
macro_rules! f {
    ($x:expr) => {
        unsafe { std::mem::transmute::<*const (), AnyFn>($x as *const ()) }
    };
}
/// A vtable of engine functions handed to the guest, and the function that records their HLE adapters (hle_abi; a
/// static's initialiser cannot). Entries are the functions with their parameters as placeholders: `x_seek(_, _)`.
macro_rules! guest_vtable {
    ($table:ident, $register:ident, [$($f:ident($($a:tt),*)),* $(,)?]) => {
        pub(crate) static $table: [AnyFn; [$(stringify!($f)),*].len()] = [$(f!($f as unsafe extern "C" fn($($a),*) -> _)),*];
        pub(crate) fn $register() {
            $(crate::hle!($f($($a),*));)*
        }
    };
}

mod files;
mod buffer;
mod media_data;
mod extractor;
mod decoder;
pub(crate) use files::*;
pub(crate) use buffer::*;
pub(crate) use media_data::*;
pub(crate) use extractor::*;
pub(crate) use decoder::*;

// horizon::movie::Status
const OK: u32 = 0;
#[allow(dead_code)]
const ERR_UNKNOWN: u32 = 0x1000;
const ERR_INVALID_RANGE: u32 = 0x1001;
const ERR_BUFFER_TOO_SMALL: u32 = 0x1005;
const ERR_NOT_SUPPORTED: u32 = 0x1006;
const ERR_IO: u32 = 0x1009;
const OUTPUT_NOT_AVAILABLE: u32 = 0x100e;
const INPUT_NOT_AVAILABLE: u32 = 0x1011;
const END_OF_STREAM: u32 = 0x2000;
const TRACK_NOT_FOUND: u32 = 0x3000;
const NULL_DECODER: u32 = 0x2004;
const NULL_EXTRACTOR: u32 = 0x3003;
const BUFFER_FLAG_EOS: u32 = 4; // BufferFlags_EndOfStream

unsafe extern "C" fn thread_priority() -> i32 {
    16
} // nn::os default priority
unsafe extern "C" fn noop() {}

/// Mangled name -> HLE function for the horizon::movie:: symbols (chained by horizon::sdk::lookup).
pub fn lookup(name: &str) -> Option<usize> {
    macro_rules! a {
        ($f:ident($($a:tt),*)) => {
            Some(crate::hle!($f($($a),*)) as usize)
        };
    }
    match name {
        "_ZN5movie9ExtractorC1ENS_13ContainerTypeE"
        | "_ZN5movie9ExtractorC1ENS_13ContainerTypeENS_9CacheSizeE" => a!(extractor_ctor(_)),
        "_ZN5movie7DecoderC1ENS_11DecoderTypeENS_19DecoderOutputFormatENS_11DecoderModeEi" => a!(decoder_ctor(_, _, _, _, _)),
        "_ZN5movie7DecoderC1ENS_11DecoderTypeENS_19DecoderOutputFormatE" => a!(decoder_ctor_before_modes(_, _, _)),
        "_ZN5movie9MediaDataC1Ev" => a!(md_ctor(_)),
        "_ZN5movie9MediaDataD1Ev" => a!(md_dtor(_)),
        "_ZN5movie9MediaDataC1ERKS0_" => a!(md_copy_ctor(_, _)),
        "_ZN5movie9MediaDataaSERKS0_" => a!(md_assign(_, _)),
        "_ZN5movie9MediaData5ClearEv" => a!(md_clear(_)),
        "_ZN5movie9MediaData8SetInt32EPKci" => a!(md_set_i32(_, _, _)),
        "_ZN5movie9MediaData8SetInt64EPKcl" => a!(md_set_i64(_, _, _)),
        "_ZN5movie9MediaData9SetStringEPKcS2_" => a!(md_set_string(_, _, _)),
        "_ZNK5movie9MediaData8ContainsEPKc" => a!(md_contains(_, _)),
        "_ZN5movie9MediaData7SetRectEPKciiii" => a!(md_set_rect(_, _, _, _, _, _)),
        "_ZNK5movie9MediaData8FindRectEPKcPiS3_S3_S3_" => a!(md_find_rect(_, _, _, _, _, _)),
        "_ZNK5movie9MediaData9FindInt32EPKcPi" => a!(md_find_i32(_, _, _)),
        "_ZNK5movie9MediaData9FindInt64EPKcPl" => a!(md_find_i64(_, _, _)),
        "_ZNK5movie9MediaData10FindStringEPKcPS2_" => a!(md_find_string(_, _, _)),
        "_ZNK5movie9MediaData12CountEntriesEv" => a!(md_count(_)),
        "_ZN5movie6BufferC1Ev" => a!(buffer_ctor(_)),
        "_ZN5movie6BufferC1Em" => a!(buffer_ctor_capacity(_, _)),
        "_ZN5movie6BufferC1EPvm" => a!(buffer_ctor_data(_, _, _)),
        "_ZN5movie6BufferC1ERKS0_" => a!(buffer_ctor_copy(_, _)),
        "_ZN5movie6BufferaSERKS0_" => a!(buffer_assign(_, _)),
        "_ZN5movie6BufferD1Ev" => a!(buffer_dtor(_)),
        "_ZNK5movie6Buffer4DataEv" => a!(buffer_data(_)),
        "_ZNK5movie6Buffer4BaseEv" => a!(buffer_base(_)),
        "_ZNK5movie6Buffer8CapacityEv" => a!(buffer_capacity(_)),
        "_ZNK5movie6Buffer6OffsetEv" => a!(buffer_offset(_)),
        "_ZNK5movie6Buffer4SizeEv" => a!(buffer_size(_)),
        "_ZN5movie6Buffer18SetDataAndCapacityEPvm" => a!(buffer_set_data_and_capacity(_, _, _)),
        "_ZN5movie6Buffer11SetCapacityEm" => a!(buffer_set_capacity(_, _)),
        "_ZN5movie6Buffer8SetRangeEii" => a!(buffer_set_range(_, _, _)),
        "_ZN5movie6Buffer12SetInt32DataEi" => a!(buffer_set_i32(_, _)),
        "_ZNK5movie6Buffer12GetInt32DataEv" => a!(buffer_get_i32(_)),
        "_ZN5movie6Buffer12SetInt64DataEl" => a!(buffer_set_i64(_, _)),
        "_ZNK5movie6Buffer12GetInt64DataEv" => a!(buffer_get_i64(_)),
        "_ZN5movie6Buffer13SetUint32DataEj" => a!(buffer_set_u32(_, _)),
        "_ZNK5movie6Buffer13GetUint32DataEv" => a!(buffer_get_u32(_)),
        "_ZN5movie6Buffer9SetNvnDevEPv" => a!(buffer_set_nvn_dev(_, _)),
        "_ZN5movie6Buffer10SetNvnSyncEPv" => a!(buffer_set_nvn_sync(_, _)),
        "_ZN5movie17GetThreadPriorityEv" => a!(thread_priority()),
        "_ZN5movie12SetAllocatorEPFPvmmS0_EPFvS0_S0_EPFS0_S0_mS0_ES0_"
        | "_ZN5movie11SetCoreMaskEm" => a!(noop()),
        _ => None,
    }
}

#[cfg(test)]
mod format_tests {
    use super::*;

    #[test]
    fn each_library_numbering_has_its_own_meaning() {
        assert_eq!(output_format(3), Format::Rgba);
        assert_eq!(output_format(4), Format::Nv12);
        assert_eq!(output_format_before_modes(2), Format::Argb);
        assert_eq!(output_format_before_modes(3), Format::Nv12);
    }
}
