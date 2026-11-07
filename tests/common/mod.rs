#![allow(dead_code)]

pub fn u16le(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes(b[off..off + 2].try_into().unwrap())
}

pub fn u32le(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes(b[off..off + 4].try_into().unwrap())
}

pub fn u64le(b: &[u8], off: usize) -> u64 {
    u64::from_le_bytes(b[off..off + 8].try_into().unwrap())
}

/// Deterministic xorshift64 generator for reproducible test signals.
pub struct XorShift(u64);

impl XorShift {
    pub fn new(seed: u64) -> Self {
        XorShift(if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed })
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// Uniform value in [-1.5, 1.5].
    pub fn next_f32(&mut self) -> f32 {
        let unit = (self.next_u64() >> 40) as f32 / ((1u64 << 24) - 1) as f32; // [0, 1]
        unit * 3.0 - 1.5
    }
}

pub fn f32_bytes(samples: &[f32]) -> Vec<u8> {
    samples.iter().flat_map(|s| s.to_le_bytes()).collect()
}

/// In-memory `Write + Seek` whose bytes stay readable from the test while a writer owns it.
#[derive(Clone)]
pub struct SharedCursor(pub std::sync::Arc<std::sync::Mutex<std::io::Cursor<Vec<u8>>>>);

impl SharedCursor {
    pub fn new() -> Self {
        SharedCursor(std::sync::Arc::new(std::sync::Mutex::new(std::io::Cursor::new(Vec::new()))))
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap().get_ref().clone()
    }
}

impl std::io::Write for SharedCursor {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::io::Seek for SharedCursor {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.lock().unwrap().seek(pos)
    }
}

/// `Write + Seek` that fails once the total written would exceed `limit` bytes.
pub struct FailAfter {
    pub limit: usize,
    written: usize,
}

impl FailAfter {
    pub fn new(limit: usize) -> Self {
        FailAfter { limit, written: 0 }
    }
}

impl std::io::Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.written + buf.len() > self.limit {
            return Err(std::io::Error::other("disk full"));
        }
        self.written += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::io::Seek for FailAfter {
    fn seek(&mut self, _pos: std::io::SeekFrom) -> std::io::Result<u64> {
        Ok(0)
    }
}
