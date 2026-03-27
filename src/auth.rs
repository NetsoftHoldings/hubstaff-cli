use crate::config::Config;
use crate::error::CliError;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::Rng;
use sha2::{Digest, Sha256};
use url::Url;

const CLIENT_ID: &str = "REDACTED_CLIENT_ID";
const CLIENT_SECRET: &str = "REDACTED_CLIENT_SECRET";
const PORT_START: u16 = 19876;
const PORT_END: u16 = 19886;
const CALLBACK_TIMEOUT_SECS: u64 = 120;

pub fn login() -> Result<(), CliError> {
    let config = Config::load()?;
    let auth_base = &config.auth_url;

    let (verifier, challenge) = generate_pkce();
    let state = generate_state();
    let (server, port) = start_callback_server()?;
    let redirect_uri = format!("http://localhost:{port}/callback");

    let auth_url = build_auth_url(auth_base, &challenge, &redirect_uri, &state);

    println!("Opening browser for authentication...");
    println!("If the browser doesn't open, visit:\n{auth_url}");

    if open::that(&auth_url).is_err() {
        eprintln!("warning: could not open browser automatically");
    }

    let code = wait_for_callback(server, &state)?;
    let tokens = exchange_code(auth_base, &code, &verifier, &redirect_uri)?;

    let mut config = Config::load()?;
    config.auth.access_token = Some(tokens.access_token);
    config.auth.refresh_token = Some(tokens.refresh_token);
    config.auth.expires_at = Some(tokens.expires_at);
    config.save()?;

    println!("Authentication successful. Tokens saved.");
    Ok(())
}

pub fn logout() -> Result<(), CliError> {
    let mut config = Config::load()?;
    config.auth.access_token = None;
    config.auth.refresh_token = None;
    config.auth.expires_at = None;
    config.save()?;
    println!("Logged out. Tokens cleared.");
    Ok(())
}

pub fn refresh_token(config: &mut Config) -> Result<(), CliError> {
    let auth_base = config.auth_url.clone();
    let refresh = config
        .auth
        .refresh_token
        .as_ref()
        .ok_or_else(|| CliError::Auth("session expired. Run 'hubstaff-cli login'".to_string()))?
        .clone();

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("{auth_base}/access_tokens"))
        .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", &refresh),
        ])
        .send()
        .map_err(|e| CliError::Network(format!("token refresh failed: {e}")))?;

    if !resp.status().is_success() {
        return Err(CliError::Auth(
            "session expired. Run 'hubstaff-cli login'".to_string(),
        ));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| CliError::Auth(format!("failed to parse refresh response: {e}")))?;

    let new_access = body["access_token"]
        .as_str()
        .ok_or_else(|| CliError::Auth("missing access_token in refresh response".to_string()))?
        .to_string();
    let new_refresh = body["refresh_token"]
        .as_str()
        .ok_or_else(|| CliError::Auth("missing refresh_token in refresh response".to_string()))?
        .to_string();

    config.auth.access_token = Some(new_access);
    config.auth.refresh_token = Some(new_refresh);
    if let Some(expires_in) = body["expires_in"].as_i64() {
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in);
        config.auth.expires_at = Some(expires_at.to_rfc3339());
    }
    config.save()?;

    Ok(())
}

struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_at: String,
}

fn generate_pkce() -> (String, String) {
    let mut rng = rand::rng();
    let verifier_bytes: Vec<u8> = (0..32).map(|_| rng.random::<u8>()).collect();
    let verifier = URL_SAFE_NO_PAD.encode(&verifier_bytes);

    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let challenge = URL_SAFE_NO_PAD.encode(hasher.finalize());

    (verifier, challenge)
}

fn generate_state() -> String {
    let mut rng = rand::rng();
    let state_bytes: Vec<u8> = (0..16).map(|_| rng.random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&state_bytes)
}

fn generate_nonce() -> String {
    let mut rng = rand::rng();
    let nonce_bytes: Vec<u8> = (0..16).map(|_| rng.random::<u8>()).collect();
    URL_SAFE_NO_PAD.encode(&nonce_bytes)
}

