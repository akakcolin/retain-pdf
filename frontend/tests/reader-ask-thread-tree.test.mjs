import test from "node:test";
import assert from "node:assert/strict";

// 让 js/reader 侧的 config/runtime 在 node 下可用
globalThis.window = globalThis.window || { location: { search: "", protocol: "http:", hostname: "127.0.0.1" } };

const {
  normalizeAiCitations,
  pathForBranch,
  shouldFallbackToLocal,
  snapshotFromTree,
  treeFromSnapshot,
  visibleMessages,
} = await import("../src/pages/reader/components/react-pdf/assistant/thread-tree.js");

function item(id, role, parentId, extra = {}) {
  return { parentId, message: { id, role, content: `${role}:${id}`, ...extra } };
}

test("visibleMessages 从 head 沿 parent 链回溯（含断链防护）", () => {
  const items = [
    item("u1", "user", null),
    item("a1", "assistant", "u1"),
    item("u2", "user", "a1"),
    item("a2", "assistant", "u2"),
  ];
  assert.deepEqual(visibleMessages(items, "a2").map((m) => m.id), ["u1", "a1", "u2", "a2"]);
  // head 无效时退化为最后一个 item
  assert.deepEqual(visibleMessages(items, "missing").map((m) => m.id), ["u1", "a1", "u2", "a2"]);
  // 环防护：parent 自环不死循环
  const cyclic = [{ parentId: "x", message: { id: "x", role: "user", content: "x" } }];
  assert.equal(visibleMessages(cyclic, "x").length, 1);
  assert.deepEqual(visibleMessages([], null), []);
});

test("pathForBranch 优先完整 parent 链", () => {
  const items = [
    item("u1", "user", null),
    item("a1", "assistant", "u1"),
    item("u2", "user", "a1"),
    item("a2", "assistant", "u2"),
  ];
  const path = pathForBranch(items, "a1", "a2");
  assert.deepEqual(path.map((i) => i.message.id), ["u1", "a1"]);
});

test("pathForBranch 在 parent 链断裂时退化为可见路径截断", () => {
  // a2 的 parentId 指向不存在的节点 → 走 fallback 线性截断
  const items = [
    item("u1", "user", null),
    item("a1", "assistant", "u1"),
    item("u2", "user", "missing-parent"),
    item("a2", "assistant", "u2"),
  ];
  const path = pathForBranch(items, "a2", "a2");
  assert.ok(path.length >= 2, "fork 路径必须含上文，不能只有孤零零一条 assistant");
  assert.equal(path[path.length - 1].message.id, "a2");
  assert.equal(path[path.length - 1].message.role, "assistant");
});

test("pathForBranch 目标 id 无效时回退到 head/最近 assistant", () => {
  const items = [
    item("u1", "user", null),
    item("a1", "assistant", "u1"),
  ];
  const path = pathForBranch(items, "aui-internal-id", "a1");
  assert.deepEqual(path.map((i) => i.message.id), ["u1", "a1"]);
  assert.deepEqual(pathForBranch([], "x", null), []);
  assert.deepEqual(pathForBranch(items, "", "a1"), []);
});

test("snapshotFromTree / treeFromSnapshot 往返保持引用与状态", () => {
  const items = [
    item("u1", "user", null),
    item("a1", "assistant", "u1", {
      progress: "完成",
      citations: [{ block_id: "p003-b0001", ref: 1, snippet: "片段" }],
      status: { type: "incomplete", reason: "cancelled" },
    }),
  ];
  const snapshot = snapshotFromTree(items, "a1");
  assert.equal(snapshot.version, 1);
  assert.equal(snapshot.headId, "a1");

  const restored = treeFromSnapshot(snapshot);
  assert.deepEqual(restored.items.map((i) => i.message.id), ["u1", "a1"]);
  assert.equal(restored.items[1].message.citations.length, 1);
  assert.equal(restored.items[1].message.status.reason, "cancelled");
  assert.equal(restored.headId, "a1");
});

test("normalizeAiCitations 过滤无 block_id 项并从 block_id 推断页码", () => {
  const citations = normalizeAiCitations([
    { block_id: "p007-b0003", ref: 2, job_id: " j1 ", snippet: " s " },
    { block_id: "", ref: 9 },
    "garbage",
    { block_id: "p012-b0001", page_idx: 4 },
  ]);
  assert.equal(citations.length, 2);
  assert.equal(citations[0].page_idx, 6, "p007 → 页码 6（0 基）");
  assert.equal(citations[0].job_id, "j1");
  assert.equal(citations[1].page_idx, 4, "显式 page_idx 优先于 block_id 推断");
  assert.deepEqual(normalizeAiCitations(null), []);
});

test("shouldFallbackToLocal 只对 502 降级", () => {
  assert.equal(shouldFallbackToLocal({ status: 502 }), true);
  assert.equal(shouldFallbackToLocal(new Error("upstream 502 Bad Gateway")), true);
  assert.equal(shouldFallbackToLocal({ status: 500 }), false);
  assert.equal(shouldFallbackToLocal(new Error("network error")), false);
});
