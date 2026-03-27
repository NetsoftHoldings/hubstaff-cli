use serde_json::Value;
use std::fmt::Write;

pub struct CompactOutput;

impl CompactOutput {
    /// Render a table from a JSON array with specified columns.
    /// `columns` is a list of (header_name, json_key) pairs.
    /// `root_key` is the key in the response that holds the array (e.g., "organizations").
    /// Returns formatted string with header, rows, and footer.
    pub fn table(
        data: &Value,
        root_key: &str,
        columns: &[(&str, &str)],
        footer_label: &str,
        footer_context: &str,
    ) -> String {
        let items = match data.get(root_key).and_then(|v| v.as_array()) {
            Some(arr) => arr,
            None => return format!("0 {footer_label} | {footer_context}"),
        };

        if items.is_empty() {
            return format!("0 {footer_label} | {footer_context}");
        }

        // Calculate column widths
        let mut widths: Vec<usize> = columns.iter().map(|(h, _)| h.len()).collect();
        let rows: Vec<Vec<String>> = items
            .iter()
            .map(|item| {
                columns
                    .iter()
                    .enumerate()
                    .map(|(i, (_, key))| {
                        let val = extract_value(item, key);
                        widths[i] = widths[i].max(val.len());
                        val
                    })
                    .collect()
            })
            .collect();

        let mut out = String::new();

        // Header
        for (i, (header, _)) in columns.iter().enumerate() {
            if i > 0 {
                out.push('\t');
            }
            let _ = write!(out, "{:<width$}", header, width = widths[i]);
        }
        out.push('\n');

        // Rows
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    out.push('\t');
                }
                let _ = write!(out, "{:<width$}", cell, width = widths[i]);
            }
            out.push('\n');
        }

        // Footer
        let count = rows.len();
        let mut footer = if footer_context.is_empty() {
            format!("{count} {footer_label}")
        } else {
            format!("{count} {footer_label} | {footer_context}")
        };

        // Pagination hint
        if let Some(pagination) = data.get("pagination")
            && let Some(next_id) = pagination.get("next_page_start_id").and_then(|v| v.as_u64())
        {
            let _ = write!(footer, " | next: --page-start {next_id}");
        }

        out.push_str(&footer);
        out
    }

    /// Render a single record as a one-liner with key:value pairs.
    pub fn one_liner(prefix: &str, fields: &[(&str, String)]) -> String {
        let mut parts = vec![prefix.to_string()];
        for (key, val) in fields {
            parts.push(format!("{key}:{val}"));
        }
        parts.join(" | ")
    }

    /// Render a single record's details as key-value pairs (for show commands).
    pub fn details(data: &Value, fields: &[(&str, &str)]) -> String {
        let mut out = String::new();
        for (label, key) in fields {
            let val = extract_value(data, key);
            let _ = writeln!(out, "{label}: {val}");
        }
        out
    }
}

