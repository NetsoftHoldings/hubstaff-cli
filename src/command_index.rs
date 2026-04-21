use crate::config::Config;
use crate::error::CliError;
use crate::persistence::write_atomic;
use crate::schema::{ApiSchema, Operation};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

const INDEX_VERSION: u32 = 1;

#[derive(Clone, Debug)]
pub struct CommandIndex {
    entries: Vec<CommandEntry>,
    trie: TrieNode,
    by_first_word: BTreeMap<String, Vec<usize>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CommandEntry {
    pub operation_id: String,
    pub method: String,
    pub path_template: String,
    pub command_words: Vec<String>,
    pub visible_path_params: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct ResolvedCommand<'a> {
    pub entry: &'a CommandEntry,
    pub command_depth: usize,
}

pub enum ResolveResult<'a> {
    Matched(ResolvedCommand<'a>),
    ShapeMismatch {
        command_words: Vec<String>,
        provided_path_count: usize,
        candidates: Vec<&'a CommandEntry>,
    },
    Ambiguous {
        input: String,
        candidates: Vec<&'a CommandEntry>,
    },
    Unknown {
        input: String,
        suggestions: Vec<String>,
    },
}

#[derive(Clone, Debug, Default)]
struct TrieNode {
    terminals: Vec<usize>,
    children: BTreeMap<String, TrieNode>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CommandIndexCache {
    version: u32,
    schema_hash: String,
    entries: Vec<CommandEntry>,
}

impl CommandIndex {
    pub fn load_or_build(schema: &ApiSchema) -> Result<Self, CliError> {
        let schema_hash = schema_hash(schema);

        if let Some(cached) = read_cache()
            && cached.version == INDEX_VERSION
            && cached.schema_hash == schema_hash
        {
            return Ok(Self::from_entries(cached.entries));
        }

        let built_entries = build_entries(schema);
        write_cache(&CommandIndexCache {
            version: INDEX_VERSION,
            schema_hash,
            entries: built_entries.clone(),
        })?;

        Ok(Self::from_entries(built_entries))
    }

    pub fn resolve<'a>(&'a self, positionals: &[String]) -> ResolveResult<'a> {
        let Some((depth, candidate_indexes)) = self.match_terminal(positionals) else {
            return ResolveResult::Unknown {
                input: positionals.join(" "),
                suggestions: self.suggestions(positionals.first().map(String::as_str), 8),
            };
        };

        let provided_path_count = positionals.len().saturating_sub(depth);
        let exact_candidates = candidate_indexes
            .iter()
            .filter_map(|index| self.entries.get(*index))
            .filter(|entry| entry.visible_path_params.len() == provided_path_count)
            .collect::<Vec<_>>();

        if exact_candidates.len() == 1 {
            return ResolveResult::Matched(ResolvedCommand {
                entry: exact_candidates[0],
                command_depth: depth,
            });
        }

        if exact_candidates.len() > 1 {
            return ResolveResult::Ambiguous {
                input: positionals.join(" "),
                candidates: exact_candidates,
            };
        }

        let candidates = candidate_indexes
            .iter()
            .filter_map(|index| self.entries.get(*index))
            .collect::<Vec<_>>();

        let command_words = candidates
            .first()
            .map(|entry| entry.command_words.clone())
            .unwrap_or_default();

        ResolveResult::ShapeMismatch {
            command_words,
            provided_path_count,
            candidates,
        }
    }

    pub fn sample_usages(&self, limit: usize) -> Vec<String> {
        let mut usages = self
            .entries
            .iter()
            .map(usage_line)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        usages.truncate(limit);
        usages
    }

    pub fn suggestions(&self, first_word: Option<&str>, limit: usize) -> Vec<String> {
        let usages = if let Some(word) = first_word {
            if let Some(indexes) = self.by_first_word.get(word) {
                indexes
                    .iter()
                    .filter_map(|index| self.entries.get(*index))
                    .map(usage_line)
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect::<Vec<_>>()
            } else {
                self.sample_usages(limit)
            }
        } else {
            self.sample_usages(limit)
        };

        usages.into_iter().take(limit).collect()
    }

