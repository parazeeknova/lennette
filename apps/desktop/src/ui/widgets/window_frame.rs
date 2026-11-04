//! Shared antialiased window and composer edges.
use gpui::IntoElement;

use crate::ui::theme::{Theme, radius};

/// Two supersampled contours follow the same surface curve. The
/// canvas has no hitbox, so it cannot intercept resizing, dragging or clicks.
pub fn window_edge(theme: Theme) -> impl IntoElement {
    surface_edge(theme, radius::XL)
}

pub fn aligned_radius(radius: f32, scale: f32) -> f32 {
    let scale = scale.max(1.0);
    (radius * scale).round() / scale
}

/// The same edge for the session pill and composer group.
pub fn surface_edge(theme: Theme, radius: f32) -> impl IntoElement {
    crate::ui::widgets::material::dual_edge(theme, radius)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn radii_land_on_device_pixels_at_fractional_scales() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            for radius in [8.0, 12.0, 22.0] {
                let physical = aligned_radius(radius, scale) * scale;
                assert!((physical - physical.round()).abs() < 1e-5);
            }
        }
    }
}
