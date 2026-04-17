use std::time::Duration;

use cameras::Device;
use dioxus::prelude::*;

use crate::channel::Channel;

const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Handle returned by [`use_devices`].
#[derive(Copy, Clone, PartialEq)]
pub struct UseDevices {
    /// The current list of cameras reported by [`cameras::devices`].
    ///
    /// Updated asynchronously by `refresh`, the UI thread is never blocked
    /// by platform device enumeration.
    pub devices: Signal<Vec<Device>>,
    /// Flips from `false` to `true` once the first refresh completes.
    ///
    /// Lets the UI distinguish "we haven't scanned yet" (show a loading
    /// state) from "we scanned and found no cameras" (show an empty state).
    /// Remains `true` for the rest of the component's lifetime.
    pub ready: Signal<bool>,
    /// Callback that rescans the platform for cameras and updates `devices`.
    ///
    /// Runs `cameras::devices()` on a worker thread. Errors are swallowed;
    /// the signal stays at its previous value.
    pub refresh: Callback<()>,
}

/// Hook that keeps a [`Signal<Vec<Device>>`] populated with the current camera
/// list.
///
/// Refreshes once on mount. Call `refresh.call(())` (e.g. from a "Refresh"
/// button's `onclick`) to rescan on demand. Enumeration runs on a worker
/// thread so the UI thread never stalls while the platform scans hardware.
pub fn use_devices() -> UseDevices {
    let mut devices = use_signal(Vec::<Device>::new);
    let mut ready = use_signal(|| false);
    let channel = use_hook(Channel::<Vec<Device>>::new);

    let poll_channel = channel.clone();
    use_hook(move || {
        spawn(async move {
            loop {
                futures_timer::Delay::new(POLL_INTERVAL).await;
                if let Some(latest) = poll_channel.drain().into_iter().last() {
                    devices.set(latest);
                    if !*ready.peek() {
                        ready.set(true);
                    }
                }
            }
        })
    });

    let refresh_tx = channel.sender.clone();
    let refresh = use_callback(move |()| {
        let tx = refresh_tx.clone();
        let _ = std::thread::Builder::new()
            .name("cameras-devices".into())
            .spawn(move || {
                if let Ok(list) = cameras::devices() {
                    let _ = tx.send(list);
                }
            });
    });

    use_effect(move || refresh.call(()));

    UseDevices {
        devices,
        ready,
        refresh,
    }
}
