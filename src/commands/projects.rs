use crate::client::HubstaffClient;
use crate::error::CliError;
use crate::output::CompactOutput;
use std::collections::HashMap;

pub fn list(
    client: &mut HubstaffClient,
    org_id: u64,
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

    let data = client.get(&format!("/organizations/{org_id}/projects"), &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::table(
        &data,
        "projects",
        &[("ID", "id"), ("NAME", "name"), ("STATUS", "status")],
        "projects",
        &format!("org:{org_id}"),
    );
    println!("{out}");
    Ok(())
}

pub fn show(client: &mut HubstaffClient, project_id: u64, json: bool) -> Result<(), CliError> {
    let data = client.get(&format!("/projects/{project_id}"), &HashMap::new())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(project) = data.get("project") {
        let out = CompactOutput::details(project, &[
            ("ID", "id"),
            ("Name", "name"),
            ("Status", "status"),
            ("Created", "created_at"),
        ]);
        print!("{out}");
    }
    Ok(())
}

pub fn create(
    client: &mut HubstaffClient,
    org_id: u64,
    name: &str,
    json: bool,
) -> Result<(), CliError> {
    let body = serde_json::json!({ "name": name });
    let data = client.post(&format!("/organizations/{org_id}/projects"), &body)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(project) = data.get("project") {
        let out = CompactOutput::one_liner("created", &[
            ("project", format!("{}", project["id"])),
            ("name", project["name"].as_str().unwrap_or("-").to_string()),
            ("status", project["status"].as_str().unwrap_or("-").to_string()),
        ]);
        println!("{out}");
    }
    Ok(())
}
