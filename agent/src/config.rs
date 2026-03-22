//! User configuration: API keys, provider, model selection.
//!
//! Stored at ~/.config/hob/config.json. Env vars take precedence
//! over config file values.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    /// Selected provider: "anthropic" or "openai".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Selected model ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Anthropic API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anthropic_api_key: Option<String>,
    /// OpenAI API key.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_api_key: Option<String>,
    /// Custom OpenAI-compatible base URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openai_base_url: Option<String>,
}

impl Config {
    /// Config file path: ~/.config/hob/config.json
    pub fn path() -> PathBuf {
        let config_dir = std::env::var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                PathBuf::from(home).join(".config")
            });
        config_dir.join("hob/config.json")
    }

    /// Load config from disk. Returns default if file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = Self::path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("failed to read config at {}", path.display()))?;
        let config: Config = serde_json::from_str(&content)
            .with_context(|| format!("failed to parse config at {}", path.display()))?;
        Ok(config)
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        Ok(())
    }

    /// Resolve the provider to use. Precedence: env var > config > auto-detect.
    pub fn resolve_provider(&self) -> Option<String> {
        std::env::var("HOB_PROVIDER")
            .ok()
            .or_else(|| self.provider.clone())
    }

    /// Resolve the model to use. Precedence: env var > config > default.
    pub fn resolve_model(&self) -> String {
        std::env::var("HOB_MODEL")
            .ok()
            .or_else(|| self.model.clone())
            .unwrap_or_else(|| crate::models::DEFAULT_MODEL.to_string())
    }

    /// Resolve the API key for a provider. Precedence: env var > config.
    pub fn resolve_api_key(&self, provider: &str) -> Option<String> {
        match provider {
            "anthropic" => std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .or_else(|| self.anthropic_api_key.clone()),
            "openai" => std::env::var("OPENAI_API_KEY")
                .ok()
                .or_else(|| self.openai_api_key.clone()),
            _ => None,
        }
    }

    /// Resolve the OpenAI base URL. Precedence: env var > config.
    pub fn resolve_base_url(&self) -> Option<String> {
        std::env::var("OPENAI_API_BASE")
            .ok()
            .or_else(|| self.openai_base_url.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.provider.is_none());
        assert!(config.model.is_none());
        assert!(config.anthropic_api_key.is_none());
    }

    #[test]
    fn test_save_and_load() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.json");

        let config = Config {
            provider: Some("anthropic".into()),
            model: Some("claude-sonnet-4-6".into()),
            anthropic_api_key: Some("sk-test".into()),
            openai_api_key: None,
            openai_base_url: None,
        };

        // Save manually to test path
        let content = serde_json::to_string_pretty(&config).unwrap();
        std::fs::write(&path, &content).unwrap();

        // Load back
        let loaded: Config = serde_json::from_str(
            &std::fs::read_to_string(&path).unwrap(),
        )
        .unwrap();
        assert_eq!(loaded.provider, Some("anthropic".into()));
        assert_eq!(loaded.model, Some("claude-sonnet-4-6".into()));
        assert_eq!(loaded.anthropic_api_key, Some("sk-test".into()));
    }

    #[test]
    fn test_resolve_model_default() {
        let config = Config::default();
        assert_eq!(config.resolve_model(), crate::models::DEFAULT_MODEL);
    }

    #[test]
    fn test_resolve_model_from_config() {
        let config = Config {
            model: Some("gpt-5.4".into()),
            ..Default::default()
        };
        // Only returns config value if env var isn't set
        // We can't control env vars in this test easily, so just check the config path
        if std::env::var("HOB_MODEL").is_err() {
            assert_eq!(config.resolve_model(), "gpt-5.4");
        }
    }

    #[test]
    fn test_resolve_api_key_from_config() {
        let config = Config {
            anthropic_api_key: Some("sk-config".into()),
            ..Default::default()
        };
        // Only returns config value if env var isn't set
        if std::env::var("ANTHROPIC_API_KEY").is_err() {
            assert_eq!(
                config.resolve_api_key("anthropic"),
                Some("sk-config".into())
            );
        }
    }

    #[test]
    fn test_serialization_skips_none() {
        let config = Config {
            provider: Some("openai".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("provider"));
        assert!(!json.contains("anthropic_api_key"));
    }
}
