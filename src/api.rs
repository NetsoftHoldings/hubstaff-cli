use crate::client::HubstaffClient;
use crate::command_index::{CommandEntry, CommandIndex, ResolveResult, usage_line};
use crate::error::CliError;
use crate::schema::{ApiSchema, Operation, ParameterLocation, ParameterSpec};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::fs;

pub fn run_dynamic(
    client: &mut HubstaffClient,
    schema: &ApiSchema,
    args: &[String],
    pretty_json: bool,
    organization_override: Option<u64>,
) -> Result<(), CliError> {
    let index = CommandIndex::load_or_build(schema)?;
    let parsed = ParsedInvocation::parse(args)?;

    if parsed.positionals.is_empty() {
        print_global_help();
        return Ok(());
    }

    let resolution = index.resolve(&parsed.positionals);

    if parsed.help {
        print_help_for_resolution(schema, &resolution, &parsed.positionals)?;
        return Ok(());
    }

    let command = match resolution {
        ResolveResult::Matched(command) => command,
        other => return Err(resolve_error(other)),
    };

    let operation = schema
        .operation(&command.entry.operation_id)
        .ok_or_else(|| {
            CliError::Config(format!(
                "operation '{}' is missing from current schema",
                command.entry.operation_id
            ))
        })?;

    ensure_supported(operation)?;

    let path_values =
        build_path_values(command.entry, &parsed.positionals[command.command_depth..])?;

    let options = parse_operation_options(operation, command.entry, &parsed.raw_options)?;
    let mut query_values = options.query_values;
    let mut path_values = path_values;

    validate_known_values(
        operation,
        &path_values,
        ParameterLocation::Path,
        "positional path arguments",
    )?;
    validate_known_values(
        operation,
        &query_values,
        ParameterLocation::Query,
        "query options",
    )?;

    fill_defaults_from_config(
        operation,
        client,
        organization_override,
        &mut path_values,
        &mut query_values,
    )?;
    validate_required_parameters(operation, &path_values, &query_values)?;
    validate_enum_values(operation, &path_values, ParameterLocation::Path)?;
    validate_enum_values(operation, &query_values, ParameterLocation::Query)?;

    let body = parse_body(
        operation,
        options.body_json.as_deref(),
        options.body_file.as_deref(),
    )?;
    let path = operation.render_path(&path_values)?;

    let response = client.request_json(&operation.method, &path, &query_values, body.as_ref())?;

    if pretty_json {
        println!("{}", serde_json::to_string_pretty(&response)?);
    } else {
        println!("{}", serde_json::to_string(&response)?);
    }

    Ok(())
}

#[derive(Debug)]
struct ParsedInvocation {
    positionals: Vec<String>,
    raw_options: Vec<RawOption>,
    help: bool,
}

#[derive(Clone, Debug)]
struct RawOption {
    name: String,
    value: Option<String>,
    dashed_value_candidate: Option<String>,
}

impl ParsedInvocation {
    fn parse(args: &[String]) -> Result<Self, CliError> {
        let mut positionals = Vec::new();
        let mut raw_options = Vec::new();
        let mut help = false;

        let mut index = 0;
        while index < args.len() {
            let token = &args[index];
            match token.as_str() {
                "-h" | "--help" => {
                    help = true;
                    index += 1;
                }
                "--" => {
                    positionals.extend_from_slice(&args[index + 1..]);
                    break;
                }
                _ if token.starts_with("--") => {
                    let stripped = token.trim_start_matches("--");
                    if stripped.is_empty() {
                        return Err(CliError::Config("empty option name".to_string()));
                    }

                    if let Some((name, value)) = stripped.split_once('=') {
                        raw_options.push(RawOption {
                            name: name.to_string(),
                            value: Some(value.to_string()),
                            dashed_value_candidate: None,
                        });
                        index += 1;
                    } else {
                        let mut dashed_value_candidate = None;
                        let value = args.get(index + 1).and_then(|next| {
                            if next.starts_with("--") {
                                dashed_value_candidate = Some(next.clone());
                                None
                            } else {
                                Some(next.clone())
                            }
                        });
                        if value.is_some() {
                            index += 1;
                        }
                        raw_options.push(RawOption {
                            name: stripped.to_string(),
                            value,
                            dashed_value_candidate,
                        });
                        index += 1;
                    }
                }
                _ if token.starts_with('-') => {
                    return Err(CliError::Config(format!("unknown option '{token}'")));
                }
                _ => {
                    positionals.push(token.clone());
                    index += 1;
                }
            }
        }

        Ok(Self {
            positionals,
            raw_options,
            help,
        })
    }
}

