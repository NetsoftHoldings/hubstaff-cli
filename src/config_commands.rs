use crate::auth::TokenSet;
use crate::config::Config;
use crate::error::CliError;
use std::time::Duration;

pub fn set(key: &str, value: &str) -> Result<(), CliError> {
    let mut config = Config::load()?;

    match key {
        "organization" => {
            let id: u64 = value
                .parse()
                .map_err(|_| CliError::Config(format!("invalid organization id: {value}")))?;
            config.organization = Some(id);
        }
        "api_url" => {
            config.api_url = value.to_string();
        }
        "auth_url" => {
            config.auth_url = value.to_string();
        }
        "schema_url" => {
            config.schema_url = Some(value.to_string());
        }
        "token" => {
            config.auth.access_token = Some(value.to_string());
            config.auth.refresh_token = None;
            config.auth.expires_at = None;
        }
        "format" => {
            if value != "json" && value != "pretty" {
                return Err(CliError::Config(
                    "format must be 'json' or 'pretty'".to_string(),
                ));
            }
            config.format = value.to_string();
        }
        _ => {
            return Err(CliError::Config(format!(
                "unknown config key: {key}. Valid keys: organization, api_url, auth_url, schema_url, token, format"
            )));
        }
    }

    config.save()?;
    let display_value = if key == "token" { "****" } else { value };
    println!("set {key} = {display_value}");
    Ok(())
}

pub fn unset(key: &str) -> Result<(), CliError> {
    let mut config = Config::load()?;
    config.unset(key)?;
    config.save()?;
    println!("unset {key}");
    Ok(())
}

pub fn reset() -> Result<(), CliError> {
    let mut config = Config::load()?;
    config.reset();
    config.save()?;
    println!("Config reset to defaults.");
    Ok(())
}

/// Exchange a personal access token (which is a refresh token) for access + refresh tokens.
pub fn set_pat(pat: &str) -> Result<(), CliError> {
    let config = Config::load()?;
    println!("Exchanging personal access token...");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(crate::HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|_| CliError::Network("couldn't build HTTP client".to_string()))?;
    let resp = client
        .post(format!(
            "{}/access_tokens",
            config.auth_url.trim_end_matches('/')
        ))
        .form(&[("grant_type", "refresh_token"), ("refresh_token", pat)])
        .send()
        .map_err(|e| CliError::Network(format!("token exchange failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let is_transient = resp.status().is_server_error() || status == 429 || status == 408;
        let body = resp.text().unwrap_or_default();
        if is_transient {
            return Err(CliError::Network(format!(
                "personal token exchange unavailable ({status}): {body}"
            )));
        }
        return Err(CliError::Auth(format!(
            "personal token exchange failed ({status}): {body}"
        )));
    }

    let body: serde_json::Value = resp
        .json()
        .map_err(|e| CliError::Auth(format!("failed to parse token response: {e}")))?;

    let tokens = TokenSet::from_json(&body)?;

    let mut config = Config::load()?;
    config.store_tokens(tokens);
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
    if let Some(organization_id) = config.organization {
        println!("organization = {organization_id}");
    }
    if let Some(schema_url) = &config.schema_url {
        println!("schema_url = {schema_url}");
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
