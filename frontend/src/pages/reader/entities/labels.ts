// 概念图谱展示文案：词表与后端 models/graph.rs 的 ENTITY_TYPES / RELATION_TYPES 对齐。
// 未知值原样回显，不吞信息（后端也会把未知关系原词留在 explanation）。

const ENTITY_TYPE_LABELS: Record<string, string> = {
  concept: "概念",
  method: "方法",
  material: "材料",
  dataset: "数据集",
  person: "人物",
  org: "机构",
  metric: "指标",
  formula: "公式",
  term: "术语",
};

const RELATION_TYPE_LABELS: Record<string, string> = {
  uses: "使用",
  improves_on: "改进",
  contradicts: "矛盾",
  part_of: "属于",
  related_to: "相关",
  defines: "定义",
  evaluates: "评估",
  produces: "产出",
};

export function entityTypeLabel(entityType: string): string {
  const key = `${entityType || ""}`.trim();
  return ENTITY_TYPE_LABELS[key] || key || "概念";
}

export function relationTypeLabel(relationType: string): string {
  const key = `${relationType || ""}`.trim();
  return RELATION_TYPE_LABELS[key] || key || "相关";
}

/** out = 被查询实体 → 邻居；in = 邻居 → 被查询实体。 */
export function directionArrow(direction: string): string {
  return `${direction || ""}`.trim() === "in" ? "←" : "→";
}

/** page_idx 0 基 → 阅读器 1 基页码文案。 */
export function mentionPageLabel(pageIdx: number): string {
  const page = Number.isFinite(Number(pageIdx)) ? Math.max(0, Math.floor(Number(pageIdx))) : 0;
  return `第 ${page + 1} 页`;
}
