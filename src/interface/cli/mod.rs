mod output;

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use tempfile::NamedTempFile;
use tokio::io::AsyncReadExt;

use crate::error::AppResult;
use crate::infrastructure::config::persist_auth;
use crate::infrastructure::nacos::{
    ConfigGetResult, ConfigSetRequest, SearchMode as ClientSearchMode,
};
use crate::interface::runtime::RuntimeBootstrap;

#[derive(Debug, Clone, Parser)]
#[command(
    name = "nactl",
    version,
    about = "Cross-compatible Nacos and r-nacos CLI"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Clone, Args, Default)]
pub struct GlobalArgs {
    #[arg(long, global = true)]
    pub server: Option<String>,

    #[arg(long, global = true)]
    pub context_path: Option<String>,

    #[arg(long, global = true)]
    pub namespace: Option<String>,

    #[arg(long, global = true)]
    pub username: Option<String>,

    #[arg(long, global = true)]
    pub password: Option<String>,

    #[arg(long, global = true)]
    pub access_token: Option<String>,

    #[arg(long, global = true)]
    pub config: Option<std::path::PathBuf>,

    #[arg(long, global = true)]
    pub timeout_secs: Option<u64>,

    #[arg(long, global = true)]
    pub verbose: bool,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Commands {
    /// Exchange username/password for an access token.
    Login(LoginArgs),
    /// Read or mutate configuration entries.
    Config(ConfigArgs),
    /// Start the local stdio MCP server.
    Mcp(McpArgs),
}

#[derive(Debug, Clone, Args)]
pub struct LoginArgs {
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,

    #[arg(long = "no-save-auth")]
    pub no_save_auth: bool,
}

#[derive(Debug, Clone, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommands,
}

#[derive(Debug, Clone, Args)]
pub struct McpArgs {}

#[derive(Debug, Clone, Subcommand)]
pub enum ConfigCommands {
    /// List configs via /nacos/v1/cs/configs.
    List(ConfigListArgs),
    /// Fetch config content.
    Get(ConfigGetArgs),
    /// Create or update a config.
    Set(ConfigSetArgs),
    /// Remove a config.
    Rm(ConfigRmArgs),
    /// Edit a config with $VISUAL or $EDITOR.
    Edit(ConfigEditArgs),
}

#[derive(Debug, Clone, Args)]
pub struct ConfigListArgs {
    #[arg(long)]
    pub data_id: Option<String>,

    #[arg(long)]
    pub group: Option<String>,

    #[arg(long, default_value_t = 1)]
    pub page: usize,

    #[arg(long, default_value_t = 20)]
    pub size: usize,

    #[arg(long, value_enum, default_value_t = SearchMode::Auto)]
    pub search: SearchMode,

    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,
}

#[derive(Debug, Clone, Args)]
pub struct ConfigGetArgs {
    pub data_id: String,

    #[arg(default_value = "DEFAULT_GROUP")]
    pub group: String,

    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub output: OutputFormat,
}

#[derive(Debug, Clone, Args)]
pub struct ConfigSetArgs {
    pub data_id: String,

    #[arg(default_value = "DEFAULT_GROUP")]
    pub group: String,

    #[arg(long)]
    pub value: Option<String>,

    #[arg(long, short = 'f')]
    pub file: Option<std::path::PathBuf>,

    #[arg(long = "type")]
    pub config_type: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ConfigRmArgs {
    pub data_id: String,

    #[arg(default_value = "DEFAULT_GROUP")]
    pub group: String,
}

#[derive(Debug, Clone, Args)]
pub struct ConfigEditArgs {
    pub data_id: String,

    #[arg(default_value = "DEFAULT_GROUP")]
    pub group: String,

    #[arg(long)]
    pub editor: Option<String>,

    #[arg(long = "type")]
    pub config_type: Option<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, ValueEnum)]
pub enum SearchMode {
    Auto,
    Accurate,
    Blur,
}

impl From<SearchMode> for ClientSearchMode {
    fn from(value: SearchMode) -> Self {
        match value {
            SearchMode::Auto => ClientSearchMode::Auto,
            SearchMode::Accurate => ClientSearchMode::Accurate,
            SearchMode::Blur => ClientSearchMode::Blur,
        }
    }
}

