use std::fmt;
use std::io;

const TAG_PCM: u16 = 1;
const TAG_FLOAT: u16 = 3;
const TAG_EXTENSIBLE: u16 = 0xFFFE;

const GUID_TAIL: [u8; 12] = [0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71];

#[derive(Debug)]
pub enum WavError {
    Io(io::Error),
    NotWav,
    Unsupported(String),
}

impl fmt::Display for WavError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WavError::Io(e) => write!(f, "error de lectura/escritura: {e}"),
            WavError::NotWav => write!(f, "el archivo no es un WAV válido"),
            WavError::Unsupported(s) => write!(f, "formato no compatible: {s}"),
        }
    }
}

impl std::error::Error for WavError {}

impl From<io::Error> for WavError {
    fn from(e: io::Error) -> Self {
        WavError::Io(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SampleKind {
    U8,
    I16,
    I24,
    I32,
    F32,
    F64,
}

impl SampleKind {
    fn bytes(self) -> usize {
        match self {
            SampleKind::U8 => 1,
            SampleKind::I16 => 2,
            SampleKind::I24 => 3,
            SampleKind::I32 | SampleKind::F32 => 4,
            SampleKind::F64 => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WavFormat {
    fmt: Vec<u8>,
    kind: SampleKind,
}

fn le16(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}

fn le32(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn int_kind(bits: u16) -> Result<SampleKind, WavError> {
    match bits {
        8 => Ok(SampleKind::U8),
        16 => Ok(SampleKind::I16),
        24 => Ok(SampleKind::I24),
        32 => Ok(SampleKind::I32),
        _ => Err(WavError::Unsupported(format!("PCM de {bits} bits"))),
    }
}

fn float_kind(bits: u16) -> Result<SampleKind, WavError> {
    match bits {
        32 => Ok(SampleKind::F32),
        64 => Ok(SampleKind::F64),
        _ => Err(WavError::Unsupported(format!("coma flotante de {bits} bits"))),
    }
}

impl WavFormat {
    /// Parses a WAVEFORMATEX / WAVEFORMATEXTENSIBLE body, keeping its bytes verbatim.
    pub fn from_waveformatex_bytes(bytes: &[u8]) -> Result<WavFormat, WavError> {
        if bytes.len() < 16 {
            return Err(WavError::Unsupported("bloque fmt de menos de 16 bytes".into()));
        }
        let tag = le16(bytes, 0);
        let bits = le16(bytes, 14);
        let (len, kind) = if tag == TAG_PCM {
            (16, int_kind(bits)?)
        } else {
            if bytes.len() < 18 {
                return Err(WavError::Unsupported("al bloque fmt le falta cbSize".into()));
            }
            let len = 18 + le16(bytes, 16) as usize;
            if bytes.len() < len {
                return Err(WavError::Unsupported("bloque fmt más corto que su cbSize".into()));
            }
            let kind = match tag {
                TAG_FLOAT => float_kind(bits)?,
                TAG_EXTENSIBLE => {
                    if len < 40 {
                        return Err(WavError::Unsupported("bloque fmt extensible demasiado corto".into()));
                    }
                    let guid = &bytes[24..40];
                    if guid[4..] != GUID_TAIL {
                        return Err(WavError::Unsupported("GUID de subformato desconocido".into()));
                    }
                    match le32(guid, 0) {
                        1 => int_kind(bits)?,
                        3 => float_kind(bits)?,
                        n => return Err(WavError::Unsupported(format!("subformato {n}"))),
                    }
                }
                other => return Err(WavError::Unsupported(format!("etiqueta de formato {other:#06x}"))),
            };
            (len, kind)
        };
        let (channels, rate, block) = (le16(bytes, 2), le32(bytes, 4), le16(bytes, 12));
        if channels == 0 || rate == 0 {
            return Err(WavError::Unsupported("sin canales o sin frecuencia".into()));
        }
        if usize::from(block) != usize::from(channels) * kind.bytes() {
            return Err(WavError::Unsupported(format!("cuadros de {block} bytes con {channels} canales")));
        }
        Ok(WavFormat { fmt: bytes[..len].to_vec(), kind })
    }

    pub fn float32_extensible(rate: u32, channels: u16) -> WavFormat {
        let block = channels * 4;
        let mask: u32 = match channels {
            1 => 0x4,
            2 => 0x3,
            _ => 0,
        };
        let mut b = Vec::with_capacity(40);
        b.extend_from_slice(&TAG_EXTENSIBLE.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * block as u32).to_le_bytes());
        b.extend_from_slice(&block.to_le_bytes());
        b.extend_from_slice(&32u16.to_le_bytes());
        b.extend_from_slice(&22u16.to_le_bytes());
        b.extend_from_slice(&32u16.to_le_bytes());
        b.extend_from_slice(&mask.to_le_bytes());
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(&GUID_TAIL);
        WavFormat { fmt: b, kind: SampleKind::F32 }
    }

    pub fn pcm(rate: u32, channels: u16, bits: u16) -> WavFormat {
        let block = channels * (bits / 8);
        let mut b = Vec::with_capacity(16);
        b.extend_from_slice(&TAG_PCM.to_le_bytes());
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * block as u32).to_le_bytes());
        b.extend_from_slice(&block.to_le_bytes());
        b.extend_from_slice(&bits.to_le_bytes());
        let kind = int_kind(bits).expect("pcm bits must be 8, 16, 24 or 32");
        WavFormat { fmt: b, kind }
    }

    pub fn fmt_chunk(&self) -> &[u8] {
        &self.fmt
    }

    /// The same format at another sample rate. Only the rate and the byte rate change.
    pub fn with_rate(&self, rate: u32) -> WavFormat {
        let mut fmt = self.fmt.clone();
        fmt[4..8].copy_from_slice(&rate.to_le_bytes());
        fmt[8..12].copy_from_slice(&rate.saturating_mul(u32::from(self.block_align())).to_le_bytes());
        WavFormat { fmt, kind: self.kind }
    }

    pub fn channels(&self) -> u16 {
        le16(&self.fmt, 2)
    }

    pub fn sample_rate(&self) -> u32 {
        le32(&self.fmt, 4)
    }

    pub fn block_align(&self) -> u16 {
        le16(&self.fmt, 12)
    }

    pub fn sample_kind(&self) -> SampleKind {
        self.kind
    }

    pub fn needs_fact(&self) -> bool {
        matches!(self.kind, SampleKind::F32 | SampleKind::F64)
    }

    pub fn describe(&self) -> String {
        let bits = if le16(&self.fmt, 0) == TAG_EXTENSIBLE {
            le16(&self.fmt, 18)
        } else {
            le16(&self.fmt, 14)
        };
        let float = if self.needs_fact() { " float" } else { "" };
        format!("{} Hz · {bits}-bit{float} · {} ch", self.sample_rate(), self.channels())
    }

    pub fn silence_byte(&self) -> u8 {
        if self.kind == SampleKind::U8 { 0x80 } else { 0x00 }
    }

    /// Appends the interleaved samples in `bytes` to `out`, scaled to [-1, 1).
    pub fn decode_f32(&self, bytes: &[u8], out: &mut Vec<f32>) {
        let step = self.kind.bytes();
        out.reserve(bytes.len() / step);
        for s in bytes.chunks_exact(step) {
            out.push(match self.kind {
                SampleKind::U8 => (s[0] as f32 - 128.0) / 128.0,
                SampleKind::I16 => i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0,
                SampleKind::I24 => {
                    let v = i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8;
                    v as f32 / 8_388_608.0
                }
                SampleKind::I32 => {
                    i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0
                }
                SampleKind::F32 => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                SampleKind::F64 => f64::from_le_bytes(s.try_into().unwrap()) as f32,
            });
        }
    }
}
