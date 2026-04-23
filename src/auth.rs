use crate::config::Config;
use crate::error::CliError;
use std::time::Duration;

const REFRESH_NETWORK_ERROR: &str =
    "Couldn't refresh your session right now. Check your internet connection and try again.";
const REFRESH_SERVICE_ERROR: &str =
    "Couldn't refresh your session right now. The auth service is unavailable; retry shortly.";
const REFRESH_AUTH_ERROR: &str =
    "Couldn't refresh your session. Please run 'hubstaff config set-pat <TOKEN>' again.";

pub struct TokenSet {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: Option<u64>,
}

impl TokenSet {
    pub fn from_json(body: &serde_json::Value) -> Result<Self, CliError> {
        let access_token = body["access_token"]
            .as_str()
            .ok_or_else(|| CliError::Auth("missing access_token in response".into()))?
            .to_string();
        let refresh_token = body["refresh_token"]
            .as_str()
            .ok_or_else(|| CliError::Auth("missing refresh_token in response".into()))?
            .to_string();
        let expires_at = body["expires_in"]
            .as_u64()
            .map(|secs| crate::time::now_secs().saturating_add(secs));
        Ok(Self {
            access_token,
            refresh_token,
            expires_at,
        })
    }
}

pub fn refresh_token(config: &mut Config) -> Result<(), CliError> {
    let auth_base = config.auth_url.trim_end_matches('/').to_string();
    let refresh = config
        .auth
        .refresh_token
        .as_ref()
        .ok_or_else(|| {
            CliError::Auth(
                "session expired. Run 'hubstaff config set-pat <TOKEN>' to re-authenticate"
                    .to_string(),
            )
        })?
        .clone();

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(crate::HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|_| CliError::Network(REFRESH_NETWORK_ERROR.to_string()))?;
    let resp = client
        .post(format!("{auth_base}/access_tokens"))
        .form(&[("grant_type", "refresh_token"), ("refresh_token", &refresh)])
        .send()
        .map_err(|_| CliError::Network(REFRESH_NETWORK_ERROR.to_string()))?;

    let status = resp.status();
    if !status.is_success() {
        let code = status.as_u16();
        if status.is_server_error() || code == 429 || code == 408 {
            return Err(CliError::Network(REFRESH_SERVICE_ERROR.to_string()));
        }
        return Err(CliError::Auth(REFRESH_AUTH_ERROR.to_string()));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|_| CliError::Auth(REFRESH_AUTH_ERROR.to_string()))?;

    let tokens =
        TokenSet::from_json(&body).map_err(|_| CliError::Auth(REFRESH_AUTH_ERROR.to_string()))?;
    config.store_tokens(tokens);
    config.save()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AuthConfig, Config};

    fn refresh_test_config(auth_url: String) -> Config {
        Config {
            auth_url,
            auth: AuthConfig {
                access_token: Some("old_access".to_string()),
                refresh_token: Some("old_refresh".to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn refresh_token_returns_human_network_error_on_connect_failure() {
        let mut config = refresh_test_config("http://127.0.0.1:9".to_string());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Network(msg) => assert_eq!(msg, REFRESH_NETWORK_ERROR),
            _ => panic!("expected network error"),
        }
    }

    #[test]
    fn refresh_token_returns_human_auth_error_on_non_success_status() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("POST", "/access_tokens")
            .with_status(401)
            .with_body(r#"{"error":"invalid_grant"}"#)
            .create();

        let mut config = refresh_test_config(server.url());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Auth(msg) => assert_eq!(msg, REFRESH_AUTH_ERROR),
            _ => panic!("expected auth error"),
        }
    }

    #[test]
    fn refresh_token_returns_network_error_on_5xx() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("POST", "/access_tokens")
            .with_status(503)
            .with_body("gateway down")
            .create();

        let mut config = refresh_test_config(server.url());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Network(msg) => assert_eq!(msg, REFRESH_SERVICE_ERROR),
            other => panic!("expected network error, got {other:?}"),
        }
    }

    #[test]
    fn refresh_token_returns_network_error_on_429() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("POST", "/access_tokens")
            .with_status(429)
            .with_body("slow down")
            .create();

        let mut config = refresh_test_config(server.url());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Network(msg) => assert_eq!(msg, REFRESH_SERVICE_ERROR),
            other => panic!("expected network error, got {other:?}"),
        }
    }

    #[test]
    fn refresh_token_returns_network_error_on_408() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("POST", "/access_tokens")
            .with_status(408)
            .with_body("request timeout")
            .create();

        let mut config = refresh_test_config(server.url());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Network(msg) => assert_eq!(msg, REFRESH_SERVICE_ERROR),
            other => panic!("expected network error, got {other:?}"),
        }
    }

    #[test]
    fn refresh_token_keeps_auth_error_on_4xx() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("POST", "/access_tokens")
            .with_status(403)
            .with_body(r#"{"error":"forbidden"}"#)
            .create();

        let mut config = refresh_test_config(server.url());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Auth(msg) => assert_eq!(msg, REFRESH_AUTH_ERROR),
            other => panic!("expected auth error, got {other:?}"),
        }
    }

    #[test]
    fn refresh_token_returns_human_auth_error_on_invalid_json() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("POST", "/access_tokens")
            .with_status(200)
            .with_body("not json")
            .create();

        let mut config = refresh_test_config(server.url());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Auth(msg) => assert_eq!(msg, REFRESH_AUTH_ERROR),
            _ => panic!("expected auth error"),
        }
    }

    #[test]
    fn refresh_token_returns_human_auth_error_on_missing_tokens() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("POST", "/access_tokens")
            .with_status(200)
            .with_body(r#"{"access_token":"new_access"}"#)
            .create();

        let mut config = refresh_test_config(server.url());
        let err = refresh_token(&mut config).unwrap_err();
        match err {
            CliError::Auth(msg) => assert_eq!(msg, REFRESH_AUTH_ERROR),
            _ => panic!("expected auth error"),
        }
    }

    #[test]
    fn refresh_trims_trailing_slash_from_auth_url() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/access_tokens")
            .with_status(401)
            .create();

        let mut config = refresh_test_config(format!("{}/", server.url()));
        let _ = refresh_token(&mut config);

        mock.assert();
    }

    #[test]
    fn refresh_sends_no_authorization_header() {
        use mockito::Matcher;

        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/access_tokens")
            .match_header("authorization", Matcher::Missing)
            .match_body(Matcher::AllOf(vec![
                Matcher::UrlEncoded("grant_type".into(), "refresh_token".into()),
                Matcher::UrlEncoded("refresh_token".into(), "old_refresh".into()),
            ]))
            .with_status(401)
            .create();

        let mut config = refresh_test_config(server.url());
        let _ = refresh_token(&mut config);

        mock.assert();
    }
}
