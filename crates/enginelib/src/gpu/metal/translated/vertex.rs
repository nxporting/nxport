// Vertex and index data of a draw: the highest index and vertex it fetches, vertex formats and attribute layouts.

use super::*;

/// Largest of `n` indices at `addr` that is not the restart index (None if there is none). Written as fixed-width
/// lanes with the restart value mapped to 0 so it vectorizes (this scan ran for most draws, ~8% of the encoding thread
/// as an iterator with a filter).
pub(crate) unsafe fn max_index(index_type: i32, addr: u64, n: usize, restart: Option<u32>) -> Option<u64> {
    fn lanes<T: Copy + Ord + Default + PartialEq, const L: usize>(v: &[T], r: Option<T>) -> T {
        let mut acc = [T::default(); L];
        let mut chunks = v.chunks_exact(L);
        match r {
            Some(r) => {
                for c in &mut chunks {
                    for k in 0..L {
                        let x = if c[k] == r { T::default() } else { c[k] };
                        acc[k] = acc[k].max(x);
                    }
                }
            }
            None => {
                for c in &mut chunks {
                    for k in 0..L {
                        acc[k] = acc[k].max(c[k]);
                    }
                }
            }
        }
        let mut m = acc.iter().copied().max().unwrap_or_default();
        for &x in chunks.remainder() {
            if Some(x) != r {
                m = m.max(x);
            }
        }
        m
    }
    if addr < 0x10000 || n == 0 {
        return None;
    }
    let (m, all_restart) = match index_type {
        0 => {
            let held = super::rt_guard::guest_copy::<u8>(addr as usize, n);
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u8, n));
            let r = restart.filter(|&r| r <= 0xFF).map(|r| r as u8);
            let m = lanes::<u8, 32>(v, r);
            (
                m as u64,
                m == 0 && r.is_some() && v.iter().all(|&x| Some(x) == r),
            )
        }
        1 => {
            let held = super::rt_guard::guest_copy::<u16>(addr as usize, n);
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u16, n));
            let r = restart.filter(|&r| r <= 0xFFFF).map(|r| r as u16);
            let m = lanes::<u16, 16>(v, r);
            (
                m as u64,
                m == 0 && r.is_some() && v.iter().all(|&x| Some(x) == r),
            )
        }
        _ => {
            let held = super::rt_guard::guest_copy::<u32>(addr as usize, n);
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u32, n));
            let m = lanes::<u32, 8>(v, restart);
            (
                m as u64,
                m == 0 && restart.is_some() && v.iter().all(|&x| Some(x) == restart),
            )
        }
    };
    (!all_restart).then_some(m)
}

/// exact_max_vertex with the index scan memoized for the rest of the submit (EncodeCtx::max_index_memo, cleared at
/// each submit start): a mesh's index range is scanned once although several passes draw it (shadow cascades,
/// G-buffer). The executor encodes a submit when it is made, so a rescan would read the same bytes. --no-maxidx-memo:
/// scan every draw; --replay-ab-maxidx: scan on replay side B.
pub(crate) unsafe fn cached_max_vertex(
    memo: &mut HashMap<(u64, i32, i32, Option<u32>), Option<u64>>,
    call: &DrawKind,
    restart: Option<u32>,
) -> Option<u64> {
    let DrawKind::Elements {
        index_type,
        count,
        index_addr,
        base_vertex,
        ..
    } = *call
    else {
        return exact_max_vertex(call, restart);
    };
    if count <= 0 {
        return exact_max_vertex(call, restart);
    }
    let m = *memo
        .entry((index_addr, count, index_type, restart))
        .or_insert_with(|| max_index(index_type, index_addr, count as usize, restart));
    Some(m? + base_vertex.max(0) as u64)
}

/// Highest vertex index a draw fetches (base vertex included, restart indices excluded).
pub(crate) unsafe fn exact_max_vertex(call: &DrawKind, restart: Option<u32>) -> Option<u64> {
    match *call {
        DrawKind::Arrays { first, count, .. } => {
            Some((first.max(0) as u64 + count.max(1) as u64).saturating_sub(1))
        }
        DrawKind::Elements {
            index_type,
            count,
            index_addr,
            base_vertex,
            ..
        } => {
            if count <= 0 {
                return None;
            }
            Some(
                max_index(index_type, index_addr, count as usize, restart)?
                    + base_vertex.max(0) as u64,
            )
        }
    }
}

