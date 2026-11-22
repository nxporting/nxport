// NVN primitive restart on hosts with Metal's rules.

/// Whether `v` contains `x`, four indices per 64-bit word (a lane equals x iff it is zero after XOR; the classic
/// has-zero-lane test, which has no false positives). ~4x the element-by-element scan.
fn contains_u16(v: &[u16], x: u16) -> bool {
    const LO: u64 = 0x0001_0001_0001_0001;
    const HI: u64 = 0x8000_8000_8000_8000;
    let pat = x as u64 * LO;
    let (head, words, tail) = unsafe { v.align_to::<u64>() };
    if head.contains(&x) || tail.contains(&x) {
        return true;
    }
    for c in words.chunks(4) {
        let mut hit = 0u64;
        for &w in c {
            let y = w ^ pat;
            hit |= y.wrapping_sub(LO) & !y & HI;
        }
        if hit != 0 {
            return true;
        }
    }
    false
}

/// NVN primitive restart for an indexed draw whose indices contain the restart value (None: draw as is). Metal restarts
/// only strips, only on 0xFFFF / 0xFFFFFFFF, and compares after adding the base vertex: TotK's strips use restart index
/// 0xFFFF with base vertices in the thousands, so the restart would become a real (garbage) vertex. The rewrite produces 32-bit indices with the base vertex added:
/// strips keep a 0xFFFFFFFF restart (Metal's own), lists drop the unfinished primitive at each restart (NVN/GL rules).
pub unsafe fn restart_rewrite(
    mode: i32,
    index_type: i32,
    addr: u64,
    count: i32,
    restart: u32,
    base_vertex: i32,
) -> Option<Vec<u32>> {
    if addr < 0x10000 || count <= 0 {
        return None;
    }
    let n = count as usize;
    // Checked before anything is allocated: most indexed draws contain no restart index.
    let values: Vec<u32> = match index_type {
        0 => {
            #[cfg(target_os = "macos")]
            let held = crate::gpu::metal::rt_guard::guest_copy::<u8>(addr as usize, n);
            #[cfg(not(target_os = "macos"))]
            let held: Option<Vec<u8>> = None;
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u8, n));
            if restart > 0xFF || !v.contains(&(restart as u8)) {
                return None;
            }
            v.iter().map(|&x| x as u32).collect()
        }
        1 => {
            #[cfg(target_os = "macos")]
            let held = crate::gpu::metal::rt_guard::guest_copy::<u16>(addr as usize, n);
            #[cfg(not(target_os = "macos"))]
            let held: Option<Vec<u16>> = None;
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u16, n));
            if restart > 0xFFFF || !contains_u16(v, restart as u16) {
                return None;
            }
            v.iter().map(|&x| x as u32).collect()
        }
        2 => {
            #[cfg(target_os = "macos")]
            let held = crate::gpu::metal::rt_guard::guest_copy::<u32>(addr as usize, n);
            #[cfg(not(target_os = "macos"))]
            let held: Option<Vec<u32>> = None;
            let v = held.as_deref().unwrap_or_else(|| std::slice::from_raw_parts(addr as *const u32, n));
            if !v.contains(&restart) {
                return None;
            }
            v.to_vec()
        }
        _ => return None,
    };
    let rebase = |v: u32| (v as i64 + base_vertex as i64).clamp(0, u32::MAX as i64 - 1) as u32;
    let per_prim = match mode {
        0 => 1,     // points
        1 => 2,     // lines
        4 => 3,     // triangles
        3 | 5 => 0, // line strip, triangle strip: Metal restarts these itself
        _ => return None,
    };
    let mut out = Vec::with_capacity(n);
    if per_prim == 0 {
        out.extend(
            values
                .iter()
                .map(|&v| if v == restart { u32::MAX } else { rebase(v) }),
        );
    } else {
        let mut cur = Vec::with_capacity(3);
        for &v in &values {
            if v == restart {
                cur.clear();
                continue;
            }
            cur.push(rebase(v));
            if cur.len() == per_prim {
                out.extend_from_slice(&cur);
                cur.clear();
            }
        }
    }
    Some(out)
}

/// Whether NVN primitive mode `mode` has no Metal equivalent and is drawn as a list (emulated_indices): line loops,
/// triangle fans, quads, quad strips and polygons.
pub fn emulated_mode(mode: i32) -> bool {
    matches!(mode, 2 | 6 | 7 | 8 | 9)
}

