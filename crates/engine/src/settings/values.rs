// Option value types with a text form (command line, preset files).

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

/// Serde through the text form.
macro_rules! text_serde {
    ($($t:ty),* $(,)?) => {$(
        impl serde::Serialize for $t {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }
        impl<'de> serde::Deserialize<'de> for $t {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                String::deserialize(d)?.parse().map_err(serde::de::Error::custom)
            }
        }
    )*};
}

fn int<T: FromStr>(s: &str, what: &str) -> Result<T, String> {
    s.trim()
        .parse()
        .map_err(|_| format!("{what}: expected a number, got {s:?}"))
}

fn hex(s: &str) -> Result<usize, String> {
    let t = s.trim();
    usize::from_str_radix(t.strip_prefix("0x").unwrap_or(t), 16)
        .map_err(|_| format!("expected a hex address, got {s:?}"))
}

/// Width x height, written `WxH`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Size {
    pub w: u32,
    pub h: u32,
}

impl FromStr for Size {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (w, h) = s
            .split_once('x')
            .ok_or_else(|| format!("expected WxH, got {s:?}"))?;
        Ok(Size {
            w: int(w, "width")?,
            h: int(h, "height")?,
        })
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}x{}", self.w, self.h)
    }
}

impl Size {
    pub fn is(&self, w: impl TryInto<u32>, h: impl TryInto<u32>) -> bool {
        w.try_into().ok() == Some(self.w) && h.try_into().ok() == Some(self.h)
    }
}

/// An address, written in hex (`0x` optional).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Addr(pub usize);

impl FromStr for Addr {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        hex(s).map(Addr)
    }
}

impl fmt::Display for Addr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:#x}", self.0)
    }
}

/// An inclusive range of numbers (frames, encoders), written `A-B`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub first: u64,
    pub last: u64,
}

impl Span {
    pub fn contains(&self, v: u64) -> bool {
        (self.first..=self.last).contains(&v)
    }
}

impl FromStr for Span {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (a, b) = s
            .split_once('-')
            .ok_or_else(|| format!("expected A-B, got {s:?}"))?;
        Ok(Span {
            first: int(a, "range start")?,
            last: int(b, "range end")?,
        })
    }
}

impl fmt::Display for Span {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}-{}", self.first, self.last)
    }
}

/// A display aspect ratio, written `W:H` (or a number: `W:1`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aspect {
    pub w: f64,
    pub h: f64,
}

impl Aspect {
    /// Width / height.
    pub fn ratio(self) -> f64 {
        self.w / self.h
    }
}

impl FromStr for Aspect {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (w, h) = match s.split_once(':') {
            Some((w, h)) => (
                int::<f64>(w, "aspect width")?,
                int::<f64>(h, "aspect height")?,
            ),
            None => (int::<f64>(s, "aspect")?, 1.0),
        };
        let a = Aspect { w, h };
        if a.ratio().is_finite() && a.ratio() > 0.0 {
            Ok(a)
        } else {
            Err(format!("not an aspect ratio: {s:?}"))
        }
    }
}

impl fmt::Display for Aspect {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.w, self.h)
    }
}

/// Frame replay: at presented frame `frame` (and every `every` frames after, if not 0) its submits run `reps` times.
/// Written `F[:K[:M]]` (K default 10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReplaySpec {
    pub frame: u64,
    pub reps: u32,
    pub every: u64,
}

impl FromStr for ReplaySpec {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let mut p = s.split(':');
        let frame = int(p.next().unwrap_or(""), "replay frame")?;
        let reps = p.next().map_or(Ok(10), |v| int(v, "replay repetitions"))?;
        let every = p.next().map_or(Ok(0), |v| int(v, "replay interval"))?;
        Ok(ReplaySpec { frame, reps, every })
    }
}

impl fmt::Display for ReplaySpec {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}:{}", self.frame, self.reps, self.every)
    }
}

/// Glitch scan of presented frames `from..from + count`, reporting frames with at least `min_px` outlier pixels.
/// Written `FROM[:COUNT[:MIN_PX]]` (defaults 600, 200).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlitchScan {
    pub from: u64,
    pub count: u64,
    pub min_px: usize,
}

impl FromStr for GlitchScan {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let mut p = s.split(':');
        let from = int(p.next().unwrap_or(""), "scan start")?;
        let count = p.next().map_or(Ok(600), |v| int(v, "scan length"))?;
        let min_px = p.next().map_or(Ok(200), |v| int(v, "minimum pixels"))?;
        Ok(GlitchScan {
            from,
            count,
            min_px,
        })
    }
}

