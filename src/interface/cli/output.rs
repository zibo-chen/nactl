use anyhow::Result;
use serde::Serialize;

use crate::infrastructure::config::RuntimeConfig;
use crate::infrastructure::nacos::{ConfigGetResult, ConfigListResult, LoginResult};
use crate::interface::cli::OutputFormat;

pub fn print_login_result(
    config: &RuntimeConfig,
    result: &LoginResult,
    output: OutputFormat,
) -> Result<()> {
    match output {
        OutputFormat::Text => {
            println!("server: {}{}", config.base_url, config.context_path);
            println!("endpoint: {}", result.endpoint);
            println!("accessToken: {}", result.access_token);
            if let Some(ttl) = result.token_ttl {
                println!("tokenTtl: {ttl}");
            }
        }
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(result)?);
        }
    }
    Ok(())
}

pub fn print_config_list(result: &ConfigListResult, output: OutputFormat) -> Result<()> {
    match output {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(result)?);
        }
        OutputFormat::Text => {
            if result.page_items.is_empty() {
                println!("no configs found");
                return Ok(());
            }

            println!(
                "page {} / {}, total {}",
                result.page_number, result.pages_available, result.total_count
            );
            println!("{:<4} {:<42} {:<24} {:<12}", "#", "DATA ID", "GROUP", "TYPE");
            for (index, item) in result.page_items.iter().enumerate() {
                println!(
                    "{:<4} {:<42} {:<24} {:<12}",
                    index + 1,
                    truncate(&item.data_id, 42),
                    truncate(
                        item.group_name
                            .as_deref()
                            .or(item.group.as_deref())
                            .unwrap_or(""),
                        24,
                    ),
                    truncate(item.config_type.as_deref().unwrap_or(""), 12),
                );
            }
        }
    }
    Ok(())
}

pub fn print_config_get(
    config: &RuntimeConfig,
    data_id: &str,
    group: &str,
    result: &Option<ConfigGetResult>,
    output: OutputFormat,
) -> Result<()> {
    match output {
        OutputFormat::Json => {
            #[derive(Serialize)]
            struct ConfigOutput<'a> {
                server: &'a str,
                context_path: &'a str,
                namespace: &'a str,
                data_id: &'a str,
                group: &'a str,
                found: bool,
                content: Option<&'a str>,
            }

            let payload = ConfigOutput {
                server: &config.base_url,
                context_path: &config.context_path,
                namespace: &config.namespace,
                data_id,
                group,
                found: result.is_some(),
                content: result.as_ref().map(|item| item.content.as_str()),
            };
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
        OutputFormat::Text => match result {
            Some(result) => print!("{}", result.content),
            None => println!("config not found"),
        },
    }
    Ok(())
}

fn truncate(value: &str, max: usize) -> String {
    let mut chars = value.chars();
    let count = value.chars().count();
    if count <= max {
        return value.to_owned();
    }
    let prefix: String = chars.by_ref().take(max.saturating_sub(1)).collect();
    format!("{prefix}~")
}