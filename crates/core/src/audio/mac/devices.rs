//! Capture device list (FR-13).

use cpal::traits::{DeviceTrait, HostTrait};

use crate::audio::source::Device;

fn name_of(device: &cpal::Device) -> Option<String> {
    device.description().ok().map(|d| d.name().to_string())
}

pub fn list_mic_devices() -> anyhow::Result<Vec<Device>> {
    let host = cpal::default_host();
    let default_id = host.default_input_device().and_then(|d| d.id().ok()).map(|id| id.to_string());
    let mut out = Vec::new();
    for device in host.input_devices()? {
        let Ok(id) = device.id().map(|id| id.to_string()) else { continue };
        let name = name_of(&device).unwrap_or_else(|| id.clone());
        out.push(Device { is_default: default_id.as_deref() == Some(id.as_str()), id, name });
    }
    out.sort_by(|a, b| b.is_default.cmp(&a.is_default).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// The input device with this ID (`None`: the system's default input).
pub fn mic_device(device_id: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    match device_id {
        Some(id) => host.device_by_id(&id.parse().ok()?),
        None => host.default_input_device(),
    }
}

/// Name of a capture device, or of the default input.
pub fn mic_device_name(device_id: Option<String>) -> Option<String> {
    name_of(&mic_device(device_id.as_deref())?)
}

/// ID of the device a default-following stream would open right now.
pub fn current_default_id(system: bool) -> Option<String> {
    let host = cpal::default_host();
    let device = if system { host.default_output_device() } else { host.default_input_device() }?;
    device.id().ok().map(|id| id.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn listing_devices_does_not_fail() {
        let devices = super::list_mic_devices().unwrap();
        assert!(devices.iter().filter(|d| d.is_default).count() <= 1);
    }
}
