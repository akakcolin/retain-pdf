#!/usr/bin/env node
// `: any` 增量预算：只允许减少，不允许增加。
// 口径与评审一致：src/js/**/*.ts 中包含 ": any" 的行数。
// 降低存量后请同步下调 ANY_BUDGET，把红利锁住。

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const ANY_BUDGET = 127;

const frontendRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const scanRoot = path.join(frontendRoot, "src", "js");

function listTsFiles(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...listTsFiles(full));
    } else if (entry.isFile() && entry.name.endsWith(".ts")) {
      out.push(full);
    }
  }
  return out;
}

let count = 0;
const worst = [];
for (const file of listTsFiles(scanRoot)) {
  const lines = fs.readFileSync(file, "utf8").split("\n");
  let fileCount = 0;
  for (const line of lines) {
    if (line.includes(": any")) {
      fileCount += 1;
    }
  }
  if (fileCount > 0) {
    worst.push([fileCount, path.relative(frontendRoot, file)]);
    count += fileCount;
  }
}

worst.sort((a, b) => b[0] - a[0]);
console.log(`[any-budget] src/js ": any" 行数: ${count} / 预算 ${ANY_BUDGET}`);
console.log("[any-budget] 存量最多的文件:");
for (const [n, file] of worst.slice(0, 5)) {
  console.log(`  ${String(n).padStart(4)}  ${file}`);
}

if (count > ANY_BUDGET) {
  console.error(
    `\n[any-budget] 超出预算 ${count - ANY_BUDGET} 行。` +
    "新代码请写真实类型（或 unknown + 收窄），不要把债务再加回来。",
  );
  process.exit(1);
}
if (count < ANY_BUDGET) {
  console.log(`\n[any-budget] 存量下降了 ${ANY_BUDGET - count} 行，建议把 ANY_BUDGET 下调到 ${count} 锁住红利。`);
}
