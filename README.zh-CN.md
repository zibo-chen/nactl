# nactl

[English](README.md)

nactl 是一个用于管理 Nacos 与 r-nacos 配置项的 Rust CLI 和 MCP 网关。CLI 与 stdio MCP Server 复用同一套运行时配置，因此命令行工作流和编辑器 Agent 可以稳定地指向同一个服务、同一套认证信息。

## 功能概览

- 支持 login、config list/get/set/rm/edit，以及 stdio MCP Server 模式。
- 同时兼容 Nacos 与 r-nacos 部署。
- 连接配置可从命令行参数、配置文件、环境变量三层解析。
- login 成功后默认会把 access token 持久化到本地配置文件，可通过 --no-save-auth 关闭。
- 暴露精简的 MCP 工具面，便于在编辑器和 Agent 中自动化读写配置。

## 安装

从源码构建：

```bash
cargo build --manifest-path ./Cargo.toml --release
./target/release/nactl --help
```

也可以直接以开发模式运行：

```bash
cargo run --manifest-path ./Cargo.toml -- --help
```

当你推送 v* tag 时，GitHub Actions 会自动为 Linux、macOS、Windows 构建发布包，并上传到 GitHub Releases。

## 运行时配置

解析优先级如下：

1. 命令行参数
2. 配置文件
3. 环境变量

支持的环境变量：

- NACOS_SERVER 或 RNACOS_SERVER
- NACOS_HOST 搭配 NACOS_PORT
- NACOS_CONTEXT_PATH
- NACOS_NAMESPACE
- NACOS_USERNAME
- NACOS_PASSWORD
- NACOS_ACCESS_TOKEN
- NACOS_TIMEOUT_SECS

默认值：

- context path 默认为 /nacos。
- namespace 默认为 public。
- timeout 默认为 10 秒。

默认配置文件位于系统配置目录下的 nactl/config.yaml。如果历史上的 nacosctl/config.yaml 已存在，程序也会自动兼容读取。

示例配置文件：

```yaml
server: http://127.0.0.1:8848
context_path: /nacos
namespace: public
username: nacos
password: nacos
access_token: ""
timeout_secs: 10
```

## 快速开始

使用用户名密码登录，并把 access token 保存到本地配置文件：

```bash
cargo run --manifest-path ./Cargo.toml -- login \
  --server http://127.0.0.1:8848 \
  --username nacos \
  --password nacos
```

列出配置项：

```bash
cargo run --manifest-path ./Cargo.toml -- config list --group DEFAULT_GROUP
```

读取单个配置：

```bash
cargo run --manifest-path ./Cargo.toml -- config get application.yaml DEFAULT_GROUP
```

从文件写入配置内容：

```bash
cargo run --manifest-path ./Cargo.toml -- config set application.yaml DEFAULT_GROUP \
  --file ./application.yaml \
  --type yaml
```

在编辑器中直接修改配置：

```bash
cargo run --manifest-path ./Cargo.toml -- config edit application.yaml DEFAULT_GROUP
```

删除配置项：

```bash
cargo run --manifest-path ./Cargo.toml -- config rm stale.yaml DEFAULT_GROUP
```

脚本场景可切换为 JSON 输出：

```bash
cargo run --manifest-path ./Cargo.toml -- config list --output json
```

## MCP Server

使用和 CLI 相同的运行时配置启动 stdio MCP Server：

```bash
cargo run --manifest-path ./Cargo.toml -- mcp
```

推荐的 VS Code MCP 配置：

```json
{
  "servers": {
    "nactl": {
      "type": "stdio",
      "command": "/absolute/path/to/nactl/target/release/nactl",
      "args": ["mcp"]
    }
  },
  "inputs": []
}
```

当前 MCP 工具：

- nactl_auth_status
- nactl_config_list
- nactl_config_get
- nactl_config_set
- nactl_config_remove

## GitHub Actions CI

.github/workflows/ci.yml 会执行以下流程：

- 在推送到 main、目标分支为 main 的 pull request，以及 v* tag 上运行 cargo test。
- 在 Ubuntu、macOS、Windows 上执行测试矩阵。
- 在打 tag 时构建 x86_64 Linux、aarch64 Linux、x86_64 macOS、aarch64 macOS、x86_64 Windows 的发布包。
- 在推送 v* tag 后自动创建 GitHub Release。

## 开发

本地常用命令：

```bash
cargo fmt --all
cargo test --manifest-path ./Cargo.toml --locked
cargo run --manifest-path ./Cargo.toml -- config --help
```