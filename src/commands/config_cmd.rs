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

/// Interactive setup for OAuth app credentials. Writes to .env in the current directory.
pub fn setup_oauth() -> Result<(), CliError> {
    use std::io::{self, Write};

    println!("Hubstaff OAuth App Setup");
    println!("========================");
    println!();
    println!("1. Go to https://developer.hubstaff.com");
    println!("2. Navigate to OAuth Apps > Create an app");
    println!("3. Set redirect URI to: http://localhost:19876/callback");
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

    print!("Client Secret: ");
    io::stdout().flush().unwrap();
    let mut client_secret = String::new();
    io::stdin()
        .read_line(&mut client_secret)
        .map_err(|e| CliError::Config(format!("failed to read input: {e}")))?;
    let client_secret = client_secret.trim();

    if client_secret.is_empty() {
        return Err(CliError::Config("client secret cannot be empty".into()));
    }

    // Write to config dir .env file so it works from any directory
    let env_path = Config::config_dir().join(".env");
    let dir = Config::config_dir();
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
    }
    std::fs::write(
        &env_path,
        format!("HUBSTAFF_CLIENT_ID={client_id}\nHUBSTAFF_CLIENT_SECRET={client_secret}\n"),
    )?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&env_path, std::fs::Permissions::from_mode(0o600))?;
    }

    println!();
    println!("Saved to {}", env_path.display());
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
