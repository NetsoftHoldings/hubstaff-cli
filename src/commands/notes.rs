use crate::client::HubstaffClient;
use crate::error::CliError;
use crate::output::CompactOutput;
use std::collections::HashMap;

pub fn list(
    client: &mut HubstaffClient,
    org_id: u64,
    start: &str,
    stop: Option<&str>,
    json: bool,
    page_start: Option<u64>,
    page_limit: Option<u64>,
) -> Result<(), CliError> {
    let start_ts = normalize_timestamp(start);
    let stop_ts = stop.map_or_else(|| chrono::Utc::now().to_rfc3339(), normalize_timestamp);

    let mut params = HashMap::new();
    params.insert("time_slot[start]".to_string(), start_ts);
    params.insert("time_slot[stop]".to_string(), stop_ts);
    if let Some(ps) = page_start {
        params.insert("page_start_id".to_string(), ps.to_string());
    }
    if let Some(pl) = page_limit {
        params.insert("page_limit".to_string(), pl.to_string());
    }

    let data = client.get(&format!("/organizations/{org_id}/notes"), &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::table(
        &data,
        "notes",
        &[
            ("ID", "id"),
            ("DESCRIPTION", "description"),
            ("USER_ID", "user_id"),
            ("PROJECT_ID", "project_id"),
        ],
        "notes",
        &format!("org:{org_id}"),
    );
    println!("{out}");
    Ok(())
}

pub fn create(
    client: &mut HubstaffClient,
    project_id: u64,
    description: &str,
    recorded_time: &str,
    json: bool,
) -> Result<(), CliError> {
    let body = serde_json::json!({
        "project_id": project_id,
        "description": description,
        "recorded_time": normalize_timestamp(recorded_time)
    });

    let data = client.post("/users/me/notes", &body)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(note) = data.get("note") {
        let out = CompactOutput::one_liner(
            "created",
            &[
                ("note", format!("{}", note["id"])),
                ("project", project_id.to_string()),
            ],
        );
        println!("{out}");
    }
    Ok(())
}

fn normalize_timestamp(input: &str) -> String {
    if input.contains('T') {
        input.to_string()
    } else {
        format!("{input}T00:00:00Z")
    }
}
