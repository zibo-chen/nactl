use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::Url;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    pub server: Option<String>,
    pub context_path: Option<String>,
    pub namespace: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub access_token: Option<String>,
    pub config: Option<PathBuf>,
    pub timeout_secs: Option<u64>,
    pub verbose: bool,
}

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub config_path: Option<PathBuf>,
    pub base_url: String,
    pub context_path: String,
    pub namespace: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub access_token: Option<String>,
    pub timeout: Duration,
    pub verbose: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FileConfig {
    server: Option<String>,
    host: Option<String>,
    port: Option<u16>,
    #[serde(alias = "contextPath")]
    context_path: Option<String>,
    namespace: Option<String>,
    username: Option<String>,
    password: Option<String>,
    #[serde(alias = "accessToken")]
    access_token: Option<String>,
    timeout_secs: Option<u64>,
}

impl RuntimeConfig {
    pub fn resolve(overrides: ConfigOverrides) -> Result<Self> {
        let config_path = overrides.config.clone().or_else(default_config_path);
        let file_config = match config_path.as_deref() {
            Some(path) if path.exists() => load_file(path)?,
            _ => FileConfig::default(),
        };

        let server = overrides
            .server
            .clone()
            .or(file_config.server.clone())
            .or_else(|| compose_server(file_config.host.as_deref(), file_config.port))
            .or_else(|| env_var("NACOS_SERVER"))
            .or_else(|| env_var("RNACOS_SERVER"))
            .or_else(|| compose_server(env_var("NACOS_HOST").as_deref(), env_u16("NACOS_PORT")))
            .context("missing server, set --server or NACOS_SERVER")?;

        let cli_context = overrides
            .context_path
            .clone()
            .or(file_config.context_path.clone())
            .or_else(|| env_var("NACOS_CONTEXT_PATH"));
        let (base_url, context_path) = split_server(&server, cli_context)?;

        let namespace = overrides
            .namespace
            .clone()
            .or(file_config.namespace.clone())
            .or_else(|| env_var("NACOS_NAMESPACE"))
            .unwrap_or_else(|| "public".to_owned());

        let username = overrides
            .username
            .clone()
            .or(file_config.username.clone())
            .or_else(|| env_var("NACOS_USERNAME"));
        let password = overrides
            .password
            .clone()
            .or(file_config.password.clone())
            .or_else(|| env_var("NACOS_PASSWORD"));
        let access_token = overrides
            .access_token
            .clone()
            .or(file_config.access_token.clone())
            .or_else(|| env_var("NACOS_ACCESS_TOKEN"));

        let timeout_secs = overrides
            .timeout_secs
            .or(file_config.timeout_secs)
            .or_else(|| env_u64("NACOS_TIMEOUT_SECS"))
            .unwrap_or(10);

        Ok(Self {
            config_path,
            base_url,
            context_path,
            namespace: normalize_namespace(&namespace),
            username,
            password,
            access_token,
            timeout: Duration::from_secs(timeout_secs),
            verbose: overrides.verbose,
        })
    }
}

pub fn persist_auth(runtime: &RuntimeConfig, access_token: &str) -> Result<PathBuf> {
    let path = runtime
        .config_path
        .clone()
        .or_else(default_config_path)
        .context("unable to resolve nactl config path")?;

    let mut config = if path.exists() {
        load_file(&path)?
    } else {
        FileConfig::default()
    };

    config.server = Some(runtime.base_url.clone());
    config.context_path = Some(runtime.context_path.clone());
    config.namespace = Some(runtime.namespace.clone());
    config.username = runtime.username.clone();
    config.password = runtime.password.clone();
    config.access_token = Some(access_token.to_owned());
    config.timeout_secs = Some(runtime.timeout.as_secs());

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }

    let content = serde_yml::to_string(&config).context("failed to serialize nactl config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

fn load_file(path: &Path) -> Result<FileConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let config = serde_yml::from_str::<FileConfig>(&content)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(config)
}

fn default_config_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;
    let primary = config_dir.join("nactl").join("config.yaml");
    if primary.exists() {
        return Some(primary);
    }

    let legacy = config_dir.join("nacosctl").join("config.yaml");
    if legacy.exists() {
        return Some(legacy);
    }

    Some(primary)
}

fn env_var(name: &str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.trim().is_empty())
}

fn env_u16(name: &str) -> Option<u16> {
    env_var(name).and_then(|value| value.parse().ok())
}

fn env_u64(name: &str) -> Option<u64> {
    env_var(name).and_then(|value| value.parse().ok())
}

fn compose_server(host: Option<&str>, port: Option<u16>) -> Option<String> {
    let host = host?.trim();
    if host.is_empty() {
        return None;
    }
    match port {
        Some(port) => Some(format!("{host}:{port}")),
        None => Some(host.to_owned()),
    }
}

fn split_server(server: &str, cli_context: Option<String>) -> Result<(String, String)> {
    let server = if server.contains("://") {
        server.to_owned()
    } else {
        format!("http://{server}")
    };
    let mut url = Url::parse(&server).with_context(|| format!("invalid server {server}"))?;

    let inferred_context = match url.path() {
        "" | "/" => None,
        path => Some(path.to_owned()),
    };
    let context_path = normalize_context_path(cli_context.or(inferred_context).as_deref());
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);

    let mut base_url = url.to_string();
    while base_url.ends_with('/') {
        base_url.pop();
    }
    Ok((base_url, context_path))
}

fn normalize_context_path(value: Option<&str>) -> String {
    let value = value.unwrap_or("/nacos").trim();
    if value.is_empty() || value == "/" {
        return String::new();
    }
    format!("/{}", value.trim_matches('/'))
}

fn normalize_namespace(namespace: &str) -> String {
    let namespace = namespace.trim();
    if namespace.is_empty() {
        "public".to_owned()
    } else {
        namespace.to_owned()
    }
}