    fn from_entries(entries: Vec<CommandEntry>) -> Self {
        let mut trie = TrieNode::default();
        let mut by_first_word = BTreeMap::<String, Vec<usize>>::new();

        for (index, entry) in entries.iter().enumerate() {
            if let Some(first_word) = entry.command_words.first() {
                by_first_word
                    .entry(first_word.clone())
                    .or_default()
                    .push(index);
            }

            let mut node = &mut trie;
            for word in &entry.command_words {
                node = node.children.entry(word.clone()).or_default();
            }
            node.terminals.push(index);
        }

        Self {
            entries,
            trie,
            by_first_word,
        }
    }

    fn match_terminal(&self, tokens: &[String]) -> Option<(usize, Vec<usize>)> {
        let mut node = &self.trie;
        let mut best: Option<(usize, Vec<usize>)> = None;

        for (index, token) in tokens.iter().enumerate() {
            let Some(next) = node.children.get(token) else {
                break;
            };
            node = next;
            if !node.terminals.is_empty() {
                best = Some((index + 1, node.terminals.clone()));
            }
        }

        best
    }
}

pub fn usage_line(entry: &CommandEntry) -> String {
    let mut usage = entry.command_words.join(" ");
    for param in &entry.visible_path_params {
        usage.push(' ');
        usage.push('<');
        usage.push_str(param);
        usage.push('>');
    }
    usage
}

fn build_entries(schema: &ApiSchema) -> Vec<CommandEntry> {
    let mut entries = schema
        .operations()
        .iter()
        .filter_map(build_entry)
        .collect::<Vec<_>>();

    entries.sort_by(|a, b| {
        usage_line(a)
            .cmp(&usage_line(b))
            .then(a.operation_id.cmp(&b.operation_id))
            .then(a.method.cmp(&b.method))
    });

    entries
}

