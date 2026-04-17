set windows-shell := ["powershell.exe", "-NoProfile", "-Command"]

_dx_version := "0.7.5"

export RUST_LOG := "info"

[private]
default:
  @just --list

# Build the workspace in release mode
build:
  cargo build --workspace --release

# Check the workspace
check:
  cargo check --workspace --all-targets

# Autoformat the workspace
format:
  cargo fmt --all

# Verify formatting
format-check:
  cargo fmt --all -- --check

# Lint the workspace
lint:
  cargo clippy --workspace --all-targets -- -D warnings

# Check just the chimeras core library
check-lib:
  cargo check -p chimeras --all-targets

# Lint just the chimeras core library
lint-lib:
  cargo clippy -p chimeras --all-targets -- -D warnings

# Check dioxus-chimeras (with and without default features)
check-dx:
  cargo check -p dioxus-chimeras --all-targets
  cargo check -p dioxus-chimeras --no-default-features --all-targets

# Lint dioxus-chimeras (with and without default features)
lint-dx:
  cargo clippy -p dioxus-chimeras --all-targets -- -D warnings
  cargo clippy -p dioxus-chimeras --no-default-features --all-targets -- -D warnings

# Check egui-chimeras
check-egui:
  cargo check -p egui-chimeras --all-targets

# Lint egui-chimeras (with and without default features)
lint-egui:
  cargo clippy -p egui-chimeras --all-targets -- -D warnings
  cargo clippy -p egui-chimeras --no-default-features --all-targets -- -D warnings

# Build rustdoc for chimeras, failing on broken links.
# (`--cfg docsrs` is set on the real docs.rs build via [package.metadata.docs.rs];
# we don't pass it here because `doc(cfg(...))` requires nightly.)
[unix]
doc:
  RUSTDOCFLAGS="-D warnings" cargo doc -p chimeras --no-deps --all-features

[windows]
doc:
  $env:RUSTDOCFLAGS = "-D warnings"; cargo doc -p chimeras --no-deps --all-features

# Build rustdoc for dioxus-chimeras, failing on broken links.
[unix]
doc-dx:
  RUSTDOCFLAGS="-D warnings" cargo doc -p dioxus-chimeras --no-deps --all-features

[windows]
doc-dx:
  $env:RUSTDOCFLAGS = "-D warnings"; cargo doc -p dioxus-chimeras --no-deps --all-features

# Build rustdoc for egui-chimeras, failing on broken links.
[unix]
doc-egui:
  RUSTDOCFLAGS="-D warnings" cargo doc -p egui-chimeras --no-deps --all-features

[windows]
doc-egui:
  $env:RUSTDOCFLAGS = "-D warnings"; cargo doc -p egui-chimeras --no-deps --all-features

# Run the Dioxus demo with hot-reloading
run-dioxus: _require-dx
  dx serve -p dioxus-demo --hotpatch

# Run the Dioxus demo in release mode
run-dioxus-release:
  cargo run -p dioxus-demo --release

# Run the egui demo
run-egui:
  cargo run -p egui-demo --release

# Take a single-frame snapshot from the first camera and write a PNG.
run-snapshot path="snapshot.png":
  cargo run -p chimeras --example snapshot -- {{path}}

# Drive a camera with the pump: stream, pause, capture, resume, stop.
run-pump:
  cargo run -p chimeras --example pump

# Stream camera hotplug events until Ctrl-C.
run-monitor:
  cargo run -p chimeras --example monitor

# Run mediamtx in the foreground to host rtsp://127.0.0.1:8554. Run this
# in one terminal, then `just rtsp-publish PATH` in another to push an
# MP4 into it. Requires mediamtx on PATH.
rtsp-host:
  mediamtx

# Publish a local MP4 file to the running mediamtx as an RTSP stream at
# rtsp://127.0.0.1:8554/<path>. Each unique path is an independent stream
# so you can run this in several terminals with different paths to feed
# the demo's grid view. Assumes `just rtsp-host` is running. Requires
# ffmpeg on PATH.
rtsp-publish file="test_video.mp4" path="live":
  ffmpeg -re -stream_loop -1 -i {{file}} -an -c:v copy -f rtsp -rtsp_transport tcp rtsp://127.0.0.1:8554/{{path}}

# Check for unused dependencies with cargo-machete
udeps:
  cargo machete

# Dry-run publish chimeras to crates.io
publish-dry:
  cargo publish -p chimeras --dry-run

# Publish chimeras to crates.io (requires cargo login)
publish:
  cargo publish -p chimeras

# Dry-run publish dioxus-chimeras to crates.io
publish-dry-dx:
  cargo publish -p dioxus-chimeras --dry-run

# Publish dioxus-chimeras to crates.io. chimeras must already be on crates.io
# at the version dioxus-chimeras depends on.
publish-dx:
  cargo publish -p dioxus-chimeras

# Dry-run publish egui-chimeras to crates.io
publish-dry-egui:
  cargo publish -p egui-chimeras --dry-run

# Publish egui-chimeras to crates.io. chimeras must already be on crates.io
# at the version egui-chimeras depends on.
publish-egui:
  cargo publish -p egui-chimeras

# Display toolchain versions
@versions:
  rustc --version
  cargo fmt -- --version
  cargo clippy -- --version
  rustup --version

[private]
[unix]
_require-dx:
  @command -v dx >/dev/null 2>&1 || (echo "dx not found, installing..." && cargo install dioxus-cli@{{_dx_version}} --locked)

[private]
[windows]
_require-dx:
  @if (-not (Get-Command dx -ErrorAction SilentlyContinue)) { Write-Host "dx not found, installing..."; cargo install dioxus-cli@{{_dx_version}} --locked }
