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
            cache_write_input_tokens: 0,
            output_tokens: 100_000,
            reasoning_output_tokens: 25_000,
            total_tokens: 1_100_000,
            codex_rollout_budget_units: None,
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
            cache_write_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            total_tokens: 1_000_000,
            codex_rollout_budget_units: None,
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
