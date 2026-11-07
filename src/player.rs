//! Playback of a frame range of a WAV file through WASAPI shared mode.
//!
//! The device is opened with the format of the variant: the file's own, except for the rate
//! a pitch changes. Windows rejects a rate other than the device's, so with a pitch the one
//! retry that lets the audio engine convert is the normal path, and without one it is a
//! fallback (playback only; the file is never touched).

use crate::variant::Variant;
use crate::wav::{WavReader, reverse_frames};
use crate::win;
use std::path::Path;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use windows::Win32::Media::Audio::{
    AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_UNSUPPORTED_FORMAT, AUDCLNT_SHAREMODE_SHARED,
    AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY, IAudioClient,
    IAudioRenderClient, WAVEFORMATEX,
};
use windows::Win32::System::Com::CLSCTX_ALL;

/// Device buffer length: 200 ms in 100-ns units.
const BUFFER_HNS: i64 = 2_000_000;
const POLL: Duration = Duration::from_millis(10);
/// How long `Player::start` waits for the device before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Default)]
struct State {
    /// Absolute file frame currently audible. Reversed it is a boundary: the frame heard
    /// next is the one before it, and it can equal the end of the range.
    position: AtomicU64,
    finished: AtomicBool,
    stop: AtomicBool,
    /// Whether reaching the end goes back to the loop start; read on every fill.
    looping: AtomicBool,
    error: Mutex<Option<String>>,
}

/// A running playback of the frames `[from, to)`, or of `[from, to)` and then
/// `[loop_from, to)` over and over while looping. Dropping it stops the sound.
/// Reversed, it walks `[loop_from, from)` backwards and then `[loop_from, to)` backwards.
pub struct Player {
    state: Arc<State>,
    thread: Option<JoinHandle<()>>,
}

impl Player {
    /// Starts playing frames `from..to` of the WAV at `path` on the default output device,
    /// as `variant` says. While looping, reaching `to` goes on at `loop_from` with no gap.
    /// Reversed, `from` is a boundary: the first frame heard is `from - 1`, and the sound
    /// ends (or wraps to `to`) at `loop_from`. `position` is a file frame either way.
    /// Returns once the device is open and playing, or says why it could not.
    /// An empty range is finished at once.
    pub fn start(
        path: &Path,
        from: u64,
        to: u64,
        loop_from: u64,
        looping: bool,
        variant: Variant,
    ) -> Result<Player, String> {
        let fail = |detail: &str| format!("No se pudo iniciar la reproducción ({detail}).");
        let state = Arc::new(State::default());
        state.position.store(from, SeqCst);
        state.looping.store(looping, SeqCst);
        // Nothing to play: done at once. Forward, that is an empty range or one that starts
        // past the end of the file; reversed, a cursor at or before the loop start (nothing
        // lies behind it) or a loop start past the end of the file.
        let frames = WavReader::open(path)
            .map_err(|e| fail(&format!("no se pudo abrir el archivo: {e}")))?
            .frames();
        let end = to.min(frames);
        let nothing = if variant.reversed { from <= loop_from || loop_from >= end } else { from >= end };
        if nothing {
            state.finished.store(true, SeqCst);
            return Ok(Player {
                state,
                thread: None,
            });
        }
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<(), String>>(1);
        let (st, path) = (state.clone(), path.to_path_buf());
        let thread = thread::Builder::new()
            .name("teeth-player".into())
            .spawn(move || player_thread(&st, &path, (from, to, loop_from), variant, ready_tx))
            .map_err(|e| fail(&e.to_string()))?;
        match ready_rx.recv_timeout(READY_TIMEOUT) {
            Ok(Ok(())) => Ok(Player {
                state,
                thread: Some(thread),
            }),
            Ok(Err(detail)) => {
                let _ = thread.join();
                Err(fail(&detail))
            }
            Err(RecvTimeoutError::Timeout) => {
                // The thread sees the flag and releases the device if it ever answers.
                state.stop.store(true, SeqCst);
                Err(fail("el dispositivo de salida no responde"))
            }
            Err(RecvTimeoutError::Disconnected) => {
                let _ = thread.join();
                Err(fail("el hilo de reproducción terminó de forma inesperada"))
            }
        }
    }

