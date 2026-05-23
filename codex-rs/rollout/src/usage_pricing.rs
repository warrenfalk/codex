use codex_protocol::protocol::TokenUsage;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCostBreakdown {
    pub total_usd: f64,
    pub input_usd: f64,
    pub cached_input_usd: f64,
    pub uncached_input_usd: f64,
    pub output_usd: f64,
    pub reasoning_output_usd: f64,
}

impl UsageCostBreakdown {
    pub fn add_assign(&mut self, other: &Self) {
        self.total_usd += other.total_usd;
        self.input_usd += other.input_usd;
        self.cached_input_usd += other.cached_input_usd;
        self.uncached_input_usd += other.uncached_input_usd;
        self.output_usd += other.output_usd;
        self.reasoning_output_usd += other.reasoning_output_usd;
    }
}

#[derive(Debug, Clone, Copy)]
struct ModelTokenPricing {
    model: &'static str,
    input_per_million_usd: f64,
    cached_input_per_million_usd: f64,
    output_per_million_usd: f64,
}

impl ModelTokenPricing {
    fn cost(self, usage: &TokenUsage) -> UsageCostBreakdown {
        let cached_input_usd =
            cost_for_tokens(usage.cached_input(), self.cached_input_per_million_usd);
        let uncached_input_usd =
            cost_for_tokens(usage.non_cached_input(), self.input_per_million_usd);
        let input_usd = cached_input_usd + uncached_input_usd;
        let output_usd = cost_for_tokens(usage.output_tokens.max(0), self.output_per_million_usd);
        let reasoning_output_usd = cost_for_tokens(
            usage.reasoning_output_tokens.max(0),
            self.output_per_million_usd,
        );

        UsageCostBreakdown {
            total_usd: input_usd + output_usd,
            input_usd,
            cached_input_usd,
            uncached_input_usd,
            output_usd,
            reasoning_output_usd,
        }
    }
}

// Standard OpenAI API token prices, USD per 1M tokens, verified against OpenAI's
// public pricing/model pages on 2026-05-05. Rollouts do not record enough
// billing context to model Batch, Flex, Priority, data residency, tool fees, or
// long-context uplifts, so those are intentionally excluded.
//
// These are intentionally explicit instead of inferred by model-family prefix:
// missing rows should be noisy rather than silently priced as the wrong model.
const OPENAI_STANDARD_TOKEN_PRICING: &[ModelTokenPricing] = &[
    ModelTokenPricing {
        model: "gpt-5.5",
        input_per_million_usd: 5.00,
        cached_input_per_million_usd: 0.50,
        output_per_million_usd: 30.00,
    },
    ModelTokenPricing {
        model: "gpt-5.4",
        input_per_million_usd: 2.50,
        cached_input_per_million_usd: 0.25,
        output_per_million_usd: 15.00,
    },
    ModelTokenPricing {
        model: "gpt-5.4-mini",
        input_per_million_usd: 0.75,
        cached_input_per_million_usd: 0.075,
        output_per_million_usd: 4.50,
    },
    ModelTokenPricing {
        model: "gpt-5.4-nano",
        input_per_million_usd: 0.20,
        cached_input_per_million_usd: 0.02,
        output_per_million_usd: 1.25,
    },
    ModelTokenPricing {
        model: "gpt-5.4-pro",
        input_per_million_usd: 30.00,
        cached_input_per_million_usd: 30.00,
        output_per_million_usd: 180.00,
    },
    ModelTokenPricing {
        model: "gpt-5.3-codex",
        input_per_million_usd: 1.75,
        cached_input_per_million_usd: 0.175,
        output_per_million_usd: 14.00,
    },
    ModelTokenPricing {
        model: "gpt-5.2",
        input_per_million_usd: 1.75,
        cached_input_per_million_usd: 0.175,
        output_per_million_usd: 14.00,
    },
    ModelTokenPricing {
        model: "gpt-5.2-codex",
        input_per_million_usd: 1.75,
        cached_input_per_million_usd: 0.175,
        output_per_million_usd: 14.00,
    },
    ModelTokenPricing {
        model: "gpt-5.2-pro",
        input_per_million_usd: 21.00,
        cached_input_per_million_usd: 21.00,
        output_per_million_usd: 168.00,
    },
    ModelTokenPricing {
        model: "gpt-5.1",
        input_per_million_usd: 1.25,
        cached_input_per_million_usd: 0.125,
        output_per_million_usd: 10.00,
    },
    ModelTokenPricing {
        model: "gpt-5.1-codex",
        input_per_million_usd: 1.25,
        cached_input_per_million_usd: 0.125,
        output_per_million_usd: 10.00,
    },
    ModelTokenPricing {
        model: "gpt-5.1-codex-max",
        input_per_million_usd: 1.25,
        cached_input_per_million_usd: 0.125,
        output_per_million_usd: 10.00,
    },
    ModelTokenPricing {
        model: "gpt-5",
        input_per_million_usd: 1.25,
        cached_input_per_million_usd: 0.125,
        output_per_million_usd: 10.00,
    },
    ModelTokenPricing {
        model: "gpt-5-codex",
        input_per_million_usd: 1.25,
        cached_input_per_million_usd: 0.125,
        output_per_million_usd: 10.00,
    },
    ModelTokenPricing {
        model: "gpt-5-mini",
        input_per_million_usd: 0.25,
        cached_input_per_million_usd: 0.025,
        output_per_million_usd: 2.00,
    },
    ModelTokenPricing {
        model: "gpt-5-nano",
        input_per_million_usd: 0.05,
        cached_input_per_million_usd: 0.005,
        output_per_million_usd: 0.40,
    },
    ModelTokenPricing {
        model: "gpt-5-pro",
        input_per_million_usd: 15.00,
        cached_input_per_million_usd: 15.00,
        output_per_million_usd: 120.00,
    },
];