impl fmt::Display for GlitchScan {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}:{}", self.from, self.count, self.min_px)
    }
}

/// Guest memory, written `0xADDR:LEN` (LEN decimal).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemRange {
    pub addr: usize,
    pub len: usize,
}

impl FromStr for MemRange {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (a, l) = s
            .split_once(':')
            .ok_or_else(|| format!("expected 0xADDR:LEN, got {s:?}"))?;
        Ok(MemRange {
            addr: hex(a)?,
            len: int(l, "length")?,
        })
    }
}

impl fmt::Display for MemRange {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:#x}:{}", self.addr, self.len)
    }
}

/// Guest memory written to a file, `0xADDR:LEN:PATH`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemDump {
    pub range: MemRange,
    pub path: PathBuf,
}

impl FromStr for MemDump {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let mut p = s.splitn(3, ':');
        let (Some(a), Some(l), Some(path)) = (p.next(), p.next(), p.next()) else {
            return Err(format!("expected 0xADDR:LEN:PATH, got {s:?}"));
        };
        Ok(MemDump {
            range: MemRange {
                addr: hex(a)?,
                len: int(l, "length")?,
            },
            path: path.into(),
        })
    }
}

impl fmt::Display for MemDump {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.range, self.path.display())
    }
}

/// A file written at a presented frame, `PATH[:FRAME]` (default frame 300).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameFile {
    pub path: PathBuf,
    pub frame: u64,
}

impl FromStr for FrameFile {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        Ok(
            match s
                .rsplit_once(':')
                .and_then(|(p, f)| f.parse().ok().map(|f| (p, f)))
            {
                Some((p, frame)) => FrameFile {
                    path: p.into(),
                    frame,
                },
                None => FrameFile {
                    path: s.into(),
                    frame: 300,
                },
            },
        )
    }
}

impl fmt::Display for FrameFile {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}:{}", self.path.display(), self.frame)
    }
}

/// Every `every`-th draw starting at `first`, written `FIRST/EVERY`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrawMask {
    pub first: u64,
    pub every: u64,
}

impl FromStr for DrawMask {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (a, b) = s
            .split_once('/')
            .ok_or_else(|| format!("expected FIRST/EVERY, got {s:?}"))?;
        let every: u64 = int(b, "interval")?;
        if every == 0 {
            return Err("interval must not be 0".into());
        }
        Ok(DrawMask {
            first: int(a, "first draw")?,
            every,
        })
    }
}

impl fmt::Display for DrawMask {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/{}", self.first, self.every)
    }
}

/// A guest code address to log registers at, `0xREL[:xN[>OFF]]`: also dump 12 qwords at register xN, or at the pointer
/// stored at [xN + OFF].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TracePoint {
    pub rel: usize,
    pub reg: Option<u8>,
    pub deref: Option<usize>,
}

impl FromStr for TracePoint {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (a, r) = match s.split_once(':') {
            Some((a, r)) => (a, Some(r)),
            None => (s, None),
        };
        let (reg, deref) = match r {
            None => (None, None),
            Some(r) => {
                let (reg, off) = match r.split_once('>') {
                    Some((x, o)) => (x, Some(hex(o)?)),
                    None => (r, None),
                };
                let n = reg
                    .trim()
                    .strip_prefix('x')
                    .ok_or_else(|| format!("expected xN, got {reg:?}"))?;
                (Some(int(n, "register")?), off)
            }
        };
        Ok(TracePoint {
            rel: hex(a)?,
            reg,
            deref,
        })
    }
}

impl fmt::Display for TracePoint {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:#x}", self.rel)?;
        if let Some(r) = self.reg {
            write!(f, ":x{r}")?;
        }
        if let Some(o) = self.deref {
            write!(f, ">{o:#x}")?;
        }
        Ok(())
    }
}

/// Keys held while the presented frame is in `first..last`, written `A-B:K[+K..]` (key codes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeldKeys {
    pub first: u64,
    pub last: u64,
    pub keys: Vec<u16>,
}

