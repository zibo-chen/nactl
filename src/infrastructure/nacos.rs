use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};

use crate::infrastructure::config::RuntimeConfig;

#[derive(Debug, Clone)]
pub struct NacosOpenApiClient {
    http: reqwest::Client,
    config: RuntimeConfig,
    dynamic_token: Option<TokenInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginResult {
    pub access_token: String,
    pub token_ttl: Option<u64>,
    pub endpoint: String,
}

#[derive(Debug, Clone)]
struct TokenInfo {
    access_token: String,
    expires_at: Option<Instant>,
}

#[derive(Debug, Clone, Copy)]
pub enum SearchMode {
    Auto,
    Accurate,
    Blur,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigListResult {
    #[serde(rename = "totalCount")]
    pub total_count: usize,
    #[serde(rename = "pageNumber")]
    pub page_number: usize,
    #[serde(rename = "pagesAvailable")]
    pub pages_available: usize,
    #[serde(rename = "pageItems")]
    pub page_items: Vec<ConfigListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigListItem {
    #[serde(rename = "dataId")]
    pub data_id: String,
    pub group: Option<String>,
    #[serde(rename = "groupName")]
    pub group_name: Option<String>,
    pub tenant: Option<String>,
    #[serde(rename = "type")]
    pub config_type: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ConfigGetResult {
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ConfigSetRequest {
    pub data_id: String,
    pub group: String,
    pub content: String,
    pub config_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginEnvelope {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "tokenTtl")]
    token_ttl: Option<u64>,
    data: Option<LoginEnvelopeData>,
    code: Option<i64>,
    message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoginEnvelopeData {
    #[serde(rename = "accessToken")]
    access_token: Option<String>,
    #[serde(rename = "tokenTtl")]
    token_ttl: Option<u64>,
}

impl NacosOpenApiClient {
    pub fn new(config: RuntimeConfig) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .context("failed to create http client")?;
        Ok(Self {
            http,
            config,
            dynamic_token: None,
        })
    }

    pub async fn login(&mut self) -> Result<LoginResult> {
        let username = self
            .config
            .username
            .clone()
            .context("missing username, set --username or nactl config")?;
        let password = self
            .config
            .password
            .clone()
            .context("missing password, set --password or nactl config")?;

        let endpoints = [
            "/v3/auth/user/login",
            "/v1/auth/users/login",
            "/v1/auth/login",
        ];

        let mut last_error = None;
        for endpoint in endpoints {
            let url = self.endpoint(endpoint);
            self.debug(format!("POST {url}"));
            let response = self
                .http
                .post(&url)
                .form(&[
                    ("username", username.as_str()),
                    ("password", password.as_str()),
                ])
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(error) => {
                    last_error = Some(anyhow!(error));
                    continue;
                }
            };

            let status = response.status();
            let body = response
                .bytes()
                .await
                .context("failed to read login response")?;
            if status == StatusCode::NOT_FOUND
                || status == StatusCode::METHOD_NOT_ALLOWED
                || status == StatusCode::NOT_IMPLEMENTED
            {
                last_error = Some(anyhow!("{} is not available", endpoint));
                continue;
            }
            if !status.is_success() {
                return Err(http_error("login", status, &body));
            }

            let login = parse_login_response(&body, endpoint)?;
            self.dynamic_token = Some(TokenInfo {
                access_token: login.access_token.clone(),
                expires_at: login
                    .token_ttl
                    .map(|ttl| Instant::now() + Duration::from_secs(ttl.saturating_sub(5))),
            });
            return Ok(login);
        }

        Err(last_error.unwrap_or_else(|| anyhow!("no compatible login endpoint found")))
    }

    pub async fn list_configs(
        &mut self,
        data_id: Option<&str>,
        group: Option<&str>,
        page: usize,
        size: usize,
        search_mode: SearchMode,
    ) -> Result<ConfigListResult> {
        let mut params = vec![
            ("pageNo", page.to_string()),
            ("pageSize", size.to_string()),
            (
                "search",
                resolve_search_mode(data_id, group, search_mode).to_owned(),
            ),
        ];
        if let Some(data_id) = data_id {
            params.push(("dataId", data_id.to_owned()));
        }
        if let Some(group) = group {
            params.push(("group", group.to_owned()));
        }
        if let Some(tenant) = self.tenant_param() {
            params.push(("tenant", tenant));
        }
        if let Some(access_token) = self.access_token_param().await? {
            params.push(("accessToken", access_token));
        }

        let response = self
            .http
            .get(self.endpoint("/v1/cs/configs"))
            .query(&params)
            .send()
            .await
            .context("failed to list configs")?;
        self.debug(format!("GET {}", self.endpoint("/v1/cs/configs")));
        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("failed to read list response")?;

        if !status.is_success() {
            return Err(http_error("list configs", status, &body));
        }

        let result: ConfigListResult =
            serde_json::from_slice(&body).context("failed to parse config list response")?;
        Ok(result)
    }

    pub async fn get_config(
        &mut self,
        data_id: &str,
        group: &str,
    ) -> Result<Option<ConfigGetResult>> {
        let mut params = vec![("dataId", data_id.to_owned()), ("group", group.to_owned())];
        if let Some(tenant) = self.tenant_param() {
            params.push(("tenant", tenant));
        }
        if let Some(access_token) = self.access_token_param().await? {
            params.push(("accessToken", access_token));
        }

        let response = self
            .http
            .get(self.endpoint("/v1/cs/configs"))
            .query(&params)
            .send()
            .await
            .context("failed to fetch config")?;
        self.debug(format!("GET {}", self.endpoint("/v1/cs/configs")));
        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("failed to read config response")?;

        if status == StatusCode::NOT_FOUND || body.as_ref() == b"config data not exist" {
            return Ok(None);
        }
        if !status.is_success() {
            return Err(http_error("get config", status, &body));
        }

        Ok(Some(ConfigGetResult {
            content: String::from_utf8(body.to_vec()).context("config content is not UTF-8")?,
        }))
    }

    pub async fn set_config(&mut self, request: &ConfigSetRequest) -> Result<()> {
        let mut params = vec![
            ("dataId", request.data_id.clone()),
            ("group", request.group.clone()),
            ("content", request.content.clone()),
        ];
        if let Some(config_type) = request.config_type.as_ref() {
            params.push(("type", config_type.clone()));
        }
        if let Some(tenant) = self.tenant_param() {
            params.push(("tenant", tenant));
        }
        if let Some(access_token) = self.access_token_param().await? {
            params.push(("accessToken", access_token));
        }

        let response = self
            .http
            .post(self.endpoint("/v1/cs/configs"))
            .form(&params)
            .send()
            .await
            .context("failed to publish config")?;
        self.debug(format!("POST {}", self.endpoint("/v1/cs/configs")));
        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("failed to read publish response")?;

        if !status.is_success() {
            return Err(http_error("set config", status, &body));
        }
        parse_bool_response("set config", &body)
    }

    pub async fn remove_config(&mut self, data_id: &str, group: &str) -> Result<()> {
        let mut params = vec![("dataId", data_id.to_owned()), ("group", group.to_owned())];
        if let Some(tenant) = self.tenant_param() {
            params.push(("tenant", tenant));
        }
        if let Some(access_token) = self.access_token_param().await? {
            params.push(("accessToken", access_token));
        }

        let response = self
            .http
            .delete(self.endpoint("/v1/cs/configs"))
            .query(&params)
            .send()
            .await
            .context("failed to remove config")?;
        self.debug(format!("DELETE {}", self.endpoint("/v1/cs/configs")));
        let status = response.status();
        let body = response
            .bytes()
            .await
            .context("failed to read remove response")?;

        if !status.is_success() {
            return Err(http_error("remove config", status, &body));
        }
        parse_bool_response("remove config", &body)
    }

    fn endpoint(&self, path: &str) -> String {
        format!(
            "{}{}{}",
            self.config.base_url, self.config.context_path, path
        )
    }

    fn debug(&self, message: String) {
        if self.config.verbose {
            eprintln!("[nactl] {message}");
        }
    }

    fn tenant_param(&self) -> Option<String> {
        if self.config.namespace.is_empty() || self.config.namespace == "public" {
            None
        } else {
            Some(self.config.namespace.clone())
        }
    }

    async fn access_token_param(&mut self) -> Result<Option<String>> {
        if let Some(token) = self.dynamic_token.as_ref() {
            if token
                .expires_at
                .map(|time| time > Instant::now())
                .unwrap_or(true)
            {
                return Ok(Some(token.access_token.clone()));
            }
        }

        if self.config.username.is_some() && self.config.password.is_some() {
            return Ok(Some(self.login().await?.access_token));
        }

        if let Some(token) = self.config.access_token.clone() {
            return Ok(Some(token));
        }

        Ok(None)
    }
}

fn resolve_search_mode(
    data_id: Option<&str>,
    group: Option<&str>,
    mode: SearchMode,
) -> &'static str {
    match mode {
        SearchMode::Accurate => "accurate",
        SearchMode::Blur => "blur",
        SearchMode::Auto => {
            let fuzzy = data_id.map(|value| value.contains('*')).unwrap_or(false)
                || group.map(|value| value.contains('*')).unwrap_or(false);
            if fuzzy { "blur" } else { "accurate" }
        }
    }
}

fn parse_login_response(body: &[u8], endpoint: &str) -> Result<LoginResult> {
    let envelope: LoginEnvelope =
        serde_json::from_slice(body).context("failed to parse login response")?;
    if let Some(code) = envelope.code {
        if code != 0 {
            bail!(
                "login failed via {endpoint}: {}",
                envelope.message.unwrap_or_else(|| format!("code={code}"))
            )
        }
    }

    let access_token = envelope
        .access_token
        .clone()
        .or_else(|| {
            envelope
                .data
                .as_ref()
                .and_then(|data| data.access_token.clone())
        })
        .context("login response does not contain accessToken")?;
    let token_ttl = envelope
        .token_ttl
        .or_else(|| envelope.data.and_then(|data| data.token_ttl));

    Ok(LoginResult {
        access_token,
        token_ttl,
        endpoint: endpoint.to_owned(),
    })
}

fn parse_bool_response(operation: &str, body: &[u8]) -> Result<()> {
    let text = String::from_utf8_lossy(body);
    if text.trim() == "true" {
        return Ok(());
    }

    #[derive(Deserialize)]
    struct BoolEnvelope {
        data: Option<bool>,
        code: Option<i64>,
        message: Option<String>,
    }

    if let Ok(envelope) = serde_json::from_slice::<BoolEnvelope>(body) {
        if envelope.code.unwrap_or(0) != 0 {
            bail!(
                "{operation} failed: {}",
                envelope
                    .message
                    .unwrap_or_else(|| "server returned an error".to_owned())
            )
        }
        if envelope.data == Some(true) {
            return Ok(());
        }
    }

    bail!("{operation} failed: unexpected response {text}")
}

fn http_error(operation: &str, status: StatusCode, body: &[u8]) -> anyhow::Error {
    let message = String::from_utf8_lossy(body).trim().to_owned();
    if message.is_empty() {
        anyhow!("{operation} failed with HTTP {}", status.as_u16())
    } else {
        anyhow!(
            "{operation} failed with HTTP {}: {message}",
            status.as_u16()
        )
    }
}
