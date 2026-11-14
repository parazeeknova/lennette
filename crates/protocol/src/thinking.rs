//! Supported OpenAI-compatible effort values, ordered from lowest latency.
//! Keep explicit model families: unknown/custom models must not receive guessed controls.

pub fn reasoning_efforts(model: &str) -> &'static [&'static str] {
    let model = model
        .rsplit('/')
        .next()
        .unwrap_or(model)
        .to_ascii_lowercase();
    if model == "gemini-4-hydrogen" {
        &["low", "medium", "high", "xhigh"]
    } else if matches!(
        model.as_str(),
        "gemini-4-carbon" | "gemini-4-argon" | "gemini-3.8-flash"
    ) {
        &["low", "medium", "high"]
    } else if model.starts_with("gemini-3.1-flash-lite-image") {
        &["minimal", "high"]
    } else if model.starts_with("gemini-3.5-flash-lite")
        || model.starts_with("gemini-3.6-flash")
        || model.starts_with("gemini-3.1-flash-lite")
        || model.starts_with("gemini-3-flash")
    {
        &["minimal", "low", "medium", "high"]
    } else if model.starts_with("gemini-3-pro") {
        &["low", "high"]
    } else if model.starts_with("gemini-2.5-pro") || model.starts_with("gemini-3.1-pro") {
        &["low", "medium", "high"]
    } else if model.starts_with("gemini-2.5-flash") || model == "deepseek-v4.1-flash-1017" {
        &["none", "low", "medium", "high"]
    } else {
        &[]
    }
}

/// Missing, stale or fallback-incompatible preferences always choose the fastest level.
pub fn reasoning_effort(model: &str, requested: Option<&str>) -> Option<&'static str> {
    let levels = reasoning_efforts(model);
    requested
        .and_then(|requested| levels.iter().copied().find(|level| *level == requested))
        .or_else(|| levels.first().copied())
}

pub fn effort_label(effort: &str) -> &'static str {
    match effort {
        "none" => "Off",
        "minimal" => "Minimal",
        "low" => "Low",
        "medium" => "Medium",
        "high" => "High",
        "xhigh" => "Extra high",
        _ => "Default",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_fallbacks_use_the_fastest_supported_effort() {
        assert_eq!(
            reasoning_effort("gemini-3.5-flash-lite", None),
            Some("minimal")
        );
        assert_eq!(reasoning_effort("gemini-4-hydrogen", None), Some("low"));
        assert_eq!(reasoning_effort("gemini-2.5-flash", None), Some("none"));
        assert_eq!(
            reasoning_effort("deepseek-ai/DeepSeek-V4.1-Flash-1017", None),
            Some("none")
        );
        assert_eq!(
            reasoning_effort("gemini-4-hydrogen", Some("xhigh")),
            Some("xhigh")
        );
        assert_eq!(
            reasoning_effort("gemini-3.8-flash", Some("xhigh")),
            Some("low")
        );
        assert_eq!(
            reasoning_effort("gemini-4-argon", Some("none")),
            Some("low")
        );
        assert_eq!(reasoning_effort("unknown-model", Some("high")), None);
    }
}
