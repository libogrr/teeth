//! Writer thread loop: drains capture events into the WAV, the peak cache and shared counters.

use crate::peaks::PeakCache;
use crate::wav::WavWriter;
use std::io::{self, Seek, Write};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;

/// Frames of zeros fed to the peak cache per lock, so long gaps never allocate much.
const SILENCE_PEAK_CHUNK_FRAMES: u64 = 4096;

pub enum Packet {
    Audio(Vec<u8>),
    Silence { frames: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub enum EndReason {
    Stopped,
    DeviceChanged,
    DeviceLost,
    CaptureError(String),
    DiskError(String),
}

pub enum CaptureEvent {
    Packet { packet: Packet, discontinuity: bool },
    End(EndReason),
}

/// State shared between the capture thread, the writer thread and the UI.
pub struct Shared {
    pub frames: AtomicU64,
    pub discontinuities: AtomicU32,
    pub queued_frames: AtomicU64,
    pub stop: AtomicBool,
    pub peaks: Mutex<PeakCache>,
    /// Where each counted cut starts, in frames, in order.
    pub cuts: Mutex<Vec<u64>>,
}

impl Shared {
    pub fn new(channels: usize) -> Shared {
        Shared {
            frames: AtomicU64::new(0),
            discontinuities: AtomicU32::new(0),
            queued_frames: AtomicU64::new(0),
            stop: AtomicBool::new(false),
            peaks: Mutex::new(PeakCache::new(channels)),
            cuts: Mutex::new(Vec::new()),
        }
    }
}

/// Consumes events until `End` or a closed channel, then finishes the WAV.
/// A write error raises `shared.stop`, finishes best-effort and reports `DiskError`.
pub fn run_writer<W: Write + Seek>(
    rx: Receiver<CaptureEvent>,
    mut writer: WavWriter<W>,
    shared: &Shared,
) -> (io::Result<W>, EndReason) {
    let rate = u64::from(writer.format().sample_rate().max(1));
    let channels = usize::from(writer.format().channels());
    let mut first = true;
    let mut since_header = 0u64;
    let mut samples: Vec<f32> = Vec::new();

    let reason = loop {
        let (packet, discontinuity) = match rx.recv() {
            Ok(CaptureEvent::Packet { packet, discontinuity }) => (packet, discontinuity),
            Ok(CaptureEvent::End(reason)) => break reason,
            Err(_) => break EndReason::Stopped,
        };
        if discontinuity && !first {
            shared.discontinuities.fetch_add(1, Ordering::SeqCst);
            shared.cuts.lock().unwrap().push(writer.frames());
        }
        first = false;

        let before = writer.frames();
        let written = match packet {
            Packet::Audio(bytes) => {
                let res = writer.write_audio(&bytes);
                if res.is_ok() {
                    samples.clear();
                    writer.format().decode_f32(&bytes, &mut samples);
                    // The cache only accepts whole frames.
                    samples.truncate(samples.len() / channels.max(1) * channels.max(1));
                    shared.peaks.lock().unwrap().push(&samples);
                }
                res
            }
            Packet::Silence { frames } => {
                let res = writer.write_silence(frames);
                if res.is_ok() {
                    let mut left = frames;
                    while left > 0 {
                        let n = left.min(SILENCE_PEAK_CHUNK_FRAMES);
                        samples.clear();
                        samples.resize(n as usize * channels, 0.0);
                        shared.peaks.lock().unwrap().push(&samples);
                        left -= n;
                    }
                }
                res
            }
        };
        let step = writer.frames() - before;

        let result = written.and_then(|()| {
            since_header += step;
            if since_header >= rate {
                since_header = 0;
                writer.update_header()?;
            }
            Ok(())
        });
        if let Err(e) = result {
            shared.stop.store(true, Ordering::SeqCst);
            return (writer.finish(), EndReason::DiskError(e.to_string()));
        }

        shared.frames.fetch_add(step, Ordering::SeqCst);
        let _ = shared
            .queued_frames
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |q| Some(q.saturating_sub(step)));
    };

    (writer.finish(), reason)
}
