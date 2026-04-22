use crate::auth;
use crate::config::Config;
use crate::error::CliError;
use reqwest::blocking::{Client, Response};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const REFRESH_SKEW_SECS: u64 = 120;

pub struct HubstaffClient {
    config: Config,
    http: Client,
    env_api_token: Option<String>,
}

#[derive(Clone, Copy)]
enum Method {
    Get,
    Post,
    Put,
    Delete,
    Patch,
}

impl HubstaffClient {
    pub fn new(config: Config) -> Result<Self, CliError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| CliError::Network(format!("failed to create HTTP client: {e}")))?;
        Ok(Self {
            config,
            http,
            env_api_token: Self::read_env_api_token(),
        })
    }

    fn read_env_api_token() -> Option<String> {
        std::env::var("HUBSTAFF_API_TOKEN")
            .ok()
            .filter(|token| !token.is_empty())
    }

    fn token(&self) -> Result<String, CliError> {
        if let Some(token) = &self.env_api_token {
            return Ok(token.clone());
        }
        self.config.get_token().ok_or_else(|| {
            CliError::Auth(
                "not authenticated. Run 'hubstaff login' or set HUBSTAFF_API_TOKEN".to_string(),
            )
        })
    }

    fn base_url(&self) -> &str {
        &self.config.api_url
    }

    fn should_refresh_proactively(&self, now_secs: u64) -> bool {
        if self.env_api_token.is_some() || self.config.auth.refresh_token.is_none() {
            return false;
        }

        let Some(expires_at) = self.config.auth.expires_at else {
            return false;
        };

        expires_at <= now_secs.saturating_add(REFRESH_SKEW_SECS)
    }

    fn ensure_fresh_token(&mut self) -> Result<(), CliError> {
        if self.should_refresh_proactively(crate::time::now_secs()) {
            auth::refresh_token(&mut self.config)?;
        }
        Ok(())
    }

    pub fn request_json(
        &mut self,
        method: &str,
        path: &str,
        params: &HashMap<String, String>,
        body: Option<&Value>,
    ) -> Result<Value, CliError> {
        let parsed_method = parse_method(method)?;
        self.request(parsed_method, path, params, body)
    }

    pub fn probe_users_me(&mut self) -> Result<u128, CliError> {
        let url = format!("{}/users/me", self.base_url().trim_end_matches('/'));
        let (resp, status, elapsed) =
            self.send_with_auth_retry(Method::Get, &url, &HashMap::new(), None)?;
        Self::parse_response(resp, status)?;
        Ok(elapsed.as_millis())
    }

    fn build_request(
        &self,
        method: Method,
        url: &str,
        params: &HashMap<String, String>,
        body: Option<&Value>,
        token: &str,
    ) -> reqwest::blocking::RequestBuilder {
        let builder = match method {
            Method::Get => self.http.get(url).query(params),
            Method::Post => self.http.post(url).query(params),
            Method::Put => self.http.put(url).query(params),
            Method::Delete => self.http.delete(url).query(params),
            Method::Patch => self.http.patch(url).query(params),
        };
        let builder = if let Some(body) = body {
            builder.json(body)
        } else {
            builder
        };
        builder.bearer_auth(token)
    }

    fn request(
        &mut self,
        method: Method,
        path: &str,
        params: &HashMap<String, String>,
        body: Option<&Value>,
    ) -> Result<Value, CliError> {
        let url = format!("{}{path}", self.base_url());
        let (resp, status, _) = self.send_with_auth_retry(method, &url, params, body)?;
        Self::parse_response(resp, status)
    }

    fn send_with_auth_retry(
        &mut self,
        method: Method,
        url: &str,
        params: &HashMap<String, String>,
        body: Option<&Value>,
    ) -> Result<(Response, u16, Duration), CliError> {
        self.ensure_fresh_token()?;
        let token = self.token()?;

        let started = Instant::now();
        let resp = self
            .build_request(method, url, params, body, &token)
            .send()?;
        let elapsed = started.elapsed();
        let status = resp.status().as_u16();

        if status != 401 {
            return Ok((resp, status, elapsed));
        }

        if self.env_api_token.is_some() {
            return Err(CliError::Auth(
                "invalid token. Check your HUBSTAFF_API_TOKEN".to_string(),
            ));
        }

        auth::refresh_token(&mut self.config)?;
        let new_token = self.token()?;

        let retry_started = Instant::now();
        let retry_resp = self
            .build_request(method, url, params, body, &new_token)
            .send()?;
        let retry_elapsed = retry_started.elapsed();
        let retry_status = retry_resp.status().as_u16();

        if retry_status == 401 {
            return Err(CliError::Auth(
                "session expired. Run 'hubstaff login'".to_string(),
            ));
        }

        Ok((retry_resp, retry_status, retry_elapsed))
    }

    fn parse_response(resp: Response, status: u16) -> Result<Value, CliError> {
        // Handle 204 No Content
        if status == 204 {
            return Ok(Value::Null);
        }

        // Rate limiting
        if status == 429 {
            let retry_after = resp
                .headers()
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown");
            return Err(CliError::Api {
                status,
                message: format!("rate limited. Retry after {retry_after}s"),
            });
        }

        let text = resp
            .text()
            .map_err(|e| CliError::Network(format!("failed to read response: {e}")))?;

        let body: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) if status >= 400 => {
                let preview = if text.len() > 200 {
                    &text[..200]
                } else {
                    &text
                };
                return Err(CliError::Api {
                    status,
                    message: format!("[{status}] {preview}"),
                });
            }
            Err(e) => return Err(CliError::from(e)),
        };

        if status >= 400 {
            let message = body["error"]
                .as_str()
                .unwrap_or("unknown API error")
                .to_string();
            return Err(CliError::Api { status, message });
        }

        Ok(body)
    }

    pub fn resolve_organization(&self, cli_organization: Option<u64>) -> Result<u64, CliError> {
        self.config.resolve_organization(cli_organization)
    }
}