/// The list (lines for a line loop, else triangles) that draws `vertices` in NVN primitive mode `mode`, restarting at each
/// `restart` value (GL rules: the unfinished primitive is dropped).
pub fn emulated_indices(mode: i32, vertices: impl Iterator<Item = u32>, restart: Option<u32>) -> Vec<u32> {
    let mut out = Vec::new();
    let mut seg: Vec<u32> = Vec::new();
    let mut flush = |seg: &mut Vec<u32>, out: &mut Vec<u32>| {
        let s = &seg[..];
        match mode {
            2 if s.len() >= 2 => {
                for i in 0..s.len() {
                    out.extend_from_slice(&[s[i], s[(i + 1) % s.len()]]);
                }
            }
            6 | 9 => {
                for i in 1..s.len().saturating_sub(1) {
                    out.extend_from_slice(&[s[0], s[i], s[i + 1]]);
                }
            }
            7 => {
                for q in s.chunks_exact(4) {
                    out.extend_from_slice(&[q[0], q[1], q[2], q[0], q[2], q[3]]);
                }
            }
            8 => {
                for i in (0..s.len().saturating_sub(3)).step_by(2) {
                    let (a, b, c, d) = (s[i], s[i + 1], s[i + 3], s[i + 2]);
                    out.extend_from_slice(&[a, b, c, a, c, d]);
                }
            }
            _ => {}
        }
        seg.clear();
    };
    for v in vertices {
        if Some(v) == restart {
            flush(&mut seg, &mut out);
        } else {
            seg.push(v);
        }
    }
    flush(&mut seg, &mut out);
    out
}

/// emulated_indices of an indexed draw's guest indices, with the base vertex added (32-bit output).
pub unsafe fn emulated_elements(
    mode: i32,
    index_type: i32,
    addr: u64,
    count: i32,
    restart: Option<u32>,
    base_vertex: i32,
) -> Vec<u32> {
    if addr < 0x10000 || count <= 0 {
        return Vec::new();
    }
    let n = count as usize;
    let values: Vec<u32> = match index_type {
        0 => std::slice::from_raw_parts(addr as *const u8, n).iter().map(|&x| x as u32).collect(),
        1 => std::slice::from_raw_parts(addr as *const u16, n).iter().map(|&x| x as u32).collect(),
        2 => std::slice::from_raw_parts(addr as *const u32, n).to_vec(),
        _ => return Vec::new(),
    };
    let rebase = |v: u32| (v as i64 + base_vertex as i64).clamp(0, u32::MAX as i64 - 1) as u32;
    emulated_indices(mode, values.into_iter(), restart).into_iter().map(rebase).collect()
}

#[cfg(test)]
mod emulated_tests {
    use super::emulated_indices as e;
    #[test]
    fn modes() {
        assert_eq!(e(7, 0..8, None), vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7]);
        assert_eq!(e(6, 0..5, None), vec![0, 1, 2, 0, 2, 3, 0, 3, 4]);
        assert_eq!(e(8, 0..6, None), vec![0, 1, 3, 0, 3, 2, 2, 3, 5, 2, 5, 4]);
        assert_eq!(e(2, 0..3, None), vec![0, 1, 1, 2, 2, 0]);
        assert_eq!(e(6, [0, 1, 2, 9, 3, 4, 5, 6].into_iter(), Some(9)), vec![0, 1, 2, 3, 4, 5, 3, 5, 6]);
        assert_eq!(e(7, 0..6, None), vec![0, 1, 2, 0, 2, 3]);
    }
}

#[cfg(test)]
mod contains_u16_tests {
    #[test]
    fn matches_slice_contains() {
        let mut seed = 0x1234_5678u32;
        for len in 0..80usize {
            for &x in &[0u16, 1, 0x7FFF, 0x8000, 0xFFFE, 0xFFFF] {
                for off in 0..4 {
                    let mut v: Vec<u16> = (0..len + off)
                        .map(|_| {
                            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                            (seed >> 16) as u16 | 1
                        })
                        .collect();
                    v.retain(|&y| y != x);
                    v.resize(len + off, if x == 1 { 2 } else { 1 });
                    let s = &v[off.min(v.len())..];
                    assert_eq!(super::contains_u16(s, x), s.contains(&x));
                    if !s.is_empty() {
                        let mut w = s.to_vec();
                        let i = (seed as usize) % w.len();
                        w[i] = x;
                        assert!(
                            super::contains_u16(&w, x),
                            "len {} x {:#x} at {}",
                            w.len(),
                            x,
                            i
                        );
                    }
                }
            }
        }
    }
}
