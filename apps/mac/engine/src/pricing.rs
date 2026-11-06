//! List prices for estimating what on-device usage costs. Providers change prices;
//! the user can override any of these. Port of `packages/engine/src/pricing.ts`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Dollars per million input and output tokens, or per minute of audio.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per_minute: Option<f64>,
}

const fn tokens(input: f64, output: f64) -> Price {
    Price { input: Some(input), output: Some(output), per_minute: None }
}
const fn input_only(input: f64) -> Price {
    Price { input: Some(input), output: None, per_minute: None }
}
const fn per_minute(rate: f64) -> Price {
    Price { input: None, output: None, per_minute: Some(rate) }
}

/// Published standard rates, November 2026, in the TypeScript list's order.
pub const MODEL_PRICES: &[(&str, Price)] = &[
    ("claude-fable-5-1", tokens(10.0, 50.0)),
    ("claude-fable-5", tokens(10.0, 50.0)),
    ("claude-opus-5-5", tokens(4.0, 20.0)),
    ("claude-opus-5", tokens(5.0, 25.0)),
    ("claude-opus-4-8", tokens(5.0, 25.0)),
    ("claude-sonnet-5-5", tokens(2.0, 10.0)),
    ("claude-sonnet-5", tokens(2.0, 10.0)),
    ("claude-haiku-5-5", tokens(0.1, 0.5)),
    ("text-embedding-3-large", input_only(0.13)),
    ("text-embedding-3-small", input_only(0.02)),
    ("gpt-4o-mini-tts", per_minute(0.015)),
    // The voice layer only; the model it hands questions to is billed by OpenAI on top.
    ("gpt-live-1", per_minute(0.05)),
    ("gemini-embedding-001", input_only(0.15)),
    ("gemini-2.5-flash", tokens(0.3, 2.5)),
];

pub type PriceOverrides = BTreeMap<String, Price>;

/// The price for a model: the user's own first, then the list. Dated ids like
/// claude-opus-5-5-20261001 match their base id.
pub fn price_for(model: &str, overrides: &PriceOverrides) -> Option<Price> {
    let dated = |id: &str| model.strip_prefix(id).is_some_and(|rest| rest.starts_with('-'));
    overrides
        .get(model)
        .or_else(|| overrides.iter().find(|(id, _)| dated(id)).map(|(_, p)| p))
        .copied()
        .or_else(|| MODEL_PRICES.iter().find(|(id, _)| *id == model).map(|(_, p)| *p))
        .or_else(|| MODEL_PRICES.iter().find(|(id, _)| dated(id)).map(|(_, p)| *p))
}

/// Estimated dollars for some usage, or None when the model has no price.
pub fn cost_of(model: &str, input_tokens: u64, output_tokens: u64, audio_seconds: f64, overrides: &PriceOverrides) -> Option<f64> {
    let p = price_for(model, overrides)?;
    Some(
        input_tokens as f64 * p.input.unwrap_or(0.0) / 1e6
            + output_tokens as f64 * p.output.unwrap_or(0.0) / 1e6
            + audio_seconds / 60.0 * p.per_minute.unwrap_or(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_of_prices_tokens_and_audio_from_the_list() {
        let none = PriceOverrides::new();
        assert_eq!(cost_of("claude-opus-5-5", 1_000_000, 100_000, 0.0, &none), Some(4.0 + 2.0));
        assert_eq!(cost_of("gpt-4o-mini-tts", 0, 0, 600.0, &none), Some(0.15));
        assert!((cost_of("gpt-live-1", 0, 0, 90.0, &none).unwrap() - 0.075).abs() < 1e-9);
    }

    #[test]
    fn unknown_models_have_no_price_until_the_user_sets_one() {
        assert_eq!(cost_of("gpt-6.1-sol", 2_000_000, 0, 0.0, &PriceOverrides::new()), None);
        let mine = PriceOverrides::from([("gpt-6.1-sol".to_string(), tokens(1.5, 6.0))]);
        assert_eq!(cost_of("gpt-6.1-sol", 2_000_000, 0, 0.0, &mine), Some(3.0));
    }

    #[test]
    fn dated_model_ids_match_their_base_price_and_overrides_win() {
        assert_eq!(price_for("claude-sonnet-5-5-20261001", &PriceOverrides::new()), Some(tokens(2.0, 10.0)));
        let mine = PriceOverrides::from([("claude-sonnet-5-5".to_string(), tokens(1.0, 5.0))]);
        assert_eq!(price_for("claude-sonnet-5-5", &mine), Some(tokens(1.0, 5.0)));
    }
}
