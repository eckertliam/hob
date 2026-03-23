//! Known model definitions with context limits and metadata.
//!
//! Provides model lookup for context limit calculation, default selection,
//! and display. Models are hardcoded for now — a future version could
//! fetch from models.dev like opencode does.

/// A known model definition.
#[derive(Debug, Clone)]
pub struct ModelInfo {
    /// API model ID (what you pass to the provider).
    pub id: &'static str,
    /// Human-readable name.
    pub name: &'static str,
    /// Provider: "anthropic" or "openai".
    pub provider: &'static str,
    /// Max input context window in tokens.
    pub context: u32,
    /// Max output tokens.
    pub max_output: u32,
    /// Cost per million input tokens (USD).
    pub cost_input: f64,
    /// Cost per million output tokens (USD).
    pub cost_output: f64,
}

/// All known models.
pub const MODELS: &[ModelInfo] = &[
    // ── Anthropic ──────────────────────────────────────────────
    ModelInfo {
        id: "claude-opus-4-6",
        name: "Claude Opus 4.6",
        provider: "anthropic",
        context: 1_000_000,
        max_output: 128_000,
        cost_input: 5.0,
        cost_output: 25.0,
    },
    ModelInfo {
        id: "claude-sonnet-4-6",
        name: "Claude Sonnet 4.6",
        provider: "anthropic",
        context: 1_000_000,
        max_output: 64_000,
        cost_input: 3.0,
        cost_output: 15.0,
    },
    ModelInfo {
        id: "claude-haiku-4-5-20251001",
        name: "Claude Haiku 4.5",
        provider: "anthropic",
        context: 200_000,
        max_output: 64_000,
        cost_input: 1.0,
        cost_output: 5.0,
    },
    // Legacy Anthropic
    ModelInfo {
        id: "claude-sonnet-4-5-20250929",
        name: "Claude Sonnet 4.5",
        provider: "anthropic",
        context: 1_000_000,
        max_output: 64_000,
        cost_input: 3.0,
        cost_output: 15.0,
    },
    ModelInfo {
        id: "claude-opus-4-5-20251101",
        name: "Claude Opus 4.5",
        provider: "anthropic",
        context: 200_000,
        max_output: 64_000,
        cost_input: 5.0,
        cost_output: 25.0,
    },
    ModelInfo {
        id: "claude-sonnet-4-20250514",
        name: "Claude Sonnet 4",
        provider: "anthropic",
        context: 200_000,
        max_output: 64_000,
        cost_input: 3.0,
        cost_output: 15.0,
    },
    ModelInfo {
        id: "claude-opus-4-20250514",
        name: "Claude Opus 4",
        provider: "anthropic",
        context: 200_000,
        max_output: 32_000,
        cost_input: 5.0,
        cost_output: 25.0,
    },
    // ── OpenAI ─────────────────────────────────────────────────
    ModelInfo {
        id: "gpt-5.4",
        name: "GPT-5.4",
        provider: "openai",
        context: 1_050_000,
        max_output: 100_000,
        cost_input: 2.5,
        cost_output: 15.0,
    },
    ModelInfo {
        id: "gpt-5.4-mini",
        name: "GPT-5.4 Mini",
        provider: "openai",
        context: 1_050_000,
        max_output: 64_000,
        cost_input: 0.75,
        cost_output: 4.5,
    },
    ModelInfo {
        id: "gpt-5.4-thinking",
        name: "GPT-5.4 Thinking",
        provider: "openai",
        context: 1_050_000,
        max_output: 100_000,
        cost_input: 2.5,
        cost_output: 15.0,
    },
    ModelInfo {
        id: "gpt-5.3-codex",
        name: "GPT-5.3 Codex",
        provider: "openai",
        context: 1_050_000,
        max_output: 100_000,
        cost_input: 1.75,
        cost_output: 14.0,
    },
    ModelInfo {
        id: "o3-mini",
        name: "o3 Mini",
        provider: "openai",
        context: 200_000,
        max_output: 100_000,
        cost_input: 1.10,
        cost_output: 4.40,
    },
];

/// Default model ID when none is specified.
pub const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

/// Look up a model by ID. Returns None for unknown models.
pub fn lookup(model_id: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.id == model_id)
}

/// Get the context window limit for a model.
/// Falls back to 200K for unknown models.
pub fn context_limit(model_id: &str) -> u32 {
    lookup(model_id).map(|m| m.context).unwrap_or(200_000)
}

