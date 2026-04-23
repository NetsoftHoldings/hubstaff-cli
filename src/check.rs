use crate::auth;
use crate::client::HubstaffClient;
use crate::config::Config;
use crate::error::CliError;
use crate::schema::{ApiSchema, SchemaCacheMeta};
use crate::time::now_secs;
use std::collections::HashMap;
use std::fs;
use std::process;

const STALE_SCHEMA_DAYS: u64 = 30;
const TOKEN_NEAR_EXPIRY_SECS: u64 = 300;
const SECS_PER_DAY: u64 = 86_400;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    Ok,
    Warn,
    Fail,
    Skip,
}

impl Status {
    const fn marker(self) -> &'static str {
        match self {
            Self::Ok => "OK  ",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
            Self::Skip => "SKIP",
        }
    }
}

#[derive(Debug, Default)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: Option<String>,
    pub remediation: Option<String>,
    pub notes: Vec<String>,
}

#[derive(Debug)]
struct Summary {
    ok: usize,
    warn: usize,
    fail: usize,
    skip: usize,
}

pub fn run() {
    let checks = collect_checks();
    emit(&checks);

    if checks.iter().any(|check| check.status == Status::Fail) {
        process::exit(1);
    }
}

fn collect_checks() -> Vec<Check> {
    let mut checks: Vec<Check> = Vec::new();

    checks.push(check_cli_version());

    let (mut config, config_ok, config_error_detail) = match Config::load() {
        Ok(cfg) => {
            let path = Config::config_path();
            let detail = if path.exists() {
                format!("{}", path.display())
            } else {
                format!("{} (not present; using defaults)", path.display())
            };
            checks.push(Check {
                name: "Config file",
                status: Status::Ok,
                detail: Some(detail),
                ..Default::default()
            });
            (cfg, true, None)
        }
        Err(e) => {
            let path = Config::config_path();
            let detail = e.to_string();
            checks.push(Check {
                name: "Config file",
                status: Status::Fail,
                detail: Some(detail.clone()),
                remediation: Some(format!("fix TOML at {} or delete to reset", path.display())),
                ..Default::default()
            });
            (Config::default(), false, Some(detail))
        }
    };

    checks.push(check_config_dir_perms());

    let env_api_token = std::env::var("HUBSTAFF_API_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());

    if config_ok {
        let has_stored_access = config.auth.access_token.is_some();
        let has_stored_refresh = config.auth.refresh_token.is_some();
        let has_stored_auth = has_stored_access || has_stored_refresh;
        let creds_ok = env_api_token.is_some() || has_stored_auth;

        checks.push(check_credentials(&config, env_api_token.is_some()));
        checks.push(check_env_shadowing(
            env_api_token.is_some(),
            has_stored_auth,
        ));
        checks.push(check_token_validity(
            &mut config,
            env_api_token.is_some(),
            creds_ok,
        ));

        let api_ok = probe_and_record_api(&mut checks, creds_ok, config.clone());
        probe_and_record_organization(&mut checks, creds_ok, api_ok, config.clone());
    } else {
        let detail = config_error_detail
            .as_deref()
            .unwrap_or("unknown config error");
        checks.push(skipped_due_to_config_load("Credentials", detail));
        checks.push(skipped_due_to_config_load("Env token shadowing", detail));
        checks.push(skipped_due_to_config_load("Token validity", detail));
        checks.push(skipped_due_to_config_load("API reachability", detail));
        checks.push(skipped_due_to_config_load("Organization access", detail));
    }

    record_schema_cache(&mut checks, &config);

    checks
}

fn skipped_due_to_config_load(name: &'static str, config_error: &str) -> Check {
    Check {
        name,
        status: Status::Skip,
        detail: Some(format!("config failed to load: {config_error}")),
        ..Default::default()
    }
}

fn check_cli_version() -> Check {
    Check {
        name: "CLI version",
        status: Status::Ok,
        detail: Some(format!("hubstaff {}", env!("CARGO_PKG_VERSION"))),
        ..Default::default()
    }
}

fn check_config_dir_perms() -> Check {
    let dir = Config::config_dir();
    if !dir.exists() {
        return Check {
            name: "Config dir perms",
            status: Status::Skip,
            detail: Some(format!("{} does not exist yet", dir.display())),
            ..Default::default()
        };
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match fs::metadata(&dir) {
            Ok(meta) => {
                let mode = meta.permissions().mode() & 0o777;
                if mode == 0o700 {
                    Check {
                        name: "Config dir perms",
                        status: Status::Ok,
                        detail: Some(format!("{} (0o{mode:o})", dir.display())),
                        ..Default::default()
                    }
                } else {
                    Check {
                        name: "Config dir perms",
                        status: Status::Warn,
                        detail: Some(format!("{} is 0o{mode:o}, expected 0o700", dir.display())),
                        remediation: Some(format!("chmod 700 {}", dir.display())),
                        ..Default::default()
                    }
                }
            }
            Err(e) => Check {
                name: "Config dir perms",
                status: Status::Warn,
                detail: Some(format!("stat failed: {e}")),
                ..Default::default()
            },
        }
    }

    #[cfg(not(unix))]
    {
        Check {
            name: "Config dir perms",
            status: Status::Skip,
            detail: Some("non-unix platform".to_string()),
            ..Default::default()
        }
    }
}

fn check_credentials(config: &Config, env_token_present: bool) -> Check {
    if env_token_present {
        return Check {
            name: "Credentials",
            status: Status::Ok,
            detail: Some("HUBSTAFF_API_TOKEN env var".to_string()),
            ..Default::default()
        };
    }

    let has_stored = config.auth.access_token.is_some() || config.auth.refresh_token.is_some();
    if has_stored {
        return Check {
            name: "Credentials",
            status: Status::Ok,
            detail: Some("PAT session".to_string()),
            ..Default::default()
        };
    }

    Check {
        name: "Credentials",
        status: Status::Fail,
        detail: Some(
            "no HUBSTAFF_API_TOKEN env var and no stored access/refresh token".to_string(),
        ),
        remediation: Some(
            "run 'hubstaff config set-pat <TOKEN>' or set HUBSTAFF_API_TOKEN".to_string(),
        ),
        ..Default::default()
    }
}

fn check_env_shadowing(env_token_present: bool, has_stored_access: bool) -> Check {
    if env_token_present && has_stored_access {
        Check {
            name: "Env token shadowing",
            status: Status::Warn,
            detail: Some("HUBSTAFF_API_TOKEN overrides your stored token".to_string()),
            remediation: Some("unset HUBSTAFF_API_TOKEN to use the stored token".to_string()),
            ..Default::default()
        }
    } else {
        Check {
            name: "Env token shadowing",
            status: Status::Ok,
            ..Default::default()
        }
    }
}

fn check_token_validity(config: &mut Config, env_token_present: bool, creds_ok: bool) -> Check {
    if !creds_ok {
        return token_validity_no_credentials();
    }
    if env_token_present {
        return Check {
            name: "Token validity",
            status: Status::Skip,
            detail: Some("using HUBSTAFF_API_TOKEN (no expiry tracked)".to_string()),
            ..Default::default()
        };
    }
    if config.auth.access_token.is_none() {
        if config.auth.refresh_token.is_none() {
            return token_validity_no_credentials();
        }
        return token_validity_after_refresh(
            config,
            "access token missing; refreshed successfully",
        );
    }

    match classify_expiry(config.auth.expires_at, now_secs()) {
        ExpiryClassification::Missing => Check {
            name: "Token validity",
            status: Status::Fail,
            detail: Some("stored token has no expires_at".to_string()),
            remediation: Some("run 'hubstaff config set-pat <TOKEN>'".to_string()),
            ..Default::default()
        },
        ExpiryClassification::Fresh { remaining_secs } => Check {
            name: "Token validity",
            status: Status::Ok,
            detail: Some(format!("{} remaining", format_duration(remaining_secs))),
            ..Default::default()
        },
        ExpiryClassification::NearExpiry { .. } => {
            if config.auth.refresh_token.is_none() {
                return token_validity_near_expiry_without_refresh();
            }
            token_validity_after_refresh(config, "near-expiry token refreshed successfully")
        }
        ExpiryClassification::Expired => {
            if config.auth.refresh_token.is_none() {
                return token_validity_expired_without_refresh();
            }
            token_validity_after_refresh(config, "expired token refreshed successfully")
        }
    }
}

fn token_validity_no_credentials() -> Check {
    Check {
        name: "Token validity",
        status: Status::Skip,
        detail: Some("no credentials".to_string()),
        ..Default::default()
    }
}

fn token_validity_near_expiry_without_refresh() -> Check {
    Check {
        name: "Token validity",
        status: Status::Warn,
        detail: Some("token near expiry and no refresh_token available".to_string()),
        remediation: Some("run 'hubstaff config set-pat <TOKEN>' before token expires".to_string()),
        ..Default::default()
    }
}

fn token_validity_expired_without_refresh() -> Check {
    Check {
        name: "Token validity",
        status: Status::Fail,
        detail: Some("token expired and no refresh_token available".to_string()),
        remediation: Some("run 'hubstaff config set-pat <TOKEN>'".to_string()),
        ..Default::default()
    }
}

fn token_validity_after_refresh(config: &mut Config, success_detail: &'static str) -> Check {
    match auth::refresh_token(config) {
        Ok(()) => Check {
            name: "Token validity",
            status: Status::Ok,
            detail: Some(success_detail.to_string()),
            ..Default::default()
        },
        Err(e) => {
            let remediation = match &e {
                CliError::Network(_) => "auth service unavailable; retry 'hubstaff check' shortly",
                _ => "run 'hubstaff config set-pat <TOKEN>'",
            };
            Check {
                name: "Token validity",
                status: Status::Fail,
                detail: Some(format!("refresh failed: {e}")),
                remediation: Some(remediation.to_string()),
                ..Default::default()
            }
        }
    }
}

fn probe_and_record_api(checks: &mut Vec<Check>, creds_ok: bool, config: Config) -> bool {
    if !creds_ok {
        checks.push(Check {
            name: "API reachability",
            status: Status::Skip,
            detail: Some("no credentials".to_string()),
            ..Default::default()
        });
        return false;
    }

    let api_url = config.api_url.clone();
    let users_me_url = format!("{api_url}/users/me");
    match HubstaffClient::new(config).and_then(|mut client| client.probe_users_me()) {
        Ok(rtt_ms) => {
            checks.push(Check {
                name: "API reachability",
                status: Status::Ok,
                detail: Some(format!("{users_me_url} OK in {rtt_ms}ms")),
                ..Default::default()
            });
            true
        }
        Err(e) => {
            let remediation = match &e {
                CliError::Network(_) => Some("check internet connection and api_url".to_string()),
                CliError::Auth(_) => Some(
                    "run 'hubstaff config set-pat <TOKEN>' (or check HUBSTAFF_API_TOKEN)"
                        .to_string(),
                ),
                _ => None,
            };
            checks.push(Check {
                name: "API reachability",
                status: Status::Fail,
                detail: Some(e.to_string()),
                remediation,
                ..Default::default()
            });
            false
        }
    }
}

fn probe_and_record_organization(
    checks: &mut Vec<Check>,
    creds_ok: bool,
    api_ok: bool,
    config: Config,
) {
    if !creds_ok {
        checks.push(Check {
            name: "Organization access",
            status: Status::Skip,
            detail: Some("no credentials".to_string()),
            ..Default::default()
        });
        return;
    }
    if !api_ok {
        checks.push(Check {
            name: "Organization access",
            status: Status::Skip,
            detail: Some("api unreachable".to_string()),
            ..Default::default()
        });
        return;
    }
    let Some(org) = config.organization else {
        checks.push(Check {
            name: "Organization access",
            status: Status::Skip,
            detail: Some("no default organization configured".to_string()),
            ..Default::default()
        });
        return;
    };

    let path = format!("/organizations/{org}");
    match HubstaffClient::new(config)
        .and_then(|mut client| client.request_json("GET", &path, &HashMap::new(), None))
    {
        Ok(_) => checks.push(Check {
            name: "Organization access",
            status: Status::Ok,
            detail: Some(format!("organization {org} reachable")),
            ..Default::default()
        }),
        Err(e) => checks.push(Check {
            name: "Organization access",
            status: Status::Fail,
            detail: Some(e.to_string()),
            remediation: Some(
                "verify organization id via 'hubstaff config show' or pass --organization"
                    .to_string(),
            ),
            ..Default::default()
        }),
    }
}

fn record_schema_cache(checks: &mut Vec<Check>, config: &Config) -> bool {
    match ApiSchema::load_cache_only() {
        Ok(schema) => {
            let ops = schema.operations().len();
            let meta = schema.cache_meta_ref();
            let age_days = meta
                .and_then(|meta| meta.fetched_at)
                .map(|fetched| now_secs().saturating_sub(fetched) / SECS_PER_DAY);

            let notes = schema_cache_notes(ops, config, meta);
            let (status, remediation) = match age_days {
                Some(age) if age > STALE_SCHEMA_DAYS => (
                    Status::Warn,
                    Some("delete the schema cache directory to force refetch".to_string()),
                ),
                _ => (Status::Ok, None),
            };

            checks.push(Check {
                name: "Schema cache",
                status,
                detail: None,
                remediation,
                notes,
            });
            true
        }
        Err(e) => {
            checks.push(Check {
                name: "Schema cache",
                status: Status::Fail,
                detail: Some(e.to_string()),
                remediation: Some("delete the schema cache directory to force refetch".to_string()),
                ..Default::default()
            });
            false
        }
    }
}

fn schema_cache_notes(ops: usize, config: &Config, meta: Option<&SchemaCacheMeta>) -> Vec<String> {
    let mut notes = Vec::with_capacity(7);
    notes.push(format!("operations = {ops}"));
    notes.push(format!("url = {}", config.effective_schema_url()));
    notes.push(match meta.and_then(|meta| meta.fetched_at) {
        Some(fetched) => format!("fetched_at = {fetched}"),
        None => "fetched_at = age unknown".to_string(),
    });
    if let Some(etag) = meta.and_then(|meta| meta.etag.as_deref()) {
        notes.push(format!("etag = {etag}"));
    }
    notes.push(format!("docs = {}", Config::schema_docs_path().display()));
    notes.push(format!("meta = {}", Config::schema_meta_path().display()));
    notes.push(format!(
        "index = {}",
        Config::schema_command_index_path().display()
    ));
    notes
}

// --- pure helpers (unit-testable) ---

#[derive(Debug, PartialEq, Eq)]
enum ExpiryClassification {
    Missing,
    Fresh { remaining_secs: u64 },
    NearExpiry { remaining_secs: u64 },
    Expired,
}

fn classify_expiry(expires_at: Option<u64>, now_secs: u64) -> ExpiryClassification {
    let Some(expires_at) = expires_at else {
        return ExpiryClassification::Missing;
    };
    if expires_at <= now_secs {
        return ExpiryClassification::Expired;
    }
    let remaining_secs = expires_at - now_secs;
    if remaining_secs <= TOKEN_NEAR_EXPIRY_SECS {
        ExpiryClassification::NearExpiry { remaining_secs }
    } else {
        ExpiryClassification::Fresh { remaining_secs }
    }
}

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    } else {
        format!("{}d {}h", secs / 86_400, (secs % 86_400) / 3600)
    }
}

