#!/usr/bin/env node
// llmux npm 包装器：把参数透传给 postinstall 下载到 vendor/ 的原生二进制。
// 用 node shim 而非直接指向二进制，可跨平台复用 npm 生成的 cmd shim。
"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const path = require("node:path");

const exeName = process.platform === "win32" ? "llmux.exe" : "llmux";
const binPath = path.join(__dirname, "..", "vendor", exeName);

if (!fs.existsSync(binPath)) {
  console.error(
    "[llmux] 未找到原生二进制 " + binPath +
    "。请确认 npm install 成功完成（postinstall 需联网下载 GitHub Release 资产）。",
  );
  process.exit(1);
}

// stdio 透传：保证 TUI / 交互式提示可用
const result = spawnSync(binPath, process.argv.slice(2), { stdio: "inherit" });
if (result.error) {
  console.error("[llmux] 启动失败: " + result.error.message);
  process.exit(1);
}
process.exit(result.status === null ? 1 : result.status);
