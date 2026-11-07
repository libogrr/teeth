use super::{WavError, WavFormat, build_header, header_len, parse_header};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

const HEAD_READ: u64 = 64 * 1024;

fn read_head(file: &mut File) -> Result<(Vec<u8>, u64), WavError> {
    let file_len = file.metadata()?.len();
    let mut head = Vec::new();
    file.seek(SeekFrom::Start(0))?;
    (&*file).take(HEAD_READ).read_to_end(&mut head)?;
    Ok((head, file_len))
}

/// Random-access reader over the audio data of a WAV file.
pub struct WavReader {
    file: Option<File>,
    format: WavFormat,
    data_offset: u64,
    data_len: u64,
}

impl WavReader {
    pub fn open(path: &Path) -> Result<WavReader, WavError> {
        let mut file = File::open(path)?;
        let (head, file_len) = read_head(&mut file)?;
        let p = parse_header(&head, file_len)?;
        Ok(WavReader { file: Some(file), format: p.format, data_offset: p.data_offset, data_len: p.data_len })
    }

    pub fn format(&self) -> &WavFormat {
        &self.format
    }

    pub fn frames(&self) -> u64 {
        self.data_len / u64::from(self.format.block_align().max(1))
    }

    pub fn data_offset(&self) -> u64 {
        self.data_offset
    }

    pub fn data_len(&self) -> u64 {
        self.data_len
    }

    /// Reads `count` frames from `start`, clipped to the end of the data.
    pub fn read_frames(&mut self, start: u64, count: u64) -> io::Result<Vec<u8>> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("reader released"))?;
        let ba = u64::from(self.format.block_align().max(1));
        let total = self.data_len / ba;
        let start = start.min(total);
        let count = count.min(total - start);
        file.seek(SeekFrom::Start(self.data_offset + start * ba))?;
        let mut buf = vec![0u8; (count * ba) as usize];
        file.read_exact(&mut buf)?;
        Ok(buf)
    }

    /// Appends the decoded samples of the requested frames to `out`.
    pub fn read_f32(&mut self, start: u64, count: u64, out: &mut Vec<f32>) -> io::Result<()> {
        let bytes = self.read_frames(start, count)?;
        self.format.decode_f32(&bytes, out);
        Ok(())
    }

    /// Closes the file so it can be moved or replaced; later reads fail.
    pub fn release(&mut self) {
        self.file = None;
    }
}

/// Format and whole-frame data length of a file of `file_len` bytes that starts with
/// `head`, when its header can be rewritten in place.
fn repair_plan(head: &[u8], file_len: u64) -> Result<(WavFormat, u64), WavError> {
    let p = parse_header(head, file_len)?;
    if p.data_offset != header_len(&p.format) {
        return Err(WavError::Unsupported("la cabecera no se puede reescribir en su sitio".into()));
    }
    let ba = u64::from(p.format.block_align().max(1));
    let avail = file_len - p.data_offset;
    Ok((p.format, avail - avail % ba))
}

/// The final header for the audio really present in a file of `file_len` bytes that
/// starts with `head`: whole frames only, RF64 past 4 GiB, as `WavWriter::finish` writes
/// it. Returns the header and the data length. The header has the same length as the
/// existing one, so it can replace it in place.
pub fn repaired_header(head: &[u8], file_len: u64) -> Result<(Vec<u8>, u64), WavError> {
    let (format, data_len) = repair_plan(head, file_len)?;
    Ok((build_header(&format, data_len, true), data_len))
}

/// Closes a file left unfinished (a crash, or a failed `finish`) the way `finish` would:
/// drops a trailing partial frame, adds the pad byte after odd data and writes the final
/// header computed from the real file length. Returns the number of frames.
pub fn repair(path: &Path) -> Result<u64, WavError> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let (head, file_len) = read_head(&mut file)?;
    let (format, data_len) = repair_plan(&head, file_len)?;
    let end = header_len(&format) + data_len;
    file.set_len(end)?;
    if data_len % 2 == 1 {
        file.seek(SeekFrom::Start(end))?;
        file.write_all(&[0])?;
    }
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&build_header(&format, data_len, true))?;
    file.flush()?;
    Ok(data_len / u64::from(format.block_align().max(1)))
}
