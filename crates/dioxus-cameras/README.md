<h1 align="center">dioxus-cameras</h1>

<p align="center">
  <a href="https://github.com/matthewjberger/cameras"><img alt="github" src="https://img.shields.io/badge/github-matthewjberger/cameras-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20"></a>
  <a href="https://crates.io/crates/dioxus-cameras"><img alt="crates.io" src="https://img.shields.io/crates/v/dioxus-cameras.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20"></a>
  <a href="https://docs.rs/dioxus-cameras"><img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-dioxus--cameras-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20"></a>
  <a href="https://github.com/matthewjberger/cameras/blob/main/LICENSE-MIT"><img alt="license" src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue?style=for-the-badge&labelColor=555555" height="20"></a>
</p>

<p align="center"><strong>Drop camera streams into your Dioxus desktop app.</strong></p>

<p align="center">
  <code>cargo add dioxus-cameras</code>
</p>

`dioxus-cameras` is the Dioxus integration for the [`cameras`](../) cross-platform camera library. It owns the glue between a `cameras::Camera` and a `<canvas>` element: a loopback HTTP preview server, a WebGL2 renderer, a `Registry` that shares frames between the camera pump and the webview, and a handful of hooks that expose the stream lifecycle as Dioxus signals.

Every generic primitive (pause/resume pump, single-frame capture, unified `CameraSource`) lives upstream in `cameras` itself so non-Dioxus callers can use it too.

## Quick Start

```toml
[dependencies]
dioxus = { version = "0.7", features = ["desktop"] }
dioxus-cameras = "0.1"
cameras = "0.1"
```

`dioxus-cameras` and `cameras` ship in lockstep: both crates share the same major + minor version (`0.1.x`). Use matching versions in your `Cargo.toml`.

```rust
use cameras::{CameraSource, PixelFormat, Resolution, StreamConfig};
use dioxus::prelude::*;
use dioxus_cameras::{
    PreviewScript, StreamPreview, register_with, start_preview_server, use_camera_stream,
};

fn main() {
    let server = start_preview_server().expect("preview server");
    register_with(&server, dioxus::LaunchBuilder::desktop()).launch(App);
}

fn App() -> Element {
    let source = use_signal::<Option<CameraSource>>(|| None);
    let config = StreamConfig {
        resolution: Resolution { width: 1280, height: 720 },
        framerate: 30,
        pixel_format: PixelFormat::Bgra8,
    };
    let stream = use_camera_stream(0, source, config);

    rsx! {
        StreamPreview { id: 0 }
        p { "{stream.status}" }
        button {
            onclick: move |_| stream.active.clone().set(!*stream.active.read()),
            "Toggle preview"
        }
        button {
            onclick: move |_| { let _ = stream.capture_frame.call(()); },
            "Take picture"
        }
        PreviewScript {}
    }
}
```

`register_with` injects the preview server's `Registry` and port into the Dioxus context so every `StreamPreview` and `use_camera_stream` call downstream can find them.

## What's in the box

### Hooks

| Hook | Returns | Purpose |
|------|---------|---------|
| `use_camera_stream(id, source, config)` | `UseCameraStream { status, active, capture_frame }` | Opens the camera, runs the pump, publishes frames to the preview, surfaces lifecycle as signals. |
| `use_devices()` | `UseDevices { devices, ready, refresh }` | Keeps a `Signal<Vec<Device>>` populated off a worker thread; refresh on demand. |
| `use_streams()` | `UseStreams { ids, add, remove }` | Manages a dynamic list of stream ids for multi-stream apps; monotonic, safe as Dioxus keys. |

All returned handles are `Copy + Clone + PartialEq` data structs with public fields (no methods, no hidden state).

### Components

| Component | Purpose |
|-----------|---------|
| `StreamPreview { id }` | A `<canvas>` bound to the preview server's `/preview/{id}.bin` URL; WebGL2-renders the live stream. |
| `PreviewScript {}` | Injects the WebGL2 driver script once per page. Render it as the last child of your root. |

### Helpers

| Function | Purpose |
|----------|---------|
| `start_preview_server()` | Binds a loopback HTTP server on an ephemeral port; returns a `PreviewServer`. |
| `register_with(&server, launch)` | Injects registry + port + keep-alive into a `LaunchBuilder`. |
| `get_or_create_sink(&registry, id)` | Returns the `LatestFrame` slot for `id`; used when running your own pump. |
| `remove_sink(&registry, id)` | Drops the slot for `id`. Normally handled automatically on unmount. |
| `publish_frame(&sink, frame)` | Writes a frame into a `LatestFrame`; used when running your own pump. |

## Live preview + take-picture

The hook returns both operations on one handle.

```rust
let stream = use_camera_stream(id, source, config);

// Pause the pump while the user is on another tab (no per-frame Rust work;
// camera stays open so capture stays fast):
stream.active.clone().set(false);

// Grab a fresh frame whether streaming or paused:
if let Some(frame) = stream.capture_frame.call(()) {
    // save, toast, upload, etc.
}

// Resume streaming:
stream.active.clone().set(true);
```

`capture_frame` is sync-blocking on the calling thread until the pump worker replies (typically one frame interval, plus up to 20ms of wake latency if paused). That's fine from an `onclick`; for rapid back-to-back captures, dispatch from a `std::thread::spawn`'d worker or a Dioxus `spawn` task.

## Versioning

`dioxus-cameras` is released in lockstep with `cameras`: both crates share the same major + minor version (for example, `cameras 0.1.x` pairs with `dioxus-cameras 0.1.x`). Patch numbers can drift between the two; every `0.1.x` of dioxus-cameras will work with any `0.1.y` of cameras.

## Features

| Feature | Default | Description |
|---------|:-------:|-------------|
| `rtsp` | off | Forwards to `cameras/rtsp`; enables `CameraSource::Rtsp` on macOS and Windows. |

Enable with `dioxus-cameras = { version = "0.1", features = ["rtsp"] }`.

## Source separation

The crate deliberately splits responsibilities:

- **`cameras`** owns the camera-side primitives: `Camera`, `CameraSource`, `pump::Pump`, etc. No Dioxus dependency.
- **`dioxus-cameras`** owns the UI-side glue: the preview server, `Registry`, hooks, and components. Depends on `dioxus` and `cameras`.

If you're integrating cameras into a non-Dioxus app (a CLI, a Tauri shell, a custom renderer), you do not need this crate. Use `cameras::pump` directly for the pause + snapshot pattern.

## Demo

The [`demo/`](../demo/) app in the parent repo exercises the full API: multi-stream grid, per-cell USB or RTSP source, live-preview toggle, take-picture button, add/remove streams, device refresh.

```bash
just run           # hot-reloading dev build
just run-release   # release build
```

## Dependency surface

- `dioxus` 0.7 with `desktop` feature
- `cameras` 0.1
- `futures-timer` 3 (runtime-agnostic timer; keeps the crate from pinning tokio)
- `bytes` 1

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
