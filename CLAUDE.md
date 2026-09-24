# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目简介

LLMux 是一个个人、本地优先的 AI API gateway 和 multiplexer。客户端工具（Claude Code / Codex / Gemini CLI 等）只需指向统一端点 `http://localhost:25975/v1`，LLMux 负责把请求路由到多个厂商账户，支持多协议透传、粘滞会话自动 failover、模型 alias 映射、API key 白名单。全部本地运行，数据不出机器。

这是原 TypeScript 版本的 Rust 重写（2026-08 重写）。前端 Web UI 的唯一维护位置是 `llmux_ui/`（`TS/llmux_ui` 已废弃，勿编辑）。

## 构建 / 运行 / 测试

```bash
# Rust 构建（可执行 crate 的 package 名是 llmux，不是 llmux-bin —— 那是目录名）
cargo build                                        # 全部
cargo build --release -p llmux                     # 单个二进制

# 运行：默认启动 server + TUI；--no-tui 跑无头 server
cargo run -p llmux
cargo run -p llmux -- start --no-tui
cargo run -p llmux -- start --no-tui --port 25999

# 测试：3 个集成测试文件，全部在 crates/*/tests/；server 用 sqlite::memory:，无需真实 DB/网络
cargo test                                         # 全部
cargo test -p llmux-core --test core_contract      # DB / schema / 加解密 / 导入导出
cargo test -p llmux-core --test gateway_contract   # 适配器 / 透传 / SSE usage / failover 排序
cargo test -p llmux-server --test server_contract <test_name>   # 单个用例
```

没有 `src/` 内联单测，没有 `#[ignore]`，没有 llmux-bin 测试。

```bash
# 前端（UI 先构建产物 llmux_ui/dist，Rust 通过 rust-embed 编译期打进二进制）
cd llmux_ui && bun install && bun run build        # = tsc && vite build（严格 TS，无增量缓存）
cd llmux_ui && bun run lint                        # eslint，max-warnings 0
cd llmux_ui && bun run dev                         # 开发 server :24444，/api 代理到 :25999
```

前端**零测试**（无 vitest/test script）。

### 发布

```bash
scripts/release.sh [version]    # 自增或指定版本 → 改 Cargo.toml → 同步 Cargo.lock → commit + tag + push
```

打 tag 触发 `.github/workflows/release.yml`（cargo-dist 0.32 生成，勿手改）：plan → build → GitHub Release → homebrew / npm。**目标矩阵的权威定义在 `dist-workspace.toml`**（5 个 target、`npm-package = "llmux-cli"`、tap `zhMoody/homebrew-tap`），不在 workflow 里；npm 发布走 reusable `publish-npm.yml`（仅 `workflow_call`，依赖 npm Trusted Publisher 指向 `release.yml`）。

## 端口 / 数据库隔离（重要约定）

`.env`（已 gitignore）控制端口和数据目录，优先级：进程环境变量 > `.env` 文件 > 内置默认值（见 `llmux-core/src/config.rs::from_env`，实际只读 `PORT` / `DATA_DIR` / `MASTER_KEY` 三个；`USAGE_RETENTION_DAYS` 由 `llmux-bin/src/main.rs` 直接读 env，不经 config）。

| 环境 | 端口 | DATA_DIR | DB 文件 |
|------|------|----------|---------|
| 线上/生产 | `25975` | `~/.config/llmux`（默认） | `<DATA_DIR>/llmux_db.db` |
| 测试 | `25999` | `~/.config/llmux-repair` | `<DATA_DIR>/llmux_db.db` |

改动/验证时务必确认 `.env` 指向测试环境，避免误碰线上库。日志级别走 `RUST_LOG`（默认 `llmux=info,tower_http=info`），**不是** README 里写的 `LOG_LEVEL`；DB 文件名是 `llmux_db.db`，**不是** README 里的 `db.sqlite` —— README 这两处是错的。

## 架构概览

Cargo workspace，三个 crate + 一个前端。`Cargo.lock` 被 gitignore，不进版本库。

