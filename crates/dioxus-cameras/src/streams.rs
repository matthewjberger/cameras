use dioxus::prelude::*;

/// Handle returned by [`use_streams`].
#[derive(Copy, Clone, PartialEq)]
pub struct UseStreams {
    /// The current list of active stream ids, in insertion order.
    ///
    /// Because `ids` is a field (not a method), call `let ids = streams.ids;`
    /// before reading so `rsx!` can use `ids()` as a Signal call, Rust's
    /// method-call syntax would otherwise treat `streams.ids()` as a method.
    pub ids: Signal<Vec<u32>>,
    /// Callback that appends a fresh id and returns it.
    ///
    /// Ids are allocated monotonically from 0 and never reused within one
    /// hook instance, so the returned id is safe to use as a Dioxus key.
    pub add: Callback<(), u32>,
    /// Callback that removes the given id from the list. No-op if absent.
    ///
    /// The stream's [`Registry`](crate::Registry) entry and camera pump are
    /// torn down automatically via the
    /// [`use_camera_stream`](crate::use_camera_stream) drop guard when the
    /// corresponding component unmounts, you do not need to touch the
    /// registry here.
    pub remove: Callback<u32>,
}

/// Hook that manages a dynamic list of stream ids for multi-stream apps.
///
/// Typical usage:
///
/// ```no_run
/// use dioxus::prelude::*;
/// use dioxus_cameras::{StreamPreview, use_streams};
///
/// fn app() -> Element {
///     let streams = use_streams();
///     let ids = streams.ids;
///     rsx! {
///         button { onclick: move |_| { streams.add.call(()); }, "Add stream" }
///         for id in ids() {
///             StreamPreview { key: "{id}", id }
///         }
///     }
/// }
/// ```
pub fn use_streams() -> UseStreams {
    let mut next_id = use_signal(|| 0u32);
    let mut ids = use_signal(Vec::<u32>::new);

    let add = use_callback(move |()| {
        let id = *next_id.peek();
        next_id.set(id + 1);
        ids.write().push(id);
        id
    });

    let remove = use_callback(move |id: u32| {
        ids.write().retain(|other| *other != id);
    });

    UseStreams { ids, add, remove }
}
