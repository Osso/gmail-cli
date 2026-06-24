use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs::{self, Permissions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

// Default OAuth credentials for gmail-cli
// These are safe to embed in version control (public OAuth client)
pub const DEFAULT_CLIENT_ID: &str =
    "690797697044-6kpkd2ethnsren8m5v27qdkj2182eb4n.apps.googleusercontent.com";
pub const DEFAULT_CLIENT_SECRET: &str = "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";

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
}

impl Config {
    pub fn client_id(&self) -> &str {
        self.client_id.as_deref().unwrap_or(DEFAULT_CLIENT_ID)
    }
    pub fn client_secret(&self) -> &str {
        self.client_secret
            .as_deref()
            .unwrap_or(DEFAULT_CLIENT_SECRET)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn config_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("gmail-cli")
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn tokens_path() -> PathBuf {
    config_dir().join("tokens.json")
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn write_secure(path: &PathBuf, content: &str) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(content.as_bytes())?;
    Ok(())
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn load_config() -> Result<Config> {
    let path = config_path();
    if path.exists() {
        let content = fs::read_to_string(&path)?;
        return Ok(serde_json::from_str(&content)?);
    }
    Ok(Config::default())
}

#[cfg_attr(coverage_nightly, coverage(off))]
fn ensure_config_dir() -> Result<PathBuf> {
    let dir = config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
        fs::set_permissions(&dir, Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn save_config(config: &Config) -> Result<()> {
    ensure_config_dir()?;
    write_secure(&config_path(), &serde_json::to_string_pretty(config)?)
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn load_tokens() -> Result<Tokens> {
    let path = tokens_path();
    let content = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&content)?)
}

#[cfg_attr(coverage_nightly, coverage(off))]
pub fn save_tokens(tokens: &Tokens) -> Result<()> {
    ensure_config_dir()?;
    write_secure(&tokens_path(), &serde_json::to_string_pretty(tokens)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_returns_empty_defaults_when_unset() {
        let config = Config::default();

        assert_eq!(config.client_id(), DEFAULT_CLIENT_ID);
        assert_eq!(config.client_secret(), DEFAULT_CLIENT_SECRET);
    }

    #[test]
    fn config_returns_custom_oauth_values() {
        let config = Config {
            client_id: Some("client".to_string()),
            client_secret: Some("secret".to_string()),
        };

        assert_eq!(config.client_id(), "client");
        assert_eq!(config.client_secret(), "secret");
    }

    #[test]
    fn tokens_roundtrip_through_json() {
        let tokens = Tokens {
            access_token: "access".to_string(),
            refresh_token: "refresh".to_string(),
        };

        let json = serde_json::to_string(&tokens).unwrap();
        let parsed: Tokens = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.access_token, "access");
        assert_eq!(parsed.refresh_token, "refresh");
    }
}
