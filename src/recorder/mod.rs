//! Recorder: wires the capture thread to the writer thread.

pub mod writer_loop;

use crate::capture::{CaptureSource, start_capture};
use crate::files::{new_temp_path, now_local};
use crate::peaks::PeakCache;
use crate::wav::{WavFormat, WavWriter};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc::channel;
use std::thread::JoinHandle;
use writer_loop::{EndReason, Shared, run_writer};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecStatus {
    pub frames: u64,
    pub discontinuities: u32,
    pub backlog_secs: f32,
}

pub struct RecordedTake {
    pub path: PathBuf,
    pub format: WavFormat,
    pub frames: u64,
    pub discontinuities: u32,
    /// Where each cut starts, in frames.
    pub cuts: Vec<u64>,
    pub peaks: PeakCache,
    pub end: EndReason,
}

type WriterResult = (io::Result<std::fs::File>, EndReason);

pub struct Recorder {
    shared: Arc<Shared>,
    format: WavFormat,
    device_name: String,
    volume_independent: bool,
    path: PathBuf,
    capture: JoinHandle<()>,
    writer: JoinHandle<WriterResult>,
}

impl Recorder {
    /// Starts recording `source` into a new temp file in `dir`.
    pub fn start(dir: &Path, source: CaptureSource) -> Result<Recorder, String> {
        let shared = Arc::new(Shared::new(0));
        let (tx, rx) = channel();
        let info = start_capture(tx, shared.clone(), source)?;
        *shared.peaks.lock().unwrap() = PeakCache::new(usize::from(info.format.channels()));

        // A path nobody uses yet, so the failure paths below may delete it.
        let path = new_temp_path(dir, &now_local());
        let wav = match WavWriter::create(&path, &info.format) {
            Ok(w) => w,
            Err(e) => {
                shared.stop.store(true, SeqCst);
                let _ = info.thread.join();
                // The file may exist with a partial header; left behind, it would be offered
                // for recovery at every start.
                let _ = std::fs::remove_file(&path);
                return Err(format!(
                    "No se pudo crear el archivo temporal {}: {e}",
                    path.display()
                ));
            }
        };

        let writer_shared = shared.clone();
        let writer = std::thread::Builder::new()
            .name("teeth-writer".into())
            .spawn(move || run_writer(rx, wav, &writer_shared));
        let writer = match writer {
            Ok(h) => h,
            Err(e) => {
                shared.stop.store(true, SeqCst);
                let _ = info.thread.join();
                // The writer (and its file handle) went down with the closure.
                let _ = std::fs::remove_file(&path);
                return Err(format!("No se pudo iniciar el hilo de escritura: {e}"));
            }
        };

        Ok(Recorder {
            shared,
            format: info.format,
            device_name: info.device_name,
            volume_independent: info.volume_independent,
            path,
            capture: info.thread,
            writer,
        })
    }

    pub fn format(&self) -> &WavFormat {
        &self.format
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// False when Windows lacks process loopback and the take follows the Windows volume.
    pub fn volume_independent(&self) -> bool {
        self.volume_independent
    }

    pub fn status(&self) -> RecStatus {
        let rate = self.format.sample_rate().max(1) as f32;
        RecStatus {
            frames: self.shared.frames.load(SeqCst),
            discontinuities: self.shared.discontinuities.load(SeqCst),
            backlog_secs: self.shared.queued_frames.load(SeqCst) as f32 / rate,
        }
    }

    pub fn shared(&self) -> &Shared {
        &self.shared
    }

    /// True once the writer thread has finished (stopped, device change/loss, or disk error).
    pub fn has_ended(&self) -> bool {
        self.writer.is_finished()
    }

    pub fn stop(self) -> Result<RecordedTake, String> {
        self.shared.stop.store(true, SeqCst);
        let capture = self.capture.join();
        let writer = self.writer.join();
        if capture.is_err() {
            return Err("El hilo de captura terminó con un error inesperado".into());
        }
        let (finished, mut end) = writer
            .map_err(|_| "El hilo de escritura terminó con un error inesperado".to_string())?;
        if let Err(e) = finished
            && !matches!(end, EndReason::DiskError(_))
        {
            end = EndReason::DiskError(e.to_string());
        }
        let peaks = std::mem::replace(&mut *self.shared.peaks.lock().unwrap(), PeakCache::new(0));
        Ok(RecordedTake {
            path: self.path,
            format: self.format,
            frames: self.shared.frames.load(SeqCst),
            discontinuities: self.shared.discontinuities.load(SeqCst),
            cuts: std::mem::take(&mut *self.shared.cuts.lock().unwrap()),
            peaks,
            end,
        })
    }
}
