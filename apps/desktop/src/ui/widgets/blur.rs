//! Shared frosted-surface recipe. The OS compositor supplies backdrop pixels;
//! we never capture the screen or blur text/foreground content.
use crate::ui::theme::Theme;
use gpui::{Rgba, Window};

#[derive(Clone, Copy)]
pub enum Surface {
    Input,
    Pill,
    Menu,
}

pub fn tint(theme: Theme, surface: Surface) -> Rgba {
    let light = theme.foreground.r < 0.5;
    let alpha = match (light, surface) {
        (true, Surface::Input) => 0.78,
        (true, Surface::Pill) => 0.82,
        (true, Surface::Menu) => 0.94,
        (false, Surface::Input) => 0.64,
        (false, Surface::Pill) => 0.72,
        (false, Surface::Menu) => 0.9,
    };
    Theme::with_alpha(
        if light {
            Theme::LIGHT.background
        } else {
            theme.background
        },
        alpha,
    )
}

pub fn enabled(opacity: f32) -> bool {
    opacity > 0.0 && opacity < 1.0
}

/// Native blur is window-wide behind the translucent GPUI composition. Insets
/// use their own tints, so text and controls stay sharp. Unsupported compositors
/// retain the tinted fallback rather than pretending a flat fill is blurred.
pub fn apply(window: &Window, theme: Theme, opacity: f32) {
    #[cfg(windows)]
    if let Some(handle) = crate::platform::window::win32_window(window) {
        use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
        let active = enabled(opacity);
        let dark: i32 = i32::from(theme.foreground.r >= 0.5);
        let backdrop: i32 = if active { 3 } else { 1 }; // DWMSBT_TRANSIENTWINDOW / NONE
        unsafe {
            let margins = windows_sys::Win32::UI::Controls::MARGINS {
                cxLeftWidth: if active { -1 } else { 0 },
                cxRightWidth: if active { -1 } else { 0 },
                cyTopHeight: if active { -1 } else { 0 },
                cyBottomHeight: if active { -1 } else { 0 },
            };
            let _ =
                windows_sys::Win32::Graphics::Dwm::DwmExtendFrameIntoClientArea(handle, &margins);
            let _ = DwmSetWindowAttribute(handle, 20, (&dark as *const i32).cast(), 4);
            let result = DwmSetWindowAttribute(handle, 38, (&backdrop as *const i32).cast(), 4);
            if result < 0 {
                // Windows 10 lacks system-backdrop attributes. GPUI's native
                // Windows acrylic path is the compatibility fallback.
                window.set_background_appearance(if active {
                    gpui::WindowBackgroundAppearance::Blurred
                } else {
                    gpui::WindowBackgroundAppearance::Transparent
                });
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = theme;
        window.set_background_appearance(if enabled(opacity) {
            gpui::WindowBackgroundAppearance::Blurred
        } else {
            gpui::WindowBackgroundAppearance::Transparent
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn opacity_endpoints_disable_native_backdrop_and_palettes_keep_legibility() {
        assert!(!enabled(0.0));
        assert!(!enabled(1.0));
        assert!(enabled(0.5));
        for theme in [Theme::LIGHT, Theme::DARK, Theme::GLASS] {
            let input = tint(theme, Surface::Input);
            let menu = tint(theme, Surface::Menu);
            assert!(input.a >= 0.6);
            assert!(menu.a > input.a);
            assert_eq!(input.r < 0.5, theme.foreground.r >= 0.5);
        }
    }
}
