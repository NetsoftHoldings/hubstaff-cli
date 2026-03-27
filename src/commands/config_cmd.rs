use crate::config::Config;
use crate::error::CliError;

pub fn set(key: &str, value: &str) -> Result<(), CliError> {
    let mut config = Config::load()?;

    match key {
        "org" => {
            let id: u64 = value
                .parse()
                .map_err(|_| CliError::Config(format!("invalid org id: {value}")))?;
            config.org = Some(id);
        }
        "api_url" => {
            config.api_url = value.to_string();
        }
        "auth_url" => {
            config.auth_url = value.to_string();
        }
        "token" => {
            config.auth.access_token = Some(value.to_string());
        }
        "format" => {
            if value != "compact" && value != "json" {
                return Err(CliError::Config(
                    "format must be 'compact' or 'json'".to_string(),
                ));
            }
            config.format = value.to_string();
        }
        _ => {
            return Err(CliError::Config(format!(
                "unknown config key: {key}. Valid keys: org, api_url, auth_url, token, format"
            )));
        }
    }

    config.save()?;
    let display_value = if key == "token" { "****" } else { value };
    println!("set {key} = {display_value}");
    Ok(())
}

/// Exchange a personal access token (which is a refresh token) for access + refresh tokens.
pub fn set_pat(pat: &str) -> Result<(), CliError> {
    let config = Config::load()?;
    println!("Exchanging personal access token...");

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("{}/access_tokens", config.auth_url))
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", pat),
        ])
        .send()
        .map_err(|e| CliError::Network(format!("token exchange failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let body = resp.text().unwrap_or_default();
        return Err(CliError::Auth(format!(
            "personal token exchange failed ({status}): {body}"
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| CliError::Auth(format!("failed to parse token response: {e}")))?;

    let access_token = body["access_token"]
        .as_str()
        .ok_or_else(|| CliError::Auth("missing access_token in response".to_string()))?;

    let refresh_token = body["refresh_token"]
        .as_str()
        .ok_or_else(|| CliError::Auth("missing refresh_token in response".to_string()))?;

    let mut config = Config::load()?;
    config.auth.access_token = Some(access_token.to_string());
    config.auth.refresh_token = Some(refresh_token.to_string());
    if let Some(expires_in) = body["expires_in"].as_i64() {
        let expires_at = chrono::Utc::now() + chrono::Duration::seconds(expires_in);
        config.auth.expires_at = Some(expires_at.to_rfc3339());
    }
    config.save()?;

    println!("Authenticated. Access token saved (expires in ~24h, auto-refreshes).");
    Ok(())
}

pub fn show() -> Result<(), CliError> {
    let config = Config::load()?;
    println!("api_url = {}", config.api_url);
    if config.auth_url != "https://account.hubstaff.com" {
        println!("auth_url = {}", config.auth_url);
    }
    if let Some(org) = config.org {
        println!("org = {org}");
    }
    println!("format = {}", config.format);
    println!();
    if config.auth.access_token.is_some() {
        println!("[auth]");
        println!("access_token = ****");
        if config.auth.refresh_token.is_some() {
            println!("refresh_token = ****");
        }
        if let Some(ref exp) = config.auth.expires_at {
            println!("expires_at = {exp}");
        }
    } else {
        println!("[auth] not configured");
    }
    Ok(())
}
