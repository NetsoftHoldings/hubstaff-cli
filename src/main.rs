mod auth;
mod client;
mod commands;
mod config;
mod error;
mod output;

use clap::{Parser, Subcommand};
use client::HubstaffClient;

#[derive(Parser)]
#[command(name = "hubstaff-cli", version, about = "Token-efficient CLI for the Hubstaff Public API v2")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Override default organization ID
    #[arg(long, global = true)]
    org: Option<u64>,

    /// Output full JSON instead of compact format
    #[arg(long, global = true)]
    json: bool,

    /// Pagination cursor (record ID to start from)
    #[arg(long, global = true)]
    page_start: Option<u64>,

    /// Results per page (default: 100, max: 500)
    #[arg(long, global = true)]
    page_limit: Option<u64>,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Authenticate via OAuth browser flow
    Login,
    /// Clear saved authentication tokens
    Logout,
    /// Manage users
    Users {
        #[command(subcommand)]
        action: UsersAction,
    },
    /// Manage organizations
    Orgs {
        #[command(subcommand)]
        action: OrgsAction,
    },
    /// Manage projects
    Projects {
        #[command(subcommand)]
        action: ProjectsAction,
    },
    /// Manage organization/project members
    Members {
        #[command(subcommand)]
        action: MembersAction,
    },
    /// Manage invites
    Invites {
        #[command(subcommand)]
        action: InvitesAction,
    },
    /// Manage tasks
    Tasks {
        #[command(subcommand)]
        action: TasksAction,
    },
    /// View tracked activities
    Activities {
        #[command(subcommand)]
        action: ActivitiesAction,
    },
    /// View daily activity summaries
    DailyActivities {
        #[command(subcommand)]
        action: DailyActivitiesAction,
    },
    /// Manage teams
    Teams {
        #[command(subcommand)]
        action: TeamsAction,
    },
    /// Manage notes
    Notes {
        #[command(subcommand)]
        action: NotesAction,
    },
    /// Create manual time entries
    TimeEntries {
        #[command(subcommand)]
        action: TimeEntriesAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a config value (keys: org, api_url, token, format)
    Set { key: String, value: String },
    /// Authenticate with a personal access token (exchanges it automatically)
    SetPat {
        /// Your personal access token from developer.hubstaff.com
        token: String,
    },
    /// Show current configuration
    Show,
}

#[derive(Subcommand)]
enum UsersAction {
    /// Show your user info
    Me,
    /// Show a user by ID
    Show { id: u64 },
}

#[derive(Subcommand)]
enum OrgsAction {
    /// List organizations
    List,
    /// Show an organization by ID
    Show { id: u64 },
}

