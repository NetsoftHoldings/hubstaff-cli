use crate::auth;
use crate::client::HubstaffClient;
use crate::error::CliError;
use crate::output::CompactOutput;
use std::collections::HashMap;
use std::io::{self, Read};

#[allow(clippy::too_many_arguments)]
pub fn list_org(
    client: &mut HubstaffClient,
    org_id: u64,
    json: bool,
    page_start: Option<u64>,
    page_limit: Option<u64>,
    search_email: Option<&str>,
    search_name: Option<&str>,
    include_removed: bool,
) -> Result<(), CliError> {
    let mut params = HashMap::new();
    if let Some(ps) = page_start {
        params.insert("page_start_id".to_string(), ps.to_string());
    }
    if let Some(pl) = page_limit {
        params.insert("page_limit".to_string(), pl.to_string());
    }
    if let Some(email) = search_email {
        params.insert("search[email]".to_string(), email.to_string());
    }
    if let Some(name) = search_name {
        params.insert("search[name]".to_string(), name.to_string());
    }
    if include_removed {
        params.insert("include_removed".to_string(), "true".to_string());
    }
    params.insert("include[]".to_string(), "users".to_string());

    let mut data = client.get(&format!("/organizations/{org_id}/members"), &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    // Merge sideloaded user data into each member
    merge_users(&mut data);

    let out = CompactOutput::table(
        &data,
        "members",
        &[
            ("USER_ID", "user_id"),
            ("NAME", "name"),
            ("EMAIL", "email"),
            ("ROLE", "membership_role"),
            ("STATUS", "membership_status"),
        ],
        "members",
        &format!("org:{org_id}"),
    );
    println!("{out}");
    Ok(())
}

pub fn list_project(
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

    params.insert("include[]".to_string(), "users".to_string());

    let mut data = client.get(&format!("/projects/{project_id}/members"), &params)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    merge_users(&mut data);

    let out = CompactOutput::table(
        &data,
        "members",
        &[
            ("USER_ID", "user_id"),
            ("NAME", "name"),
            ("EMAIL", "email"),
            ("ROLE", "membership_role"),
            ("STATUS", "membership_status"),
        ],
        "members",
        &format!("project:{project_id}"),
    );
    println!("{out}");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn create(
    client: &mut HubstaffClient,
    org_id: u64,
    email: &str,
    first_name: &str,
    last_name: &str,
    role: Option<&str>,
    password: Option<&str>,
    password_stdin: bool,
    project_ids: &[u64],
    team_ids: &[u64],
    json: bool,
) -> Result<(), CliError> {
    let actual_password = if let Some(p) = password {
        p.to_string()
    } else if password_stdin {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::Config(format!("failed to read password from stdin: {e}")))?;
        buf.trim().to_string()
    } else {
        auth::generate_password()
    };

    let auto_generated = password.is_none() && !password_stdin;

    let mut body = serde_json::json!({
        "user": {
            "email": email,
            "first_name": first_name,
            "last_name": last_name,
            "password": actual_password,
            "require_password_change": true
        }
    });

    if let Some(r) = role {
        body["role"] = serde_json::Value::String(r.to_string());
    }
    if !project_ids.is_empty() {
        body["project_ids"] = serde_json::json!(project_ids);
    }
    if !team_ids.is_empty() {
        body["team_ids"] = serde_json::json!(team_ids);
    }

    let data = client.post(&format!("/organizations/{org_id}/members"), &body)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    if let Some(member) = data.get("member") {
        let mut fields = vec![
            ("member", format!("{}", member["id"])),
            ("email", email.to_string()),
            ("role", member["membership_role"].as_str().unwrap_or("-").to_string()),
        ];
        if auto_generated {
            fields.push(("generated_password", actual_password));
        }
        let out = CompactOutput::one_liner("created", &fields);
        println!("{out}");
    }
    Ok(())
}

pub fn remove(
    client: &mut HubstaffClient,
    org_id: u64,
    user_id: u64,
    json: bool,
) -> Result<(), CliError> {
    let body = serde_json::json!({
        "members": [{ "user_id": user_id, "role": "remove" }]
    });

    let data = client.put(&format!("/organizations/{org_id}/update_members"), &body)?;

    if json {
        println!("{}", serde_json::to_string_pretty(&data)?);
        return Ok(());
    }

    let out = CompactOutput::one_liner("removed", &[
        ("user", user_id.to_string()),
        ("org", org_id.to_string()),
    ]);
    println!("{out}");
    Ok(())
}

/// Merge sideloaded user data (name, email) into each member record.
fn merge_users(data: &mut serde_json::Value) {
    let users: HashMap<u64, (String, String)> = data
        .get("users")
        .and_then(|u| u.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|u| {
                    let id = u.get("id")?.as_u64()?;
                    let name = u.get("name").and_then(|v| v.as_str()).unwrap_or("-");
                    let email = u.get("email").and_then(|v| v.as_str()).unwrap_or("-");
                    Some((id, (name.to_string(), email.to_string())))
                })
                .collect()
        })
        .unwrap_or_default();

    if let Some(members) = data.get_mut("members").and_then(|m| m.as_array_mut()) {
        for member in members {
            if let Some(uid) = member.get("user_id").and_then(|v| v.as_u64()) {
                if let Some((name, email)) = users.get(&uid) {
                    member["name"] = serde_json::Value::String(name.clone());
                    member["email"] = serde_json::Value::String(email.clone());
                }
            }
        }
    }
}
