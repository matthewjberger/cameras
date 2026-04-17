<h1 align="center">egui-cameras</h1>

<p align="center">
  <a href="https://github.com/matthewjberger/cameras"><img alt="github" src="https://img.shields.io/badge/github-matthewjberger/cameras-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20"></a>
  <a href="https://crates.io/crates/egui-cameras"><img alt="crates.io" src="https://img.shields.io/crates/v/egui-cameras.svg?style=for-the-badge&color=fc8d62&logo=rust" height="20"></a>
  <a href="https://docs.rs/egui-cameras"><img alt="docs.rs" src="https://img.shields.io/badge/docs.rs-egui--cameras-66c2a5?style=for-the-badge&labelColor=555555&logo=docs.rs" height="20"></a>
  <a href="https://github.com/matthewjberger/cameras/blob/main/LICENSE-MIT"><img alt="license" src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue?style=for-the-badge&labelColor=555555" height="20"></a>
</p>

<p align="center"><strong>Drop live camera streams into your egui / eframe app.</strong></p>

<p align="center">
  <code>cargo add egui-cameras</code>
</p>

`egui-cameras` is the egui integration for the [`cameras`](../cameras/) cross-platform camera library. It owns the thin glue between a running `cameras::pump::Pump` and an `egui::TextureHandle`, so you can render live camera frames as an `egui::Image` with a few lines of code.

Every camera-side primitive (pause / resume pump, single-frame capture, unified `CameraSource`, hotplug monitor) lives upstream in `cameras` itself and is re-exported from this crate for convenience.

## Quick Start

```toml
[dependencies]
cameras = "0.1"
egui-cameras = "0.1"
eframe = "0.32"
```

```rust
use cameras::{PixelFormat, Resolution, StreamConfig};
use eframe::egui;

struct App {
    stream: egui_cameras::Stream,
}

impl App {
    fn new() -> Result<Self, cameras::Error> {
        let devices = cameras::devices()?;
        let device = devices.first().ok_or(cameras::Error::DeviceNotFound("no cameras".into()))?;
        let config = StreamConfig {
            resolution: Resolution { width: 1280, height: 720 },
            framerate: 30,
            pixel_format: PixelFormat::Bgra8,
        };
        let camera = cameras::open(device, config)?;
        Ok(Self { stream: egui_cameras::spawn(camera) })
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui_cameras::update_texture(&mut self.stream, ctx).ok();
        egui::CentralPanel::default().show(ctx, |ui| {
            egui_cameras::show(&self.stream, ui);
        });
        ctx.request_repaint();
    }
}
```

## What's in the box

| Item | Purpose |
|------|---------|
| `Stream` | Bundle of a `Pump` + `Sink` + `TextureHandle`. Holds everything one live camera needs. |
| `Sink` | Shared slot the pump writes each frame into. Cheap to clone. |
| `spawn(camera) -> Stream` | Convenience: spawn a pump and wire it to a fresh `Stream` with a default texture name. |
| `spawn_named(camera, name) -> Stream` | Like `spawn`, but lets you name the texture (useful for multi-camera apps). |
| `spawn_pump(camera, sink) -> Pump` | Lower-level: spawn a pump that writes into your own `Sink`. |
| `publish_frame(sink, frame)` | Write a frame into a sink (for custom pump code). |
| `take_frame(sink) -> Option<Frame>` | Pull the latest frame out of a sink. |
| `frame_to_color_image(frame)` | Convert a cameras `Frame` into an `egui::ColorImage`. |
| `update_texture(stream, ctx)` | Upload the latest frame to the stream's texture. Call each `update` tick. |
| `show(stream, ui)` | Draw the texture as an `egui::Image` scaled to fit the available area. |

Pump controls (`set_active`, `capture_frame`, `stop_and_join`) are re-exported directly from `cameras::pump`.

## Pause + snapshot

The same pattern as dioxus-cameras: pause the pump when the user isn't looking, grab fresh frames on demand without closing the camera.

```rust
use egui_cameras::{capture_frame, set_active};

# fn example(stream: &egui_cameras::Stream) -> Option<cameras::Frame> {
// Park the pump (no per-frame Rust work; camera stays open):
set_active(&stream.pump, false);

// Grab a fresh snapshot regardless of pause state:
capture_frame(&stream.pump)
# }
```

## Features

| Feature | Default | Description |
|---------|:-------:|-------------|
| `rtsp` | off | Forwards to `cameras/rtsp`; enables `CameraSource::Rtsp` on macOS and Windows. |

## Versioning

`egui-cameras` ships in lockstep with `cameras` and `dioxus-cameras`. All three crates share the same major + minor version (`0.1.x`). Match your `Cargo.toml` versions accordingly.

## Demo

The [`egui-demo`](../../apps/egui-demo/) app in the parent repo exercises the full API: live preview, pause toggle, take-picture button.

```bash
just run-egui
```

## License

Dual-licensed under MIT or Apache-2.0 at your option.
