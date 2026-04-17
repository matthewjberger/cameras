//! Runtime camera controls — focus, exposure, white-balance, PTZ, and related
//! image adjustments.
//!
//! The types here describe "what a camera can do" ([`ControlCapabilities`],
//! [`ControlRange`]), "what the caller wants to set" ([`Controls`]), and a
//! stable enumeration of every control ([`ControlKind`]). The three free
//! functions at the bottom ([`control_capabilities`], [`read_controls`],
//! [`apply_controls`]) dispatch through the active platform backend.
//!
//! Every item in this module is gated on the `controls` Cargo feature.

use crate::ActiveBackend;
use crate::backend::BackendControls;
use crate::error::Error;
use crate::types::Device;

/// AC mains frequency choice for cameras that support power-line-frequency filtering.
///
/// Supported on Linux via `V4L2_CID_POWER_LINE_FREQUENCY` and on Windows via
/// `IAMVideoProcAmp`'s `VideoProcAmp_PowerLineFrequency` property (id `10`).
/// macOS reports [`None`] for this capability — AVFoundation does not expose it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PowerLineFrequency {
    /// Flicker-suppression disabled.
    Disabled,
    /// 50 Hz mains.
    Hz50,
    /// 60 Hz mains.
    Hz60,
    /// Hardware auto-detects mains frequency.
    Auto,
}

/// Requested tweaks to a device's runtime controls.
///
/// Each field uses [`Option::None`] to mean "leave the current value alone"
/// and [`Option::Some`] to mean "apply this value." Values are in each
/// platform's native range; consult [`ControlCapabilities`] for the exact
/// endpoints before writing.
///
/// Platforms reject out-of-range or unsupported writes with
/// [`crate::Error::Unsupported`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Controls {
    /// Manual focus position. See [`ControlCapabilities::focus`] for range semantics.
    pub focus: Option<f32>,
    /// Enable (`true`) or disable (`false`) continuous auto-focus.
    pub auto_focus: Option<bool>,
    /// Manual exposure value in each platform's native unit (seconds on macOS, microseconds on Linux).
    pub exposure: Option<f32>,
    /// Enable (`true`) or disable (`false`) auto-exposure. Read-back collapses V4L2 priority modes (shutter/aperture priority) into `Some(true)`; write-back of `Some(true)` applies full AUTO (value 0).
    pub auto_exposure: Option<bool>,
    /// Manual white-balance temperature (Kelvin on Linux, synthesized via gains round-trip on macOS).
    pub white_balance_temperature: Option<f32>,
    /// Enable (`true`) or disable (`false`) auto white balance.
    pub auto_white_balance: Option<bool>,
    /// Image brightness in native units.
    pub brightness: Option<f32>,
    /// Image contrast in native units.
    pub contrast: Option<f32>,
    /// Image saturation in native units.
    pub saturation: Option<f32>,
    /// Image sharpness in native units.
    pub sharpness: Option<f32>,
    /// Sensor gain in native units (ISO on macOS).
    pub gain: Option<f32>,
    /// Backlight compensation in native units.
    pub backlight_compensation: Option<f32>,
    /// AC power-line frequency for flicker suppression.
    pub power_line_frequency: Option<PowerLineFrequency>,
    /// Pan axis in native units. PTZ-capable devices only.
    pub pan: Option<f32>,
    /// Tilt axis in native units. PTZ-capable devices only.
    pub tilt: Option<f32>,
    /// Zoom factor in native units. PTZ-capable devices only.
    pub zoom: Option<f32>,
}

/// Reported range for one numeric camera control.
///
/// All fields are in the platform's native unit for the control — do not
/// assume a normalized 0..1 scale. Read endpoints from this struct before
/// constructing [`Controls`] values.
#[derive(Copy, Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ControlRange {
    /// Minimum accepted value, inclusive.
    pub min: f32,
    /// Maximum accepted value, inclusive.
    pub max: f32,
    /// Smallest step between accepted values. `0.0` means continuous.
    pub step: f32,
    /// Factory default value.
    pub default: f32,
}

/// Power-line-frequency capability detail on devices that expose it.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct PowerLineFrequencyCapability {
    /// `true` if 50 Hz filtering is selectable on this device.
    pub hz50: bool,
    /// `true` if 60 Hz filtering is selectable on this device.
    pub hz60: bool,
    /// `true` if the "off" mode is selectable on this device.
    pub disabled: bool,
    /// `true` if hardware auto-detect mode is selectable on this device.
    pub auto: bool,
    /// Factory default mode.
    pub default: PowerLineFrequency,
}

