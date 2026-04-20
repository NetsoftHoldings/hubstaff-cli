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

    let data = client.get(&format!("/organizations/{org_id}/teams"), &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::table(
        &data,
        "teams",
        &[("ID", "id"), ("NAME", "name")],
        "teams",
        &format!("org:{org_id}"),
    );
    println!("{out}");
    Ok(())
}

pub fn show(client: &mut HubstaffClient, team_id: u64, json: bool) -> Result<(), CliError> {
    let data = client.get(&format!("/teams/{team_id}"), &HashMap::new())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(team) = data.get("team") {
        let out = CompactOutput::details(
            team,
            &[("ID", "id"), ("Name", "name"), ("Created", "created_at")],
        );
        print!("{out}");
    }
    Ok(())
}
