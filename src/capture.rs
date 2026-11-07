// Parts of this file are derived from ONCExDEV/system-audio-recorder, src/capture.rs
// (https://github.com/ONCExDEV/system-audio-recorder): the silent render stream that keeps
// loopback packets flowing (`start_keepalive`), the packet drain loop (`drain`), stopping
// both clients on drop, and polling the default device ID. Its format handling, which
// converts to 48 kHz stereo 16-bit, is not used: Teeth captures the native mix format.
//
// MIT License
//
// Copyright (c) 2026 System Audio Recorder contributors
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

//! WASAPI capture of what the PC plays, in the default output device's own mix format.
//!
//! Teeth captures with process loopback, which hears the chosen processes before the
//! Windows master volume. Where Windows refuses it (builds before process loopback), it
//! falls back to the loopback of the default output device, which on some devices comes
//! after the volume; `CaptureInfo::volume_independent` says which one runs.
//!
//! Either stream is opened with exactly the device's `GetMixFormat()` and no conversion
//! flags; packet bytes are copied untouched. A silent render stream on the same device
//! keeps packets arriving while nothing plays, so silence is recorded as real time.
//!
//! A packet is a cut when Windows flags a discontinuity or when its QPC position does not
//! follow the previous packet. The second is how process loopback shows the audio it
//! loses, unflagged, each time a captured stream starts or stops.

use crate::recorder::writer_loop::{CaptureEvent, EndReason, Packet, Shared};
use crate::wav::WavFormat;
use crate::win;
use std::ffi::c_void;
use std::fmt;
use std::mem::ManuallyDrop;
use std::sync::Arc;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::mpsc::{self, RecvTimeoutError, Sender, SyncSender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, E_NOINTERFACE, ERROR_TIMEOUT, HANDLE};
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
    AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS,
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation,
    IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl,
    IAudioCaptureClient, IAudioClient, IAudioRenderClient, IMMDevice,
    PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
    PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
    WAVEFORMATEX,
};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Com::{CLSCTX_ALL, CoTaskMemFree};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;
use windows::core::{HRESULT, IUnknown, Interface, Ref};

/// Buffer length for both streams: 200 ms in 100-ns units.
const BUFFER_HNS: i64 = 2_000_000;
/// Polling interval of the endpoint-loopback fallback, which has no event.
const POLL: Duration = Duration::from_millis(10);
/// Longest wait for the process-loopback event before draining and checking anyway.
const EVENT_TIMEOUT_MS: u32 = 100;
const DEVICE_POLL: Duration = Duration::from_millis(500);
/// How long `start_capture` waits for the device before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(10);
/// How long process-loopback activation may take before the fallback is used. Shorter
/// than `READY_TIMEOUT`, so the fallback still has time to open.
const ACTIVATE_TIMEOUT: Duration = Duration::from_secs(5);
const SILENT: u32 = AUDCLNT_BUFFERFLAGS_SILENT.0 as u32;
const DISCONTINUITY: u32 = AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY.0 as u32;

/// Largest mismatch, in 100-ns units, between where a packet starts by its QPC position
/// and where the previous one ended before it counts as a cut: 2 ms.
const GAP_HNS: u64 = 20_000;

/// Follows the QPC position of each packet to find time the stream skipped.
///
/// Process loopback loses audio when a stream it captures starts or stops: it delivers
/// zeros, then jumps ahead in time, with no `AUDCLNT_BUFFERFLAGS_DATA_DISCONTINUITY`. The
/// jump shows as a packet that does not start where the previous one ended.
struct Timeline {
    rate: u64,
    /// QPC position, in 100-ns units, where the last packet ended.
    end: Option<u64>,
}

impl Timeline {
    fn new(rate: u32) -> Timeline {
        Timeline { rate: u64::from(rate.max(1)), end: None }
    }