struct OperationOptions {
    query_values: HashMap<String, String>,
    body_json: Option<String>,
    body_file: Option<String>,
}

fn parse_operation_options(
    operation: &Operation,
    entry: &CommandEntry,
    raw_options: &[RawOption],
) -> Result<OperationOptions, CliError> {
    let query_param_names = operation
        .parameters
        .iter()
        .filter(|parameter| parameter.location == ParameterLocation::Query)
        .map(|parameter| parameter.name.clone())
        .collect::<HashSet<_>>();

    let path_param_names = operation
        .parameters
        .iter()
        .filter(|parameter| parameter.location == ParameterLocation::Path)
        .map(|parameter| parameter.name.clone())
        .collect::<HashSet<_>>();

    let body_param_names = operation
        .parameters
        .iter()
        .filter(|parameter| {
            matches!(
                parameter.location,
                ParameterLocation::Body | ParameterLocation::FormData
            )
        })
        .map(|parameter| parameter.name.clone())
        .collect::<HashSet<_>>();

    let mut query_values = HashMap::new();
    let mut body_json = None;
    let mut body_file = None;

    for raw in raw_options {
        match raw.name.as_str() {
            "query" => {
                let assignment = required_option_value(raw, "--query")?;
                let (name, value) = assignment.split_once('=').ok_or_else(|| {
                    CliError::Config(format!(
                        "--query expects NAME=VALUE entries, got '{assignment}'"
                    ))
                })?;
                insert_unique_query(&mut query_values, name, value)?;
            }
            "body-json" => {
                let value = required_option_value(raw, "--body-json")?;
                if body_file.is_some() {
                    return Err(CliError::Config(
                        "--body-json conflicts with --body-file".to_string(),
                    ));
                }
                body_json = Some(value.to_string());
            }
            "body-file" => {
                let value = required_option_value(raw, "--body-file")?;
                if body_json.is_some() {
                    return Err(CliError::Config(
                        "--body-file conflicts with --body-json".to_string(),
                    ));
                }
                body_file = Some(value.to_string());
            }
            "help" => {}
            name if query_param_names.contains(name) => {
                let value = required_option_value(raw, &format!("--{name}"))?;
                insert_unique_query(&mut query_values, name, value)?;
            }
            name if path_param_names.contains(name) => {
                return Err(CliError::Config(format!(
                    "--{name} is a path parameter; provide it positionally: {}",
                    usage_line(entry)
                )));
            }
            name if body_param_names.contains(name) => {
                let value = required_option_value(raw, &format!("--{name}"))?;
                if body_file.is_some() {
                    return Err(CliError::Config(format!(
                        "--{name} conflicts with --body-file"
                    )));
                }
                if body_json.is_some() {
                    return Err(CliError::Config(format!(
                        "multiple body options provided (--body-json and --{name})"
                    )));
                }
                body_json = Some(value.to_string());
            }
            name => {
                return Err(CliError::Config(format!(
                    "unknown option '--{name}' for '{}'. Use --help to see valid options",
                    usage_line(entry)
                )));
            }
        }
    }

    Ok(OperationOptions {
        query_values,
        body_json,
        body_file,
    })
}

fn required_option_value<'a>(raw: &'a RawOption, flag: &str) -> Result<&'a str, CliError> {
    if let Some(value) = raw.value.as_deref() {
        return Ok(value);
    }

    if let Some(candidate) = raw.dashed_value_candidate.as_deref() {
        return Err(CliError::Config(format!(
            "{flag} requires a value. If the value starts with '--', pass it with '=' (for example: {flag}={candidate})"
        )));
    }

    Err(CliError::Config(format!("{flag} requires a value")))
}

fn insert_unique_query(
    query_values: &mut HashMap<String, String>,
    name: &str,
    value: &str,
) -> Result<(), CliError> {
    if query_values
        .insert(name.to_string(), value.to_string())
        .is_some()
    {
        return Err(CliError::Config(format!(
            "query parameter '{name}' was provided multiple times"
        )));
    }
    Ok(())
}

