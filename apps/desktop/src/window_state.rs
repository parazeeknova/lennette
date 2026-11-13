//! Device-local window geometry, kept out of account sync.
use gpui::{App, Bounds, Pixels, Window, WindowBounds, point, px, size};
use std::path::{Path, PathBuf};

fn path(database: Option<&Path>, name: &str) -> Option<PathBuf> {
    Some(database?.parent()?.join(format!("window-{name}.json")))
}

pub fn restore(
    database: Option<&Path>,
    name: &str,
    minimum: (f32, f32),
    default: Bounds<Pixels>,
    cx: &App,
) -> Bounds<Pixels> {
    let saved = path(database, name)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<[f32; 4]>(&bytes).ok());
    let Some([x, y, width, height]) = saved else {
        return default;
    };
    let screens: Vec<_> = cx.displays().iter().map(|d| d.bounds()).collect();
    fit([x, y, width, height], minimum, default, &screens)
}

fn fit(
    [x, y, width, height]: [f32; 4],
    minimum: (f32, f32),
    default: Bounds<Pixels>,
    screens: &[Bounds<Pixels>],
) -> Bounds<Pixels> {
    if ![x, y, width, height].iter().all(|v| v.is_finite())
        || width < minimum.0
        || height < minimum.1
    {
        return default;
    }
    let saved = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
    // Reclaim windows after a monitor is disconnected or its scale changes.
    let Some(screen) = screens.iter().find(|screen| screen.contains(&saved.origin)) else {
        return default;
    };
    let width = saved.size.width.min(screen.size.width);
    let height = saved.size.height.min(screen.size.height);
    Bounds::new(
        point(
            saved.origin.x.clamp(screen.left(), screen.right() - width),
            saved.origin.y.clamp(screen.top(), screen.bottom() - height),
        ),
        size(width, height),
    )
}

pub fn save(database: Option<&Path>, name: &str, window: &Window) {
    // Hidden startup/style events precede GPUI applying the requested bounds.
    // They must not replace the last user-selected geometry with CW_USEDEFAULT.
    #[cfg(windows)]
    if crate::platform::window::win32_window(window)
        .is_none_or(|handle| unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(handle) } == 0)
    {
        return;
    }
    let WindowBounds::Windowed(bounds) = window.window_bounds() else {
        return;
    };
    let Some(path) = path(database, name) else {
        return;
    };
    let values = [
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    ];
    if values[2] < 100.0 || values[3] < 100.0 {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec(&values)
        && let Err(error) = std::fs::write(path, bytes)
    {
        tracing::warn!(%error, "could not save window geometry");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geometry_survives_negative_monitor_coordinates_and_is_kept_on_screen() {
        let default = Bounds::new(point(px(0.0), px(0.0)), size(px(720.0), px(620.0)));
        let monitor = Bounds::new(point(px(-1920.0), px(0.0)), size(px(1920.0), px(1080.0)));
        let saved = [-1800.0, 80.0, 900.0, 700.0];
        assert_eq!(
            fit(saved, (680.0, 420.0), default, &[monitor]).origin,
            point(px(-1800.0), px(80.0))
        );
        let clamped = fit(
            [-100.0, 900.0, 900.0, 700.0],
            (680.0, 420.0),
            default,
            &[monitor],
        );
        assert_eq!(clamped.right(), px(0.0));
        assert_eq!(clamped.bottom(), px(1080.0));
        assert_eq!(fit(saved, (680.0, 420.0), default, &[]), default);
        assert_eq!(
            fit(
                [0.0, 0.0, f32::NAN, 700.0],
                (680.0, 420.0),
                default,
                &[monitor]
            ),
            default
        );
    }
}
