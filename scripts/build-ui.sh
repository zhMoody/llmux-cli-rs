#!/usr/bin/env bash
# 构建前端产物，供 rust_embed 在编译期嵌入二进制。
# cargo-dist 在 `dist build`（cargo build）之前调用本脚本；缺少 llmux_ui/dist 会导致编译失败。
set -euo pipefail

# 脚本路径 → 仓库根 → llmux_ui
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT/llmux_ui"

# 确认 bun 可用（macOS/Linux CI runner 通常未预装）
if ! command -v bun >/dev/null 2>&1; then
    echo "bun not found; installing to ~/.bun ..."
    curl -fsSL https://bun.sh/install | bash >/dev/null
    export PATH="$HOME/.bun/bin:$PATH"
fi

bun install
bun run build
echo "✅ Frontend built into $ROOT/llmux_ui/dist"
