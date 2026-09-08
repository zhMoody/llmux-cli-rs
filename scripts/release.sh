#!/usr/bin/env bash
# 一键发布：bump 版本 → 同步 Cargo.lock → 提交 → 打 tag → 推送。
# 版本号只在这里出现一次：
#   ./scripts/release.sh          # patch 自增（0.5.22 → 0.5.23）
#   ./scripts/release.sh minor    # minor 自增（0.5.x → 0.6.0）
#   ./scripts/release.sh major    # major 自增（0.x → 1.0.0）
#   ./scripts/release.sh 0.6.0    # 显式指定版本
#
# 之后 cargo-dist（release.yml）自动完成：构建 5 平台 → 建 GitHub Release →
# 推 Homebrew tap → 经 custom-publish-npm（OIDC）发布 npm（llmux-cli）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# ── 0. 前置检查 ──────────────────────────────────────────────
if ! git diff --quiet || ! git diff --cached --quiet; then
    echo "❌ 工作区有未提交改动，请先 commit 或 stash。" >&2
    exit 1
fi

CUR="$(grep -m1 '^version' Cargo.toml | sed -E 's/version = "([^"]+)"/\1/')"
[ -n "$CUR" ] || { echo "❌ 无法从 Cargo.toml 读取当前版本" >&2; exit 1; }

# ── 1. 计算新版本 ────────────────────────────────────────────
NEW="$(node -e '
const cur = process.argv[1];
const arg = process.argv[2];
const [maj, min, pat] = cur.split(".").map(Number);
let v;
if (/^\d+\.\d+\.\d+$/.test(arg)) {
  v = arg;
} else if (arg === "major") {
  v = `${maj + 1}.0.0`;
} else if (arg === "minor") {
  v = `${maj}.${min + 1}.0`;
} else if (arg === undefined || arg === "" || arg === "patch") {
  v = `${maj}.${min}.${pat + 1}`;
} else {
  console.error(`未知参数: ${arg}（可用 patch|minor|major|X.Y.Z）`);
  process.exit(1);
}
console.log(v);
' "$CUR" "${1:-patch}")"

echo "当前版本: $CUR"
echo "新版本:   $NEW"
read -r -p "确认发布 v$NEW ? [y/N] " ans
[[ "$ans" =~ ^[yY]$ ]] || { echo "已取消"; exit 1; }

# ── 2. 可选：npm 线上版本冲突预检（网络失败仅警告）────────────
# npm 包装包名是 llmux-cli（cargo-dist 的 npm-package 配置），版本必须 > 线上。
if command -v npm >/dev/null 2>&1; then
    ONLINE="$(npm view llmux-cli version 2>/dev/null || true)"
    if [ -n "$ONLINE" ]; then
        # 仅当新版本 > 线上版本才放行
        if node -e '
            const a = process.argv[1].split(".").map(Number);
            const b = process.argv[2].split(".").map(Number);
            const ok = a[0] > b[0]
                || (a[0] === b[0] && a[1] > b[1])
                || (a[0] === b[0] && a[1] === b[1] && a[2] > b[2]);
            process.exit(ok ? 0 : 1);
        ' "$NEW" "$ONLINE"; then
            echo "✓ npm 线上 $ONLINE → 将发 $NEW"
        else
            echo "❌ npm 线上已是 $ONLINE（≥ $NEW），无法发布。请升更高版本。" >&2
            exit 1
        fi
    fi
fi

# ── 3. 同步版本号（Cargo.toml + Cargo.lock；npm 版本由 dist 自动跟随）───────
sed -i '' "s/^version = \"$CUR\"/version = \"$NEW\"/" Cargo.toml
# 同步 Cargo.lock（cargo metadata 会按 workspace 重写 lock）
cargo metadata --no-deps --format-version 1 >/dev/null 2>&1 || {
    echo "⚠️ cargo metadata 失败，Cargo.lock 可能未同步"; }

echo "✓ 版本号已同步: Cargo.toml / Cargo.lock"

# ── 4. 提交 + 打 tag + 推送 ──────────────────────────────────
git add Cargo.toml Cargo.lock
git commit -m "release: v$NEW"
git tag "v$NEW"
git push origin main
git push origin "v$NEW"

echo ""
echo "✅ 已推送 main + tag v$NEW，cargo-dist 正在构建 Release（约 7-10 分钟）"
echo "   查看进度: gh run watch"
echo "   完成后自动发布：GitHub Release / Homebrew / npm(llmux-cli@$NEW)"
