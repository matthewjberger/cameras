use dioxus::prelude::*;

use crate::PREVIEW_JS;

/// The preview server's port, injected into the Dioxus context by
/// [`PreviewServer::register_with`](crate::PreviewServer::register_with).
///
/// Crate-private because `register_with` is the only intended entry point:
/// users should not have to know the context type name.
#[derive(Clone, Copy)]
pub(crate) struct PreviewPort(pub(crate) u16);

/// A `<canvas>` bound to the preview server for `id`.
///
/// Reads the preview port from context (see
/// [`PreviewServer::register_with`](crate::PreviewServer::register_with)),
/// emits the `data-stream-id` and `data-preview-url` attributes that
/// [`PREVIEW_JS`](crate::PREVIEW_JS) scans for, and applies the
/// `cameras-preview-canvas` class so that user CSS can target it.
///
/// Wrap it in your own sized container, the JS renderer resizes to the
/// parent's bounding rect.
///
/// # Panics
///
/// Panics at render time if called outside an app that has been wired up with
/// [`PreviewServer::register_with`](crate::PreviewServer::register_with).
#[component]
pub fn StreamPreview(id: u32) -> Element {
    let port = try_consume_context::<PreviewPort>()
        .expect(
            "`StreamPreview` requires `PreviewServer::register_with` to be called at launch, \
             see the dioxus-cameras crate docs",
        )
        .0;
    let url = format!("http://127.0.0.1:{port}/preview/{id}.bin");
    rsx! {
        canvas {
            id: "cameras-preview-{id}",
            class: "cameras-preview-canvas",
            "data-stream-id": "{id}",
            "data-preview-url": "{url}",
        }
    }
}

/// Injects the WebGL2 preview renderer script once into the page.
///
/// Render this exactly once, typically as the last child of your root
/// component. Every [`StreamPreview`] in the DOM is discovered and driven by
/// this script; without it the preview canvases stay blank.
///
/// Equivalent to:
///
/// ```ignore
/// rsx! { script { dangerous_inner_html: "{PREVIEW_JS}" } }
/// ```
#[component]
pub fn PreviewScript() -> Element {
    rsx! { script { dangerous_inner_html: "{PREVIEW_JS}" } }
}