    /// Absolute file frame that is audible now, within the played range. Reversed it is a
    /// boundary, so the frame heard next is the one before it, and it can equal the end of
    /// the range.
    pub fn position(&self) -> u64 {
        self.state.position.load(SeqCst)
    }

    /// Turns looping on or off while playing; it takes effect on the next fill.
    pub fn set_looping(&self, on: bool) {
        self.state.looping.store(on, SeqCst);
    }

    /// True once everything was heard, or playback ended early (stop, error, device change).
    pub fn is_finished(&self) -> bool {
        self.state.finished.load(SeqCst)
    }

    /// The error that ended playback early, if any. A device change is not an error.
    pub fn error(&self) -> Option<String> {
        self.state.error.lock().ok().and_then(|e| e.clone())
    }

    /// Stops the sound, releases the device and returns the last position.
    pub fn stop(mut self) -> u64 {
        self.shutdown();
        self.position()
    }

    fn shutdown(&mut self) {
        self.state.stop.store(true, SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.state.finished.store(true, SeqCst);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// A failed step: the text, and whether it was the device going away.
struct Fail {
    msg: String,
    lost: bool,
}

impl Fail {
    fn new(msg: impl Into<String>) -> Fail {
        Fail {
            msg: msg.into(),
            lost: false,
        }
    }
}

/// Adapter for `map_err`: names the failed WASAPI call.
fn wasapi(op: &'static str) -> impl Fn(windows::core::Error) -> Fail {
    move |e| Fail {
        msg: format!("{op}: {e}"),
        lost: e.code() == AUDCLNT_E_DEVICE_INVALIDATED,
    }
}

fn player_thread(
    st: &State,
    path: &Path,
    (from, to, loop_from): (u64, u64, u64),
    variant: Variant,
    ready: SyncSender<Result<(), String>>,
) {
    win::com_init();
    // Declared first, so it drops last: whatever happens below (even a panic), the
    // player ends up finished and COM is released after every COM object is gone.
    let _done = Done(st);
    {
        match Output::open(st, path, (from, to, loop_from), variant) {
            Err(f) => {
                let _ = ready.send(Err(f.msg));
            }
            Ok(mut out) => {
                // A closed channel means `start` gave up waiting: release everything.
                if ready.send(Ok(())).is_ok()
                    && let Err(f) = out.run(st)
                    && !f.lost
                    && let Ok(mut e) = st.error.lock()
                {
                    *e = Some(f.msg);
                }
            }
        }
    }
}

/// Marks playback finished and balances `com_init` when the playback thread ends.
struct Done<'a>(&'a State);

impl Drop for Done<'_> {
    fn drop(&mut self) {
        self.0.finished.store(true, SeqCst);
        win::com_uninit();
    }
}

/// What has been submitted to the device: a first lap `[from, end)`, then `laps` laps
/// `[loop_from, end)`, up to `next`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stream {
    from: u64,
    loop_from: u64,
    /// Exclusive end of every lap, clamped to the file.
    end: u64,
    /// Next absolute frame to submit.
    next: u64,
    /// Laps begun at `loop_from` so far.
    laps: u64,
}

impl Stream {
    /// The next span to submit, `(first frame, length)`, at most `max` frames long. At the
    /// end it goes back to `loop_from` while looping; `None` when nothing is left.
    fn span(&mut self, max: u64, looping: bool) -> Option<(u64, u64)> {
        if self.next >= self.end {
            if !looping || self.loop_from >= self.end {
                return None;
            }
            self.next = self.loop_from;
            self.laps += 1;
        }
        Some((self.next, max.min(self.end - self.next)))
    }

