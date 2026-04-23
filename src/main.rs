mod api;
mod auth;
mod check;
mod client;
mod command_index;
mod commands_list;
mod config;
mod config_commands;
mod error;
mod persistence;
mod schema;
mod time;

pub(crate) const HTTP_TIMEOUT_SECS: u64 = 40;

use clap::{Parser, Subcommand};
use client::HubstaffClient;

#[derive(Parser)]
#[command(
    name = "hubstaff",
    version,
    about = "Schema-driven CLI for the Hubstaff Public API v2",
    arg_required_else_help = true,
    after_help = "Dynamic API commands:\n  \
        Every endpoint in the Hubstaff API is available directly.\n  \
        Run `hubstaff list` to see every command, or try:\n\n    \
        hubstaff users me\n    \
        hubstaff projects list\n    \
        hubstaff teams update_members 42\n\n  \
        Use `hubstaff <command> --help` for per-endpoint flags."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Emit minified single-line JSON (shortcut for `format = "json"`). Overrides the config default.
    #[arg(long, short = 'j', global = true, conflicts_with = "pretty")]
    json: bool,

    /// Emit pretty-printed, colorized JSON (shortcut for `format = "pretty"`). Overrides the config default.
    #[arg(long, short = 'p', global = true)]
    pretty: bool,

    /// Override the default organization for this invocation
    #[arg(long, short = 'o', global = true)]
    organization: Option<u64>,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// List every available API command grouped by resource
    List,
    /// Run diagnostic checks on config, credentials, and API connectivity
    Check,
    #[command(external_subcommand)]
    Dynamic(Vec<String>),
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a config value (keys: organization, api_url, auth_url, schema_url, token, format)
    Set { key: String, value: String },
    /// Unset a config value and restore its default (keys: organization, api_url, auth_url, schema_url, format, token)
    Unset { key: String },
    /// Reset all config values to defaults, including auth tokens.
    Reset,
    /// Authenticate with a personal access token (exchanges it automatically)
    SetPat {
        /// Your personal access token from developer.hubstaff.com
        token: String,
    },
    /// Show current configuration
    Show,
}

fn main() {
    let cli = Cli::parse();

    let result = run(&cli);
    if let Err(e) = result {
        e.exit();
    }
}

fn run(cli: &Cli) -> Result<(), error::CliError> {
    match &cli.command {
        Commands::Config { action } => match action {
            ConfigAction::Set { key, value } => config_commands::set(key, value),
            ConfigAction::Unset { key } => config_commands::unset(key),
            ConfigAction::Reset => config_commands::reset(),
            ConfigAction::SetPat { token } => config_commands::set_pat(token),
            ConfigAction::Show => config_commands::show(),
        },
        Commands::List => commands_list::list(),
        Commands::Check => {
            check::run();
            Ok(())
        }
        Commands::Dynamic(args) => {
            let cfg = config::Config::load()?;
            let effective = if cli.json {
                "json"
            } else if cli.pretty {
                "pretty"
            } else {
                cfg.format.as_str()
            };
            let pretty = effective == "pretty";
            let schema = schema::ApiSchema::load(&cfg)?;
            let mut client = HubstaffClient::new(cfg)?;
            api::run_dynamic(&mut client, &schema, args, pretty, cli.organization)
        }
    }
}