fn build_entry(operation: &Operation) -> Option<CommandEntry> {
    let full_segments = parse_path_segments(&operation.path_template);
    if full_segments.is_empty() {
        return None;
    }

    let logical_segments = strip_organization_prefix(&full_segments);
    let mut command_words = logical_segments
        .iter()
        .filter_map(|segment| match segment {
            PathSegment::Static(value) => Some(value.clone()),
            PathSegment::Param(_) => None,
        })
        .collect::<Vec<_>>();

    if let Some(action) = synthetic_action(&operation.method, &logical_segments) {
        command_words.push(action.to_string());
    }

    if command_words.is_empty() {
        return None;
    }

    let visible_path_params = logical_segments
        .iter()
        .filter_map(|segment| match segment {
            PathSegment::Param(name) if name != "organization_id" => Some(name.clone()),
            PathSegment::Param(_) | PathSegment::Static(_) => None,
        })
        .collect::<Vec<_>>();

    Some(CommandEntry {
        operation_id: operation.id.clone(),
        method: operation.method.clone(),
        path_template: operation.path_template.clone(),
        command_words,
        visible_path_params,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PathSegment {
    Static(String),
    Param(String),
}

fn parse_path_segments(path_template: &str) -> Vec<PathSegment> {
    path_template
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .map(|segment| {
            let param = segment
                .strip_prefix('{')
                .and_then(|candidate| candidate.strip_suffix('}'));
            if let Some(name) = param {
                PathSegment::Param(name.to_string())
            } else {
                PathSegment::Static(segment.to_string())
            }
        })
        .collect()
}

fn strip_organization_prefix(segments: &[PathSegment]) -> Vec<PathSegment> {
    if should_strip_organization_prefix(segments) {
        return segments[2..].to_vec();
    }
    segments.to_vec()
}

fn should_strip_organization_prefix(segments: &[PathSegment]) -> bool {
    if segments.len() <= 2 {
        return false;
    }

    match (&segments[0], &segments[1], &segments[2]) {
        (
            PathSegment::Static(root),
            PathSegment::Param(id_name),
            PathSegment::Static(next_segment),
        ) => {
            root == "organizations"
                && id_name == "organization_id"
                && !next_segment.starts_with("update_")
        }
        _ => false,
    }
}

fn synthetic_action(method: &str, segments: &[PathSegment]) -> Option<&'static str> {
    if is_canonical_collection_path(segments) {
        return match method {
            "GET" => Some("list"),
            "POST" => Some("create"),
            _ => None,
        };
    }

    if is_canonical_item_path(segments) {
        return match method {
            "GET" => Some("get"),
            "PUT" | "PATCH" => Some("update"),
            "DELETE" => Some("delete"),
            _ => None,
        };
    }

    None
}

fn is_canonical_collection_path(segments: &[PathSegment]) -> bool {
    if segments.is_empty() || !matches!(segments.last(), Some(PathSegment::Static(_))) {
        return false;
    }

    segments.iter().enumerate().all(|(index, segment)| {
        matches!(
            (index % 2, segment),
            (0, PathSegment::Static(_)) | (1, PathSegment::Param(_))
        )
    })
}

fn is_canonical_item_path(segments: &[PathSegment]) -> bool {
    if segments.len() < 2 || !matches!(segments.last(), Some(PathSegment::Param(_))) {
        return false;
    }

    segments.iter().enumerate().all(|(index, segment)| {
        matches!(
            (index % 2, segment),
            (0, PathSegment::Static(_)) | (1, PathSegment::Param(_))
        )
    })
}

fn schema_hash(schema: &ApiSchema) -> String {
    if let Some(existing) = schema.schema_hash() {
        return existing.to_string();
    }

    let mut hasher = Sha256::new();
    for operation in schema.operations() {
        hasher.update(operation.id.as_bytes());
        hasher.update(b"\0");
        hasher.update(operation.method.as_bytes());
        hasher.update(b"\0");
        hasher.update(operation.path_template.as_bytes());
        hasher.update(b"\0");
    }
    let mut hex = String::with_capacity(64);
    {
        use std::fmt::Write as _;
        for byte in hasher.finalize() {
            write!(&mut hex, "{byte:02x}").expect("writing to String should not fail");
        }
    }
    hex
}

fn read_cache() -> Option<CommandIndexCache> {
    let content = fs::read_to_string(Config::schema_command_index_path()).ok()?;
    serde_json::from_str(&content).ok()
}

fn write_cache(cache: &CommandIndexCache) -> Result<(), CliError> {
    let schema_dir = Config::schema_dir();
    if !schema_dir.exists() {
        fs::create_dir_all(&schema_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&schema_dir, fs::Permissions::from_mode(0o700))?;
        }
    }

    let content = serde_json::to_vec_pretty(cache)
        .map_err(|e| CliError::Config(format!("command index serialize failed: {e}")))?;
    write_atomic(&Config::schema_command_index_path(), &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::Operation;

    fn op(id: &str, method: &str, path: &str) -> Operation {
        Operation {
            id: id.to_string(),
            method: method.to_string(),
            path_template: path.to_string(),
            summary: None,
            description: None,
            tags: Vec::new(),
            parameters: Vec::new(),
        }
    }

    #[test]
    fn build_entry_maps_projects_list() {
        let entry =
            build_entry(&op("x", "GET", "/organizations/{organization_id}/projects")).unwrap();
        assert_eq!(entry.command_words, vec!["projects", "list"]);
        assert!(entry.visible_path_params.is_empty());
    }

    #[test]
    fn build_entry_keeps_non_standard_literal_action() {
        let entry = build_entry(&op("x", "PUT", "/teams/{team_id}/update_members")).unwrap();
        assert_eq!(entry.command_words, vec!["teams", "update_members"]);
        assert_eq!(entry.visible_path_params, vec!["team_id"]);
    }

    // Snapshot of the full command table built from the committed live schema.
    // Refresh via `just refresh-schema-fixture` when the API evolves; the diff
    // on this snapshot is the review surface for command-shape drift.
    #[test]
    fn schema_command_table_snapshot() {
        use crate::schema::{ApiSchema, SchemaSource};
        use serde_json::Value;

        let raw = include_str!("../tests/fixtures/schema.json");
        let value: Value = serde_json::from_str(raw).expect("fixture is valid JSON");
        let schema = ApiSchema::from_schema(&value, SchemaSource::Cache, None)
            .expect("fixture parses into ApiSchema");

        let entries = build_entries(&schema);
        let mut lines = entries
            .iter()
            .map(|entry| {
                format!(
                    "{:6} {:60} -> {}",
                    entry.method,
                    entry.path_template,
                    usage_line(entry)
                )
            })
            .collect::<Vec<_>>();
        lines.sort();
        let table = lines.join("\n");

        insta::assert_snapshot!(table);
    }

    fn entry(words: &[&str], ids: &[&str], method: &str, op_id: &str) -> CommandEntry {
        CommandEntry {
            operation_id: op_id.to_string(),
            method: method.to_string(),
            path_template: format!("/{}", words.join("/")),
            command_words: words.iter().map(|value| (*value).to_string()).collect(),
            visible_path_params: ids.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn resolve_matched_returns_single_candidate() {
        let index = CommandIndex::from_entries(vec![entry(
            &["teams", "update_members"],
            &["team_id"],
            "PUT",
            "putTeamsUpdateMembers",
        )]);
        let positionals = vec![
            "teams".to_string(),
            "update_members".to_string(),
            "42".to_string(),
        ];

        match index.resolve(&positionals) {
            ResolveResult::Matched(found) => {
                assert_eq!(found.command_depth, 2);
                assert_eq!(found.entry.operation_id, "putTeamsUpdateMembers");
            }
            _ => panic!("expected matched result"),
        }
    }

    #[test]
    fn resolve_shape_mismatch_when_missing_path_arg() {
        let index = CommandIndex::from_entries(vec![entry(
            &["teams", "update_members"],
            &["team_id"],
            "PUT",
            "putTeamsUpdateMembers",
        )]);
        let positionals = vec!["teams".to_string(), "update_members".to_string()];

        match index.resolve(&positionals) {
            ResolveResult::ShapeMismatch {
                provided_path_count,
                candidates,
                ..
            } => {
                assert_eq!(provided_path_count, 0);
                assert_eq!(candidates.len(), 1);
                assert_eq!(candidates[0].visible_path_params, vec!["team_id"]);
            }
            _ => panic!("expected shape mismatch result"),
        }
    }

    #[test]
    fn resolve_ambiguous_when_multiple_exact_candidates() {
        let index = CommandIndex::from_entries(vec![
            entry(&["users", "me"], &[], "GET", "getUsersMe"),
            entry(&["users", "me"], &[], "POST", "postUsersMe"),
        ]);
        let positionals = vec!["users".to_string(), "me".to_string()];

        match index.resolve(&positionals) {
            ResolveResult::Ambiguous { candidates, .. } => {
                assert_eq!(candidates.len(), 2);
            }
            _ => panic!("expected ambiguous result"),
        }
    }

    #[test]
    fn resolve_unknown_includes_suggestions_by_prefix() {
        let index = CommandIndex::from_entries(vec![
            entry(&["projects", "list"], &[], "GET", "getProjects"),
            entry(&["projects", "get"], &["project_id"], "GET", "getProject"),
        ]);
        let positionals = vec!["projects".to_string(), "missing".to_string()];

        match index.resolve(&positionals) {
            ResolveResult::Unknown { suggestions, .. } => {
                assert!(
                    suggestions.iter().any(|value| value == "projects list"),
                    "expected 'projects list' in suggestions, got {suggestions:?}"
                );
                assert!(
                    suggestions
                        .iter()
                        .any(|value| value == "projects get <project_id>"),
                    "expected 'projects get <project_id>' in suggestions, got {suggestions:?}"
                );
            }
            _ => panic!("expected unknown result"),
        }
    }
}
