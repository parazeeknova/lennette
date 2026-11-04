//! Iconography.
//!
//! Uses the Lucide icon font — the same set the reference UI uses — so marks are
//! consistent vector glyphs rather than emoji. The font is registered once at
//! startup; icons then render as ordinary text tinted by `text_color`.

use gpui::{
    FontWeight, ImageSource, IntoElement, ParentElement, RenderImage, Resource, Styled, div, px,
};

use crate::ui::theme::Theme;

/// Family name reported by the bundled Lucide font.
pub const FAMILY: &str = "lucide";

/// The bundled Lucide TTF, registered with the text system at startup.
pub const FONT_BYTES: &[u8] = lucide_icons::LUCIDE_FONT_BYTES;

/// A single icon at a given point size.
pub fn icon(theme: &Theme, glyph: lucide_icons::Icon, size: f32) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size + 4.0))
        .font_family(FAMILY)
        .font_weight(FontWeight::NORMAL)
        .text_size(px(size))
        .text_color(theme.muted_foreground)
        .child(char::from(glyph).to_string())
}

/// An icon tinted with an explicit colour, for accents and active states.
pub fn icon_colored(color: gpui::Rgba, glyph: lucide_icons::Icon, size: f32) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .justify_center()
        .size(px(size + 4.0))
        .font_family(FAMILY)
        .text_size(px(size))
        .text_color(color)
        .child(char::from(glyph).to_string())
}

/// Decoded brand logo image, cached in memory for instantaneous rendering.
pub fn brand_logo() -> ImageSource {
    static LOGO: std::sync::LazyLock<Option<std::sync::Arc<RenderImage>>> =
        std::sync::LazyLock::new(|| {
            let bytes = include_bytes!("../../assets/lennette.png");
            let img = image::load_from_memory_with_format(bytes, image::ImageFormat::Png).ok()?;
            let mut rgba = img.into_rgba8();
            // GPUI RenderImage expects BGRA layout.
            for pixel in rgba.as_chunks_mut::<4>().0 {
                pixel.swap(0, 2);
            }
            let frame = image::Frame::new(rgba);
            Some(std::sync::Arc::new(RenderImage::new([frame])))
        });

    if let Some(render_image) = LOGO.as_ref() {
        ImageSource::Render(render_image.clone())
    } else {
        ImageSource::Resource(Resource::Embedded("lennette.png".into()))
    }
}

/// Bundled monochrome families and common model-name aliases.
type ModelAsset = (&'static [&'static str], &'static str, &'static [u8]);
const MODEL_FAMILIES: &[ModelAsset] = &[
    (
        &["deepseek"],
        "model-icons/deepseek.svg",
        include_bytes!("../../assets/model-icons/deepseek.svg"),
    ),
    (
        &["claude", "anthropic"],
        "model-icons/claude.svg",
        include_bytes!("../../assets/model-icons/claude.svg"),
    ),
    (
        &["gemini"],
        "model-icons/gemini.svg",
        include_bytes!("../../assets/model-icons/gemini.svg"),
    ),
    (
        &["gpt", "openai", "chatgpt", "o1", "o3", "o4"],
        "model-icons/openai.svg",
        include_bytes!("../../assets/model-icons/openai.svg"),
    ),
    (
        &["grok", "xai"],
        "model-icons/grok.svg",
        include_bytes!("../../assets/model-icons/grok.svg"),
    ),
    (
        &["qwen", "qwq"],
        "model-icons/qwen.svg",
        include_bytes!("../../assets/model-icons/qwen.svg"),
    ),
    (
        &["kimi", "moonshot"],
        "model-icons/kimi.svg",
        include_bytes!("../../assets/model-icons/kimi.svg"),
    ),
    (
        &["glm", "zai", "zhipu", "chatglm"],
        "model-icons/zai.svg",
        include_bytes!("../../assets/model-icons/zai.svg"),
    ),
    (
        &["meta", "llama"],
        "model-icons/meta.svg",
        include_bytes!("../../assets/model-icons/meta.svg"),
    ),
    (
        &["mistral", "codestral", "pixtral", "devstral", "magistral"],
        "model-icons/mistral.svg",
        include_bytes!("../../assets/model-icons/mistral.svg"),
    ),
    (
        &["minimax"],
        "model-icons/minimax.svg",
        include_bytes!("../../assets/model-icons/minimax.svg"),
    ),
    (
        &["cohere", "command"],
        "model-icons/cohere.svg",
        include_bytes!("../../assets/model-icons/cohere.svg"),
    ),
    (
        &["perplexity", "sonar"],
        "model-icons/perplexity.svg",
        include_bytes!("../../assets/model-icons/perplexity.svg"),
    ),
    (
        &["amazon", "nova", "bedrock"],
        "model-icons/bedrock.svg",
        include_bytes!("../../assets/model-icons/bedrock.svg"),
    ),
    (
        &["nvidia", "nemotron"],
        "model-icons/nvidia.svg",
        include_bytes!("../../assets/model-icons/nvidia.svg"),
    ),
    (
        &["microsoft", "phi"],
        "model-icons/microsoft.svg",
        include_bytes!("../../assets/model-icons/microsoft.svg"),
    ),
    (
        &["gemma"],
        "model-icons/gemma.svg",
        include_bytes!("../../assets/model-icons/gemma.svg"),
    ),
    (
        &["doubao"],
        "model-icons/doubao.svg",
        include_bytes!("../../assets/model-icons/doubao.svg"),
    ),
    (
        &["hunyuan", "tencent"],
        "model-icons/hunyuan.svg",
        include_bytes!("../../assets/model-icons/hunyuan.svg"),
    ),
    (
        &["ai21", "jamba"],
        "model-icons/ai21.svg",
        include_bytes!("../../assets/model-icons/ai21.svg"),
    ),
    (
        &["baidu", "ernie"],
        "model-icons/baidu.svg",
        include_bytes!("../../assets/model-icons/baidu.svg"),
    ),
    (
        &["ollama"],
        "model-icons/ollama.svg",
        include_bytes!("../../assets/model-icons/ollama.svg"),
    ),
    (
        &["huggingface"],
        "model-icons/huggingface.svg",
        include_bytes!("../../assets/model-icons/huggingface.svg"),
    ),
    (
        &["deepmind"],
        "model-icons/deepmind.svg",
        include_bytes!("../../assets/model-icons/deepmind.svg"),
    ),
    (
        &["bytedance", "seed", "seedream"],
        "model-icons/bytedance.svg",
        include_bytes!("../../assets/model-icons/bytedance.svg"),
    ),
    (
        &["yi", "01ai"],
        "model-icons/yi.svg",
        include_bytes!("../../assets/model-icons/yi.svg"),
    ),
];

