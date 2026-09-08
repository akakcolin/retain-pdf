import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const PROJECT_ROOT = process.cwd();
const JS_ROOT = join(PROJECT_ROOT, "src/js");

const { TEXT_KEYS } = await import("../src/shared/dom/text-keys.js");

function listTsFiles(dir) {
  const out = [];
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    if (statSync(full).isDirectory()) {
      out.push(...listTsFiles(full));
    } else if (entry.endsWith(".ts")) {
      out.push(full);
    }
  }
  return out;
}

test("TEXT_KEYS 值唯一且保持 kebab-case", () => {
  const values = Object.values(TEXT_KEYS);
  assert.equal(new Set(values).size, values.length, "文案槽位 key 不得重复");
  for (const value of values) {
    assert.match(value, /^[a-z0-9]+(?:-[a-z0-9]+)*$/, `非法 key: ${value}`);
  }
});

test("src/js 的 setText 调用一律走 TEXT_KEYS 常量，禁止字符串字面量", () => {
  const offenders = [];
  for (const file of listTsFiles(JS_ROOT)) {
    const src = readFileSync(file, "utf8");
    // 允许 text-keys.ts 自身（常量定义处）出现 "key" 字面量
    if (file.endsWith("dom/text-keys.ts")) {
      continue;
    }
    const matches = src.match(/setText\(\s*["'`]/g);
    if (matches) {
      offenders.push(file);
    }
  }
  assert.deepEqual(offenders, [], "setText 字面量调用必须改用 TEXT_KEYS 常量");
});

test("composition 装配顺序契约：workflowDialog.bindEvents 先于 createRuntimeFeatures", () => {
  // 契约来源：recent-jobs 的 scheduleRefresh 同步读 isWorkflowOpen(DOM data-open)；
  // 若 workflow close() 还没把 data-open 写成 0，刷新会被 isSuspended 吞掉。
  // 见 composition/README.md 第 5 条与 composition.ts 行内注释。
  const src = readFileSync(join(PROJECT_ROOT, "src/pages/home/composition.ts"), "utf8");
  const bindIndex = src.indexOf("workflowDialog.bindEvents()");
  const runtimeIndex = src.indexOf("createRuntimeFeatures(");
  assert.ok(bindIndex > 0, "composition.ts 中应存在 workflowDialog.bindEvents()");
  assert.ok(runtimeIndex > 0, "composition.ts 中应存在 createRuntimeFeatures(");
  assert.ok(
    bindIndex < runtimeIndex,
    "workflowDialog.bindEvents() 必须先于 createRuntimeFeatures（含 mountRecentJobsFeature）",
  );
});
