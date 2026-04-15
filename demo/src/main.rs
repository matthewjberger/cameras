use chimeras::{Credentials, Device, Frame, PixelFormat, Resolution, StreamConfig};
use dioxus::desktop::wry::http::Response as HttpResponse;
use dioxus::prelude::*;
use image::ImageEncoder;
use image::codecs::png::PngEncoder;
use std::borrow::Cow;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const APP_CSS: &str = include_str!("../assets/app.css");
const PREVIEW_JS: &str = include_str!("../assets/preview.js");
const PROTOCOL: &str = "chimeras";

const PREVIEW_MAGIC: [u8; 4] = *b"CHIM";
const PREVIEW_VERSION: u8 = 1;
const PREVIEW_FORMAT_NONE: u8 = 0;
const PREVIEW_FORMAT_NV12: u8 = 1;
const PREVIEW_FORMAT_BGRA: u8 = 2;
const PREVIEW_FORMAT_RGBA: u8 = 3;
const PREVIEW_HEADER_LEN: usize = 24;

#[cfg(any(target_os = "windows", target_os = "android"))]
const PREVIEW_FETCH_URL: &str = "http://chimeras.localhost/preview.bin";

#[cfg(not(any(target_os = "windows", target_os = "android")))]
const PREVIEW_FETCH_URL: &str = "chimeras://localhost/preview.bin";

fn main() {
    let latest_frame = LatestFrame::new();
    let latest_for_protocol = latest_frame.clone();

    dioxus::LaunchBuilder::desktop()
        .with_cfg(
            dioxus::desktop::Config::new()
                .with_menu(None)
                .with_custom_protocol(PROTOCOL.to_string(), move |_id, _request| {
                    serve_frame(&latest_for_protocol)
                })
                .with_window(
                    dioxus::desktop::WindowBuilder::new()
                        .with_title("chimeras demo")
                        .with_inner_size(dioxus::desktop::LogicalSize::new(1100.0, 760.0)),
                ),
        )
        .with_context(latest_frame)
        .launch(App);
}

struct Session {
    #[allow(dead_code)]
    pump: Option<std::thread::JoinHandle<()>>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown
            .store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(handle) = self.pump.take() {
            let _ = handle.join();
        }
    }
}

#[derive(Clone)]
struct LatestFrame {
    frame: Arc<Mutex<Option<Frame>>>,
    counter: Arc<AtomicU32>,
}

impl LatestFrame {
    fn new() -> Self {
        Self {
            frame: Arc::new(Mutex::new(None)),
            counter: Arc::new(AtomicU32::new(0)),
        }
    }

    fn set(&self, frame: Frame) {
        if let Ok(mut slot) = self.frame.lock() {
            *slot = Some(frame);
            self.counter.fetch_add(1, Ordering::Release);
        }
    }

    fn take(&self) -> Option<Frame> {
        self.frame.lock().ok().and_then(|mut slot| slot.take())
    }

    fn snapshot(&self) -> Option<Frame> {
        self.frame.lock().ok()?.clone()
    }

    fn counter(&self) -> u32 {
        self.counter.load(Ordering::Acquire)
    }
}

fn serve_frame(latest: &LatestFrame) -> HttpResponse<Cow<'static, [u8]>> {
    let counter = latest.counter();
    let body = match latest.snapshot() {
        Some(frame) => encode_preview(&frame, counter),
        None => preview_header(PREVIEW_FORMAT_NONE, 0, 0, 0, counter),
    };
    let len = body.len();
    HttpResponse::builder()
        .status(200)
        .header("Content-Type", "application/octet-stream")
        .header("Content-Length", len.to_string())
        .header("Cache-Control", "no-store")
        .header("Access-Control-Allow-Origin", "*")
        .body(Cow::Owned(body))
        .unwrap()
}

fn preview_header(format: u8, width: u32, height: u32, stride: u32, counter: u32) -> Vec<u8> {
    let mut header = Vec::with_capacity(PREVIEW_HEADER_LEN);
    header.extend_from_slice(&PREVIEW_MAGIC);
    header.push(PREVIEW_VERSION);
    header.push(format);
    header.extend_from_slice(&[0u8, 0u8]);
    header.extend_from_slice(&width.to_le_bytes());
    header.extend_from_slice(&height.to_le_bytes());
    header.extend_from_slice(&stride.to_le_bytes());
    header.extend_from_slice(&counter.to_le_bytes());
    header
}

