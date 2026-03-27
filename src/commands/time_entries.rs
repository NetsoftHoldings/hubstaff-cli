use crate::client::HubstaffClient;
use crate::error::CliError;
use crate::output::CompactOutput;

pub fn create(
    client: &mut HubstaffClient,
    project_id: u64,
    start: &str,
    stop: &str,
    json: bool,
) -> Result<(), CliError> {
    let body = serde_json::json!({
        "project_id": project_id,
        "started_at": normalize_timestamp(start),
        "stopped_at": normalize_timestamp(stop)
    });

    let data = client.post("/users/me/time_entries", &body)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(entry) = data.get("time_entry") {
        let out = CompactOutput::one_liner("created", &[
            ("time_entry", format!("{}", entry["id"])),
            ("project", project_id.to_string()),
            ("start", entry["started_at"].as_str().unwrap_or("-").to_string()),
            ("stop", entry["stopped_at"].as_str().unwrap_or("-").to_string()),
        ]);
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
