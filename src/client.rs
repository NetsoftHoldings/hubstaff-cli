use crate::auth;
use crate::config::Config;
use crate::error::CliError;
use reqwest::blocking::{Client, Response};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

pub struct HubstaffClient {
    config: Config,
    http: Client,
}

enum Method {
    Get,
    Post,
    Put,
    Delete,
}

impl HubstaffClient {
    pub fn new(config: Config) -> Result<Self, CliError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| CliError::Network(format!("failed to create HTTP client: {e}")))?;
        Ok(Self { config, http })
    }

    fn token(&self) -> Result<String, CliError> {
        self.config.get_token().ok_or_else(|| {
            CliError::Auth(
                "not authenticated. Run 'hubstaff-cli login' or set HUBSTAFF_API_TOKEN".to_string(),
            )
        })
    }

    fn base_url(&self) -> &str {
        &self.config.api_url
    }

    pub fn get(&mut self, path: &str, params: &HashMap<String, String>) -> Result<Value, CliError> {
        self.request(Method::Get, path, params, &Value::Null)
    }

    pub fn post(&mut self, path: &str, body: &Value) -> Result<Value, CliError> {
        self.request(Method::Post, path, &HashMap::new(), body)
    }

    pub fn put(&mut self, path: &str, body: &Value) -> Result<Value, CliError> {
        self.request(Method::Put, path, &HashMap::new(), body)
    }

    pub fn delete(&mut self, path: &str) -> Result<Value, CliError> {
        self.request(Method::Delete, path, &HashMap::new(), &Value::Null)
    }

    fn build_request(
        &self,
        method: &Method,
        url: &str,
        params: &HashMap<String, String>,
        body: &Value,
        token: &str,
    ) -> reqwest::blocking::RequestBuilder {
        let builder = match method {
            Method::Get => self.http.get(url).query(params),
            Method::Post => self.http.post(url).json(body),
            Method::Put => self.http.put(url).json(body),
            Method::Delete => self.http.delete(url),
        };
        builder.bearer_auth(token)
    }

    fn request(
        &mut self,
        method: Method,
        path: &str,
        params: &HashMap<String, String>,
        body: &Value,
    ) -> Result<Value, CliError> {
        let url = format!("{}{path}", self.base_url());
        let token = self.token()?;

        let resp = self
            .build_request(&method, &url, params, body, &token)
            .send()?;

        let status = resp.status().as_u16();

        // Token refresh on 401
        if status == 401 {
            if std::env::var("HUBSTAFF_API_TOKEN").is_ok_and(|v| !v.is_empty()) {
                return Err(CliError::Auth(
                    "invalid token. Check your HUBSTAFF_API_TOKEN".to_string(),
                ));
            }

            auth::refresh_token(&mut self.config)?;
            let new_token = self.token()?;

            let retry_resp = self
                .build_request(&method, &url, params, body, &new_token)
                .send()?;

            let retry_status = retry_resp.status().as_u16();
            if retry_status == 401 {
                return Err(CliError::Auth(
                    "session expired. Run 'hubstaff-cli login'".to_string(),
                ));
            }
            return Self::parse_response(retry_resp, retry_status);
        }

        Self::parse_response(resp, status)
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
                code: "rate_limited".to_string(),
                message: format!("rate limited. Retry after {retry_after}s"),
            });
        }

        let text = resp
            .text()
            .map_err(|e| CliError::Network(format!("failed to read response: {e}")))?;

        let body: Value = match serde_json::from_str(&text) {
            Ok(v) => v,
            Err(_) if status >= 400 => {
                let preview = if text.len() > 200 { &text[..200] } else { &text };
                return Err(CliError::Api {
                    status,
                    code: "non_json_error".to_string(),
                    message: format!("[{status}] {preview}"),
                });
            }
            Err(e) => return Err(CliError::from(e)),
        };

        if status >= 400 {
            let code = body["code"]
                .as_str()
                .unwrap_or("api_error")
                .to_string();
            let message = body["error"]
                .as_str()
                .unwrap_or("unknown API error")
                .to_string();
            return Err(CliError::Api {
                status,
                code,
                message,
            });
        }

        Ok(body)
    }

    pub fn resolve_org(&self, cli_org: Option<u64>) -> Result<u64, CliError> {
        self.config.resolve_org(cli_org)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito;

    fn test_config(server_url: &str) -> Config {
        let mut config = Config::default();
        config.api_url = server_url.to_string();
        config.auth.access_token = Some("test_token".to_string());
        config
    }

    #[test]
    fn get_success() {
        // SAFETY: test environment
        unsafe { std::env::remove_var("HUBSTAFF_API_TOKEN") };

        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/users/me")
            .match_header("authorization", "Bearer test_token")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"user":{"id":1,"name":"Test"}}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = HubstaffClient::new(config).unwrap();
        let result = client.get("/users/me", &HashMap::new()).unwrap();

        assert_eq!(result["user"]["id"], 1);
        assert_eq!(result["user"]["name"], "Test");
        mock.assert();
    }

    #[test]
    fn get_with_query_params() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/organizations/5/members")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("page_limit".into(), "10".into()),
            ]))
            .with_status(200)
            .with_body(r#"{"members":[]}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = HubstaffClient::new(config).unwrap();
        let mut params = HashMap::new();
        params.insert("page_limit".to_string(), "10".to_string());
        client.get("/organizations/5/members", &params).unwrap();

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
        let mut client = HubstaffClient::new(config).unwrap();
        let body = serde_json::json!({"name": "New"});
        let result = client.post("/organizations/1/projects", &body).unwrap();

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
        let mut client = HubstaffClient::new(config).unwrap();
        let body = serde_json::json!({"members": [{"user_id": 1, "role": "remove"}]});
        let result = client.put("/organizations/1/update_members", &body).unwrap();

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
        let mut client = HubstaffClient::new(config).unwrap();
        let result = client.delete("/invites/42").unwrap();

        assert!(result.is_null());
        mock.assert();
    }

    #[test]
    fn api_error_400() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/bad")
            .with_status(400)
            .with_body(r#"{"code":"invalid_params","error":"bad request"}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = HubstaffClient::new(config).unwrap();
        let err = client.get("/bad", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { status, code, message } => {
                assert_eq!(status, 400);
                assert_eq!(code, "invalid_params");
                assert_eq!(message, "bad request");
            }
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn api_error_404() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/missing")
            .with_status(404)
            .with_body(r#"{"code":"not_found","error":"resource not found"}"#)
            .create();

        let config = test_config(&server.url());
        let mut client = HubstaffClient::new(config).unwrap();
        let err = client.get("/missing", &HashMap::new()).unwrap_err();

        assert_eq!(err.exit_code(), 1);
    }

    #[test]
    fn rate_limited_429() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/limited")
            .with_status(429)
            .with_header("retry-after", "30")
            .with_body("")
            .create();

        let config = test_config(&server.url());
        let mut client = HubstaffClient::new(config).unwrap();
        let err = client.get("/limited", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { status, code, message } => {
                assert_eq!(status, 429);
                assert_eq!(code, "rate_limited");
                assert!(message.contains("30"));
            }
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn rate_limited_429_no_header() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/limited")
            .with_status(429)
            .with_body("")
            .create();

        let config = test_config(&server.url());
        let mut client = HubstaffClient::new(config).unwrap();
        let err = client.get("/limited", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { message, .. } => assert!(message.contains("unknown")),
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn non_json_error_response() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/html-error")
            .with_status(502)
            .with_body("<html><body>Bad Gateway</body></html>")
            .create();

        let config = test_config(&server.url());
        let mut client = HubstaffClient::new(config).unwrap();
        let err = client.get("/html-error", &HashMap::new()).unwrap_err();

        match err {
            CliError::Api { status, code, message } => {
                assert_eq!(status, 502);
                assert_eq!(code, "non_json_error");
                assert!(message.contains("Bad Gateway"));
            }
            _ => panic!("expected Api error"),
        }
    }

    #[test]
    fn auth_error_no_token() {
        // SAFETY: test environment
        unsafe { std::env::remove_var("HUBSTAFF_API_TOKEN") };
        let config = Config::default(); // no token
        let mut client = HubstaffClient::new(config).unwrap();
        let err = client.get("/anything", &HashMap::new()).unwrap_err();

        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn auth_401_with_env_var_token() {
        let mut server = mockito::Server::new();
        server
            .mock("GET", "/protected")
            .with_status(401)
            .with_body(r#"{"error":"invalid_token"}"#)
            .create();

        // Use a unique env var test: set it, make request, clean up
        // SAFETY: test environment
        unsafe { std::env::set_var("HUBSTAFF_API_TOKEN", "bad_env_token") };

        let mut config = Config::default();
        config.api_url = server.url();
        // No config token — only env var
        let mut client = HubstaffClient::new(config).unwrap();
        let err = client.get("/protected", &HashMap::new()).unwrap_err();

        // Clean up immediately
        // SAFETY: test environment
        unsafe { std::env::remove_var("HUBSTAFF_API_TOKEN") };

        // Should tell user to check env var, not try refresh
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn resolve_org_delegates_to_config() {
        let mut config = Config::default();
        config.org = Some(42);
        let client = HubstaffClient::new(config).unwrap();
        assert_eq!(client.resolve_org(None).unwrap(), 42);
        assert_eq!(client.resolve_org(Some(99)).unwrap(), 99);
    }

    #[test]
    fn bearer_token_injected() {
        let mut server = mockito::Server::new();
        let mock = server
            .mock("GET", "/test")
            .match_header("authorization", "Bearer my_secret_token")
            .with_status(200)
            .with_body(r#"{}"#)
            .create();

        let mut config = test_config(&server.url());
        config.auth.access_token = Some("my_secret_token".to_string());
        let mut client = HubstaffClient::new(config).unwrap();

        // SAFETY: test environment
        unsafe { std::env::remove_var("HUBSTAFF_API_TOKEN") };
        client.get("/test", &HashMap::new()).unwrap();

        mock.assert();
    }
}