pub fn estimate_openai_standard_cost(
    model_provider: Option<&str>,
    model: Option<&str>,
    usage: &TokenUsage,
) -> Result<UsageCostBreakdown, PricingLookupError> {
    let Some(model) = model else {
        return Err(PricingLookupError::MissingModel);
    };
    let Some(normalized_model) = normalize_model_id(model) else {
        return Err(PricingLookupError::MissingModel);
    };
    if model_provider.is_some_and(is_known_non_openai_provider) {
        return Err(PricingLookupError::UnsupportedProvider);
    }

    OPENAI_STANDARD_TOKEN_PRICING
        .iter()
        .find(|pricing| pricing.model == normalized_model)
        .map(|pricing| pricing.cost(usage))
        .ok_or(PricingLookupError::UnknownModel)
}

pub fn pricing_lookup_description(model_provider: Option<&str>, model: Option<&str>) -> String {
    match (model_provider, model) {
        (Some(provider), Some(model)) => format!("{provider}/{model}"),
        (Some(provider), None) => format!("{provider}/<missing-model>"),
        (None, Some(model)) => model.to_string(),
        (None, None) => "<missing-provider>/<missing-model>".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PricingLookupError {
    MissingModel,
    UnsupportedProvider,
    UnknownModel,
}

fn is_known_non_openai_provider(provider: &str) -> bool {
    matches!(
        provider,
        "amazon-bedrock" | "lmstudio" | "ollama" | "oss" | "local"
    )
}

fn normalize_model_id(model: &str) -> Option<String> {
    let lowercase = model.trim().to_ascii_lowercase();
    let suffix = lowercase.rsplit('/').next().unwrap_or(&lowercase);
    if suffix.is_empty() {
        return None;
    }
    Some(strip_snapshot_date_suffix(suffix).to_string())
}

fn strip_snapshot_date_suffix(model: &str) -> &str {
    if model.len() > 11 {
        let suffix_start = model.len() - 10;
        let separator_start = suffix_start - 1;
        let maybe_date = &model[suffix_start..];
        if model.as_bytes()[separator_start] == b'-' && is_yyyy_mm_dd(maybe_date) {
            return &model[..separator_start];
        }
    }
    model
}

fn is_yyyy_mm_dd(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10 {
        return false;
    }
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 7) {
            if *byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn cost_for_tokens(tokens: i64, per_million_usd: f64) -> f64 {
    tokens.max(0) as f64 * per_million_usd / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use codex_protocol::protocol::TokenUsage;
    use pretty_assertions::assert_eq;

    use super::*;

    #[test]
    fn estimates_cached_uncached_and_output_costs() {
        let cost = estimate_openai_standard_cost(
            Some("openai"),
            Some("gpt-5.4"),
            &TokenUsage {
                input_tokens: 1_000_000,
                cached_input_tokens: 400_000,
                output_tokens: 100_000,
                reasoning_output_tokens: 25_000,
                total_tokens: 1_100_000,
            },
        )
        .expect("pricing should exist");

        assert_eq!(
            cost,
            UsageCostBreakdown {
                total_usd: 3.1,
                input_usd: 1.6,
                cached_input_usd: 0.1,
                uncached_input_usd: 1.5,
                output_usd: 1.5,
                reasoning_output_usd: 0.375,
            }
        );
    }

    #[test]
    fn prices_snapshot_model_ids_as_base_model() {
        let cost = estimate_openai_standard_cost(
            Some("openai"),
            Some("gpt-5.4-2026-03-05"),
            &TokenUsage {
                input_tokens: 1_000_000,
                cached_input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 1_000_000,
            },
        )
        .expect("snapshot pricing should use base model");

        assert_eq!(cost.uncached_input_usd, 2.5);
    }

    #[test]
    fn rejects_known_non_openai_provider() {
        assert_eq!(
            estimate_openai_standard_cost(Some("ollama"), Some("gpt-5.4"), &TokenUsage::default())
                .expect_err("local provider should not use OpenAI prices"),
            PricingLookupError::UnsupportedProvider
        );
    }
}
