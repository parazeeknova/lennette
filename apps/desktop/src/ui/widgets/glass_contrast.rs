//! Adaptive ink for transparent native windows. Windows capture exclusion lets
//! us measure the desktop underneath our own window without a feedback loop.
//! Samples stay in memory and are never saved, attached to prompts, or uploaded.
use crate::ui::theme::Theme;
use gpui::{Context, Task, Window};

#[derive(Default)]
pub struct GlassContrast {
    dark_ink: bool,
    task: Option<Task<()>>,
}

impl GlassContrast {
    pub fn adapt<T: 'static>(
        &mut self,
        theme: Theme,
        window: &Window,
        cx: &mut Context<T>,
        access: fn(&mut T) -> &mut Self,
    ) -> Theme {
        if !theme.is_glass() {
            return theme;
        }
        // Other capture backends may include our own glyphs or require a portal.
        // Do not repeatedly capture or open permission prompts on those systems.
        if cfg!(windows) && self.task.is_none() {
            let bounds = window.bounds();
            let scale = window.scale_factor();
            let region = [
                f32::from(bounds.origin.x) * scale,
                f32::from(bounds.origin.y) * scale,
                f32::from(bounds.size.width) * scale,
                f32::from(bounds.size.height) * scale,
            ];
            let sample = cx
                .background_executor()
                .spawn(async move { sample(region) });
            self.task = Some(cx.spawn(async move |this, cx| {
                let luminance = sample.await;
                this.update(cx, |view, cx| {
                    let contrast = access(view);
                    if let Some(luminance) = luminance {
                        contrast.dark_ink = choose_dark_ink(contrast.dark_ink, luminance);
                        cx.notify();
                    }
                })
                .ok();
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(800))
                    .await;
                this.update(cx, |view, cx| {
                    access(view).task = None;
                    cx.notify();
                })
                .ok();
            }));
        }
        if self.dark_ink {
            Theme {
                foreground: gpui::rgb(0x101010),
                muted_foreground: gpui::rgb(0x282828),
                primary: gpui::rgb(0x101010),
                destructive: Theme::LIGHT.destructive,
                success: Theme::LIGHT.success,
                warning: Theme::LIGHT.warning,
                border: Theme::with_alpha(gpui::rgb(0x000000), theme.border.a),
                frame_border: Theme::with_alpha(gpui::rgb(0x000000), theme.frame_border.a),
                ..theme
            }
        } else {
            theme
        }
    }
}

fn choose_dark_ink(previous: bool, luminance: f32) -> bool {
    // Hysteresis prevents flashing as a background crosses mid gray.
    if luminance > 0.6 {
        true
    } else if luminance < 0.4 {
        false
    } else {
        previous
    }
}

fn sample([x, y, width, height]: [f32; 4]) -> Option<f32> {
    let monitor =
        xcap::Monitor::from_point((x + width / 2.0) as i32, (y + height / 2.0) as i32).ok()?;
    let mx = monitor.x().ok()? as f32;
    let my = monitor.y().ok()? as f32;
    let left = (x - mx).max(0.0) as u32;
    let top = (y - my).max(0.0) as u32;
    let right = (x + width - mx).max(0.0).min(monitor.width().ok()? as f32) as u32;
    let bottom = (y + height - my)
        .max(0.0)
        .min(monitor.height().ok()? as f32) as u32;
    if right <= left || bottom <= top {
        return None;
    }
    let image = monitor
        .capture_region(left, top, right - left, bottom - top)
        .ok()?;
    let mut sum = 0.0;
    let mut count = 0;
    for y in (0..image.height()).step_by(24) {
        for x in (0..image.width()).step_by(24) {
            let pixel = image.get_pixel(x, y).0;
            sum += (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                / 255.0;
            count += 1;
        }
    }
    (count > 0).then(|| sum / count as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ink_tracks_background_without_flickering_at_mid_gray() {
        assert!(choose_dark_ink(false, 1.0));
        assert!(!choose_dark_ink(true, 0.0));
        assert!(choose_dark_ink(true, 0.5));
        assert!(!choose_dark_ink(false, 0.5));
    }
}
