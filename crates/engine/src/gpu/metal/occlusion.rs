// Occlusion queries (SAMPLES_PASSED): counting encoders, resolving reports when command buffers complete, and their
// journal (--log-occl, --journal-occl-reads).

use super::*;

/// --log-occl: SAMPLES_PASSED reports whose draws (since the counter reset) did not all reach a counting encoder.
pub(super) fn log_occl() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| crate::settings::get().log.log_occl)
}

/// SAMPLES_PASSED occlusion queries on Metal visibility results.
///
/// NVN keeps one running sample counter: ResetCounter zeroes it and every ReportCounter writes its current value
/// (NVNcounterData {u64 value, u64 timestamp}). Metal counts per visibility-buffer slot and, at the end of a render pass,
/// overwrites the slots that pass used (others keep their values). So each stretch of counted draws inside one encoder
/// gets its own slot, and a report is the sum of the slots used since the last reset. Reports are written to guest
/// memory once the command buffer has completed (`resolve`, when `submit` or a sync wait retires it), like a query read
/// a frame or two late on hardware.
pub(super) struct Occlusion {
    pub(super) buf: Option<Retained<ProtocolObject<dyn MTLBuffer>>>,
    /// Next free slot in `buf` (a ring; one command buffer in flight uses a few hundred).
    pub(super) next: usize,
    /// A ResetCounter has been seen: draws are counted from then on.
    pub(super) counting: bool,
    /// Slots summed by the next report.
    pub(super) slots: Vec<usize>,
    /// Slot the open encoder is counting into.
    pub(super) cur: Option<usize>,
    /// Reports recorded in the command buffer being built (moved to its `InFlight` entry on commit).
    pub(super) pending: Vec<(Vec<usize>, u64)>,
    /// --log-occl: draw commands executed since the last reset, and how many of them reached a counting encoder.
    pub(super) draws_seen: u32,
    pub(super) draws_counted: u32,
    /// Reports recorded in the current frame (journal ordinal).
    pub(super) reports_in_frame: u32,
    pub(super) ord_frame: u64,
}

impl Default for Occlusion {
    fn default() -> Self {
        Occlusion {
            buf: None,
            next: 0,
            counting: false,
            slots: Vec::new(),
            cur: None,
            pending: Vec::new(),
            draws_seen: 0,
            draws_counted: 0,
            reports_in_frame: 0,
            ord_frame: 0,
        }
    }
}

impl Occlusion {
    /// Metal limits visibility result offsets to 256 KiB on Apple GPUs (larger ones are silently not written).
    const SLOTS: usize = 1 << 15;

    /// The visibility buffer every render pass is given (so counting can start inside any open encoder).
    pub(super) fn buffer(
        &mut self,
        device: &ProtocolObject<dyn MTLDevice>,
    ) -> Option<Retained<ProtocolObject<dyn MTLBuffer>>> {
        if self.buf.is_none() {
            self.buf = device.newBufferWithLength_options(
                Self::SLOTS * 8,
                MTLResourceOptions::StorageModeShared,
            );
        }
        self.buf.clone()
    }

    pub(super) fn resolve(&self, reports: Vec<(Vec<usize>, u64)>) {
        if let Some(buf) = &self.buf {
            resolve_occlusion(buf, &reports);
        }
    }
}

/// Writes SAMPLES_PASSED reports of a completed command buffer: each the sum of its visibility slots.
pub(super) fn resolve_occlusion(buf: &ProtocolObject<dyn MTLBuffer>, reports: &[(Vec<usize>, u64)]) {
    let vals = buf.contents().as_ptr() as *const u64;
    for (slots, addr) in reports {
        let n: u64 = slots.iter().map(|&s| unsafe { vals.add(s).read() }).sum();
        if tracing_frame() {
            println!(
                "[cmdtrace] SAMPLES_PASSED {:#x} = {} ({} slots)",
                addr,
                n,
                slots.len()
            );
        }
        // The host address and the count it held before (journal; matches the occl-read lines).
        let host = crate::nvn::api::translate_virtual(*addr as usize);
        let before = if journal_on() && host >= 0x10000 {
            unsafe { *(report_guard::engine_ptr(host) as *const u64) as i64 }
        } else {
            0
        };
        write_counter(1, *addr, Some(n));
        report_guard::close(host);
        if journal_on() || log_occl() {
            if let Some((f, ord, pos, draws)) = OCCL_TAGS.lock().unwrap().remove(addr) {
                let line = format!(
                    "occl f{} #{} pos {:?} draws {} = {} @{:#x} was {} t {:.3}",
                    f,
                    ord,
                    pos,
                    draws,
                    n,
                    host,
                    before,
                    host_secs() * 1e3
                );
                if log_occl() && n == 0 {
                    println!("[occl] {}", line);
                }
                journal(|| line);
            }
        }
    }
}

