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
