// Video: VP9 through vp9-mt (bit-exact with libvpx, tile columns on worker threads), H.264 through OpenH264.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, VecDeque};

use openh264::formats::YUVSource;

use crate::{Decode, Decoded};

/// Copies a picture's visible planes into 8-bit 4:2:0 ones (chroma ((w + 1) / 2) x ((h + 1) / 2), resampled by nearest
/// sample if the stream is not 4:2:0).
fn planes(
    (w, h): (usize, usize),
    y: (&[u8], usize),
    chroma: [(&[u8], usize, usize, usize); 2],
) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let mut py = Vec::with_capacity(w * h);
    for r in 0..h {
        py.extend_from_slice(&y.0[r * y.1..r * y.1 + w]);
    }
    let (cw, ch) = ((w + 1) / 2, (h + 1) / 2);
    let [u, v] = chroma.map(|(d, stride, pw, ph)| {
        let mut p = Vec::with_capacity(cw * ch);
        for r in 0..ch {
            let sr = (r * ph / ch).min(ph.saturating_sub(1));
            for c in 0..cw {
                p.push(d[sr * stride + (c * pw / cw).min(pw.saturating_sub(1))]);
            }
        }
        p
    });
    (py, u, v)
}

pub(crate) struct Vp9 {
    dec: vp9_mt::Vp9Decoder,
}

impl Vp9 {
    pub(crate) fn new() -> Vp9 {
        Vp9 { dec: vp9_mt::Vp9Decoder::with_threads(0) }
    }
}

impl Decode for Vp9 {
    fn decode(&mut self, data: &[u8], pts_us: i64, out: &mut VecDeque<Decoded>) -> Result<(), String> {
        self.dec.push(data, Some(pts_us)).map_err(|e| format!("vp9: {e:?}"))?;
        // VP9 does not reorder: a sample shows at most one picture (a superframe's hidden frames show none).
        while let Ok(p) = self.dec.next_picture() {
            let shift = p.bit_depth().saturating_sub(8);
            let to8 = |i: usize| -> (Vec<u8>, usize, usize, usize) {
                let (d, stride) = p.plane(i);
                let (pw, ph) = p.plane_size(i);
                let mut b = Vec::with_capacity(pw * ph);
                for r in 0..ph {
                    b.extend(d[r * stride..r * stride + pw].iter().map(|&s| (s >> shift) as u8));
                }
                (b, pw, pw, ph)
            };
            let (y, u, v) = (to8(0), to8(1), to8(2));
            let (w, h) = (y.2, y.3);
            let (py, pu, pv) = planes((w, h), (&y.0, y.1), [(&u.0, u.1, u.2, u.3), (&v.0, v.1, v.2, v.3)]);
            out.push_back(Decoded::Picture { pts_us, width: w as u32, height: h as u32, y: py, u: pu, v: pv });
        }
        Ok(())
    }

    fn drain(&mut self, _out: &mut VecDeque<Decoded>) {}

    fn reset(&mut self) {
        self.dec.flush();
    }
}

pub(crate) struct Avc {
    dec: openh264::decoder::Decoder,
    /// Bytes in each NAL unit's length prefix (avcC's lengthSizeMinusOne + 1); 0: the samples are Annex B already.
    nal_len: usize,
    /// The parameter sets from avcC, in Annex B, sent before the first sample.
    header: Vec<u8>,
    sent_header: bool,
    /// Presentation times of samples sent and not yet shown: pictures come out in presentation order, so each takes the
    /// earliest.
    pending: BinaryHeap<Reverse<i64>>,
}

const START: [u8; 4] = [0, 0, 0, 1];

/// An OpenH264 decoder that holds pictures back for reordering (B-frames); the crate's default flushes one after every
/// sample that showed none, which breaks streams with B-frames.
fn new_h264() -> Result<openh264::decoder::Decoder, openh264::Error> {
    use openh264::decoder::{DecoderConfig, Flush};
    openh264::decoder::Decoder::with_api_config(
        openh264::OpenH264API::from_source(),
        DecoderConfig::new().flush_after_decode(Flush::NoFlush),
    )
}

