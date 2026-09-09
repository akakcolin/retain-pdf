import test from "node:test";
import assert from "node:assert/strict";

// 力导向布局：纯函数、确定性（无随机）。同输入必须同输出，坐标有限且落在画布内。

const { layoutGraph, GRAPH_WIDTH, GRAPH_HEIGHT } = await import(
  "../src/pages/reader/entities/graph-layout.js"
);

const node = (id) => ({ entity_id: id });
const edge = (from, to) => ({ from_entity_id: from, to_entity_id: to });

test("layoutGraph 确定性：同输入两次调用坐标相同", () => {
  const nodes = [node("a"), node("b"), node("c"), node("d")];
  const edges = [edge("a", "b"), edge("b", "c"), edge("c", "d"), edge("d", "a")];
  const first = layoutGraph(nodes, edges, 340, 300);
  const second = layoutGraph(nodes, edges, 340, 300);
  for (const id of ["a", "b", "c", "d"]) {
    assert.deepEqual(first.get(id), second.get(id));
  }
});

test("layoutGraph 坐标有限且落在画布内", () => {
  const nodes = [node("root"), node("x"), node("y"), node("z")];
  const edges = [edge("root", "x"), edge("root", "y"), edge("root", "z")];
  const positions = layoutGraph(nodes, edges, 340, 300);
  assert.equal(positions.size, 4);
  for (const point of positions.values()) {
    assert.ok(Number.isFinite(point.x) && Number.isFinite(point.y), "坐标有限");
    assert.ok(point.x >= 0 && point.x <= 340, `x=${point.x}`);
    assert.ok(point.y >= 0 && point.y <= 300, `y=${point.y}`);
  }
});

test("layoutGraph 单节点居中", () => {
  const positions = layoutGraph([node("solo")], [], 340, 300);
  assert.deepEqual(positions.get("solo"), { x: 170, y: 150 });
});

test("layoutGraph 空输入返回空表", () => {
  assert.equal(layoutGraph([], [], 340, 300).size, 0);
});

test("layoutGraph 无边也不产生 NaN", () => {
  const positions = layoutGraph([node("a"), node("b"), node("c")], [], 340, 300);
  assert.equal(positions.size, 3);
  for (const point of positions.values()) {
    assert.ok(Number.isFinite(point.x) && Number.isFinite(point.y));
  }
});

test("layoutGraph 忽略端点缺失的边与自环", () => {
  const nodes = [node("a"), node("b")];
  const positions = layoutGraph(
    nodes,
    [edge("a", "ghost"), edge("a", "a")],
    340,
    300,
  );
  assert.equal(positions.size, 2);
  for (const point of positions.values()) {
    assert.ok(Number.isFinite(point.x) && Number.isFinite(point.y));
  }
});

test("layoutGraph 非法尺寸回落到默认画布", () => {
  const positions = layoutGraph([node("solo")], [], 0, Number.NaN);
  assert.deepEqual(positions.get("solo"), {
    x: GRAPH_WIDTH / 2,
    y: GRAPH_HEIGHT / 2,
  });
});
