use crate::client::HubstaffClient;
use crate::error::CliError;
use crate::output::CompactOutput;
use std::collections::HashMap;

pub fn list(
    client: &mut HubstaffClient,
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

    let data = client.get("/organizations", &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::table(
        &data,
        "organizations",
        &[("ID", "id"), ("NAME", "name"), ("STATUS", "status")],
        "organizations",
        "",
    );
    println!("{out}");
    Ok(())
}

pub fn show(client: &mut HubstaffClient, org_id: u64, json: bool) -> Result<(), CliError> {
    let data = client.get(&format!("/organizations/{org_id}"), &HashMap::new())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(org) = data.get("organization") {
        let out = CompactOutput::details(
            org,
            &[
                ("ID", "id"),
                ("Name", "name"),
                ("Status", "status"),
                ("Created", "created_at"),
            ],
        );
        print!("{out}");
    }
    Ok(())
}
