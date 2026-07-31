use crate::config::Config;
use crate::error::CliError;
use crate::persistence::write_atomic;
use crate::schema::{ApiSchema, Operation};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
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

    pub fn entries(&self) -> &[CommandEntry] {
        &self.entries
    }

    pub fn descendants(&self, prefix: &[String]) -> Option<Vec<&CommandEntry>> {
        let mut node = &self.trie;
        for token in prefix {
            node = node.children.get(token)?;
        }
        let mut indexes = Vec::new();
        collect_terminals(node, &mut indexes);
        let mut entries = indexes
            .into_iter()
            .filter_map(|index| self.entries.get(index))
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| usage_line(entry));
        Some(entries)
    }

    pub fn suggestions(&self, first_word: Option<&str>, limit: usize) -> Vec<String> {
        let usages = if let Some(word) = first_word
            && let Some(indexes) = self.by_first_word.get(word)
        {
            indexes
                .iter()
                .filter_map(|index| self.entries.get(*index))
                .map(usage_line)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        } else {
            self.entries
                .iter()
                .map(usage_line)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .take(limit)
                .collect::<Vec<_>>()
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

fn collect_terminals(node: &TrieNode, out: &mut Vec<usize>) {
    out.extend(node.terminals.iter().copied());
    for child in node.children.values() {
        collect_terminals(child, out);
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

    let mut hasher = ring::digest::Context::new(&ring::digest::SHA256);
    for operation in schema.operations() {
        hasher.update(operation.id.as_bytes());
        hasher.update(b"\0");
        hasher.update(operation.method.as_bytes());
        hasher.update(b"\0");
        hasher.update(operation.path_template.as_bytes());
        hasher.update(b"\0");
    }
    let digest = hasher.finish();
    let mut out = String::with_capacity(64);
    for byte in digest.as_ref() {
        write!(&mut out, "{byte:02x}").expect("write to String");
    }
    out
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
        use crate::schema::ApiSchema;
        use serde_json::Value;

        let raw = include_str!("../tests/fixtures/schema.json");
        let value: Value = serde_json::from_str(raw).expect("fixture is valid JSON");
        let schema = ApiSchema::from_schema(&value, None).expect("fixture parses into ApiSchema");

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
    fn descendants_returns_subtree_terminals_for_valid_prefix() {
        let index = CommandIndex::from_entries(vec![
            entry(&["activities", "list"], &[], "GET", "listActivities"),
            entry(&["activities", "daily"], &[], "GET", "listDailyActivities"),
            entry(
                &["activities", "daily", "updates"],
                &[],
                "GET",
                "listDailyActivityUpdates",
            ),
            entry(&["projects", "list"], &[], "GET", "listProjects"),
        ]);

        let under_activities = index
            .descendants(&["activities".to_string()])
            .expect("activities prefix should be known");
        let usages = under_activities
            .iter()
            .map(|e| usage_line(e))
            .collect::<Vec<_>>();
        assert_eq!(
            usages,
            vec![
                "activities daily",
                "activities daily updates",
                "activities list"
            ]
        );

        let under_daily = index
            .descendants(&["activities".to_string(), "daily".to_string()])
            .expect("activities daily prefix should be known");
        let usages = under_daily
            .iter()
            .map(|e| usage_line(e))
            .collect::<Vec<_>>();
        assert_eq!(usages, vec!["activities daily", "activities daily updates"]);
    }

    #[test]
    fn descendants_returns_none_for_unknown_prefix() {
        let index = CommandIndex::from_entries(vec![entry(
            &["projects", "list"],
            &[],
            "GET",
            "listProjects",
        )]);

        assert!(index.descendants(&["mystery".to_string()]).is_none());
        assert!(
            index
                .descendants(&["projects".to_string(), "bogus".to_string()])
                .is_none()
        );
    }

    #[test]
    fn descendants_with_empty_prefix_returns_all_entries() {
        let index = CommandIndex::from_entries(vec![
            entry(&["projects", "list"], &[], "GET", "listProjects"),
            entry(&["users", "me"], &[], "GET", "getUsersMe"),
        ]);

        let all = index.descendants(&[]).expect("empty prefix is valid");
        assert_eq!(all.len(), 2);
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
    /// Agent Skills limits, from the published SKILL.md contract.
    const MAX_SKILL_NAME_LEN: usize = 64;
    const MAX_SKILL_DESCRIPTION_LEN: usize = 1024;

    fn skills_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skills")
    }

    /// Every `skills/*/SKILL.md`, parsed. Asserts the directory exists rather than returning
    /// early: `skills/` is committed, so a missing directory means the check is running
    /// against the wrong tree, which must fail loudly instead of passing vacuously.
    fn skill_manifests() -> Vec<(String, std::path::PathBuf, SkillManifest)> {
        let dir = skills_dir();
        assert!(
            dir.is_dir(),
            "skills/ is committed and must exist at {}",
            dir.display()
        );
        let mut manifests = Vec::new();
        for entry in std::fs::read_dir(&dir).expect("skills dir is readable") {
            let path = entry.expect("readable dir entry").path();
            let manifest = path.join("SKILL.md");
            if !manifest.is_file() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .expect("skill dir has a utf-8 name")
                .to_string();
            let text = std::fs::read_to_string(&manifest).expect("SKILL.md is readable");
            manifests.push((name, path, SkillManifest::parse(&text)));
        }
        assert!(!manifests.is_empty(), "no SKILL.md found under skills/");
        manifests
    }

    #[test]
    fn skills_manifests_are_well_formed() {
        for (dir_name, _, skill) in skill_manifests() {
            assert_eq!(
                skill.name.as_deref(),
                Some(dir_name.as_str()),
                "{dir_name}/SKILL.md: top-level `name` must match the directory name"
            );
            assert!(
                skill.name.as_deref().is_some_and(is_valid_skill_name),
                "{dir_name}/SKILL.md: `name` must be <={MAX_SKILL_NAME_LEN} chars of lowercase letters, digits and hyphens, and must not contain 'anthropic' or 'claude'"
            );
            let description_len = skill.description.chars().count();
            assert!(
                (1..=MAX_SKILL_DESCRIPTION_LEN).contains(&description_len),
                "{dir_name}/SKILL.md: `description` must be 1..={MAX_SKILL_DESCRIPTION_LEN} chars, got {description_len}"
            );
            assert!(
                !skill.commands.is_empty(),
                "{dir_name}/SKILL.md: declare the CLI commands used under `metadata.commands`"
            );
        }
    }

    /// Skills declare, in their frontmatter, every CLI command they instruct an agent to run.
    /// Those commands are synthesized from the API schema rather than hand-written, so a change
    /// in path-to-command derivation can silently invalidate a published skill. This makes that
    /// failure loud.
    ///
    /// Runs offline against `tests/fixtures/schema.json`. Refresh it with
    /// `just refresh-schema-fixture` when the API evolves — the diff is the review surface.
    #[test]
    fn skills_declared_commands_resolve_against_schema() {
        use crate::schema::ApiSchema;
        use serde_json::Value;
        use std::collections::HashSet;

        let raw = include_str!("../tests/fixtures/schema.json");
        let value: Value = serde_json::from_str(raw).expect("fixture is valid JSON");
        let schema = ApiSchema::from_schema(&value, None).expect("fixture parses into ApiSchema");
        let known: HashSet<String> = build_entries(&schema)
            .iter()
            .map(|entry| entry.command_words.join(" "))
            .collect();

        let mut checked = 0_usize;
        for (dir_name, _, skill) in skill_manifests() {
            for command in &skill.commands {
                assert!(
                    known.contains(command),
                    "{dir_name}/SKILL.md declares `hubstaff {command}`, which no longer resolves. \
                     Either the skill is stale or command synthesis changed; run `hubstaff list` \
                     and update the skill."
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "no skill commands were checked; the guard is not doing anything"
        );
    }

    #[test]
    fn skills_examples_are_valid_json() {
        use serde_json::Value;

        for (_, path, _) in skill_manifests() {
            let examples = path.join("examples");
            if !examples.is_dir() {
                continue;
            }
            for file in std::fs::read_dir(&examples).expect("examples dir is readable") {
                let example = file.expect("readable example entry").path();
                if example.extension().is_some_and(|ext| ext == "json") {
                    let body = std::fs::read_to_string(&example).expect("example is readable");
                    assert!(
                        serde_json::from_str::<Value>(&body).is_ok(),
                        "{}: not valid JSON",
                        example.display()
                    );
                }
            }
        }
    }

    // ── SkillManifest::parse ────────────────────────────────────────────────────────────────
    // These lock the behaviours the guard depends on. Each corresponds to a real bug found in
    // review: without them, a later simplification of the parser silently reintroduces one and
    // the guard keeps passing while checking the wrong thing.

    fn manifest(front: &str) -> SkillManifest {
        SkillManifest::parse(&format!("---\n{front}---\n\n# body\n"))
    }

    #[test]
    fn parse_block_scalar_stops_at_the_next_top_level_key() {
        // Previously `license: MIT` was appended to the description, so the length lint measured
        // the wrong string and any unrecognised key silently landed inside it.
        let skill = manifest(
            "name: x\ndescription: >-\n  hello\nlicense: MIT\nmetadata:\n  commands:\n    - projects list\n",
        );
        assert_eq!(skill.description, "hello");
    }

    #[test]
    fn parse_ignores_trailing_whitespace_when_measuring_indent() {
        // With `trim()` instead of `trim_start()`, the trailing spaces made this key look indented,
        // so it failed to terminate the block scalar above it.
        let skill = manifest(
            "name: x\ndescription: >-\n  hello\nlicense: MIT   \nmetadata:\n  commands:\n    - projects list\n",
        );
        assert_eq!(skill.description, "hello");
    }

    #[test]
    fn parse_handles_crlf_line_endings() {
        let skill = SkillManifest::parse(
            "---\r\nname: x\r\ndescription: d\r\nmetadata:\r\n  commands:\r\n    - projects list\r\n---\r\n",
        );
        assert_eq!(skill.name.as_deref(), Some("x"));
        assert_eq!(skill.commands, vec!["projects list"]);
    }

    #[test]
    fn parse_requires_a_closing_delimiter() {
        let skill = SkillManifest::parse("---\nname: x\ndescription: d\n");
        assert_eq!(skill.name, None, "an unterminated block is not frontmatter");
    }

    #[test]
    fn parse_requires_name_and_description_at_the_top_level() {
        // A nested `metadata.name` does not satisfy the documented contract.
        let skill =
            manifest("metadata:\n  name: x\n  description: d\n  commands:\n    - projects list\n");
        assert_eq!(skill.name, None);
        assert!(skill.description.is_empty());
    }

    #[test]
    fn parse_rejects_commands_at_the_top_level() {
        let skill = manifest("name: x\ndescription: d\ncommands:\n  - projects list\n");
        assert!(
            skill.commands.is_empty(),
            "`commands` is only meaningful under `metadata`"
        );
    }

    #[test]
    fn parse_keeps_the_command_list_open_across_comments_and_blank_lines() {
        // Ending the list on a comment silently dropped every command below it.
        let skill = manifest(
            "name: x\ndescription: d\nmetadata:\n  commands:\n    - projects list\n    # a comment\n\n    - projects get\n",
        );
        assert_eq!(skill.commands, vec!["projects list", "projects get"]);
    }

    #[test]
    fn parse_accepts_quoted_command_items() {
        let skill = manifest(
            "name: x\ndescription: d\nmetadata:\n  commands:\n    - \"projects list\"\n    - 'projects get'\n",
        );
        assert_eq!(skill.commands, vec!["projects list", "projects get"]);
    }

    #[test]
    fn parse_closes_the_metadata_block_at_the_next_top_level_key() {
        let skill = manifest(
            "name: x\ndescription: d\nmetadata:\n  commands:\n    - projects list\nlicense: MIT\n  - projects get\n",
        );
        assert_eq!(
            skill.commands,
            vec!["projects list"],
            "items after the metadata block ends must not be collected"
        );
    }

    #[test]
    fn parse_collects_commands_only_directly_under_metadata() {
        // The documented contract is `metadata.commands`. A list buried deeper must not stand in
        // for it, or a manifest can violate the contract while the guard reports success.
        let skill = manifest(
            "name: x\ndescription: d\nmetadata:\n  other:\n    commands:\n      - projects list\n",
        );
        assert!(
            skill.commands.is_empty(),
            "only `metadata.commands` counts, not a list nested deeper"
        );
    }

    #[test]
    #[should_panic(expected = "malformed entry in `metadata.commands`")]
    fn parse_rejects_a_list_item_missing_the_space_after_the_dash() {
        // A single missing space used to truncate the list silently: a 16-entry manifest became a
        // 1-entry manifest and the guard still passed.
        manifest(
            "name: x\ndescription: d\nmetadata:\n  commands:\n    - projects list\n    -projects get\n    - projects delete\n",
        );
    }

    #[test]
    #[should_panic(expected = "malformed entry in `metadata.commands`")]
    fn parse_rejects_a_bare_dash_list_item() {
        manifest("name: x\ndescription: d\nmetadata:\n  commands:\n    - projects list\n    -\n");
    }

    #[test]
    fn parse_allows_a_sibling_key_after_the_command_list() {
        // `commands` is not always the last key in `metadata`; a sibling must end the list
        // cleanly rather than tripping the malformed-entry panic.
        let skill = manifest(
            "name: x\ndescription: d\nmetadata:\n  commands:\n    - projects list\n  scopes: \"read\"\n",
        );
        assert_eq!(skill.commands, vec!["projects list"]);
    }

    #[test]
    fn skill_name_rejects_reserved_words() {
        // Agent Skills forbids `anthropic` and `claude` in a skill name; a manifest using one
        // would pass our lint and then be rejected on upload.
        assert!(is_valid_skill_name("time-off-sync"));
        assert!(!is_valid_skill_name("claude-time-off"));
        assert!(!is_valid_skill_name("anthropic-sync"));
        assert!(!is_valid_skill_name("Uppercase"));
        assert!(!is_valid_skill_name(""));
        assert!(!is_valid_skill_name(&"a".repeat(65)));
    }

    fn is_valid_skill_name(name: &str) -> bool {
        !name.is_empty()
            && name.chars().count() <= MAX_SKILL_NAME_LEN
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
            // Agent Skills reserves these; a manifest using one passes a charset check and is
            // then rejected on upload.
            && !name.contains("anthropic")
            && !name.contains("claude")
    }

    /// Minimal reader for the flat subset of SKILL.md frontmatter this guard checks. Deliberately
    /// not a YAML parser: keeping the linted fields flat avoids taking on a YAML dependency for a
    /// test, and keeps the failure mode obvious when a contributor nests something unexpected.
    #[derive(Default)]
    struct SkillManifest {
        name: Option<String>,
        description: String,
        commands: Vec<String>,
    }

    impl SkillManifest {
        fn parse(text: &str) -> Self {
            // Normalise line endings first: `strip_prefix("---\n")` misses on a CRLF file, which
            // would yield an empty manifest and a failure blaming `name` instead of the encoding.
            let normalized = text.replace("\r\n", "\n");
            let mut manifest = Self::default();
            let Some(body) = normalized.strip_prefix("---\n") else {
                return manifest;
            };
            // A frontmatter block with no closing delimiter is malformed. Bail instead of
            // treating the rest of the document as frontmatter, which would let a broken
            // manifest satisfy this guard.
            let Some((front, _)) = body.split_once("\n---") else {
                return manifest;
            };

            // Indent of the `description` key while its block scalar is open.
            let mut description_indent: Option<usize> = None;
            // Indent of the `commands:` key while its list is open.
            let mut commands_indent: Option<usize> = None;
            // Indent of the first key directly inside `metadata:`, i.e. the level `commands`
            // must appear at.
            let mut metadata_child_indent: Option<usize> = None;
            // `commands` is only meaningful nested under `metadata`, so track where that
            // block starts and how far it extends.
            let mut metadata_indent: Option<usize> = None;
            for line in front.lines() {
                let trimmed = line.trim();
                // trim_start only: trailing whitespace must not inflate the indent, or a
                // top-level key with trailing spaces fails to close an enclosing block.
                let indent = line.len() - line.trim_start().len();

                // A block scalar ends at a blank line, or at any key drawn at or left of the
                // `description` key itself. Without the latter, `license: MIT` and every
                // unrecognised key after it get appended to the description.
                if let Some(level) = description_indent {
                    if trimmed.is_empty() || (indent <= level && is_yaml_key_line(trimmed)) {
                        description_indent = None;
                    } else {
                        if !manifest.description.is_empty() {
                            manifest.description.push(' ');
                        }
                        manifest.description.push_str(trimmed);
                        continue;
                    }
                }

                if !trimmed.is_empty()
                    && trimmed != "metadata:"
                    && metadata_indent.is_some_and(|level| indent <= level)
                {
                    metadata_indent = None;
                    metadata_child_indent = None;
                    commands_indent = None;
                }

                // First key inside `metadata:` establishes the child level. `commands` has to sit
                // at exactly that level; a list buried deeper is not `metadata.commands`.
                if let Some(level) = metadata_indent
                    && metadata_child_indent.is_none()
                    && !trimmed.is_empty()
                    && indent > level
                    && is_yaml_key_line(trimmed)
                {
                    metadata_child_indent = Some(indent);
                }

                if trimmed == "metadata:" && indent == 0 {
                    metadata_indent = Some(indent);
                    metadata_child_indent = None;
                    commands_indent = None;
                } else if let Some(rest) = trimmed.strip_prefix("name:") {
                    // `name` and `description` are top-level keys. A nested `metadata.name`
                    // does not satisfy the documented contract and must not stand in for one.
                    if indent == 0 {
                        manifest.name = Some(rest.trim().trim_matches(['"', '\'']).to_string());
                    }
                } else if let Some(rest) = trimmed.strip_prefix("description:") {
                    if indent == 0 {
                        let inline = rest.trim();
                        if matches!(inline, "" | ">" | ">-" | ">+" | "|" | "|-" | "|+") {
                            description_indent = Some(indent);
                        } else {
                            manifest.description = inline.trim_matches(['"', '\'']).to_string();
                        }
                    }
                } else if trimmed == "commands:"
                    && metadata_child_indent.is_some_and(|level| indent == level)
                {
                    commands_indent = Some(indent);
                } else if let Some(level) = commands_indent {
                    if let Some(item) = trimmed.strip_prefix("- ") {
                        // Accept quoted items: `- "policies list"` is valid YAML and must not fail
                        // the guard for a manifest that is actually correct.
                        let item = item.trim().trim_matches(['"', '\'']);
                        manifest.commands.push(item.to_string());
                    } else if trimmed.is_empty() || trimmed.starts_with('#') {
                        // Comments and blank lines stay inside the list.
                    } else if indent <= level && is_yaml_key_line(trimmed) {
                        // A sibling key at or left of `commands:` legitimately ends the list.
                        commands_indent = None;
                    } else {
                        // Anything else is malformed — a missing space after the dash, a bare `-`,
                        // flow style. Silently ending the list here would drop every declaration
                        // below it and let the guard pass while checking almost nothing, which is
                        // the exact failure this guard exists to prevent.
                        panic!(
                            "malformed entry in `metadata.commands`: {trimmed:?}. \
                             Each entry must be a `- <command>` list item."
                        );
                    }
                }
            }
            manifest
        }
    }

    /// True for a line that opens a YAML mapping key, e.g. `license: MIT` or `metadata:`.
    fn is_yaml_key_line(trimmed: &str) -> bool {
        trimmed.split_once(':').is_some_and(|(key, _)| {
            !key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        })
    }
}