fn extract_value(item: &Value, key: &str) -> String {
    // Support nested keys with dot notation (e.g., "user.name")
    let value = if key.contains('.') {
        let parts: Vec<&str> = key.split('.').collect();
        let mut current = item;
        for part in parts {
            current = match current.get(part) {
                Some(v) => v,
                None => return String::from("-"),
            };
        }
        current
    } else {
        match item.get(key) {
            Some(v) => v,
            None => return String::from("-"),
        }
    };

    match value {
        Value::String(s) => {
            // Truncate long strings (char-safe to avoid multi-byte panic)
            if s.chars().count() > 50 {
                let truncated: String = s.chars().take(47).collect();
                format!("{truncated}...")
            } else {
                s.clone()
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::from("-"),
        _ => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn table_with_data() {
        let data = json!({
            "items": [
                {"id": 1, "name": "Alice"},
                {"id": 2, "name": "Bob"}
            ]
        });
        let out = CompactOutput::table(
            &data,
            "items",
            &[("ID", "id"), ("NAME", "name")],
            "items",
            "test",
        );
        assert!(out.contains("ID"));
        assert!(out.contains("Alice"));
        assert!(out.contains("Bob"));
        assert!(out.contains("2 items | test"));
    }

    #[test]
    fn table_empty_array() {
        let data = json!({"items": []});
        let out = CompactOutput::table(&data, "items", &[("ID", "id")], "items", "org:1");
        assert_eq!(out, "0 items | org:1");
    }

    #[test]
    fn table_missing_root_key() {
        let data = json!({"other": []});
        let out = CompactOutput::table(&data, "items", &[("ID", "id")], "items", "org:1");
        assert_eq!(out, "0 items | org:1");
    }

    #[test]
    fn table_empty_footer_context() {
        let data = json!({"orgs": [{"id": 1}]});
        let out = CompactOutput::table(&data, "orgs", &[("ID", "id")], "organizations", "");
        assert!(out.contains("1 organizations"));
        assert!(!out.contains("| \n"));
    }

    #[test]
    fn table_with_pagination() {
        let data = json!({
            "items": [{"id": 1}],
            "pagination": {"next_page_start_id": 42}
        });
        let out = CompactOutput::table(&data, "items", &[("ID", "id")], "items", "test");
        assert!(out.contains("next: --page-start 42"));
    }

    #[test]
    fn table_without_pagination() {
        let data = json!({"items": [{"id": 1}]});
        let out = CompactOutput::table(&data, "items", &[("ID", "id")], "items", "test");
        assert!(!out.contains("next:"));
    }

    #[test]
    fn one_liner_format() {
        let out = CompactOutput::one_liner("created", &[
            ("id", "123".to_string()),
            ("name", "Test".to_string()),
        ]);
        assert_eq!(out, "created | id:123 | name:Test");
    }

    #[test]
    fn one_liner_empty_fields() {
        let out = CompactOutput::one_liner("done", &[]);
        assert_eq!(out, "done");
    }

    #[test]
    fn details_format() {
        let data = json!({"id": 42, "name": "Test Project"});
        let out = CompactOutput::details(&data, &[("ID", "id"), ("Name", "name")]);
        assert!(out.contains("ID: 42"));
        assert!(out.contains("Name: Test Project"));
    }

    #[test]
    fn details_missing_field() {
        let data = json!({"id": 42});
        let out = CompactOutput::details(&data, &[("ID", "id"), ("Name", "name")]);
        assert!(out.contains("Name: -"));
    }

    #[test]
    fn extract_value_string() {
        let item = json!({"name": "Alice"});
        assert_eq!(extract_value(&item, "name"), "Alice");
    }

    #[test]
    fn extract_value_number() {
        let item = json!({"id": 42});
        assert_eq!(extract_value(&item, "id"), "42");
    }

    #[test]
    fn extract_value_bool() {
        let item = json!({"active": true});
        assert_eq!(extract_value(&item, "active"), "true");
    }

    #[test]
    fn extract_value_null() {
        let item = json!({"val": null});
        assert_eq!(extract_value(&item, "val"), "-");
    }

    #[test]
    fn extract_value_missing_key() {
        let item = json!({"id": 1});
        assert_eq!(extract_value(&item, "missing"), "-");
    }

    #[test]
    fn extract_value_nested_dot_notation() {
        let item = json!({"user": {"name": "Alice", "email": "alice@test.com"}});
        assert_eq!(extract_value(&item, "user.name"), "Alice");
        assert_eq!(extract_value(&item, "user.email"), "alice@test.com");
    }

    #[test]
    fn extract_value_nested_missing() {
        let item = json!({"user": {"name": "Alice"}});
        assert_eq!(extract_value(&item, "user.email"), "-");
    }

    #[test]
    fn extract_value_nested_missing_parent() {
        let item = json!({"id": 1});
        assert_eq!(extract_value(&item, "user.name"), "-");
    }

    #[test]
    fn extract_value_truncates_long_strings() {
        let long = "a".repeat(60);
        let item = json!({"desc": long});
        let result = extract_value(&item, "desc");
        assert!(result.ends_with("..."));
        assert!(result.chars().count() == 50);
    }

    #[test]
    fn extract_value_does_not_truncate_short_strings() {
        let item = json!({"desc": "short text"});
        assert_eq!(extract_value(&item, "desc"), "short text");
    }

    #[test]
    fn extract_value_truncates_multibyte_safely() {
        // 60 emoji characters — each is multi-byte but should not panic
        let emoji_str: String = "🎉".repeat(60);
        let item = json!({"desc": emoji_str});
        let result = extract_value(&item, "desc");
        assert!(result.ends_with("..."));
        // Should be 47 emoji chars + "..." = 50 display chars
        assert_eq!(result.chars().count(), 50);
    }

    #[test]
    fn extract_value_exactly_50_chars() {
        let exactly_50 = "a".repeat(50);
        let item = json!({"desc": exactly_50});
        let result = extract_value(&item, "desc");
        assert_eq!(result.len(), 50);
        assert!(!result.contains("..."));
    }
}
