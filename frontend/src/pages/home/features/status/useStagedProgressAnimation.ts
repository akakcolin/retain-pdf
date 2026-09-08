// staged 进度动画 hook(蓝图 §2 features/status/,风险 §8.1——全项目最容易
// 翻车的时序点)。
//
// 拷贝自 components/status/job-status-card-progress-animation.js 的
// createStatusCardProgressAnimation(该文件属"死,由 StatusCard.jsx 家族
// 替代"清单,js/components/ 禁止 import;buildProgressOptions/
// shouldAnimateRenderPageProgress 是纯 VM(自 js/job-status 迁入),原样 import)。
//
// 铁律(风险 §8.1):displayedProgressByStage 与 timer 必须是 useRef,不是
// useState——每 120ms 跳一页的动画如果改用 useState,会导致每 tick 触发一次
// 整组件重渲染,且闭包捕获的是旧的 state 值(setState 的函数式更新虽能绕开
// 闭包旧值问题,但仍无法避免每 tick 重渲——ref 是唯一同时满足"跨 tick 持久化
// 又不触发渲染"的方案)。真正需要触发渲染的只有 renderOptions(通过独立的
// useState 输出,交给 ProgressBlock.jsx 渲染)。

import { useEffect, useRef, useState } from "react";
import {
  buildProgressOptions,
  shouldAnimateRenderPageProgress,
} from "../../composition/external.js";

const TICK_DELAY_MS = 120;

export function useStagedProgressAnimation({ selected, selectedIsCurrent, snapshot, selectedProgress, jobId }) {
  const displayedProgressByStageRef = useRef({});
  const timerRef = useRef(null);
  const [renderOptions, setRenderOptions] = useState(null);

  function clear() {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }

  function rememberProgress(stageKey, current, total) {
    displayedProgressByStageRef.current[stageKey] = {
      current: Number.isFinite(current) ? current : null,
      total: Number.isFinite(total) ? total : null,
    };
  }

  // 换 job 复位(风险 §8.1 附带语义):displayedProgressByStage 是"跨阶段的
  // 已显示进度记忆",job 切换后旧任务的记忆必须清空,否则新任务同名阶段会
  // 复用旧任务的显示进度做动画起点。
  useEffect(() => {
    clear();
    displayedProgressByStageRef.current = {};
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [jobId]);

  useEffect(() => {
    const previous = displayedProgressByStageRef.current[selected];
    const {
      previousCurrent,
      shouldAnimate,
      targetCurrent,
      targetTotal,
    } = shouldAnimateRenderPageProgress({ selected, selectedIsCurrent, snapshot, selectedProgress, previous });

    if (!shouldAnimate) {
      clear();
      rememberProgress(selected, targetCurrent, targetTotal);
      setRenderOptions(buildProgressOptions({ selected, selectedIsCurrent, snapshot, selectedProgress }));
      return undefined;
    }

    clear();
    let displayedCurrent = previousCurrent;
    const tick = () => {
      displayedCurrent = Math.min(targetCurrent, displayedCurrent + 1);
      rememberProgress(selected, displayedCurrent, targetTotal);
      setRenderOptions(buildProgressOptions({
        selected, selectedIsCurrent, snapshot, selectedProgress, displayedCurrent,
      }));
      if (displayedCurrent < targetCurrent) {
        timerRef.current = setTimeout(tick, TICK_DELAY_MS);
      }
    };
    tick();
    return clear;
    // selected/selectedIsCurrent/snapshot/selectedProgress 均来自 props 派生值,
    // 每次上游快照变化时这些引用天然变化,依赖表随其变化重跑动画判定——
    // 与旧世界 render({selected,...}) 每次快照回调都被调用一次的时序等价。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selected, selectedIsCurrent, snapshot, selectedProgress]);

  useEffect(() => clear, []);

  return renderOptions;
}
