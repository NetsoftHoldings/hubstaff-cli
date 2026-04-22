// Integration tests — run the CLI binary and verify output

mod helpers {
    use std::path::PathBuf;

    pub fn cli_bin() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_hubstaff"))
    }

    pub fn temp_xdg() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("failed to create temp XDG dir")
    }

    pub fn run(args: &[&str], xdg_dir: &str) -> (String, String, i32) {
        let mut cmd = std::process::Command::new(cli_bin());
        cmd.args(args);
        cmd.env("XDG_CONFIG_HOME", xdg_dir);
        cmd.env_remove("HUBSTAFF_API_TOKEN");
        let output = cmd.output().expect("failed to execute CLI");
        (
            String::from_utf8_lossy(&output.stdout).to_string(),
            String::from_utf8_lossy(&output.stderr).to_string(),
            output.status.code().unwrap_or(-1),
        )
    }

    pub fn seed_schema_cache(xdg_dir: &str) {
        seed_schema_cache_with_source_url(xdg_dir, "https://api.hubstaff.com/v2/docs");
    }

    pub fn seed_schema_cache_with_source_url(xdg_dir: &str, source_url: &str) {
        use std::fs;
        use std::path::Path;

        let schema_dir = Path::new(xdg_dir)
            .join("hubstaff")
            .join("schema")
            .join("v2");
        fs::create_dir_all(&schema_dir).expect("failed to create schema dir");

        let docs = r#"{
  "swagger": "2.0",
  "paths": {
    "/v2/organizations/{organization_id}/projects": {
      "parameters": [
        {"name": "organization_id", "in": "path", "required": true, "type": "integer"}
      ],
      "get": {
        "operationId": "getProjects",
        "summary": "List organization projects",
        "parameters": [
          {"name": "page_limit", "in": "query", "required": false, "type": "integer"}
        ],
        "responses": {"200": {"description": "ok"}}
      }
    },
    "/v2/teams/{team_id}/update_members": {
      "parameters": [
        {"name": "team_id", "in": "path", "required": true, "type": "integer"}
      ],
      "put": {
        "operationId": "putTeamsUpdateMembers",
        "summary": "Update team members",
        "responses": {"200": {"description": "ok"}}
      }
    },
    "/v2/users/me": {
      "get": {
        "operationId": "getUsersMe",
        "summary": "Get current user",
        "responses": {"200": {"description": "ok"}}
      }
    }
  }
}"#;

        let meta =
            format!("fetched_at = 4070908800\netag = \"test\"\nsource_url = \"{source_url}\"\n");

        fs::write(schema_dir.join("docs.json"), docs).expect("failed to write docs cache");
        fs::write(schema_dir.join("meta.toml"), meta).expect("failed to write meta cache");
    }
}

#[test]
fn cli_version() {
    let xdg = helpers::temp_xdg();
    let (stdout, _, code) = helpers::run(&["--version"], xdg.path().to_str().unwrap());
    assert_eq!(code, 0);
    assert!(stdout.contains("hubstaff"));
}

#[test]
fn cli_help_lists_hardcoded_commands() {
    let xdg = helpers::temp_xdg();
    let (stdout, _, code) = helpers::run(&["--help"], xdg.path().to_str().unwrap());
    assert_eq!(code, 0);
    for cmd in ["schema", "config", "login", "logout"] {
        assert!(stdout.contains(cmd), "missing command: {cmd}");
    }
    assert!(!stdout.contains("api"));
}

#[test]
fn cli_doctor_subcommand_is_not_supported() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, stderr, code) = helpers::run(&["doctor"], dir);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("doctor"),
        "expected error to mention doctor subcommand, got: {stderr}"
    );
}

#[test]
fn cli_diagnose_subcommand_is_not_supported() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, stderr, code) = helpers::run(&["diagnose"], dir);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("diagnose"),
        "expected error to mention diagnose subcommand, got: {stderr}"
    );
}

#[test]
fn cli_config_set_and_show() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    // Set organization
    let (stdout, _, code) = helpers::run(&["config", "set", "organization", "42"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("set organization = 42"));

    // Show config
    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("organization = 42"));
    assert!(stdout.contains("api_url = https://api.hubstaff.com/v2"));
    assert!(stdout.contains("format = json"));

    // Set token — should mask
    let (stdout, _, code) = helpers::run(&["config", "set", "token", "secret123"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("****"));
    assert!(!stdout.contains("secret123"));

    // Show — token masked
    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("access_token = ****"));

    // Set custom auth_url
    let (stdout, _, code) = helpers::run(
        &[
            "config",
            "set",
            "auth_url",
            "https://account.staging.hbstf.co",
        ],
        dir,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("auth_url"));

    // Show — custom auth_url visible
    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("auth_url = https://account.staging.hbstf.co"));
}

