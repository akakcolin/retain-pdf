#!/usr/bin/env node
// js/ 归属门禁（ADR 0009）：每个 src/js 源文件必须在 js-ownership.json 登记
// keep/migrate，未登记的新模块直接失败；keep 数量只降不升。
// 存量下降后请同步下调 keepBudget 锁住红利。

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const LABELS = new Set(["keep", "migrate", "delete"]);
const SOURCE_EXT = [".ts", ".tsx", ".js", ".jsx"];

const frontendRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const jsRoot = path.join(frontendRoot, "src", "js");
const manifestPath = path.join(frontendRoot, "js-ownership.json");

function listSourceFiles(dir) {
  const out = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name.startsWith("._")) {
      continue;
    }
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...listSourceFiles(full));
    } else if (entry.isFile() && SOURCE_EXT.some((ext) => entry.name.endsWith(ext))) {
      out.push(path.relative(frontendRoot, full));
    }
  }
  return out.sort();
}

const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
if (manifest.schema !== "frontend_js_ownership_v1") {
  console.error(`[js-ownership] schema 不匹配: ${manifest.schema}`);
  process.exit(1);
}
const labels = manifest.files || {};
const budget = Number(manifest.keepBudget);

const onDisk = listSourceFiles(jsRoot);
const onDiskSet = new Set(onDisk);
const unlabeled = onDisk.filter((file) => !(file in labels));
const stale = Object.keys(labels).filter((file) => !onDiskSet.has(file));
const unknown = onDisk.filter((file) => file in labels && !LABELS.has(labels[file]));

const counts = { keep: 0, migrate: 0, delete: 0 };
for (const file of onDisk) {
  const label = labels[file];
  if (label in counts) {
    counts[label] += 1;
  }
}

console.log(
  `[js-ownership] src/js 源文件: ${onDisk.length}  ` +
  `keep ${counts.keep} / migrate ${counts.migrate} / delete ${counts.delete}  ` +
  `keepBudget ${budget}`,
);

let failed = false;
if (unlabeled.length > 0) {
  failed = true;
  console.error(`\n[js-ownership] 以下 ${unlabeled.length} 个文件未登记归属，` +
    "新模块必须先写入 js-ownership.json（ADR 0009：js/ 不再承接新逻辑）：");
  for (const file of unlabeled) {
    console.error(`  ${file}`);
  }
}
if (stale.length > 0) {
  failed = true;
  console.error(`\n[js-ownership] 以下 ${stale.length} 个登记条目已无对应文件，请从清单删除：`);
  for (const file of stale) {
    console.error(`  ${file}`);
  }
}
if (unknown.length > 0) {
  failed = true;
  console.error(`\n[js-ownership] 非法标签（仅允许 keep/migrate/delete）：`);
  for (const file of unknown) {
    console.error(`  ${file}: ${labels[file]}`);
  }
}
if (counts.keep > budget) {
  failed = true;
  console.error(`\n[js-ownership] keep 数量 ${counts.keep} 超出预算 ${budget}。` +
    "新逻辑写 pages/，不要把 js/ 保留面继续做大。");
} else if (counts.keep < budget) {
  console.log(`\n[js-ownership] keep 下降 ${budget - counts.keep} 个，建议把 keepBudget 下调到 ${counts.keep} 锁住红利。`);
}
if (failed) {
  process.exit(1);
}
