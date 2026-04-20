use crate::client::HubstaffClient;
use crate::error::CliError;
use crate::output::CompactOutput;
use std::collections::HashMap;

pub fn list(
    client: &mut HubstaffClient,
    project_id: u64,
    json: bool,
    page_start: Option<u64>,
    page_limit: Option<u64>,
) -> Result<(), CliError> {
    let mut params = HashMap::new();
    if let Some(ps) = page_start {
        params.insert("page_start_id".to_string(), ps.to_string());
    }
    if let Some(pl) = page_limit {
        params.insert("page_limit".to_string(), pl.to_string());
    }

    let data = client.get(&format!("/projects/{project_id}/tasks"), &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::table(
        &data,
        "tasks",
        &[
            ("ID", "id"),
            ("SUMMARY", "summary"),
            ("STATUS", "status"),
            ("ASSIGNEE", "assignee_id"),
        ],
        "tasks",
        &format!("project:{project_id}"),
    );
    println!("{out}");
    Ok(())
}

pub fn show(client: &mut HubstaffClient, task_id: u64, json: bool) -> Result<(), CliError> {
    let data = client.get(&format!("/tasks/{task_id}"), &HashMap::new())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(task) = data.get("task") {
        let out = CompactOutput::details(
            task,
            &[
                ("ID", "id"),
                ("Summary", "summary"),
                ("Status", "status"),
                ("Assignee", "assignee_id"),
                ("Project", "project_id"),
                ("Created", "created_at"),
            ],
        );
        print!("{out}");
    }
    Ok(())
}

pub fn create(
    client: &mut HubstaffClient,
    project_id: u64,
    summary: &str,
    assignee_id: Option<u64>,
    json: bool,
) -> Result<(), CliError> {
    let mut body = serde_json::json!({ "summary": summary });
    if let Some(aid) = assignee_id {
        body["assignee_id"] = serde_json::json!(aid);
    }

    let data = client.post(&format!("/projects/{project_id}/tasks"), &body)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(task) = data.get("task") {
        let out = CompactOutput::one_liner(
            "created",
            &[
                ("task", format!("{}", task["id"])),
                (
                    "summary",
                    task["summary"].as_str().unwrap_or("-").to_string(),
                ),
                ("project", project_id.to_string()),
            ],
        );
        println!("{out}");
    }
    Ok(())
}