impl Avc {
    pub(crate) fn new(avcc: &[u8]) -> Result<Avc, String> {
        let mut header = Vec::new();
        let mut nal_len = 0;
        if avcc.len() >= 7 && avcc[0] == 1 {
            nal_len = (avcc[4] & 3) as usize + 1;
            let mut o = 5;
            let mut sets = |count: usize, o: &mut usize| {
                for _ in 0..count {
                    if *o + 2 > avcc.len() {
                        return;
                    }
                    let n = u16::from_be_bytes([avcc[*o], avcc[*o + 1]]) as usize;
                    *o += 2;
                    if *o + n > avcc.len() {
                        return;
                    }
                    header.extend_from_slice(&START);
                    header.extend_from_slice(&avcc[*o..*o + n]);
                    *o += n;
                }
            };
            let sps = (avcc[5] & 31) as usize;
            o += 1;
            sets(sps, &mut o);
            if o < avcc.len() {
                let pps = avcc[o] as usize;
                o += 1;
                sets(pps, &mut o);
            }
        }
        let dec = new_h264().map_err(|e| format!("h264: {e}"))?;
        Ok(Avc { dec, nal_len, header, sent_header: false, pending: BinaryHeap::new() })
    }

    fn picture(yuv: &openh264::decoder::DecodedYUV<'_>, pts_us: i64) -> Decoded {
        let (w, h) = yuv.dimensions();
        let (ys, us, vs) = yuv.strides();
        let (cw, ch) = ((w + 1) / 2, (h + 1) / 2);
        let (y, u, v) = planes((w, h), (yuv.y(), ys), [(yuv.u(), us, cw, ch), (yuv.v(), vs, cw, ch)]);
        Decoded::Picture { pts_us, width: w as u32, height: h as u32, y, u, v }
    }
}

impl Decode for Avc {
    fn decode(&mut self, data: &[u8], pts_us: i64, out: &mut VecDeque<Decoded>) -> Result<(), String> {
        let mut buf = Vec::with_capacity(data.len() + self.header.len() + 16);
        if !self.sent_header {
            buf.extend_from_slice(&self.header);
            self.sent_header = true;
        }
        if self.nal_len == 0 {
            buf.extend_from_slice(data);
        } else {
            let mut o = 0;
            while o + self.nal_len <= data.len() {
                let n = data[o..o + self.nal_len].iter().fold(0usize, |a, &b| a << 8 | b as usize);
                o += self.nal_len;
                if o + n > data.len() {
                    break;
                }
                buf.extend_from_slice(&START);
                buf.extend_from_slice(&data[o..o + n]);
                o += n;
            }
        }
        self.pending.push(Reverse(pts_us));
        match self.dec.decode(&buf) {
            Ok(Some(yuv)) => {
                let pts = self.pending.pop().map_or(pts_us, |Reverse(p)| p);
                out.push_back(Self::picture(&yuv, pts));
                Ok(())
            }
            Ok(None) => Ok(()),
            Err(e) => Err(format!("h264: {e}")),
        }
    }

    fn drain(&mut self, out: &mut VecDeque<Decoded>) {
        if let Ok(frames) = self.dec.flush_remaining() {
            for yuv in &frames {
                let pts = self.pending.pop().map_or(0, |Reverse(p)| p);
                out.push_back(Self::picture(yuv, pts));
            }
        }
        self.pending.clear();
    }

    fn reset(&mut self) {
        if let Ok(d) = new_h264() {
            self.dec = d;
        }
        self.sent_header = false;
        self.pending.clear();
    }
}

/// BT.601 limited-range 4:2:0 to RGBA (bytes R, G, B, A).
pub(crate) fn yuv420_to_rgba(out: &mut [u8], w: usize, h: usize, y: &[u8], u: &[u8], v: &[u8]) {
    let cw = (w + 1) / 2;
    for r in 0..h {
        let (yr, cr) = (&y[r * w..r * w + w], r / 2 * cw);
        for c in 0..w {
            let yy = (yr[c] as i32 - 16) * 298;
            let (d, e) = (u[cr + c / 2] as i32 - 128, v[cr + c / 2] as i32 - 128);
            let px = &mut out[(r * w + c) * 4..(r * w + c) * 4 + 4];
            px[0] = ((yy + 409 * e + 128) >> 8).clamp(0, 255) as u8;
            px[1] = ((yy - 100 * d - 208 * e + 128) >> 8).clamp(0, 255) as u8;
            px[2] = ((yy + 516 * d + 128) >> 8).clamp(0, 255) as u8;
            px[3] = 255;
        }
    }
}