/// Resolve families across direct providers and gateway-prefixed model IDs.
pub fn model_asset_path(provider: &str, model: &str) -> Option<&'static str> {
    let id = format!("{model} {provider}").to_lowercase();
    let parts: Vec<_> = id
        .split(|ch: char| !ch.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect();
    MODEL_FAMILIES
        .iter()
        .find(|(aliases, _, _)| {
            aliases.iter().any(|alias| {
                parts.iter().any(|part| {
                    part == alias
                        || part.strip_prefix(alias).is_some_and(|suffix| {
                            suffix.starts_with(|ch: char| ch.is_ascii_digit())
                        })
                })
            })
        })
        .map(|(_, path, _)| *path)
}

pub fn model_paths() -> impl Iterator<Item = &'static str> {
    MODEL_FAMILIES.iter().map(|(_, path, _)| *path)
}

pub fn model_icon(theme: &Theme, provider: &str, model: &str, size: f32) -> gpui::AnyElement {
    if let Some(path) = model_asset_path(provider, model) {
        div()
            .size(px(size + 4.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .child(
                gpui::svg()
                    .path(path)
                    .size(px(size))
                    .text_color(theme.foreground),
            )
            .into_any_element()
    } else {
        icon(theme, lucide_icons::Icon::Bot, size).into_any_element()
    }
}

/// Single source for the embedded SVG assets used by every native window.
pub fn model_asset(path: &str) -> Option<&'static [u8]> {
    if path == "model-icons/LICENSE" {
        return Some(include_bytes!("../../assets/model-icons/LICENSE"));
    }
    MODEL_FAMILIES
        .iter()
        .find(|(_, asset_path, _)| *asset_path == path)
        .map(|(_, _, bytes)| *bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_families_use_bundled_monochrome_assets_and_unknowns_fall_back() {
        for (aliases, path, bytes) in MODEL_FAMILIES {
            assert_eq!(
                model_asset_path("merge-gateway", &format!("vendor/{}-3.1", aliases[0])),
                Some(*path)
            );
            assert_eq!(model_asset(path), Some(*bytes));
            let svg = std::str::from_utf8(bytes).unwrap();
            assert!(svg.contains("<svg") && svg.contains("viewBox"));
            assert!(!svg.contains("<script") && !svg.contains("<image"));
        }
        assert_eq!(
            model_asset_path("merge-gateway", "Google/Gemini-Flash"),
            Some("model-icons/gemini.svg")
        );
        assert_eq!(
            model_asset_path("openrouter", "z-ai/glm-5"),
            Some("model-icons/zai.svg")
        );
        assert_eq!(model_asset_path("custom", "philosophy-model"), None);
        assert_eq!(model_asset_path("custom", "unrecognized-model"), None);
    }

    #[test]
    fn bundled_font_is_present() {
        assert!(FONT_BYTES.len() > 1000, "lucide font should be embedded");
    }

    #[test]
    fn icon_names_are_stable() {
        assert_eq!(format!("{}", lucide_icons::Icon::Mic), "mic");
        assert_eq!(format!("{}", lucide_icons::Icon::Camera), "camera");
        assert_eq!(format!("{}", lucide_icons::Icon::Settings), "settings");
    }

    #[test]
    fn brand_logo_loads_successfully() {
        let source = brand_logo();
        assert!(matches!(source, ImageSource::Render(_)));
    }
}