    /// Records a packet of `frames` frames whose first frame is at `qpc`. True when it
    /// starts more than `GAP_HNS` away from the end of the previous one. A zero position
    /// (none reported) starts over.
    fn is_gap(&mut self, qpc: u64, frames: u64) -> bool {
        if qpc == 0 {
            self.end = None;
            return false;
        }
        let gap = self.end.is_some_and(|end| qpc.abs_diff(end) > GAP_HNS);
        self.end = Some(qpc + frames * 10_000_000 / self.rate);
        gap
    }
}

/// Whose sound a take records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSource {
    /// Every process except Teeth's own process tree, so Teeth never records its playback.
    SystemExceptSelf,
    /// Only the process tree of this process ID.
    Process(u32),
}

impl CaptureSource {
    /// Process-loopback target and mode; `own_pid` is the ID of the calling process.
    fn loopback_params(self, own_pid: u32) -> AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
        let (pid, mode) = match self {
            CaptureSource::SystemExceptSelf => {
                (own_pid, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)
            }
            CaptureSource::Process(pid) => (pid, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE),
        };
        AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
            TargetProcessId: pid,
            ProcessLoopbackMode: mode,
        }
    }
}

/// Whether a failed process loopback may fall back to the device's own loopback: only for
/// the whole system, which the device loopback also records.
fn may_fall_back(source: CaptureSource) -> bool {
    source == CaptureSource::SystemExceptSelf
}

/// Whether a Windows build has process loopback. Microsoft documents build 20348, but it
/// works from Windows 10 2004 (build 19041), where OBS enables its app capture too; a
/// refusal is still caught when the recording starts.
fn single_app_build(build: u32) -> bool {
    build >= 19041
}

/// How the error of a capture that could not start begins, before `({detalle}).`
pub const CAPTURE_FAILED: &str = "No se pudo iniciar la captura de audio";

/// Whether this Windows can record a single app.
pub fn process_loopback_supported() -> bool {
    let mut info = OSVERSIONINFOW { dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32, ..Default::default() };
    // SAFETY: `info` is a correctly sized, writable OSVERSIONINFOW.
    let status = unsafe { RtlGetVersion(&mut info) };
    status.is_ok() && single_app_build(info.dwBuildNumber)
}

/// Activation parameters of a process-loopback client for `source`.
fn activation_params(source: CaptureSource, own_pid: u32) -> AUDIOCLIENT_ACTIVATION_PARAMS {
    AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
            ProcessLoopbackParams: source.loopback_params(own_pid),
        },
    }
}

/// A `VT_BLOB` PROPVARIANT whose blob is `params`. Keep `params` alive and in place while
/// the result is in use.
///
/// It is wrapped in `ManuallyDrop` because `PROPVARIANT`'s `Drop` runs `PropVariantClear`,
/// which would hand `params` (Rust memory, not a COM allocation) to `CoTaskMemFree` and
/// corrupt the heap. Nothing in it needs freeing.
fn blob_propvariant(params: &mut AUDIOCLIENT_ACTIVATION_PARAMS) -> ManuallyDrop<PROPVARIANT> {
    let mut value = ManuallyDrop::new(PROPVARIANT::default());
    // SAFETY: `value` is a zeroed PROPVARIANT owned here; this sets its type tag and the
    // blob member that the tag selects.
    unsafe {
        let inner = &mut *value.Anonymous.Anonymous;
        inner.vt = VT_BLOB;
        inner.Anonymous.blob.cbSize = size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32;
        inner.Anonymous.blob.pBlobData = (params as *mut AUDIOCLIENT_ACTIVATION_PARAMS).cast();
    }
    value
}

#[derive(Debug)]
pub struct CaptureInfo {
    pub format: WavFormat,
    pub device_name: String,
    /// True with process loopback (captured before the Windows volume); false on the
    /// endpoint-loopback fallback, whose level may follow the volume.
    pub volume_independent: bool,
    pub thread: JoinHandle<()>,
}

/// What the capture thread reports once both streams run.
struct Started {
    format: WavFormat,
    device_name: String,
    volume_independent: bool,
}

type Ready = Result<Started, String>;

