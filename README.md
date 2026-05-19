# nactl

[简体中文](README.zh-CN.md)

nactl is a Rust CLI and MCP gateway for managing Nacos and r-nacos configuration entries. The same runtime configuration is shared by the CLI and the stdio MCP server, so shell workflows and editor agents can target the same service with the same credentials.

## Features

- Supports login, config list/get/set/remove/edit, and stdio MCP server mode.
- Works with both Nacos and r-nacos deployments.
- Resolves connection settings from CLI flags, config file, and environment variables.
- Persists an access token after login unless you opt out with --no-save-auth.
- Exposes a small MCP tool surface for config automation in editors and agents.

## Install

Build from source:

```bash
cargo build --manifest-path ./Cargo.toml --release
./target/release/nactl --help
```

Or run without a release build:

```bash
cargo run --manifest-path ./Cargo.toml -- --help
```

When you push a v* tag, GitHub Actions will build release archives for Linux, macOS, and Windows and publish them to GitHub Releases.

## Runtime Configuration

Resolution order is:

1. CLI flags
2. Config file
3. Environment variables

Supported environment variables:

- NACOS_SERVER or RNACOS_SERVER
- NACOS_HOST with NACOS_PORT
- NACOS_CONTEXT_PATH
- NACOS_NAMESPACE
- NACOS_USERNAME
- NACOS_PASSWORD
- NACOS_ACCESS_TOKEN
- NACOS_TIMEOUT_SECS

Defaults:

- Context path defaults to /nacos.
- Namespace defaults to public.
- Timeout defaults to 10 seconds.

The default config file path is the platform config directory under nactl/config.yaml. A legacy nacosctl/config.yaml path is still detected if it already exists.

Example config file:

```yaml
server: http://127.0.0.1:8848
context_path: /nacos
namespace: public
username: nacos
password: nacos
access_token: ""
timeout_secs: 10
```

## Quick Start

Log in with username/password and save the access token to the local config file:

```bash
cargo run --manifest-path ./Cargo.toml -- login \
  --server http://127.0.0.1:8848 \
  --username nacos \
  --password nacos
```

List config entries:

```bash
cargo run --manifest-path ./Cargo.toml -- config list --group DEFAULT_GROUP
```

Fetch one config item:

```bash
cargo run --manifest-path ./Cargo.toml -- config get application.yaml DEFAULT_GROUP
```

Write content from a file:

```bash
cargo run --manifest-path ./Cargo.toml -- config set application.yaml DEFAULT_GROUP \
  --file ./application.yaml \
  --type yaml
```

Edit a config item in your editor:

```bash
cargo run --manifest-path ./Cargo.toml -- config edit application.yaml DEFAULT_GROUP
```

Remove a config item:

```bash
cargo run --manifest-path ./Cargo.toml -- config rm stale.yaml DEFAULT_GROUP
```

Use JSON output for automation:

```bash
cargo run --manifest-path ./Cargo.toml -- config list --output json
```

## MCP Server

Start the stdio MCP server with the same runtime configuration used by the CLI:

```bash
cargo run --manifest-path ./Cargo.toml -- mcp
```

Recommended VS Code MCP configuration:

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

Current MCP tools:

- nactl_auth_status
- nactl_config_list
- nactl_config_get
- nactl_config_set
- nactl_config_remove

## GitHub Actions CI

The workflow in .github/workflows/ci.yml does the following:

- Runs cargo test on push to main, pull requests targeting main, and v* tags.
- Tests on Ubuntu, macOS, and Windows.
- Builds tagged release archives for x86_64 Linux, aarch64 Linux, x86_64 macOS, aarch64 macOS, and x86_64 Windows.
- Publishes GitHub Releases automatically when a v* tag is pushed.

## Development

Useful local commands:

```bash
cargo fmt --all
cargo test --manifest-path ./Cargo.toml --locked
cargo run --manifest-path ./Cargo.toml -- config --help
```