/// Identifier for every control field on [`Controls`] and [`ControlCapabilities`].
///
/// Useful for UI iteration, config serialization, and fetching platform-scoped
/// caveats via [`ControlKind::caveat`]. Iterate [`ControlKind::ALL`] to visit
/// every control in a stable order.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ControlKind {
    /// Manual focus position.
    Focus,
    /// Auto-focus toggle.
    AutoFocus,
    /// Manual exposure value.
    Exposure,
    /// Auto-exposure toggle.
    AutoExposure,
    /// Manual white-balance temperature.
    WhiteBalanceTemperature,
    /// Auto-white-balance toggle.
    AutoWhiteBalance,
    /// Image brightness.
    Brightness,
    /// Image contrast.
    Contrast,
    /// Image saturation.
    Saturation,
    /// Image sharpness.
    Sharpness,
    /// Sensor gain (ISO on macOS).
    Gain,
    /// Backlight compensation.
    BacklightCompensation,
    /// AC mains frequency filtering.
    PowerLineFrequency,
    /// Pan axis (PTZ-capable devices only).
    Pan,
    /// Tilt axis (PTZ-capable devices only).
    Tilt,
    /// Zoom factor (PTZ-capable devices only).
    Zoom,
}

impl ControlKind {
    /// Every [`ControlKind`] variant in declaration order.
    pub const ALL: [ControlKind; 16] = [
        ControlKind::Focus,
        ControlKind::AutoFocus,
        ControlKind::Exposure,
        ControlKind::AutoExposure,
        ControlKind::WhiteBalanceTemperature,
        ControlKind::AutoWhiteBalance,
        ControlKind::Brightness,
        ControlKind::Contrast,
        ControlKind::Saturation,
        ControlKind::Sharpness,
        ControlKind::Gain,
        ControlKind::BacklightCompensation,
        ControlKind::PowerLineFrequency,
        ControlKind::Pan,
        ControlKind::Tilt,
        ControlKind::Zoom,
    ];

    /// Snake_case name matching the corresponding field on [`Controls`].
    pub fn label(&self) -> &'static str {
        match self {
            ControlKind::Focus => "focus",
            ControlKind::AutoFocus => "auto_focus",
            ControlKind::Exposure => "exposure",
            ControlKind::AutoExposure => "auto_exposure",
            ControlKind::WhiteBalanceTemperature => "white_balance_temperature",
            ControlKind::AutoWhiteBalance => "auto_white_balance",
            ControlKind::Brightness => "brightness",
            ControlKind::Contrast => "contrast",
            ControlKind::Saturation => "saturation",
            ControlKind::Sharpness => "sharpness",
            ControlKind::Gain => "gain",
            ControlKind::BacklightCompensation => "backlight_compensation",
            ControlKind::PowerLineFrequency => "power_line_frequency",
            ControlKind::Pan => "pan",
            ControlKind::Tilt => "tilt",
            ControlKind::Zoom => "zoom",
        }
    }

    /// Platform-specific caveat for this control on the current target, if any.
    ///
    /// Returns `Some` only when the current target cannot expose the control
    /// regardless of device — useful as UI tooltip text explaining why a
    /// capability row is marked unsupported. Currently populated for macOS
    /// controls that AVFoundation does not surface.
    pub fn caveat(&self) -> Option<&'static str> {
        #[cfg(target_os = "macos")]
        {
            match self {
                ControlKind::Brightness
                | ControlKind::Contrast
                | ControlKind::Saturation
                | ControlKind::Sharpness
                | ControlKind::BacklightCompensation => Some(
                    "macOS: AVFoundation doesn't expose per-channel image-processing controls. \
                     Apply CPU/GPU post-processing (shaders, color matrices) over the Frame \
                     bytes in your app. The library is capture-only.",
                ),
                ControlKind::PowerLineFrequency => {
                    Some("macOS: AVFoundation doesn't expose AC mains frequency filtering.")
                }
                ControlKind::Pan | ControlKind::Tilt => Some(
                    "macOS: AVFoundation doesn't expose pan/tilt controls for built-in or UVC cameras.",
                ),
                ControlKind::Focus
                | ControlKind::AutoFocus
                | ControlKind::Exposure
                | ControlKind::AutoExposure
                | ControlKind::WhiteBalanceTemperature
                | ControlKind::AutoWhiteBalance
                | ControlKind::Gain
                | ControlKind::Zoom => None,
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }
}

/// What a device reports it can do, per control.
///
/// Each field is [`Some`] when the platform exposes the control on this
/// device and [`None`] when it does not. For numeric controls, `Some` carries
/// the native [`ControlRange`]. For auto toggles, `Some(true)` means the
/// device supports auto, `Some(false)` means manual-only, `None` means no
/// auto control.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ControlCapabilities {
    /// Focus-position capability.
    pub focus: Option<ControlRange>,
    /// Auto-focus toggle capability.
    pub auto_focus: Option<bool>,
    /// Exposure-value capability.
    pub exposure: Option<ControlRange>,
    /// Auto-exposure toggle capability.
    pub auto_exposure: Option<bool>,
    /// White-balance-temperature capability.
    pub white_balance_temperature: Option<ControlRange>,
    /// Auto-white-balance toggle capability.
    pub auto_white_balance: Option<bool>,
    /// Brightness capability.
    pub brightness: Option<ControlRange>,
    /// Contrast capability.
    pub contrast: Option<ControlRange>,
    /// Saturation capability.
    pub saturation: Option<ControlRange>,
    /// Sharpness capability.
    pub sharpness: Option<ControlRange>,
    /// Gain capability.
    pub gain: Option<ControlRange>,
    /// Backlight-compensation capability.
    pub backlight_compensation: Option<ControlRange>,
    /// Power-line-frequency capability.
    pub power_line_frequency: Option<PowerLineFrequencyCapability>,
    /// Pan capability.
    pub pan: Option<ControlRange>,
    /// Tilt capability.
    pub tilt: Option<ControlRange>,
    /// Zoom capability.
    pub zoom: Option<ControlRange>,
}

