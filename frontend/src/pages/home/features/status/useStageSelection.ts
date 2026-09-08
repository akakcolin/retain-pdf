// 阶段选择语义 hook(蓝图 §2 features/status/)。
//
// 语义拷贝自 components/status/job-status-card-selection.js 的
// createStatusCardSelectionState(该文件属"死,由 StatusCard.jsx 家族替代"
// 清单,js/components/ 禁止 import——这里重写为 useState 驱动,resolve 逻辑
// 本身直接调 stage-flow-model.js(自 js/job-status 迁入)的 resolveSelectedStatusStage,
// 纯函数原样复用,不拷贝):
// - 换 job(jobId 变化):selectedStageKey/manualStageSelection 复位;
// - currentStageKey 推进(轮询命中新阶段):manualStageSelection 复位,除非
//   用户又手动点了一次(selectStage 会重新置 true 并立即用新的
//   currentStageKey 校验是否仍可选,不可选则回退跟随当前阶段——
//   isSelectableStatusStage 语义:只能选"已到达或正在进行"的阶段)。

import { useCallback, useEffect, useState } from "react";
import { resolveSelectedStatusStage } from "../../composition/external.js";

type StageSelectionState = {
  currentJobId: string;
  currentStageKey: string;
  selectedStageKey: string;
  manualStageSelection: boolean;
};

const INITIAL_STATE: StageSelectionState = {
  currentJobId: "",
  currentStageKey: "",
  selectedStageKey: "",
  manualStageSelection: false,
};

export function useStageSelection({ jobId = "", currentStageKey = "" } = {}) {
  const [state, setState] = useState<StageSelectionState>(INITIAL_STATE);

  useEffect(() => {
    setState((prev) => {
      const normalizedJobId = `${jobId || ""}`.trim();
      const normalizedStageKey = `${currentStageKey || ""}`.trim();
      const jobChanged = Boolean(normalizedJobId && normalizedJobId !== prev.currentJobId);
      const base = jobChanged
        ? { ...prev, currentJobId: normalizedJobId, selectedStageKey: "", manualStageSelection: false }
        : prev;
      const previousStageKey = base.currentStageKey;
      const stageAdvanced = Boolean(previousStageKey && previousStageKey !== normalizedStageKey);
      const manualStageSelection = stageAdvanced ? false : base.manualStageSelection;
      const resolved = resolveSelectedStatusStage({
        currentStageKey: normalizedStageKey,
        selectedStageKey: base.selectedStageKey,
        manualStageSelection,
      });
      return {
        currentJobId: base.currentJobId,
        currentStageKey: normalizedStageKey,
        selectedStageKey: resolved.selectedStageKey,
        manualStageSelection: resolved.manualStageSelection,
      };
    });
  }, [jobId, currentStageKey]);

  const selectStage = useCallback((stageKey) => {
    setState((prev) => {
      const resolved = resolveSelectedStatusStage({
        currentStageKey: prev.currentStageKey,
        selectedStageKey: stageKey,
        manualStageSelection: true,
      });
      return {
        ...prev,
        selectedStageKey: resolved.selectedStageKey,
        manualStageSelection: resolved.manualStageSelection,
      };
    });
  }, []);

  const selectedIsCurrent = !state.selectedStageKey || state.selectedStageKey === state.currentStageKey;

  return {
    currentStageKey: state.currentStageKey,
    selectedStageKey: state.selectedStageKey,
    manualStageSelection: state.manualStageSelection,
    selectedIsCurrent,
    selectStage,
  };
}
