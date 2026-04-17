//! Loopback HTTP server that publishes frames from a [`FrameSource`] to the
//! Dioxus webview over `/preview/{id}.bin`.

use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use bytes::Bytes;
use cameras::{Frame, PixelFormat};
use dioxus::LaunchBuilder;

use crate::component::PreviewPort;
use crate::poison::recover_lock;
use crate::registry::{FrameSource, Registry};

const PREVIEW_MAGIC: [u8; 4] = *b"CAMS";
const PREVIEW_VERSION: u8 = 1;
const PREVIEW_FORMAT_NONE: u8 = 0;
const PREVIEW_FORMAT_NV12: u8 = 1;
const PREVIEW_FORMAT_BGRA: u8 = 2;
const PREVIEW_FORMAT_RGBA: u8 = 3;
const PREVIEW_HEADER_LEN: usize = 24;
const SHUTDOWN_KICK_TIMEOUT: Duration = Duration::from_millis(100);

/// A running preview server.
///
/// Obtained from [`start_preview_server`]. Serves frames from the embedded
/// [`Registry`] over HTTP on a loopback port at `/preview/{id}.bin`. Each
/// response is a 24-byte binary header followed by raw pixel data, see
/// [`PREVIEW_JS`](crate::PREVIEW_JS) for the client-side decoder.
///
/// Clone freely, all state is shared behind `Arc`s. The listener thread lives
/// exactly as long as the last clone; dropping every [`PreviewServer`] shuts
/// the server down cleanly.
///
/// The `port` field is public so callers can read it directly. The embedded
/// [`Registry`] is crate-private because the canonical way to access it is
/// [`use_context::<Registry>()`](dioxus::prelude::use_context) after calling
/// [`register_with`] at launch time.
///
/// # Coupling to `Registry`
///
/// The server is deliberately hard-coded to read from [`Registry`] rather
/// than being generic over a custom frame-source trait. Internally the
/// listener does use a `pub(crate)` `FrameSource` abstraction so swapping is
/// a small change if it ever becomes necessary, but exposing that surface
/// publicly would let users plug in a custom source that the bundled hooks
/// ([`use_camera_stream`](crate::use_camera_stream)) silently do not write
/// to, which is a worse footgun than the coupling. If your app needs a
/// non-`Registry` frame source, open a PR and we'll cut a typed entry
/// point.
#[derive(Clone)]
pub struct PreviewServer {
    /// The TCP port the server is listening on.
    pub port: u16,
    pub(crate) registry: Registry,
    pub(crate) _listener: Arc<ListenerGuard>,
}

impl fmt::Debug for PreviewServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreviewServer")
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

/// Start the preview server on a random loopback port, backed by a fresh
/// [`Registry`].
///
/// The listener thread is torn down when the last [`PreviewServer`] clone
/// drops; no thread is leaked if multiple servers are started in one process.
pub fn start_preview_server() -> io::Result<PreviewServer> {
    let registry = Registry::default();
    let shutdown = Arc::new(AtomicBool::new(false));
    let (port, handle) = spawn_listener(Arc::new(registry.clone()), Arc::clone(&shutdown))?;
    let listener = Arc::new(ListenerGuard {
        port,
        shutdown,
        handle: Mutex::new(Some(handle)),
    });
    Ok(PreviewServer {
        port,
        registry,
        _listener: listener,
    })
}

/// Inject the registry, port, and a keep-alive clone of `server` into a
/// [`LaunchBuilder`] so that [`StreamPreview`](crate::StreamPreview),
/// [`use_camera_stream`](crate::use_camera_stream), and consumers of
/// [`Registry`] can pick them up via `use_context`.
///
/// The keep-alive clone ensures the listener thread survives as long as the
/// app does, even if the original [`PreviewServer`] binding goes out of scope
/// before `launch(...)` runs.
///
/// ```no_run
/// use dioxus::prelude::*;
/// fn app() -> Element { rsx! { div { "hello" } } }
/// let server = dioxus_cameras::start_preview_server().unwrap();
/// dioxus_cameras::register_with(&server, dioxus::LaunchBuilder::desktop()).launch(app);
/// ```
pub fn register_with(server: &PreviewServer, launch: LaunchBuilder) -> LaunchBuilder {
    launch
        .with_context(server.registry.clone())
        .with_context(PreviewPort(server.port))
        .with_context(server.clone())
}

