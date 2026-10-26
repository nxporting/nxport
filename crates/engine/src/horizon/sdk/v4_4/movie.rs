use super::super::Entry;

pub(super) static TABLE: &[Entry] = &[
    // The movie library's Decoder constructor before it took a trailing int (MK8D's subsdk0).
    Entry(
        "_ZN5movie7DecoderC1ENS_11DecoderTypeENS_19DecoderOutputFormatENS_11DecoderModeE",
        adapt!(fn(this: *mut usize, ty: i32, format: i32, mode: i32) {
            crate::horizon::movie::decoder_ctor(this, ty, format, mode, 0)
        }),
    ),
];