fn build_path_values(
    entry: &CommandEntry,
    path_positionals: &[String],
) -> Result<HashMap<String, String>, CliError> {
    if path_positionals.len() != entry.visible_path_params.len() {
        return Err(CliError::Config(format!(
            "'{}' expects {} path argument(s)",
            entry.command_words.join(" "),
            entry.visible_path_params.len()
        )));
    }

    Ok(entry
        .visible_path_params
        .iter()
        .zip(path_positionals.iter())
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect())
}

fn resolve_error(result: ResolveResult<'_>) -> CliError {
    match result {
        ResolveResult::Matched(_) => CliError::Config("unexpected resolver state".to_string()),
        ResolveResult::Unknown { input, suggestions } => {
            if suggestions.is_empty() {
                CliError::Config(format!("unknown command '{input}'"))
            } else {
                CliError::Config(format!(
                    "unknown command '{input}'. Try: {}",
                    suggestions.join("; ")
                ))
            }
        }
        ResolveResult::Ambiguous { input, candidates } => {
            let details = candidates
                .iter()
                .map(|entry| {
                    format!(
                        "{} {} ({})",
                        entry.method, entry.path_template, entry.operation_id
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            CliError::Config(format!("ambiguous command '{input}': {details}"))
        }
        ResolveResult::ShapeMismatch {
            command_words,
            provided_path_count,
            candidates,
        } => {
            if candidates.len() == 1 {
                let candidate = candidates[0];
                CliError::Config(format!(
                    "'{}' expects {} path argument(s): {}",
                    command_words.join(" "),
                    candidate.visible_path_params.len(),
                    candidate.visible_path_params.join(", ")
                ))
            } else {
                let expected = candidates
                    .iter()
                    .map(|entry| {
                        format!(
                            "{} (expects {} path args)",
                            usage_line(entry),
                            entry.visible_path_params.len()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                CliError::Config(format!(
                    "'{}' with {} path argument(s) did not match. Candidates: {expected}",
                    command_words.join(" "),
                    provided_path_count
                ))
            }
        }
    }
}

fn print_help_for_resolution(
    schema: &ApiSchema,
    resolution: &ResolveResult<'_>,
    positionals: &[String],
) -> Result<(), CliError> {
    match resolution {
        ResolveResult::Matched(command) => {
            let operation = schema
                .operation(&command.entry.operation_id)
                .ok_or_else(|| {
                    CliError::Config(format!(
                        "operation '{}' is missing from current schema",
                        command.entry.operation_id
                    ))
                })?;
            print_operation_help(command.entry, operation);
        }
        ResolveResult::ShapeMismatch { candidates, .. } if candidates.len() == 1 => {
            let entry = candidates[0];
            let operation = schema.operation(&entry.operation_id).ok_or_else(|| {
                CliError::Config(format!(
                    "operation '{}' is missing from current schema",
                    entry.operation_id
                ))
            })?;
            print_operation_help(entry, operation);
        }
        ResolveResult::ShapeMismatch { candidates, .. } => {
            println!("Multiple command shapes match this prefix:");
            for entry in candidates {
                println!("  {}", usage_line(entry));
            }
        }
        ResolveResult::Ambiguous { candidates, .. } => {
            println!("Ambiguous command. Matching operations:");
            for entry in candidates {
                println!(
                    "  {} {} ({})",
                    entry.method, entry.path_template, entry.operation_id
                );
            }
        }
        ResolveResult::Unknown { suggestions, .. } => {
            print_global_help();
            if !suggestions.is_empty() {
                println!();
                println!("Suggestions:");
                for usage in suggestions {
                    println!("  {usage}");
                }
            }
        }
    }

    if positionals.is_empty() {
        print_global_help();
    }

    Ok(())
}

fn print_global_help() {
    println!("Schema-driven API command mode");
    println!();
    println!("Usage:");
    println!("  hubstaff <command> [path_ids...] [query options]");
    println!("  hubstaff <command> [path_ids...] [--body-json JSON | --body-file PATH]");
    println!();
    println!("Examples:");
    println!("  hubstaff users me");
    println!("  hubstaff projects list");
    println!("  hubstaff teams update_members 123");
    println!("  hubstaff projects list --page_limit 10");
    println!();
    println!("Discover commands:");
    println!("  hubstaff commands list");
}

fn print_operation_help(entry: &CommandEntry, operation: &Operation) {
    println!("Command:");
    println!("  {}", usage_line(entry));
    println!();
    println!("Operation:");
    println!("  method = {}", operation.method);
    println!("  path = {}", operation.path_template);

    if let Some(summary) = &operation.summary {
        println!("  summary = {summary}");
    }
    if let Some(description) = &operation.description {
        println!("  description = {description}");
    }
    if !operation.tags.is_empty() {
        println!("  tags = {}", operation.tags.join(", "));
    }

    if !entry.visible_path_params.is_empty() {
        println!();
        println!("Path arguments:");
        for param_name in &entry.visible_path_params {
            if let Some(param) = operation
                .parameters
                .iter()
                .find(|candidate| candidate.name == *param_name)
            {
                print_parameter_help(param, "positional");
            }
        }
    }

    let query_params = operation
        .parameters
        .iter()
        .filter(|parameter| parameter.location == ParameterLocation::Query)
        .collect::<Vec<_>>();
    if !query_params.is_empty() {
        println!();
        println!("Query options:");
        for parameter in query_params {
            print_parameter_help(parameter, "--");
        }
    }

    if operation.has_body_parameter() {
        println!();
        println!("Body:");
        println!("  --body-json <JSON>");
        println!("  --body-file <PATH>");
        println!(
            "  Values that start with '--' must use '=' syntax (example: --body-json=--literal-value)"
        );

        for parameter in operation.parameters.iter().filter(|parameter| {
            matches!(
                parameter.location,
                ParameterLocation::Body | ParameterLocation::FormData
            )
        }) {
            println!("  --{} <JSON>  (alias for body)", parameter.name);
        }
    }
}

fn print_parameter_help(parameter: &ParameterSpec, mode: &str) {
    let required = if parameter.required {
        "required"
    } else {
        "optional"
    };
    let type_name = parameter.data_type.as_deref().unwrap_or("-");

    if mode == "--" {
        print!("  --{}", parameter.name);
    } else {
        print!("  <{}>", parameter.name);
    }

    print!(" ({required}, type={type_name})");

    if !parameter.enum_values.is_empty() {
        print!(" enum=[{}]", parameter.enum_values.join(", "));
    }

    if let Some(description) = &parameter.description {
        print!(" - {description}");
    }

    println!();
}

fn ensure_supported(operation: &Operation) -> Result<(), CliError> {
    let unsupported_form_data: Vec<&ParameterSpec> = operation
        .parameters
        .iter()
        .filter(|parameter| parameter.location == ParameterLocation::FormData)
        .collect();

    if !unsupported_form_data.is_empty() {
        let names = unsupported_form_data
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CliError::Config(format!(
            "operation '{}' uses unsupported formData parameters: {names}; multipart/form-data payloads are not supported yet",
            operation.id
        )));
    }

    let unsupported_required_headers: Vec<&ParameterSpec> = operation
        .parameters
        .iter()
        .filter(|parameter| parameter.required && parameter.location == ParameterLocation::Header)
        .collect();

    if unsupported_required_headers.is_empty() {
        return Ok(());
    }

    let names = unsupported_required_headers
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    Err(CliError::Config(format!(
        "operation '{}' requires unsupported header parameters: {names}",
        operation.id
    )))
}

fn validate_known_values(
    operation: &Operation,
    values: &HashMap<String, String>,
    location: ParameterLocation,
    source: &str,
) -> Result<(), CliError> {
    for key in values.keys() {
        let exists = operation
            .parameters
            .iter()
            .any(|parameter| parameter.location == location && parameter.name == *key);

        if !exists {
            return Err(CliError::Config(format!(
                "{source} '{key}' is not valid for operation '{}'",
                operation.id
            )));
        }
    }

    Ok(())
}

fn fill_defaults_from_config(
    operation: &Operation,
    client: &HubstaffClient,
    organization_override: Option<u64>,
    path_values: &mut HashMap<String, String>,
    query_values: &mut HashMap<String, String>,
) -> Result<(), CliError> {
    for parameter in operation
        .parameters
        .iter()
        .filter(|parameter| parameter.required)
    {
        if parameter.name != "organization_id" {
            continue;
        }

        let needs_value = match parameter.location {
            ParameterLocation::Path => !path_values.contains_key("organization_id"),
            ParameterLocation::Query => !query_values.contains_key("organization_id"),
            _ => false,
        };

        if !needs_value {
            continue;
        }

        let organization_id = client
            .resolve_organization(organization_override)?
            .to_string();
        match parameter.location {
            ParameterLocation::Path => {
                path_values.insert("organization_id".to_string(), organization_id);
            }
            ParameterLocation::Query => {
                query_values.insert("organization_id".to_string(), organization_id);
            }
            _ => {}
        }
    }

    Ok(())
}

fn validate_required_parameters(
    operation: &Operation,
    path_values: &HashMap<String, String>,
    query_values: &HashMap<String, String>,
) -> Result<(), CliError> {
    for parameter in operation
        .parameters
        .iter()
        .filter(|parameter| parameter.required)
    {
        match parameter.location {
            ParameterLocation::Path => {
                if !path_values.contains_key(&parameter.name) {
                    return Err(CliError::Config(format!(
                        "missing required path parameter '{}'",
                        parameter.name
                    )));
                }
            }
            ParameterLocation::Query => {
                if !query_values.contains_key(&parameter.name) {
                    return Err(CliError::Config(format!(
                        "missing required query parameter '{}'",
                        parameter.name
                    )));
                }
            }
            ParameterLocation::Body | ParameterLocation::Header | ParameterLocation::FormData => {}
        }
    }

    Ok(())
}

fn validate_enum_values(
    operation: &Operation,
    values: &HashMap<String, String>,
    location: ParameterLocation,
) -> Result<(), CliError> {
    for (name, value) in values {
        let parameter = operation
            .parameters
            .iter()
            .find(|parameter| parameter.location == location && parameter.name == *name);

        let Some(parameter) = parameter else {
            continue;
        };

        if !parameter.enum_values.is_empty() && !parameter.enum_values.contains(value) {
            return Err(CliError::Config(format!(
                "parameter '{}' must be one of [{}]",
                name,
                parameter.enum_values.join(", ")
            )));
        }
    }

    Ok(())
}

fn parse_body(
    operation: &Operation,
    body_json: Option<&str>,
    body_file: Option<&str>,
) -> Result<Option<Value>, CliError> {
    if !operation.has_body_parameter() {
        if body_json.is_some() || body_file.is_some() {
            return Err(CliError::Config(format!(
                "operation '{}' does not accept a request body",
                operation.id
            )));
        }
        return Ok(None);
    }

    let body_value = if let Some(raw_json) = body_json {
        Some(parse_json_body(raw_json)?)
    } else if let Some(path) = body_file {
        let content = fs::read_to_string(path)
            .map_err(|e| CliError::Config(format!("failed to read --body-file '{path}': {e}")))?;
        Some(parse_json_body(&content)?)
    } else {
        None
    };

    if operation.requires_body() && body_value.is_none() {
        return Err(CliError::Config(format!(
            "operation '{}' requires a request body; provide --body-json or --body-file",
            operation.id
        )));
    }

    Ok(body_value)
}

fn parse_json_body(raw: &str) -> Result<Value, CliError> {
    serde_json::from_str(raw).map_err(|e| CliError::Config(format!("invalid JSON body: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn param(name: &str, location: ParameterLocation, required: bool) -> ParameterSpec {
        ParameterSpec {
            name: name.to_string(),
            location,
            required,
            description: None,
            data_type: Some("string".to_string()),
            enum_values: Vec::new(),
        }
    }

    fn operation(parameters: Vec<ParameterSpec>) -> Operation {
        Operation {
            id: "op_id".to_string(),
            method: "GET".to_string(),
            path_template: "/teams/{team_id}/members".to_string(),
            summary: None,
            description: None,
            tags: Vec::new(),
            parameters,
        }
    }

    fn command_entry() -> CommandEntry {
        CommandEntry {
            operation_id: "op_id".to_string(),
            method: "GET".to_string(),
            path_template: "/teams/{team_id}/members".to_string(),
            command_words: vec!["teams".to_string(), "members".to_string()],
            visible_path_params: vec!["team_id".to_string()],
        }
    }

    #[test]
    fn parse_operation_options_accepts_operation_specific_query_flags() {
        let operation = operation(vec![
            param("team_id", ParameterLocation::Path, true),
            param("page_limit", ParameterLocation::Query, false),
        ]);
        let entry = command_entry();
        let raw = vec![RawOption {
            name: "page_limit".to_string(),
            value: Some("10".to_string()),
            dashed_value_candidate: None,
        }];

        let parsed = parse_operation_options(&operation, &entry, &raw).unwrap();
        assert_eq!(
            parsed.query_values.get("page_limit"),
            Some(&"10".to_string())
        );
    }

    #[test]
    fn ensure_supported_rejects_optional_form_data_parameters() {
        let operation = operation(vec![param(
            "attachment",
            ParameterLocation::FormData,
            false,
        )]);

        let Err(err) = ensure_supported(&operation) else {
            panic!("expected config error");
        };
        let CliError::Config(message) = err else {
            panic!("expected config error");
        };
        assert!(message.contains("unsupported formData parameters"));
        assert!(message.contains("attachment"));
        assert!(message.contains("multipart/form-data"));
    }

    #[test]
    fn ensure_supported_rejects_required_form_data_parameters() {
        let operation = operation(vec![param("attachment", ParameterLocation::FormData, true)]);

        let Err(err) = ensure_supported(&operation) else {
            panic!("expected config error");
        };
        let CliError::Config(message) = err else {
            panic!("expected config error");
        };
        assert!(message.contains("unsupported formData parameters"));
        assert!(message.contains("attachment"));
    }

    #[test]
    fn ensure_supported_allows_operations_without_form_data_parameters() {
        let operation = operation(vec![
            param("team_id", ParameterLocation::Path, true),
            param("page_limit", ParameterLocation::Query, false),
            param("body", ParameterLocation::Body, false),
        ]);

        ensure_supported(&operation).unwrap();
    }

    #[test]
    fn parse_operation_options_rejects_path_param_flags() {
        let operation = operation(vec![param("team_id", ParameterLocation::Path, true)]);
        let entry = command_entry();
        let raw = vec![RawOption {
            name: "team_id".to_string(),
            value: Some("42".to_string()),
            dashed_value_candidate: None,
        }];

        let Err(err) = parse_operation_options(&operation, &entry, &raw) else {
            panic!("expected config error");
        };
        let CliError::Config(message) = err else {
            panic!("expected config error");
        };
        assert!(message.contains("path parameter"));
        assert!(message.contains("teams members <team_id>"));
    }

    #[test]
    fn parse_operation_options_rejects_unknown_option() {
        let operation = operation(vec![param("team_id", ParameterLocation::Path, true)]);
        let entry = command_entry();
        let raw = vec![RawOption {
            name: "unknown".to_string(),
            value: Some("x".to_string()),
            dashed_value_candidate: None,
        }];

        let Err(err) = parse_operation_options(&operation, &entry, &raw) else {
            panic!("expected config error");
        };
        let CliError::Config(message) = err else {
            panic!("expected config error");
        };
        assert!(message.contains("unknown option '--unknown'"));
    }

    #[test]
    fn resolve_error_includes_suggestions_for_unknown() {
        let err = resolve_error(ResolveResult::Unknown {
            input: "teams mystery".to_string(),
            suggestions: vec!["teams members <team_id>".to_string()],
        });
        let CliError::Config(message) = err else {
            panic!("expected config error");
        };
        assert!(message.contains("unknown command 'teams mystery'"));
        assert!(message.contains("teams members <team_id>"));
    }

    #[test]
    fn parsed_invocation_tracks_dash_prefixed_candidate_values() {
        let args = vec![
            "teams".to_string(),
            "members".to_string(),
            "123".to_string(),
            "--body-json".to_string(),
            "--literal-value".to_string(),
        ];

        let parsed = ParsedInvocation::parse(&args).unwrap();
        assert_eq!(parsed.raw_options.len(), 2);
        assert_eq!(parsed.raw_options[0].name, "body-json");
        assert_eq!(parsed.raw_options[0].value, None);
        assert_eq!(
            parsed.raw_options[0].dashed_value_candidate,
            Some("--literal-value".to_string())
        );
    }

    #[test]
    fn parse_operation_options_suggests_equals_for_dash_prefixed_values() {
        let operation = operation(vec![
            param("team_id", ParameterLocation::Path, true),
            param("body", ParameterLocation::Body, false),
        ]);
        let entry = command_entry();
        let raw = vec![RawOption {
            name: "body-json".to_string(),
            value: None,
            dashed_value_candidate: Some("--literal-value".to_string()),
        }];

        let Err(err) = parse_operation_options(&operation, &entry, &raw) else {
            panic!("expected config error");
        };
        let CliError::Config(message) = err else {
            panic!("expected config error");
        };
        assert!(message.contains("--body-json requires a value"));
        assert!(message.contains("--body-json=--literal-value"));
    }
}
