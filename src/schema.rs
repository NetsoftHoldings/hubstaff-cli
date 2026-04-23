use crate::config::Config;
use crate::error::CliError;
use crate::persistence::write_atomic;
use crate::time::now_secs;
use reqwest::blocking::Client;
use reqwest::header::{ETAG, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::time::Duration as StdDuration;

#[derive(Clone, Debug)]
pub struct ApiSchema {
    operations: Vec<Operation>,
    by_operation_id: HashMap<String, usize>,
    cache_meta: Option<SchemaCacheMeta>,
}

#[derive(Clone, Debug)]
pub struct Operation {
    pub id: String,
    pub method: String,
    pub path_template: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub tags: Vec<String>,
    pub parameters: Vec<ParameterSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParameterLocation {
    Path,
    Query,
    Body,
    Header,
    FormData,
}

#[derive(Clone, Debug)]
pub struct ParameterSpec {
    pub name: String,
    pub location: ParameterLocation,
    pub required: bool,
    pub description: Option<String>,
    pub data_type: Option<String>,
    pub enum_values: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SchemaCacheMeta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

enum FetchOutcome {
    NotModified,
    Updated {
        schema: Value,
        etag: Option<String>,
        source_url: String,
    },
}

impl ApiSchema {
    pub fn load(config: &Config) -> Result<Self, CliError> {
        let schema_url = config.effective_schema_url();
        let cached_schema = read_cached_schema();
        let cached_meta = read_cache_meta();
        let can_use_cached_fallback =
            cache_meta_matches_source(cached_meta.as_ref(), schema_url.as_str());

        match Self::refresh(config, false) {
            Ok(fresh) => Ok(fresh),
            Err(refresh_err) => {
                if can_use_cached_fallback
                    && let Some(schema) = cached_schema
                    && let Ok(parsed) = Self::from_schema(&schema, cached_meta)
                {
                    return Ok(parsed);
                }
                Err(refresh_err)
            }
        }
    }

    pub fn load_cache_only() -> Result<Self, CliError> {
        let cached = read_cached_schema()
            .ok_or_else(|| CliError::Config("schema cache is missing".to_string()))?;
        let meta = read_cache_meta();
        Self::from_schema(&cached, meta)
    }

    pub fn refresh(config: &Config, force: bool) -> Result<Self, CliError> {
        let http = http_client()?;
        let schema_url = config.effective_schema_url();
        let existing_meta = read_cache_meta();
        let etag = if force {
            None
        } else if cache_meta_matches_source(existing_meta.as_ref(), schema_url.as_str()) {
            existing_meta.as_ref().and_then(|meta| meta.etag.as_deref())
        } else {
            None
        };

        match fetch_schema(&http, &schema_url, etag)? {
            FetchOutcome::NotModified => {
                if !cache_meta_matches_source(existing_meta.as_ref(), schema_url.as_str()) {
                    return Err(CliError::Config(
                        "schema endpoint returned 304 but local cache provenance does not match requested schema_url".to_string(),
                    ));
                }
                let cached = read_cached_schema().ok_or_else(|| {
                    CliError::Config("schema returned 304 but local cache is missing".to_string())
                })?;
                let mut meta = existing_meta.unwrap_or_default();
                meta.fetched_at = Some(now_secs());
                if meta.schema_hash.is_none() {
                    meta.schema_hash = Some(hash_schema_json(&cached)?);
                }
                if meta.source_url.is_none() {
                    meta.source_url = Some(schema_url);
                }
                write_meta(&meta)?;
                Self::from_schema(&cached, Some(meta))
            }
            FetchOutcome::Updated {
                schema,
                etag,
                source_url,
            } => {
                write_cache(&schema, etag, &source_url)?;
                let cache_meta = read_cache_meta();
                Self::from_schema(&schema, cache_meta)
            }
        }
    }

    pub fn cache_meta_ref(&self) -> Option<&SchemaCacheMeta> {
        self.cache_meta.as_ref()
    }

    pub fn schema_hash(&self) -> Option<&str> {
        self.cache_meta
            .as_ref()
            .and_then(|meta| meta.schema_hash.as_deref())
    }

    pub fn operations(&self) -> &[Operation] {
        &self.operations
    }

    pub fn operation(&self, operation_id: &str) -> Option<&Operation> {
        self.by_operation_id
            .get(operation_id)
            .and_then(|index| self.operations.get(*index))
    }

    pub(crate) fn from_schema(
        schema: &Value,
        cache_meta: Option<SchemaCacheMeta>,
    ) -> Result<Self, CliError> {
        let paths = schema
            .get("paths")
            .and_then(Value::as_object)
            .ok_or_else(|| CliError::Config("schema is missing 'paths' object".to_string()))?;

        let global_parameters = schema.get("parameters").and_then(Value::as_object);
        let mut operations = Vec::new();
        let mut by_operation_id = HashMap::new();

        for (path_template, path_item) in paths {
            let Some(path_item_obj) = path_item.as_object() else {
                continue;
            };

            let path_level_params = parse_parameter_array(
                path_item_obj.get("parameters"),
                global_parameters,
                path_template,
            )?;

            for method in ["get", "post", "put", "delete", "patch"] {
                let Some(operation_value) = path_item_obj.get(method) else {
                    continue;
                };
                let Some(operation_obj) = operation_value.as_object() else {
                    continue;
                };

                let op_level_params = parse_parameter_array(
                    operation_obj.get("parameters"),
                    global_parameters,
                    path_template,
                )?;
                let parameters = merge_parameters(path_level_params.clone(), op_level_params);

                let operation_id = operation_obj
                    .get("operationId")
                    .and_then(Value::as_str)
                    .map_or_else(
                        || synthesize_operation_id(method, path_template),
                        str::to_string,
                    );

                if by_operation_id.contains_key(&operation_id) {
                    return Err(CliError::Config(format!(
                        "duplicate operation_id '{operation_id}' in schema"
                    )));
                }

                let operation = Operation {
                    id: operation_id.clone(),
                    method: method.to_uppercase(),
                    path_template: normalize_path_template(path_template),
                    summary: operation_obj
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    description: operation_obj
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    tags: operation_obj
                        .get("tags")
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(Value::as_str)
                                .map(str::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default(),
                    parameters,
                };

                by_operation_id.insert(operation_id, operations.len());
                operations.push(operation);
            }
        }

        operations.sort_by(|a, b| a.id.cmp(&b.id));
        by_operation_id = operations
            .iter()
            .enumerate()
            .map(|(idx, op)| (op.id.clone(), idx))
            .collect();

        Ok(Self {
            operations,
            by_operation_id,
            cache_meta,
        })
    }
}

fn cache_meta_matches_source(meta: Option<&SchemaCacheMeta>, schema_url: &str) -> bool {
    meta.and_then(|cache| cache.source_url.as_deref()) == Some(schema_url)
}

impl Operation {
    pub fn render_path(&self, path_params: &HashMap<String, String>) -> Result<String, CliError> {
        let mut rendered = self.path_template.clone();
        for (key, value) in path_params {
            let encoded = encode_path_parameter(value);
            rendered = rendered.replace(&format!("{{{key}}}"), &encoded);
        }

        if rendered.contains('{') || rendered.contains('}') {
            return Err(CliError::Config(format!(
                "missing required path parameter(s) for operation '{}'",
                self.id
            )));
        }

        Ok(rendered)
    }

    pub fn has_body_parameter(&self) -> bool {
        self.parameters.iter().any(|param| {
            matches!(
                param.location,
                ParameterLocation::Body | ParameterLocation::FormData
            )
        })
    }

    pub fn requires_body(&self) -> bool {
        self.parameters.iter().any(|param| {
            matches!(
                param.location,
                ParameterLocation::Body | ParameterLocation::FormData
            ) && param.required
        })
    }
}

fn encode_path_parameter(raw: &str) -> String {
    let mut encoded = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(&mut encoded, "%{byte:02X}").expect("writing to String should not fail");
        }
    }
    encoded
}

fn parse_parameter_array(
    node: Option<&Value>,
    global_parameters: Option<&Map<String, Value>>,
    path_template: &str,
) -> Result<Vec<ParameterSpec>, CliError> {
    let Some(values) = node else {
        return Ok(Vec::new());
    };

    let Some(array) = values.as_array() else {
        return Ok(Vec::new());
    };

    let mut out = Vec::new();
    for item in array {
        if let Some(parsed) = parse_parameter(item, global_parameters, path_template)? {
            out.push(parsed);
        }
    }
    Ok(out)
}

fn parse_parameter(
    node: &Value,
    global_parameters: Option<&Map<String, Value>>,
    path_template: &str,
) -> Result<Option<ParameterSpec>, CliError> {
    let resolved = resolve_parameter(node, global_parameters)?;
    let Some(obj) = resolved.as_object() else {
        return Ok(None);
    };

    let Some(name) = obj.get("name").and_then(Value::as_str) else {
        return Ok(None);
    };

    let Some(location) = obj
        .get("in")
        .and_then(Value::as_str)
        .and_then(parse_location)
    else {
        return Ok(None);
    };

    let mut required = obj
        .get("required")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if location == ParameterLocation::Path {
        required = true;
    }

    let data_type = if location == ParameterLocation::Body {
        obj.get("schema")
            .and_then(Value::as_object)
            .and_then(|schema| {
                schema
                    .get("type")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or_else(|| {
                        if schema.contains_key("$ref") {
                            Some("object".to_string())
                        } else {
                            None
                        }
                    })
            })
    } else {
        obj.get("type").and_then(Value::as_str).map(str::to_string)
    };

    let enum_values = obj
        .get("enum")
        .and_then(Value::as_array)
        .map(|vals| {
            vals.iter()
                .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_string))
                .collect()
        })
        .unwrap_or_default();

    // Swagger path params must map to a placeholder in path.
    if location == ParameterLocation::Path && !path_template.contains(&format!("{{{name}}}")) {
        return Ok(None);
    }

    Ok(Some(ParameterSpec {
        name: name.to_string(),
        location,
        required,
        description: obj
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_string),
        data_type,
        enum_values,
    }))
}