fn emit(checks: &[Check]) {
    let summary = summarize(checks);

    let name_width = checks
        .iter()
        .map(|check| check.name.len())
        .max()
        .unwrap_or(0);

    for check in checks {
        let detail = check.detail.as_deref().unwrap_or("");
        println!(
            "{:<name_width$}  {}  {detail}",
            check.name,
            check.status.marker(),
        );
        if let Some(remediation) = &check.remediation
            && matches!(check.status, Status::Fail | Status::Warn)
        {
            println!("{:<name_width$}  ->  {remediation}", "");
        }
        for note in &check.notes {
            println!("{:<name_width$}        {note}", "");
        }
    }

    println!(
        "\nsummary: {} OK, {} WARN, {} FAIL, {} SKIP",
        summary.ok, summary.warn, summary.fail, summary.skip
    );
}

fn summarize(checks: &[Check]) -> Summary {
    let mut summary = Summary {
        ok: 0,
        warn: 0,
        fail: 0,
        skip: 0,
    };
    for check in checks {
        match check.status {
            Status::Ok => summary.ok += 1,
            Status::Warn => summary.warn += 1,
            Status::Fail => summary.fail += 1,
            Status::Skip => summary.skip += 1,
        }
    }
    summary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_expiry_missing_when_none() {
        assert_eq!(classify_expiry(None, 1_000), ExpiryClassification::Missing);
    }

    #[test]
    fn classify_expiry_expired_when_past() {
        assert_eq!(
            classify_expiry(Some(990), 1_000),
            ExpiryClassification::Expired
        );
    }

    #[test]
    fn classify_expiry_expired_when_equal_to_now() {
        assert_eq!(
            classify_expiry(Some(1_000), 1_000),
            ExpiryClassification::Expired
        );
    }

    #[test]
    fn classify_expiry_near_when_within_skew() {
        match classify_expiry(Some(1_060), 1_000) {
            ExpiryClassification::NearExpiry { remaining_secs } => {
                assert_eq!(remaining_secs, 60);
                assert!(remaining_secs <= TOKEN_NEAR_EXPIRY_SECS);
            }
            other => panic!("expected NearExpiry, got {other:?}"),
        }
    }

    #[test]
    fn classify_expiry_fresh_when_far_in_future() {
        match classify_expiry(Some(1_000 + 3_600), 1_000) {
            ExpiryClassification::Fresh { remaining_secs } => {
                assert!(remaining_secs > TOKEN_NEAR_EXPIRY_SECS);
            }
            other => panic!("expected Fresh, got {other:?}"),
        }
    }

    #[test]
    fn format_duration_ranges() {
        assert_eq!(format_duration(45), "45s");
        assert_eq!(format_duration(90), "1m");
        assert_eq!(format_duration(3700), "1h 1m");
        assert_eq!(format_duration(90_000), "1d 1h");
    }

    #[test]
    fn summarize_counts_each_status() {
        let checks = vec![
            Check {
                name: "a",
                status: Status::Ok,
                ..Default::default()
            },
            Check {
                name: "b",
                status: Status::Warn,
                ..Default::default()
            },
            Check {
                name: "c",
                status: Status::Fail,
                ..Default::default()
            },
            Check {
                name: "d",
                status: Status::Skip,
                ..Default::default()
            },
            Check {
                name: "e",
                status: Status::Ok,
                ..Default::default()
            },
        ];
        let summary = summarize(&checks);
        assert_eq!(summary.ok, 2);
        assert_eq!(summary.warn, 1);
        assert_eq!(summary.fail, 1);
        assert_eq!(summary.skip, 1);
    }

    fn config_with_tokens() -> Config {
        Config {
            auth: crate::config::AuthConfig {
                access_token: Some("stored_access".to_string()),
                refresh_token: Some("stored_refresh".to_string()),
                expires_at: Some(now_secs().saturating_add(3600)),
            },
            ..Default::default()
        }
    }

    #[test]
    fn check_credentials_ok_env_token_wins() {
        let config = Config::default();
        let check = check_credentials(&config, true);
        assert_eq!(check.status, Status::Ok);
        assert_eq!(check.detail.as_deref(), Some("HUBSTAFF_API_TOKEN env var"));
    }

    #[test]
    fn check_credentials_ok_pat_session_with_stored_tokens() {
        let config = config_with_tokens();
        let check = check_credentials(&config, false);
        assert_eq!(check.status, Status::Ok);
        assert_eq!(check.detail.as_deref(), Some("PAT session"));
    }

    #[test]
    fn check_credentials_fail_without_any_stored_or_env_token() {
        let config = Config::default();
        let check = check_credentials(&config, false);
        assert_eq!(check.status, Status::Fail);
        assert!(
            check
                .detail
                .as_deref()
                .unwrap_or_default()
                .contains("no stored access/refresh token")
        );
    }

    #[test]
    fn token_validity_missing_expires_at_is_fail() {
        let mut config = Config::default();
        config.auth.access_token = Some("access".to_string());
        config.auth.expires_at = None;

        let check = check_token_validity(&mut config, false, true);
        assert_eq!(check.status, Status::Fail);
        assert_eq!(
            check.detail.as_deref(),
            Some("stored token has no expires_at")
        );
    }

    #[test]
    fn token_validity_near_expiry_without_refresh_token_is_warn() {
        let mut config = Config::default();
        config.auth.access_token = Some("access".to_string());
        config.auth.expires_at = Some(now_secs().saturating_add(120));

        let check = check_token_validity(&mut config, false, true);
        assert_eq!(check.status, Status::Warn);
        assert_eq!(
            check.detail.as_deref(),
            Some("token near expiry and no refresh_token available")
        );
    }

    #[test]
    fn token_validity_expired_without_refresh_token_is_fail() {
        let mut config = Config::default();
        config.auth.access_token = Some("access".to_string());
        config.auth.expires_at = Some(now_secs().saturating_sub(120));

        let check = check_token_validity(&mut config, false, true);
        assert_eq!(check.status, Status::Fail);
        assert_eq!(
            check.detail.as_deref(),
            Some("token expired and no refresh_token available")
        );
    }

    #[test]
    fn schema_cache_notes_includes_all_paths_and_ops() {
        let config = Config::default();
        let notes = schema_cache_notes(138, &config, None);
        assert!(notes.iter().any(|n| n == "operations = 138"));
        assert!(notes.iter().any(|n| n.starts_with("url = ")));
        assert!(notes.iter().any(|n| n == "fetched_at = age unknown"));
        assert!(notes.iter().any(|n| n.starts_with("docs = ")));
        assert!(notes.iter().any(|n| n.starts_with("meta = ")));
        assert!(notes.iter().any(|n| n.starts_with("index = ")));
        assert!(!notes.iter().any(|n| n.starts_with("etag = ")));
    }

    #[test]
    fn schema_cache_notes_includes_etag_and_fetched_at_timestamp() {
        let config = Config::default();
        let fetched = now_secs().saturating_sub(SECS_PER_DAY * 3);
        let meta = SchemaCacheMeta {
            etag: Some("W/\"abc\"".to_string()),
            fetched_at: Some(fetched),
            schema_hash: None,
            source_url: None,
        };
        let notes = schema_cache_notes(10, &config, Some(&meta));
        assert!(notes.iter().any(|n| n == "etag = W/\"abc\""));
        assert!(
            notes
                .iter()
                .any(|n| n == &format!("fetched_at = {fetched}"))
        );
    }
}
