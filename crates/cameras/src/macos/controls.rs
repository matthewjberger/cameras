use objc2_av_foundation::{
    AVCaptureExposureMode, AVCaptureFocusMode, AVCaptureWhiteBalanceGains,
    AVCaptureWhiteBalanceMode,
};
use objc2_core_media::CMTime;

use crate::error::Error;
use crate::macos::enumerate::find_device;
use crate::types::{ControlCapabilities, ControlRange, Controls, DeviceId};

pub(crate) fn control_capabilities(id: &DeviceId) -> Result<ControlCapabilities, Error> {
    let device = find_device(id)?;
    let format = unsafe { device.activeFormat() };

    let focus_range = if unsafe { device.isLockingFocusWithCustomLensPositionSupported() } {
        let lens_position = unsafe { device.lensPosition() };
        Some(ControlRange {
            min: 0.0,
            max: 1.0,
            step: 0.0,
            default: lens_position,
        })
    } else {
        None
    };

    let auto_focus =
        if unsafe { device.isFocusModeSupported(AVCaptureFocusMode::ContinuousAutoFocus) } {
            Some(true)
        } else if unsafe { device.isFocusModeSupported(AVCaptureFocusMode::Locked) } {
            Some(false)
        } else {
            None
        };

    let min_exposure = unsafe { format.minExposureDuration() };
    let max_exposure = unsafe { format.maxExposureDuration() };
    let exposure_range = if cm_time_is_positive(min_exposure) && cm_time_is_positive(max_exposure) {
        Some(ControlRange {
            min: cm_time_to_seconds(min_exposure),
            max: cm_time_to_seconds(max_exposure),
            step: 0.0,
            default: cm_time_to_seconds(unsafe { device.exposureDuration() }),
        })
    } else {
        None
    };

    let auto_exposure =
        if unsafe { device.isExposureModeSupported(AVCaptureExposureMode::ContinuousAutoExposure) }
        {
            Some(true)
        } else if unsafe { device.isExposureModeSupported(AVCaptureExposureMode::Custom) } {
            Some(false)
        } else {
            None
        };

    let iso_range = {
        let min_iso = unsafe { format.minISO() };
        let max_iso = unsafe { format.maxISO() };
        if max_iso > min_iso {
            Some(ControlRange {
                min: min_iso,
                max: max_iso,
                step: 0.0,
                default: unsafe { device.ISO() },
            })
        } else {
            None
        }
    };

    let auto_white_balance = if unsafe {
        device.isWhiteBalanceModeSupported(AVCaptureWhiteBalanceMode::ContinuousAutoWhiteBalance)
    } {
        Some(true)
    } else if unsafe { device.isWhiteBalanceModeSupported(AVCaptureWhiteBalanceMode::Locked) } {
        Some(false)
    } else {
        None
    };

    let zoom_range = {
        let min_zoom = unsafe { device.minAvailableVideoZoomFactor() };
        let max_zoom = unsafe { device.maxAvailableVideoZoomFactor() };
        let current_zoom = unsafe { device.videoZoomFactor() };
        if max_zoom > min_zoom {
            Some(ControlRange {
                min: min_zoom as f32,
                max: max_zoom as f32,
                step: 0.0,
                default: current_zoom as f32,
            })
        } else {
            None
        }
    };

    Ok(ControlCapabilities {
        focus: focus_range,
        auto_focus,
        exposure: exposure_range,
        auto_exposure,
        white_balance_temperature: None,
        auto_white_balance,
        brightness: None,
        contrast: None,
        saturation: None,
        sharpness: None,
        gain: iso_range,
        backlight_compensation: None,
        power_line_frequency: None,
        pan: None,
        tilt: None,
        zoom: zoom_range,
    })
}

