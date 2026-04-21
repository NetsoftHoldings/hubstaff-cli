use crate::command_index::{CommandEntry, CommandIndex, usage_line};
use crate::config::Config;
use crate::error::CliError;
use crate::schema::ApiSchema;
use std::collections::BTreeMap;
use std::fmt::Write as _;

pub fn list() -> Result<(), CliError> {
    let cfg = Config::load()?;
    let schema = ApiSchema::load(&cfg)?;
    let index = CommandIndex::load_or_build(&schema)?;

    let output = render(index.entries(), |operation_id| {
        schema
            .operation(operation_id)
            .and_then(|operation| operation.summary.as_deref())
    });
    print!("{output}");
    Ok(())
}

fn render<'a>(entries: &'a [CommandEntry], summary: impl Fn(&str) -> Option<&'a str>) -> String {
    let mut groups: BTreeMap<&str, Vec<&CommandEntry>> = BTreeMap::new();
    for entry in entries {
        if let Some(head) = entry.command_words.first() {
            groups.entry(head.as_str()).or_default().push(entry);
        }
    }

    let mut out = String::new();
    for (index, (resource, mut group_entries)) in groups.into_iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }

        group_entries.sort_by_key(|entry| usage_line(entry));

        let rendered = group_entries
            .iter()
            .map(|entry| (usage_line(entry), summary(&entry.operation_id)))
            .collect::<Vec<_>>();
        let width = rendered
            .iter()
            .map(|(usage, _)| usage.len())
            .max()
            .unwrap_or(0);

        writeln!(&mut out, "{resource}:").expect("write to String");
        for (usage, description) in rendered {
            match description {
                Some(text) => {
                    writeln!(&mut out, "  {usage:<width$}  {text}").expect("write to String");
                }
                None => writeln!(&mut out, "  {usage}").expect("write to String"),
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(words: &[&str], params: &[&str], op_id: &str) -> CommandEntry {
        CommandEntry {
            operation_id: op_id.to_string(),
            method: "GET".to_string(),
            path_template: format!("/{}", words.join("/")),
            command_words: words.iter().map(|value| (*value).to_string()).collect(),
            visible_path_params: params.iter().map(|value| (*value).to_string()).collect(),
        }
    }

    #[test]
    fn groups_by_resource_aligns_summaries() {
        let entries = vec![
            entry(&["projects", "list"], &[], "getProjects"),
            entry(&["projects", "get"], &["project_id"], "getProject"),
            entry(&["teams", "list"], &[], "getTeams"),
        ];
        let summaries: BTreeMap<&str, &str> = [
            ("getProjects", "List projects"),
            ("getProject", "Get a single project"),
            ("getTeams", "List teams"),
        ]
        .into_iter()
        .collect();

        let output = render(&entries, |id| summaries.get(id).copied());

        let expected = "\
projects:
  projects get <project_id>  Get a single project
  projects list              List projects

teams:
  teams list  List teams
";
        assert_eq!(output, expected);
    }

    #[test]
    fn omits_summary_column_when_missing() {
        let entries = vec![entry(&["users", "me"], &[], "getUsersMe")];
        let output = render(&entries, |_| None);
        assert_eq!(output, "users:\n  users me\n");
    }
}
