import { createStore, type Store } from "../../app-framework/store.js";

export const JOB_POLL_INTERVAL_MS = 1000;

const RUNTIME_POLLING_STORE_KEY = Symbol.for("retainpdf.runtimePollingStore");

/** Normalized runtime-polling sub-store snapshot. */
export interface RuntimePollingState {
  jobId: string;
  generation: number;
  pollInFlight: boolean;
}

/** Host-state or partial fields accepted when seeding the store. */
export type RuntimePollingInitialState = Partial<RuntimePollingState> & {
  currentJobId?: string;
  currentJobPollGeneration?: number;
  currentJobPollInFlight?: boolean;
  currentJobStartedAt?: string;
  /** DOM / Node timer handle (number or Timeout depending on lib). */
  timer?: unknown;
  [key: string]: unknown;
};

export type IntervalClearFn = (handle?: unknown) => void;
export type IntervalSetFn = (
  handler: (...args: unknown[]) => void,
  timeout?: number,
  ...args: unknown[]
) => unknown;

export interface RuntimePollingStatePortOptions {
  clearIntervalFn?: IntervalClearFn;
  setIntervalFn?: IntervalSetFn;
  now?: () => string;
}

export type RuntimePollingActions = {
  stop(currentState: RuntimePollingState): RuntimePollingState;
  /** 停止 + 作废旧 generation：导航离开/卸载时使在途 poll 的渲染失效。 */
  invalidate(currentState: RuntimePollingState): RuntimePollingState;
  beginPoll(currentState: RuntimePollingState): RuntimePollingState;
  finishPoll(currentState: RuntimePollingState): RuntimePollingState;
  startJob(currentState: RuntimePollingState, jobId: unknown): RuntimePollingState;
};

export type RuntimePollingStore = Store<RuntimePollingState, RuntimePollingActions>;

export interface RuntimePollingStartResult {
  generation: number;
  startedAt: string;
}

export interface RuntimePollingStatePort {
  store: RuntimePollingStore;
  getSnapshot: () => RuntimePollingState;
  stop: () => RuntimePollingState;
  /** stop + generation 前移：使 stop 之前发出的在途 poll 全部失效（导航离开用）。 */
  invalidate: () => RuntimePollingState;
  beginPoll: () => number | null;
  finishPoll: () => RuntimePollingState;
  isCurrentGeneration: (jobId: unknown, generation: unknown) => boolean;
  startJob: (jobId: unknown) => RuntimePollingStartResult;
  startTimer: (
    callback: (...args: unknown[]) => void,
    intervalMs?: number,
  ) => unknown;
}

function normalizePollingState(
  initialState: RuntimePollingInitialState = {},
): RuntimePollingState {
  return {
    jobId: `${initialState.jobId ?? initialState.currentJobId ?? ""}`.trim(),
    generation: Number(initialState.generation ?? initialState.currentJobPollGeneration ?? 0),
    pollInFlight: Boolean(initialState.pollInFlight ?? initialState.currentJobPollInFlight),
  };
}

export function createRuntimePollingStore(
  initialState: RuntimePollingInitialState = {},
): RuntimePollingStore {
  return createStore<RuntimePollingState, RuntimePollingActions>({
    name: "runtimePolling",
    initialState: normalizePollingState(initialState),
    actions: {
      stop(currentState) {
        return {
          ...currentState,
          pollInFlight: false,
        };
      },
      invalidate(currentState) {
        return {
          ...currentState,
          generation: Number(currentState.generation || 0) + 1,
          pollInFlight: false,
        };
      },
      beginPoll(currentState) {
        if (currentState.pollInFlight) {
          return currentState;
        }
        return {
          ...currentState,
          pollInFlight: true,
        };
      },
      finishPoll(currentState) {
        return {
          ...currentState,
          pollInFlight: false,
        };
      },
      startJob(currentState, jobId) {
        return {
          ...currentState,
          jobId: `${jobId || ""}`.trim(),
          generation: Number(currentState.generation || 0) + 1,
          pollInFlight: false,
        };
      },
    },
  });
}

function asPollingHost(state: object): RuntimePollingInitialState {
  return state as RuntimePollingInitialState;
}

function pollingStoreSlot(state: object): RuntimePollingStore | undefined {
  return (state as Record<PropertyKey, unknown>)[RUNTIME_POLLING_STORE_KEY] as
    | RuntimePollingStore
    | undefined;
}

