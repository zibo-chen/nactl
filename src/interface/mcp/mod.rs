use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt,
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Implementation, ServerCapabilities, ServerInfo},
    schemars,
    schemars::JsonSchema,
    service::{RxJsonRpcMessage, TxJsonRpcMessage},
    tool, tool_handler, tool_router,
    transport::Transport,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{
    io::{self, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, Stdout},
    sync::Mutex,
};

use crate::{
    application::service::NactlApplicationService,
    error::AppResult,
    infrastructure::{
        config::RuntimeConfig,
        nacos::{ConfigSetRequest, SearchMode},
    },
    interface::{cli::GlobalArgs, runtime::RuntimeBootstrap},
};

pub async fn run_stdio(global: &GlobalArgs) -> AppResult<()> {
    let runtime = RuntimeBootstrap::from_global_args(global)?;
    let server = NactlMcpServer::new(runtime.service, runtime.config);
    let running = server.serve(StandardIoTransport::new()).await?;
    running.waiting().await?;
    Ok(())
}

struct NactlMcpServer {
    service: Arc<Mutex<NactlApplicationService>>,
    runtime: RuntimeConfig,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl NactlMcpServer {
    fn new(service: NactlApplicationService, runtime: RuntimeConfig) -> Self {
        Self {
            service: Arc::new(Mutex::new(service)),
            runtime,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        name = "nactl_auth_status",
        description = "读取当前 nactl MCP 会话的目标服务和鉴权来源",
        annotations(read_only_hint = true)
    )]
    async fn auth_status_tool(&self) -> Result<CallToolResult, McpError> {
        structured_tool_result(
            AuthStatus {
                server: self.runtime.base_url.clone(),
                context_path: self.runtime.context_path.clone(),
                namespace: self.runtime.namespace.clone(),
                has_access_token: self.runtime.access_token.is_some(),
                has_username_password: self.runtime.username.is_some() && self.runtime.password.is_some(),
                config_path: self
                    .runtime
                    .config_path
                    .as_ref()
                    .map(|path| path.display().to_string()),
            },
            false,
        )
    }

    #[tool(
        name = "nactl_config_list",
        description = "列出配置项，支持 data_id、group、分页与搜索模式",
        annotations(read_only_hint = true)
    )]
    async fn config_list_tool(
        &self,
        params: Parameters<ConfigListArgs>,
    ) -> Result<CallToolResult, McpError> {
        let args = params.0;
        let page = args.page.unwrap_or(1);
        let size = args.size.unwrap_or(20);
        let result = {
            let mut service = self.service.lock().await;
            service
                .list_configs(
                    args.data_id.as_deref(),
                    args.group.as_deref(),
                    page,
                    size,
                    args.search_mode.unwrap_or(McpSearchMode::Auto).into(),
                )
                .await
        };

        match result {
            Ok(payload) => structured_tool_result(payload, false),
            Err(error) => Ok(self.error_tool_result("nactl_config_list", error)),
        }
    }

    #[tool(
        name = "nactl_config_get",
        description = "读取指定配置项内容",
        annotations(read_only_hint = true)
    )]
    async fn config_get_tool(
        &self,
        params: Parameters<ConfigGetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let args = params.0;
        let group = args.group.unwrap_or_else(|| "DEFAULT_GROUP".to_string());
        let result = {
            let mut service = self.service.lock().await;
            service.get_config(&args.data_id, &group).await
        };

        match result {
            Ok(result) => structured_tool_result(
                ConfigGetOutput {
                data_id: args.data_id,
                group,
                namespace: args.namespace_hint.unwrap_or_else(|| "public".to_string()),
                found: result.is_some(),
                content: result.map(|item| item.content),
                },
                false,
            ),
            Err(error) => Ok(self.error_tool_result("nactl_config_get", error)),
        }
    }

    #[tool(
        name = "nactl_config_set",
        description = "创建或更新配置项"
    )]
    async fn config_set_tool(
        &self,
        params: Parameters<ConfigSetArgs>,
    ) -> Result<CallToolResult, McpError> {
        let args = params.0;
        let group = args.group.unwrap_or_else(|| "DEFAULT_GROUP".to_string());
        let request = ConfigSetRequest {
            data_id: args.data_id.clone(),
            group: group.clone(),
            content: args.content,
            config_type: args.config_type,
        };
        let result = {
            let mut service = self.service.lock().await;
            service.set_config(&request).await
        };

        match result {
            Ok(()) => structured_tool_result(
                ConfigMutationOutput {
                action: "set".to_string(),
                data_id: args.data_id,
                group,
                success: true,
                },
                false,
            ),
            Err(error) => Ok(self.error_tool_result("nactl_config_set", error)),
        }
    }

    #[tool(
        name = "nactl_config_remove",
        description = "删除配置项"
    )]
    async fn config_remove_tool(
        &self,
        params: Parameters<ConfigRemoveArgs>,
    ) -> Result<CallToolResult, McpError> {
        let args = params.0;
        let group = args.group.unwrap_or_else(|| "DEFAULT_GROUP".to_string());
        let result = {
            let mut service = self.service.lock().await;
            service.remove_config(&args.data_id, &group).await
        };

        match result {
            Ok(()) => structured_tool_result(
                ConfigMutationOutput {
                action: "remove".to_string(),
                data_id: args.data_id,
                group,
                success: true,
                },
                false,
            ),
            Err(error) => Ok(self.error_tool_result("nactl_config_remove", error)),
        }
    }

    fn error_tool_result(&self, tool: &str, error: anyhow::Error) -> CallToolResult {
        structured_tool_result_unchecked(
            ToolError {
                tool: tool.to_string(),
                message: error.to_string(),
            },
            true,
        )
    }
}