pub(crate) fn read_controls(id: &DeviceId) -> Result<Controls, Error> {
    let device = find_device(id)?;
    let focus = Some(unsafe { device.lensPosition() });
    let auto_focus = Some(matches!(
        unsafe { device.focusMode() },
        AVCaptureFocusMode::ContinuousAutoFocus | AVCaptureFocusMode::AutoFocus
    ));
    let exposure = Some(cm_time_to_seconds(unsafe { device.exposureDuration() }));
    let auto_exposure = Some(matches!(
        unsafe { device.exposureMode() },
        AVCaptureExposureMode::ContinuousAutoExposure | AVCaptureExposureMode::AutoExpose
    ));
    let gain = Some(unsafe { device.ISO() });
    let auto_white_balance = Some(matches!(
        unsafe { device.whiteBalanceMode() },
        AVCaptureWhiteBalanceMode::ContinuousAutoWhiteBalance
            | AVCaptureWhiteBalanceMode::AutoWhiteBalance
    ));
    let zoom = Some(unsafe { device.videoZoomFactor() } as f32);

    let gains = unsafe { device.deviceWhiteBalanceGains() };
    let white_balance_temperature = if white_balance_gains_valid(&gains) {
        let temp_tint = unsafe { device.temperatureAndTintValuesForDeviceWhiteBalanceGains(gains) };
        Some(temp_tint.temperature)
    } else {
        None
    };

    Ok(Controls {
        focus,
        auto_focus,
        exposure,
        auto_exposure,
        white_balance_temperature,
        auto_white_balance,
        brightness: None,
        contrast: None,
        saturation: None,
        sharpness: None,
        gain,
        backlight_compensation: None,
        power_line_frequency: None,
        pan: None,
        tilt: None,
        zoom,
    })
}

pub(crate) fn apply_controls(id: &DeviceId, controls: &Controls) -> Result<(), Error> {
    reject_unsupported(controls)?;

    let device = find_device(id)?;
    unsafe { device.lockForConfiguration() }.map_err(|error| Error::Backend {
        platform: "macos",
        message: error.to_string(),
    })?;

    let result = apply_inside_lock(&device, controls);

    unsafe { device.unlockForConfiguration() };
    result
}

fn reject_unsupported(controls: &Controls) -> Result<(), Error> {
    if controls.auto_exposure == Some(true)
        && (controls.exposure.is_some() || controls.gain.is_some())
    {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "auto_exposure_with_explicit_exposure_or_gain",
        });
    }
    if controls.brightness.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "brightness",
        });
    }
    if controls.contrast.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "contrast",
        });
    }
    if controls.saturation.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "saturation",
        });
    }
    if controls.sharpness.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "sharpness",
        });
    }
    if controls.backlight_compensation.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "backlight_compensation",
        });
    }
    if controls.power_line_frequency.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "power_line_frequency",
        });
    }
    if controls.pan.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "pan",
        });
    }
    if controls.tilt.is_some() {
        return Err(Error::Unsupported {
            platform: "macos",
            reason: "tilt",
        });
    }
    Ok(())
}

