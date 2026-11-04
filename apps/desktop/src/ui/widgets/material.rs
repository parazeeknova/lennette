//! Rounded dual contours rasterized independently of GPUI's quad border shader.
//! Four small cached corner patches keep memory independent of window dimensions.
use crate::ui::theme::Theme;
use gpui::{Bounds, Corners, IntoElement, RenderImage, Rgba, Styled, canvas, point, px, size};
use std::{cell::RefCell, collections::VecDeque, sync::Arc};

#[derive(Clone, PartialEq)]
struct Key {
    radius: u32,
    cap: u32,
    outer: [u32; 4],
    inner: [u32; 4],
}
struct Texture {
    key: Key,
    patches: [Arc<RenderImage>; 4],
}
thread_local! { static CACHE: RefCell<VecDeque<Texture>> = const { RefCell::new(VecDeque::new()) }; }
const SAMPLES: u32 = 8;
const CACHE_LIMIT: usize = 24;
fn bits(c: Rgba) -> [u32; 4] {
    [c.r.to_bits(), c.g.to_bits(), c.b.to_bits(), c.a.to_bits()]
}

/// Exact signed distance to a circular rounded rectangle, in device pixels.
fn distance(x: f32, y: f32, width: f32, height: f32, radius: f32) -> f32 {
    let qx = (x - width * 0.5).abs() - (width * 0.5 - radius);
    let qy = (y - height * 0.5).abs() - (height * 0.5 - radius);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius
}

fn raster(cap: u32, radius: f32, outer: Rgba, inner: Rgba) -> image::RgbaImage {
    let extent = cap * 2 + 1;
    let mut image = image::RgbaImage::new(extent, extent);
    for y in 0..extent {
        for x in 0..extent {
            // No work for the transparent interior of the patch atlas.
            let center = distance(
                x as f32 + 0.5,
                y as f32 + 0.5,
                extent as f32,
                extent as f32,
                radius,
            );
            if !(-3.0..=1.0).contains(&center) {
                continue;
            }
            let (mut outer_samples, mut inner_samples) = (0u32, 0u32);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let d = distance(
                        x as f32 + (sx as f32 + 0.5) / SAMPLES as f32,
                        y as f32 + (sy as f32 + 0.5) / SAMPLES as f32,
                        extent as f32,
                        extent as f32,
                        radius,
                    );
                    if (-1.0..=0.0).contains(&d) {
                        outer_samples += 1;
                    } else if (-2.0..-1.0).contains(&d) {
                        inner_samples += 1;
                    }
                }
            }
            let a = outer.a * outer_samples as f32 / (SAMPLES * SAMPLES) as f32;
            let b = inner.a * inner_samples as f32 / (SAMPLES * SAMPLES) as f32;
            let alpha = a + b;
            if alpha > 0.0 {
                let channel = |o: f32, i: f32| {
                    (((o * a + i * b) / alpha).clamp(0.0, 1.0) * 255.0).round() as u8
                };
                // GPUI's image atlas expects straight-alpha BGRA.
                image.put_pixel(
                    x,
                    y,
                    image::Rgba([
                        channel(outer.b, inner.b),
                        channel(outer.g, inner.g),
                        channel(outer.r, inner.r),
                        (alpha * 255.0).round() as u8,
                    ]),
                );
            }
        }
    }
    image
}

/// Two one-device-pixel contours, identical on all four straight sides.
fn straight_edges(w: f32, h: f32, c: f32, theme: Theme) -> [(f32, f32, f32, f32, Rgba); 8] {
    let outer = theme.frame_outer_border;
    let inner = theme.frame_border;
    [
        (c, 0.0, w - 2.0 * c, 1.0, outer),
        (c, 1.0, w - 2.0 * c, 1.0, inner),
        (c, h - 1.0, w - 2.0 * c, 1.0, outer),
        (c, h - 2.0, w - 2.0 * c, 1.0, inner),
        (0.0, c, 1.0, h - 2.0 * c, outer),
        (1.0, c, 1.0, h - 2.0 * c, inner),
        (w - 1.0, c, 1.0, h - 2.0 * c, outer),
        (w - 2.0, c, 1.0, h - 2.0 * c, inner),
    ]
}