/// Report which runtime controls the given device exposes and their native ranges.
///
/// Fields on the returned [`ControlCapabilities`] are `None` for controls the
/// platform / device does not expose. Ranges are in each platform's native
/// unit — do not assume a normalized scale.
pub fn control_capabilities(device: &Device) -> Result<ControlCapabilities, Error> {
    <ActiveBackend as BackendControls>::control_capabilities(&device.id)
}

/// Read the current value of every exposed control on `device`.
///
/// Fields are `None` for controls the device does not expose. Read-back of
/// `auto_exposure` collapses V4L2 priority modes into `Some(true)`.
pub fn read_controls(device: &Device) -> Result<Controls, Error> {
    <ActiveBackend as BackendControls>::read_controls(&device.id)
}

/// Apply every [`Some`]-valued field in `controls` to `device`.
///
/// `None` fields are left at their current value. Returns the first platform
/// failure encountered; does not preflight against [`control_capabilities`].
pub fn apply_controls(device: &Device, controls: &Controls) -> Result<(), Error> {
    <ActiveBackend as BackendControls>::apply_controls(&device.id, controls)
}

/// Build a [`Controls`] that, when applied, returns every exposed control to
/// a sensible "factory" state.
///
/// For axes that have an auto mode (focus, exposure, white-balance
/// temperature), prefers enabling auto over writing a manual default: on
/// most UVC devices writing a manual value implicitly disables auto, so
/// leaving the numeric field `None` lets the camera's own AE / AF / AWB
/// algorithms converge instead of pinning a stale value. When auto is not
/// available on an axis, the numeric field falls back to
/// [`ControlRange::default`].
///
/// Orphan numeric fields (brightness, contrast, saturation, sharpness, gain,
/// backlight_compensation, pan, tilt, zoom, power_line_frequency) always
/// carry their platform-reported default, because UVC has no auto mode for
/// image-adjustment knobs.
///
/// Fields the device does not expose stay `None`.
///
/// Platform caveats apply: V4L2 reports genuine driver defaults; Media
/// Foundation reports UVC-populated defaults which most drivers honor;
/// AVFoundation synthesizes defaults from current-state reads rather than
/// tracking true factory values.
pub fn default_controls(capabilities: &ControlCapabilities) -> Controls {
    let auto_toggle = |supported: Option<bool>| match supported {
        Some(true) => Some(true),
        _ => None,
    };
    let numeric_with_auto_fallback = |range: Option<&ControlRange>, auto: Option<bool>| {
        if auto == Some(true) {
            None
        } else {
            range.map(|range| range.default)
        }
    };
    Controls {
        focus: numeric_with_auto_fallback(capabilities.focus.as_ref(), capabilities.auto_focus),
        auto_focus: auto_toggle(capabilities.auto_focus),
        exposure: numeric_with_auto_fallback(
            capabilities.exposure.as_ref(),
            capabilities.auto_exposure,
        ),
        auto_exposure: auto_toggle(capabilities.auto_exposure),
        white_balance_temperature: numeric_with_auto_fallback(
            capabilities.white_balance_temperature.as_ref(),
            capabilities.auto_white_balance,
        ),
        auto_white_balance: auto_toggle(capabilities.auto_white_balance),
        brightness: capabilities.brightness.as_ref().map(|range| range.default),
        contrast: capabilities.contrast.as_ref().map(|range| range.default),
        saturation: capabilities.saturation.as_ref().map(|range| range.default),
        sharpness: capabilities.sharpness.as_ref().map(|range| range.default),
        gain: capabilities.gain.as_ref().map(|range| range.default),
        backlight_compensation: capabilities
            .backlight_compensation
            .as_ref()
            .map(|range| range.default),
        power_line_frequency: capabilities
            .power_line_frequency
            .as_ref()
            .map(|capability| capability.default),
        pan: capabilities.pan.as_ref().map(|range| range.default),
        tilt: capabilities.tilt.as_ref().map(|range| range.default),
        zoom: capabilities.zoom.as_ref().map(|range| range.default),
    }
}

/// Probe the device's capabilities, build a defaults [`Controls`] via
/// [`default_controls`], and apply it in one call.
///
/// Intended as a "the camera looks wrong, start over" escape hatch for UIs.
/// See [`default_controls`] for the platform-specific meaning of "default."
pub fn reset_to_defaults(device: &Device) -> Result<(), Error> {
    let capabilities = control_capabilities(device)?;
    let controls = default_controls(&capabilities);
    apply_controls(device, &controls)
}
