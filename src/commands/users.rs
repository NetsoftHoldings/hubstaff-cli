use crate::client::HubstaffClient;
use crate::error::CliError;
use crate::output::CompactOutput;
use std::collections::HashMap;

pub fn me(client: &mut HubstaffClient, json: bool) -> Result<(), CliError> {
    let data = client.get("/users/me", &HashMap::new())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(user) = data.get("user") {
        let out = CompactOutput::details(user, &[
            ("ID", "id"),
            ("Name", "name"),
            ("Email", "email"),
            ("Time Zone", "time_zone"),
            ("Created", "created_at"),
        ]);
        print!("{out}");
    }
    Ok(())
}

pub fn show(client: &mut HubstaffClient, user_id: u64, json: bool) -> Result<(), CliError> {
    let data = client.get(&format!("/users/{user_id}"), &HashMap::new())?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(user) = data.get("user") {
        let out = CompactOutput::details(user, &[
            ("ID", "id"),
            ("Name", "name"),
            ("Email", "email"),
            ("Time Zone", "time_zone"),
            ("Created", "created_at"),
        ]);
        print!("{out}");
    }
    Ok(())
}
