use crate::model::TokenUsage;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct Price {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

/// Seed pricing (USD per million tokens), keyed by canonical model id.
///
/// Rates are published list prices as of 2026-06 for the Anthropic Claude and
/// OpenAI GPT-5 families. They are seed estimates: ingest computes a session's
/// cost from these and stores it in `session.estimated_cost_usd`. The
/// `model_pricing` table exists as the intended override point for refining
/// these later; today the values here are authoritative. Unknown models
/// estimate to 0.0 (surfaced as "unknown" in the UI).
///
/// Claude is keyed by tier (opus/sonnet/haiku): every current version within a
/// tier shares a price, so `canonical_model` collapses versions onto one key.
/// Anthropic cache rates follow the documented multipliers — cache read = 0.1x
/// input, 5-minute cache write = 1.25x input. OpenAI has no separate
/// cache-write charge, so `cache_write_per_mtok` mirrors the input rate there.
pub fn default_pricing() -> HashMap<&'static str, Price> {
    let mut m = HashMap::new();

    m.insert(
        "claude-opus",
        Price {
            input_per_mtok: 5.0,
            output_per_mtok: 25.0,
            cache_read_per_mtok: 0.5,
            cache_write_per_mtok: 6.25,
        },
    );
    m.insert(
        "claude-sonnet",
        Price {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.3,
            cache_write_per_mtok: 3.75,
        },
    );
    m.insert(
        "claude-haiku",
        Price {
            input_per_mtok: 1.0,
            output_per_mtok: 5.0,
            cache_read_per_mtok: 0.1,
            cache_write_per_mtok: 1.25,
        },
    );

    // OpenAI GPT-5 family. gpt-5 and gpt-5.1 share a price.
    m.insert(
        "gpt-5",
        Price {
            input_per_mtok: 1.25,
            output_per_mtok: 10.0,
            cache_read_per_mtok: 0.125,
            cache_write_per_mtok: 1.25,
        },
    );
    m.insert(
        "gpt-5.4",
        Price {
            input_per_mtok: 2.5,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.25,
            cache_write_per_mtok: 2.5,
        },
    );
    m.insert(
        "gpt-5.4-mini",
        Price {
            input_per_mtok: 0.75,
            output_per_mtok: 4.5,
            cache_read_per_mtok: 0.075,
            cache_write_per_mtok: 0.75,
        },
    );
    m.insert(
        "gpt-5.4-nano",
        Price {
            input_per_mtok: 0.2,
            output_per_mtok: 1.25,
            cache_read_per_mtok: 0.02,
            cache_write_per_mtok: 0.2,
        },
    );
    m.insert(
        "gpt-5.5",
        Price {
            input_per_mtok: 5.0,
            output_per_mtok: 30.0,
            cache_read_per_mtok: 0.5,
            cache_write_per_mtok: 5.0,
        },
    );

    // Codex models. Codex bills in credits (1 credit = $0.04, calibrated against
    // the GPT-5.4 card: 62.5 credits/Mtok in = $2.50). gpt-5.2 and gpt-5.3-codex
    // share a rate (43.75 / 4.375 / 350 credits = $1.75 / $0.175 / $14). Codex's
    // automatic code review ("codex-auto-review") runs on gpt-5.3-codex.
    m.insert(
        "gpt-5.3-codex",
        Price {
            input_per_mtok: 1.75,
            output_per_mtok: 14.0,
            cache_read_per_mtok: 0.175,
            cache_write_per_mtok: 1.75,
        },
    );

    m
}