fn resolve_parameter(
    node: &Value,
    global_parameters: Option<&Map<String, Value>>,
) -> Result<Value, CliError> {
    let Some(reference) = node.get("$ref").and_then(Value::as_str) else {
        return Ok(node.clone());
    };

    let Some(name) = reference.strip_prefix("#/parameters/") else {
        return Err(CliError::Config(format!(
            "unsupported parameter reference '{reference}'"
        )));
    };

    let Some(globals) = global_parameters else {
        return Err(CliError::Config(format!(
            "parameter reference '{reference}' cannot be resolved"
        )));
    };

    globals.get(name).cloned().ok_or_else(|| {
        CliError::Config(format!(
            "parameter reference '{reference}' cannot be resolved"
        ))
    })
}

fn merge_parameters(
    path_level: Vec<ParameterSpec>,
    operation_level: Vec<ParameterSpec>,
) -> Vec<ParameterSpec> {
    let mut merged = path_level;

    for param in operation_level {
        if let Some(index) = merged
            .iter()
            .position(|existing| existing.name == param.name && existing.location == param.location)
        {
            merged[index] = param;
        } else {
            merged.push(param);
        }
    }

    merged
}

fn parse_location(value: &str) -> Option<ParameterLocation> {
    match value {
        "path" => Some(ParameterLocation::Path),
        "query" => Some(ParameterLocation::Query),
        "body" => Some(ParameterLocation::Body),
        "header" => Some(ParameterLocation::Header),
        "formData" => Some(ParameterLocation::FormData),
        _ => None,
    }
}