pub(crate) struct ListenerGuard {
    pub(crate) port: u16,
    pub(crate) shutdown: Arc<AtomicBool>,
    pub(crate) handle: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let addr: SocketAddr = match format!("127.0.0.1:{}", self.port).parse() {
            Ok(addr) => addr,
            Err(_) => return,
        };
        if let Ok(stream) = TcpStream::connect_timeout(&addr, SHUTDOWN_KICK_TIMEOUT) {
            let _ = stream.shutdown(Shutdown::Both);
        }
        if let Some(handle) = recover_lock(&self.handle).take() {
            let _ = handle.join();
        }
    }
}

fn spawn_listener<S: FrameSource>(
    source: Arc<S>,
    shutdown: Arc<AtomicBool>,
) -> io::Result<(u16, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    let handle = std::thread::Builder::new()
        .name("cameras-preview-server".into())
        .spawn(move || run_listener(listener, source, shutdown))?;
    Ok((port, handle))
}

fn run_listener<S: FrameSource>(listener: TcpListener, source: Arc<S>, shutdown: Arc<AtomicBool>) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let Ok((stream, _)) = listener.accept() else {
            break;
        };
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        let source = Arc::clone(&source);
        let _ = std::thread::Builder::new()
            .name("cameras-preview-conn".into())
            .spawn(move || {
                let _ = stream.set_nodelay(true);
                let _ = handle_connection(stream, source.as_ref());
            });
    }
}

fn handle_connection<S: FrameSource + ?Sized>(mut stream: TcpStream, source: &S) -> io::Result<()> {
    let mut request_buf = [0u8; 2048];
    loop {
        let n = stream.read(&mut request_buf)?;
        if n == 0 {
            return Ok(());
        }
        let id = parse_preview_id(&request_buf[..n]);
        write_response(&mut stream, source, id)?;
    }
}

fn parse_preview_id(request_bytes: &[u8]) -> Option<u32> {
    let text = std::str::from_utf8(request_bytes).ok()?;
    let path = text.split_whitespace().nth(1)?;
    let rest = path.strip_prefix("/preview/")?;
    let id_str = rest.strip_suffix(".bin")?;
    id_str.parse().ok()
}

fn write_response<S: FrameSource + ?Sized>(
    stream: &mut TcpStream,
    source: &S,
    id: Option<u32>,
) -> io::Result<()> {
    let parts = match id.and_then(|id| source.snapshot(id)) {
        Some((frame, counter)) => preview_parts(&frame, counter),
        None => PreviewParts {
            header: preview_header(PREVIEW_FORMAT_NONE, 0, 0, 0, 0),
            primary: None,
            secondary: None,
        },
    };
    let total_body_len = parts.header.len()
        + parts.primary.as_ref().map(|b| b.len()).unwrap_or(0)
        + parts.secondary.as_ref().map(|b| b.len()).unwrap_or(0);
    let http_header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: keep-alive\r\n\r\n",
        total_body_len
    );
    stream.write_all(http_header.as_bytes())?;
    stream.write_all(&parts.header)?;
    if let Some(primary) = &parts.primary {
        stream.write_all(primary)?;
    }
    if let Some(secondary) = &parts.secondary {
        stream.write_all(secondary)?;
    }
    Ok(())
}

struct PreviewParts {
    header: Vec<u8>,
    primary: Option<Bytes>,
    secondary: Option<Bytes>,
}