/// Bytes a vertex attribute of this Metal format occupies.
pub(crate) fn vertex_format_size(f: MTLVertexFormat) -> usize {
    use MTLVertexFormat as V;
    match f {
        V::UChar | V::Char | V::UCharNormalized | V::CharNormalized => 1,
        V::UChar2
        | V::Char2
        | V::UChar2Normalized
        | V::Char2Normalized
        | V::UShort
        | V::Short
        | V::UShortNormalized
        | V::ShortNormalized
        | V::Half => 2,
        V::UChar3 | V::Char3 | V::UChar3Normalized | V::Char3Normalized => 3,
        V::UChar4
        | V::Char4
        | V::UChar4Normalized
        | V::Char4Normalized
        | V::UShort2
        | V::Short2
        | V::UShort2Normalized
        | V::Short2Normalized
        | V::Half2
        | V::Float
        | V::Int
        | V::UInt
        | V::Int1010102Normalized
        | V::UInt1010102Normalized
        | V::UChar4Normalized_BGRA
        | V::FloatRG11B10
        | V::FloatRGB9E5 => 4,
        V::UShort3 | V::Short3 | V::UShort3Normalized | V::Short3Normalized | V::Half3 => 6,
        V::UShort4
        | V::Short4
        | V::UShort4Normalized
        | V::Short4Normalized
        | V::Half4
        | V::Float2
        | V::Int2
        | V::UInt2 => 8,
        V::Float3 | V::Int3 | V::UInt3 => 12,
        _ => 16,
    }
}

/// Highest vertex index a draw fetches from per-vertex streams (base vertex included, restart indices excluded), or
/// None when it cannot exceed a `size`-byte stream of `stride` (cheap bound first: no index scan for most draws).
pub(crate) unsafe fn max_fetched_vertex(
    call: &DrawKind,
    stride: u64,
    size: u64,
    exact: impl FnOnce() -> Option<u64>,
) -> Option<u64> {
    let fits = |v: u64| (v + 1) * stride <= size;
    match *call {
        DrawKind::Arrays { first, count, .. } => {
            let last = (first.max(0) as u64 + count.max(1) as u64).saturating_sub(1);
            (!fits(last)).then_some(last)
        }
        DrawKind::Elements {
            index_type,
            count,
            index_addr,
            base_vertex,
            ..
        } => {
            if index_addr < 0x10000 || count <= 0 {
                return None;
            }
            let type_max: u64 = match index_type {
                0 => 0xFF,
                1 => 0xFFFF,
                _ => u32::MAX as u64,
            };
            if fits(type_max + base_vertex.max(0) as u64) {
                return None;
            }
            let last = exact()?;
            (!fits(last)).then_some(last)
        }
    }
}

/// Class of a vertex attribute as the shader's raw inputs see it: 0 float, 1 uint, 2 sint.
pub(crate) fn vertex_format(nvn: u32) -> Option<(MTLVertexFormat, i32)> {
    use MTLVertexFormat as V;
    Some(match nvn {
        1 => (V::UCharNormalized, 0),
        2 => (V::CharNormalized, 0),
        3 => (V::UChar, 1),
        4 => (V::Char, 2),
        5 => (V::Half, 0),
        6 => (V::UShortNormalized, 0),
        7 => (V::ShortNormalized, 0),
        8 => (V::UShort, 1),
        9 => (V::Short, 2),
        10 => (V::Float, 0),
        11 => (V::UInt, 1),
        12 => (V::Int, 2),
        13 => (V::UChar2Normalized, 0),
        14 => (V::Char2Normalized, 0),
        15 => (V::UChar2, 1),
        16 => (V::Char2, 2),
        17 => (V::Half2, 0),
        18 => (V::UShort2Normalized, 0),
        19 => (V::Short2Normalized, 0),
        20 => (V::UShort2, 1),
        21 => (V::Short2, 2),
        22 => (V::Float2, 0),
        23 => (V::UInt2, 1),
        24 => (V::Int2, 2),
        25 => (V::UChar3Normalized, 0),
        26 => (V::Char3Normalized, 0),
        27 => (V::UChar3, 1),
        28 => (V::Char3, 2),
        29 => (V::Half3, 0),
        30 => (V::UShort3Normalized, 0),
        31 => (V::Short3Normalized, 0),
        32 => (V::UShort3, 1),
        33 => (V::Short3, 2),
        34 => (V::Float3, 0),
        35 => (V::UInt3, 1),
        36 => (V::Int3, 2),
        37 | 109 => (V::UChar4Normalized, 0),
        38 | 110 => (V::Char4Normalized, 0),
        39 | 111 => (V::UChar4, 1),
        40 | 112 => (V::Char4, 2),
        41 | 113 => (V::Half4, 0),
        42 | 114 => (V::UShort4Normalized, 0),
        43 | 115 => (V::Short4Normalized, 0),
        44 | 116 => (V::UShort4, 1),
        45 | 117 => (V::Short4, 2),
        46 | 118 => (V::Float4, 0),
        47 | 119 => (V::UInt4, 1),
        48 | 120 => (V::Int4, 2),
        61 => (V::UInt1010102Normalized, 0),
        105 => (V::Int1010102Normalized, 0),
        63 => (V::FloatRG11B10, 0),
        64 => (V::FloatRGB9E5, 0),
        // *_UI2F / *_I2F: integer data converted to float by the fetch unit; Metal value-converts an integer
        // vertex format feeding a float input, which is the same operation.
        81 => (V::UChar, 0),
        82 => (V::Char, 0),
        83 => (V::UShort, 0),
        84 => (V::Short, 0),
        85 => (V::UInt, 0),
        86 => (V::Int, 0),
        87 => (V::UChar2, 0),
        88 => (V::Char2, 0),
        89 => (V::UShort2, 0),
        90 => (V::Short2, 0),
        91 => (V::UInt2, 0),
        92 => (V::Int2, 0),
        93 => (V::UChar3, 0),
        94 => (V::Char3, 0),
        95 => (V::UShort3, 0),
        96 => (V::Short3, 0),
        97 => (V::UInt3, 0),
        98 => (V::Int3, 0),
        99 => (V::UChar4, 0),
        100 => (V::Char4, 0),
        101 => (V::UShort4, 0),
        102 => (V::Short4, 0),
        103 => (V::UInt4, 0),
        104 => (V::Int4, 0),
        _ => return None,
    })
}

