import test from "node:test";
import assert from "node:assert/strict";
import {
  READER_COLUMN_LIMITS,
  clampColumnWidth,
  loadColumnWidths,
  saveColumnWidths,
  createReaderColumnResizer,
} from "../src/pages/reader/legacy/controllers/column-resizer.js";

function memoryStorage(seed) {
  const map = new Map(seed ? Object.entries(seed) : []);
  return {
    getItem: (k) => (map.has(k) ? map.get(k) : null),
    setItem: (k, v) => map.set(k, `${v}`),
    removeItem: (k) => map.delete(k),
    _map: map,
  };
}

test("clampColumnWidth 夹取到区间并取整,非法回退默认", () => {
  assert.equal(clampColumnWidth(10, READER_COLUMN_LIMITS.left), READER_COLUMN_LIMITS.left.min);
  assert.equal(clampColumnWidth(9999, READER_COLUMN_LIMITS.right), READER_COLUMN_LIMITS.right.max);
  assert.equal(clampColumnWidth(300.6, READER_COLUMN_LIMITS.left), 301);
  assert.equal(clampColumnWidth("abc", READER_COLUMN_LIMITS.left), READER_COLUMN_LIMITS.left.default);
});

test("宽度持久化:save 后 load 回读,且读取时再夹取", () => {
  const storage = memoryStorage();
  saveColumnWidths({ left: 320, right: 500 }, storage);
  assert.deepEqual(loadColumnWidths(storage), { left: 320, right: 500 });

  // 越界的持久值读取时被夹回区间
  storage.setItem("retainpdf-reader-cols-v1", JSON.stringify({ left: 5, right: 9000 }));
  const loaded = loadColumnWidths(storage);
  assert.equal(loaded.left, READER_COLUMN_LIMITS.left.min);
  assert.equal(loaded.right, READER_COLUMN_LIMITS.right.max);
});

test("无 storage / 脏数据:回退默认宽度不抛", () => {
  assert.deepEqual(loadColumnWidths(null), {
    left: READER_COLUMN_LIMITS.left.default,
    right: READER_COLUMN_LIMITS.right.default,
  });
  const bad = memoryStorage({ "retainpdf-reader-cols-v1": "{not json" });
  assert.deepEqual(loadColumnWidths(bad), {
    left: READER_COLUMN_LIMITS.left.default,
    right: READER_COLUMN_LIMITS.right.default,
  });
  assert.doesNotThrow(() => saveColumnWidths({ left: 1, right: 1 }, null));
});

test("applyLeft/applyRight 写入 body CSS 变量并夹取", () => {
  const setCalls = [];
  const body = { style: { setProperty: (k, v) => setCalls.push([k, v]) }, classList: { add() {}, remove() {} } };
  const documentRef = {
    body,
    getElementById: () => null,
    defaultView: { innerWidth: 1440 },
    addEventListener() {},
    removeEventListener() {},
  };
  const resizer = createReaderColumnResizer({ documentRef, storage: memoryStorage() });
  resizer.applyLeft(320);
  resizer.applyRight(9999);
  assert.deepEqual(resizer.widths(), { left: 320, right: READER_COLUMN_LIMITS.right.max });
  assert.ok(setCalls.some(([k, v]) => k === "--reader-left-col" && v === "320px"));
  assert.ok(setCalls.some(([k, v]) => k === "--reader-right-col" && v === `${READER_COLUMN_LIMITS.right.max}px`));
});

test("bindEvents 应用持久宽度,并给两个把手绑定 pointerdown", () => {
  const handles = {
    "reader-col-resizer-left": { listeners: {}, addEventListener(t, fn) { this.listeners[t] = fn; } },
    "reader-col-resizer-right": { listeners: {}, addEventListener(t, fn) { this.listeners[t] = fn; } },
  };
  const setCalls = [];
  const documentRef = {
    body: { style: { setProperty: (k, v) => setCalls.push([k, v]) }, classList: { add() {}, remove() {} } },
    getElementById: (id) => handles[id] || null,
    defaultView: { innerWidth: 1440 },
    addEventListener() {},
    removeEventListener() {},
  };
  const storage = memoryStorage({ "retainpdf-reader-cols-v1": JSON.stringify({ left: 300, right: 420 }) });
  createReaderColumnResizer({ documentRef, storage }).bindEvents();
  assert.ok(setCalls.some(([k, v]) => k === "--reader-left-col" && v === "300px"));
  assert.ok(setCalls.some(([k, v]) => k === "--reader-right-col" && v === "420px"));
  assert.equal(typeof handles["reader-col-resizer-left"].listeners.pointerdown, "function");
  assert.equal(typeof handles["reader-col-resizer-right"].listeners.pointerdown, "function");
});