/// Starts capturing `source` on a dedicated thread, in the default output device's format.
///
/// Returns once the streams are open and run, with the mix format, the device name and
/// whether the capture is independent of the Windows volume. Packets then flow into `tx`
/// until `shared.stop`, a device change or an error; the thread always finishes with one
/// `CaptureEvent::End`.
pub fn start_capture(
    tx: Sender<CaptureEvent>,
    shared: Arc<Shared>,
    source: CaptureSource,
) -> Result<CaptureInfo, String> {
    let fail = |detail: &str| format!("{CAPTURE_FAILED} ({detail}).");
    let (ready_tx, ready_rx) = mpsc::sync_channel::<Ready>(1);
    let thread = thread::Builder::new()
        .name("teeth-capture".into())
        .spawn(move || capture_thread(tx, shared, source, ready_tx))
        .map_err(|e| fail(&e.to_string()))?;
    match ready_rx.recv_timeout(READY_TIMEOUT) {
        Ok(Ok(started)) => Ok(CaptureInfo {
            format: started.format,
            device_name: started.device_name,
            volume_independent: started.volume_independent,
            thread,
        }),
        Ok(Err(detail)) => {
            let _ = thread.join();
            Err(fail(&detail))
        }
        // The thread sees the closed channel and releases the device if it ever answers.
        Err(RecvTimeoutError::Timeout) => Err(fail("el dispositivo de salida no responde")),
        Err(RecvTimeoutError::Disconnected) => {
            let _ = thread.join();
            Err(fail("el hilo de captura terminó de forma inesperada"))
        }
    }
}

fn capture_thread(
    tx: Sender<CaptureEvent>,
    shared: Arc<Shared>,
    source: CaptureSource,
    ready: SyncSender<Ready>,
) {
    // The MTA: process-loopback activation completes on a worker thread, and this thread
    // pumps no messages.
    win::com_init();
    win::raise_to_pro_audio();
    match Loopback::open(source) {
        Err(detail) => {
            let _ = ready.send(Err(detail));
        }
        Ok((loopback, started)) => {
            if ready.send(Ok(started)).is_ok() {
                let reason = loopback.run(&tx, &shared);
                // Stop and release both clients before announcing the end.
                drop(loopback);
                let _ = tx.send(CaptureEvent::End(reason));
            }
        }
    }
    win::com_uninit();
}

/// A failed WASAPI call: which one, and the HRESULT.
struct Failure {
    op: &'static str,
    err: windows::core::Error,
}

impl Failure {
    fn end_reason(&self) -> EndReason {
        if self.err.code() == AUDCLNT_E_DEVICE_INVALIDATED {
            EndReason::DeviceLost
        } else {
            EndReason::CaptureError(self.to_string())
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.op, self.err)
    }
}

impl From<Failure> for String {
    fn from(f: Failure) -> String {
        f.to_string()
    }
}

