# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目简介

LLMux 是一个个人、本地优先的 AI API gateway 和 multiplexer。客户端工具（Claude Code / Codex / Gemini CLI 等）只需指向统一端点 `http://localhost:25975/v1`，LLMux 负责把请求路由到多个厂商账户，支持多协议透传、粘滞会话自动 failover、模型 alias 映射、API key 白名单。全部本地运行，数据不出机器。

这是原 TypeScript 版本的 Rust 重写（2026-08 重写）。前端 Web UI 的唯一维护位置是 `llmux_ui/`（`TS/llmux_ui` 已废弃，勿编辑）。

## 构建 / 运行 / 测试

```bash
# Rust 构建（可执行 crate 的 package 名是 llmux，不是 llmux-bin）
cargo build                                        # 全部
cargo build --release -p llmux                     # 单个二进制

# 运行：默认启动 server + TUI；--no-tui 跑无头 server
cargo run -p llmux
cargo run -p llmux -- start --no-tui
cargo run -p llmux -- start --no-tui --port 25999

# 测试（集成测试在 crates/*/tests/*.rs；server 用 sqlite::memory:，无需真实 DB/网络）
cargo test                                         # 全部
cargo test -p llmux-core --test gateway_contract   # 单文件
cargo test -p llmux-server --test server_contract <test_name>   # 单个用例

# 前端（UI 先构建产物 llmux_ui/dist，Rust 通过 rust-embed 打进二进制）
cd llmux_ui && bun install && bun run build        # = tsc && vite build（严格 TS）
cd llmux_ui && bun run lint                        # eslint，max-warnings 0
cd llmux_ui && bun run dev                         # 开发 server :24444，/api 代理到 :25999
```

CI（`.github/workflows/publish.yml`）在 release published 时触发：先 `bun install && bun run build` 构建 UI，再按目标矩阵 `cargo build --release`，上传二进制 + sha256。发布前需确保 `llmux_ui/dist` 是最新构建。

## 端口 / 数据库隔离（重要约定）

`.env`（已 gitignore）控制端口和数据目录，优先级：进程环境变量 > `.env` 文件 > 内置默认值（见 `llmux-core/src/config.rs::from_env`）。

| 环境 | 端口 | DATA_DIR | DB 文件 |
|------|------|----------|---------|
| 线上/生产 | `25975` | `~/.config/llmux`（默认） | `<DATA_DIR>/llmux_db.db` |
| 测试 | `25999` | `~/.config/llmux-repair` | `<DATA_DIR>/llmux_db.db` |

改动/验证时务必确认 `.env` 指向测试环境，避免误碰线上库。相关环境变量：`PORT` / `LOG_LEVEL` / `DATA_DIR` / `MASTER_KEY` / `USAGE_RETENTION_DAYS`（启动时清理超过该天数的 usage_logs）。

## 架构概览

Cargo workspace，三个 crate + 一个前端：

- **`crates/llmux-core`** — 领域层，无 HTTP：
  - `adapters/` — 构造各厂商上游请求（openai/anthropic/gemini 协议透传）、`execute_provider_request`（reqwest，复用单例 client）。
  - `dispatcher.rs` — 粘滞会话路由状态机：解析 model alias → 目标模型 + 厂商 + 账户集（`model_alias_accounts` 绑定，单账户标 `is_preferred`）；按 stickiness 选择首选/回退账户，失败自动 failover + 指数退避探测，状态持久化到 `dispatch_state` 表。
  - `db.rs` / `migrations/` — SQLite 初始化。schema 是 2026-08-05 推倒重来的全新版；0.3.x 旧库检测到后备份为 `<db>.legacy-<unix>.bak` 并重建空库，**不迁移旧数据**。改 schema 后需删库重建或用 `api/settings/reset`（purge_database）。
  - `repo.rs` — 数据访问查询函数；`models.rs` — 领域类型；`crypto.rs` — AES-GCM 加解密厂商 key。
  - `usage.rs` — 用量上报与过期清理；`export_import.rs` — 配置导入导出。

- **`crates/llmux-server`** — axum HTTP 层：
  - `app.rs` — 路由注册 + `AppState`（pool / master_key / dispatch_router / models_cache / tui_tx）；`test_state()` 返回 in-memory sqlite 的 state（测试入口）。
  - `middleware.rs::v1_auth_middleware` — 校验网关 API key（Bearer / `x-api-key` / `x-goog-api-key`），注入 `AuthContext`（含 allowed_models 白名单）。
  - `routes/v1/` — 上游代理端点：`/v1/chat/completions` `/v1/responses` `/v1/messages` `/v1/models`，以及 `/v1beta/*`（gemini）。兼容 `ANTHROPIC_BASE_URL=/v1` 造成的 `/v1/v1/` 双重前缀（`normalize_gateway_uri`）。
  - `routes/api/` — 管理 API：accounts/vendors/models/keys/settings/usage/health/system（写 Claude Code / Codex / Gemini CLI 配置）。
  - `static_ui.rs` — rust-embed 把 `llmux_ui/dist` 打进二进制，SPA fallback 到 index.html；`/api/*` 与 `/v1/*` 之外的路径走 SPA。
  - `api_docs.rs` / `api_schemas.rs` — utoipa Swagger，`/swagger` + `/api-docs/openapi.json`。

- **`crates/llmux-bin`** — `llmux` 二进制：clap CLI（`start` / `status` / `stop`，后两者为占位），任务拆分：起 server + 可选 ratatui TUI（`tui.rs` 在主线程阻塞运行，server 在 tokio task 后台）。

- **`llmux_ui/`** — React + TS + Vite + Tailwind + zustand + framer-motion。`src/api/` 封装后端 REST，`src/pages/` 对应各管理页，`src/components/ui/` 基础组件，`src/i18n/` 中英双语，`src/router/` 路田。Vite dev :24444 把 `/api` 代理到测试实例 `:25999`。

### 请求流转（一条 /v1 请求）

1. `v1_auth_middleware` 校验网关 key 并注入 `AuthContext`（含模型白名单）。
2. handler 解析 model alias → `DispatchRouter::select` 得到厂商/账户/首选账户。
3. `adapters::build_*_request` 构造上游请求（协议透传），`execute_provider_request` 发出，SSE 流式回传客户端。
4. `usage_logs` 记录本次请求（无 token 列，仅成功率/时延）；`model_health` 记录拨测结果。

### 数据模型关键点（schema `0001_init.sql`）

- `vendors` — 厂商目录，内置种子 + 用户自建；`protocols`（JSON 数组）、`openai_responses`、`coding_plan`（火山方舟等 coding plan 套餐开关）。
- `accounts` — 上游账户，`api_key_enc` 加密存储，`openai_compatible` 标记，`weight` 权重。
- `model_aliases` + `model_alias_accounts` — alias 与账户绑定，`position` / `is_preferred`。
- `api_keys` + `api_key_models` — 网关 key（明文）与其模型白名单。
- `usage_logs` / `model_health` / `dispatch_state` / `app_settings` — 监控、健康、运行时、类型化 KV。

## 代码约定

遵循仓库/全局规范：注释与沟通用简体中文、专业术语保留英文；禁止 `any`（用 `unknown` + 类型收窄）；复杂逻辑必须写注释；优先编辑已有文件、小步改动；正式文档放 `docs/`（specs/plans），方案讨论放 `discuss/`，日志用 `tracing` 按模块提升日志级别（`llmux=debug`）。

schema 推倒重来需设计评审时，参考 `docs/superpowers/specs/2026-08-05-llmux-db-redesign-design.md` 与 `docs/2026-08-05-db-redesign-implementation.md`。