pub(crate) struct VAttr {
    pub(crate) stream: usize,
    pub(crate) format: u32,
    pub(crate) offset: u32,
}

pub(crate) fn decode_attr(bits: u32) -> VAttr {
    VAttr {
        stream: (bits & 31) as usize,
        format: (bits >> 5) & 0x1FF,
        offset: bits >> 14,
    }
}

/// A draw's vertex attribute descriptors and stream layout (the vertex half of LayoutMemo).
pub(crate) struct Layout {
    pub(crate) attr_descs: smallvec::SmallVec<[(u32, MTLVertexFormat, usize, usize, i32); 16]>,
    pub(crate) streams_used: [bool; MAX_STREAMS],
    pub(crate) needs_default_attr: bool,
    pub(crate) streams_eff: smallvec::SmallVec<[u64; 16]>,
    pub(crate) repack: Vec<(usize, u64, Vec<(usize, usize, usize)>)>,
}

/// The vertex layout for `vs` with NVN vertex attribute bits (per entry of vs.attrs) and stream bits.
pub(crate) fn derive_layout(vs: &CachedStage, attr_bits: &[Option<u32>], vertex_streams: &[u64]) -> Layout {
    let mut attr_descs: smallvec::SmallVec<[(u32, MTLVertexFormat, usize, usize, i32); 16]> =
        smallvec::SmallVec::new(); // loc, fmt, bufIdx, offset, kind
    let mut streams_used = [false; MAX_STREAMS];
    let mut needs_default_attr = false;
    for (attr_i, &loc) in vs.attrs.iter().enumerate() {
        let bits = attr_bits.get(attr_i).copied().flatten();
        let va = bits.map(decode_attr);
        match va.as_ref().and_then(|a| vertex_format(a.format).map(|f| (a, f))) {
            Some((a, (fmt, kind))) if a.stream < MAX_STREAMS => {
                streams_used[a.stream] = true;
                attr_descs.push((loc, fmt, VB_BASE - a.stream, a.offset as usize, kind));
            }
            _ => {
                needs_default_attr = true;
                attr_descs.push((loc, MTLVertexFormat::Float4, DEFAULT_ATTR_BUFFER, 0, 0));
            }
        }
    }

    // Metal needs 4-byte aligned vertex strides and attribute offsets; NVN does not (TotK's skinned shadow meshes use
    // a 14-byte stride with attributes at offsets 6 and 10). Metal misfetched them: garbage bone indices, skinning
    // matrices read out of bounds, vertices flung across the screen. Such per-vertex streams are repacked on the CPU
    // into an aligned layout (each attribute copied to its own aligned slot) for the vertices the draw fetches.
    let mut streams_eff: smallvec::SmallVec<[u64; 16]> = vertex_streams.iter().copied().collect();
    let mut repack: Vec<(usize, u64, Vec<(usize, usize, usize)>)> = Vec::new(); // (stream, new stride, [(src off, dst off, len)])
    for s in 0..MAX_STREAMS {
        if !streams_used[s] {
            continue;
        }
        let sbits = vertex_streams.get(s).copied().unwrap_or(0);
        let (sstride, sdiv) = (sbits & 0xFFFF_FFFF, sbits >> 32);
        let buf_idx = VB_BASE - s;
        let misaligned = sstride % 4 != 0 || attr_descs.iter().any(|a| a.2 == buf_idx && a.3 % 4 != 0);
        if sstride == 0 || sdiv != 0 || !misaligned {
            continue;
        }
        let mut cursor = 0usize;
        let mut copies = Vec::new();
        for a in attr_descs.iter_mut().filter(|a| a.2 == buf_idx) {
            let len = vertex_format_size(a.1);
            let dst = (cursor + 3) & !3;
            copies.push((a.3, dst, len));
            a.3 = dst;
            cursor = dst + len;
        }
        let new_stride = ((cursor + 3) & !3).max(4) as u64;
        if let Some(e) = streams_eff.get_mut(s) {
            *e = new_stride;
        }
        repack.push((s, new_stride, copies));
    }
    Layout { attr_descs, streams_used, needs_default_attr, streams_eff, repack }
}

