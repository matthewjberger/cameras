use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::poison::recover_lock;

/// Clonable wrapper around a multi-producer, single-consumer channel.
///
/// The receiver is wrapped in an `Arc<Mutex<_>>` so the whole handle is
/// `Clone` for any `T` (including non-`Clone` payloads), making it trivial
/// to store in a Dioxus `use_hook` slot and hand copies to both a poll task
/// and to background worker threads.
///
/// The manual `Clone` impl below is required because `#[derive(Clone)]` would
/// synthesize a `T: Clone` bound that we explicitly do not want.
pub(crate) struct Channel<T> {
    pub(crate) sender: Sender<T>,
    pub(crate) receiver: Arc<Mutex<Receiver<T>>>,
}

impl<T> Clone for Channel<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            receiver: Arc::clone(&self.receiver),
        }
    }
}

impl<T> Channel<T> {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
        }
    }

    /// Drain every pending value without blocking.
    pub(crate) fn drain(&self) -> Vec<T> {
        let guard = recover_lock(&self.receiver);
        std::iter::from_fn(|| guard.try_recv().ok()).collect()
    }
}