#[derive(Subcommand)]
enum ProjectsAction {
    /// List projects in an organization
    List,
    /// Show a project by ID
    Show { id: u64 },
    /// Create a project
    Create {
        /// Project name
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
enum MembersAction {
    /// List members (use --org for org members, --project for project members)
    List {
        /// List project members instead of org members
        #[arg(long)]
        project: Option<u64>,
        /// Search by email
        #[arg(long)]
        search_email: Option<String>,
        /// Search by name
        #[arg(long)]
        search_name: Option<String>,
        /// Include removed members
        #[arg(long)]
        include_removed: bool,
    },
    /// Create a member in an organization
    Create {
        #[arg(long)]
        email: String,
        #[arg(long)]
        first_name: String,
        #[arg(long)]
        last_name: String,
        /// Role: organization_manager, project_manager, project_user, project_viewer
        #[arg(long)]
        role: Option<String>,
        /// Password (or use --password-stdin)
        #[arg(long)]
        password: Option<String>,
        /// Read password from stdin
        #[arg(long)]
        password_stdin: bool,
        /// Project IDs to assign (comma-separated)
        #[arg(long, value_delimiter = ',')]
        project_ids: Vec<u64>,
        /// Team IDs to assign (comma-separated)
        #[arg(long, value_delimiter = ',')]
        team_ids: Vec<u64>,
    },
    /// Remove a member from an organization
    Remove {
        /// User ID to remove
        #[arg(long)]
        user_id: u64,
    },
}

#[derive(Subcommand)]
enum InvitesAction {
    /// List invites for an organization
    List {
        /// Filter by status: all, pending, accepted, expired
        #[arg(long)]
        status: Option<String>,
    },
    /// Show an invite by ID
    Show { id: u64 },
    /// Create an invite
    Create {
        #[arg(long)]
        email: String,
        /// Role: organization_manager, project_manager, project_user, project_viewer
        #[arg(long)]
        role: Option<String>,
        /// Project IDs (comma-separated)
        #[arg(long, value_delimiter = ',')]
        project_ids: Vec<u64>,
    },
    /// Delete a pending/expired invite
    Delete { id: u64 },
}

#[derive(Subcommand)]
enum TasksAction {
    /// List tasks for a project
    List {
        /// Project ID
        #[arg(long)]
        project: u64,
    },
    /// Show a task by ID
    Show { id: u64 },
    /// Create a task
    Create {
        /// Project ID
        #[arg(long)]
        project: u64,
        /// Task summary
        #[arg(long)]
        summary: String,
        /// Assignee user ID
        #[arg(long)]
        assignee_id: Option<u64>,
    },
}

#[derive(Subcommand)]
enum ActivitiesAction {
    /// List activities (requires --start)
    List {
        /// Start time (ISO 8601 or YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// Stop time (defaults to now)
        #[arg(long)]
        stop: Option<String>,
    },
}

#[derive(Subcommand)]
enum DailyActivitiesAction {
    /// List daily activity summaries (requires --start)
    List {
        /// Start date (YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// Stop date (defaults to today)
        #[arg(long)]
        stop: Option<String>,
    },
}

#[derive(Subcommand)]
enum TeamsAction {
    /// List teams
    List,
    /// Show a team by ID
    Show { id: u64 },
}

#[derive(Subcommand)]
enum NotesAction {
    /// List notes (requires --start)
    List {
        /// Start time (ISO 8601 or YYYY-MM-DD)
        #[arg(long)]
        start: String,
        /// Stop time (defaults to now)
        #[arg(long)]
        stop: Option<String>,
    },
    /// Create a note
    Create {
        #[arg(long)]
        project: u64,
        #[arg(long)]
        description: String,
        /// Recorded time (ISO 8601 or YYYY-MM-DD)
        #[arg(long)]
        recorded_time: String,
    },
}

#[derive(Subcommand)]
enum TimeEntriesAction {
    /// Create a manual time entry
    Create {
        #[arg(long)]
        project: u64,
        /// Start time (ISO 8601)
        #[arg(long)]
        start: String,
        /// Stop time (ISO 8601)
        #[arg(long)]
        stop: String,
    },
}

fn main() {
    // Load .env file if present (silently ignore if missing)
    let _ = dotenvy::dotenv();

    let cli = Cli::parse();

    let result = run(&cli);
    if let Err(e) = result {
        e.exit(cli.json);
    }
}

fn run(cli: &Cli) -> Result<(), error::CliError> {
    match &cli.command {
        Commands::Config { action } => match action {
            ConfigAction::Set { key, value } => commands::config_cmd::set(key, value),
            ConfigAction::SetPat { token } => commands::config_cmd::set_pat(token),
            ConfigAction::Show => commands::config_cmd::show(),
        },
        Commands::Login => commands::login::login(),
        Commands::Logout => commands::login::logout(),

        // All commands below need an HTTP client
        cmd => {
            let cfg = config::Config::load()?;
            let mut client = HubstaffClient::new(cfg)?;

            match cmd {
                Commands::Users { action } => match action {
                    UsersAction::Me => commands::users::me(&mut client, cli.json),
                    UsersAction::Show { id } => commands::users::show(&mut client, *id, cli.json),
                },
                Commands::Orgs { action } => match action {
                    OrgsAction::List => {
                        commands::orgs::list(&mut client, cli.json, cli.page_start, cli.page_limit)
                    }
                    OrgsAction::Show { id } => commands::orgs::show(&mut client, *id, cli.json),
                },
                Commands::Projects { action } => {
                    match action {
                        ProjectsAction::List => {
                            let org = client.resolve_org(cli.org)?;
                            commands::projects::list(
                                &mut client,
                                org,
                                cli.json,
                                cli.page_start,
                                cli.page_limit,
                            )
                        }
                        ProjectsAction::Show { id } => {
                            commands::projects::show(&mut client, *id, cli.json)
                        }
                        ProjectsAction::Create { name } => {
                            let org = client.resolve_org(cli.org)?;
                            commands::projects::create(&mut client, org, name, cli.json)
                        }
                    }
                }
                Commands::Members { action } => match action {
                    MembersAction::List {
                        project,
                        search_email,
                        search_name,
                        include_removed,
                    } => {
                        if let Some(pid) = project {
                            commands::members::list_project(
                                &mut client,
                                *pid,
                                cli.json,
                                cli.page_start,
                                cli.page_limit,
                            )
                        } else {
                            let org = client.resolve_org(cli.org)?;
                            commands::members::list_org(
                                &mut client,
                                org,
                                cli.json,
                                cli.page_start,
                                cli.page_limit,
                                search_email.as_deref(),
                                search_name.as_deref(),
                                *include_removed,
                            )
                        }
                    }
                    MembersAction::Create {
                        email,
                        first_name,
                        last_name,
                        role,
                        password,
                        password_stdin,
                        project_ids,
                        team_ids,
                    } => {
                        let org = client.resolve_org(cli.org)?;
                        commands::members::create(
                            &mut client,
                            org,
                            email,
                            first_name,
                            last_name,
                            role.as_deref(),
                            password.as_deref(),
                            *password_stdin,
                            project_ids,
                            team_ids,
                            cli.json,
                        )
                    }
                    MembersAction::Remove { user_id } => {
                        let org = client.resolve_org(cli.org)?;
                        commands::members::remove(&mut client, org, *user_id, cli.json)
                    }
                },
                Commands::Invites { action } => match action {
                    InvitesAction::List { status } => {
                        let org = client.resolve_org(cli.org)?;
                        commands::invites::list(
                            &mut client,
                            org,
                            cli.json,
                            cli.page_start,
                            cli.page_limit,
                            status.as_deref(),
                        )
                    }
                    InvitesAction::Show { id } => {
                        commands::invites::show(&mut client, *id, cli.json)
                    }
                    InvitesAction::Create {
                        email,
                        role,
                        project_ids,
                    } => {
                        let org = client.resolve_org(cli.org)?;
                        commands::invites::create(
                            &mut client,
                            org,
                            email,
                            role.as_deref(),
                            project_ids,
                            cli.json,
                        )
                    }
                    InvitesAction::Delete { id } => {
                        commands::invites::delete(&mut client, *id, cli.json)
                    }
                },
                Commands::Tasks { action } => match action {
                    TasksAction::List { project } => commands::tasks::list(
                        &mut client,
                        *project,
                        cli.json,
                        cli.page_start,
                        cli.page_limit,
                    ),
                    TasksAction::Show { id } => commands::tasks::show(&mut client, *id, cli.json),
                    TasksAction::Create {
                        project,
                        summary,
                        assignee_id,
                    } => commands::tasks::create(
                        &mut client,
                        *project,
                        summary,
                        *assignee_id,
                        cli.json,
                    ),
                },
                Commands::Activities { action } => match action {
                    ActivitiesAction::List { start, stop } => {
                        let org = client.resolve_org(cli.org)?;
                        commands::activities::list(
                            &mut client,
                            org,
                            start,
                            stop.as_deref(),
                            cli.json,
                            cli.page_start,
                            cli.page_limit,
                        )
                    }
                },
                Commands::DailyActivities { action } => match action {
                    DailyActivitiesAction::List { start, stop } => {
                        let org = client.resolve_org(cli.org)?;
                        commands::daily_activities::list(
                            &mut client,
                            org,
                            start,
                            stop.as_deref(),
                            cli.json,
                            cli.page_start,
                            cli.page_limit,
                        )
                    }
                },
                Commands::Teams { action } => match action {
                    TeamsAction::List => {
                        let org = client.resolve_org(cli.org)?;
                        commands::teams::list(
                            &mut client,
                            org,
                            cli.json,
                            cli.page_start,
                            cli.page_limit,
                        )
                    }
                    TeamsAction::Show { id } => commands::teams::show(&mut client, *id, cli.json),
                },
                Commands::Notes { action } => match action {
                    NotesAction::List { start, stop } => {
                        let org = client.resolve_org(cli.org)?;
                        commands::notes::list(
                            &mut client,
                            org,
                            start,
                            stop.as_deref(),
                            cli.json,
                            cli.page_start,
                            cli.page_limit,
                        )
                    }
                    NotesAction::Create {
                        project,
                        description,
                        recorded_time,
                    } => commands::notes::create(
                        &mut client,
                        *project,
                        description,
                        recorded_time,
                        cli.json,
                    ),
                },
                Commands::TimeEntries { action } => match action {
                    TimeEntriesAction::Create {
                        project,
                        start,
                        stop,
                    } => commands::time_entries::create(
                        &mut client,
                        *project,
                        start,
                        stop,
                        cli.json,
                    ),
                },
                Commands::Config { .. } | Commands::Login | Commands::Logout => unreachable!(),
            }
        }
    }
}