- **`crates/llmux-core`** — 领域层，无 HTTP：
  - `dispatcher.rs` — 路由与粘滞会话状态机的核心：
    - `sanitize_model_name`（剥 ANSI 与 Claude Code 的 `[1m]` 长上下文后缀）→ `resolve_model`：alias 绑定账户集 > alias 绑 vendor > 前缀回退（`claude-`→anthropic、`gemini-`/`models/gemini-`→gemini、其余→openai）。
    - `DispatchRouter::select(dispatch_key, accounts, preferred_id) -> (Vec<Account>, DispatchMeta)` 按 sticky 状态排出尝试顺序；`record_result(...)` 迁移状态。`Primary` 失败转 `Fallback`；连续 5 次成功或超过 backoff 才探测首选账户（30s 起，翻倍封顶 600s）；超过 1024 条目按「只保留 Fallback」淘汰。
    - 状态以内存为准（`Arc<Mutex<DispatchRouter>>`），通过 `snapshot()` / `restore()` 与 `dispatch_state` 表往返：启动时由 `main.rs` 载入，运行中由 `llmux-server` 的 `dispatch_flush` 任务每 1s 把**脏**状态全量重写落盘（快照在锁内取、DB 写在锁外）。`Instant` 计时在落盘时换算成墙钟毫秒，因此**进程停机时长会自动计入探测退避**——停满 `probe_backoff_secs` 后重启，首个请求即触发探测。
    - ⚠️ `is_retryable_status` **只覆盖 401/403/429，5xx 不重试**，且被测试锁定（改行为需同步改测试）。
  - `adapters/mod.rs` — 单一文件（无 openai.rs/anthropic.rs 拆分）：`Account`/`ChatRequest`/`ProviderRequest` 等 VO + `build_openai_request` / `build_openai_passthrough` + `execute_provider_request`（全局 `OnceLock` reqwest client）+ `test_provider_connection`（401/403 视为连通性通过）。
  - `proxy/mod.rs` — **Anthropic 透传在这，不在 adapters**：`build_anthropic_passthrough_request` + SSE/JSON 的 usage token 解析（逐字段取 max，非累加）。
  - `db.rs` / `migrations/0001_init.sql` — SQLite 初始化，10 张表。schema 是 2026-08-05 推倒重来的全新版；0.3.x 旧库检测到后备份为 `<db>.legacy-<unix>.bak` 并重建空库，**不迁移旧数据**。改 schema 后需删库重建或用 `api/settings/reset`（purge_database）。
  - `repo.rs` 数据访问查询；`models.rs` 领域类型；`crypto.rs` AES-256-GCM + scrypt（密文自描述 `v2:{log_n}:{salt}:{nonce}:{ct}`）；`settings.rs` `app_settings` KV + gateway_key；`usage.rs` 用量查询与清理；`export_import.rs` 配置导入导出。

