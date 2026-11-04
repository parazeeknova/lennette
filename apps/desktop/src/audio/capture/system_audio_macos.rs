//! ScreenCaptureKit system audio on macOS 13+. Captures the system mix, excluding
//! this process, without a loopback driver. Setup and consent are asynchronous.
use std::ffi::{CStr, c_void};
use std::ptr::NonNull;

use crate::platform::macos::*;

#[derive(Debug, thiserror::Error)]
#[error("macOS system-audio capture failed: {0}")]
pub struct SystemAudioError(String);

pub struct SystemAudio {
    native: NonNull<c_void>,
}

// SAFETY: every native access to the audio state is protected by its NSLock.
// The SCStream callbacks run on a dedicated serial queue; no AppKit objects are used.
unsafe impl Send for SystemAudio {}

impl SystemAudio {
    pub fn start(_source: Option<&str>) -> Result<Self, SystemAudioError> {
        // ScreenCaptureKit captures the system mix rather than a particular output
        // device. The saved Windows/Linux selector is intentionally ignored.
        let native = NonNull::new(unsafe { lenny_macos_audio_new() }).ok_or_else(|| {
            SystemAudioError("Could not create the ScreenCaptureKit stream.".into())
        })?;
        Ok(Self { native })
    }

    pub fn source(&self) -> &str {
        "macOS system audio"
    }

    pub fn take(&mut self, limit: usize) -> Vec<f32> {
        let mut samples = vec![0.0; limit.min(32_000)];
        // SAFETY: native owner is live; the bridge writes at most samples.len() floats.
        let count = unsafe {
            lenny_macos_audio_take(self.native.as_ptr(), samples.as_mut_ptr(), samples.len())
        };
        samples.truncate(count);
        samples
    }

    pub fn is_running(&self) -> bool {
        // SAFETY: the retained native owner lives until Drop.
        unsafe { lenny_macos_audio_running(self.native.as_ptr()) }
    }

    pub fn dropped(&self) -> u64 {
        // SAFETY: the retained native owner lives until Drop.
        unsafe { lenny_macos_audio_dropped(self.native.as_ptr()) }
    }

    pub fn failure(&self) -> Option<String> {
        let mut buffer = [0i8; 2048];
        // SAFETY: the bridge copies a terminated string within the provided buffer.
        unsafe {
            lenny_macos_audio_failure(self.native.as_ptr(), buffer.as_mut_ptr(), buffer.len())
        };
        let message = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy();
        (!message.is_empty()).then(|| {
            format!("{message} Allow Lennette in System Settings → Privacy & Security → Screen & System Audio Recording, then restart listening.")
        })
    }

    pub fn stop(self) {
        drop(self);
    }
}

impl Drop for SystemAudio {
    fn drop(&mut self) {
        // SAFETY: release exactly the reference returned by audio_new; the bridge
        // retains callback state until its asynchronous stop has completed.
        unsafe { lenny_macos_audio_free(self.native.as_ptr()) };
    }
}
