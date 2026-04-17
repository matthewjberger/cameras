<h1 align="center">
  <img src="assets/aperture.svg" alt="" width="48" align="center">
  chimeras
</h1>

<p align="center"><strong>A cross-platform camera library for Rust, plus first-class UI-framework integrations.</strong></p>

<p align="center">
  <a href="https://github.com/matthewjberger/chimeras"><img alt="github" src="https://img.shields.io/badge/github-matthewjberger/chimeras-8da0cb?style=for-the-badge&labelColor=555555&logo=github" height="20"></a>
  <a href="https://github.com/matthewjberger/chimeras/blob/main/LICENSE-MIT"><img alt="license" src="https://img.shields.io/badge/license-MIT%2FApache--2.0-blue?style=for-the-badge&labelColor=555555" height="20"></a>
</p>

## Crates

| Crate | crates.io | docs.rs | Purpose |
|-------|-----------|---------|---------|
| [`chimeras`](crates/chimeras/) | [![crates.io](https://img.shields.io/crates/v/chimeras.svg?logo=rust&color=fc8d62)](https://crates.io/crates/chimeras) | [![docs.rs](https://img.shields.io/badge/docs.rs-chimeras-66c2a5?logo=docs.rs)](https://docs.rs/chimeras) | Enumerate, probe, open, and stream cameras. macOS (AVFoundation), Windows (Media Foundation), Linux (V4L2). Optional RTSP. |
| [`dioxus-chimeras`](crates/dioxus-chimeras/) | [![crates.io](https://img.shields.io/crates/v/dioxus-chimeras.svg?logo=rust&color=fc8d62)](https://crates.io/crates/dioxus-chimeras) | [![docs.rs](https://img.shields.io/badge/docs.rs-dioxus--chimeras-66c2a5?logo=docs.rs)](https://docs.rs/dioxus-chimeras) | Hooks + components for using chimeras inside a Dioxus desktop app. WebGL2 preview rendering. |
| [`egui-chimeras`](crates/egui-chimeras/) | [![crates.io](https://img.shields.io/crates/v/egui-chimeras.svg?logo=rust&color=fc8d62)](https://crates.io/crates/egui-chimeras) | [![docs.rs](https://img.shields.io/badge/docs.rs-egui--chimeras-66c2a5?logo=docs.rs)](https://docs.rs/egui-chimeras) | Helpers for using chimeras inside an egui / eframe app. Frame-to-texture conversion. |

Each integration crate is thin; almost everything lives in `chimeras` itself (`chimeras::pump`, `chimeras::source`, `chimeras::monitor`, etc.). The integration crates just bridge a running `chimeras::pump::Pump` to the target UI framework's texture / canvas model.

## Layout

```
chimeras/
├── crates/
│   ├── chimeras/         ← the core library
│   ├── dioxus-chimeras/  ← Dioxus integration
│   └── egui-chimeras/    ← egui integration
└── apps/
    ├── dioxus-demo/      ← multi-stream grid, USB + RTSP sources
    └── egui-demo/        ← single-stream viewer with pause / snapshot
```

## Demos

```bash
just run-dioxus   # Dioxus desktop app: multi-stream grid, USB + RTSP
just run-egui     # egui / eframe app: single-stream viewer + snapshot
```

## Versioning

All three crates ship in lockstep on the same major + minor version (currently `0.2.x`). Use matching minor versions across your `Cargo.toml` when depending on them.

## Publishing

```bash
just publish       # chimeras
just publish-dx    # dioxus-chimeras
just publish-egui  # egui-chimeras
```

## License

Dual-licensed under either of

- [MIT License](LICENSE-MIT)
- [Apache License, Version 2.0](LICENSE-APACHE)

at your option.