#[test]
fn cli_config_set_invalid_key() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, stderr, code) = helpers::run(&["config", "set", "bad_key", "val"], dir);
    assert_eq!(code, 3);
    assert!(stderr.contains("unknown config key"));
}

#[test]
fn cli_config_set_invalid_format() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, stderr, code) = helpers::run(&["config", "set", "format", "xml"], dir);
    assert_eq!(code, 3);
    assert!(stderr.contains("'json' or 'pretty'"));
}

#[test]
fn cli_config_explicit_default_schema_url_is_preserved_with_custom_api_url() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (stdout, _, code) = helpers::run(
        &[
            "config",
            "set",
            "api_url",
            "https://staging.api.hubstaff.com/v2",
        ],
        dir,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("set api_url = https://staging.api.hubstaff.com/v2"));

    let (stdout, _, code) = helpers::run(
        &[
            "config",
            "set",
            "schema_url",
            "https://api.hubstaff.com/v2/docs",
        ],
        dir,
    );
    assert_eq!(code, 0);
    assert!(stdout.contains("set schema_url = https://api.hubstaff.com/v2/docs"));

    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("api_url = https://staging.api.hubstaff.com/v2"));
    assert!(stdout.contains("schema_url = https://api.hubstaff.com/v2/docs"));
    assert!(
        !stdout.contains("schema_url = https://staging.api.hubstaff.com/v2/docs"),
        "schema_url should stay on the explicit override"
    );
}

#[test]
fn cli_config_set_token_clears_oauth_refresh_state() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let cfg_dir = xdg.path().join("hubstaff");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("config.toml"),
        "[auth]\naccess_token = \"old_access\"\nrefresh_token = \"old_refresh\"\nexpires_at = 4070908800\n",
    )
    .unwrap();

    let (_, _, code) = helpers::run(&["config", "set", "token", "raw_pat"], dir);
    assert_eq!(code, 0);

    let content = std::fs::read_to_string(cfg_dir.join("config.toml")).unwrap();
    assert!(content.contains("access_token"));
    assert!(
        !content.contains("refresh_token"),
        "refresh_token should be cleared when setting raw token; got: {content}"
    );
    assert!(
        !content.contains("expires_at"),
        "expires_at should be cleared when setting raw token; got: {content}"
    );
}

#[test]
fn cli_config_unset_restores_api_url_default() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, _, code) = helpers::run(
        &[
            "config",
            "set",
            "api_url",
            "https://staging.api.hubstaff.com/v2",
        ],
        dir,
    );
    assert_eq!(code, 0);

    let (stdout, _, code) = helpers::run(&["config", "unset", "api_url"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("unset api_url"));

    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("api_url = https://api.hubstaff.com/v2"));
}

#[test]
fn cli_config_unset_organization_clears_option() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, _, code) = helpers::run(&["config", "set", "organization", "42"], dir);
    assert_eq!(code, 0);

    let (stdout, _, code) = helpers::run(&["config", "unset", "organization"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("unset organization"));

    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(!stdout.contains("organization = "));
}

#[test]
fn cli_config_unset_token_rejected() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, stderr, code) = helpers::run(&["config", "unset", "token"], dir);
    assert_eq!(code, 3);
    assert!(stderr.contains("hubstaff logout"));
}

#[test]
fn cli_config_unset_unknown_key() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let (_, stderr, code) = helpers::run(&["config", "unset", "bogus"], dir);
    assert_eq!(code, 3);
    assert!(stderr.contains("unknown config key"));
}

