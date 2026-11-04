//! Shared antialiased window and composer edges.
use gpui::{BorderStyle, IntoElement, Styled, canvas, px, quad, transparent_black};

use crate::ui::theme::{Theme, radius};

/// A single antialiased stroke follows the surface curve. The
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
    canvas(
        |_, _, _| (),
        move |bounds, (), window, _| {
            let hairline = px(1.0 / window.scale_factor());
            let outer_radius = px(aligned_radius(radius, window.scale_factor())
                .min(f32::from(bounds.size.width.min(bounds.size.height)) / 2.0));
            // Two adjacent one-device-pixel contours produced a dark/light
            // staircase at fractional display scales. Keep one quiet contour,
            // inset half its width so the antialiasing fringe isn't clipped.
            window.paint_quad(quad(
                bounds.inset(hairline / 2.0),
                outer_radius - hairline / 2.0,
                transparent_black(),
                hairline,
                theme.frame_border,
                BorderStyle::Solid,
            ));
        },
    )
    .absolute()
    .inset_0()
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
