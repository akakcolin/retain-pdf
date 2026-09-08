// 子阶段流程条(蓝图 §2 features/status/;数据源 buildSubstageViewModel 是
// substage-view-model.js 纯 VM(自 js/job-status 迁入),原样 import——镜像
// job-status-card-substages.js 的 syncStageSubstageStates DOM 结构,
// --status-substage-count CSS 变量契约保留)。

import type { StatusCardSnapshot, StatusCardStageProgress } from "./status-card-store.js";
import type { CSSProperties } from "react";
import { buildSubstageViewModel } from "../../composition/external.js";

type SubstageFlowProps = {
  selectedStageKey?: string;
  selectedIsCurrent?: boolean;
  snapshot?: StatusCardSnapshot | null;
  selectedProgress?: StatusCardStageProgress | null;
};

export function SubstageFlow({ selectedStageKey, selectedIsCurrent, snapshot, selectedProgress }: SubstageFlowProps) {
  const viewModel = buildSubstageViewModel({ selectedStageKey, selectedIsCurrent, snapshot, selectedProgress });

  return (
    <div
      className={`status-substage-flow${viewModel.hidden ? " hidden" : ""}`}
      aria-label="任务子阶段"
      style={{ ["--status-substage-count"]: `${viewModel.cssCount}` } as CSSProperties}
    >
      {viewModel.items.map((item) => (
        <span
          key={item.key}
          className={`status-substage-step${item.active ? " is-active" : ""}${item.done ? " is-done" : ""}`}
          data-substage-key={item.key}
        >
          {item.label}
        </span>
      ))}
    </div>
  );
}
