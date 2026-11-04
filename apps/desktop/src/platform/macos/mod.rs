//! Narrow native bridge. Window/shortcut operations are confined to the UI thread.
use std::ffi::{c_char, c_void};

unsafe extern "C" {
    pub fn lenny_macos_screenshot(
        display: u32,
        bytes: *mut *mut c_void,
        length: *mut usize,
        error: *mut c_char,
        error_capacity: usize,
    ) -> i32;
    pub fn lenny_macos_bytes_free(bytes: *mut c_void);
    pub fn lenny_macos_prepare_window(view: *mut c_void) -> bool;
    pub fn lenny_macos_set_above(view: *mut c_void, above: bool) -> bool;
    pub fn lenny_macos_set_icon_hidden(hidden: bool) -> bool;
    pub fn lenny_macos_hotkeys_new() -> *mut c_void;
    pub fn lenny_macos_hotkey_add(
        context: *mut c_void,
        keycode: u32,
        modifiers: u32,
        action: u32,
    ) -> i32;
    pub fn lenny_macos_hotkey_poll(context: *mut c_void) -> i32;
    pub fn lenny_macos_hotkeys_free(context: *mut c_void);
    pub fn lenny_macos_tray_new(png: *const u8, length: usize) -> *mut c_void;
    pub fn lenny_macos_tray_poll(context: *mut c_void) -> i32;
    pub fn lenny_macos_tray_tooltip(context: *mut c_void, text: *const c_char);
    pub fn lenny_macos_tray_free(context: *mut c_void);
    pub fn lenny_macos_audio_new() -> *mut c_void;
    pub fn lenny_macos_audio_take(context: *mut c_void, output: *mut f32, limit: usize) -> usize;
    pub fn lenny_macos_audio_running(context: *mut c_void) -> bool;
    pub fn lenny_macos_audio_dropped(context: *mut c_void) -> u64;
    pub fn lenny_macos_audio_failure(context: *mut c_void, output: *mut c_char, capacity: usize);
    pub fn lenny_macos_audio_free(context: *mut c_void);
}

/// Sonoma+ screenshot API with explicit self-exclusion. Ventura uses xcap.
pub fn screenshot(display: u32) -> Option<Result<image::RgbaImage, String>> {
    let mut bytes = std::ptr::null_mut();
    let mut length = 0;
    let mut error = [0i8; 2048];
    // SAFETY: all output pointers refer to live writable variables. The adapter
    // waits for completion and never retains these pointers in its callbacks.
    let status = unsafe {
        lenny_macos_screenshot(
            display,
            &mut bytes,
            &mut length,
            error.as_mut_ptr(),
            error.len(),
        )
    };
    match status {
        -1 => None,
        1 => {
            // SAFETY: success transfers a malloc allocation of exactly length bytes.
            let data = unsafe { std::slice::from_raw_parts(bytes.cast::<u8>(), length) };
            let result = image::load_from_memory(data)
                .map(|image| image.to_rgba8())
                .map_err(|e| e.to_string());
            unsafe { lenny_macos_bytes_free(bytes) };
            Some(result)
        }
        _ => {
            // SAFETY: the bridge always terminates the supplied error buffer.
            let message = unsafe { std::ffi::CStr::from_ptr(error.as_ptr()) }.to_string_lossy();
            Some(Err(format!(
                "{message} Check System Settings → Privacy & Security → Screen & System Audio Recording."
            )))
        }
    }
}
