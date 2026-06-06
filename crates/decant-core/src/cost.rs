use crate::model::TokenUsage;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy)]
pub struct Price {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

/// Seed pricing (USD per million tokens). Estimates; editable in the DB later.
/// Unknown models estimate to 0.0 (surfaced as "unknown" in the UI).
pub fn default_pricing() -> HashMap<&'static str, Price> {
    let mut m = HashMap::new();
    // Claude (Anthropic) — representative published rates.
    m.insert("claude-opus-4-7", Price { input_per_mtok: 15.0, output_per_mtok: 75.0, cache_read_per_mtok: 1.5, cache_write_per_mtok: 18.75 });
    m.insert("claude-sonnet-4-6", Price { input_per_mtok: 3.0, output_per_mtok: 15.0, cache_read_per_mtok: 0.3, cache_write_per_mtok: 3.75 });
    m.insert("claude-haiku-4-5", Price { input_per_mtok: 1.0, output_per_mtok: 5.0, cache_read_per_mtok: 0.1, cache_write_per_mtok: 1.25 });
    m
}

/// Estimate cost in USD for one session's usage under a pricing table.
pub fn estimate_cost(model: Option<&str>, usage: &TokenUsage, pricing: &HashMap<&'static str, Price>) -> f64 {
    let Some(model) = model else { return 0.0 };
    let Some(p) = pricing.get(model) else { return 0.0 };
    let per = |tokens: i64, rate: f64| (tokens as f64) * rate / 1_000_000.0;
    per(usage.input, p.input_per_mtok)
        + per(usage.output, p.output_per_mtok)
        + per(usage.cache_read, p.cache_read_per_mtok)
        + per(usage.cache_creation, p.cache_write_per_mtok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_model_costs_add_up() {
        let pricing = default_pricing();
        let usage = TokenUsage { input: 1_000_000, output: 1_000_000, cache_read: 0, cache_creation: 0 };
        let cost = estimate_cost(Some("claude-opus-4-7"), &usage, &pricing);
        assert!((cost - 90.0).abs() < 1e-6, "got {cost}");
    }

    #[test]
    fn unknown_model_is_zero() {
        let pricing = default_pricing();
        let usage = TokenUsage { input: 5_000, output: 5_000, cache_read: 0, cache_creation: 0 };
        assert_eq!(estimate_cost(Some("gpt-5.4"), &usage, &pricing), 0.0);
        assert_eq!(estimate_cost(None, &usage, &pricing), 0.0);
    }
}
