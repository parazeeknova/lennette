//! Visual tokens.
//!
//! Values are taken from the reference build's stylesheet so the port keeps the
//! same dark surface, muted text and hairline borders. Kept in one place so the
//! whole app draws from a single palette.

use gpui::Rgba;

/// Vertical spacing steps used across the app.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 24.0;
}

/// Border radii, matching the reference's `rounded-lg` / `rounded-xl` scale.
pub mod radius {
    pub const SM: f32 = 8.0;
    pub const MD: f32 = 12.0;
    pub const LG: f32 = 16.0;
    pub const XL: f32 = 22.0;
    pub const FULL: f32 = 999.0;
}

/// Build an opaque colour from a 24-bit RGB literal at compile time.
const fn rgb_hex(value: u32) -> Rgba {
    Rgba {
        r: ((value >> 16) & 0xff) as f32 / 255.0,
        g: ((value >> 8) & 0xff) as f32 / 255.0,
        b: (value & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

/// Build a colour with alpha from a 24-bit literal at compile time.
const fn rgba(r: u32, g: u32, b: u32, a: f32) -> Rgba {
    Rgba {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

/// Default window tint opacity.
pub const DEFAULT_OPACITY: f32 = 0.5;

/// Semantic surface and text colours.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    pub background: Rgba,
    pub card: Rgba,
    pub foreground: Rgba,
    pub muted: Rgba,
    pub muted_foreground: Rgba,
    pub border: Rgba,
    /// Outer window frame, independent of decorative control outlines/opacity.
    pub frame_border: Rgba,
    /// Dark outer edge; the light inner frame_border sits one physical pixel in.
    pub frame_outer_border: Rgba,
    pub input: Rgba,
    pub accent: Rgba,
    pub primary: Rgba,
    pub destructive: Rgba,
    pub success: Rgba,
    pub warning: Rgba,
}

impl Theme {
    pub fn promotion(self) -> Self {
        let white = gpui::rgb(0xffffff);
        Self {
            background: gpui::rgb(0x3979be),
            foreground: white,
            muted_foreground: white,
            primary: white,
            frame_border: Self::with_alpha(
                white,
                if self.frame_border.a > 0.0 { 0.85 } else { 0.0 },
            ),
            frame_outer_border: Self::with_alpha(
                white,
                if self.frame_outer_border.a > 0.0 {
                    0.35
                } else {
                    0.0
                },
            ),
            ..self
        }
    }

    pub fn from_settings(settings: &crate::core::settings::Settings) -> Self {
        let mut theme = Self::from_choice(settings.theme);
        if !settings.show_borders {
            theme.border.a = 0.0;
            theme.frame_border.a = 0.0;
            theme.frame_outer_border.a = 0.0;
        }
        theme
    }

    /// Glass leaves the desktop fully visible behind text and controls.
    pub const GLASS: Theme = Theme {
        background: rgba(0x11, 0x13, 0x18, 0.0),
        card: rgba(0x16, 0x1b, 0x24, 0.0),
        foreground: rgb_hex(0xf5f5f5),
        muted: rgba(0xff, 0xff, 0xff, 0.0),
        muted_foreground: rgb_hex(0xd0d4dc),
        border: rgba(0xff, 0xff, 0xff, 0.055),
        frame_border: rgba(0xff, 0xff, 0xff, 0.18),
        frame_outer_border: rgba(0x45, 0x48, 0x50, 0.38),
        input: rgba(0x0c, 0x11, 0x1b, 0.0),
        accent: rgba(0xff, 0xff, 0xff, 0.0),
        primary: rgb_hex(0xf7f7f7),
        destructive: rgb_hex(0xe5837f),
        success: rgb_hex(0x8fd9b0),
        warning: rgb_hex(0xe8c48c),
    };

    /// Dark theme, matching the reference overlay's default appearance.
    pub const DARK: Theme = Theme {
        background: rgba(0x08, 0x08, 0x08, 0.88),
        card: rgba(0x18, 0x18, 0x18, 0.52),
        foreground: rgb_hex(0xf4f4f4),
        muted: rgba(0xff, 0xff, 0xff, 0.08),
        muted_foreground: rgb_hex(0xa6a6a6),
        border: rgba(0xff, 0xff, 0xff, 0.085),
        frame_border: rgba(0xff, 0xff, 0xff, 0.22),
        frame_outer_border: rgba(0x45, 0x48, 0x50, 0.55),
        input: rgba(0x12, 0x12, 0x12, 0.42),
        accent: rgba(0xff, 0xff, 0xff, 0.12),
        primary: rgb_hex(0xf5f5f5),
        destructive: rgb_hex(0xd35a5a),
        success: rgb_hex(0x6fcf97),
        warning: rgb_hex(0xd8a657),
    };

    /// Light theme: modern clean light palette with crisp contrast.
    pub const LIGHT: Theme = Theme {
        background: rgba(0xf8, 0xf9, 0xfa, 0.95),
        card: rgba(0xff, 0xff, 0xff, 0.95),
        foreground: rgb_hex(0x111827),
        muted: rgba(0x00, 0x00, 0x00, 0.06),
        muted_foreground: rgb_hex(0x6b7280),
        border: rgba(0x00, 0x00, 0x00, 0.075),
        frame_border: rgba(0xff, 0xff, 0xff, 0.90),
        frame_outer_border: rgba(0x55, 0x5b, 0x66, 0.26),
        input: rgba(0xff, 0xff, 0xff, 0.90),
        accent: rgba(0x00, 0x00, 0x00, 0.08),
        primary: rgb_hex(0x111827),
        destructive: rgb_hex(0xdc2626),
        success: rgb_hex(0x16a34a),
        warning: rgb_hex(0xd97706),
    };

    /// Primary fills need opaque inverse text; page alpha must never affect glyphs.
    pub fn primary_foreground(self) -> Rgba {
        if self.is_glass() {
            self.foreground
        } else {
            Self::with_alpha(self.background, 1.0)
        }
    }

    pub fn is_glass(self) -> bool {
        self.background.a < 0.2
    }

    /// Keep semantic backgrounds clear in glass while preserving foreground colours.
    pub fn fill(self, colour: Rgba) -> Rgba {
        if self.is_glass() {
            Self::with_alpha(colour, 0.0)
        } else {
            colour
        }
    }

    pub fn primary_fill(self) -> Rgba {
        self.fill(self.primary)
    }

    /// Interactive outlines stay quiet in every theme, including hover/focus.
    pub fn control_border(self, active: bool) -> Rgba {
        Self::with_alpha(
            self.border,
            if active {
                self.border.a * 1.35
            } else {
                self.border.a
            },
        )
    }

    pub fn focus_border(self) -> Rgba {
        Self::with_alpha(self.foreground, 0.16)
    }

    /// Popups follow the same transparency policy as the rest of the app.
    pub fn popover(self) -> Rgba {
        crate::ui::widgets::blur::tint(self, crate::ui::widgets::blur::Surface::Menu)
    }

    pub fn menu_hover(self) -> Rgba {
        if self.is_glass() {
            self.accent
        } else {
            self.muted
        }
    }

    pub const DEFAULT: Theme = Self::DARK;

    pub const fn new() -> Self {
        Self::DARK
    }

    pub fn from_choice(choice: crate::core::models::settings::ThemeChoice) -> Self {
        match choice {
            crate::core::models::settings::ThemeChoice::Dark => Self::DARK,
            crate::core::models::settings::ThemeChoice::Light => Self::LIGHT,
            crate::core::models::settings::ThemeChoice::Glass => Self::GLASS,
            crate::core::models::settings::ThemeChoice::System => Self::DARK,
        }
    }
}

impl Theme {
    /// Surface colours at a given window opacity.
    ///
    /// Every panel, pill and button must use these rather than the raw tokens,
    /// otherwise opaque children float over a see-through window.
    pub fn surfaces(self, opacity: f32) -> Surfaces {
        Surfaces {
            opacity: if self.is_glass() { 0.0 } else { opacity },
            theme: self,
        }
    }

    /// The same colour at a given opacity, for see-through surfaces.
    pub fn with_alpha(colour: Rgba, alpha: f32) -> Rgba {
        Rgba {
            a: alpha.clamp(0.0, 1.0),
            ..colour
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::DARK
    }
}

/// A theme viewed at a specific window opacity.
#[derive(Debug, Clone, Copy)]
pub struct Surfaces {
    pub opacity: f32,
    pub theme: Theme,
}

impl Surfaces {
    /// Scale a colour's alpha by the window opacity.
    ///
    /// Multiplying preserves the relative strength of each translucent token.
    fn fade(self, colour: Rgba) -> Rgba {
        if colour.a == 0.0 {
            return colour;
        }
        // Below the old 35% floor a surface kept a visible tint, so 0% never
        // reached a truly clear window. Scale straight from the chosen value.
        Rgba {
            a: (colour.a * self.opacity).clamp(0.0, 1.0),
            ..colour
        }
    }

    /// Non-glass controls retain a tint floor; glass surfaces stay clear.
    fn fade_readable(self, colour: Rgba) -> Rgba {
        if self.theme.is_glass() {
            return Theme::with_alpha(colour, 0.0);
        }
        if colour.a == 0.0 {
            return colour;
        }
        let weight = 0.35 + self.opacity * 0.65;
        Rgba {
            a: (colour.a * weight).clamp(0.0, 1.0),
            ..colour
        }
    }

    /// The page follows the opacity setting, except glass is always clear.
    pub fn background(self) -> Rgba {
        Theme::with_alpha(
            self.theme.background,
            if self.theme.is_glass() {
                0.0
            } else {
                self.opacity
            },
        )
    }

    /// Glass panels are clear; other themes retain a legibility floor.
    pub fn card(self) -> Rgba {
        self.fade_readable(self.theme.card)
    }

    pub fn muted(self) -> Rgba {
        self.fade_readable(self.theme.muted)
    }

    pub fn accent(self) -> Rgba {
        self.fade_readable(self.theme.accent)
    }

    pub fn input(self) -> Rgba {
        self.fade_readable(self.theme.input)
    }

    /// Primary actions stay clear in glass.
    pub fn primary(self) -> Rgba {
        if self.theme.is_glass() {
            return self.theme.primary_fill();
        }
        let boosted = (self.opacity + 0.35).clamp(0.0, 1.0);
        Rgba {
            a: boosted,
            ..self.theme.primary
        }
    }

    pub fn muted_foreground(self) -> Rgba {
        if self.theme.is_glass() {
            self.theme.muted_foreground
        } else {
            self.fade(self.theme.muted_foreground)
        }
    }

    pub fn border(self) -> Rgba {
        if self.theme.is_glass() {
            self.theme.border
        } else {
            self.fade(self.theme.border)
        }
    }
}

/// Stable code metrics and programming ligatures in fenced and inline code.
pub fn code_font_family() -> gpui::SharedString {
    if cfg!(target_os = "macos") {
        "Menlo".into()
    } else {
        "FiraCode Nerd Font".into()
    }
}

/// Bundled proportional UI face, consistent even when the desktop uses a code font.
pub fn ui_font_family() -> gpui::SharedString {
    "Satoshi".into()
}

/// Base em size. Every text size in the UI is expressed in rems against this,
/// so one value scales the whole interface.
pub const BASE_REM: f32 = 16.0;

/// The em size for a stored font-scale percentage.
///
/// GPUI resolves `text_size` written in rems against the window's rem size, so
/// setting this on the window scales labels as well as their line heights.
pub fn rem_size_for(font_scale: u32) -> gpui::Pixels {
    gpui::px(BASE_REM * font_scale.clamp(85, 140) as f32 / 100.0)
}

/// A pixel size expressed as a scale-independent length.
pub fn scaled(font_size: f32) -> gpui::Rems {
    gpui::rems(font_size / BASE_REM)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_scale_moves_rem_size_and_stays_readable() {
        assert_eq!(rem_size_for(100), gpui::px(16.0));
        assert_eq!(rem_size_for(125), gpui::px(20.0));
        // Out-of-range values from hand-edited settings are clamped, never tiny.
        assert_eq!(rem_size_for(10), gpui::px(16.0 * 0.85));
        assert_eq!(rem_size_for(500), gpui::px(16.0 * 1.40));
        // A 12px label stays 12px at 100% and grows with the setting.
        assert_eq!(scaled(12.0), gpui::rems(0.75));
    }

    #[test]
    fn glass_clears_all_fills_at_every_opacity_and_keeps_text_visible() {
        let theme = Theme::GLASS;
        for opacity in [0.0, 0.5, 1.0] {
            let surfaces = theme.surfaces(opacity);
            for colour in [
                surfaces.card(),
                surfaces.muted(),
                surfaces.input(),
                surfaces.accent(),
                surfaces.primary(),
                theme.menu_hover(),
                theme.fill(theme.destructive),
            ] {
                assert_eq!(colour.a, 0.0);
            }
            assert!(surfaces.border().a <= 0.1);
            assert!(theme.control_border(true).a <= 0.12);
            assert!(theme.focus_border().a > 0.0);
            assert_eq!(surfaces.background().a, 0.0);
            assert_eq!(theme.popover().a, 0.0);
            assert_eq!(theme.primary_foreground(), theme.foreground);
            assert_eq!(theme.foreground.a, 1.0);
        }
        assert!(Theme::DARK.primary_fill().a > 0.0);
        assert!(Theme::DARK.popover().a > 0.0);
        assert!(Theme::LIGHT.primary_fill().a > 0.0);
    }

    #[test]
    fn border_preference_keeps_focus_and_error_colours() {
        let settings = crate::core::settings::Settings {
            show_borders: false,
            ..Default::default()
        };
        let theme = Theme::from_settings(&settings);
        assert_eq!(theme.surfaces(1.0).border().a, 0.0);
        assert!(theme.primary.a > 0.0);
        assert!(theme.destructive.a > 0.0);
    }

    #[test]
    fn window_frame_respects_border_preference_in_every_theme() {
        use crate::core::models::settings::ThemeChoice;
        for choice in [ThemeChoice::Glass, ThemeChoice::Dark, ThemeChoice::Light] {
            let mut settings = crate::core::settings::Settings {
                theme: choice,
                show_borders: true,
                transparency: 0,
                ..Default::default()
            };
            assert!(Theme::from_settings(&settings).frame_border.a > 0.0);
            assert!(Theme::from_settings(&settings).frame_outer_border.a > 0.0);
            settings.show_borders = false;
            assert_eq!(Theme::from_settings(&settings).frame_border.a, 0.0);
            assert_eq!(Theme::from_settings(&settings).frame_outer_border.a, 0.0);
        }
        assert!(Theme::GLASS.control_border(true).a < 0.1);
    }

    #[test]
    fn tinted_palettes_fade_surfaces_and_keep_interactive_edges_quiet() {
        for theme in [Theme::DARK, Theme::LIGHT] {
            let low = theme.surfaces(0.0);
            let high = theme.surfaces(1.0);
            assert_eq!(low.background().a, 0.0);
            assert!(low.card().a < high.card().a);
            assert!(low.input().a < high.input().a);
            assert!(low.muted().a < high.muted().a);
            assert!(low.accent().a < high.accent().a);
            assert!(theme.control_border(true).a <= 0.12);
            assert!(theme.focus_border().a < 0.2);
            assert_eq!(theme.foreground.a, 1.0);
        }
    }

    #[test]
    fn full_opacity_makes_the_window_opaque() {
        for theme in [Theme::DARK, Theme::LIGHT] {
            let full = theme.surfaces(1.0);
            assert!((full.card().a - theme.card.a).abs() < 1e-6);
            assert_eq!(full.background().a, 1.0);
            assert_eq!(theme.surfaces(0.5).background().a, 0.5);
            assert_eq!(theme.surfaces(0.0).background().a, 0.0);
        }
    }

    #[test]
    fn hex_maps_channels_correctly() {
        let white = rgb_hex(0xffffff);
        assert_eq!(white.r, 1.0);
        assert_eq!(white.a, 1.0);
        assert_eq!(rgb_hex(0x000000).r, 0.0);
        assert!((rgb_hex(0x808080).r - 0.5019).abs() < 1e-3);
    }
}