    /// The frame audible while the last `queued` frames submitted wait in the device. They
    /// may reach back into earlier laps.
    fn heard(&self, queued: u64) -> u64 {
        let lap_start = if self.laps == 0 { self.from } else { self.loop_from };
        let into = self.next.saturating_sub(lap_start);
        if queued <= into {
            return self.next - queued;
        }
        // Frames before the start of the current lap; at least one.
        let mut back = queued - into;
        let lap = self.end.saturating_sub(self.loop_from);
        if self.laps > 1 && lap > 0 {
            let earlier = (self.laps - 1).saturating_mul(lap);
            if back <= earlier {
                return self.end - ((back - 1) % lap + 1);
            }
            back -= earlier;
        }
        self.end.saturating_sub(back).max(self.from)
    }
}

/// The first file frame of the chunk `(at, len)` of a stream. Reversed, the stream counts
/// in the order the sound is heard, and a chunk is the file frames `[start, start + len)`
/// read backwards; `mirror` is `loop_from + end`, so every lap stays in `[loop_from, end)`.
fn file_start(mirror: Option<u64>, at: u64, len: u64) -> u64 {
    mirror.map_or(at, |m| m.saturating_sub(at + len))
}

/// The file frame (a boundary, reversed) for a stream position that was heard.
fn file_position(mirror: Option<u64>, heard: u64) -> u64 {
    mirror.map_or(heard, |m| m.saturating_sub(heard))
}

/// An initialized, started render stream and its source file.
struct Output {
    client: IAudioClient,
    render: IAudioRenderClient,
    reader: WavReader,
    buffer_frames: u32,
    block_align: usize,
    /// `Some` when playing reversed; see `file_start`. Fixed at open, whatever `end` does.
    mirror: Option<u64>,
    stream: Stream,
}

