//! Drive a camera with `cameras::pump`: stream frames into a caller-provided
//! sink closure, pause and resume the pump without closing the camera, and
//! grab fresh snapshots on demand.
//!
//! This is the template for integrating cameras into a non-Dioxus app (a
//! CLI, an egui / iced window, a Tauri shell, a background daemon). The sink
//! closure receives each frame; do whatever your app needs with it.
//!
//! ```bash
//! cargo run --example pump
//! ```

use std::error::Error;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

use cameras::pump;
use cameras::{PixelFormat, Resolution, StreamConfig};

fn main() -> Result<(), Box<dyn Error>> {
    let devices = cameras::devices()?;
    let device = devices.first().ok_or("no cameras connected")?;
    println!("opening {}", device.name);

    let config = StreamConfig {
        resolution: Resolution {
            width: 1280,
            height: 720,
        },
        framerate: 30,
        pixel_format: PixelFormat::Bgra8,
    };
    let camera = cameras::open(device, config)?;

    let start = Instant::now();
    let received = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&received);

    let pump = pump::spawn(camera, move |_frame| {
        // Replace this with whatever your app does with frames: push to a
        // channel, update an egui texture, write to disk, forward to an ML
        // model, etc.
        counter.fetch_add(1, Ordering::Relaxed);
    });

    let stamp = |msg: &str| {
        println!("[{:.1}s] {msg}", start.elapsed().as_secs_f32());
    };

    stamp("pump spawned, streaming");
    sleep(Duration::from_secs(2));
    stamp(&format!(
        "received {} frames",
        received.load(Ordering::Relaxed)
    ));

    stamp("pausing pump");
    pump::set_active(&pump, false);
    sleep(Duration::from_millis(200));

    stamp("requesting capture_frame while paused");
    match pump::capture_frame(&pump) {
        Some(frame) => stamp(&format!(
            "got {}x{} {:?} frame",
            frame.width, frame.height, frame.pixel_format
        )),
        None => stamp("capture failed"),
    }

    stamp("resuming pump");
    pump::set_active(&pump, true);
    let baseline = received.load(Ordering::Relaxed);
    sleep(Duration::from_secs(2));
    stamp(&format!(
        "received {} additional frames after resume",
        received.load(Ordering::Relaxed) - baseline
    ));

    stamp("stop_and_join");
    pump::stop_and_join(pump);
    Ok(())
}
