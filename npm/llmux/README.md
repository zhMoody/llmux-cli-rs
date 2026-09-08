# llmux-cli

[![License](https://img.shields.io/badge/License-AGPL--3.0-orange)](https://github.com/zhMoody/llmux-cli-rs/blob/main/LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80-orange?logo=rust)](https://rustup.rs/)

**LLMux — 本地优先的个人 AI API 网关与多路复用器** / A local-first personal AI API gateway and multiplexer.

> This is the **Rust rewrite** of the original TypeScript project. This npm package is a lightweight installer wrapper: on `postinstall` it downloads the platform-specific prebuilt binary from the GitHub Release — no `cargo`, no `bun`, no build step required.
>
> 这是原 TypeScript 项目的 **Rust 重构版**。本 npm 包是一个轻量安装壳：`postinstall` 时会从 GitHub Release 自动下载对应平台（macOS / Linux / Windows × arm64 / x64）的预编译二进制，无需安装 Rust 或 Bun。

## Install / 安装

```bash
npm install -g llmux-cli
```

Pin to the latest version explicitly (optional) / 也可显式锁 `latest`（可选）：

```bash
npm install -g llmux-cli@latest
```

## Usage / 使用

After install, the `llmux` command is available on your PATH:

```bash
# 启动 server + TUI（默认）
llmux

# 无 TUI 的 server 模式
llmux start --no-tui

# 指定端口
llmux start --no-tui --port 25999
```

Open the management dashboard at `http://localhost:25975`.

## What it does / 它能做什么

- **统一入口** — 客户端只需指向 `http://localhost:25975/v1`，即可访问所有已配置的 provider 与模型（OpenAI / Anthropic / Gemini 协议透传）
- **粘性会话 + 智能 failover** — 请求固定首选账户保持 prompt cache 热度，故障自动切备用账户并指数退避探测恢复
- **模型 alias** — 冗长模型 ID 映射为短别名，随时替换底层模型
- **API Key 白名单** — 为网关密钥配置模型访问白名单，隔离真实凭证
- **一键工具配置** — 内置 Claude Code / Codex / Gemini CLI / VS Code 快速配置向导

## Other install methods / 其他安装方式

| Platform | Command |
| --- | --- |
| Homebrew | `brew install zhmoody/tap/llmux` |
| Linux | `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/zhMoody/llmux-cli-rs/releases/latest/download/llmux-installer.sh \| sh` |
| Windows | `powershell -ExecutionPolicy Bypass -c "irm https://github.com/zhMoody/llmux-cli-rs/releases/latest/download/llmux-installer.ps1 \| iex"` |

## Requirements / 环境要求

- **Node.js ≥ 18**（仅安装期需要；安装完成后二进制运行不依赖 Node）
- 下载需联网访问 GitHub Releases；若网络受限可改用上述 curl / brew 安装方式

## Troubleshooting / 常见问题

二进制下载失败时（网络 / 代理问题）：

```bash
# 重新触发 postinstall 下载
npm rebuild llmux-cli

# 或跳过下载（适用于本地已手动放置二进制的场景）
LLMUX_SKIP_DOWNLOAD=1 npm rebuild llmux-cli
```

## Docs / 文档

完整文档（English / 中文）：<https://github.com/zhMoody/llmux-cli-rs>

## License

[AGPL-3.0](https://github.com/zhMoody/llmux-cli-rs/blob/main/LICENSE)