impl Drop for Output {
    fn drop(&mut self) {
        // SAFETY: stopping a live client; at teardown the result does not matter.
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

impl Output {
    fn open(
        st: &State,
        path: &Path,
        (from, to, loop_from): (u64, u64, u64),
        variant: Variant,
    ) -> Result<Output, Fail> {
        let reader = WavReader::open(path)
            .map_err(|e| Fail::new(format!("no se pudo abrir el archivo: {e}")))?;
        let block_align = usize::from(reader.format().block_align());
        if block_align == 0 {
            return Err(Fail::new("formato no admitido: nBlockAlign es 0"));
        }
        // WAVEFORMATEX needs its cbSize field; plain PCM stores only the first 16 bytes.
        let mut wfx = variant.format(reader.format()).fmt_chunk().to_vec();
        if wfx.len() == 16 {
            wfx.extend_from_slice(&[0, 0]);
        }
        let device = win::default_render_device().map_err(wasapi("GetDefaultAudioEndpoint"))?;
        let pwfx = wfx.as_ptr().cast::<WAVEFORMATEX>();
        let init = |flags: u32| -> windows::core::Result<IAudioClient> {
            // SAFETY: COM is initialized on this thread; `wfx` holds a complete WAVEFORMATEX
            // (header plus cbSize extension bytes) and outlives the call.
            unsafe {
                let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
                client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER_HNS, 0, pwfx, None)?;
                Ok(client)
            }
        };
        let client = match init(0) {
            Ok(c) => c,
            // A failed client cannot be initialized again, so `init` activates a new one.
            Err(e) if e.code() == AUDCLNT_E_UNSUPPORTED_FORMAT => {
                init(AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY)
                    .map_err(wasapi("Initialize (con conversión)"))?
            }
            Err(e) => return Err(wasapi("Initialize")(e)),
        };
        // SAFETY: COM calls on the initialized client.
        let (render, buffer_frames) = unsafe {
            let render: IAudioRenderClient = client.GetService().map_err(wasapi("GetService"))?;
            let frames = client.GetBufferSize().map_err(wasapi("GetBufferSize"))?;
            (render, frames)
        };
        let end = to.min(reader.frames());
        let mirror = variant.reversed.then_some(loop_from + end);
        let from = file_position(mirror, from.min(end));
        let mut out = Output {
            client,
            render,
            reader,
            buffer_frames,
            block_align,
            mirror,
            stream: Stream { from, loop_from, end, next: from, laps: 0 },
        };
        out.fill(st.looping.load(SeqCst))?;
        // SAFETY: starting an initialized client; `out` stops it on drop.
        unsafe { out.client.Start() }.map_err(wasapi("Start"))?;
        Ok(out)
    }

    /// Submits as many frames as fit in the free part of the buffer, going back to the
    /// loop start at the end while `looping`. Returns the padding (frames queued, not yet
    /// played) afterwards.
    fn fill(&mut self, looping: bool) -> Result<u32, Fail> {
        // SAFETY: COM call on a live client.
        let padding = unsafe { self.client.GetCurrentPadding() }.map_err(wasapi("GetCurrentPadding"))?;
        let mut free = u64::from(self.buffer_frames.saturating_sub(padding));
        let mut bytes = Vec::new();
        while free > 0 {
            let Some((at, len)) = self.stream.span(free, looping) else { break };
            let chunk = self
                .reader
                .read_frames(file_start(self.mirror, at, len), len)
                .map_err(|e| Fail::new(format!("lectura del archivo: {e}")))?;
            let got = (chunk.len() / self.block_align) as u64;
            if got == 0 {
                // The file is shorter than its header claims: end where the data ends.
                self.stream.end = at;
                break;
            }
            let from_byte = bytes.len();
            bytes.extend_from_slice(&chunk[..got as usize * self.block_align]);
            if self.mirror.is_some() {
                reverse_frames(&mut bytes[from_byte..], self.block_align);
            }
            self.stream.next = at + got;
            free -= got;
        }
        let got = (bytes.len() / self.block_align) as u32;
        if got == 0 {
            return Ok(padding);
        }
        // SAFETY: COM calls on a live client; the buffer holds the `got` frames requested,
        // is written within that size and released right away.
        unsafe {
            let dst = self.render.GetBuffer(got).map_err(wasapi("GetBuffer"))?;
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
            self.render
                .ReleaseBuffer(got, 0)
                .map_err(wasapi("ReleaseBuffer"))?;
        }
        Ok(padding + got)
    }

    /// Feeds the device every 10 ms until the range was heard, stop, or an error.
    fn run(&mut self, st: &State) -> Result<(), Fail> {
        loop {
            let padding = u64::from(self.fill(st.looping.load(SeqCst))?);
            st.position.store(file_position(self.mirror, self.stream.heard(padding)), SeqCst);
            if self.stream.next >= self.stream.end && padding == 0 {
                st.position.store(file_position(self.mirror, self.stream.end), SeqCst);
                return Ok(());
            }
            if st.stop.load(SeqCst) {
                return Ok(());
            }
            thread::sleep(POLL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(from: u64, loop_from: u64, end: u64) -> Stream {
        Stream { from, loop_from, end, next: from, laps: 0 }
    }

    /// Submits up to `n` frames the way `Output::fill` does; returns the spans.
    fn submit(s: &mut Stream, mut n: u64, looping: bool) -> Vec<(u64, u64)> {
        let mut spans = Vec::new();
        while n > 0 {
            let Some((at, len)) = s.span(n, looping) else { break };
            spans.push((at, len));
            s.next = at + len;
            n -= len;
        }
        spans
    }

    /// The file frames heard for the first `n` frames submitted, in the order of sound.
    /// For a reversed stream (`m` is `Some`): every chunk is read backwards.
    fn heard_frames(s: &mut Stream, m: Option<u64>, n: u64, looping: bool) -> Vec<u64> {
        let mut frames = Vec::new();
        for (at, len) in submit(s, n, looping) {
            let start = file_start(m, at, len);
            frames.extend((start..start + len).rev());
        }
        frames
    }

    #[test]
    fn without_looping_the_stream_stops_at_the_end() {
        let mut s = stream(500, 100, 1_000);
        assert_eq!(submit(&mut s, 300, false), vec![(500, 300)]);
        assert_eq!(submit(&mut s, 9_600, false), vec![(800, 200)]);
        assert_eq!(s.span(9_600, false), None);
        assert_eq!((s.next, s.laps), (1_000, 0));
    }

    #[test]
    fn looping_wraps_from_the_end_to_the_loop_start_without_a_gap() {
        let mut s = stream(500, 100, 1_000);
        assert_eq!(submit(&mut s, 1_500, true), vec![(500, 500), (100, 900), (100, 100)]);
        assert_eq!((s.next, s.laps), (200, 2));
    }

    #[test]
    fn a_lap_shorter_than_the_buffer_repeats_within_one_fill() {
        let mut s = stream(105, 100, 110);
        assert_eq!(submit(&mut s, 28, true), vec![(105, 5), (100, 10), (100, 10), (100, 3)]);
        assert_eq!((s.next, s.laps), (103, 3));
    }

    #[test]
    fn looping_turned_off_mid_lap_ends_at_the_end() {
        let mut s = stream(500, 100, 1_000);
        submit(&mut s, 700, true);
        assert_eq!(submit(&mut s, 9_600, false), vec![(300, 700)]);
        assert_eq!(s.span(9_600, false), None);
    }

    #[test]
    fn heard_frame_counts_back_within_the_current_lap() {
        let mut s = stream(500, 100, 1_000);
        submit(&mut s, 300, true);
        assert_eq!(s.heard(0), 800);
        assert_eq!(s.heard(300), 500);
    }

    #[test]
    fn frames_still_queued_from_the_previous_lap_are_heard_first() {
        let mut s = stream(500, 100, 1_000);
        submit(&mut s, 700, true); // 500 of the first lap, then 100..300
        assert_eq!(s.heard(200), 100);
        assert_eq!(s.heard(201), 999);
        assert_eq!(s.heard(250), 950);
        assert_eq!(s.heard(700), 500);
    }

    #[test]
    fn heard_frame_walks_back_over_many_short_laps() {
        let mut s = stream(105, 100, 110);
        submit(&mut s, 48, true); // 5 frames of the first lap, four laps of 10, then 100..103
        assert_eq!((s.next, s.laps), (103, 5));
        assert_eq!(s.heard(3), 100);
        assert_eq!(s.heard(4), 109);
        assert_eq!(s.heard(13), 100);
        assert_eq!(s.heard(14), 109);
        assert_eq!(s.heard(43), 100);
        assert_eq!(s.heard(44), 109);
        assert_eq!(s.heard(48), 105);
        assert_eq!(s.heard(60), 105);
    }

    #[test]
    fn reverse_reads_each_lap_backwards_and_wraps_without_a_gap() {
        // Selection [100, 110), cursor at 105.
        let m = Some(210);
        let mut s = stream(file_position(m, 105), 100, 110);
        assert_eq!(
            heard_frames(&mut s, m, 18, true),
            [104, 103, 102, 101, 100, 109, 108, 107, 106, 105, 104, 103, 102, 101, 100, 109, 108, 107]
        );
    }

    #[test]
    fn the_reversed_position_walks_back_and_ends_on_the_start_handle() {
        let m = Some(210);
        let mut s = stream(file_position(m, 105), 100, 110);
        submit(&mut s, 3, false);
        assert_eq!(file_position(m, s.heard(0)), 102);
        assert_eq!(file_position(m, s.heard(3)), 105);
        assert_eq!(file_position(m, s.end), 100);
        assert_eq!(file_position(None, 105), 105);
    }

    #[test]
    fn a_reversed_loop_position_reaches_back_into_the_previous_lap() {
        let m = Some(210);
        let mut s = stream(file_position(m, 110), 100, 110);
        submit(&mut s, 13, true); // one lap, then 3 frames of the next
        assert_eq!(file_position(m, s.heard(3)), 110);
        assert_eq!(file_position(m, s.heard(4)), 101);
    }

    #[test]
    fn one_frame_and_last_frame_selections_stay_in_range() {
        // One frame, the last of a take of 1000.
        let m = Some(1_999);
        let mut s = stream(file_position(m, 1_000), 999, 1_000);
        assert_eq!(heard_frames(&mut s, m, 3, true), [999, 999, 999]);
        // The whole take: starts on the last frame, ends on the first.
        let m = Some(1_000);
        let mut s = stream(file_position(m, 1_000), 0, 1_000);
        let frames = heard_frames(&mut s, m, 1_000, false);
        assert_eq!((frames[0], frames[999]), (999, 0));
    }
}
