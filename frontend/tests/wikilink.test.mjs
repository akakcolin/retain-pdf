import test from "node:test";
import assert from "node:assert/strict";
import { JSDOM } from "jsdom";
import { injectWikiLinks } from "../src/pages/reader/entities/wikilink.ts";

// 概念页正文 [[实体名]] → 按钮（命中）或纯文本（未命中）。纯 DOM，无网络。

const LINK = {
  surface: "图神经网络",
  entity_id: "ent-2",
  name: "GNN",
  entity_type: "method",
  aliases: ["图神经网络"],
};

function render(html) {
  const dom = new JSDOM(`<!doctype html><div id="r">${html}</div>`);
  return { dom, root: dom.window.document.getElementById("r") };
}

test("命中的 [[X]] 变成按钮，点击回调带解析结果", () => {
  const { dom, root } = render("<p>参见 [[图神经网络]] 的综述。</p>");
  const opened = [];
  injectWikiLinks(
    root,
    new Map([[LINK.surface, LINK]]),
    (link) => opened.push(link),
    dom.window.document,
  );
  const button = root.querySelector("button.reader-entities-wikilink");
  assert.ok(button, "渲染出 wikilink 按钮");
  assert.equal(button.textContent, "图神经网络", "按钮文案用正文原样文本");
  assert.ok(!root.textContent.includes("[["));
  button.click();
  assert.equal(opened.length, 1);
  assert.equal(opened[0].entity_id, "ent-2");
});

test("未命中的 [[X]] 还原成纯文本，不留括号", () => {
  const { dom, root } = render("<p>与 [[不存在]] 无关。</p>");
  injectWikiLinks(root, new Map([[LINK.surface, LINK]]), null, dom.window.document);
  assert.equal(root.querySelectorAll("button.reader-entities-wikilink").length, 0);
  assert.equal(root.textContent, "与 不存在 无关。");
});

test("一个都没解析到时（空 map）也要去掉括号", () => {
  const { dom, root } = render("<p>与 [[不存在]] 无关。</p>");
  injectWikiLinks(root, new Map(), null, dom.window.document);
  assert.equal(root.querySelectorAll("button.reader-entities-wikilink").length, 0);
  assert.equal(root.textContent, "与 不存在 无关。");
});

test("code/pre 内不处理", () => {
  const { dom, root } = render("<pre>[[图神经网络]]</pre><p>[[图神经网络]]</p>");
  injectWikiLinks(root, new Map([[LINK.surface, LINK]]), null, dom.window.document);
  assert.equal(root.querySelectorAll("button.reader-entities-wikilink").length, 1);
  assert.ok(root.querySelector("pre").textContent.includes("[[图神经网络]]"));
});

test("未闭合的括号不破坏原文", () => {
  const { dom, root } = render("<p>[[图神经网络</p>");
  injectWikiLinks(root, new Map([[LINK.surface, LINK]]), null, dom.window.document);
  assert.equal(root.textContent, "[[图神经网络");
});
