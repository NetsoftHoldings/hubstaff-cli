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
    let start_date = normalize_date(start);
    let stop_date = stop
        .map(normalize_date)
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());

    let mut params = HashMap::new();
    params.insert("date[start]".to_string(), start_date);
    params.insert("date[stop]".to_string(), stop_date);
    if let Some(ps) = page_start {
        params.insert("page_start_id".to_string(), ps.to_string());
    }
    if let Some(pl) = page_limit {
        params.insert("page_limit".to_string(), pl.to_string());
    }

    let data = client.get(
        &format!("/organizations/{org_id}/activities/daily"),
        &params,
    )?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::table(
        &data,
        "daily_activities",
        &[
            ("DATE", "date"),
            ("USER_ID", "user_id"),
            ("PROJECT_ID", "project_id"),
            ("TRACKED", "tracked"),
            ("KEYBOARD", "keyboard"),
            ("MOUSE", "mouse"),
        ],
        "daily activities",
        &format!("org:{org_id}"),
    );
    println!("{out}");
    Ok(())
}

/// Extract just the date portion if a timestamp is provided
fn normalize_date(input: &str) -> String {
    if input.contains('T') {
        input.split('T').next().unwrap_or(input).to_string()
    } else {
        input.to_string()
    }
}