fn preview_parts(frame: &Frame, counter: u32) -> PreviewParts {
    match frame.pixel_format {
        PixelFormat::Nv12 => PreviewParts {
            header: preview_header(
                PREVIEW_FORMAT_NV12,
                frame.width,
                frame.height,
                frame.stride,
                counter,
            ),
            primary: Some(frame.plane_primary.clone()),
            secondary: Some(frame.plane_secondary.clone()),
        },
        PixelFormat::Bgra8 => {
            let stride = if frame.stride == 0 {
                frame.width * 4
            } else {
                frame.stride
            };
            PreviewParts {
                header: preview_header(
                    PREVIEW_FORMAT_BGRA,
                    frame.width,
                    frame.height,
                    stride,
                    counter,
                ),
                primary: Some(frame.plane_primary.clone()),
                secondary: None,
            }
        }
        _ => {
            let Ok(rgba) = cameras::to_rgba8(frame) else {
                return PreviewParts {
                    header: preview_header(PREVIEW_FORMAT_NONE, 0, 0, 0, counter),
                    primary: None,
                    secondary: None,
                };
            };
            let stride = frame.width * 4;
            PreviewParts {
                header: preview_header(
                    PREVIEW_FORMAT_RGBA,
                    frame.width,
                    frame.height,
                    stride,
                    counter,
                ),
                primary: Some(Bytes::from(rgba)),
                secondary: None,
            }
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_id_from_valid_get_request() {
        let request = b"GET /preview/42.bin HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n";
        assert_eq!(parse_preview_id(request), Some(42));
    }

    #[test]
    fn parse_id_from_zero() {
        let request = b"GET /preview/0.bin HTTP/1.1\r\n\r\n";
        assert_eq!(parse_preview_id(request), Some(0));
    }

    #[test]
    fn parse_id_rejects_non_numeric() {
        let request = b"GET /preview/abc.bin HTTP/1.1\r\n\r\n";
        assert_eq!(parse_preview_id(request), None);
    }

    #[test]
    fn parse_id_rejects_missing_extension() {
        let request = b"GET /preview/42 HTTP/1.1\r\n\r\n";
        assert_eq!(parse_preview_id(request), None);
    }

    #[test]
    fn parse_id_rejects_wrong_prefix() {
        let request = b"GET /other/42.bin HTTP/1.1\r\n\r\n";
        assert_eq!(parse_preview_id(request), None);
    }

    #[test]
    fn parse_id_rejects_empty_request() {
        assert_eq!(parse_preview_id(b""), None);
    }

    #[test]
    fn parse_id_rejects_invalid_utf8() {
        let bytes = [0xFF, 0xFE, 0xFD, 0xFC];
        assert_eq!(parse_preview_id(&bytes), None);
    }

    #[test]
    fn header_has_expected_length_and_magic() {
        let header = preview_header(PREVIEW_FORMAT_RGBA, 1920, 1080, 7680, 42);
        assert_eq!(header.len(), PREVIEW_HEADER_LEN);
        assert_eq!(&header[0..4], &PREVIEW_MAGIC);
        assert_eq!(header[4], PREVIEW_VERSION);
        assert_eq!(header[5], PREVIEW_FORMAT_RGBA);
        assert_eq!(header[6], 0);
        assert_eq!(header[7], 0);
    }

    #[test]
    fn header_fields_are_little_endian() {
        let width = 0x0000_0780_u32;
        let height = 0x0000_0438_u32;
        let stride = 0x0000_1E00_u32;
        let counter = 0xDEAD_BEEF_u32;
        let header = preview_header(PREVIEW_FORMAT_NV12, width, height, stride, counter);
        assert_eq!(&header[8..12], &width.to_le_bytes());
        assert_eq!(&header[12..16], &height.to_le_bytes());
        assert_eq!(&header[16..20], &stride.to_le_bytes());
        assert_eq!(&header[20..24], &counter.to_le_bytes());
    }

    #[test]
    fn header_for_empty_frame_has_zero_fields() {
        let header = preview_header(PREVIEW_FORMAT_NONE, 0, 0, 0, 0);
        assert_eq!(header.len(), PREVIEW_HEADER_LEN);
        assert_eq!(header[5], PREVIEW_FORMAT_NONE);
        assert!(header[8..].iter().all(|&b| b == 0));
    }
}