#[test]
fn cli_config_reset_restores_defaults_and_preserves_auth() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    helpers::run(&["config", "set", "organization", "42"], dir);
    helpers::run(
        &["config", "set", "api_url", "https://custom.example/v2"],
        dir,
    );
    helpers::run(&["config", "set", "auth_url", "https://auth.example"], dir);
    helpers::run(
        &[
            "config",
            "set",
            "schema_url",
            "https://custom.example/v2/docs",
        ],
        dir,
    );
    helpers::run(&["config", "set", "format", "json"], dir);
    helpers::run(&["config", "set", "token", "secret"], dir);

    let (stdout, _, code) = helpers::run(&["config", "reset"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("Config reset to defaults"));

    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("api_url = https://api.hubstaff.com/v2"));
    assert!(!stdout.contains("auth_url = "));
    assert!(!stdout.contains("organization = "));
    assert!(!stdout.contains("schema_url = "));
    assert!(stdout.contains("format = json"));
    assert!(stdout.contains("access_token = ****"));
}

#[test]
fn cli_logout_clears_tokens() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    helpers::run(&["config", "set", "token", "mytoken"], dir);
    let (stdout, _, code) = helpers::run(&["logout"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("Logged out"));

    let (stdout, _, _) = helpers::run(&["config", "show"], dir);
    assert!(stdout.contains("not configured"));
}

#[test]
fn cli_check_skips_config_dependent_checks_when_config_is_invalid() {
    fn find_check_line<'a>(stdout: &'a str, name: &str) -> &'a str {
        stdout
            .lines()
            .find(|line| line.starts_with(name))
            .unwrap_or_else(|| panic!("missing check line: {name}\nstdout:\n{stdout}"))
    }

    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let cfg_dir = xdg.path().join("hubstaff");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("config.toml"), "bad = [").unwrap();

    let (stdout, stderr, code) = helpers::run(&["check"], dir);
    assert_eq!(code, 1, "stderr={stderr}");

    let config_file = find_check_line(&stdout, "Config file");
    assert!(config_file.contains("FAIL"));
    assert!(
        config_file.contains("config parse error"),
        "config file check should surface parse error, got: {config_file}"
    );

    for name in [
        "Credentials",
        "Env token shadowing",
        "Token validity",
        "API reachability",
        "Organization access",
    ] {
        let check_line = find_check_line(&stdout, name);
        assert!(
            check_line.contains("SKIP"),
            "expected {name} to be skipped, got: {check_line}"
        );
        assert!(
            check_line.contains("config failed to load: config parse error"),
            "expected config failure detail for {name}, got: {check_line}"
        );
    }
}

#[test]
fn cli_check_treats_refresh_only_session_as_credentials() {
    fn find_check_line<'a>(stdout: &'a str, name: &str) -> &'a str {
        stdout
            .lines()
            .find(|line| line.starts_with(name))
            .unwrap_or_else(|| panic!("missing check line: {name}\nstdout:\n{stdout}"))
    }

    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();

    let cfg_dir = xdg.path().join("hubstaff");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(
        cfg_dir.join("config.toml"),
        "api_url = \"http://127.0.0.1:9\"\nauth_url = \"http://127.0.0.1:9\"\n[auth]\nrefresh_token = \"refresh_only\"\n",
    )
    .unwrap();

    let (stdout, stderr, code) = helpers::run(&["check"], dir);
    assert_eq!(code, 1, "stderr={stderr}");

    let credentials = find_check_line(&stdout, "Credentials");
    assert!(credentials.contains("OK"));
    assert!(credentials.contains("PAT session"));

    let token_validity = find_check_line(&stdout, "Token validity");
    assert!(
        !token_validity.contains("SKIP"),
        "refresh-only sessions should run token validity checks"
    );
    assert!(
        !token_validity.contains("no credentials"),
        "refresh-only sessions should not be treated as missing credentials"
    );

    let api_reachability = find_check_line(&stdout, "API reachability");
    assert!(
        !api_reachability.contains("SKIP"),
        "refresh-only sessions should run API reachability checks"
    );
}

#[test]
fn dynamic_projects_list_uses_schema_mapping() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();
    let schema_source = "http://127.0.0.1:1/docs";
    helpers::seed_schema_cache_with_source_url(dir, schema_source);

    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/organizations/7/projects")
        .match_header("authorization", "Bearer test_token")
        .match_query(mockito::Matcher::UrlEncoded(
            "page_limit".into(),
            "10".into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"projects":[]}"#)
        .create();

    let api_url = server.url();
    let _ = helpers::run(&["config", "set", "api_url", &api_url], dir);
    let _ = helpers::run(&["config", "set", "schema_url", schema_source], dir);
    let _ = helpers::run(&["config", "set", "organization", "7"], dir);
    let _ = helpers::run(&["config", "set", "token", "test_token"], dir);

    let (stdout, stderr, code) = helpers::run(&["projects", "list", "--page_limit", "10"], dir);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("\"projects\":[]"));
    assert!(
        xdg.path()
            .join("hubstaff")
            .join("schema")
            .join("v2")
            .join("command_index.json")
            .exists()
    );

    mock.assert();
}