impl FromStr for HeldKeys {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let (range, keys) = s
            .split_once(':')
            .ok_or_else(|| format!("expected A-B:KEYS, got {s:?}"))?;
        let (a, b) = range
            .split_once('-')
            .ok_or_else(|| format!("expected A-B, got {range:?}"))?;
        let keys = keys
            .split('+')
            .map(|k| int(k, "key code"))
            .collect::<Result<_, _>>()?;
        Ok(HeldKeys {
            first: int(a, "first frame")?,
            last: int(b, "last frame")?,
            keys,
        })
    }
}

impl fmt::Display for HeldKeys {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let keys: Vec<String> = self.keys.iter().map(|k| k.to_string()).collect();
        write!(f, "{}-{}:{}", self.first, self.last, keys.join("+"))
    }
}

text_serde!(
    Size, Addr, Span, Aspect, ReplaySpec, GlitchScan, MemRange, MemDump, FrameFile,
    DrawMask, TracePoint, HeldKeys
);

/// Choice options.
macro_rules! choice {
    ($(#[$m:meta])* $name:ident { $($(#[$vm:meta])* $v:ident),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
        #[serde(rename_all = "kebab-case")]
        pub enum $name { $($(#[$vm])* $v),* }
    };
}

choice!(
    /// Which work the GPU executor does (bisection aid).
    GfxMode {
        /// Encode everything but submit nothing (no GPU work).
        Dry,
        /// Submit clears only.
        Clears,
        /// Submit clears and draws (no copies).
        Draws,
        /// Everything.
        All,
    }
);

choice!(
    /// Occlusion report pages held unreadable by the game until the GPU wrote them.
    ReportGuard {
        On,
        Off,
        /// Only log who touches them.
        Log,
    }
);

choice!(
    /// Guest pages under textures that live in Metal.
    TexBacking {
        Off,
        /// Held inaccessible and, after an exact rebuild check, discarded.
        On,
        /// Held and logged only.
        Log,
    }
);

choice!(
    /// Diagnostic page traps on GPU-written guest memory under the early fence.
    FenceTrap {
        Off,
        /// All GPU-written memory.
        On,
        /// Occlusion report pages only.
        Occl,
    }
);

/// Frame rate / pacing mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum FpsMode {
    #[default]
    Unlocked,
    Stock30,
}

impl FpsMode {
    pub const ALL: [FpsMode; 2] = [FpsMode::Unlocked, FpsMode::Stock30];
    pub const NAMES: [&'static str; 2] = ["Unlocked (60+ FPS)", "Stock (30 FPS)"];
}

impl clap::ValueEnum for FpsMode {
    fn value_variants<'a>() -> &'a [Self] {
        &Self::ALL
    }
    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        match self {
            FpsMode::Unlocked => Some(clap::builder::PossibleValue::new("unlocked").alias("60").alias("uncapped")),
            FpsMode::Stock30 => Some(clap::builder::PossibleValue::new("stock").alias("30")),
        }
    }
}

impl FromStr for FpsMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let t = s.trim().to_ascii_lowercase();
        match t.as_str() {
            "unlocked" | "uncapped" | "60" | "true" => Ok(FpsMode::Unlocked),
            "stock" | "30" | "false" => Ok(FpsMode::Stock30),
            _ => Err(format!("expected 'unlocked' or 'stock', got {s:?}")),
        }
    }
}

impl fmt::Display for FpsMode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FpsMode::Unlocked => write!(f, "unlocked"),
            FpsMode::Stock30 => write!(f, "stock"),
        }
    }
}

impl<'de> serde::Deserialize<'de> for FpsMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct FpsVisitor;
        impl<'de> serde::de::Visitor<'de> for FpsVisitor {
            type Value = FpsMode;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an FPS mode ('unlocked', 'stock', 30, 60)")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<FpsMode, E> {
                v.parse().map_err(E::custom)
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<FpsMode, E> {
                match v {
                    30 => Ok(FpsMode::Stock30),
                    60 => Ok(FpsMode::Unlocked),
                    _ => Err(E::custom(format!("unsupported fps: {v} (expected 30 or 60)"))),
                }
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<FpsMode, E> {
                self.visit_i64(v as i64)
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<FpsMode, E> {
                if v {
                    Ok(FpsMode::Unlocked)
                } else {
                    Ok(FpsMode::Stock30)
                }
            }
        }
        d.deserialize_any(FpsVisitor)
    }
}

impl serde::Serialize for FpsMode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            FpsMode::Unlocked => "unlocked",
            FpsMode::Stock30 => "stock",
        })
    }
}

