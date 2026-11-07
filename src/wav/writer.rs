use super::{WavFormat, build_header};
use std::fs::File;
use std::io::{self, Seek, SeekFrom, Write};
use std::path::Path;

const SILENCE_CHUNK: usize = 64 * 1024;

/// Streams audio bytes into a WAV file, rewriting the header on demand.
pub struct WavWriter<W: Write + Seek> {
    out: W,
    format: WavFormat,
    data_len: u64,
}

impl WavWriter<File> {
    pub fn create(path: &Path, format: &WavFormat) -> io::Result<WavWriter<File>> {
        WavWriter::new(File::create(path)?, format)
    }
}

impl<W: Write + Seek> WavWriter<W> {
    /// Writes the provisional header and positions the writer at the start of the data.
    pub fn new(mut out: W, format: &WavFormat) -> io::Result<WavWriter<W>> {
        out.write_all(&build_header(format, 0, false))?;
        Ok(WavWriter { out, format: format.clone(), data_len: 0 })
    }

    pub fn write_audio(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.out.write_all(bytes)?;
        self.data_len += bytes.len() as u64;
        Ok(())
    }

    pub fn write_silence(&mut self, frames: u64) -> io::Result<()> {
        let mut remaining = frames * u64::from(self.format.block_align());
        let chunk = vec![self.format.silence_byte(); SILENCE_CHUNK.min(remaining as usize)];
        while remaining > 0 {
            let n = remaining.min(chunk.len() as u64) as usize;
            self.write_audio(&chunk[..n])?;
            remaining -= n as u64;
        }
        Ok(())
    }

    pub fn data_len(&self) -> u64 {
        self.data_len
    }

    pub fn frames(&self) -> u64 {
        self.data_len / u64::from(self.format.block_align().max(1))
    }

    pub fn format(&self) -> &WavFormat {
        &self.format
    }

    fn write_header(&mut self, final_header: bool) -> io::Result<()> {
        self.out.seek(SeekFrom::Start(0))?;
        self.out.write_all(&build_header(&self.format, self.data_len, final_header))
    }

    /// Rewrites the header with the current length and returns to the end of the file.
    pub fn update_header(&mut self) -> io::Result<()> {
        self.write_header(false)?;
        self.out.seek(SeekFrom::End(0))?;
        Ok(())
    }

    /// Pads odd data, writes the final header and flushes.
    pub fn finish(mut self) -> io::Result<W> {
        if self.data_len % 2 == 1 {
            self.out.write_all(&[0])?;
        }
        self.write_header(true)?;
        self.out.flush()?;
        Ok(self.out)
    }
}
