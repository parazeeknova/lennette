//! Audio input devices, microphone streams, and system audio capture.

pub mod devices;
pub mod microphone;
#[cfg(not(any(windows, target_os = "macos")))]
pub mod system_audio;
#[cfg(windows)]
#[path = "system_audio_windows.rs"]
pub mod system_audio;

#[cfg(target_os = "macos")]
#[path = "system_audio_macos.rs"]
pub mod system_audio;

pub use devices::*;
pub use microphone::*;
pub use system_audio::*;