/// What encode_draw needs from a draw: the plan's results (shaders, render targets) and a copy of the NVN state the
/// draw reads (only the bindings its shaders use), so the second half needs no NVN state from `State` (parallel
/// encoding).
/// encode_draw's vertex layout and pipeline for one set of inputs (EncodeCtx::layout_memo).
pub(crate) struct LayoutMemo {
    pub(crate) attr_descs: smallvec::SmallVec<[(u32, MTLVertexFormat, usize, usize, i32); 16]>,
    pub(crate) streams_used: [bool; MAX_STREAMS],
    pub(crate) needs_default_attr: bool,
    pub(crate) streams_eff: smallvec::SmallVec<[u64; 16]>,
    pub(crate) repack: Vec<(usize, u64, Vec<(usize, usize, usize)>)>,
    pub(crate) pipe: Retained<ProtocolObject<dyn MTLRenderPipelineState>>,
}

#[cfg(test)]
mod index_scan_tests {
    use super::max_index;

    fn naive(v: &[u64], r: Option<u64>) -> Option<u64> {
        v.iter().copied().filter(|&x| Some(x) != r).max()
    }

    #[test]
    fn max_index_matches_naive() {
        let mut seed = 0x1234_5678u64;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for n in [1usize, 2, 7, 15, 16, 17, 31, 33, 64, 100, 1000] {
            for case in 0..6 {
                let v16: Vec<u16> = (0..n)
                    .map(|_| match case {
                        0 => (rnd() % 600) as u16,
                        1 => {
                            if rnd() % 3 == 0 {
                                0xFFFF
                            } else {
                                (rnd() % 600) as u16
                            }
                        }
                        2 => 0xFFFF,
                        3 => 0,
                        4 => {
                            if rnd() % 2 == 0 {
                                0xFFFF
                            } else {
                                0
                            }
                        }
                        _ => (rnd() % 0x10000) as u16,
                    })
                    .collect();
                let wide: Vec<u64> = v16.iter().map(|&x| x as u64).collect();
                for r in [None, Some(0xFFFFu32), Some(0u32)] {
                    let got = unsafe { max_index(1, v16.as_ptr() as u64, n, r) };
                    assert_eq!(
                        got,
                        naive(&wide, r.map(|x| x as u64)),
                        "u16 n={} case={} r={:?}",
                        n,
                        case,
                        r
                    );
                }
                let v32: Vec<u32> = v16
                    .iter()
                    .map(|&x| if x == 0xFFFF { u32::MAX } else { x as u32 * 3 })
                    .collect();
                let wide32: Vec<u64> = v32.iter().map(|&x| x as u64).collect();
                for r in [None, Some(u32::MAX)] {
                    let got = unsafe { max_index(2, v32.as_ptr() as u64, n, r) };
                    assert_eq!(
                        got,
                        naive(&wide32, r.map(|x| x as u64)),
                        "u32 n={} case={}",
                        n,
                        case
                    );
                }
                let v8: Vec<u8> = v16.iter().map(|&x| x as u8).collect();
                let wide8: Vec<u64> = v8.iter().map(|&x| x as u64).collect();
                for r in [None, Some(0xFFu32), Some(0xFFFFu32)] {
                    let got = unsafe { max_index(0, v8.as_ptr() as u64, n, r) };
                    let rr = r.filter(|&x| x <= 0xFF).map(|x| x as u64);
                    assert_eq!(got, naive(&wide8, rr), "u8 n={} case={} r={:?}", n, case, r);
                }
            }
        }
    }
}
