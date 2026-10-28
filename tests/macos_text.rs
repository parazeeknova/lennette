//! Run on the main thread: AppKit must not be initialized on a test worker.
//! Detect GPUI's NoopTextSystem, which can otherwise launch an entirely blank UI.
#[cfg(target_os = "macos")]
fn main() {
    use gpui::{Application, FontWeight, font, px};
    use std::borrow::Cow;
    let application = Application::new();
    let text = application.text_system();
    text.add_fonts(vec![
        Cow::Borrowed(include_bytes!(
            "../src/assets/fonts/OTF/Satoshi-Regular.otf"
        )),
        Cow::Borrowed(include_bytes!("../src/assets/fonts/OTF/Satoshi-Medium.otf")),
        Cow::Borrowed(include_bytes!("../src/assets/fonts/OTF/Satoshi-Bold.otf")),
        Cow::Borrowed(lennette::features::icons::FONT_BYTES),
    ])
    .expect("load the actual bundled fonts");
    assert!(
        text.all_font_names().iter().any(|name| name == "Satoshi"),
        "macOS must use the real font-kit text backend, not NoopTextSystem"
    );
    for weight in [FontWeight::NORMAL, FontWeight::MEDIUM, FontWeight::BOLD] {
        let id = text.resolve_font(&gpui::Font {
            weight,
            ..font("Satoshi")
        });
        let bounds = text
            .typographic_bounds(id, px(16.0), 'H')
            .expect("shape a visible glyph");
        assert!(bounds.size.width > px(0.0) && bounds.size.height > px(0.0));
        assert!(text.units_per_em(id) > 0);
    }
    println!(
        "macOS text smoke passed: real bundled fonts and nonempty glyphs at regular, medium and bold weights"
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {}
