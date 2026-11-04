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
pub struct Model {
    pub id: &'static str,
    pub name: &'static str,
    /// Conservative uncached rates, USD microdollars per million tokens.
    pub input_rate: i64,
    pub output_rate: i64,
    pub multiplier: u32,
    pub plus_only: bool,
}
pub const MODELS: &[Model] = &[
    Model {
        id: "openai/gpt-6-luna",
        name: "GPT-6 Luna",
        input_rate: 100_000,
        output_rate: 500_000,
        multiplier: 1,
        plus_only: false,
    },
    Model {
        id: "anthropic/claude-haiku-5-5",
        name: "Claude Haiku 5.5",
        input_rate: 100_000,
        output_rate: 500_000,
        multiplier: 1,
        plus_only: false,
    },
    Model {
        id: "deepseek/deepseek-v4.1-flash",
        name: "DeepSeek V4.1 Flash",
        input_rate: 300_000,
        output_rate: 1_200_000,
        multiplier: 3,
        plus_only: false,
    },
    Model {
        id: "google/gemini-3.5-flash-lite",
        name: "Gemini 3.5 Flash-Lite",
        input_rate: 300_000,
        output_rate: 2_500_000,
        multiplier: 5,
        plus_only: false,
    },
    Model {
        id: "google/gemini-3.8-flash",
        name: "Gemini 3.8 Flash",
        input_rate: 750_000,
        output_rate: 3_750_000,
        multiplier: 8,
        plus_only: false,
    },
    Model {
        id: "anthropic/claude-sonnet-5-5",
        name: "Claude Sonnet 5.5",
        input_rate: 2_000_000,
        output_rate: 10_000_000,
        multiplier: 20,
        plus_only: true,
    },
    Model {
        id: "openai/gpt-6.1-sol",
        name: "GPT-6.1 Sol",
        input_rate: 2_000_000,
        output_rate: 10_000_000,
        multiplier: 20,
        plus_only: true,
    },
];
impl Model {
    pub fn cost(self, input: i64, output: i64) -> i64 {
        ((i128::from(input.max(0)) * i128::from(self.input_rate)
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
        assert_eq!(MODELS[0].cost(1, 1), 1);
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
