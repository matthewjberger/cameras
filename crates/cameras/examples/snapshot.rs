//! Open the first connected camera, pull one frame, and write it to disk as a
//! PNG. The template for any non-Dioxus "take a picture" use case.
//!
//! ```bash
//! cargo run --example snapshot                  # writes snapshot.png
//! cargo run --example snapshot -- photo.png     # writes photo.png
//! ```

use std::error::Error;
use std::time::Duration;

use cameras::{PixelFormat, Resolution, StreamConfig};
use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "snapshot.png".into());

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
    let frame = cameras::next_frame(&camera, Duration::from_secs(2))?;
    let rgba = cameras::to_rgba8(&frame)?;

    let file = std::fs::File::create(&path)?;
    let encoder = PngEncoder::new(std::io::BufWriter::new(file));
    encoder.write_image(&rgba, frame.width, frame.height, ExtendedColorType::Rgba8)?;

    println!("wrote {}x{} frame to {}", frame.width, frame.height, path);
    Ok(())
}
