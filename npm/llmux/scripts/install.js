// llmux npm postinstall：从 GitHub Release 下载当前平台的原生二进制到 vendor/。
// 设计约束：零 npm 依赖（发布后 install 不拉任何第三方包），下载/解压走系统自带工具。
"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const https = require("node:https");
const { execSync } = require("node:child_process");

const REPO = "zhMoody/llmux-cli-rs";
const PKG_DIR = path.join(__dirname, "..");
const VENDOR_DIR = path.join(PKG_DIR, "vendor");
const BIN_NAME = process.platform === "win32" ? "llmux.exe" : "llmux";

// process.platform / process.arch → cargo-dist target triple
function resolveTarget() {
  const key = `${process.platform}-${process.arch}`;
  const map = {
    "darwin-arm64": "aarch64-apple-darwin",
    "darwin-x64": "x86_64-apple-darwin",
    "linux-arm64": "aarch64-unknown-linux-gnu",
    "linux-x64": "x86_64-unknown-linux-gnu",
    "win32-x64": "x86_64-pc-windows-msvc",
  };
  const target = map[key];
  if (!target) {
    throw new Error(`不支持的平台: ${process.platform}-${process.arch}（llmux 暂无此平台的预编译包）`);
  }
  return target;
}

// 读 package.json 的 version 作为发布 tag 后缀（CI 发布前已用 tag 同步）
function resolveTag() {
  const pkg = JSON.parse(fs.readFileSync(path.join(PKG_DIR, "package.json"), "utf8"));
  return `v${pkg.version}`;
}

function httpsGetJson(url) {
  return new Promise((resolve, reject) => {
    https
      .get(url, { headers: { "User-Agent": "llmux-npm-installer" } }, (res) => {
        if (res.statusCode >= 400) {
          reject(new Error(`HTTP ${res.statusCode}: ${url}`));
          res.resume();
          return;
        }
        let body = "";
        res.on("data", (c) => (body += c));
        res.on("end", () => {
          try {
            resolve(JSON.parse(body));
          } catch {
            reject(new Error("响应不是合法 JSON"));
          }
        });
      })
      .on("error", reject);
  });
}

function httpsDownload(url, dest) {
  return new Promise((resolve, reject) => {
    const file = fs.createWriteStream(dest);
    https
      .get(url, { headers: { "User-Agent": "llmux-npm-installer" } }, (res) => {
        if (res.statusCode >= 400) {
          reject(new Error(`HTTP ${res.statusCode}: ${url}`));
          res.resume();
          return;
        }
        res.pipe(file);
        file.on("finish", () => file.close(resolve));
      })
      .on("error", (err) => {
        fs.rmSync(dest, { force: true });
        reject(err);
      });
  });
}

// 从 release 资产里选当前 target 的归档（优先 .tar.xz，其次 .zip）
async function resolveAsset(tag, target) {
  const data = await httpsGetJson(
    `https://api.github.com/repos/${REPO}/releases/tags/${encodeURIComponent(tag)}`,
  );
  if (!Array.isArray(data.assets)) {
    throw new Error(`找不到 Release ${tag}（可能尚未发布或 tag 与 package.json 版本不一致）`);
  }
  const prefix = `llmux-${target}.`;
  const candidates = data.assets
    .map((a) => a.name)
    .filter((n) => n.startsWith(prefix))
    .sort((a, b) => {
      const rank = (n) => (n.endsWith(".tar.xz") ? 0 : n.endsWith(".zip") ? 1 : 2);
      return rank(a) - rank(b);
    });
  const asset = candidates[0];
  if (!asset) {
    throw new Error(`Release ${tag} 中找不到 ${prefix}* 资产（target=${target}）`);
  }
  return `https://github.com/${REPO}/releases/download/${tag}/${encodeURIComponent(asset)}`;
}

// 递归在解压目录里找二进制（cargo-dist 归档可能带顶层目录）
function findBinary(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      const found = findBinary(p);
      if (found) return found;
    } else if (entry.name === BIN_NAME) {
      return p;
    }
  }
  return null;
}

function extract(archive) {
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "llmux-npm-"));
  try {
    if (process.platform === "win32") {
      // Windows 自带 tar.exe(bsdtar) 可解 zip；展开后目录结构保留
      execSync(`tar -xf "${archive}" -C "${tmp}"`, { stdio: "inherit" });
    } else {
      // macOS bsdtar 与 Linux GNU tar 都支持 -J(xz)
      execSync(`tar -xJf "${archive}" -C "${tmp}"`, { stdio: "inherit" });
    }
    const bin = findBinary(tmp);
    if (!bin) throw new Error("解压后未找到二进制文件");
    return bin;
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

async function main() {
  if (process.env.LLMUX_SKIP_DOWNLOAD === "1") return; // 便于离线调试/本地开发跳过
  const target = resolveTarget();
  const tag = resolveTag();
  console.log(`[llmux] 从 GitHub Release ${tag} 下载 ${target} 二进制...`);

  const assetUrl = await resolveAsset(tag, target);
  const archive = path.join(os.tmpdir(), `llmux-${target}-${Date.now()}.archive`);
  try {
    await httpsDownload(assetUrl, archive);
    const bin = extract(archive);
    fs.mkdirSync(VENDOR_DIR, { recursive: true });
    const dest = path.join(VENDOR_DIR, BIN_NAME);
    fs.copyFileSync(bin, dest);
    if (process.platform !== "win32") {
      fs.chmodSync(dest, 0o755);
    }
    console.log(`[llmux] 安装完成 → ${dest}`);
  } finally {
    fs.rmSync(archive, { force: true });
  }
}

main().catch((err) => {
  console.error(`[llmux] 二进制下载失败: ${err.message}`);
  console.error("[llmux] 可重试：npm rebuild llmux；若持续失败请检查网络或到 GitHub Releases 确认资产。");
  // 下载失败不阻断安装（bin/cli.js 会在缺少二进制时给出明确提示）
  process.exit(0);
});