trait Op<T> {
    fn op(self, op: &'static str) -> Result<T, Failure>;
}

impl<T> Op<T> for windows::core::Result<T> {
    fn op(self, op: &'static str) -> Result<T, Failure> {
        self.map_err(|err| Failure { op, err })
    }
}

/// Mix format allocated by WASAPI, freed with `CoTaskMemFree` when dropped.
struct MixFormat(*mut WAVEFORMATEX);

impl Drop for MixFormat {
    fn drop(&mut self) {
        // SAFETY: the pointer came from GetMixFormat and is freed exactly once.
        unsafe { CoTaskMemFree(Some(self.0 as *const c_void)) };
    }
}

/// An auto-reset event, closed when dropped.
struct Event(HANDLE);

impl Event {
    fn new() -> Result<Event, Failure> {
        // SAFETY: no security attributes and no name; the handle is owned by the result.
        unsafe { CreateEventW(None, false, false, None) }
            .map(Event)
            .op("CreateEventW")
    }
}

impl Drop for Event {
    fn drop(&mut self) {
        // SAFETY: a handle from CreateEventW, closed exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Signals the waiting capture thread that `ActivateAudioInterfaceAsync` finished.
/// `implement` makes it agile, as that call requires: it runs on a worker thread.
#[windows_core::implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(SyncSender<()>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(
        &self,
        _operation: Ref<IActivateAudioInterfaceAsyncOperation>,
    ) -> windows::core::Result<()> {
        // A waiter that gave up has dropped the receiver; then nobody needs the signal.
        let _ = self.0.try_send(());
        Ok(())
    }
}

/// Activates a process-loopback client for `source` and initializes it, event-driven, with
/// the untouched mix format: no AUTOCONVERTPCM, no SRC flags.
///
/// # Safety
/// COM must be initialized on this thread in the MTA, and `mix` must point to a valid mix
/// format.
unsafe fn process_loopback(
    source: CaptureSource,
    mix: *const WAVEFORMATEX,
) -> Result<(IAudioClient, Event), Failure> {
    // `std::process::id` is `GetCurrentProcessId`.
    let mut params = activation_params(source, std::process::id());
    let blob = blob_propvariant(&mut params);
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(done_tx).into();
    // SAFETY: static device path; `blob` and the `params` it points to outlive the call,
    // which copies them. `handler` is agile and stays alive until this function returns.
    let operation = unsafe {
        ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*blob as *const PROPVARIANT),
            &handler,
        )
    }
    .op("ActivateAudioInterfaceAsync")?;
    if done_rx.recv_timeout(ACTIVATE_TIMEOUT).is_err() {
        return Err(Failure {
            op: "ActivateAudioInterfaceAsync (sin respuesta)",
            err: ERROR_TIMEOUT.to_hresult().into(),
        });
    }
    let mut activated = HRESULT(0);
    let mut unknown: Option<IUnknown> = None;
    // SAFETY: the operation completed; valid out pointers.
    unsafe { operation.GetActivateResult(&mut activated, &mut unknown) }
        .op("GetActivateResult")?;
    activated.ok().op("Activación de la captura por aplicación")?;
    let client: IAudioClient = unknown
        .ok_or_else(|| windows::core::Error::from(E_NOINTERFACE))
        .and_then(|u| u.cast())
        .op("IAudioClient (captura por aplicación)")?;
    let event = Event::new()?;
    // SAFETY: `mix` is valid per the contract above; `event` outlives the client in
    // `Loopback`, which drops it last.
    unsafe {
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                BUFFER_HNS,
                0,
                mix,
                None,
            )
            .op("Initialize (captura por aplicación)")?;
        client.SetEventHandle(event.0).op("SetEventHandle")?;
    }
    Ok((client, event))
}

/// The fallback: loopback of `device` itself, initialized with the untouched mix format
/// (no AUTOCONVERTPCM, no SRC flags) and polled.
///
/// # Safety
/// COM must be initialized on this thread, and `mix` must point to a valid mix format.
unsafe fn endpoint_loopback(
    device: &IMMDevice,
    mix: *const WAVEFORMATEX,
) -> Result<IAudioClient, Failure> {
    // SAFETY: per the contract above.
    unsafe {
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None).op("Activate")?;
        client
            .Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                AUDCLNT_STREAMFLAGS_LOOPBACK,
                BUFFER_HNS,
                0,
                mix,
                None,
            )
            .op("Initialize (loopback)")?;
        Ok(client)
    }
}

/// The capture stream and the silent render stream of one take.
/// Dropping it stops both clients, then releases them.
struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    keepalive: IAudioClient,
    render: IAudioRenderClient,
    render_frames: u32,
    block_align: usize,
    sample_rate: u32,
    device_id: Option<String>,
    /// The process-loopback client's packet event; `None` on the endpoint-loopback
    /// fallback, which polls. Declared last so it is closed after the client is released.
    event: Option<Event>,
}

impl Drop for Loopback {
    fn drop(&mut self) {
        // SAFETY: stopping live clients; at teardown their result does not matter.
        unsafe {
            let _ = self.client.Stop();
            let _ = self.keepalive.Stop();
        }
    }
}

