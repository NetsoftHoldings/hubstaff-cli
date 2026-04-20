// Integration tests — run the CLI binary and verify output

mod helpers {
    use std::path::PathBuf;

    pub fn cli_bin() -> PathBuf {
        PathBuf::from(env!("CARGO_BIN_EXE_hubstaff"))
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

        let meta = format!(
            "fetched_at = \"2099-01-01T00:00:00Z\"\netag = \"test\"\nsource_url = \"{source_url}\"\n"
        );

        fs::write(schema_dir.join("docs.json"), docs).expect("failed to write docs cache");
        fs::write(schema_dir.join("meta.toml"), meta).expect("failed to write meta cache");
    }
}

#[test]
fn cli_version() {
    let (stdout, _, code) = helpers::run(&["--version"], "/tmp/hcli-test-ver");
    assert_eq!(code, 0);
    assert!(stdout.contains("hubstaff"));
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-ver");
}

#[test]
fn cli_help_lists_hardcoded_commands() {
    let (stdout, _, code) = helpers::run(&["--help"], "/tmp/hcli-test-help");
    assert_eq!(code, 0);
    for cmd in ["schema", "config", "login", "logout"] {
        assert!(stdout.contains(cmd), "missing command: {cmd}");
    }
    assert!(!stdout.contains("api"));
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-help");
}

#[test]
fn cli_config_set_and_show() {
    let dir = "/tmp/hcli-test-cfg";
    let _ = std::fs::remove_dir_all(dir);

    // Set organization
    let (stdout, _, code) = helpers::run(&["config", "set", "organization", "42"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("set organization = 42"));

    // Show config
    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("organization = 42"));
    assert!(stdout.contains("api_url = https://api.hubstaff.com/v2"));
    assert!(stdout.contains("format = compact"));

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

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cli_config_set_invalid_key() {
    let dir = "/tmp/hcli-test-cfg-inv";
    let _ = std::fs::remove_dir_all(dir);

    let (_, stderr, code) = helpers::run(&["config", "set", "bad_key", "val"], dir);
    assert_eq!(code, 3);
    assert!(stderr.contains("unknown config key"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cli_config_set_invalid_format() {
    let dir = "/tmp/hcli-test-cfg-fmt";
    let _ = std::fs::remove_dir_all(dir);

    let (_, stderr, code) = helpers::run(&["config", "set", "format", "xml"], dir);
    assert_eq!(code, 3);
    assert!(stderr.contains("compact"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cli_config_explicit_default_schema_url_is_preserved_with_custom_api_url() {
    let dir = "/tmp/hcli-test-schema-url-explicit-default";
    let _ = std::fs::remove_dir_all(dir);

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

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cli_logout_clears_tokens() {
    let dir = "/tmp/hcli-test-logout";
    let _ = std::fs::remove_dir_all(dir);

    helpers::run(&["config", "set", "token", "mytoken"], dir);
    let (stdout, _, code) = helpers::run(&["logout"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("Logged out"));

    let (stdout, _, _) = helpers::run(&["config", "show"], dir);
    assert!(stdout.contains("not configured"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dynamic_projects_list_uses_schema_mapping() {
    let dir = "/tmp/hcli-test-dynamic-projects";
    let _ = std::fs::remove_dir_all(dir);
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
        std::path::Path::new(dir)
            .join("hubstaff")
            .join("schema")
            .join("v2")
            .join("command_index.json")
            .exists()
    );

    mock.assert();
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dynamic_nonstandard_action_uses_literal_segment() {
    let dir = "/tmp/hcli-test-dynamic-nonstandard";
    let _ = std::fs::remove_dir_all(dir);
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
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dynamic_command_does_not_use_cache_when_source_url_mismatches_effective_schema_url() {
    let dir = "/tmp/hcli-test-schema-cache-mismatch";
    let _ = std::fs::remove_dir_all(dir);
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

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dynamic_command_uses_cache_when_source_url_matches_effective_schema_url() {
    use std::path::Path;

    let dir = "/tmp/hcli-test-schema-cache-match";
    let _ = std::fs::remove_dir_all(dir);
    helpers::seed_schema_cache(dir);

    let schema_source = "http://127.0.0.1:1/docs";
    let schema_dir = Path::new(dir).join("hubstaff").join("schema").join("v2");
    let meta = format!(
        "fetched_at = \"2099-01-01T00:00:00Z\"\netag = \"test\"\nsource_url = \"{schema_source}\"\n"
    );
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
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn schema_show_is_read_only_for_cache_meta() {
    use std::path::Path;

    let dir = "/tmp/hcli-test-schema-show-readonly";
    let _ = std::fs::remove_dir_all(dir);
    helpers::seed_schema_cache(dir);

    let meta_path = Path::new(dir)
        .join("hubstaff")
        .join("schema")
        .join("v2")
        .join("meta.toml");
    let before = std::fs::read_to_string(&meta_path).expect("failed to read meta before show");

    let (_, stderr, code) = helpers::run(&["schema", "show"], dir);
    assert_eq!(code, 0, "stderr={stderr}");

    let after = std::fs::read_to_string(&meta_path).expect("failed to read meta after show");
    assert_eq!(before, after, "schema show should not mutate cache meta");

    let _ = std::fs::remove_dir_all(dir);
}