pub async fn run() -> AppResult<()> {
    let cli = Cli::parse();

    if let Commands::Mcp(_) = &cli.command {
        return crate::interface::mcp::run_stdio(&cli.global).await;
    }

    let mut bootstrap = RuntimeBootstrap::from_global_args(&cli.global)?;

    match cli.command {
        Commands::Login(args) => {
            let token = bootstrap.service.login().await?;
            let saved_path = if args.no_save_auth {
                None
            } else {
                Some(persist_auth(&bootstrap.config, &token.access_token)?)
            };
            output::print_login_result(&bootstrap.config, &token, args.output)?;
            if args.output == OutputFormat::Text {
                if let Some(path) = saved_path {
                    println!("savedAuth: {}", path.display());
                }
            }
        }
        Commands::Config(args) => match args.command {
            ConfigCommands::List(args) => {
                let response = bootstrap
                    .service
                    .list_configs(
                        args.data_id.as_deref(),
                        args.group.as_deref(),
                        args.page,
                        args.size,
                        args.search.into(),
                    )
                    .await?;
                output::print_config_list(&response, args.output)?;
            }
            ConfigCommands::Get(args) => {
                let result = bootstrap
                    .service
                    .get_config(&args.data_id, &args.group)
                    .await?;
                output::print_config_get(
                    &bootstrap.config,
                    &args.data_id,
                    &args.group,
                    &result,
                    args.output,
                )?;
            }
            ConfigCommands::Set(args) => {
                let content = resolve_content(args.value.as_deref(), args.file.as_deref()).await?;
                let request = ConfigSetRequest {
                    data_id: args.data_id,
                    group: args.group,
                    content,
                    config_type: args.config_type,
                };
                bootstrap.service.set_config(&request).await?;
                println!("updated {}/{}", request.group, request.data_id);
            }
            ConfigCommands::Rm(args) => {
                bootstrap
                    .service
                    .remove_config(&args.data_id, &args.group)
                    .await?;
                println!("removed {}/{}", args.group, args.data_id);
            }
            ConfigCommands::Edit(args) => {
                let current = bootstrap
                    .service
                    .get_config(&args.data_id, &args.group)
                    .await?;
                let edited = edit_content(
                    &args.data_id,
                    &args.group,
                    current.as_ref(),
                    args.editor.as_deref(),
                )?;
                let Some(content) = edited else {
                    println!("no changes");
                    return Ok(());
                };
                let request = ConfigSetRequest {
                    data_id: args.data_id,
                    group: args.group,
                    content,
                    config_type: args.config_type,
                };
                bootstrap.service.set_config(&request).await?;
                println!("updated {}/{}", request.group, request.data_id);
            }
        },
        Commands::Mcp(_) => unreachable!("mcp subcommand should return before CLI dispatch"),
    }

    Ok(())
}

async fn resolve_content(value: Option<&str>, file: Option<&Path>) -> AppResult<String> {
    if let Some(value) = value {
        return ensure_non_empty(value.to_owned());
    }

    if let Some(file) = file {
        let content = fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        return ensure_non_empty(content);
    }

    let mut stdin = tokio::io::stdin();
    let mut buffer = Vec::new();
    stdin
        .read_to_end(&mut buffer)
        .await
        .context("failed to read stdin")?;
    let content = String::from_utf8(buffer).context("stdin is not valid UTF-8")?;
    ensure_non_empty(content)
}

fn ensure_non_empty(content: String) -> AppResult<String> {
    if content.trim().is_empty() {
        bail!("config content is empty")
    }
    Ok(content)
}

fn edit_content(
    data_id: &str,
    group: &str,
    current: Option<&ConfigGetResult>,
    editor_override: Option<&str>,
) -> AppResult<Option<String>> {
    let temp = NamedTempFile::new().context("failed to create temp file")?;
    let original = current.map(|item| item.content.clone()).unwrap_or_default();
    if !original.is_empty() {
        fs::write(temp.path(), &original).context("failed to seed temp file")?;
    }

    let editor = select_editor(editor_override)?;
    let status = Command::new(&editor)
        .arg(temp.path())
        .status()
        .with_context(|| format!("failed to launch editor {editor}"))?;
    if !status.success() {
        bail!("editor exited with status {status}")
    }

    let updated = fs::read_to_string(temp.path()).context("failed to read edited file")?;
    let updated = updated.trim_end_matches('\n').to_owned();
    let original = original.trim_end_matches('\n').to_owned();

    if updated == original {
        return Ok(None);
    }
    if updated.trim().is_empty() {
        bail!("refusing to publish empty content for {group}/{data_id}")
    }
    Ok(Some(updated))
}

fn select_editor(editor_override: Option<&str>) -> AppResult<String> {
    if let Some(editor) = editor_override {
        return Ok(editor.to_owned());
    }

    if let Ok(editor) = env::var("VISUAL") {
        if !editor.trim().is_empty() {
            return Ok(editor);
        }
    }
    if let Ok(editor) = env::var("EDITOR") {
        if !editor.trim().is_empty() {
            return Ok(editor);
        }
    }

    Ok("vi".to_owned())
}
