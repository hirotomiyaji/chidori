//! Audio capture and per-source DSP.

pub mod clock;
pub mod decode;
pub mod dsp;
pub mod file_source;
pub mod level;
#[cfg(target_os = "macos")]
pub mod mac;
pub mod resample;
pub mod source;
#[cfg(windows)]
pub mod win;

use crossbeam_channel::Sender;

use crate::session::model::SourceId;
use source::{AudioSource, CaptureTarget, Device, SourceStatus};

/// This OS's capture stream for a target (section 6).
#[cfg(any(windows, target_os = "macos"))]
pub fn open_source(id: SourceId, target: CaptureTarget, status: Sender<SourceStatus>) -> Box<dyn AudioSource> {
    #[cfg(windows)]
    {
        Box::new(win::capture::WasapiSource::new(id, target, status))
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(mac::capture::CoreAudioSource::new(id, target, status))
    }
}

/// The mics to choose from, the default first (FR-13).
pub fn list_mic_devices() -> anyhow::Result<Vec<Device>> {
    #[cfg(windows)]
    {
        win::devices::list_mic_devices()
    }
    #[cfg(target_os = "macos")]
    {
        mac::devices::list_mic_devices()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        Ok(Vec::new())
    }
}

/// Name of a capture device, or of the default one.
pub fn mic_device_name(device_id: Option<String>) -> Option<String> {
    #[cfg(windows)]
    {
        win::devices::mic_device_name(device_id)
    }
    #[cfg(target_os = "macos")]
    {
        mac::devices::mic_device_name(device_id)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = device_id;
        None
    }
}

/// The running root process of this executable, or 0.
pub fn find_app_root(exe: &std::path::Path) -> u32 {
    #[cfg(windows)]
    {
        win::process::find_root_by_exe(&win::process::snapshot(), exe).unwrap_or(0)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        0
    }
}

/// A process and its descendants (an app's helper processes).
pub fn process_tree(root_pid: u32) -> Vec<u32> {
    #[cfg(windows)]
    {
        win::process::tree_of(&win::process::snapshot(), root_pid)
    }
    #[cfg(not(windows))]
    {
        vec![root_pid]
    }
}
