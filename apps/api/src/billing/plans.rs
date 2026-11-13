use serde::Serialize;

/// Integer microdollars throughout: no floating-point quota arithmetic.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Plan {
    pub id: &'static str,
    pub monthly_cents: i64,
    pub ai_microdollars: i64,
    pub transcription_seconds: i64,
    pub storage_bytes: i64,
    pub max_devices: u64,
}
pub const STANDARD: Plan = Plan {
    id: "managed",
    monthly_cents: 1710,
    ai_microdollars: 2_000_000,
    transcription_seconds: 300 * 60,
    storage_bytes: 10_000_000_000,
    max_devices: 3,
};
pub const PLUS: Plan = Plan {
    id: "managed_plus",
    monthly_cents: 4000,
    ai_microdollars: 6_000_000,
    transcription_seconds: 1200 * 60,
    storage_bytes: 50_000_000_000,
    max_devices: 6,
};
pub fn plan(id: &str) -> Option<Plan> {
    match id {
        "managed" => Some(STANDARD),
        "managed_plus" => Some(PLUS),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProvider {
    Gemini,
    DeepSeek,
}
impl ModelProvider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::DeepSeek => "deepseek",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    pub provider: ModelProvider,
    /// Conservative uncached rates, USD microdollars per million tokens.
    pub input_rate: i64,
    pub cached_input_rate: i64,
    pub output_rate: i64,
    pub multiplier: u32,
    pub plus_only: bool,
}
pub const MODELS: &[Model] = &[
    Model {
        id: "gemini-3.5-flash-lite",
        name: "Gemini 3.5 Flash-Lite",
        provider: ModelProvider::Gemini,
        input_rate: 300_000,
        cached_input_rate: 30_000,
        output_rate: 2_500_000,
        multiplier: 1,
        plus_only: false,
    },
    Model {
        id: "gemini-3.8-flash",
        name: "Gemini 3.8 Flash",
        provider: ModelProvider::Gemini,
        input_rate: 1_500_000,
        cached_input_rate: 150_000,
        output_rate: 7_500_000,
        multiplier: 5,
        plus_only: false,
    },
    Model {
        id: "gemini-4-hydrogen",
        name: "Gemini 4 Hydrogen",
        provider: ModelProvider::Gemini,
        input_rate: 1_500_000,
        cached_input_rate: 75_000,
        output_rate: 7_500_000,
        multiplier: 5,
        plus_only: false,
    },
    Model {
        id: "gemini-4-argon",
        name: "Gemini 4 Argon",
        provider: ModelProvider::Gemini,
        input_rate: 4_000_000,
        cached_input_rate: 200_000,
        output_rate: 20_000_000,
        multiplier: 14,
        plus_only: true,
    },
    Model {
        id: "gemini-4-carbon",
        name: "Gemini 4 Carbon",
        provider: ModelProvider::Gemini,
        input_rate: 4_000_000,
        cached_input_rate: 200_000,
        output_rate: 20_000_000,
        multiplier: 14,
        plus_only: true,
    },
    Model {
        id: "deepseek-ai/DeepSeek-V4.1-Flash-1017",
        name: "DeepSeek V4.1 Flash",
        provider: ModelProvider::DeepSeek,
        // Rates supplied for the hosted Modal endpoint, per million tokens.
        input_rate: 250_000,
        cached_input_rate: 5_000,
        output_rate: 1_000_000,
        multiplier: 1,
        plus_only: false,
    },
];

/// Standard uncached pricing from https://ai.google.dev/gemini-api/docs/pricing.
/// Keep the catalog and billing on the same rates as introductory prices expire.
pub fn models() -> Vec<Model> {
    models_on(chrono::Utc::now().date_naive())
}

fn models_on(date: chrono::NaiveDate) -> Vec<Model> {
    MODELS
        .iter()
        .copied()
        .map(|mut model| {
            match model.id {
                "gemini-3.8-flash"
                    if date < chrono::NaiveDate::from_ymd_opt(2027, 1, 1).unwrap() =>
                {
                    model.input_rate = 750_000;
                    model.cached_input_rate = 75_000;
                    model.output_rate = 3_750_000;
                }
                "gemini-4-argon" | "gemini-4-carbon"
                    if date < chrono::NaiveDate::from_ymd_opt(2026, 12, 1).unwrap() =>
                {
                    model.input_rate = 2_000_000;
                    model.cached_input_rate = 100_000;
                    model.output_rate = 10_000_000;
                }
                _ => {}
            }
            // Gemini Flash-Lite is the 1x rate baseline.
            // This is a relative rate label; actual credits use measured token cost.
            model.multiplier = ((model.input_rate + 299_999) / 300_000)
                .max((model.output_rate + 2_499_999) / 2_500_000)
                as u32;
            model
        })
        .collect()
}
impl Model {
    pub fn cost(self, input: i64, output: i64) -> i64 {
        self.cost_with_cached(input, output, 0)
    }
    pub fn cost_with_cached(self, input: i64, output: i64, cached: i64) -> i64 {
        let input = input.max(0);
        let cached = cached.clamp(0, input);
        ((i128::from(input - cached) * i128::from(self.input_rate)
            + i128::from(cached) * i128::from(self.cached_input_rate)
            + i128::from(output.max(0)) * i128::from(self.output_rate)
            + 999_999)
            / 1_000_000)
            .min(i128::from(i64::MAX)) as i64
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gemini_pricing_and_weights_change_at_the_published_boundaries() {
        let catalog = |y, m, d| models_on(chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap());
        let november = catalog(2026, 11, 30);
        let december = catalog(2026, 12, 1);
        let january = catalog(2027, 1, 1);
        assert_eq!(
            november.iter().map(|m| m.multiplier).collect::<Vec<_>>(),
            [1, 3, 5, 7, 7, 1]
        );
        assert_eq!(
            december.iter().map(|m| m.multiplier).collect::<Vec<_>>(),
            [1, 3, 5, 14, 14, 1]
        );
        assert_eq!(
            january.iter().map(|m| m.multiplier).collect::<Vec<_>>(),
            [1, 5, 5, 14, 14, 1]
        );
        assert_eq!(november[3].cost(1_000_000, 1_000_000), 12_000_000);
        assert_eq!(december[3].cost(1_000_000, 1_000_000), 24_000_000);
        assert_eq!(january[1].cost(1_000_000, 1_000_000), 9_000_000);
        let deepseek = *MODELS.last().unwrap();
        assert_eq!(
            deepseek.cost_with_cached(1_000_000, 1_000_000, 500_000),
            1_127_500
        );
        assert_eq!(deepseek.cost_with_cached(1_000_000, 0, 1_000_000), 5_000);
    }
    #[test]
    fn conservative_margins_ignore_promotional_credits() {
        for p in [STANDARD, PLUS] {
            let revenue = p.monthly_cents as f64 / 100.;
            let speech = p.transcription_seconds as f64 / 60. * 2. * 0.0092;
            let fee = revenue * 0.06 + 0.40; // international card + subscription surcharge
            let overhead = if p.id == "managed" { 1.0 } else { 2.0 };
            assert!(
                revenue - speech - fee - overhead - p.ai_microdollars as f64 / 1_000_000. >= 7.0
            );
        }
    }
    #[test]
    fn costs_round_up_and_models_are_unique() {
        assert_eq!(MODELS[0].cost(1, 1), 3);
        let mut names = std::collections::HashSet::new();
        for m in MODELS {
            assert!(names.insert(m.id));
            assert!(m.multiplier > 0);
        }
    }

    #[test]
    fn storage_quotas_use_the_advertised_decimal_gigabytes() {
        assert_eq!(STANDARD.storage_bytes, 10_000_000_000);
        assert_eq!(PLUS.storage_bytes, 50_000_000_000);
    }
}
