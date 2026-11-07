use super::{WavError, WavFormat};

const RIFF_AND_PLACEHOLDER: u64 = 12 + 36;
const FACT_LEN: u64 = 12;
const DATA_HEADER_LEN: u64 = 8;

/// Total size in bytes of the header that precedes the audio data.
pub fn header_len(format: &WavFormat) -> u64 {
    let fact = if format.needs_fact() { FACT_LEN } else { 0 };
    RIFF_AND_PLACEHOLDER + 8 + format.fmt_chunk().len() as u64 + fact + DATA_HEADER_LEN
}

fn clamp32(v: u64) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

/// Builds the header for `data_len` bytes of audio. Always `header_len(format)` bytes long.
/// With `final_header` and a file over 4 GiB, an RF64 header is produced.
pub fn build_header(format: &WavFormat, data_len: u64, final_header: bool) -> Vec<u8> {
    let hlen = header_len(format);
    let riff_real = hlen - 8 + data_len + (data_len % 2);
    let frames = data_len / u64::from(format.block_align().max(1));
    let rf64 = final_header && riff_real > u64::from(u32::MAX);

    let mut h = Vec::with_capacity(hlen as usize);
    if rf64 {
        h.extend_from_slice(b"RF64");
        h.extend_from_slice(&u32::MAX.to_le_bytes());
        h.extend_from_slice(b"WAVE");
        h.extend_from_slice(b"ds64");
        h.extend_from_slice(&28u32.to_le_bytes());
        h.extend_from_slice(&riff_real.to_le_bytes());
        h.extend_from_slice(&data_len.to_le_bytes());
        h.extend_from_slice(&frames.to_le_bytes());
        h.extend_from_slice(&0u32.to_le_bytes());
    } else {
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&clamp32(riff_real).to_le_bytes());
        h.extend_from_slice(b"WAVE");
        h.extend_from_slice(b"JUNK");
        h.extend_from_slice(&28u32.to_le_bytes());
        h.extend_from_slice(&[0u8; 28]);
    }
    let fmt = format.fmt_chunk();
    h.extend_from_slice(b"fmt ");
    h.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
    h.extend_from_slice(fmt);
    if format.needs_fact() {
        let v = if rf64 { u32::MAX } else { clamp32(frames) };
        h.extend_from_slice(b"fact");
        h.extend_from_slice(&4u32.to_le_bytes());
        h.extend_from_slice(&v.to_le_bytes());
    }
    let d = if rf64 { u32::MAX } else { clamp32(data_len) };
    h.extend_from_slice(b"data");
    h.extend_from_slice(&d.to_le_bytes());
    debug_assert_eq!(h.len() as u64, hlen);
    h
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedHeader {
    pub format: WavFormat,
    pub data_offset: u64,
    pub data_len: u64,
    pub rf64: bool,
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}

/// Parses a RIFF/RF64 header from the first bytes of a file of `file_len` bytes.
/// Unknown chunks are skipped; a data size that is unset or overruns the file is
/// replaced by the real remaining length, rounded down to whole frames.
pub fn parse_header(bytes: &[u8], file_len: u64) -> Result<ParsedHeader, WavError> {
    if bytes.len() < 12 || &bytes[8..12] != b"WAVE" {
        return Err(WavError::NotWav);
    }
    let rf64 = match &bytes[..4] {
        b"RIFF" => false,
        b"RF64" => true,
        _ => return Err(WavError::NotWav),
    };
    let mut ds64_data: Option<u64> = None;
    let mut format: Option<WavFormat> = None;
    let mut pos = 12usize;
    loop {
        if pos + 8 > bytes.len() {
            return Err(WavError::NotWav);
        }
        let id = &bytes[pos..pos + 4];
        let size = u32_at(bytes, pos + 4);
        let body = pos + 8;
        match id {
            b"data" => {
                let format = format.ok_or(WavError::NotWav)?;
                let data_offset = body as u64;
                let avail = file_len.saturating_sub(data_offset);
                let declared = if rf64 && size == u32::MAX {
                    ds64_data.ok_or(WavError::NotWav)?
                } else {
                    u64::from(size)
                };
                let data_len = if (!rf64 && size == u32::MAX) || declared > avail {
                    avail - avail % u64::from(format.block_align().max(1))
                } else {
                    declared
                };
                return Ok(ParsedHeader { format, data_offset, data_len, rf64 });
            }
            b"fmt " => {
                let end = body + size as usize;
                if end > bytes.len() {
                    return Err(WavError::NotWav);
                }
                format = Some(WavFormat::from_waveformatex_bytes(&bytes[body..end])?);
            }
            b"ds64" if rf64 => {
                if size < 28 || body + 28 > bytes.len() {
                    return Err(WavError::NotWav);
                }
                ds64_data = Some(u64_at(bytes, body + 8));
            }
            _ => {}
        }
        pos = body + size as usize + (size as usize % 2);
    }
}
