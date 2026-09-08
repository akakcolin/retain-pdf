import test from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";

// detail.html / reader.html 现由 esbuild 打包的 dist/{detail,reader}.bundle.js 挂载 React
// 树(Phase 1 / 2b cutover),但 src/pages/detail/legacy、src/js/reader(含
// pages/reader/legacy)下保留的纯逻辑仍以
// 字符串字面量引用 DOM id/class,esbuild 不做这类校验:id 改名、typo、删掉 CSS 类都只会
// 在运行时静默失效(dom/query.js 的守卫会吞掉 null)。本测试交叉校验:job-detail / reader
// 目录下 JS 出现的每个 "detail-*" / "reader-*" 字符串字面量,必须能在对应页面 HTML 的
// id/class、src/styles 的类定义、src/pages/{detail,reader} 的 JSX(id=.../className=...),
// 或 JS 自建元素(id="...")中找到归属。
//
// home 页(index.html / src/pages/home)未纳入本文件:home 没有单一 id 前缀约定(各 feature
// 域各自命名),用 tests/home-app-component.test.mjs(渲染 HomeApp 断言契约 id)+ 各域
// *-component.test.mjs(如 recent-jobs-library-component / status-card-component 等,
// 渲染实际 React 树断言 DOM 契约)覆盖,是比这里的字符串字面量扫描更强的检查——直接渲染
// 组件断言真实 DOM,而不是扫描源码里的字符串猜测归属。Phase 4 复核确认此判断仍然成立,
// 不需要把 home 补进本文件。

const PROJECT_ROOT = process.cwd();
const STYLES_ROOT = join(PROJECT_ROOT, "src/styles");

// 已确认的历史遗留引用(运行时元素/类确实不存在)。新增条目前必须先人工确认,
// 并注明原因;一旦引用恢复归属,下方的 hygiene 用例会强制从这里移除。
const KNOWN_ORPHANS = {
  "src/pages/detail/legacy": Object.freeze([
    // 模板生成的类,src/styles 中没有对应规则(无样式 div)
    "detail-artifact-meta",
  ]),
  "src/js/reader": Object.freeze([]),
  "src/pages/reader/legacy": Object.freeze([]),
};

const PAGES = [
  {
    jsDir: "src/pages/detail/legacy",
    prefix: "detail",
    htmlFile: "detail.html",
    // Phase 1 cutover 后 detail.html 只剩 #detail-root 挂载点,页面骨架
    // (id/class)改由 React 树渲染:归属校验需要扫描新世界 JSX 的
    // id="..." 与 className="..."(保留的旧纯逻辑仍按 id 写这些节点)。
    jsxDir: "src/pages/detail",
  },
  {
    jsDir: "src/js/reader",
    prefix: "reader",
    htmlFile: "reader.html",
    // Phase 2b cutover 后 reader.html 只剩 #reader-root 挂载点,页面骨架
    // (id/class)改由 React 树渲染(照 detail 先例扫描新世界 JSX)。
    jsxDir: "src/pages/reader",
  },
  {
    // 顶层 reader 纯逻辑已随 ADR 0009 迁入 pages/reader/legacy;子目录
    // (favorites/annotations/ai/downloads)仍留 js/reader,故两者都要扫。
    jsDir: "src/pages/reader/legacy",
    prefix: "reader",
    htmlFile: "reader.html",
    jsxDir: "src/pages/reader",
  },
];

function walkFiles(dir, extension) {
  const results = [];
  for (const entry of readdirSync(dir)) {
    const fullPath = join(dir, entry);
    if (statSync(fullPath).isDirectory()) {
      results.push(...walkFiles(fullPath, extension));
    } else if (entry.endsWith(extension)) {
      results.push(fullPath);
    }
  }
  return results;
}

function collectLiterals(jsFiles, prefix) {
  const pattern = new RegExp(`["'](${prefix}-[a-z0-9-]+)["']`, "g");
  const literals = new Map();
  for (const file of jsFiles) {
    const text = readFileSync(file, "utf8");
    for (const match of text.matchAll(pattern)) {
      const literal = match[1];
      if (!literals.has(literal)) {
        literals.set(literal, relative(PROJECT_ROOT, file));
      }
    }
  }
  return literals;
}

