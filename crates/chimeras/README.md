<h1 align="center">chimeras</h1>

<p align="center">
  <a href="https://github.com/matthewjberger/chimeras"><img alt="github" src="https://img.shields.io/badge/github-matthewjberger/chimeras-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20"></a>
  <a href="https://crates.io/crates/chimeras"><img alt="crates.io" src="https://img.shields.io/crates/v/chimeras.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20"></a>
  <a href="https://docs.rs/chimeras"><img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-chimeras-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20"></a>
  <a href="https://github.com/matthewjberger/chimeras/blob/main/LICENSE-MIT"><img alt="license" src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue?style=for-the-badge&labelColor=555555" height="20"></a>
</p>

<p align="center"><strong>A cross-platform camera library for Rust.</strong></p>

<p align="center">
  <code>cargo add chimeras</code>
</p>

`chimeras` enumerates cameras, probes their supported formats, opens a streaming session, and delivers frames. It runs on macOS (AVFoundation), Windows (Media Foundation), and Linux (V4L2) with the same API on each platform.

The public surface is plain data types (`Device`, `Capabilities`, `FormatDescriptor`, `StreamConfig`, `Frame`) and a handful of free functions. There are no trait objects in the public API, no hidden global state, and no `unsafe` required of consumers.

https://github.com/user-attachments/assets/8e2f4a5f-0e70-4cf7-a8de-942c0d2fada5

## Quick Start

Add this to your `Cargo.toml`:

```toml
[dependencies]
chimeras = "0.1"
```

And in `main.rs`:

```rust
use std::time::Duration;

fn main() -> Result<(), chimeras::Error> {
    let devices = chimeras::devices()?;
    let device = devices.first().expect("no cameras");

    let capabilities = chimeras::probe(device)?;
    let config = chimeras::StreamConfig {
        resolution: chimeras::Resolution { width: 1280, height: 720 },
        framerate: 30,
        pixel_format: chimeras::PixelFormat::Bgra8,
    };

    let camera = chimeras::open(device, config)?;
    let frame = chimeras::next_frame(&camera, Duration::from_secs(2))?;
    let rgb = chimeras::to_rgb8(&frame)?;

    println!("{}x{}, {} bytes rgb", frame.width, frame.height, rgb.len());
    Ok(())
}
```

Dropping the `Camera` stops the stream. Dropping the `DeviceMonitor` joins its worker.

## Platform Support

| Platform | USB / Built-in | RTSP (`rtsp` feature) |
|----------|----------------|------------------------|
| macOS    | AVFoundation (via `objc2`) | retina + VideoToolbox (H.264 / H.265 / MJPEG) |
| Windows  | Media Foundation (via `windows`) | retina + Media Foundation (H.264 / H.265 / MJPEG) |
| Linux    | V4L2 mmap streaming (via `v4l`) | not supported |

## API Overview

Enumerate and probe:

```rust
let devices = chimeras::devices()?;
let capabilities = chimeras::probe(&devices[0])?;
```

Open a camera and read frames:

```rust
use std::time::Duration;

let camera = chimeras::open(&devices[0], config)?;
let frame = chimeras::next_frame(&camera, Duration::from_secs(2))?;
```

Convert pixel formats (BGRA8, RGBA8, YUYV, NV12, MJPEG via `zune-jpeg`):

```rust
let rgb = chimeras::to_rgb8(&frame)?;
let rgba = chimeras::to_rgba8(&frame)?;
```

Watch for camera hotplug:

```rust
use std::time::Duration;

let monitor = chimeras::monitor()?;
while let Ok(event) = chimeras::next_event(&monitor, Duration::from_secs(1)) {
    match event {
        chimeras::DeviceEvent::Added(device) => println!("+ {}", device.name),
        chimeras::DeviceEvent::Removed(id) => println!("- {}", id.0),
    }
}
```

Pick a fallback format if the exact request is not supported:

```rust
let picked = chimeras::best_format(&capabilities, &config).expect("no fallback");
```

## Higher-level primitives

Two optional modules layer on top of the core. They are pure conveniences; callers who want full control can stay on `open` and `next_frame`.