/// --journal-occl-reads (TotK 1.1.2, profile hook at main+0x10c4d44): the game's occlusion query read, called
/// with a query object. The object keeps three report slots at +0x00 / +0x20 / +0x40 of its report buffer ([obj+8]:
/// GPU address at +0x48, memory pool at +0x50; the pool's CPU pointer at +0x100, its GPU base at [[+0x110]+0x18]), a
/// state byte per slot (+0x22..+0x24: 1 = issued, advanced to 3, or to 2 with flag bit 2; 3 = readable), the result at
/// +0x18 (reset to -1) and flags at +0x20 (bit 1 visible, bit 0 result read). The first slot in state 3 is read:
/// count 0 clears bit 1, else it stays set; no readable slot leaves "visible, no result". Journals the object's state
/// before the read: `occl-read obj <obj> fl <flags> st <s0>,<s1>,<s2> <host addr>=<count> ...` (match the
/// addresses with the `@` addresses of the `occl` lines).
pub(crate) unsafe extern "C" fn journal_occl_read(obj: usize) {
    if !journal_on() || obj < 0x10000 {
        return;
    }
    let rd = |a: usize| -> usize {
        if a >= 0x10000 {
            *(a as *const usize)
        } else {
            0
        }
    };
    let buf = rd(obj + 8);
    let pool = if buf != 0 { rd(buf + 0x50) } else { 0 };
    let cpu = if pool != 0 { rd(pool + 0x100) } else { 0 };
    let p110 = if pool != 0 { rd(pool + 0x110) } else { 0 };
    let off = if p110 != 0 {
        rd(buf + 0x48).wrapping_sub(rd(p110 + 0x18))
    } else {
        0
    };
    let b = |o: usize| *((obj + o) as *const u8);
    let slots: Vec<String> = (0..3)
        .map(|k| {
            let a = cpu.wrapping_add(off).wrapping_add(0x20 * k);
            if cpu != 0 {
                format!("{:#x}={}", a, *(a as *const u64) as i64)
            } else {
                "-".to_string()
            }
        })
        .collect();
    let thread = std::thread::current();
    let line = format!(
        "occl-read obj {:#x} fl {:#x} st {},{},{} {} t {:.3} {}",
        obj,
        b(0x20),
        b(0x22),
        b(0x23),
        b(0x24),
        slots.join(" "),
        host_secs() * 1e3,
        thread.name().unwrap_or("?")
    );
    journal(|| line);
}

/// Journal tags of recorded SAMPLES_PASSED reports: frame, ordinal in the frame, query box position, draws.
pub(super) static OCCL_TAGS: Mutex<HashMap<u64, (u64, u32, Vec<f32>, u32)>> =
    Mutex::new(HashMap::with_hasher(rustc_hash::FxBuildHasher));

impl Gfx {
    /// Real occlusion queries need the translated-shader draw path (the only one that counts); otherwise SAMPLES_PASSED
    /// reports 0.
    pub(super) fn real_samples(&self) -> bool {
        self.shader_lib.is_some()
    }

    pub(super) fn occl_reset(&self, st: &mut State) {
        self.occl_stop(st);
        st.occl.counting = true;
        st.occl.slots.clear();
        st.occl.draws_seen = 0;
        st.occl.draws_counted = 0;
    }

    pub(super) fn occl_report(&self, st: &mut State, addr: u64) {
        // Later draws must not land in slots this report already sums (they are only final when the pass ends).
        self.occl_stop(st);
        if st.occl.counting && addr >= 0x10000 {
            if log_occl() && st.occl.draws_seen != st.occl.draws_counted {
                static N: AtomicU64 = AtomicU64::new(0);
                if N.fetch_add(1, Ordering::Relaxed) < 2000 {
                    println!("[occl] f{} report {:#x}: {} draw commands since reset, {} counted ({} slots)", presented_frames() + 1, addr,
                        st.occl.draws_seen, st.occl.draws_counted, st.occl.slots.len());
                }
            }
            if journal_on() || log_occl() {
                // The query box's position (its draw's vertex uniform buffer 1 starts with it) names the object across frames.
                let pos = st
                    .uniform_buffers
                    .get(&(0, 1))
                    .filter(|(a, sz)| *a >= 0x10000 && *sz >= 16)
                    .map(|(a, _)| unsafe {
                        std::slice::from_raw_parts(
                            report_guard::engine_ptr(crate::nvn::api::translate_virtual(
                                *a as usize,
                            )) as *const f32,
                            3,
                        )
                        .to_vec()
                    })
                    .unwrap_or_default();
                if st.occl.ord_frame != presented_frames() {
                    st.occl.ord_frame = presented_frames();
                    st.occl.reports_in_frame = 0;
                }
                let ord = st.occl.reports_in_frame;
                OCCL_TAGS
                    .lock()
                    .unwrap()
                    .insert(addr, (presented_frames() + 1, ord, pos, st.occl.draws_seen));
            }
            st.occl.reports_in_frame += 1;
            let slots = st.occl.slots.clone();
            st.occl.pending.push((slots, addr));
            note_gpu_guest_write(GuestWrite::Occlusion, addr, 16);
        } else {
            // No count to report (no reset since the last report): nothing will be written there.
            report_guard::close(crate::nvn::api::translate_virtual(addr as usize));
        }
    }

    pub(super) fn occl_stop(&self, st: &mut State) {
        if st.occl.cur.take().is_some() {
            if let Some(enc) = &st.enc {
                enc.push(issuer::Op::Visibility(
                    objc2_metal::MTLVisibilityResultMode::Disabled,
                    0,
                ));
            }
        }
    }

    /// Called before each draw in the open encoder: starts counting into a fresh slot if needed.
    pub(super) fn occl_before_draw(&self, st: &mut State) {
        if !st.occl.counting || st.occl.cur.is_some() {
            return;
        }
        let Some(enc) = st.enc.clone() else { return };
        let s = st.occl.next;
        st.occl.next = (s + 1) % Occlusion::SLOTS;
        enc.push(issuer::Op::Visibility(
            objc2_metal::MTLVisibilityResultMode::Counting,
            s * 8,
        ));
        st.occl.slots.push(s);
        st.occl.cur = Some(s);
    }
}
