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

use clap::{Parser, Subcommand};
use client::HubstaffClient;

#[derive(Parser)]
#[command(
    name = "hubstaff",
    version,
    about = "Schema-driven CLI for the Hubstaff Public API v2",
    arg_required_else_help = true
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
    /// Authenticate via OAuth browser flow
    Login,
    /// Clear saved authentication tokens
    Logout,
    /// Manage cached API schema
    Schema {
        #[command(subcommand)]
        action: SchemaAction,
    },
    /// Browse the available API commands
    #[command(name = "commands")]
    Browse {
        #[command(subcommand)]
        action: CommandsAction,
    },
    /// Run diagnostic checks on config, credentials, and API connectivity
    Check,
    #[command(external_subcommand)]
    Dynamic(Vec<String>),
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Set a config value (keys: organization, api_url, auth_url, schema_url, token, format)
    Set { key: String, value: String },
    /// Unset a config value and restore its default (keys: organization, api_url, auth_url, schema_url, format)
    Unset { key: String },
    /// Reset all non-auth config values to defaults (does not clear tokens; use 'hubstaff logout')
    Reset,
    /// Authenticate with a personal access token (exchanges it automatically)
    SetPat {
        /// Your personal access token from developer.hubstaff.com
        token: String,
    },
    /// Set up OAuth app credentials for browser login
    SetupOauth,
    /// Show current configuration
    Show,
}

#[derive(Subcommand)]
enum SchemaAction {
    /// Refresh schema cache from remote docs endpoint
    Refresh {
        /// Ignore ETag and force full fetch
        #[arg(long)]
        force: bool,
    },
    /// Show schema cache status
    Show,
}

#[derive(Subcommand)]
enum CommandsAction {
    /// List every available command grouped by resource
    List,
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
            ConfigAction::SetupOauth => config_commands::setup_oauth(),
            ConfigAction::Show => config_commands::show(),
        },
        Commands::Login => auth::login(),
        Commands::Logout => auth::logout(),
        Commands::Schema { action } => {
            let cfg = config::Config::load()?;
            match action {
                SchemaAction::Refresh { force } => {
                    let loaded = schema::ApiSchema::refresh(&cfg, *force)?;
                    print_schema_status(&cfg, &loaded);
                    Ok(())
                }
                SchemaAction::Show => {
                    let loaded = schema::ApiSchema::load_cache_only()?;
                    print_schema_status(&cfg, &loaded);
                    Ok(())
                }
            }
        }
        Commands::Browse { action } => match action {
            CommandsAction::List => commands_list::list(),
        },
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

fn print_schema_status(cfg: &config::Config, schema: &schema::ApiSchema) {
    println!("schema_url = {}", cfg.effective_schema_url());
    println!("source = {}", schema.source().as_str());
    println!("operations = {}", schema.operations().len());

    if let Some(meta) = schema.cache_meta_ref() {
        if let Some(fetched_at) = &meta.fetched_at {
            println!("fetched_at = {fetched_at}");
        }
        if let Some(etag) = &meta.etag {
            println!("etag = {etag}");
        }
    }

    println!(
        "cache_docs = {}",
        config::Config::schema_docs_path().display()
    );
    println!(
        "cache_meta = {}",
        config::Config::schema_meta_path().display()
    );
    println!(
        "cache_command_index = {}",
        config::Config::schema_command_index_path().display()
    );
}