fn parse_method(method: &str) -> Result<Method, CliError> {
    match method.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::Get),
        "POST" => Ok(Method::Post),
        "PUT" => Ok(Method::Put),
        "DELETE" => Ok(Method::Delete),
        "PATCH" => Ok(Method::Patch),
        _ => Err(CliError::Config(format!(
            "unsupported HTTP method: {method}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_with_env_token(
        config: Config,
        env_api_token: Option<String>,
    ) -> Result<HubstaffClient, CliError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| CliError::Network(format!("failed to create HTTP client: {e}")))?;
        Ok(HubstaffClient {
            config,
            http,
            env_api_token,
        })
    }

    fn test_config(server_url: &str) -> Config {
        Config {
            api_url: server_url.to_string(),
            auth: crate::config::AuthConfig {
                access_token: Some("test_token".to_string()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn test_client(config: Config) -> HubstaffClient {
        new_with_env_token(config, None).unwrap()
    }

    fn get_json(
        client: &mut HubstaffClient,
        path: &str,
        params: &HashMap<String, String>,
    ) -> Result<Value, CliError> {
        client.request_json("GET", path, params, None)
    }

    fn post_json(client: &mut HubstaffClient, path: &str, body: &Value) -> Result<Value, CliError> {
        client.request_json("POST", path, &HashMap::new(), Some(body))
    }

    fn put_json(client: &mut HubstaffClient, path: &str, body: &Value) -> Result<Value, CliError> {
        client.request_json("PUT", path, &HashMap::new(), Some(body))
    }

    fn delete_json(client: &mut HubstaffClient, path: &str) -> Result<Value, CliError> {
        client.request_json("DELETE", path, &HashMap::new(), None)
    }

    fn delete_json_with_body(
        client: &mut HubstaffClient,
        path: &str,
        body: &Value,
    ) -> Result<Value, CliError> {
        client.request_json("DELETE", path, &HashMap::new(), Some(body))
    }

    #[test]
    fn get_success() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/users/me")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"id":1,"name":"Test"}}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let result = get_json(&mut client, "/users/me", &HashMap::new()).unwrap();

        assert_eq!(result["user"]["id"], 1);
        assert_eq!(result["user"]["name"], "Test");
        mock.assert();
    }

    #[test]
    fn get_with_query_params() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/organizations/5/members")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "page_limit".into(),
                "10".into(),
            )]))
            .with_status(200)
            .with_body(r#"{"members":[]}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let mut params = HashMap::new();
        params.insert("page_limit".to_string(), "10".to_string());
        get_json(&mut client, "/organizations/5/members", &params).unwrap();

        mock.assert();
    }

    #[test]
    fn post_success() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/organizations/1/projects")
            .match_header("authorization", "Bearer test_token")
            .with_status(201)
            .with_body(r#"{"project":{"id":99,"name":"New"}}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let body = serde_json::json!({"name": "New"});
        let result = post_json(&mut client, "/organizations/1/projects", &body).unwrap();

        assert_eq!(result["project"]["id"], 99);
        mock.assert();
    }

    #[test]
    fn post_without_body_does_not_send_json_null() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/organizations/1/projects")
            .match_header("content-type", mockito::Matcher::Missing)
            .with_status(201)
            .with_body(r#"{"project":{"id":99,"name":"New"}}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let result = client
            .request_json("POST", "/organizations/1/projects", &HashMap::new(), None)
            .unwrap();

        assert_eq!(result["project"]["id"], 99);
        mock.assert();
    }

    #[test]
    fn post_with_explicit_null_body_sends_json_null() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/organizations/1/projects")
            .match_header(
                "content-type",
                mockito::Matcher::Regex("application/json".to_string()),
            )
            .match_body("null")
            .with_status(201)
            .with_body(r#"{"project":{"id":99,"name":"New"}}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let result = client
            .request_json(
                "POST",
                "/organizations/1/projects",
                &HashMap::new(),
                Some(&Value::Null),
            )
            .unwrap();

        assert_eq!(result["project"]["id"], 99);
        mock.assert();
    }

    #[test]
    fn put_success() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("PUT", "/organizations/1/update_members")
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let body = serde_json::json!({"members": [{"user_id": 1, "role": "remove"}]});
        let result = put_json(&mut client, "/organizations/1/update_members", &body).unwrap();

        assert_eq!(result["ok"], true);
        mock.assert();
    }

    #[test]
    fn patch_success_path_and_payload() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("PATCH", "/tasks/42")
            .match_header("authorization", "Bearer test_token")
            .match_header(
                "content-type",
                mockito::Matcher::Regex("application/json".to_string()),
            )
            .match_body(r#"{"status":"done"}"#)
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let body = serde_json::json!({"status": "done"});
        let result = client
            .request_json("PATCH", "/tasks/42", &HashMap::new(), Some(&body))
            .unwrap();

        assert_eq!(result["ok"], true);
        mock.assert();
    }

    #[test]
    fn post_with_query_params() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("POST", "/organizations/1/projects")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "include_archived".into(),
                "true".into(),
            )]))
            .with_status(201)
            .with_body(r#"{"project":{"id":99}}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let mut params = HashMap::new();
        params.insert("include_archived".to_string(), "true".to_string());
        let body = serde_json::json!({"name": "New"});
        let result = client
            .request_json("POST", "/organizations/1/projects", &params, Some(&body))
            .unwrap();

        assert_eq!(result["project"]["id"], 99);
        mock.assert();
    }

    #[test]
    fn put_with_query_params() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("PUT", "/organizations/1/update_members")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "notify".into(),
                "false".into(),
            )]))
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let mut params = HashMap::new();
        params.insert("notify".to_string(), "false".to_string());
        let body = serde_json::json!({"members": [{"user_id": 1, "role": "remove"}]});
        let result = client
            .request_json(
                "PUT",
                "/organizations/1/update_members",
                &params,
                Some(&body),
            )
            .unwrap();

        assert_eq!(result["ok"], true);
        mock.assert();
    }

    #[test]
    fn patch_with_query_params() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("PATCH", "/tasks/42")
            .match_query(mockito::Matcher::AllOf(vec![mockito::Matcher::UrlEncoded(
                "sync".into(),
                "1".into(),
            )]))
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let mut params = HashMap::new();
        params.insert("sync".to_string(), "1".to_string());
        let body = serde_json::json!({"status": "done"});
        let result = client
            .request_json("PATCH", "/tasks/42", &params, Some(&body))
            .unwrap();

        assert_eq!(result["ok"], true);
        mock.assert();
    }

    #[test]
    fn patch_without_body_does_not_send_json_null() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("PATCH", "/tasks/42")
            .match_header("content-type", mockito::Matcher::Missing)
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let result = client
            .request_json("PATCH", "/tasks/42", &HashMap::new(), None)
            .unwrap();

        assert_eq!(result["ok"], true);
        mock.assert();
    }

    #[test]
    fn delete_204_no_content() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("DELETE", "/invites/42")
            .with_status(204)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let result = delete_json(&mut client, "/invites/42").unwrap();

        assert!(result.is_null());
        mock.assert();
    }

    #[test]
    fn delete_with_body_sends_json() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("DELETE", "/invites/42")
            .match_header(
                "content-type",
                mockito::Matcher::Regex("application/json".to_string()),
            )
            .match_body(r#"{"reason":"duplicate"}"#)
            .with_status(200)
            .with_body(r#"{"ok":true}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let body = serde_json::json!({"reason": "duplicate"});
        let result = delete_json_with_body(&mut client, "/invites/42", &body).unwrap();

        assert_eq!(result["ok"], true);
        mock.assert();
    }

    #[test]
    fn api_error_400() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/bad")
            .with_status(400)
            .with_body(r#"{"code":"invalid_params","error":"bad request"}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let err = get_json(&mut client, "/bad", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { status, message } => {
                assert_eq!(status, 400);
                assert_eq!(message, "bad request");
            }
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn api_error_404() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/missing")
            .with_status(404)
            .with_body(r#"{"code":"not_found","error":"resource not found"}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let err = get_json(&mut client, "/missing", &HashMap::new()).unwrap_err();

        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn rate_limited_429() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/limited")
            .with_status(429)
            .with_header("retry-after", "30")
            .with_body("")
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let err = get_json(&mut client, "/limited", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { status, message } => {
                assert_eq!(status, 429);
                assert!(message.contains("30"));
            }
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn rate_limited_429_no_header() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/limited")
            .with_status(429)
            .with_body("")
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let err = get_json(&mut client, "/limited", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { message, .. } => assert!(message.contains("unknown")),
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn non_json_error_response() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/html-error")
            .with_status(502)
            .with_body("<html><body>Bad Gateway</body></html>")
            .create();

        let config = test_config(&server.url());
        let mut client = test_client(config);
        let err = get_json(&mut client, "/html-error", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { status, message } => {
                assert_eq!(status, 502);
                assert!(message.contains("Bad Gateway"));
            }
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn auth_error_no_token() {
        let config = Config::default(); // no token
        let mut client = test_client(config);
        let err = get_json(&mut client, "/anything", &HashMap::new()).unwrap_err();

        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn auth_401_with_env_var_token() {
        let mut server = mockito::Server::new();
        let _mock = server
            .mock("GET", "/protected")
            .with_status(401)
            .with_body(r#"{"error":"invalid_token"}"#)
            .create();

        // No config token — only injected env token.
        let config = Config {
            api_url: server.url(),
            ..Default::default()
        };
        let mut client = new_with_env_token(config, Some("bad_env_token".to_string())).unwrap();
        let err = get_json(&mut client, "/protected", &HashMap::new()).unwrap_err();

        // Should tell user to check env var, not try refresh
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn resolve_organization_delegates_to_config() {
        let config = Config {
            organization: Some(42),
            ..Default::default()
        };
        let client = test_client(config);
        assert_eq!(client.resolve_organization(None).unwrap(), 42);
        assert_eq!(client.resolve_organization(Some(99)).unwrap(), 99);
    }

    #[test]
    fn bearer_token_injected() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/test")
            .match_header("authorization", "Bearer my_secret_token")
            .with_status(200)
            .with_body(r"{}")
            .create();

        let mut config = test_config(&server.url());
        config.auth.access_token = Some("my_secret_token".to_string());
        let mut client = test_client(config);
        get_json(&mut client, "/test", &HashMap::new()).unwrap();

        mock.assert();
    }

    #[test]
    fn proactive_refresh_false_with_env_token() {
        let now = crate::time::now_secs();
        let config = Config {
            auth: crate::config::AuthConfig {
                access_token: Some("test_token".to_string()),
                refresh_token: Some("refresh_token".to_string()),
                expires_at: Some(now),
            },
            ..Default::default()
        };
        let client = new_with_env_token(config, Some("env_api_token".to_string())).unwrap();

        assert!(!client.should_refresh_proactively(now));
    }

    #[test]
    fn proactive_refresh_false_when_expiry_far_in_future() {
        let now = crate::time::now_secs();
        let config = Config {
            auth: crate::config::AuthConfig {
                access_token: Some("test_token".to_string()),
                refresh_token: Some("refresh_token".to_string()),
                expires_at: Some(now + 20 * 60),
            },
            ..Default::default()
        };
        let client = test_client(config);

        assert!(!client.should_refresh_proactively(now));
    }

    #[test]
    fn proactive_refresh_true_when_expiry_within_skew() {
        let now = crate::time::now_secs();
        let config = Config {
            auth: crate::config::AuthConfig {
                access_token: Some("test_token".to_string()),
                refresh_token: Some("refresh_token".to_string()),
                expires_at: Some(now + 30),
            },
            ..Default::default()
        };
        let client = test_client(config);

        assert!(client.should_refresh_proactively(now));
    }

    #[test]
    fn request_fails_when_proactive_refresh_fails() {
        let mut config = test_config("http://127.0.0.1:9");
        config.auth.refresh_token = Some("refresh_token".to_string());
        config.auth.expires_at = Some(crate::time::now_secs() + 30);
        // Force refresh attempt to fail regardless of env by using an unreachable auth endpoint.
        config.auth_url = "http://127.0.0.1:9".to_string();

        let mut client = test_client(config);
        let err = get_json(&mut client, "/users/me", &HashMap::new()).unwrap_err();
        match err {
            CliError::Network(message) => {
                assert!(message.contains("Couldn't refresh your session right now"));
            }
            CliError::Config(message) => {
                assert!(message.contains("OAuth client ID not set"));
            }
            _ => panic!("expected proactive refresh error"),
        }
    }
}
