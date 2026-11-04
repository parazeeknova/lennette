//! Carbon hotkeys share GPUI's AppKit event loop. No keyboard-monitor permissions.
use std::ffi::c_void;
use std::ptr::NonNull;

use super::{Action, Binding};
use crate::platform::macos::*;

pub(super) struct Hotkeys {
    native: NonNull<c_void>,
    // Keep registration and destruction on the AppKit main thread.
    _main_thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

impl Hotkeys {
    pub fn start(bindings: &[Binding]) -> Result<(Self, Vec<String>), String> {
        // SAFETY: ShortcutListener is created on GPUI's main thread; bridge checks it.
        let native = NonNull::new(unsafe { lenny_macos_hotkeys_new() })
            .ok_or("macOS hotkeys must be initialized on the application main thread")?;
        let listener = Self {
            native,
            _main_thread: std::marker::PhantomData,
        };
        let mut failed = Vec::new();
        for binding in bindings {
            let Some(key) = keycode(&binding.key) else {
                failed.push(format!("{} (key unavailable on macOS)", binding.display));
                continue;
            };
            let modifiers = if binding.ctrl { 1 << 12 } else { 0 }
                | if binding.command { 1 << 8 } else { 0 }
                | if binding.shift { 1 << 9 } else { 0 }
                | if binding.alt { 1 << 11 } else { 0 };
            let Some(action) = Action::ALL.iter().position(|a| *a == binding.action) else {
                continue;
            };
            // SAFETY: native context is live; action is an index into Action::ALL.
            let status =
                unsafe { lenny_macos_hotkey_add(native.as_ptr(), key, modifiers, action as u32) };
            if status != 0 {
                failed.push(format!(
                    "{} (macOS hotkey error {status}; check for a shortcut conflict)",
                    binding.display
                ));
            }
        }
        Ok((listener, failed))
    }

    pub fn try_action(&self) -> Option<Action> {
        // SAFETY: polling and Carbon dispatch both occur on GPUI's main thread.
        let action = unsafe { lenny_macos_hotkey_poll(self.native.as_ptr()) };
        usize::try_from(action)
            .ok()
            .and_then(|i| Action::ALL.get(i))
            .copied()
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        // SAFETY: this !Send owner unregisters on its creating UI thread.
        unsafe { lenny_macos_hotkeys_free(self.native.as_ptr()) };
    }
}

// Carbon virtual key codes are physical ANSI positions, independent of Shift.
// Defaults preserve the existing Control+Shift shortcuts on all platforms.
fn keycode(key: &str) -> Option<u32> {
    Some(match key {
        "a" => 0,
        "s" => 1,
        "d" => 2,
        "f" => 3,
        "h" => 4,
        "g" => 5,
        "z" => 6,
        "x" => 7,
        "c" => 8,
        "v" => 9,
        "b" => 11,
        "q" => 12,
        "w" => 13,
        "e" => 14,
        "r" => 15,
        "y" => 16,
        "t" => 17,
        "1" => 18,
        "2" => 19,
        "3" => 20,
        "4" => 21,
        "6" => 22,
        "5" => 23,
        "9" => 25,
        "7" => 26,
        "8" => 28,
        "0" => 29,
        "o" => 31,
        "u" => 32,
        "i" => 34,
        "p" => 35,
        "enter" => 36,
        "l" => 37,
        "j" => 38,
        "k" => 40,
        "backslash" => 42,
        "n" => 45,
        "m" => 46,
        "space" => 49,
        "f1" => 122,
        "f2" => 120,
        "f3" => 99,
        "f4" => 118,
        "f5" => 96,
        "f6" => 97,
        "f7" => 98,
        "f8" => 100,
        "f9" => 101,
        "f10" => 109,
        "f11" => 103,
        "f12" => 111,
        "f13" => 105,
        "f14" => 107,
        "f15" => 113,
        "f16" => 106,
        "f17" => 64,
        "f18" => 79,
        "f19" => 80,
        "f20" => 90,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_default_has_a_distinct_native_key() {
        let keys: std::collections::HashSet<_> = Binding::defaults()
            .iter()
            .map(|binding| keycode(&binding.key).expect("default must have a native key"))
            .collect();
        assert_eq!(keys.len(), Action::ALL.len());
        assert_eq!(keycode("backslash"), Some(42));
        assert_eq!(keycode("f24"), None);
    }
}