/// Map a raw model string to a canonical pricing key.
///
/// Model strings vary widely by source. Claude Code emits clean ids
/// (`claude-opus-4-8`), sometimes with a date (`claude-haiku-4-5-20251001`), a
/// context suffix (`claude-opus-4-8[1m]`), or a bare alias (`opus`). Codex
/// emits Bedrock-style ARNs (`us.anthropic.claude-opus-4-6-v1`,
/// `us.anthropic.claude-haiku-4-5-20251001-v1:0`) alongside OpenAI ids
/// (`gpt-5.4`, `gpt-5.5`). We match on family/tier so every variant prices
/// correctly instead of silently falling through to $0. Returns `None` for
/// models we deliberately don't price (e.g. `<synthetic>`, `exa-research-*`,
/// `codex-auto-review`, or any unrecognized vendor).
fn canonical_model(raw: &str) -> Option<&'static str> {
    let s = raw.to_ascii_lowercase();

    // Anthropic Claude — match the tier regardless of version, vendor prefix,
    // date stamp, context suffix, or alias form.
    if s.contains("claude") || s == "opus" || s == "sonnet" || s == "haiku" {
        if s.contains("opus") {
            return Some("claude-opus");
        }
        if s.contains("sonnet") {
            return Some("claude-sonnet");
        }
        if s.contains("haiku") {
            return Some("claude-haiku");
        }
        return None; // a "claude-*" we don't recognize
    }

    if s.starts_with("codex-auto-review") {
        return Some("gpt-5.3-codex");
    }

    // OpenAI GPT-5 family — check the most specific tiers first.
    if s.starts_with("gpt-5.4-nano") {
        return Some("gpt-5.4-nano");
    }
    if s.starts_with("gpt-5.4-mini") {
        return Some("gpt-5.4-mini");
    }
    if s.starts_with("gpt-5.4") {
        return Some("gpt-5.4");
    }
    if s.starts_with("gpt-5.5") {
        return Some("gpt-5.5");
    }
    // gpt-5.3-codex (and -spark) and gpt-5.2 share the Codex rate card.
    if s.starts_with("gpt-5.3-codex") || s.starts_with("gpt-5.2") {
        return Some("gpt-5.3-codex");
    }
    // gpt-5, gpt-5.1, and any other gpt-5 sub-variant we don't special-case.
    if s.starts_with("gpt-5") {
        return Some("gpt-5");
    }

    None
}

/// Whether `model` maps to a known price. Used when picking a session's primary
/// model so synthetic/auxiliary markers (`<synthetic>`, `exa-research-*`) don't
/// mask the real, billable model and zero out the session's cost.
pub fn is_priceable(model: &str) -> bool {
    canonical_model(model).is_some()
}