- **`crates/llmux-server`** — axum HTTP 层：
  - `app.rs` — 路由注册 + `AppState`（pool / master_key / dispatch_router / models_cache / test_queue / tui_tx）；`test_state()` 返回 in-memory sqlite 的 state（测试入口）。`app(state)` + `test_request(...)`（`tower::ServiceExt::oneshot` 封装，测试不起 TCP）。
  - **鉴权边界**：`v1_auth_middleware` 用 `.route_layer` 挂载，按 axum 语义**只作用于此前注册的 `/v1*` 路由** —— 因此 **`/api/*` 与 `/swagger` 完全无鉴权**（本地工具的设计选择，改动时勿误以为有保护）。
  - `middleware.rs` — key 提取优先级 `x-api-key` > `x-goog-api-key` > `Authorization: Bearer`；只注入 `AuthContext{key_name, allowed_models}`，**中间件不做模型校验**，校验在 handler 调 `is_model_allowed`（JSON 数组精确匹配，无通配）。
  - `routes/v1/` — 上游代理：`openai.rs`（`chat_completions`/`responses` 共用 `openai_dispatch`，含 failover 循环）、`anthropic.rs`（`messages`）、`gemini.rs`（`:model_and_action`）、`models.rs`（`/v1/models`，按 `x-api-key`/`anthropic-version` 头返回 Anthropic 或 OpenAI 形状）、`helpers.rs`。
    - 双重前缀：`/v1/v1/*` 是**额外注册的 10 条重复路由**（兼容 `ANTHROPIC_BASE_URL=/v1`）；`normalize_gateway_uri`（在 `app.rs`，不在 routes/v1）只用于日志/TUI 路径上报。
    - SSE：三个 streaming passthrough 同构 —— 上游 `bytes_stream()` 映射成 `axum::Error` 后 `Body::from_stream`，**字节原样透传，无 SSE 解析/分帧**；usage 也不解析流，`tokio::spawn` 里直接记成功。
  - `routes/` 根下是 `/api/*` 管理端点（**无 `routes/api/` 目录**）：`accounts.rs` / `vendors.rs` / `keys.rs` / `settings.rs` / `usage.rs` / `health.rs` / `auth.rs`（`/api/auth/web-session`），另有子目录 `routes/models/`（available / aliases / health / testing）与 `routes/system/`（claude / codex / gemini，读写本机 CLI 配置 + 备份 + preview）。`/api/activity/stream` 是唯一用 axum 官方 `Sse` 的端点（1.5s 轮询 + 心跳注释）。
  - **错误处理没有自定义 Error enum，也没有 `IntoResponse`**（依赖了 `thiserror` 但全 crate 未用），两套并行构造器：`error.rs`（OpenAI 形状，给 `/api/*` 和 fallback 用）与 `middleware::send_error`（按 `is_anthropic` 二选一，给 `/v1/*` 用 —— 注意 Gemini 客户端实际拿到的是 OpenAI 形状）。
  - `static_ui.rs` — rust-embed（`#[folder = "../../llmux_ui/dist"]`）把前端打进二进制，SPA fallback 到 index.html，带 `..`/`\` 穿越防护。`/api/*` 与 `/v1/*` 之外走 SPA。**`llmux_ui/dist` 是有意纳入版本库的**（根 `.gitignore` 忽略 `dist/`，但 `llmux_ui/.gitignore` 用 `!dist/**` 反向放开），删掉它会导致 release 构建失败。
  - `api_docs.rs` / `api_schemas.rs` — utoipa Swagger，`/swagger` + `/api-docs/openapi.json`（覆盖不全，勿当权威端点清单）。

- **`crates/llmux-bin`** — package 名 `llmux`：clap CLI（`start` 带 `--port`/`--no-tui`；`status`/`stop` 是只 println 的占位）。TUI 模式由 `tokio::spawn` 后台跑 `axum::serve`、主线程阻塞跑 `tui::run_tui`，TUI 退出后 `server.abort()`；无 TUI 则前台 serve。`tui.rs` 只有 2 个页面（Dashboard / Traffic），日志上限 500 条。

- **`llmux_ui/`** — React 18 + TS + Vite + **Tailwind v3** + zustand + framer-motion。`src/api/`（单例 axios `client.ts` + 7 个领域模块，无统一 `request<T>()`，响应拦截器把 `AxiosError` 统一转 `Error`）、`src/pages/`、`src/components/ui/`（14 个自制基础组件，无组件库）、`src/i18n/`（自研轻量方案，**三语 zh/en/ja**，扁平点号 key，回退链 当前语言 → en → key）、`src/router/`。服务端状态不走 zustand，而是 `hooks/useCachedData.ts`（模块级 Map 缓存 + TTL 30s 静默刷新）。Vite dev :24444 把 `/api` 代理到测试实例 `:25999`。

### 请求流转（一条 /v1 请求）

1. `v1_auth_middleware` 校验网关 key，注入 `AuthContext`（含 allowed_models）。
2. handler 校验模型白名单 → `resolve_model` 解析 alias → 按协议过滤账户（`openai`/`custom`，或 gemini + `openai_compatible`）→ `DispatchRouter::select` 得到有序账户列表。
3. `adapters`/`proxy` 构造上游请求（协议透传，只加认证头 + 改写 model），`execute_provider_request` 发出；失败时按 `is_retryable_status` 换下一个账户。
4. SSE 流式原样回传；`usage_logs` 记录本次请求（**无 token 列**，仅成功率/时延），`model_health` 记录拨测结果。

### 数据模型关键点（schema `0001_init.sql`，10 张表）

- `vendors` — 厂商目录，18 个内置种子 + 用户自建；`protocols`（JSON 数组）、`openai_responses`、`coding_plan`（火山方舟等 coding plan 套餐开关）、`default_*_url` / `coding_*_url`。
- `accounts` — 上游账户，`api_key_enc` 加密存储，`openai_compatible` 标记，`weight` 权重。
- `model_aliases` + `model_alias_accounts` — alias 与账户绑定，`position` / `is_preferred`；部分唯一索引强制每 alias 最多一个首选。
- `api_keys` + `api_key_models` — 网关 key（**明文存储**，单用户本地场景的取舍）与其模型白名单（空表 = 不限制）。
- `usage_logs` / `model_health` / `dispatch_state`（建而未用）/ `app_settings` — 监控、健康、运行时、类型化 KV。

## 关键工程陷阱

- **前端 `dist` 必须构建并提交**：CI 里没有任何 bun 步骤（`scripts/build-ui.sh` 无人调用），release 编译时 `rust-embed` 直接嵌入仓库里已提交的 `llmux_ui/dist`。忘了构建提交，发布出去的就是旧 UI，且**静默无报错**。
- **改 schema 必须删库重建**（或走 `api/settings/reset`），没有增量迁移。
- `docs/release.md` 部分内容已失效：`gh workflow run publish-npm.yml` 不可用（该 workflow 只有 `workflow_call`），`npm/llmux/package.json` 已不存在（npm 包描述由 cargo-dist 依据 `dist-workspace.toml` + `crates/llmux-bin/Cargo.toml` 的 `readme` 字段生成）。
- 前端大文件：`ModelBrowser.tsx`(697) / `Dashboard.tsx`(497) / `CodexPanel.tsx`(388)，改动风险集中在这几处。

## 代码约定

遵循仓库/全局规范：注释与沟通用简体中文、专业术语保留英文；禁止 `any`（用 `unknown` + 类型收窄）；复杂逻辑必须写注释；优先编辑已有文件、小步改动；正式文档放 `docs/`（specs/plans），方案讨论放 `discuss/`，日志用 `tracing` 按模块提升日志级别（`llmux=debug`）。