function collectOwnership(htmlText, jsTexts, cssText, jsxTexts = []) {
  const ids = new Set(
    [...htmlText.matchAll(/id="([a-z0-9-]+)"/g)].map((m) => m[1]),
  );
  for (const text of [...jsTexts, ...jsxTexts]) {
    for (const match of text.matchAll(/\bid\s*=\s*"([a-z0-9-]+)"/g)) {
      ids.add(match[1]);
    }
  }
  const classes = new Set();
  for (const match of htmlText.matchAll(/class="([^"]+)"/g)) {
    for (const name of match[1].split(/\s+/)) {
      classes.add(name);
    }
  }
  // React 页面骨架:JSX 的 className 等价于旧 HTML 的 class 归属
  for (const text of jsxTexts) {
    for (const match of text.matchAll(/className="([^"]+)"/g)) {
      for (const name of match[1].split(/\s+/)) {
        classes.add(name);
      }
    }
  }
  for (const match of cssText.matchAll(/\.([a-z0-9][a-z0-9-]*)/g)) {
    classes.add(match[1]);
  }
  return { ids, classes };
}

function isOwned(literal, ownership) {
  if (ownership.ids.has(literal) || ownership.classes.has(literal)) {
    return true;
  }
  // 复合 id 模式,如 showReaderPaneEmpty 用 "reader-pdf" 拼出 "reader-pdf-wrap"
  const family = `${literal}-`;
  for (const id of ownership.ids) {
    if (id.startsWith(family)) {
      return true;
    }
  }
  return false;
}

function analyzePage({ jsDir, prefix, htmlFile, jsxDir = "" }) {
  // TS 迁移后源文件是 .ts/.tsx；仍兼容残留 .js/.jsx
  const jsFiles = [
    ...walkFiles(join(PROJECT_ROOT, jsDir), ".ts"),
    ...walkFiles(join(PROJECT_ROOT, jsDir), ".js"),
  ];
  const jsTexts = jsFiles.map((file) => readFileSync(file, "utf8"));
  const jsxTexts = jsxDir
    ? [
      ...walkFiles(join(PROJECT_ROOT, jsxDir), ".tsx"),
      ...walkFiles(join(PROJECT_ROOT, jsxDir), ".jsx"),
      ...walkFiles(join(PROJECT_ROOT, jsxDir), ".ts"),
      ...walkFiles(join(PROJECT_ROOT, jsxDir), ".js"),
    ].map((file) => readFileSync(file, "utf8"))
    : [];
  const htmlText = readFileSync(join(PROJECT_ROOT, htmlFile), "utf8");
  const cssText = walkFiles(STYLES_ROOT, ".css")
    .map((file) => readFileSync(file, "utf8"))
    .join("\n");
  const literals = collectLiterals(jsFiles, prefix);
  const ownership = collectOwnership(htmlText, jsTexts, cssText, jsxTexts);
  return { literals, ownership };
}

for (const page of PAGES) {
  const allowlist = new Set(KNOWN_ORPHANS[page.jsDir]);

  test(`${page.jsDir} 的 ${page.prefix}-* 引用在 ${page.htmlFile}/样式/模板中有归属`, () => {
    const { literals, ownership } = analyzePage(page);
    assert.ok(literals.size > 0, `未在 ${page.jsDir} 中找到任何 ${page.prefix}-* 字面量,检查扫描逻辑`);
    const orphans = [];
    for (const [literal, file] of literals) {
      if (!isOwned(literal, ownership) && !allowlist.has(literal)) {
        orphans.push(`${literal} (首见于 ${file})`);
      }
    }
    assert.deepEqual(
      orphans,
      [],
      `以下引用在 ${page.htmlFile} 的 id/class、src/styles 类定义、JS 自建元素中均不存在,` +
        `会在运行时静默失效:\n  ${orphans.join("\n  ")}`,
    );
  });

  test(`${page.jsDir} 的 KNOWN_ORPHANS 清单没有过期条目`, () => {
    const { literals, ownership } = analyzePage(page);
    const stale = [];
    for (const literal of allowlist) {
      if (!literals.has(literal) || isOwned(literal, ownership)) {
        stale.push(literal);
      }
    }
    assert.deepEqual(
      stale,
      [],
      `以下 KNOWN_ORPHANS 条目已不再是孤儿(引用被删除或已恢复归属),请从清单移除:${stale.join(", ")}`,
    );
  });
}
