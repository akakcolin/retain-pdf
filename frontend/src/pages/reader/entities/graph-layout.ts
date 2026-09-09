// 手写力导向布局：确定性（无随机）、固定迭代，同输入同输出，可测。
// 只做布局计算，不碰 DOM、不依赖图形库。

export type LayoutNode = { entity_id: string };
export type LayoutEdge = { from_entity_id: string; to_entity_id: string };
export type Point = { x: number; y: number };

/** 布局画布尺寸（与 SVG viewBox 一致，避免测量 DOM）。 */
export const GRAPH_WIDTH = 340;
export const GRAPH_HEIGHT = 300;

const PADDING = 24;
const ITERATIONS = 300;
const REPULSION = 6000;
const SPRING = 0.06;
const SPRING_LENGTH = 70;
const CENTER_PULL = 0.02;
const MAX_STEP = 12;

/** 从根节点（nodes[0]）起布局。返回每个 entity_id 的坐标，值保证有限。 */
export function layoutGraph(
  nodes: LayoutNode[],
  edges: LayoutEdge[],
  width: number,
  height: number,
): Map<string, Point> {
  const positions = new Map<string, Point>();
  const w = Number.isFinite(width) && width > 0 ? width : GRAPH_WIDTH;
  const h = Number.isFinite(height) && height > 0 ? height : GRAPH_HEIGHT;
  const count = nodes.length;
  if (count === 0) return positions;

  const cx = w / 2;
  const cy = h / 2;
  if (count === 1) {
    positions.set(nodes[0].entity_id, { x: cx, y: cy });
    return positions;
  }

  const xs = new Float64Array(count);
  const ys = new Float64Array(count);
  const indexById = new Map<string, number>();
  const radius = Math.max(PADDING, Math.min(w, h) / 2 - PADDING);
  nodes.forEach((node, index) => {
    indexById.set(node.entity_id, index);
    if (index === 0) {
      // 根固定中心，初值即终值（不受力）。
      xs[index] = cx;
      ys[index] = cy;
      return;
    }
    const angle = ((index - 1) / (count - 1)) * Math.PI * 2;
    xs[index] = cx + Math.cos(angle) * radius;
    ys[index] = cy + Math.sin(angle) * radius;
  });

  // 解析边到索引对；端点不在节点集里或自环直接丢。
  const links: Array<[number, number]> = [];
  for (const edge of edges) {
    const from = indexById.get(edge.from_entity_id);
    const to = indexById.get(edge.to_entity_id);
    if (from !== undefined && to !== undefined && from !== to) {
      links.push([from, to]);
    }
  }

  for (let step = 0; step < ITERATIONS; step += 1) {
    const fx = new Float64Array(count);
    const fy = new Float64Array(count);

    // 节点两两斥力。
    for (let i = 0; i < count; i += 1) {
      for (let j = i + 1; j < count; j += 1) {
        let dx = xs[i] - xs[j];
        let dy = ys[i] - ys[j];
        let d2 = dx * dx + dy * dy;
        if (d2 < 1e-6) {
          // 重合时给一个确定性的分离方向。
          dx = i - j || 1;
          dy = 1;
          d2 = dx * dx + dy * dy;
        }
        const d = Math.sqrt(d2);
        const force = REPULSION / d2;
        const ux = dx / d;
        const uy = dy / d;
        fx[i] += ux * force;
        fy[i] += uy * force;
        fx[j] -= ux * force;
        fy[j] -= uy * force;
      }
    }

    // 边弹簧引力。
    for (const [a, b] of links) {
      const dx = xs[b] - xs[a];
      const dy = ys[b] - ys[a];
      const d = Math.sqrt(dx * dx + dy * dy) || 1e-6;
      const force = (d - SPRING_LENGTH) * SPRING;
      const ux = dx / d;
      const uy = dy / d;
      fx[a] += ux * force;
      fy[a] += uy * force;
      fx[b] -= ux * force;
      fy[b] -= uy * force;
    }

    // 向心弱力 + 积分；根 pinned 不动。
    for (let i = 0; i < count; i += 1) {
      if (i === 0) continue;
      fx[i] += (cx - xs[i]) * CENTER_PULL;
      fy[i] += (cy - ys[i]) * CENTER_PULL;
      xs[i] += clampStep(fx[i]);
      ys[i] += clampStep(fy[i]);
    }
  }

  // 收尾按包围盒等比缩放并居中到画布内（留 PADDING）。
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (let i = 0; i < count; i += 1) {
    minX = Math.min(minX, xs[i]);
    minY = Math.min(minY, ys[i]);
    maxX = Math.max(maxX, xs[i]);
    maxY = Math.max(maxY, ys[i]);
  }
  const spanX = maxX - minX || 1e-6;
  const spanY = maxY - minY || 1e-6;
  const availW = Math.max(1, w - PADDING * 2);
  const availH = Math.max(1, h - PADDING * 2);
  const scale = Math.min(availW / spanX, availH / spanY);
  const offsetX = PADDING + (availW - spanX * scale) / 2;
  const offsetY = PADDING + (availH - spanY * scale) / 2;

  nodes.forEach((node, index) => {
    const x = offsetX + (xs[index] - minX) * scale;
    const y = offsetY + (ys[index] - minY) * scale;
    positions.set(node.entity_id, {
      x: Number.isFinite(x) ? x : cx,
      y: Number.isFinite(y) ? y : cy,
    });
  });
  return positions;
}

function clampStep(value: number): number {
  if (!Number.isFinite(value)) return 0;
  return Math.max(-MAX_STEP, Math.min(MAX_STEP, value));
}
