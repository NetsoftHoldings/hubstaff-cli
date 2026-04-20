use crate::error::CliError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    #[serde(default = "default_api_url")]
    pub api_url: String,
    #[serde(
        default = "default_auth_url",
        skip_serializing_if = "is_default_auth_url"
    )]
    pub auth_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org: Option<u64>,
    #[serde(default = "default_format")]
    pub format: String,
    #[serde(default, skip_serializing_if = "AuthConfig::is_empty")]
    pub auth: AuthConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: default_api_url(),
            auth_url: default_auth_url(),
            org: None,
            format: default_format(),
            auth: AuthConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AuthConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

impl AuthConfig {
    pub fn is_empty(&self) -> bool {
        self.access_token.is_none() && self.refresh_token.is_none() && self.expires_at.is_none()
    }
}

fn default_api_url() -> String {
    "https://api.hubstaff.com/v2".to_string()
}

fn default_auth_url() -> String {
    "https://account.hubstaff.com".to_string()
}

fn is_default_auth_url(url: &str) -> bool {
    url == default_auth_url()
}

fn default_format() -> String {
    "compact".to_string()
}

impl Config {
    pub fn config_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            PathBuf::from(xdg).join("hubstaff")
        } else if let Some(config) = dirs::config_dir() {
            config.join("hubstaff")
        } else {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".config")
                .join("hubstaff")
        }
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn load() -> Result<Self, CliError> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Config::default());
        }
        let content = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<(), CliError> {
        let dir = Self::config_dir();
        if !dir.exists() {
            fs::create_dir_all(&dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
            }
        }

        let path = Self::config_path();
        let content = toml::to_string_pretty(self)?;
        fs::write(&path, &content)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    pub fn get_token(&self) -> Option<String> {
        // Environment token precedence is handled by HubstaffClient.
        self.auth.access_token.clone()
    }

    pub fn resolve_org(&self, cli_org: Option<u64>) -> Result<u64, CliError> {
        cli_org.or(self.org).ok_or_else(|| {
            CliError::Config(
                "--org required. Set a default with 'hubstaff config set org <id>'".to_string(),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_correct_values() {
        let config = Config::default();
        assert_eq!(config.api_url, "https://api.hubstaff.com/v2");
        assert_eq!(config.auth_url, "https://account.hubstaff.com");
        assert_eq!(config.format, "compact");
        assert!(config.org.is_none());
        assert!(config.auth.is_empty());
    }

    #[test]
    fn auth_config_is_empty_when_all_none() {
        let auth = AuthConfig::default();
        assert!(auth.is_empty());
    }

    #[test]
    fn auth_config_not_empty_with_access_token() {
        let auth = AuthConfig {
            access_token: Some("tok".to_string()),
            ..Default::default()
        };
        assert!(!auth.is_empty());
    }

    #[test]
    fn auth_config_not_empty_with_refresh_token() {
        let auth = AuthConfig {
            refresh_token: Some("ref".to_string()),
            ..Default::default()
        };
        assert!(!auth.is_empty());
    }

    #[test]
    fn config_serialization_skips_empty_auth() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("[auth]"));
    }

    #[test]
    fn config_serialization_includes_auth_when_present() {
        let mut config = Config::default();
        config.auth.access_token = Some("test_token".to_string());
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("[auth]"));
        assert!(toml_str.contains("access_token"));
    }

    #[test]
    fn config_serialization_skips_none_org() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("org"));
    }

    #[test]
    fn config_serialization_includes_org_when_set() {
        let config = Config {
            org: Some(12345),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("org = 12345"));
    }

    #[test]
    fn config_serialization_skips_default_auth_url() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("auth_url"));
    }

    #[test]
    fn config_serialization_includes_custom_auth_url() {
        let config = Config {
            auth_url: "https://account.staging.hbstf.co".to_string(),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("auth_url"));
    }

    #[test]
    fn config_deserialization_with_defaults() {
        let toml_str = r"org = 42";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.org, Some(42));
        assert_eq!(config.api_url, "https://api.hubstaff.com/v2");
        assert_eq!(config.auth_url, "https://account.hubstaff.com");
        assert_eq!(config.format, "compact");
    }

    #[test]
    fn config_deserialization_with_all_fields() {
        let toml_str = r#"
api_url = "https://custom.api.com/v2"
auth_url = "https://custom.auth.com"
org = 99
format = "json"

[auth]
access_token = "tok123"
refresh_token = "ref456"
expires_at = "2026-04-01T00:00:00Z"
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.api_url, "https://custom.api.com/v2");
        assert_eq!(config.auth_url, "https://custom.auth.com");
        assert_eq!(config.org, Some(99));
        assert_eq!(config.format, "json");
        assert_eq!(config.auth.access_token.as_deref(), Some("tok123"));
        assert_eq!(config.auth.refresh_token.as_deref(), Some("ref456"));
    }

    #[test]
    fn config_roundtrip_serialization() {
        let config = Config {
            org: Some(555),
            auth: AuthConfig {
                access_token: Some("my_token".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.org, Some(555));
        assert_eq!(parsed.auth.access_token.as_deref(), Some("my_token"));
        assert_eq!(parsed.api_url, config.api_url);
    }

    #[test]
    fn get_token_returns_config_token() {
        let mut config = Config::default();
        config.auth.access_token = Some("config_token".to_string());
        assert_eq!(config.get_token(), Some("config_token".to_string()));
    }

    #[test]
    fn get_token_returns_none_when_empty() {
        let config = Config::default();
        assert!(config.get_token().is_none());
    }

    #[test]
    fn resolve_org_uses_cli_arg_first() {
        let config = Config {
            org: Some(100),
            ..Default::default()
        };
        assert_eq!(config.resolve_org(Some(200)).unwrap(), 200);
    }

    #[test]
    fn resolve_org_falls_back_to_config() {
        let config = Config {
            org: Some(100),
            ..Default::default()
        };
        assert_eq!(config.resolve_org(None).unwrap(), 100);
    }

    #[test]
    fn resolve_org_errors_when_neither_set() {
        let config = Config::default();
        let err = config.resolve_org(None).unwrap_err();
        match err {
            CliError::Config(msg) => assert!(msg.contains("--org required")),
            _ => panic!("expected Config error"),
        }
    }
}
