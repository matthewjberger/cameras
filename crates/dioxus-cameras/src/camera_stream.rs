//! High-level Dioxus hook that drives a single preview stream: opens a
//! camera, runs a [`cameras::pump::Pump`], and exposes the lifecycle as
//! signals.

use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use cameras::pump::{self, Pump};
use cameras::{CameraSource, Frame, StreamConfig, open_source, source_label};
use dioxus::prelude::*;

use crate::channel::Channel;
use crate::registry::{Registry, get_or_create_sink, publish_frame, remove_sink};

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The lifecycle state surfaced by [`use_camera_stream`].
///
/// `label` is a human-readable identifier for the source (device name or RTSP
/// URL). Implements [`fmt::Display`] so you can format it directly into a
/// string: `format!("{status}")`.
#[derive(Clone, Debug)]
pub enum StreamStatus {
    /// No source is set.
    Idle,
    /// A source is set; [`cameras::open_source`] is running on a background
    /// thread.
    Connecting {
        /// Human-readable label of the source being opened.
        label: String,
    },
    /// The camera is open and the pump is active (or paused but ready).
    Streaming {
        /// Human-readable label of the active source.
        label: String,
    },
    /// The last connect attempt failed.
    ///
    /// Match on `error` (a typed [`cameras::Error`]) if you need to branch on
    /// the failure kind, for example, distinguishing
    /// [`Error::PermissionDenied`](cameras::Error::PermissionDenied) from
    /// [`Error::DeviceInUse`](cameras::Error::DeviceInUse).
    Failed {
        /// The error returned by [`cameras::open_source`].
        error: cameras::Error,
    },
}

impl PartialEq for StreamStatus {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (StreamStatus::Idle, StreamStatus::Idle) => true,
            (StreamStatus::Connecting { label: a }, StreamStatus::Connecting { label: b }) => {
                a == b
            }
            (StreamStatus::Streaming { label: a }, StreamStatus::Streaming { label: b }) => a == b,
            (StreamStatus::Failed { error: a }, StreamStatus::Failed { error: b }) => {
                a.to_string() == b.to_string()
            }
            _ => false,
        }
    }
}

impl fmt::Display for StreamStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StreamStatus::Idle => f.write_str("Idle"),
            StreamStatus::Connecting { label } => write!(f, "Connecting to {label}..."),
            StreamStatus::Streaming { label } => write!(f, "Streaming: {label}"),
            StreamStatus::Failed { error } => write!(f, "Open failed: {error}"),
        }
    }
}

/// Handle returned by [`use_camera_stream`].
///
/// All fields are public signals / callbacks, data-oriented, no methods,
/// no hidden state. Read `status`, write `active`, call `capture_frame`
/// directly.
#[derive(Copy, Clone, PartialEq)]
pub struct UseCameraStream {
    /// Lifecycle of the stream. See [`StreamStatus`].
    pub status: Signal<StreamStatus>,
    /// Whether the pump actively streams frames to the preview.
    ///
    /// Default `true`. Setting to `false` parks the pump, no more
    /// `cameras::next_frame` calls, no per-frame Rust work, but keeps the
    /// camera handle open so `capture_frame` remains fast. Toggle based on
    /// whether the preview is currently visible to the user.
    pub active: Signal<bool>,
    /// Grab a single fresh frame on demand.
    ///
    /// Works regardless of `active`. Returns `None` if the stream is not
    /// connected or the camera errored.
    ///
    /// # UI-thread blocking
    ///
    /// This callback **blocks the calling thread** until the pump worker
    /// replies, typically one frame interval (~16-33ms at 30-60fps), plus
    /// up to 20ms of wake latency if the pump is paused. When called from an
    /// `onclick` (which runs on the UI thread), expect a brief UI stall.
    ///
    /// For single photo-button captures this is imperceptible. For rapid
    /// back-to-back captures, dispatch the call from a
    /// [`spawn_blocking`](std::thread::spawn)'d worker or Dioxus
    /// [`spawn`](dioxus::prelude::spawn) task so the UI thread stays
    /// responsive.
    pub capture_frame: Callback<(), Option<Frame>>,
}

