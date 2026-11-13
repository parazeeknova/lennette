//! Device-local window geometry, kept out of account sync.
use gpui::{App, Bounds, Pixels, Window, WindowBounds, point, px, size};
use std::path::{Path, PathBuf};

fn path(database: Option<&Path>, name: &str) -> Option<PathBuf> {
    Some(database?.parent()?.join(format!("window-{name}.json")))
}

#[derive(serde::Serialize, serde::Deserialize)]
struct SavedGeometry {
    bounds: [f32; 4],
    #[serde(default)]
    display_uuid: Option<uuid::Uuid>,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum SavedRecord {
    Current(SavedGeometry),
    Legacy([f32; 4]),
}

pub struct Placement {
    pub bounds: Bounds<Pixels>,
    pub display_id: Option<gpui::DisplayId>,
}

fn select_screen(
    saved: Option<uuid::Uuid>,
    origin: gpui::Point<Pixels>,
    screens: &[(Option<uuid::Uuid>, Bounds<Pixels>)],
) -> Option<usize> {
    saved
        .and_then(|uuid| screens.iter().position(|(id, _)| *id == Some(uuid)))
        .or_else(|| {
            screens
                .iter()
                .position(|(_, bounds)| bounds.contains(&origin))
        })
}

pub fn restore(
    database: Option<&Path>,
    name: &str,
    minimum: (f32, f32),
    default: Bounds<Pixels>,
    cx: &App,
) -> Placement {
    let displays = cx.displays();
    let screens: Vec<_> = displays
        .iter()
        .map(|d| (d.uuid().ok(), d.bounds()))
        .collect();
    let fallback = select_screen(None, default.center(), &screens);
    let saved = path(database, name)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice::<SavedRecord>(&bytes).ok())
        .map(|record| match record {
            SavedRecord::Current(saved) => saved,
            SavedRecord::Legacy(bounds) => SavedGeometry {
                bounds,
                display_uuid: None,
            },
        });
    let Some(saved) = saved else {
        return Placement {
            bounds: default,
            display_id: fallback.map(|i| displays[i].id()),
        };
    };
    let origin = point(px(saved.bounds[0]), px(saved.bounds[1]));
    let selected = select_screen(saved.display_uuid, origin, &screens).or(fallback);
    let bounds = selected
        .map(|i| fit_on_screen(saved.bounds, minimum, default, &screens[i].1))
        .unwrap_or(default);
    Placement {
        bounds,
        display_id: selected.map(|i| displays[i].id()),
    }
}

fn fit_on_screen(
    [x, y, width, height]: [f32; 4],
    minimum: (f32, f32),
    default: Bounds<Pixels>,
    screen: &Bounds<Pixels>,
) -> Bounds<Pixels> {
    if ![x, y, width, height].iter().all(|v| v.is_finite())
        || width < minimum.0
        || height < minimum.1
    {
        return default;
    }
    let saved = Bounds::new(point(px(x), px(y)), size(px(width), px(height)));
    let width = saved.size.width.min(screen.size.width);
    let height = saved.size.height.min(screen.size.height);
    Bounds::new(
        point(
            saved
                .origin
                .x
                .clamp(screen.left(), (screen.right() - width).max(screen.left())),
            saved
                .origin
                .y
                .clamp(screen.top(), (screen.bottom() - height).max(screen.top())),
        ),
        size(width, height),
    )
}

pub fn save(database: Option<&Path>, name: &str, window: &Window, cx: &App) {
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
    let saved = SavedGeometry {
        bounds: values,
        display_uuid: window.display(cx).and_then(|display| display.uuid().ok()),
    };
    if let Ok(bytes) = serde_json::to_vec(&saved)
        && let Err(error) = std::fs::write(path, bytes)
    {
        tracing::warn!(%error, "could not save window geometry");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uuid_disambiguates_overlapping_scaled_monitor_coordinates() {
        let primary = uuid::Uuid::new_v4();
        let secondary = uuid::Uuid::new_v4();
        let screens = [
            (
                Some(primary),
                Bounds::new(point(px(0.0), px(0.0)), size(px(1920.0), px(1080.0))),
            ),
            (
                Some(secondary),
                Bounds::new(point(px(960.0), px(0.0)), size(px(1280.0), px(720.0))),
            ),
        ];
        let origin = point(px(1000.0), px(80.0));
        assert_eq!(select_screen(Some(secondary), origin, &screens), Some(1));
        assert_eq!(select_screen(None, origin, &screens), Some(0));
        assert_eq!(
            select_screen(Some(secondary), origin, &screens[..1]),
            Some(0)
        );
        let legacy: SavedRecord = serde_json::from_str("[1000,80,900,600]").unwrap();
        assert!(matches!(
            legacy,
            SavedRecord::Legacy([1000.0, 80.0, 900.0, 600.0])
        ));
    }

    #[test]
    fn fractional_display_sizes_cannot_invert_the_restore_clamp_range() {
        let screen = Bounds::new(
            point(px(1280.0), px(1280.0)),
            size(px(2560.0 / 1.5), px(2560.0 / 1.5)),
        );
        let fitted = fit_on_screen(
            [1300.0, 1300.0, 1800.0, 1800.0],
            (480.0, 420.0),
            screen,
            &screen,
        );
        assert_eq!(fitted.origin, screen.origin);
        assert_eq!(fitted.size, screen.size);
    }

    #[test]
    fn geometry_survives_negative_monitor_coordinates_and_is_kept_on_screen() {
        let default = Bounds::new(point(px(0.0), px(0.0)), size(px(720.0), px(620.0)));
        let monitor = Bounds::new(point(px(-1920.0), px(0.0)), size(px(1920.0), px(1080.0)));
        let saved = [-1800.0, 80.0, 900.0, 700.0];
        assert_eq!(
            fit_on_screen(saved, (680.0, 420.0), default, &monitor).origin,
            point(px(-1800.0), px(80.0))
        );
        let clamped = fit_on_screen(
            [-100.0, 900.0, 900.0, 700.0],
            (680.0, 420.0),
            default,
            &monitor,
        );
        assert_eq!(clamped.right(), px(0.0));
        assert_eq!(clamped.bottom(), px(1080.0));
        assert_eq!(select_screen(None, default.origin, &[]), None);
        assert_eq!(
            fit_on_screen(
                [0.0, 0.0, f32::NAN, 700.0],
                (680.0, 420.0),
                default,
                &monitor
            ),
            default
        );
    }
}
