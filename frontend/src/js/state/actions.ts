import {
  resetDeepSeekBalanceState as resetDeepSeekBalanceStateSlice,
  resetOcrValidationCache as resetOcrValidationCacheSlice,
  resetOcrValidationState as resetOcrValidationStateSlice,
  setDeepSeekBalanceState as setDeepSeekBalanceStateSlice,
  setOcrValidationCache as setOcrValidationCacheSlice,
} from "./credential-state.js";
import {
  resetDeveloperConfig as resetDeveloperConfigSlice,
  setDeveloperConfig as setDeveloperConfigSlice,
} from "./developer-state.js";
import {
  setDesktopConfigured as setDesktopConfiguredSlice,
  setDesktopMode as setDesktopModeSlice,
} from "./desktop-state.js";
import {
  setHomeRecentJobsLoadingState as setHomeRecentJobsLoadingStateSlice,
  setHomeViewMode as setHomeViewModeSlice,
} from "./home-state.js";
import {
  resetJobSecondaryState as resetJobSecondaryStateSlice,
  resetJobState as resetJobStateSlice,
} from "./job-state.js";
import { resetRecentJobsListState as resetRecentJobsListStateSlice } from "./recent-jobs-state.js";
import {
  clearAppliedPageRange as clearAppliedPageRangeSlice,
  setAppliedPageRange as setAppliedPageRangeSlice,
  resetUploadState as resetUploadStateSlice,
  setUploadState as setUploadStateSlice,
} from "./upload-state.js";
import type { InitialState } from "./slices.js";

// 全局 state 单例已下线(架构评审 P2-6):action 一律要求显式传入目标 state,
// 避免"看起来是全局操作、实际只有 desktop 模块在用"的误导。

export function resetJobState(target: InitialState) {
  resetJobStateSlice(target);
}

export function resetJobSecondaryState(target: InitialState) {
  resetJobSecondaryStateSlice(target);
}

export function resetUploadState(target: InitialState, options = {}) {
  resetUploadStateSlice(target, options);
}

export function setUploadState(target: InitialState, payload = {}) {
  setUploadStateSlice(target, payload);
}

export function setAppliedPageRange(target: InitialState, value = "") {
  setAppliedPageRangeSlice(target, value);
}

export function clearAppliedPageRange(target: InitialState) {
  clearAppliedPageRangeSlice(target);
}

export function resetRecentJobsListState(target: InitialState) {
  resetRecentJobsListStateSlice(target);
}

export function resetOcrValidationState(target: InitialState) {
  resetOcrValidationStateSlice(target);
}

export function resetOcrValidationCache(target: InitialState) {
  resetOcrValidationCacheSlice(target);
}

export function setOcrValidationCache(target: InitialState, payload = {}) {
  setOcrValidationCacheSlice(target, payload);
}

export function resetDeepSeekBalanceState(target: InitialState) {
  resetDeepSeekBalanceStateSlice(target);
}

export function setDeepSeekBalanceState(target: InitialState, balanceCny, checked = true) {
  setDeepSeekBalanceStateSlice(target, balanceCny, checked);
}

export function setDeveloperConfig(target: InitialState, config = {}) {
  setDeveloperConfigSlice(target, config);
}

export function resetDeveloperConfig(target: InitialState) {
  resetDeveloperConfigSlice(target);
}

export function setDesktopMode(target: InitialState, value = true) {
  setDesktopModeSlice(target, value);
}

export function setDesktopConfigured(target: InitialState, value = false) {
  setDesktopConfiguredSlice(target, value);
}

export function setHomeViewMode(target: InitialState, mode) {
  setHomeViewModeSlice(target, mode);
}

export function setHomeRecentJobsLoadingState(target: InitialState, loadingState, error = "") {
  setHomeRecentJobsLoadingStateSlice(target, loadingState, error);
}