impl Loopback {
    /// Opens the capture of `source` and the silent stream on the default output device,
    /// both with its mix format, and starts them.
    fn open(source: CaptureSource) -> Result<(Loopback, Started), String> {
        let device = win::default_render_device().op("GetDefaultAudioEndpoint")?;
        let device_id = win::device_id(&device);
        let device_name =
            win::device_name(&device).unwrap_or_else(|| "Salida de audio predeterminada".into());
        // SAFETY: COM is initialized on this thread (in the MTA, see `capture_thread`).
        // `mix` stays alive until every client is initialized with it, and its guard frees
        // it on every path.
        let (lb, format) = unsafe {
            // Process loopback has no mix format of its own: the device's is the format.
            let keepalive: IAudioClient = device.Activate(CLSCTX_ALL, None).op("Activate")?;
            let mix = MixFormat(keepalive.GetMixFormat().op("GetMixFormat")?);
            let format = WavFormat::from_waveformatex_bytes(&win::mix_format_bytes(mix.0))
                .map_err(|e| format!("formato de mezcla no admitido: {e}"))?;
            let block_align = usize::from(format.block_align());
            if block_align == 0 {
                return Err("formato de mezcla no admitido: nBlockAlign es 0".into());
            }
            keepalive
                .Initialize(AUDCLNT_SHAREMODE_SHARED, 0, BUFFER_HNS, 0, mix.0, None)
                .op("Initialize (flujo silencioso)")?;
            let render: IAudioRenderClient =
                keepalive.GetService().op("GetService (flujo silencioso)")?;
            let render_frames = keepalive.GetBufferSize().op("GetBufferSize")?;
            // With the whole system, any failure of process loopback (a Windows build
            // without it, or a refusal) falls back to the device's own loopback; `event`
            // tells them apart later. One chosen app never falls back: the device would
            // record everything else too.
            let (client, event) = match process_loopback(source, mix.0) {
                Ok((client, event)) => (client, Some(event)),
                Err(_) if may_fall_back(source) => (endpoint_loopback(&device, mix.0)?, None),
                Err(e) => return Err(e.into()),
            };
            let capture: IAudioCaptureClient = client.GetService().op("GetService (captura)")?;
            let lb = Loopback {
                client,
                capture,
                keepalive,
                render,
                render_frames,
                block_align,
                sample_rate: format.sample_rate(),
                device_id,
                event,
            };
            (lb, format)
        };
        lb.refill_silence()?;
        // SAFETY: starting initialized clients; `lb` stops them on drop if a call fails.
        unsafe {
            lb.keepalive.Start().op("Start (flujo silencioso)")?;
            lb.client.Start().op("Start (loopback)")?;
        }
        let started = Started {
            format,
            device_name,
            volume_independent: lb.event.is_some(),
        };
        Ok((lb, started))
    }

    /// Captures until stop, device change or error.
    fn run(&self, tx: &Sender<CaptureEvent>, shared: &Shared) -> EndReason {
        let mut last_check = Instant::now();
        let mut timeline = Timeline::new(self.sample_rate);
        loop {
            self.wait();
            let drained = self.refill_silence().and_then(|()| self.drain(tx, shared, &mut timeline));
            let listening = match drained {
                Ok(listening) => listening,
                Err(f) => return f.end_reason(),
            };
            // Stop is checked right after a drain, so the take keeps audio up to that moment.
            // Without a listener nobody wants the packets either.
            if !listening || shared.stop.load(SeqCst) {
                return EndReason::Stopped;
            }
            if last_check.elapsed() >= DEVICE_POLL {
                last_check = Instant::now();
                if self.default_device_changed() {
                    return EndReason::DeviceChanged;
                }
            }
        }
    }