/// Estimate cost in USD for one session's usage under a pricing table.
pub fn estimate_cost(
    model: Option<&str>,
    usage: &TokenUsage,
    pricing: &HashMap<&'static str, Price>,
) -> f64 {
    let Some(model) = model else { return 0.0 };
    let Some(key) = canonical_model(model) else {
        return 0.0;
    };
    let Some(p) = pricing.get(key) else {
        return 0.0;
    };
    let per = |tokens: i64, rate: f64| (tokens as f64) * rate / 1_000_000.0;
    per(usage.input, p.input_per_mtok)
        + per(usage.output, p.output_per_mtok)
        + per(usage.cache_read, p.cache_read_per_mtok)
        + per(usage.cache_creation, p.cache_write_per_mtok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage_1m() -> TokenUsage {
        TokenUsage {
            input: 1_000_000,
            output: 1_000_000,
            cache_read: 0,
            cache_creation: 0,
        }
    }

    #[test]
    fn opus_input_output_costs_add_up() {
        let pricing = default_pricing();
        // 1M input @ $5 + 1M output @ $25 = $30.
        let cost = estimate_cost(Some("claude-opus-4-7"), &usage_1m(), &pricing);
        assert!((cost - 30.0).abs() < 1e-6, "got {cost}");
    }

    #[test]
    fn cache_tokens_are_priced() {
        let pricing = default_pricing();
        let usage = TokenUsage {
            input: 0,
            output: 0,
            cache_read: 1_000_000,
            cache_creation: 1_000_000,
        };
        // opus: cache read $0.50 + cache write $6.25 = $6.75.
        let cost = estimate_cost(Some("claude-opus-4-8"), &usage, &pricing);
        assert!((cost - 6.75).abs() < 1e-6, "got {cost}");
    }

    #[test]
    fn claude_variants_normalize_to_their_tier() {
        let pricing = default_pricing();
        let u = usage_1m();

        // Date suffix, 1M-context suffix, bare alias, and Bedrock ARN all price as opus.
        let opus = estimate_cost(Some("claude-opus-4-8"), &u, &pricing);
        assert!((opus - 30.0).abs() < 1e-6);
        for m in [
            "claude-opus-4-6",
            "claude-opus-4-8[1m]",
            "opus",
            "us.anthropic.claude-opus-4-6-v1",
        ] {
            assert!(
                (estimate_cost(Some(m), &u, &pricing) - opus).abs() < 1e-6,
                "{m}"
            );
        }

        // Bedrock haiku ARN (vendor prefix + date + version suffix) prices as haiku.
        let haiku = estimate_cost(Some("claude-haiku-4-5"), &u, &pricing);
        let haiku_arn = estimate_cost(
            Some("us.anthropic.claude-haiku-4-5-20251001-v1:0"),
            &u,
            &pricing,
        );
        assert!((haiku_arn - haiku).abs() < 1e-6);

        // Sonnet 4.5 ARN prices as the sonnet tier.
        let sonnet = estimate_cost(Some("claude-sonnet-4-6"), &u, &pricing);
        let sonnet_45 = estimate_cost(
            Some("us.anthropic.claude-sonnet-4-5-20250929-v1:0"),
            &u,
            &pricing,
        );
        assert!((sonnet_45 - sonnet).abs() < 1e-6);
    }

    #[test]
    fn gpt_family_is_priced() {
        let pricing = default_pricing();
        let u = usage_1m();
        assert!((estimate_cost(Some("gpt-5"), &u, &pricing) - 11.25).abs() < 1e-6); // 1.25 + 10
        assert!((estimate_cost(Some("gpt-5.1"), &u, &pricing) - 11.25).abs() < 1e-6); // shares gpt-5
        assert!((estimate_cost(Some("gpt-5.4"), &u, &pricing) - 17.5).abs() < 1e-6); // 2.5 + 15
        assert!((estimate_cost(Some("gpt-5.4-mini"), &u, &pricing) - 5.25).abs() < 1e-6); // 0.75 + 4.5
        assert!((estimate_cost(Some("gpt-5.4-nano"), &u, &pricing) - 1.45).abs() < 1e-6); // 0.2 + 1.25
        assert!((estimate_cost(Some("gpt-5.5"), &u, &pricing) - 35.0).abs() < 1e-6);
        // 5 + 30
    }

    #[test]
    fn codex_models_are_priced() {
        let pricing = default_pricing();
        let u = usage_1m();
        // gpt-5.3-codex (and gpt-5.2): $1.75 in + $14 out = $15.75.
        assert!((estimate_cost(Some("gpt-5.3-codex"), &u, &pricing) - 15.75).abs() < 1e-6);
        assert!((estimate_cost(Some("gpt-5.2"), &u, &pricing) - 15.75).abs() < 1e-6);
        // Codex automatic review is billed at the gpt-5.3-codex rate.
        assert!((estimate_cost(Some("codex-auto-review"), &u, &pricing) - 15.75).abs() < 1e-6);
    }

    #[test]
    fn unknown_models_are_zero() {
        let pricing = default_pricing();
        let usage = TokenUsage {
            input: 5_000,
            output: 5_000,
            cache_read: 0,
            cache_creation: 0,
        };
        // Strings we deliberately don't price (no per-token rate or not an LLM).
        assert_eq!(estimate_cost(Some("<synthetic>"), &usage, &pricing), 0.0);
        assert_eq!(
            estimate_cost(Some("exa-research-pro"), &usage, &pricing),
            0.0
        );
        assert_eq!(
            estimate_cost(Some("some-future-llm"), &usage, &pricing),
            0.0
        );
        assert_eq!(estimate_cost(None, &usage, &pricing), 0.0);
    }
}