#[test]
fn dynamic_projects_list_prefers_global_organization_override() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();
    let schema_source = "http://127.0.0.1:1/docs";
    helpers::seed_schema_cache_with_source_url(dir, schema_source);

    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/organizations/9/projects")
        .match_header("authorization", "Bearer test_token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"projects":[{"id":1}]}"#)
        .create();

    let api_url = server.url();
    let _ = helpers::run(&["config", "set", "api_url", &api_url], dir);
    let _ = helpers::run(&["config", "set", "schema_url", schema_source], dir);
    let _ = helpers::run(&["config", "set", "organization", "7"], dir);
    let _ = helpers::run(&["config", "set", "token", "test_token"], dir);

    let (stdout, stderr, code) = helpers::run(&["--organization", "9", "projects", "list"], dir);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains(r#""projects":[{"id":1}]"#));

    mock.assert();
}

#[test]
fn dynamic_nonstandard_action_uses_literal_segment() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();
    let schema_source = "http://127.0.0.1:1/docs";
    helpers::seed_schema_cache_with_source_url(dir, schema_source);

    let mut server = mockito::Server::new();
    let mock = server
        .mock("PUT", "/teams/42/update_members")
        .match_header("authorization", "Bearer test_token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"ok":true}"#)
        .create();

    let api_url = server.url();
    let _ = helpers::run(&["config", "set", "api_url", &api_url], dir);
    let _ = helpers::run(&["config", "set", "schema_url", schema_source], dir);
    let _ = helpers::run(&["config", "set", "token", "test_token"], dir);

    let (stdout, stderr, code) = helpers::run(&["teams", "update_members", "42"], dir);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("\"ok\":true"));

    mock.assert();
}

#[test]
fn dynamic_command_does_not_use_cache_when_source_url_mismatches_effective_schema_url() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();
    helpers::seed_schema_cache(dir);

    let (stdout, _, code) = helpers::run(&["config", "set", "api_url", "http://127.0.0.1:1"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("set api_url = http://127.0.0.1:1"));

    let (_, stderr, code) = helpers::run(&["users", "me"], dir);
    assert_eq!(code, 4);
    assert!(
        stderr.contains("schema fetch failed"),
        "expected refresh failure, got stderr={stderr}"
    );
}

#[test]
fn dynamic_command_uses_cache_when_source_url_matches_effective_schema_url() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();
    helpers::seed_schema_cache(dir);

    let schema_source = "http://127.0.0.1:1/docs";
    let schema_dir = xdg.path().join("hubstaff").join("schema").join("v2");
    let meta =
        format!("fetched_at = 4070908800\netag = \"test\"\nsource_url = \"{schema_source}\"\n");
    std::fs::write(schema_dir.join("meta.toml"), meta).expect("failed to rewrite meta");

    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/users/me")
        .match_header("authorization", "Bearer test_token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"id":1}"#)
        .create();

    let api_url = server.url();
    let _ = helpers::run(&["config", "set", "api_url", &api_url], dir);
    let _ = helpers::run(&["config", "set", "schema_url", schema_source], dir);
    let _ = helpers::run(&["config", "set", "token", "test_token"], dir);

    let (stdout, stderr, code) = helpers::run(&["users", "me"], dir);
    assert_eq!(code, 0, "stderr={stderr}");
    assert!(stdout.contains("\"id\":1"));

    mock.assert();
}

#[test]
fn schema_show_is_read_only_for_cache_meta() {
    let xdg = helpers::temp_xdg();
    let dir = xdg.path().to_str().unwrap();
    helpers::seed_schema_cache(dir);

    let meta_path = xdg
        .path()
        .join("hubstaff")
        .join("schema")
        .join("v2")
        .join("meta.toml");
    let before = std::fs::read_to_string(&meta_path).expect("failed to read meta before show");

    let (_, stderr, code) = helpers::run(&["schema", "show"], dir);
    assert_eq!(code, 0, "stderr={stderr}");

    let after = std::fs::read_to_string(&meta_path).expect("failed to read meta after show");
    assert_eq!(before, after, "schema show should not mutate cache meta");
}