#[tool_handler]
impl ServerHandler for NactlMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: "nactl-mcp".to_string(),
                title: Some("nactl MCP Server".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some(
                    "Expose Nacos and r-nacos config operations through the Model Context Protocol"
                        .to_string(),
                ),
                icons: None,
                website_url: None,
            },
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(format!(
                "Use these tools to read or mutate Nacos configuration through nactl. Target server: {}{}.",
                self.runtime.base_url, self.runtime.context_path
            )),
            ..Default::default()
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigListArgs {
    data_id: Option<String>,
    group: Option<String>,
    page: Option<usize>,
    size: Option<usize>,
    search_mode: Option<McpSearchMode>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigGetArgs {
    data_id: String,
    group: Option<String>,
    namespace_hint: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigSetArgs {
    data_id: String,
    group: Option<String>,
    content: String,
    config_type: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ConfigRemoveArgs {
    data_id: String,
    group: Option<String>,
}

#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum McpSearchMode {
    Auto,
    Accurate,
    Blur,
}

impl From<McpSearchMode> for SearchMode {
    fn from(value: McpSearchMode) -> Self {
        match value {
            McpSearchMode::Auto => SearchMode::Auto,
            McpSearchMode::Accurate => SearchMode::Accurate,
            McpSearchMode::Blur => SearchMode::Blur,
        }
    }
}

#[derive(Debug, Serialize)]
struct AuthStatus {
    server: String,
    context_path: String,
    namespace: String,
    has_access_token: bool,
    has_username_password: bool,
    config_path: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConfigGetOutput {
    data_id: String,
    group: String,
    namespace: String,
    found: bool,
    content: Option<String>,
}

#[derive(Debug, Serialize)]
struct ConfigMutationOutput {
    action: String,
    data_id: String,
    group: String,
    success: bool,
}

#[derive(Debug, Serialize)]
struct ToolError {
    tool: String,
    message: String,
}

fn structured_tool_result<T>(payload: T, is_error: bool) -> Result<CallToolResult, McpError>
where
    T: Serialize,
{
    let structured = serde_json::to_value(payload).map_err(|error| {
        McpError::internal_error(format!("failed to serialize tool result: {error}"), None)
    })?;

    Ok(if is_error {
        CallToolResult::structured_error(structured)
    } else {
        CallToolResult::structured(structured)
    })
}

fn structured_tool_result_unchecked<T>(payload: T, is_error: bool) -> CallToolResult
where
    T: Serialize,
{
    let structured =
        serde_json::to_value(payload).unwrap_or_else(|_| Value::Object(Default::default()));
    if is_error {
        CallToolResult::structured_error(structured)
    } else {
        CallToolResult::structured(structured)
    }
}

struct StandardIoTransport {
    reader: BufReader<io::Stdin>,
    writer: Arc<Mutex<WriterState>>,
}

impl StandardIoTransport {
    fn new() -> Self {
        Self {
            reader: BufReader::new(io::stdin()),
            writer: Arc::new(Mutex::new(WriterState {
                output: Some(io::stdout()),
                framing: None,
            })),
        }
    }
}

impl Transport<RoleServer> for StandardIoTransport {
    type Error = std::io::Error;

    fn send(
        &mut self,
        item: TxJsonRpcMessage<RoleServer>,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send + 'static {
        let writer = self.writer.clone();
        async move {
            let payload = serde_json::to_vec(&item).map_err(invalid_data_error)?;
            let mut guard = writer.lock().await;
            let framing = guard.framing.unwrap_or(StdioFraming::ContentLength);
            let output = guard.output.as_mut().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotConnected, "transport is closed")
            })?;
            match framing {
                StdioFraming::ContentLength => {
                    let header = format!("Content-Length: {}\r\n\r\n", payload.len());
                    output.write_all(header.as_bytes()).await?;
                    output.write_all(&payload).await?;
                }
                StdioFraming::JsonLine => {
                    output.write_all(&payload).await?;
                    output.write_all(b"\n").await?;
                }
            }
            output.flush().await?;
            Ok(())
        }
    }

    fn receive(&mut self) -> impl Future<Output = Option<RxJsonRpcMessage<RoleServer>>> + Send {
        async {
            match read_transport_message::<_, RxJsonRpcMessage<RoleServer>>(&mut self.reader).await {
                Ok(Some((message, framing))) => {
                    let mut guard = self.writer.lock().await;
                    if guard.framing.is_none() {
                        guard.framing = Some(framing);
                    }
                    Some(message)
                }
                Ok(None) => None,
                Err(error) => {
                    eprintln!("mcp transport read error: {error}");
                    None
                }
            }
        }
    }

    fn close(&mut self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        let writer = self.writer.clone();
        async move {
            let mut guard = writer.lock().await;
            guard.output = None;
            Ok(())
        }
    }
}

