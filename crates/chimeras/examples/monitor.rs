//! Print camera hotplug events as they happen.
//!
//! Demonstrates `chimeras::monitor` and `chimeras::next_event`. The monitor
//! emits an `Added` event for every device already present when it starts,
//! then streams live `Added` / `Removed` events as cameras appear and
//! disappear.
//!
//! ```bash
//! cargo run --example monitor   # watches until Ctrl-C
//! ```

use std::error::Error;
use std::time::Duration;

use chimeras::{DeviceEvent, Error as ChimerasError};

fn main() -> Result<(), Box<dyn Error>> {
    let monitor = chimeras::monitor()?;
    println!("watching for camera hotplug (Ctrl-C to exit)");

    loop {
        match chimeras::next_event(&monitor, Duration::from_secs(1)) {
            Ok(DeviceEvent::Added(device)) => println!("+ {} ({})", device.name, device.id.0),
            Ok(DeviceEvent::Removed(id)) => println!("- {}", id.0),
            Err(ChimerasError::Timeout) => continue,
            Err(err) => return Err(err.into()),
        }
    }
}