    /// Waits for packets: the process-loopback event, at most `EVENT_TIMEOUT_MS`, or `POLL`
    /// on the endpoint-loopback fallback.
    fn wait(&self) {
        match &self.event {
            // SAFETY: a live event handle owned by `self`. Timeout or failure alike just
            // lead to the next drain.
            Some(event) => unsafe {
                let _ = WaitForSingleObject(event.0, EVENT_TIMEOUT_MS);
            },
            None => thread::sleep(POLL),
        }
    }

    /// Tops the silent render stream up to a full buffer of silence.
    ///
    /// Being a stream on the device itself, it also reports AUDCLNT_E_DEVICE_INVALIDATED
    /// when the device goes away or its format changes, which process loopback, not opened
    /// on the device, may not report.
    fn refill_silence(&self) -> Result<(), Failure> {
        // SAFETY: COM calls on live, initialized clients. The buffer is released as
        // silent, so its contents are never read and need not be written.
        unsafe {
            let padding = self.keepalive.GetCurrentPadding().op("GetCurrentPadding")?;
            let free = self.render_frames.saturating_sub(padding);
            if free > 0 {
                self.render
                    .GetBuffer(free)
                    .op("GetBuffer (flujo silencioso)")?;
                self.render
                    .ReleaseBuffer(free, SILENT)
                    .op("ReleaseBuffer (flujo silencioso)")?;
            }
        }
        Ok(())
    }

    /// Sends every pending packet, flagging as a discontinuity the ones Windows marks and
    /// the ones `timeline` finds out of time. `Ok(false)` means the receiver is gone.
    fn drain(
        &self,
        tx: &Sender<CaptureEvent>,
        shared: &Shared,
        timeline: &mut Timeline,
    ) -> Result<bool, Failure> {
        loop {
            // SAFETY: COM call on a live, initialized capture client.
            if unsafe { self.capture.GetNextPacketSize() }.op("GetNextPacketSize")? == 0 {
                return Ok(true);
            }
            let mut data: *mut u8 = std::ptr::null_mut();
            let (mut n, mut flags, mut qpc) = (0u32, 0u32, 0u64);
            // SAFETY: valid out pointers; the device position is not requested.
            unsafe {
                self.capture
                    .GetBuffer(&mut data, &mut n, &mut flags, None, Some(&mut qpc))
            }
            .op("GetBuffer")?;
            let packet = if flags & SILENT != 0 || data.is_null() {
                Packet::Silence {
                    frames: u64::from(n),
                }
            } else {
                // SAFETY: until ReleaseBuffer, `data` holds `n` frames of the mix format the
                // stream was initialized with, i.e. `n * block_align` readable bytes.
                let bytes =
                    unsafe { std::slice::from_raw_parts(data, n as usize * self.block_align) };
                Packet::Audio(bytes.to_vec())
            };
            // SAFETY: releases exactly the frames obtained above (0 is always allowed).
            unsafe { self.capture.ReleaseBuffer(n) }.op("ReleaseBuffer")?;
            if n == 0 {
                return Ok(true);
            }
            shared.queued_frames.fetch_add(u64::from(n), SeqCst);
            let skipped = timeline.is_gap(qpc, u64::from(n));
            let discontinuity = flags & DISCONTINUITY != 0 || skipped;
            if tx
                .send(CaptureEvent::Packet {
                    packet,
                    discontinuity,
                })
                .is_err()
            {
                return Ok(false);
            }
        }
    }