async fn read_transport_message<R, T>(
    reader: &mut BufReader<R>,
) -> Result<Option<(T, StdioFraming)>, std::io::Error>
where
    R: AsyncRead + Unpin,
    T: serde::de::DeserializeOwned,
{
    let mut first_line = String::new();
    let bytes_read = reader.read_line(&mut first_line).await?;
    if bytes_read == 0 {
        return Ok(None);
    }

    if let Some(message) = try_parse_json_line::<T>(&first_line)? {
        return Ok(Some((message, StdioFraming::JsonLine)));
    }

    let mut content_length = parse_content_length_header(&first_line)?;

    loop {
        let mut line = String::new();
        let bytes_read = reader.read_line(&mut line).await?;
        if bytes_read == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "unexpected EOF while reading MCP headers",
            ));
        }

        if line == "\r\n" {
            break;
        }

        if let Some(header_length) = parse_content_length_header(&line)? {
            content_length = Some(header_length);
        }
    }

    let content_length = content_length.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing Content-Length header",
        )
    })?;

    let mut payload = vec![0u8; content_length];
    reader.read_exact(&mut payload).await?;
    let message = serde_json::from_slice::<T>(&payload).map_err(invalid_data_error)?;
    Ok(Some((message, StdioFraming::ContentLength)))
}

fn invalid_data_error(error: impl std::error::Error + Send + Sync + 'static) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StdioFraming {
    ContentLength,
    JsonLine,
}

struct WriterState {
    output: Option<Stdout>,
    framing: Option<StdioFraming>,
}

fn try_parse_json_line<T>(line: &str) -> Result<Option<T>, std::io::Error>
where
    T: serde::de::DeserializeOwned,
{
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let first = trimmed.as_bytes()[0];
    if first != b'{' && first != b'[' {
        return Ok(None);
    }

    let message = serde_json::from_str::<T>(trimmed).map_err(invalid_data_error)?;
    Ok(Some(message))
}

fn parse_content_length_header(line: &str) -> Result<Option<usize>, std::io::Error> {
    let Some((name, value)) = line.split_once(':') else {
        return Ok(None);
    };

    if !name.eq_ignore_ascii_case("content-length") {
        return Ok(None);
    }

    let parsed = value.trim().parse::<usize>().map_err(invalid_data_error)?;
    Ok(Some(parsed))
}