fn build_auth_url(auth_base: &str, challenge: &str, redirect_uri: &str, state: &str) -> String {
    let nonce = generate_nonce();
    let mut url = Url::parse(&format!("{auth_base}/authorizations/new")).unwrap();
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", CLIENT_ID)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", "openid hubstaff:read hubstaff:write")
        .append_pair("code_challenge", challenge)
        .append_pair("code_challenge_method", "S256")
        .append_pair("state", state)
        .append_pair("nonce", &nonce);
    url.to_string()
}

fn start_callback_server() -> Result<(tiny_http::Server, u16), CliError> {
    for port in PORT_START..=PORT_END {
        match tiny_http::Server::http(format!("127.0.0.1:{port}")) {
            Ok(server) => return Ok((server, port)),
            Err(_) => continue,
        }
    }
    Err(CliError::Auth(format!(
        "could not bind to any port in range {PORT_START}-{PORT_END}"
    )))
}

fn wait_for_callback(server: tiny_http::Server, expected_state: &str) -> Result<String, CliError> {
    let timeout = std::time::Duration::from_secs(CALLBACK_TIMEOUT_SECS);
    let start = std::time::Instant::now();

    loop {
        if start.elapsed() > timeout {
            return Err(CliError::Auth(
                "authentication timed out. Try 'hubstaff-cli login' again".to_string(),
            ));
        }

        match server.recv_timeout(std::time::Duration::from_secs(1)) {
            Ok(Some(request)) => {
                let url_str = format!("http://localhost{}", request.url());
                let url = Url::parse(&url_str)
                    .map_err(|e| CliError::Auth(format!("failed to parse callback URL: {e}")))?;

                let code = url
                    .query_pairs()
                    .find(|(k, _)| k == "code")
                    .map(|(_, v)| v.to_string());

                let error = url
                    .query_pairs()
                    .find(|(k, _)| k == "error")
                    .map(|(_, v)| v.to_string());

                if let Some(err) = error {
                    let html = "<html><body><h2>Authentication failed</h2>\
                                <p>Please try again.</p></body></html>";
                    let response = tiny_http::Response::from_string(html)
                        .with_header(
                            "Content-Type: text/html"
                                .parse::<tiny_http::Header>()
                                .unwrap(),
                        );
                    let _ = request.respond(response);
                    return Err(CliError::Auth(format!("authentication denied: {err}")));
                }

                // Validate state parameter (CSRF protection)
                let callback_state = url
                    .query_pairs()
                    .find(|(k, _)| k == "state")
                    .map(|(_, v)| v.to_string());

                if callback_state.as_deref() != Some(expected_state) {
                    let html = "<html><body><h2>Authentication failed</h2>\
                                <p>Invalid state parameter.</p></body></html>";
                    let response = tiny_http::Response::from_string(html)
                        .with_header(
                            "Content-Type: text/html"
                                .parse::<tiny_http::Header>()
                                .unwrap(),
                        );
                    let _ = request.respond(response);
                    return Err(CliError::Auth(
                        "invalid state parameter in callback (possible CSRF)".to_string(),
                    ));
                }

                if let Some(code) = code {
                    let html = "<html><body><h2>Authentication successful</h2>\
                                <p>You can close this tab.</p></body></html>";
                    let response = tiny_http::Response::from_string(html).with_header(
                        "Content-Type: text/html"
                            .parse::<tiny_http::Header>()
                            .unwrap(),
                    );
                    let _ = request.respond(response);
                    return Ok(code);
                }

                // Not the callback we expected, respond with 404
                let response = tiny_http::Response::from_string("not found")
                    .with_status_code(tiny_http::StatusCode(404));
                let _ = request.respond(response);
            }
            Ok(None) => continue,
            Err(_) => continue,
        }
    }
}