fn synthesize_operation_id(method: &str, path_template: &str) -> String {
    let normalized = path_template
        .trim_start_matches('/')
        .replace('/', "_")
        .replace(['{', '}'], "")
        .replace('-', "_");
    format!("{}_{}", method.to_lowercase(), normalized)
}

fn normalize_path_template(path: &str) -> String {
    let no_prefix = if path == "/v2" {
        "/".to_string()
    } else if let Some(without_prefix) = path.strip_prefix("/v2") {
        without_prefix.to_string()
    } else {
        path.to_string()
    };

    if no_prefix.starts_with('/') {
        no_prefix
    } else {
        format!("/{no_prefix}")
    }
}

fn http_client() -> Result<Client, CliError> {
    Client::builder()
        .timeout(StdDuration::from_secs(crate::HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| CliError::Network(format!("failed to create schema HTTP client: {e}")))
}

fn fetch_schema(
    http: &Client,
    schema_url: &str,
    etag: Option<&str>,
) -> Result<FetchOutcome, CliError> {
    let mut request = http.get(schema_url);
    if let Some(etag) = etag {
        request = request.header(IF_NONE_MATCH, etag);
    }

    let response = request
        .send()
        .map_err(|e| CliError::Network(format!("schema fetch failed: {e}")))?;

    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FetchOutcome::NotModified);
    }

    if !response.status().is_success() {
        return Err(CliError::Network(format!(
            "schema fetch failed with HTTP {}",
            response.status()
        )));
    }

    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    let schema = response
        .json::<Value>()
        .map_err(|e| CliError::Config(format!("failed to parse schema JSON: {e}")))?;

    Ok(FetchOutcome::Updated {
        schema,
        etag,
        source_url: schema_url.to_string(),
    })
}