fn apply_inside_lock(
    device: &objc2::rc::Retained<objc2_av_foundation::AVCaptureDevice>,
    controls: &Controls,
) -> Result<(), Error> {
    if let Some(enabled) = controls.auto_focus {
        let mode = if enabled {
            AVCaptureFocusMode::ContinuousAutoFocus
        } else {
            AVCaptureFocusMode::Locked
        };
        if !unsafe { device.isFocusModeSupported(mode) } {
            return Err(Error::Unsupported {
                platform: "macos",
                reason: "auto_focus",
            });
        }
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            device.setFocusMode(mode);
        }));
        if let Err(exception) = result {
            return Err(objc_exception_to_error(exception, "auto_focus"));
        }
    }

    if let Some(enabled) = controls.auto_exposure {
        let mode = if enabled {
            AVCaptureExposureMode::ContinuousAutoExposure
        } else {
            AVCaptureExposureMode::Custom
        };
        if !unsafe { device.isExposureModeSupported(mode) } {
            return Err(Error::Unsupported {
                platform: "macos",
                reason: "auto_exposure",
            });
        }
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            device.setExposureMode(mode);
        }));
        if let Err(exception) = result {
            return Err(objc_exception_to_error(exception, "auto_exposure"));
        }
    }

    if let Some(enabled) = controls.auto_white_balance {
        let mode = if enabled {
            AVCaptureWhiteBalanceMode::ContinuousAutoWhiteBalance
        } else {
            AVCaptureWhiteBalanceMode::Locked
        };
        if !unsafe { device.isWhiteBalanceModeSupported(mode) } {
            return Err(Error::Unsupported {
                platform: "macos",
                reason: "auto_white_balance",
            });
        }
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            device.setWhiteBalanceMode(mode);
        }));
        if let Err(exception) = result {
            return Err(objc_exception_to_error(exception, "auto_white_balance"));
        }
    }

    if let Some(position) = controls.focus {
        if !unsafe { device.isLockingFocusWithCustomLensPositionSupported() } {
            return Err(Error::Unsupported {
                platform: "macos",
                reason: "focus",
            });
        }
        let clamped = position.clamp(0.0, 1.0);
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            device.setFocusModeLockedWithLensPosition_completionHandler(clamped, None);
        }));
        if let Err(exception) = result {
            return Err(objc_exception_to_error(exception, "focus"));
        }
    }

    if controls.exposure.is_some() || controls.gain.is_some() {
        let format = unsafe { device.activeFormat() };
        let duration = match controls.exposure {
            Some(seconds) => seconds_to_cm_time(seconds),
            None => unsafe { device.exposureDuration() },
        };
        let iso = match controls.gain {
            Some(value) => {
                let min = unsafe { format.minISO() };
                let max = unsafe { format.maxISO() };
                value.clamp(min, max)
            }
            None => unsafe { device.ISO() },
        };
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            device.setExposureModeCustomWithDuration_ISO_completionHandler(duration, iso, None);
        }));
        if let Err(exception) = result {
            return Err(objc_exception_to_error(exception, "exposure_iso"));
        }
    }

    if let Some(temperature) = controls.white_balance_temperature {
        if !unsafe { device.isLockingWhiteBalanceWithCustomDeviceGainsSupported() } {
            return Err(Error::Unsupported {
                platform: "macos",
                reason: "white_balance_temperature",
            });
        }
        let current_gains = unsafe { device.deviceWhiteBalanceGains() };
        let current_tint = if white_balance_gains_valid(&current_gains) {
            let current_temp_tint =
                unsafe { device.temperatureAndTintValuesForDeviceWhiteBalanceGains(current_gains) };
            current_temp_tint.tint
        } else {
            0.0
        };
        let temp_tint = objc2_av_foundation::AVCaptureWhiteBalanceTemperatureAndTintValues {
            temperature,
            tint: current_tint,
        };
        let gains = unsafe { device.deviceWhiteBalanceGainsForTemperatureAndTintValues(temp_tint) };
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            device.setWhiteBalanceModeLockedWithDeviceWhiteBalanceGains_completionHandler(
                gains, None,
            );
        }));
        if let Err(exception) = result {
            return Err(objc_exception_to_error(
                exception,
                "white_balance_temperature",
            ));
        }
    }

    if let Some(zoom) = controls.zoom {
        let min_zoom = unsafe { device.minAvailableVideoZoomFactor() };
        let max_zoom = unsafe { device.maxAvailableVideoZoomFactor() };
        let clamped = (zoom as f64).clamp(min_zoom, max_zoom);
        let result = objc2::exception::catch(std::panic::AssertUnwindSafe(|| unsafe {
            device.setVideoZoomFactor(clamped);
        }));
        if let Err(exception) = result {
            return Err(objc_exception_to_error(exception, "zoom"));
        }
    }

    Ok(())
}

fn cm_time_is_positive(time: CMTime) -> bool {
    time.value > 0 && time.timescale > 0
}

fn cm_time_to_seconds(time: CMTime) -> f32 {
    if time.timescale == 0 {
        return 0.0;
    }
    (time.value as f64 / time.timescale as f64) as f32
}

fn seconds_to_cm_time(seconds: f32) -> CMTime {
    let timescale: i32 = 1_000_000;
    let value = (seconds as f64 * timescale as f64).round() as i64;
    CMTime {
        value,
        timescale,
        flags: objc2_core_media::CMTimeFlags::Valid,
        epoch: 0,
    }
}

fn white_balance_gains_valid(gains: &AVCaptureWhiteBalanceGains) -> bool {
    gains.redGain > 0.0 && gains.greenGain > 0.0 && gains.blueGain > 0.0
}

fn objc_exception_to_error(
    exception: Option<objc2::rc::Retained<objc2::exception::Exception>>,
    reason: &'static str,
) -> Error {
    let message = exception
        .as_ref()
        .map(|value| value.to_string())
        .unwrap_or_else(|| "ObjC exception (nil)".into());
    Error::Backend {
        platform: "macos",
        message: format!("{reason}: {message}"),
    }
}
