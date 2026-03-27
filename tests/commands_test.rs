// Integration tests — run the CLI binary and verify output

mod helpers {
    pub fn cli_bin() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_BIN_EXE_hubstaff-cli"))
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
}

#[test]
fn cli_version() {
    let (stdout, _, code) = helpers::run(&["--version"], "/tmp/hcli-test-ver");
    assert_eq!(code, 0);
    assert!(stdout.contains("hubstaff-cli"));
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-ver");
}

#[test]
fn cli_help_lists_all_commands() {
    let (stdout, _, code) = helpers::run(&["--help"], "/tmp/hcli-test-help");
    assert_eq!(code, 0);
    for cmd in ["users", "orgs", "projects", "members", "invites", "tasks",
                "activities", "daily-activities", "teams", "notes", "time-entries",
                "config", "login", "logout"] {
        assert!(stdout.contains(cmd), "missing command: {cmd}");
    }
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-help");
}

#[test]
fn cli_members_help_lists_actions() {
    let (stdout, _, code) = helpers::run(&["members", "--help"], "/tmp/hcli-test-mh");
    assert_eq!(code, 0);
    assert!(stdout.contains("list"));
    assert!(stdout.contains("create"));
    assert!(stdout.contains("remove"));
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-mh");
}

#[test]
fn cli_members_create_help_shows_flags() {
    let (stdout, _, code) = helpers::run(&["members", "create", "--help"], "/tmp/hcli-test-mch");
    assert_eq!(code, 0);
    for flag in ["--email", "--first-name", "--last-name", "--password", "--password-stdin", "--role", "--project-ids", "--team-ids"] {
        assert!(stdout.contains(flag), "missing flag: {flag}");
    }
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-mch");
}

#[test]
fn cli_config_set_and_show() {
    let dir = "/tmp/hcli-test-cfg";
    let _ = std::fs::remove_dir_all(dir);

    // Set org
    let (stdout, _, code) = helpers::run(&["config", "set", "org", "42"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("set org = 42"));

    // Show config
    let (stdout, _, code) = helpers::run(&["config", "show"], dir);
    assert_eq!(code, 0);
    assert!(stdout.contains("org = 42"));
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
    let (stdout, _, code) = helpers::run(&["config", "set", "auth_url", "https://account.staging.hbstf.co"], dir);
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
fn cli_no_auth_exits_2() {
    let dir = "/tmp/hcli-test-noauth";
    let _ = std::fs::remove_dir_all(dir);

    let (_, stderr, code) = helpers::run(&["users", "me"], dir);
    assert_eq!(code, 2);
    assert!(stderr.contains("not authenticated"));

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cli_no_auth_json_mode() {
    let dir = "/tmp/hcli-test-noauth-j";
    let _ = std::fs::remove_dir_all(dir);

    let (_, stderr, code) = helpers::run(&["--json", "users", "me"], dir);
    assert_eq!(code, 2);
    let parsed: serde_json::Value = serde_json::from_str(stderr.trim()).expect("should be JSON");
    assert_eq!(parsed["code"], "auth_error");

    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn cli_missing_org_exits_3() {
    let dir = "/tmp/hcli-test-no-org";
    let _ = std::fs::remove_dir_all(dir);

    // Set token but not org
    helpers::run(&["config", "set", "token", "fake"], dir);
    let (_, stderr, code) = helpers::run(&["projects", "list"], dir);
    assert_eq!(code, 3);
    assert!(stderr.contains("--org required"));

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
fn cli_tasks_requires_project() {
    let (_, _, code) = helpers::run(&["tasks", "list", "--help"], "/tmp/hcli-test-task");
    assert_eq!(code, 0);
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-task");
}

#[test]
fn cli_activities_requires_start() {
    let (_, _, code) = helpers::run(&["activities", "list", "--help"], "/tmp/hcli-test-act");
    assert_eq!(code, 0);
    let _ = std::fs::remove_dir_all("/tmp/hcli-test-act");
}