/// Hook that drives a single preview stream end-to-end.
///
/// Given a stream `id`, a `source` signal, and a [`StreamConfig`], the hook:
///
/// 1. Watches `source` for changes.
/// 2. Opens the camera on a background thread (so the UI does not block).
/// 3. Starts a [`cameras::pump::Pump`] that publishes frames to the
///    [`Registry`] slot for `id`.
/// 4. Reports progress through [`UseCameraStream::status`].
/// 5. Tears down the previous camera automatically when `source` changes.
/// 6. Cleans up the [`Registry`] entry and pump when the component unmounts:
///    the consumer never has to call [`remove_sink`] manually.
///
/// Pair with a [`StreamPreview`](crate::StreamPreview) element bound to the
/// same `id` to render the frames on-screen. Use
/// [`UseCameraStream::capture_frame`] for on-demand snapshots, and
/// [`UseCameraStream::active`] to pause the pump when the preview is hidden.
///
/// # Reconnect semantics
///
/// **Every write to `source` triggers a reconnect**, even if the new value
/// equals the current one. Dioxus signals notify subscribers unconditionally.
/// To avoid redundant reconnects, gate the write yourself:
/// [`CameraSource`] implements [`PartialEq`]:
///
/// ```ignore
/// if source.peek().as_ref() != Some(&next) {
///     source.set(Some(next));
/// }
/// ```
///
/// Rapid back-to-back source changes (A → B → A before any of them finishes
/// connecting) will spawn one connect thread per change. Cameras doesn't
/// expose cancellation, so each [`open_source`] call runs to completion; an
/// internal generation counter then discards stale results so only the
/// latest source wins. The orphaned threads consume CPU until they finish
/// but can never corrupt state.
///
/// # Panics
///
/// Panics at render time if called outside an app wired up with
/// [`register_with`](crate::register_with).
pub fn use_camera_stream(
    id: u32,
    source: Signal<Option<CameraSource>>,
    config: StreamConfig,
) -> UseCameraStream {
    let registry = try_consume_context::<Registry>().expect(
        "`use_camera_stream` requires `register_with` to be called at launch, \
         see the dioxus-cameras crate docs",
    );

    let mut status = use_signal(|| StreamStatus::Idle);
    let active = use_signal(|| true);

    let pump_cell = use_hook(|| Rc::new(RefCell::new(None::<Pump>)));
    let channel = use_hook(Channel::<StreamEvent>::new);
    let generation = use_hook(|| Arc::new(AtomicU64::new(0)));

    {
        let registry = registry.clone();
        use_drop(move || remove_sink(&registry, id));
    }

    {
        let pump_cell = Rc::clone(&pump_cell);
        use_effect(move || {
            let is_active = *active.read();
            if let Some(pump_ref) = pump_cell.borrow().as_ref() {
                pump::set_active(pump_ref, is_active);
            }
        });
    }

    {
        let channel = channel.clone();
        let generation = Arc::clone(&generation);
        let pump_cell = Rc::clone(&pump_cell);
        use_hook(move || {
            spawn(async move {
                loop {
                    futures_timer::Delay::new(POLL_INTERVAL).await;
                    let events = channel.drain();
                    if events.is_empty() {
                        continue;
                    }
                    let current = generation.load(Ordering::Relaxed);
                    for event in events {
                        if event.generation != current {
                            continue;
                        }
                        match event.payload {
                            StreamEventPayload::Connected {
                                pump: new_pump,
                                label,
                            } => {
                                pump::set_active(&new_pump, *active.peek());
                                *pump_cell.borrow_mut() = Some(new_pump);
                                status.set(StreamStatus::Streaming { label });
                            }
                            StreamEventPayload::Failed { error } => {
                                status.set(StreamStatus::Failed { error });
                            }
                        }
                    }
                }
            })
        });
    }

    let capture_frame = {
        let pump_cell = Rc::clone(&pump_cell);
        use_callback(move |()| pump_cell.borrow().as_ref().and_then(pump::capture_frame))
    };

    {
        let effect_tx = channel.sender.clone();
        let effect_generation = Arc::clone(&generation);
        let pump_cell = Rc::clone(&pump_cell);
        let registry = registry.clone();
        use_effect(move || {
            let requested = source.read().clone();
            let generation_value = effect_generation.fetch_add(1, Ordering::Relaxed) + 1;

            *pump_cell.borrow_mut() = None;

            let Some(requested) = requested else {
                status.set(StreamStatus::Idle);
                return;
            };

            let label = source_label(&requested);
            status.set(StreamStatus::Connecting {
                label: label.clone(),
            });

            let tx = effect_tx.clone();
            let registry = registry.clone();
            let _ = std::thread::Builder::new()
                .name("cameras-connect".into())
                .spawn(move || {
                    let payload = match open_source(requested, config) {
                        Ok(camera) => {
                            let sink = get_or_create_sink(&registry, id);
                            let pump =
                                pump::spawn(camera, move |frame| publish_frame(&sink, frame));
                            StreamEventPayload::Connected { pump, label }
                        }
                        Err(error) => StreamEventPayload::Failed { error },
                    };
                    let _ = tx.send(StreamEvent {
                        generation: generation_value,
                        payload,
                    });
                });
        });
    }

    UseCameraStream {
        status,
        active,
        capture_frame,
    }
}

struct StreamEvent {
    generation: u64,
    payload: StreamEventPayload,
}

enum StreamEventPayload {
    Connected { pump: Pump, label: String },
    Failed { error: cameras::Error },
}