fn exchange_code(
    auth_base: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, CliError> {
    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("{auth_base}/access_tokens"))
        .basic_auth(CLIENT_ID, Some(CLIENT_SECRET))
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier),
        ])
        .send()
        .map_err(|e| CliError::Network(format!("token exchange failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();
        return Err(CliError::Auth(format!(
            "token exchange failed ({status}): {body}"
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| CliError::Auth(format!("failed to parse token response: {e}")))?;

    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| CliError::Auth("missing access_token in response".to_string()))?
        .to_string();

    let refresh_token = body["refresh_token"]
        .as_str()
        .ok_or_else(|| CliError::Auth("missing refresh_token in response".to_string()))?
        .to_string();

    let expires_at = if let Some(expires_in) = body["expires_in"].as_i64() {
        let at = chrono::Utc::now() + chrono::Duration::seconds(expires_in);
        at.to_rfc3339()
    } else {
        String::new()
    };

    Ok(TokenResponse {
        access_token,
        refresh_token,
        expires_at,
    })
}

pub fn generate_password() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%"
        .chars()
        .collect();
    (0..16).map(|_| chars[rng.random_range(0..chars.len())]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_verifier_and_challenge_are_different() {
        let (verifier, challenge) = generate_pkce();
        assert_ne!(verifier, challenge);
    }

    #[test]
    fn pkce_verifier_is_base64url() {
        let (verifier, _) = generate_pkce();
        assert!(!verifier.is_empty());
        assert!(!verifier.contains('+'));
        assert!(!verifier.contains('/'));
        assert!(!verifier.contains('='));
    }

    #[test]
    fn pkce_challenge_is_sha256_of_verifier() {
        let (verifier, challenge) = generate_pkce();
        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let expected = URL_SAFE_NO_PAD.encode(hasher.finalize());
        assert_eq!(challenge, expected);
    }

    #[test]
    fn pkce_generates_unique_pairs() {
        let (v1, _) = generate_pkce();
        let (v2, _) = generate_pkce();
        assert_ne!(v1, v2);
    }

    #[test]
    fn state_is_non_empty() {
        let state = generate_state();
        assert!(!state.is_empty());
    }

    #[test]
    fn state_generates_unique_values() {
        let s1 = generate_state();
        let s2 = generate_state();
        assert_ne!(s1, s2);
    }

    #[test]
    fn nonce_is_non_empty() {
        let nonce = generate_nonce();
        assert!(!nonce.is_empty());
    }

    #[test]
    fn nonce_generates_unique_values() {
        let n1 = generate_nonce();
        let n2 = generate_nonce();
        assert_ne!(n1, n2);
    }

    #[test]
    fn auth_url_contains_required_params() {
        let url = build_auth_url(
            "https://account.hubstaff.com",
            "test_challenge",
            "http://localhost:19876/callback",
            "test_state",
        );
        assert!(url.contains("response_type=code"));
        assert!(url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("scope="));
        assert!(url.contains("code_challenge=test_challenge"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=test_state"));
        assert!(url.contains("nonce="));
    }

    #[test]
    fn auth_url_uses_custom_base() {
        let url = build_auth_url(
            "https://account.staging.hbstf.co",
            "ch",
            "http://localhost:19876/callback",
            "st",
        );
        assert!(url.starts_with("https://account.staging.hbstf.co/authorizations/new"));
    }

    #[test]
    fn password_generation_length() {
        let pw = generate_password();
        assert_eq!(pw.len(), 16);
    }

    #[test]
    fn password_generation_has_valid_chars() {
        let valid = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%";
        for _ in 0..10 {
            let pw = generate_password();
            for c in pw.chars() {
                assert!(valid.contains(c), "unexpected char: {c}");
            }
        }
    }

    #[test]
    fn password_generation_is_random() {
        let pw1 = generate_password();
        let pw2 = generate_password();
        assert_ne!(pw1, pw2);
    }

    #[test]
    fn callback_server_binds_to_port() {
        let result = start_callback_server();
        assert!(result.is_ok());
        let (_, port) = result.unwrap();
        assert!(port >= PORT_START && port <= PORT_END);
    }
}