fn encode_preview(frame: &Frame, counter: u32) -> Vec<u8> {
    match frame.pixel_format {
        PixelFormat::Nv12 => {
            let mut out = preview_header(
                PREVIEW_FORMAT_NV12,
                frame.width,
                frame.height,
                frame.stride,
                counter,
            );
            out.reserve(frame.plane_primary.len() + frame.plane_secondary.len());
            out.extend_from_slice(&frame.plane_primary);
            out.extend_from_slice(&frame.plane_secondary);
            out
        }
        PixelFormat::Bgra8 => {
            let stride = if frame.stride == 0 {
                frame.width * 4
            } else {
                frame.stride
            };
            let mut out = preview_header(
                PREVIEW_FORMAT_BGRA,
                frame.width,
                frame.height,
                stride,
                counter,
            );
            out.extend_from_slice(&frame.plane_primary);
            out
        }
        _ => {
            let Ok(rgba) = chimeras::to_rgba8(frame) else {
                return preview_header(PREVIEW_FORMAT_NONE, 0, 0, 0, counter);
            };
            let stride = frame.width * 4;
            let mut out = preview_header(
                PREVIEW_FORMAT_RGBA,
                frame.width,
                frame.height,
                stride,
                counter,
            );
            out.extend_from_slice(&rgba);
            out
        }
    }
}

