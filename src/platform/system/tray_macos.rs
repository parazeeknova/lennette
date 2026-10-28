//! AppKit menu-bar status item. Native functions guard access to the main thread.
use super::{TrayAction, TrayBackend};
use crate::platform::macos::*;
use std::ffi::{CString, c_void};

pub(super) struct Backend(usize);

pub(super) fn start() -> Result<Backend, String> {
    // SAFETY: initialization checks that GPUI's main thread is calling.
    let icon = include_bytes!("../../assets/lennette_app_icon.png");
    let native = unsafe { lenny_macos_tray_new(icon.as_ptr(), icon.len()) };
    if native.is_null() {
        Err("The macOS menu bar must be initialized on the application main thread".into())
    } else {
        Ok(Backend(native as usize))
    }
}

impl TrayBackend for Backend {
    fn poll(&self) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        loop {
            // SAFETY: owner retains the native object; bridge rejects non-main threads.
            let action = unsafe { lenny_macos_tray_poll(self.0 as *mut c_void) };
            actions.push(match action {
                0 => TrayAction::ShowOverlay,
                1 => TrayAction::OpenDashboard,
                2 => TrayAction::ToggleListening,
                3 => TrayAction::PrivacyMode,
                4 => TrayAction::Quit,
                _ => break,
            });
        }
        actions
    }

    fn set_tooltip(&self, text: &str) {
        let Ok(text) = CString::new(text) else { return };
        // SAFETY: string remains live for the synchronous copying call.
        unsafe { lenny_macos_tray_tooltip(self.0 as *mut c_void, text.as_ptr()) };
    }
}

impl Drop for Backend {
    fn drop(&mut self) {
        // SAFETY: release the reference from tray_new exactly once. AppKit cleanup
        // is dispatched to its main queue if a Send caller drops elsewhere.
        unsafe { lenny_macos_tray_free(self.0 as *mut c_void) };
    }
}