/// Get the max output tokens for a model.
/// Falls back to 16K for unknown models.
pub fn max_output(model_id: &str) -> u32 {
    lookup(model_id).map(|m| m.max_output).unwrap_or(16_384)
}

/// Calculate USD cost for token usage.
pub fn calculate_cost(model_id: &str, input_tokens: u32, output_tokens: u32) -> f64 {
    let info = lookup(model_id);
    let (ci, co) = info
        .map(|m| (m.cost_input, m.cost_output))
        .unwrap_or((3.0, 15.0)); // default to Sonnet pricing
    (input_tokens as f64 / 1_000_000.0) * ci + (output_tokens as f64 / 1_000_000.0) * co
}

/// Format a USD cost for display.
pub fn format_cost(cost: f64) -> String {
    if cost < 0.01 {
        format!("${:.4}", cost)
    } else {
        format!("${:.2}", cost)
    }
}

/// Infer provider from model ID.
/// Returns "anthropic" or "openai", or None for unknown models.
pub fn infer_provider(model_id: &str) -> Option<&'static str> {
    if let Some(info) = lookup(model_id) {
        return Some(info.provider);
    }
    // Heuristic fallback for unknown models
    if model_id.starts_with("claude") {
        Some("anthropic")
    } else if model_id.starts_with("gpt-") || model_id.starts_with("o3") {
        Some("openai")
    } else {
        None
    }
}

/// List models for a given provider.
pub fn models_for_provider(provider: &str) -> Vec<&'static ModelInfo> {
    MODELS.iter().filter(|m| m.provider == provider).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lookup_known_model() {
        let m = lookup("claude-sonnet-4-6").unwrap();
        assert_eq!(m.name, "Claude Sonnet 4.6");
        assert_eq!(m.context, 1_000_000);
    }

    #[test]
    fn test_lookup_unknown_returns_none() {
        assert!(lookup("nonexistent-model").is_none());
    }

    #[test]
    fn test_context_limit_known() {
        assert_eq!(context_limit("claude-opus-4-6"), 1_000_000);
        assert_eq!(context_limit("gpt-5.4"), 1_050_000);
        assert_eq!(context_limit("claude-haiku-4-5-20251001"), 200_000);
    }

    #[test]
    fn test_context_limit_unknown_defaults() {
        assert_eq!(context_limit("mystery-model"), 200_000);
    }

    #[test]
    fn test_infer_provider_known() {
        assert_eq!(infer_provider("claude-sonnet-4-6"), Some("anthropic"));
        assert_eq!(infer_provider("gpt-5.4"), Some("openai"));
    }

    #[test]
    fn test_infer_provider_heuristic() {
        assert_eq!(infer_provider("claude-future-99"), Some("anthropic"));
        assert_eq!(infer_provider("gpt-99"), Some("openai"));
    }

    #[test]
    fn test_infer_provider_unknown() {
        assert_eq!(infer_provider("llama-3"), None);
    }

    #[test]
    fn test_models_for_provider() {
        let anthropic = models_for_provider("anthropic");
        assert!(anthropic.len() >= 4);
        assert!(anthropic.iter().all(|m| m.provider == "anthropic"));

        let openai = models_for_provider("openai");
        assert!(openai.len() >= 3);
        assert!(openai.iter().all(|m| m.provider == "openai"));
    }

    #[test]
    fn test_default_model_exists() {
        assert!(lookup(DEFAULT_MODEL).is_some());
    }

    #[test]
    fn test_max_output_known() {
        assert_eq!(max_output("claude-opus-4-6"), 128_000);
        assert_eq!(max_output("gpt-5.4-mini"), 64_000);
    }

    #[test]
    fn test_calculate_cost_sonnet() {
        // 1M input tokens at $3/M + 1M output at $15/M = $18
        let cost = calculate_cost("claude-sonnet-4-6", 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_cost_small_usage() {
        // 10K input at $3/M + 1K output at $15/M
        let cost = calculate_cost("claude-sonnet-4-6", 10_000, 1_000);
        assert!((cost - 0.045).abs() < 0.001);
    }

    #[test]
    fn test_calculate_cost_unknown_model() {
        // Falls back to Sonnet pricing
        let cost = calculate_cost("unknown-model", 1_000_000, 1_000_000);
        assert!((cost - 18.0).abs() < 0.01);
    }

    #[test]
    fn test_format_cost() {
        assert_eq!(format_cost(0.045), "$0.04");
        assert_eq!(format_cost(1.50), "$1.50");
        assert_eq!(format_cost(0.001), "$0.0010");
    }
}