    /// True when Windows now reports a different default output device.
    fn default_device_changed(&self) -> bool {
        let Some(start) = &self.device_id else {
            return false;
        };
        let now = win::default_render_device()
            .ok()
            .and_then(|d| win::device_id(&d));
        now.is_some_and(|id| id != *start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::Audio::PROCESS_LOOPBACK_MODE;

    fn target(params: &AUDIOCLIENT_ACTIVATION_PARAMS) -> (u32, PROCESS_LOOPBACK_MODE) {
        assert_eq!(
            params.ActivationType,
            AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK
        );
        // SAFETY: process-loopback activation selects this union member.
        let p = unsafe { params.Anonymous.ProcessLoopbackParams };
        (p.TargetProcessId, p.ProcessLoopbackMode)
    }

    #[test]
    fn single_app_capture_needs_windows_10_2004() {
        assert!(!single_app_build(18363));
        assert!(single_app_build(19041));
        assert!(single_app_build(19045));
        assert!(single_app_build(26300));
    }

    #[test]
    fn only_the_whole_system_may_fall_back_to_the_device() {
        assert!(may_fall_back(CaptureSource::SystemExceptSelf));
        assert!(!may_fall_back(CaptureSource::Process(42)));
    }

    #[test]
    fn system_except_self_excludes_the_own_process_tree() {
        let params = activation_params(CaptureSource::SystemExceptSelf, 4321);
        assert_eq!(
            target(&params),
            (4321, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE)
        );
    }

    #[test]
    fn process_includes_only_that_process_tree() {
        let params = activation_params(CaptureSource::Process(77), 4321);
        assert_eq!(
            target(&params),
            (77, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE)
        );
    }

    /// QPC units (100 ns) per millisecond.
    const MS: u64 = 10_000;

    #[test]
    fn back_to_back_packets_are_not_gaps() {
        let mut t = Timeline::new(48_000);
        assert!(!t.is_gap(1_000 * MS, 480));
        assert!(!t.is_gap(1_010 * MS, 480));
        assert!(!t.is_gap(1_020 * MS, 480));
    }

    #[test]
    fn packets_whose_length_is_not_whole_milliseconds_are_not_gaps() {
        // 512 frames at 48 kHz last 10.6667 ms; the engine stamps them at that pace.
        let mut t = Timeline::new(48_000);
        for i in 0..100u64 {
            let qpc = 1_000 * MS + i * 512 * 10_000_000 / 48_000;
            assert!(!t.is_gap(qpc, 512), "packet {i}");
        }
    }

    #[test]
    fn a_packet_that_starts_after_the_previous_ended_is_a_gap() {
        let mut t = Timeline::new(48_000);
        t.is_gap(1_000 * MS, 480);
        assert!(t.is_gap(1_020 * MS, 480));
    }

    #[test]
    fn a_packet_that_starts_before_the_previous_ended_is_a_gap() {
        let mut t = Timeline::new(48_000);
        t.is_gap(1_000 * MS, 480);
        assert!(t.is_gap(1_005 * MS, 480));
    }

    #[test]
    fn jitter_up_to_two_milliseconds_is_not_a_gap() {
        let mut t = Timeline::new(48_000);
        t.is_gap(1_000 * MS, 480);
        assert!(!t.is_gap(1_012 * MS, 480));
        assert!(!t.is_gap(1_020 * MS, 480));
    }

    #[test]
    fn timing_resumes_from_the_packet_after_a_gap() {
        let mut t = Timeline::new(48_000);
        t.is_gap(1_000 * MS, 480);
        assert!(t.is_gap(1_030 * MS, 480));
        assert!(!t.is_gap(1_040 * MS, 480));
    }

    #[test]
    fn a_missing_position_is_never_a_gap() {
        let mut t = Timeline::new(48_000);
        t.is_gap(1_000 * MS, 480);
        assert!(!t.is_gap(0, 480));
        assert!(!t.is_gap(2_000 * MS, 480));
    }

    #[test]
    fn blob_points_at_the_params() {
        let mut params = activation_params(CaptureSource::Process(9), 1);
        let value = blob_propvariant(&mut params);
        // SAFETY: `blob_propvariant` set the tag to VT_BLOB, which selects `blob`.
        let (vt, size, data) = unsafe {
            let inner = &value.Anonymous.Anonymous;
            (inner.vt, inner.Anonymous.blob.cbSize, inner.Anonymous.blob.pBlobData)
        };
        assert_eq!(vt, VT_BLOB);
        assert_eq!(size as usize, size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>());
        assert_eq!(data.cast::<AUDIOCLIENT_ACTIVATION_PARAMS>(), &raw mut params);
        // `value` is ManuallyDrop: leaving scope must not PropVariantClear the blob.
    }
}