### `chimeras::source`: one enum for USB and RTSP

`CameraSource` lets UIs and configs carry a single "where do frames come from" value instead of branching between `open` and `open_rtsp` at every call site.

```rust
use chimeras::{CameraSource, Device, StreamConfig};

fn open_any(device: Device, config: StreamConfig) -> Result<chimeras::Camera, chimeras::Error> {
    let source = CameraSource::Usb(device);
    chimeras::open_source(source, config)
}
```

`CameraSource` implements `PartialEq`, `Eq`, and `Hash` (USB compared by device id, RTSP by URL plus credentials) so it works as a map key or a `Signal` value.

### `chimeras::pump`: background frame pump with pause and snapshot

`pump::spawn` takes a `Camera` and a sink closure, runs the frame loop on its own thread, and returns a `Pump` handle with three operations:

- `pump::set_active(&pump, bool)`: pause or resume streaming without closing the camera (no per-frame work while paused).
- `pump::capture_frame(&pump) -> Option<Frame>`: fetch a single fresh frame on demand, works whether the pump is active or paused.
- `pump::stop_and_join(pump)`: deterministic teardown.

```rust
use chimeras::pump;

fn drive(camera: chimeras::Camera) {
    let p = pump::spawn(camera, |frame| {
        // publish the frame wherever: a channel, a Mutex, your own UI state.
        let _ = frame;
    });

    // Take a snapshot regardless of whether the pump is active:
    if let Some(_snapshot) = pump::capture_frame(&p) { /* use the frame */ }

    // Preview hidden? Park the pump.
    pump::set_active(&p, false);

    // Bring it back later.
    pump::set_active(&p, true);

    pump::stop_and_join(p);
}
```

Pause eliminates Rust-side per-frame work. The OS camera pipeline keeps running. See the `pump` module docs for the full trade-off.

## Dioxus integration

If you are building a Dioxus app, see the companion crate [`dioxus-chimeras`](dioxus-chimeras/), which provides:

- `use_camera_stream` hook with `active` + `capture_frame` on the returned handle.
- `use_devices` and `use_streams` hooks for camera enumeration and multi-stream id management.
- A loopback HTTP preview server plus a WebGL2 canvas renderer (`StreamPreview` + `PreviewScript`).

## Testing RTSP Locally

The `demo/` app can view RTSP streams on macOS and Windows. To exercise the full path without a real IP camera, serve a local MP4 as an RTSP stream using [`mediamtx`](https://github.com/bluenviron/mediamtx) and `ffmpeg` (both on `PATH`):

```bash
# terminal 1: start mediamtx with the repo's mediamtx.yml
just rtsp-host

# terminal 2: publish an MP4 file as an RTSP stream on rtsp://127.0.0.1:8554/live
just rtsp-publish path/to/some.mp4

# terminal 3: launch the demo app
just run
```

In the demo window, switch the source toggle to **RTSP**, paste `rtsp://127.0.0.1:8554/live` into the URL field, and press **Connect**. On macOS and Windows, H.264/H.265 streams are hardware-decoded (VideoToolbox / Media Foundation); MJPEG streams are delivered verbatim and decoded via `zune-jpeg` on demand.

## Examples

Runnable integration templates for using chimeras outside Dioxus (CLI, egui / iced, Tauri, daemons, anything). See the [examples](examples/) directory.

| Example | What it shows |
|---------|---------------|
| [`snapshot`](examples/snapshot.rs) | Open, grab one frame, save as PNG. `to_rgba8` + file I/O. |
| [`pump`](examples/pump.rs) | `pump::spawn` with a closure sink, `set_active` pause/resume, `capture_frame` while paused. The template for plugging chimeras into your own runtime. |
| [`monitor`](examples/monitor.rs) | Camera hotplug event loop with `monitor` + `next_event`. |

```bash
just run-snapshot           # writes snapshot.png from the first camera
just run-pump               # 5-second stream + pause + capture demo
just run-monitor            # hotplug events until Ctrl-C
```

## Publishing

```bash
just publish      # chimeras
just publish-dx   # dioxus-chimeras
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.