pub fn dual_edge(theme: Theme, radius: f32) -> impl IntoElement {
    canvas(
        |_, _, _| (),
        move |bounds, (), window, cx| {
            if theme.frame_border.a == 0.0 && theme.frame_outer_border.a == 0.0 {
                return;
            }
            let scale = window.scale_factor();
            let width = (f32::from(bounds.size.width) * scale).round().max(0.0) as u32;
            let height = (f32::from(bounds.size.height) * scale).round().max(0.0) as u32;
            if width < 6 || height < 6 {
                return;
            }
            let radius = (radius * scale)
                .min(width.min(height) as f32 * 0.5)
                .min(96.0);
            let cap = (radius.ceil() as u32 + 2)
                .min(width / 2)
                .min(height / 2)
                .max(3);
            let key = Key {
                radius: radius.to_bits(),
                cap,
                outer: bits(theme.frame_outer_border),
                inner: bits(theme.frame_border),
            };
            let patches = CACHE.with_borrow_mut(|cache| {
                if let Some(index) = cache.iter().position(|entry| entry.key == key) {
                    let entry = cache.remove(index).unwrap();
                    let result = entry.patches.clone();
                    cache.push_back(entry);
                    return result;
                }
                let image = raster(cap, radius, theme.frame_outer_border, theme.frame_border);
                let c = cap;
                let regions = [
                    (0, 0, c, c),
                    (c + 1, 0, c, c),
                    (0, c + 1, c, c),
                    (c + 1, c + 1, c, c),
                ];
                let patches = regions.map(|(x, y, w, h)| {
                    Arc::new(RenderImage::new([image::Frame::new(
                        image::imageops::crop_imm(&image, x, y, w, h).to_image(),
                    )]))
                });
                cache.push_back(Texture {
                    key,
                    patches: patches.clone(),
                });
                if cache.len() > CACHE_LIMIT
                    && let Some(old) = cache.pop_front()
                {
                    for image in old.patches {
                        cx.drop_image(image, None);
                    }
                }
                patches
            });
            let c = cap as f32;
            let w = width as f32;
            let h = height as f32;
            let destinations = [
                (0.0, 0.0, c, c),
                (w - c, 0.0, c, c),
                (0.0, h - c, c, c),
                (w - c, h - c, c, c),
            ];
            // Stretching one-texel strips through the linearly filtered sprite
            // atlas samples neighboring texels. Paint straight edges directly;
            // only the supersampled corners need textures.
            for (x, y, w, h, color) in straight_edges(w, h, c, theme) {
                if w > 0.0 && h > 0.0 {
                    window.paint_quad(gpui::fill(
                        Bounds::new(
                            point(
                                px(((f32::from(bounds.origin.x) * scale).floor() + x) / scale),
                                px(((f32::from(bounds.origin.y) * scale).floor() + y) / scale),
                            ),
                            size(px(w / scale), px(h / scale)),
                        ),
                        color,
                    ));
                }
            }
            for (image, (x, y, w, h)) in patches.into_iter().zip(destinations) {
                if w <= 0.0 || h <= 0.0 {
                    continue;
                }
                let origin = point(
                    px((f32::from(bounds.origin.x) * scale).floor() / scale + x / scale),
                    px((f32::from(bounds.origin.y) * scale).floor() / scale + y / scale),
                );
                if let Err(error) = window.paint_image(
                    Bounds::new(origin, size(px(w / scale), px(h / scale))),
                    Corners::all(px(0.0)),
                    image,
                    0,
                    false,
                ) {
                    tracing::debug!(%error,"surface contour unavailable");
                }
            }
        },
    )
    .absolute()
    .inset_0()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn contours_have_symmetric_smooth_corners_and_transparent_centers() {
        for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
            let radius: f32 = 22.0 * scale;
            let cap = radius.ceil() as u32 + 2;
            let image = raster(
                cap,
                radius,
                Theme::DARK.frame_outer_border,
                Theme::DARK.frame_border,
            );
            let n = image.width();
            assert_eq!(image.get_pixel(cap, cap).0[3], 0);
            assert_eq!(image.get_pixel(0, 0).0[3], 0);
            for y in 0..n {
                for x in 0..n {
                    assert_eq!(image.get_pixel(x, y), image.get_pixel(n - 1 - x, y));
                }
            }
            assert!(image.pixels().any(|p| p.0[3] > 0 && p.0[3] < 20));
        }
    }
    #[test]
    fn straight_sides_match_each_other_and_the_corner_join_in_every_theme() {
        for theme in [Theme::LIGHT, Theme::DARK, Theme::GLASS] {
            for scale in [1.0, 1.25, 1.5, 1.75, 2.0] {
                let radius: f32 = 22.0 * scale;
                let cap = radius.ceil() as u32 + 2;
                let image = raster(cap, radius, theme.frame_outer_border, theme.frame_border);
                let edges = straight_edges(400.0, 240.0, cap as f32, theme);
                for pair in edges.as_chunks::<2>().0 {
                    assert_eq!(bits(pair[0].4), bits(theme.frame_outer_border));
                    assert_eq!(bits(pair[1].4), bits(theme.frame_border));
                    for (depth, edge) in pair.iter().enumerate() {
                        assert!(edge.2 == 1.0 || edge.3 == 1.0);
                        let color = edge.4;
                        let expected =
                            [color.b, color.g, color.r, color.a].map(|v| (v * 255.0).round() as u8);
                        // The last corner column/row joins the solid sides
                        // with exactly the same quantized color and opacity.
                        assert_eq!(image.get_pixel(cap, depth as u32).0, expected);
                        assert_eq!(image.get_pixel(depth as u32, cap).0, expected);
                    }
                }
                let n = image.width();
                for y in 0..n {
                    for x in 0..n {
                        assert_eq!(image.get_pixel(x, y), image.get_pixel(y, x));
                    }
                }
            }
        }
    }

    #[test]
    fn border_preference_and_two_contours_are_preserved() {
        let clear = Theme::with_alpha(Theme::DARK.border, 0.0);
        assert!(raster(24, 22.0, clear, clear).pixels().all(|p| p.0[3] == 0));
        let image = raster(
            24,
            22.0,
            Theme::DARK.frame_outer_border,
            Theme::DARK.frame_border,
        );
        assert!(image.get_pixel(24, 0).0[3] > 0);
        assert!(image.get_pixel(24, 1).0[3] > 0);
        assert_eq!(image.get_pixel(24, 3).0[3], 0);
    }
}
