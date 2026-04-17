//! Shared per-stream frame state: [`Registry`] owns a map of id → [`LatestFrame`],
//! and [`LatestFrame`] holds the most recent [`Frame`] for one stream.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cameras::Frame;

use crate::poison::recover_lock;

/// Crate-internal abstraction over anything the preview server can read
/// frames from.
///
/// Kept private so [`crate::server`] depends on the trait rather than on
/// [`Registry`] directly; lets the server switch to a different backing
/// store (or `Arc<dyn FrameSource>`) later without public-API churn.
pub(crate) trait FrameSource: Send + Sync + 'static {
    fn snapshot(&self, id: u32) -> Option<(Frame, u32)>;
}

/// A shared map from stream id to [`LatestFrame`].
///
/// Cheap to clone, internally it holds an `Arc`, so the same registry is
/// shared across Dioxus contexts, background threads, and HTTP handlers.
///
/// Obtained from [`crate::PreviewServer`]. Users don't typically construct
/// one directly; instead they read it from context via `use_context::<Registry>()`
/// (which [`crate::PreviewServer`] registers via
/// [`register_with`](crate::register_with)).
#[derive(Clone, Default)]
pub struct Registry {
    pub(crate) inner: Arc<Mutex<HashMap<u32, LatestFrame>>>,
}

impl FrameSource for Registry {
    fn snapshot(&self, id: u32) -> Option<(Frame, u32)> {
        let guard = recover_lock(&self.inner);
        guard.get(&id)?.snapshot_with_counter()
    }
}

/// Return the [`LatestFrame`] slot for `id`, creating one if absent.
pub fn get_or_create_sink(registry: &Registry, id: u32) -> LatestFrame {
    let mut guard = recover_lock(&registry.inner);
    guard.entry(id).or_default().clone()
}

/// Drop the [`LatestFrame`] slot for `id`.
///
/// Other clones of that sink continue to function; the registry just stops
/// handing it out on future [`get_or_create_sink`] calls.
pub fn remove_sink(registry: &Registry, id: u32) {
    let mut guard = recover_lock(&registry.inner);
    guard.remove(&id);
}

/// A shareable slot holding the latest [`Frame`] for one stream.
///
/// Returned by [`get_or_create_sink`] and fed by the
/// [pump](cameras::pump::spawn)'s sink closure. Most callers just wire a
/// [`LatestFrame`] from the registry into a pump and never touch it directly
///, the hook [`use_camera_stream`](crate::use_camera_stream) does exactly
/// that. If you're running your own pump, call [`publish_frame`] on each
/// frame.
#[derive(Clone, Default)]
pub struct LatestFrame {
    pub(crate) frame: Arc<Mutex<Option<Frame>>>,
    pub(crate) counter: Arc<AtomicU32>,
}

impl LatestFrame {
    pub(crate) fn snapshot_with_counter(&self) -> Option<(Frame, u32)> {
        let slot = recover_lock(&self.frame);
        let frame = slot.as_ref()?.clone();
        let counter = self.counter.load(Ordering::Acquire);
        Some((frame, counter))
    }
}

/// Publish `frame` as the latest value on `sink`, replacing any previous
/// frame.
///
/// The sink's monotonic counter increments with every call so HTTP clients
/// can skip redundant texture uploads.
pub fn publish_frame(sink: &LatestFrame, frame: Frame) {
    let mut slot = recover_lock(&sink.frame);
    *slot = Some(frame);
    sink.counter.fetch_add(1, Ordering::Release);
}
