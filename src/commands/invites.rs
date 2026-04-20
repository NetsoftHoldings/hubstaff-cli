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
    status_filter: Option<&str>,
) -> Result<(), CliError> {
    let mut params = HashMap::new();
    if let Some(ps) = page_start {
        params.insert("page_start_id".to_string(), ps.to_string());
    }
    if let Some(pl) = page_limit {
        params.insert("page_limit".to_string(), pl.to_string());
    }
    if let Some(s) = status_filter {
        params.insert("status".to_string(), s.to_string());
    }

    let data = client.get(&format!("/organizations/{org_id}/invites"), &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::table(
        &data,
        "invites",
        &[
            ("ID", "id"),
            ("EMAIL", "email"),
            ("ROLE", "role"),
            ("STATUS", "status"),
        ],
        "invites",
        &format!("org:{org_id}"),
    );
    println!("{out}");
    Ok(())
}

pub fn show(client: &mut HubstaffClient, invite_id: u64, json: bool) -> Result<(), CliError> {
    let data = client.get(&format!("/invites/{invite_id}"), &HashMap::new())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(invite) = data.get("invite") {
        let out = CompactOutput::details(
            invite,
            &[
                ("ID", "id"),
                ("Email", "email"),
                ("Role", "role"),
                ("Status", "status"),
                ("Created", "created_at"),
            ],
        );
        print!("{out}");
    }
    Ok(())
}

pub fn create(
    client: &mut HubstaffClient,
    org_id: u64,
    email: &str,
    role: Option<&str>,
    project_ids: &[u64],
    json: bool,
) -> Result<(), CliError> {
    let mut body = serde_json::json!({ "email": email });
    if let Some(r) = role {
        body["role"] = serde_json::Value::String(r.to_string());
    }
    if !project_ids.is_empty() {
        body["project_ids"] = serde_json::json!(project_ids);
    }

    let data = client.post(&format!("/organizations/{org_id}/invites"), &body)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(invite) = data.get("invite") {
        let out = CompactOutput::one_liner(
            "created",
            &[
                ("invite", format!("{}", invite["id"])),
                ("email", email.to_string()),
                ("role", invite["role"].as_str().unwrap_or("-").to_string()),
                (
                    "status",
                    invite["status"].as_str().unwrap_or("-").to_string(),
                ),
            ],
        );
        println!("{out}");
    }
    Ok(())
}

pub fn delete(client: &mut HubstaffClient, invite_id: u64, json: bool) -> Result<(), CliError> {
    let data = client.delete(&format!("/invites/{invite_id}"))?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    println!("deleted invite:{invite_id}");
    Ok(())
}