fn refresh_devices(
    mut devices: Signal<Vec<Device>>,
    mut status: Signal<String>,
    mut selected_index: Signal<usize>,
) {
    match chimeras::devices() {
        Ok(list) => {
            let count = list.len();
            if *selected_index.peek() >= count {
                selected_index.set(0);
            }
            devices.set(list);
            status.set(match count {
                0 => "No cameras detected".into(),
                1 => "1 camera available".into(),
                n => format!("{n} cameras available"),
            });
        }
        Err(error) => status.set(format!("Enumerate failed: {error}")),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum SourceMode {
    Usb,
    Rtsp,
}

#[component]
fn App() -> Element {
    let devices = use_signal(Vec::<Device>::new);
    let selected_index = use_signal(|| 0usize);
    let status = use_signal(|| "Idle".to_string());
    let session: Signal<Option<Session>> = use_signal(|| None);
    let saved_path = use_signal(|| None::<String>);
    let source_mode = use_signal(|| SourceMode::Usb);
    let rtsp_url = use_signal(|| "rtsp://127.0.0.1:8554/live".to_string());
    let rtsp_username = use_signal(String::new);
    let rtsp_password = use_signal(String::new);

    let latest_frame = use_context::<LatestFrame>();

    use_effect(move || {
        refresh_devices(devices, status, selected_index);
    });

    let refresh = move |_| {
        refresh_devices(devices, status, selected_index);
    };

    let connect = {
        let latest_frame = latest_frame.clone();
        move |_| {
            let mode = *source_mode.peek();
            let config = StreamConfig {
                resolution: Resolution {
                    width: 1280,
                    height: 720,
                },
                framerate: 30,
                pixel_format: PixelFormat::Bgra8,
            };

            let (open_result, label) = match mode {
                SourceMode::Usb => {
                    let selected = *selected_index.peek();
                    let Some(device) = devices.peek().get(selected).cloned() else {
                        status.clone().set("No camera selected".into());
                        return;
                    };
                    let label = device.name.clone();
                    session.clone().set(None);
                    status.clone().set(format!("Connecting to {label}..."));
                    (chimeras::open(&device, config), label)
                }
                SourceMode::Rtsp => {
                    let url = rtsp_url.peek().trim().to_string();
                    if url.is_empty() {
                        status.clone().set("RTSP URL is empty".into());
                        return;
                    }
                    let username = rtsp_username.peek().trim().to_string();
                    let password = rtsp_password.peek().to_string();
                    let credentials = if username.is_empty() && password.is_empty() {
                        None
                    } else {
                        Some(Credentials { username, password })
                    };
                    session.clone().set(None);
                    status.clone().set(format!("Connecting to {url}..."));
                    let label = url.clone();
                    (chimeras::open_rtsp(&url, credentials, config), label)
                }
            };

            match open_result {
                Ok(camera) => {
                    let shutdown = Arc::new(std::sync::atomic::AtomicBool::new(false));
                    let shutdown_for_pump = Arc::clone(&shutdown);
                    let latest_for_pump = latest_frame.clone();
                    let pump = std::thread::Builder::new()
                        .name("demo-camera-pump".into())
                        .spawn(move || {
                            let camera = camera;
                            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                while !shutdown_for_pump.load(std::sync::atomic::Ordering::Relaxed)
                                {
                                    match chimeras::next_frame(&camera, Duration::from_millis(500))
                                    {
                                        Ok(frame) => latest_for_pump.set(frame),
                                        Err(chimeras::Error::Timeout) => continue,
                                        Err(_) => break,
                                    }
                                }
                            }));
                        })
                        .expect("failed to spawn camera pump thread");
                    session.clone().set(Some(Session {
                        pump: Some(pump),
                        shutdown,
                    }));
                    status.clone().set(format!("Streaming: {label}"));
                }
                Err(error) => status.clone().set(format!("Open failed: {error}")),
            }
        }
    };

    let capture = {
        let latest_frame = latest_frame.clone();
        move |_| {
            let Some(frame) = latest_frame.take() else {
                status.clone().set("No frame to capture".into());
                return;
            };
            let rgb = match chimeras::to_rgb8(&frame) {
                Ok(rgb) => rgb,
                Err(error) => {
                    status.clone().set(format!("Decode failed: {error}"));
                    return;
                }
            };
            let timestamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let path = std::env::temp_dir().join(format!("chimeras-capture-{timestamp}.png"));
            let file = match std::fs::File::create(&path) {
                Ok(file) => file,
                Err(error) => {
                    status.clone().set(format!("Save failed: {error}"));
                    return;
                }
            };
            let encoder = PngEncoder::new(file);
            if let Err(error) = encoder.write_image(
                &rgb,
                frame.width,
                frame.height,
                image::ExtendedColorType::Rgb8,
            ) {
                status.clone().set(format!("Save failed: {error}"));
                return;
            }
            saved_path.clone().set(Some(path.to_string_lossy().into()));
            status.clone().set(format!("Saved to {}", path.display()));
        }
    };

    let device_count = devices().len();
    let is_connected = session.peek().is_some();
    let connect_label = if is_connected { "Reconnect" } else { "Connect" };
    let mode = source_mode();
    let is_usb = mode == SourceMode::Usb;
    let is_rtsp = mode == SourceMode::Rtsp;
    let connect_enabled = match mode {
        SourceMode::Usb => device_count > 0,
        SourceMode::Rtsp => !rtsp_url().trim().is_empty(),
    };

    rsx! {
        style { {APP_CSS} }
        div { class: "app",
            header { class: "title-bar",
                h1 { "chimeras" }
                span { class: "subtitle", "cross-platform camera demo" }
            }

            section { class: "controls",
                div { class: "mode-toggle",
                    button {
                        class: if is_usb { "mode-btn mode-btn-active" } else { "mode-btn" },
                        onclick: move |_| source_mode.clone().set(SourceMode::Usb),
                        "USB"
                    }
                    button {
                        class: if is_rtsp { "mode-btn mode-btn-active" } else { "mode-btn" },
                        onclick: move |_| source_mode.clone().set(SourceMode::Rtsp),
                        "RTSP"
                    }
                }

                if is_usb {
                    div { class: "field",
                        span { class: "field-label", "Camera" }
                        select {
                            class: "input",
                            disabled: device_count == 0,
                            onchange: move |event| {
                                if let Ok(index) = event.value().parse::<usize>() {
                                    selected_index.clone().set(index);
                                }
                            },
                            if device_count == 0 {
                                option { "No cameras detected" }
                            } else {
                                for (index, device) in devices().iter().enumerate() {
                                    option { value: "{index}", "{device.name}" }
                                }
                            }
                        }
                    }
                } else {
                    div { class: "rtsp-inputs",
                        div { class: "field",
                            span { class: "field-label", "RTSP URL" }
                            input {
                                class: "input",
                                r#type: "text",
                                placeholder: "rtsp://127.0.0.1:8554/live",
                                value: "{rtsp_url()}",
                                oninput: move |event| rtsp_url.clone().set(event.value()),
                            }
                        }
                        div { class: "field field-narrow",
                            span { class: "field-label", "Username" }
                            input {
                                class: "input",
                                r#type: "text",
                                value: "{rtsp_username()}",
                                oninput: move |event| rtsp_username.clone().set(event.value()),
                            }
                        }
                        div { class: "field field-narrow",
                            span { class: "field-label", "Password" }
                            input {
                                class: "input",
                                r#type: "password",
                                value: "{rtsp_password()}",
                                oninput: move |event| rtsp_password.clone().set(event.value()),
                            }
                        }
                    }
                }

                div { class: "button-row",
                    if is_usb {
                        button {
                            class: "btn btn-ghost",
                            onclick: refresh,
                            "Refresh"
                        }
                    }
                    button {
                        class: "btn btn-primary",
                        disabled: !connect_enabled,
                        onclick: connect,
                        "{connect_label}"
                    }
                    button {
                        class: "btn btn-accent",
                        disabled: !is_connected,
                        onclick: capture,
                        "Capture"
                    }
                }
            }

            section { class: "status",
                span { class: "status-label", "Status" }
                span {
                    class: "status-dot",
                    "data-state": if is_connected { "live" } else { "idle" },
                }
                span { class: "status-value", "{status()}" }
            }

            section { class: "preview",
                canvas {
                    id: "chimeras-preview",
                    class: if is_connected { "preview-canvas" } else { "preview-canvas hidden" },
                }
                if !is_connected {
                    div { class: "preview-placeholder",
                        div { class: "placeholder-icon", "●" }
                        div { class: "placeholder-text",
                            if is_rtsp {
                                "Enter an RTSP URL and press Connect"
                            } else if device_count == 0 {
                                "Plug in a camera, grant permission, and press Refresh"
                            } else {
                                "Press Connect to start streaming"
                            }
                        }
                    }
                }
            }
            script { dangerous_inner_html: "window.__chimerasPreviewUrl={PREVIEW_FETCH_URL:?};{PREVIEW_JS}" }

            if let Some(path) = saved_path() {
                section { class: "saved-note",
                    span { class: "saved-label", "Last capture" }
                    code { class: "saved-path", "{path}" }
                }
            }
        }
    }
}