export function runtimePollingStoreFor(
  state: object | null | undefined,
): RuntimePollingStore {
  if (!state || typeof state !== "object") {
    return createRuntimePollingStore();
  }
  const existing = pollingStoreSlot(state);
  if (!existing) {
    Object.defineProperty(state, RUNTIME_POLLING_STORE_KEY, {
      configurable: false,
      enumerable: false,
      value: createRuntimePollingStore(asPollingHost(state)),
      writable: false,
    });
  }
  return pollingStoreSlot(state) as RuntimePollingStore;
}

function applyRuntimePollingAction(
  state: object,
  action: (store: RuntimePollingStore) => RuntimePollingState,
): RuntimePollingState {
  const store = runtimePollingStoreFor(state);
  return action(store);
}

export function createRuntimePollingStatePort(
  state: object,
  {
    clearIntervalFn = clearInterval as IntervalClearFn,
    setIntervalFn = setInterval as IntervalSetFn,
    now = () => new Date().toISOString(),
  }: RuntimePollingStatePortOptions = {},
): RuntimePollingStatePort {
  const host = asPollingHost(state);
  const store = runtimePollingStoreFor(state);
  return {
    store,
    getSnapshot: () => store.getSnapshot(),
    stop() {
      if (host?.timer) {
        clearIntervalFn(host.timer);
        host.timer = null;
      }
      return applyRuntimePollingAction(state, (currentStore) => currentStore.actions.stop());
    },
    invalidate() {
      if (host?.timer) {
        clearIntervalFn(host.timer);
        host.timer = null;
      }
      return applyRuntimePollingAction(state, (currentStore) => currentStore.actions.invalidate());
    },
    beginPoll() {
      const current = store.getSnapshot();
      if (current.pollInFlight) {
        return null;
      }
      const snapshot = applyRuntimePollingAction(state, (currentStore) => currentStore.actions.beginPoll());
      return snapshot.generation;
    },
    finishPoll() {
      return applyRuntimePollingAction(state, (currentStore) => currentStore.actions.finishPoll());
    },
    isCurrentGeneration(jobId, generation) {
      const current = store.getSnapshot();
      return current.jobId === jobId && Number(generation) === Number(current.generation || 0);
    },
    startJob(jobId) {
      const snapshot = applyRuntimePollingAction(
        state,
        (currentStore) => currentStore.actions.startJob(jobId),
      );
      // 新任务即新计时：无条件刷新，避免沿用上一任务残留的 startedAt。
      if (host) {
        host.currentJobStartedAt = now();
      }
      return {
        generation: Number(snapshot.generation || 0),
        startedAt: `${host?.currentJobStartedAt || ""}`,
      };
    },
    startTimer(callback, intervalMs = JOB_POLL_INTERVAL_MS) {
      if (host?.timer) {
        clearIntervalFn(host.timer);
      }
      const timer = setIntervalFn(callback, intervalMs);
      if (host) {
        host.timer = timer;
      }
      return timer;
    },
  };
}

export function stopPolling(state: unknown) {
  createRuntimePollingStatePort(state as object).stop();
}

/**
 * 导航离开 / 返回主页时调用：停表 + 作废当前 generation，
 * 使 stop 之前发出的在途 poll 在返回后不再写回渲染。
 */
export function invalidateJobPolls(state: unknown) {
  createRuntimePollingStatePort(state as object).invalidate();
}

export function beginJobPoll(state: unknown) {
  return createRuntimePollingStatePort(state as object).beginPoll();
}

export function finishJobPoll(state: unknown) {
  createRuntimePollingStatePort(state as object).finishPoll();
}

export function isCurrentJobGeneration(
  state: unknown,
  jobId: unknown,
  generation: unknown,
) {
  return createRuntimePollingStatePort(state as object).isCurrentGeneration(jobId, generation);
}

export function startRuntimeJob(state: unknown, jobId: unknown) {
  return createRuntimePollingStatePort(state as object).startJob(jobId);
}

export function startPollingTimer(
  state: unknown,
  callback: (...args: unknown[]) => void,
  intervalMs = JOB_POLL_INTERVAL_MS,
) {
  createRuntimePollingStatePort(state as object).startTimer(callback, intervalMs);
}
