use crate::auth::TokenSet;
use crate::config::Config;
use crate::error::CliError;

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
    println!("Config reset to defaults. Auth tokens left intact (use 'hubstaff logout' to clear).");
    Ok(())
}

/// Exchange a personal access token (which is a refresh token) for access + refresh tokens.
pub fn set_pat(pat: &str) -> Result<(), CliError> {
    let config = Config::load()?;
    println!("Exchanging personal access token...");

    let client = reqwest::blocking::Client::new();
    let resp = client
        .post(format!("{}/access_tokens", config.auth_url))
        .form(&[("grant_type", "refresh_token"), ("refresh_token", pat)])
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

    let tokens = TokenSet::from_json(&body)?;

    let mut config = Config::load()?;
    config.store_tokens(tokens);
    config.save()?;

    println!("Authenticated. Access token saved (expires in ~24h, auto-refreshes).");
    Ok(())
}

/// Interactive setup for OAuth app credentials. Persists them to `config.toml`.
pub fn setup_oauth() -> Result<(), CliError> {
    use std::io::{self, Write};

    println!("Hubstaff OAuth App Setup");
    println!("========================");
    println!();
    println!("1. Go to https://developer.hubstaff.com");
    println!("2. Navigate to OAuth Apps > Create an app");
    println!("3. Set redirect URI to: http://127.0.0.1:19876/callback");
    println!(
        "   This must match exactly, and port 19876 must be available when you run 'hubstaff login'."
    );
    println!("4. Copy the Client ID and Client Secret below.");
    println!();

    print!("Client ID: ");
    io::stdout().flush().unwrap();
    let mut client_id = String::new();
    io::stdin()
        .read_line(&mut client_id)
        .map_err(|e| CliError::Config(format!("failed to read input: {e}")))?;
    let client_id = client_id.trim();

    if client_id.is_empty() {
        return Err(CliError::Config("client ID cannot be empty".into()));
    }

    let client_secret = rpassword::prompt_password("Client Secret: ")
        .map_err(|e| CliError::Config(format!("failed to read input: {e}")))?;
    let client_secret = client_secret.trim();

    if client_secret.is_empty() {
        return Err(CliError::Config("client secret cannot be empty".into()));
    }

    let mut config = Config::load()?;
    config.oauth.client_id = Some(client_id.to_string());
    config.oauth.client_secret = Some(client_secret.to_string());
    config.save()?;

    println!();
    println!("You can now run: hubstaff login");
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
    if config.oauth.is_empty() {
        println!("[oauth] not configured");
    } else {
        println!("[oauth]");
        if config.oauth.client_id.is_some() {
            println!("client_id = ****");
        }
        if config.oauth.client_secret.is_some() {
            println!("client_secret = ****");
        }
    }
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
