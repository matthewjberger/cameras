//! Long-running background pump that pulls frames from a [`Camera`] and hands
//! each one to a caller-provided sink closure.
//!
//! The pump owns the camera, so all frame I/O is serialized through a single
//! worker thread. Callers can:
//! - Pause and resume streaming without closing the camera ([`set_active`]).
//! - Grab a single fresh frame on demand whether the pump is streaming or
//!   paused ([`capture_frame`]). The camera stays warm so latency is one
//!   frame interval plus up to 20ms of pause-wake.
//! - Stop the pump deterministically ([`stop_and_join`]) or let the
//!   [`Pump`]'s `Drop` tear it down asynchronously.
//!
//! # Pause semantics
//!
//! [`set_active(false)`](set_active) eliminates *Rust-side* per-frame work:
//! no more [`next_frame`] calls, no sink invocations, effectively zero CPU
//! for the pump thread. The OS-level camera pipeline, however, keeps
//! running: AVFoundation still delivers sample buffers, Media Foundation's
//! source reader still decodes, V4L2 still DMAs frames into userspace, and
//! they land in cameras' bounded internal channel and get dropped when it
//! overflows.
//!
//! On AC power this OS-side cost is typically <5% of one core for 1080p30,
//! and negligible. On battery it is measurable; if that matters, close the
//! [`Camera`] entirely (drop the [`Pump`]). Truly stopping the OS pipeline
//! without closing the device would require a separate primitive at this
//! layer (AVFoundation `stopRunning`, MF source-reader flush, V4L2
//! `STREAMOFF`); not provided today.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender, SyncSender};
use std::thread::{JoinHandle, sleep};
use std::time::Duration;

use crate::{Camera, DEFAULT_FRAME_TIMEOUT, Error, Frame, next_frame};

const PAUSED_POLL_INTERVAL: Duration = Duration::from_millis(20);
const COMMAND_QUEUE_CAPACITY: usize = 16;

/// A running camera pump.
///
/// Obtained from [`spawn`]. The struct holds private worker state; interact
/// with it through the free functions in this module.
pub struct Pump {
    pub(crate) worker: Option<JoinHandle<()>>,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) commands: SyncSender<PumpCommand>,
}

impl Drop for Pump {
    fn drop(&mut self) {
        if self.worker.is_some() {
            self.shutdown.store(true, Ordering::Relaxed);
        }
    }
}

pub(crate) enum PumpCommand {
    Capture { reply: Sender<Option<Frame>> },
}

/// Spawn a worker thread that pulls frames from `camera` and hands each one
/// to `on_frame`.
///
/// The pump starts in the active state (streaming). The worker stops when the
/// returned [`Pump`] is dropped, [`stop_and_join`] is called, or the camera
/// reports a non-timeout error.
pub fn spawn<F>(camera: Camera, mut on_frame: F) -> Pump
where
    F: FnMut(Frame) + Send + 'static,
{
    let shutdown = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicBool::new(true));
    let (command_tx, command_rx) = mpsc::sync_channel::<PumpCommand>(COMMAND_QUEUE_CAPACITY);

    let shutdown_for_worker = Arc::clone(&shutdown);
    let active_for_worker = Arc::clone(&active);
    let worker = std::thread::Builder::new()
        .name("cameras-pump".into())
        .spawn(move || {
            let camera = camera;
            loop {
                if shutdown_for_worker.load(Ordering::Relaxed) {
                    break;
                }

                let mut handled_command = false;
                while let Ok(command) = command_rx.try_recv() {
                    match command {
                        PumpCommand::Capture { reply } => {
                            let frame = match next_frame(&camera, DEFAULT_FRAME_TIMEOUT) {
                                Ok(frame) => {
                                    on_frame(frame.clone());
                                    Some(frame)
                                }
                                Err(_) => None,
                            };
                            let _ = reply.send(frame);
                        }
                    }
                    handled_command = true;
                }
                if handled_command {
                    continue;
                }

                if !active_for_worker.load(Ordering::Relaxed) {
                    sleep(PAUSED_POLL_INTERVAL);
                    continue;
                }

                match next_frame(&camera, DEFAULT_FRAME_TIMEOUT) {
                    Ok(frame) => on_frame(frame),
                    Err(Error::Timeout) => continue,
                    Err(_) => break,
                }
            }
        })
        .expect("failed to spawn cameras pump thread");

    Pump {
        worker: Some(worker),
        shutdown,
        active,
        commands: command_tx,
    }
}

/// Toggle whether the pump actively streams frames to its sink.
///
/// - `true` (the default): worker pulls frames continuously and calls the
///   sink for each one.
/// - `false`: worker parks. No [`next_frame`] calls, no sink invocations,
///   no per-frame Rust work. The camera handle stays open so
///   [`capture_frame`] remains fast.
pub fn set_active(pump: &Pump, active: bool) {
    pump.active.store(active, Ordering::Relaxed);
}

/// Request a single fresh frame from the pump.
///
/// Works whether the pump is active or paused. The request is queued to the
/// worker thread, which pulls one frame via [`next_frame`], hands it to the
/// sink (so any attached listener also receives it), and returns it.
///
/// Blocks the calling thread until the worker replies, typically one frame
/// interval, plus up to 20ms of wake latency if the pump is paused.
///
/// Returns `None` if the command queue is full, the worker has shut down, or
/// the camera errored while reading.
pub fn capture_frame(pump: &Pump) -> Option<Frame> {
    let (reply_tx, reply_rx) = mpsc::channel();
    pump.commands
        .try_send(PumpCommand::Capture { reply: reply_tx })
        .ok()?;
    reply_rx.recv().ok().flatten()
}

/// Consume the pump, signal the worker to stop, and block until it has
/// exited.
///
/// Use when you need to guarantee the camera has released its handle before
/// returning, for example before re-opening the same device. Equivalent to
/// `drop(pump)` plus an explicit join.
pub fn stop_and_join(mut pump: Pump) {
    pump.shutdown.store(true, Ordering::Relaxed);
    if let Some(worker) = pump.worker.take() {
        let _ = worker.join();
    }
}
