use crate::auth::TokenSet;
use crate::error::CliError;
use crate::persistence::write_atomic;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

const DEFAULT_API_URL: &str = "https://api.hubstaff.com/v2";
const DEFAULT_AUTH_URL: &str = "https://account.hubstaff.com";
const DEFAULT_FORMAT: &str = "json";

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct Config {
    pub api_url: String,
    #[serde(skip_serializing_if = "is_default_auth_url")]
    pub auth_url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<u64>,
    /// Explicit schema docs URL override. When unset, derived from `api_url`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_url: Option<String>,
    pub format: String,
    #[serde(skip_serializing_if = "OAuthConfig::is_empty")]
    pub oauth: OAuthConfig,
    #[serde(skip_serializing_if = "AuthConfig::is_empty")]
    pub auth: AuthConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: DEFAULT_API_URL.to_string(),
            auth_url: DEFAULT_AUTH_URL.to_string(),
            organization: None,
            schema_url: None,
            format: DEFAULT_FORMAT.to_string(),
            oauth: OAuthConfig::default(),
            auth: AuthConfig::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct OAuthConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret: Option<String>,
}

impl OAuthConfig {
    pub fn is_empty(&self) -> bool {
        self.client_id.is_none() && self.client_secret.is_none()
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AuthConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl AuthConfig {
    pub fn is_empty(&self) -> bool {
        self.access_token.is_none() && self.refresh_token.is_none() && self.expires_at.is_none()
    }
}

fn is_default_auth_url(url: &str) -> bool {
    url == DEFAULT_AUTH_URL
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

    pub fn schema_dir() -> PathBuf {
        Self::config_dir().join("schema").join("v2")
    }

    pub fn schema_docs_path() -> PathBuf {
        Self::schema_dir().join("docs.json")
    }

    pub fn schema_meta_path() -> PathBuf {
        Self::schema_dir().join("meta.toml")
    }

    pub fn schema_command_index_path() -> PathBuf {
        Self::schema_dir().join("command_index.json")
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

    pub fn ensure_dir() -> Result<PathBuf, CliError> {
        let dir = Self::config_dir();
        fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        Ok(dir)
    }

    pub fn save(&self) -> Result<(), CliError> {
        Self::ensure_dir()?;
        let path = Self::config_path();
        let content = toml::to_string_pretty(self)?;
        write_atomic(&path, content.as_bytes())?;
        Ok(())
    }

    pub fn store_tokens(&mut self, tokens: TokenSet) {
        self.auth.access_token = Some(tokens.access_token);
        self.auth.refresh_token = Some(tokens.refresh_token);
        self.auth.expires_at = tokens.expires_at;
    }

    pub fn oauth_client_id_present(&self) -> bool {
        self.oauth
            .client_id
            .as_deref()
            .is_some_and(|s| !s.is_empty())
            || std::env::var("HUBSTAFF_CLIENT_ID").is_ok_and(|v| !v.is_empty())
    }

    pub fn oauth_client_secret_present(&self) -> bool {
        self.oauth
            .client_secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
            || std::env::var("HUBSTAFF_CLIENT_SECRET").is_ok_and(|v| !v.is_empty())
    }

    pub fn has_oauth_app(&self) -> bool {
        self.oauth_client_id_present() && self.oauth_client_secret_present()
    }

    pub fn get_token(&self) -> Option<String> {
        // Environment token precedence is handled by HubstaffClient.
        self.auth.access_token.clone()
    }

    pub fn resolve_organization(&self, cli_organization: Option<u64>) -> Result<u64, CliError> {
        cli_organization.or(self.organization).ok_or_else(|| {
            CliError::Config(
                "--organization required. Set a default with 'hubstaff config set organization <id>'"
                    .to_string(),
            )
        })
    }

    pub fn effective_schema_url(&self) -> String {
        self.schema_url
            .clone()
            .unwrap_or_else(|| format!("{}/docs", self.api_url.trim_end_matches('/')))
    }

    pub fn unset(&mut self, key: &str) -> Result<(), CliError> {
        match key {
            "organization" => self.organization = None,
            "schema_url" => self.schema_url = None,
            "api_url" => self.api_url = DEFAULT_API_URL.to_string(),
            "auth_url" => self.auth_url = DEFAULT_AUTH_URL.to_string(),
            "format" => self.format = DEFAULT_FORMAT.to_string(),
            "client_id" => self.oauth.client_id = None,
            "client_secret" => self.oauth.client_secret = None,
            "token" | "refresh_token" => {
                return Err(CliError::Config(
                    "cannot unset auth tokens here; run 'hubstaff logout'".to_string(),
                ));
            }
            _ => {
                return Err(CliError::Config(format!(
                    "unknown config key: {key}. Valid keys: organization, api_url, auth_url, schema_url, format, client_id, client_secret"
                )));
            }
        }
        Ok(())
    }

    pub fn reset(&mut self) {
        let auth = std::mem::take(&mut self.auth);
        *self = Config {
            auth,
            ..Config::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_schema_url() -> String {
        format!("{DEFAULT_API_URL}/docs")
    }

    #[test]
    fn default_config_has_correct_values() {
        let config = Config::default();
        assert_eq!(config.api_url, "https://api.hubstaff.com/v2");
        assert_eq!(config.auth_url, "https://account.hubstaff.com");
        assert_eq!(config.schema_url, None);
        assert_eq!(
            config.effective_schema_url(),
            "https://api.hubstaff.com/v2/docs"
        );
        assert_eq!(config.format, "json");
        assert!(config.organization.is_none());
        assert!(config.oauth.is_empty());
        assert!(config.auth.is_empty());
    }

    #[test]
    fn oauth_config_is_empty_when_all_none() {
        let oauth = OAuthConfig::default();
        assert!(oauth.is_empty());
    }

    #[test]
    fn oauth_config_not_empty_with_client_id() {
        let oauth = OAuthConfig {
            client_id: Some("id".to_string()),
            ..Default::default()
        };
        assert!(!oauth.is_empty());
    }

    #[test]
    fn config_serialization_skips_empty_oauth() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("[oauth]"));
    }

    #[test]
    fn config_serialization_includes_oauth_when_present() {
        let config = Config {
            oauth: OAuthConfig {
                client_id: Some("cid".to_string()),
                client_secret: Some("csec".to_string()),
            },
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("[oauth]"));
        assert!(toml_str.contains("client_id = \"cid\""));
        assert!(toml_str.contains("client_secret = \"csec\""));
    }

    #[test]
    fn config_unset_clears_client_id() {
        let mut config = Config {
            oauth: OAuthConfig {
                client_id: Some("cid".to_string()),
                client_secret: Some("csec".to_string()),
            },
            ..Default::default()
        };
        config.unset("client_id").unwrap();
        assert!(config.oauth.client_id.is_none());
        assert_eq!(config.oauth.client_secret.as_deref(), Some("csec"));
    }

    #[test]
    fn config_unset_clears_client_secret() {
        let mut config = Config {
            oauth: OAuthConfig {
                client_id: Some("cid".to_string()),
                client_secret: Some("csec".to_string()),
            },
            ..Default::default()
        };
        config.unset("client_secret").unwrap();
        assert_eq!(config.oauth.client_id.as_deref(), Some("cid"));
        assert!(config.oauth.client_secret.is_none());
    }

    #[test]
    fn has_oauth_app_false_when_both_missing() {
        let config = Config::default();
        assert!(!config.has_oauth_app());
    }

    #[test]
    fn has_oauth_app_false_when_only_client_id_in_config() {
        let config = Config {
            oauth: OAuthConfig {
                client_id: Some("cid".to_string()),
                client_secret: None,
            },
            ..Default::default()
        };
        assert!(!config.has_oauth_app());
    }

    #[test]
    fn has_oauth_app_false_when_only_client_secret_in_config() {
        let config = Config {
            oauth: OAuthConfig {
                client_id: None,
                client_secret: Some("csec".to_string()),
            },
            ..Default::default()
        };
        assert!(!config.has_oauth_app());
    }

    #[test]
    fn has_oauth_app_true_when_both_in_config() {
        let config = Config {
            oauth: OAuthConfig {
                client_id: Some("cid".to_string()),
                client_secret: Some("csec".to_string()),
            },
            ..Default::default()
        };
        assert!(config.has_oauth_app());
    }

    #[test]
    fn has_oauth_app_false_when_config_values_are_empty_strings() {
        let config = Config {
            oauth: OAuthConfig {
                client_id: Some(String::new()),
                client_secret: Some(String::new()),
            },
            ..Default::default()
        };
        assert!(!config.has_oauth_app());
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
    fn store_tokens_overwrites_access_and_refresh() {
        let mut config = Config {
            auth: AuthConfig {
                access_token: Some("old_access".into()),
                refresh_token: Some("old_refresh".into()),
                expires_at: Some(1_735_689_600),
            },
            ..Default::default()
        };
        config.store_tokens(TokenSet {
            access_token: "new_access".into(),
            refresh_token: "new_refresh".into(),
            expires_at: Some(1_893_456_000),
        });
        assert_eq!(config.auth.access_token.as_deref(), Some("new_access"));
        assert_eq!(config.auth.refresh_token.as_deref(), Some("new_refresh"));
        assert_eq!(config.auth.expires_at, Some(1_893_456_000));
    }

    #[test]
    fn store_tokens_clears_stale_expires_at_when_new_has_none() {
        let mut config = Config {
            auth: AuthConfig {
                access_token: Some("old_access".into()),
                refresh_token: Some("old_refresh".into()),
                expires_at: Some(1_735_689_600),
            },
            ..Default::default()
        };
        config.store_tokens(TokenSet {
            access_token: "new_access".into(),
            refresh_token: "new_refresh".into(),
            expires_at: None,
        });
        assert_eq!(config.auth.expires_at, None);
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
    fn config_serialization_skips_none_organization() {
        let config = Config::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("organization"));
    }

    #[test]
    fn config_serialization_includes_organization_when_set() {
        let config = Config {
            organization: Some(12345),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("organization = 12345"));
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
    fn config_deserialization_with_organization_key() {
        let toml_str = r"organization = 42";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.organization, Some(42));
    }

    #[test]
    fn config_deserialization_with_all_fields() {
        let toml_str = r#"
api_url = "https://custom.api.com/v2"
auth_url = "https://custom.auth.com"
organization = 99
format = "json"

[auth]
access_token = "tok123"
refresh_token = "ref456"
expires_at = 1775347200
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.api_url, "https://custom.api.com/v2");
        assert_eq!(config.auth_url, "https://custom.auth.com");
        assert_eq!(config.organization, Some(99));
        assert_eq!(config.format, "json");
        assert_eq!(config.auth.access_token.as_deref(), Some("tok123"));
        assert_eq!(config.auth.refresh_token.as_deref(), Some("ref456"));
        assert_eq!(config.auth.expires_at, Some(1_775_347_200));
    }

    #[test]
    fn config_roundtrip_serialization() {
        let config = Config {
            organization: Some(555),
            auth: AuthConfig {
                access_token: Some("my_token".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };

        let toml_str = toml::to_string_pretty(&config).unwrap();
        let parsed: Config = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.organization, Some(555));
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
    fn resolve_organization_uses_cli_arg_first() {
        let config = Config {
            organization: Some(100),
            ..Default::default()
        };
        assert_eq!(config.resolve_organization(Some(200)).unwrap(), 200);
    }

    #[test]
    fn resolve_organization_falls_back_to_config() {
        let config = Config {
            organization: Some(100),
            ..Default::default()
        };
        assert_eq!(config.resolve_organization(None).unwrap(), 100);
    }

    #[test]
    fn resolve_organization_errors_when_neither_set() {
        let config = Config::default();
        let err = config.resolve_organization(None).unwrap_err();
        match err {
            CliError::Config(msg) => assert!(msg.contains("--organization required")),
            _ => panic!("expected Config error"),
        }
    }

    #[test]
    fn effective_schema_url_follows_custom_api_url() {
        let config = Config {
            api_url: "https://staging.api.hubstaff.com/v2".to_string(),
            ..Default::default()
        };
        assert_eq!(
            config.effective_schema_url(),
            "https://staging.api.hubstaff.com/v2/docs"
        );
    }

    #[test]
    fn effective_schema_url_honors_explicit_schema_url() {
        let config = Config {
            schema_url: Some("https://example.com/schema.json".to_string()),
            ..Default::default()
        };
        assert_eq!(
            config.effective_schema_url(),
            "https://example.com/schema.json"
        );
    }

    #[test]
    fn effective_schema_url_honors_explicit_default_schema_url() {
        let config = Config {
            api_url: "https://staging.api.hubstaff.com/v2".to_string(),
            schema_url: Some(default_schema_url()),
            ..Default::default()
        };
        assert_eq!(
            config.effective_schema_url(),
            "https://api.hubstaff.com/v2/docs"
        );
    }

    #[test]
    fn config_roundtrip_preserves_explicit_default_schema_url() {
        let config = Config {
            api_url: "https://staging.api.hubstaff.com/v2".to_string(),
            schema_url: Some(default_schema_url()),
            ..Default::default()
        };
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(toml_str.contains("schema_url = \"https://api.hubstaff.com/v2/docs\""));

        let parsed: Config = toml::from_str(&toml_str).unwrap();
        assert_eq!(parsed.schema_url, Some(default_schema_url()));
        assert_eq!(
            parsed.effective_schema_url(),
            "https://api.hubstaff.com/v2/docs"
        );
    }
}