fn read_cached_schema() -> Option<Value> {
    let content = fs::read_to_string(Config::schema_docs_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn read_cache_meta() -> Option<SchemaCacheMeta> {
    let content = fs::read_to_string(Config::schema_meta_path()).ok()?;
    toml::from_str(&content).ok()
}

fn write_cache(schema: &Value, etag: Option<String>, source_url: &str) -> Result<(), CliError> {
    let schema_dir = Config::schema_dir();
    if !schema_dir.exists() {
        fs::create_dir_all(&schema_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&schema_dir, fs::Permissions::from_mode(0o700))?;
        }
    }

    let schema_json = serde_json::to_vec_pretty(schema)
        .map_err(|e| CliError::Config(format!("schema cache serialize failed: {e}")))?;
    write_atomic(&Config::schema_docs_path(), &schema_json)?;

    let meta = SchemaCacheMeta {
        etag,
        fetched_at: Some(now_secs()),
        schema_hash: Some(hash_schema_json(schema)?),
        source_url: Some(source_url.to_string()),
    };
    write_meta(&meta)
}

fn hash_schema_json(schema: &Value) -> Result<String, CliError> {
    let bytes = serde_json::to_vec(schema)
        .map_err(|e| CliError::Config(format!("schema hash serialization failed: {e}")))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes);
    let mut out = String::with_capacity(64);
    for byte in digest.as_ref() {
        write!(&mut out, "{byte:02x}").expect("write to String");
    }
    Ok(out)
}

fn write_meta(meta: &SchemaCacheMeta) -> Result<(), CliError> {
    let content = toml::to_string_pretty(meta)?;
    write_atomic(&Config::schema_meta_path(), content.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_path_template_strips_v2_prefix() {
        assert_eq!(normalize_path_template("/v2/users/me"), "/users/me");
        assert_eq!(normalize_path_template("/users/me"), "/users/me");
    }

    #[test]
    fn render_path_replaces_placeholders() {
        let operation = Operation {
            id: "x".to_string(),
            method: "GET".to_string(),
            path_template: "/organizations/{organization_id}/projects/{project_id}".to_string(),
            summary: None,
            description: None,
            tags: Vec::new(),
            parameters: Vec::new(),
        };
        let params = HashMap::from([
            ("organization_id".to_string(), "1".to_string()),
            ("project_id".to_string(), "2".to_string()),
        ]);
        assert_eq!(
            operation.render_path(&params).unwrap(),
            "/organizations/1/projects/2"
        );
    }

    #[test]
    fn render_path_percent_encodes_reserved_chars() {
        let operation = Operation {
            id: "x".to_string(),
            method: "GET".to_string(),
            path_template: "/users/{user_id}".to_string(),
            summary: None,
            description: None,
            tags: Vec::new(),
            parameters: Vec::new(),
        };
        let params = HashMap::from([("user_id".to_string(), "a/b c?d#e%f".to_string())]);
        assert_eq!(
            operation.render_path(&params).unwrap(),
            "/users/a%2Fb%20c%3Fd%23e%25f"
        );
    }

    #[test]
    fn synthesize_operation_id_is_stable() {
        assert_eq!(
            synthesize_operation_id("get", "/v2/organizations/{organization_id}/members"),
            "get_v2_organizations_organization_id_members"
        );
    }

    #[test]
    fn cache_meta_matches_source_is_true_for_matching_source_url() {
        let meta = SchemaCacheMeta {
            source_url: Some("https://api.hubstaff.com/v2/docs".to_string()),
            ..Default::default()
        };

        assert!(cache_meta_matches_source(
            Some(&meta),
            "https://api.hubstaff.com/v2/docs"
        ));
    }

    #[test]
    fn cache_meta_matches_source_is_false_for_missing_or_mismatched_source_url() {
        let missing = SchemaCacheMeta::default();
        assert!(!cache_meta_matches_source(
            Some(&missing),
            "https://api.hubstaff.com/v2/docs"
        ));

        let mismatched = SchemaCacheMeta {
            source_url: Some("https://api.hubstaff.com/v2/docs".to_string()),
            ..Default::default()
        };
        assert!(!cache_meta_matches_source(
            Some(&mismatched),
            "https://staging.api.hubstaff.com/v2/docs"
        ));
        assert!(!cache_meta_matches_source(
            None,
            "https://api.hubstaff.com/v2/docs"
        ));
    }
}
