/**
 * Generic immutable store.
 *
 * Action reducers: `(state: TState, ...args) => Partial<TState> | TState`
 * Runtime always replaces state with the returned object (no deep merge).
 *
 * 性能模型（2026-09 重构）：
 * - 写：克隆一次可变草稿给 updater（action 允许原地改草稿），返回对象写时深冻一次；
 * - 读：getSnapshot() / 订阅通知零拷贝，直接共享已冻结的内部状态；
 * - 快照只读：调用方原地修改会在严格模式下抛 TypeError（ESM 默认严格）；
 * - 非 plain 数据（函数 / Map / 类实例）按引用共享、不冻结，且只告警一次。
 *
 * Typed call sites:
 *   createStore<State, Actions>({ initialState, actions })
 * or rely on inference from `initialState` + `actions`.
 *
 * Defaults are `any` so bare `createStore({...})` stays loose when inference
 * cannot pin a shape (and `actions` remains a string-index map).
 */

type IsAny<T> = 0 extends 1 & T ? true : false;

export type StoreActionResult<TState> = Partial<TState> | TState;

/** Action reducer: (state, ...args) => next state (or partial). */
export type StoreAction<TState, TArgs extends any[] = any[]> = (
  state: TState,
  ...args: TArgs
) => StoreActionResult<TState>;

/**
 * Map action reducer map → bound action API (state arg removed; returns snapshot).
 * When TActions is `any`, expose a string-index map so existing call sites keep working.
 */
export type BoundStoreActions<TState, TActions> = IsAny<TActions> extends true
  ? Record<string, (...args: any[]) => TState>
  : {
      [K in keyof TActions]: TActions[K] extends (
        state: any,
        ...args: infer TArgs
      ) => any
        ? (...args: TArgs) => TState
        : never;
    };

export type StoreChangeMeta<TState> = {
  action: string;
  previousState: TState;
  store: string;
};

export type StoreListener<TState> = (
  snapshot: TState,
  meta: StoreChangeMeta<TState>,
) => void;

export type StoreSetState<TState> = (
  updater: TState | ((state: TState) => StoreActionResult<TState>),
  actionName?: string,
) => TState;

export type StoreBatchApi<TState, TActions> = {
  actions: BoundStoreActions<TState, TActions>;
  getSnapshot: () => TState;
  setState: StoreSetState<TState>;
};

export type Store<TState = any, TActions = any> = {
  readonly name: string;
  batch: <TResult = TState>(
    callback: (api: StoreBatchApi<TState, TActions>) => TResult,
  ) => TResult;
  getSnapshot: () => TState;
  setState: StoreSetState<TState>;
  subscribe: (listener: StoreListener<TState>) => () => void;
  reset: (nextState?: TState) => TState;
  actions: Readonly<BoundStoreActions<TState, TActions>>;
};

export type CreateStoreOptions<TState, TActions> = {
  name?: string;
  initialState?: TState;
  actions?: TActions;
};

/**
 * Structural constraint for action maps.
 * Return type is intentionally loose (`any`) so call sites can annotate
 * `state: SomeState` even when `TState` is a narrower inferred object.
 * Documented contract remains Partial<TState> | TState.
 */
export type StoreActionsConstraint<TState> = Record<
  string,
  (state: TState, ...args: any[]) => any
>;

function isPlainObject(value: unknown): value is Record<string, unknown> {
  if (!value || typeof value !== "object") {
    return false;
  }
  const proto = Object.getPrototypeOf(value);
  return proto === Object.prototype || proto === null;
}

/**
 * 可变草稿克隆：优先 structuredClone（原生深拷贝）。
 * 遇到不可克隆值（函数 / File / DOM 节点 / 类实例）时退化为
 * 「plain object / 数组递归拷贝，其余按引用传递」——宁可引用共享，
 * 不可让整个 setState 崩溃（修复前 structuredClone 直接抛异常）。
 */
function cloneMutable<T>(value: T, onFallback?: (value: unknown) => void): T {
  if (typeof structuredClone === "function") {
    try {
      return structuredClone(value);
    } catch {
      // fall through to tolerant clone
    }
  }
  return cloneMutableTolerant(value, onFallback);
}

function cloneMutableTolerant<T>(value: T, onFallback?: (value: unknown) => void): T {
  if (Array.isArray(value)) {
    return value.map((item) => cloneMutableTolerant(item, onFallback)) as T;
  }
  if (isPlainObject(value)) {
    const copy: Record<string, unknown> = {};
    for (const [key, item] of Object.entries(value)) {
      copy[key] = cloneMutableTolerant(item, onFallback);
    }
    return copy as T;
  }
  if (value !== null && typeof value === "object" || typeof value === "function") {
    onFallback?.(value);
  }
  return value;
}

/**
 * 快照冻结：写时深冻一次（plain object / 数组），读与通知零拷贝。
 * 已冻结的子树直接跳过——action 以 spread 拷贝时未变更分支保持共享引用，
 * 因此重复写的冻结成本约等于「变更路径」而非全树。
 * 非 plain 数据（Map / 类实例 / 函数）不冻结、按引用共享。
 */
function freezeForSnapshot<T>(value: T): T {
  if (!value || typeof value !== "object" || Object.isFrozen(value)) {
    return value;
  }
  if (Array.isArray(value)) {
    for (const item of value) {
      freezeForSnapshot(item);
    }
    return Object.freeze(value);
  }
  if (isPlainObject(value)) {
    for (const item of Object.values(value)) {
      freezeForSnapshot(item);
    }
    return Object.freeze(value);
  }
  return value;
}

/**
 * Create a typed store. Defaults stay `any` so existing untyped call sites keep compiling.
 *
 * @example
 * const store = createStore<CounterState, CounterActions>({
 *   initialState: { count: 0 },
 *   actions: {
 *     inc(state, by = 1) {
 *       return { ...state, count: state.count + by };
 *     },
 *   },
 * });
 * store.actions.inc(2); // typed
 */
export function createStore<
  TState = any,
  TActions extends StoreActionsConstraint<TState> = any,
>({
  name = "store",
  initialState = {} as TState,
  actions = {} as TActions,
}: CreateStoreOptions<TState, TActions> = {} as CreateStoreOptions<
  TState,
  TActions
>): Store<TState, TActions> {
  let warnedNonCloneable = false;
  const warnNonCloneable = (value: unknown) => {
    if (warnedNonCloneable || typeof console === "undefined") {
      return;
    }
    warnedNonCloneable = true;
    console.warn(
      `Store "${name}" 含有不可深拷贝的值（按引用共享，勿原地改）：`,
      value,
    );
  };

  let state = freezeForSnapshot(cloneMutable(initialState, warnNonCloneable));
  const listeners = new Set<StoreListener<TState>>();
  let batchDepth = 0;
  let pendingNotification: {
    action: string;
    previousState: TState;
  } | null = null;

  /** 快照即内部状态（写时已冻结）；读路径 O(1)，禁止调用方原地改。 */
  function getSnapshot(): TState {
    return state;
  }

  function notify(actionName: string, previousState: TState) {
    const snapshot = getSnapshot();
    for (const listener of listeners) {
      listener(snapshot, {
        action: actionName,
        previousState,
        store: name,
      });
    }
  }

  function queueNotification(actionName: string, previousState: TState) {
    if (batchDepth <= 0) {
      notify(actionName, previousState);
      return;
    }
    pendingNotification = {
      action: pendingNotification?.action || actionName,
      previousState: pendingNotification?.previousState || previousState,
    };
  }

  function setState(
    updater: TState | ((state: TState) => StoreActionResult<TState>),
    actionName = "setState",
  ): TState {
    const previousState = state;
    // updater 收到可变草稿（契约允许原地改）；直接对象形式不克隆，由调用方让渡所有权。
    const nextState = typeof updater === "function"
      ? (updater as (state: TState) => StoreActionResult<TState>)(cloneMutable(state, warnNonCloneable))
      : updater;
    if (!nextState || typeof nextState !== "object") {
      throw new TypeError(`Store "${name}" action "${actionName}" must return an object state.`);
    }
    state = freezeForSnapshot(nextState as TState);
    queueNotification(actionName, previousState);
    return getSnapshot();
  }

  const boundActions = {} as BoundStoreActions<TState, TActions>;
  for (const [actionName, action] of Object.entries(actions || {})) {
    if (typeof action !== "function") {
      continue;
    }
    (boundActions as Record<string, (...args: any[]) => TState>)[actionName] = (
      ...args: any[]
    ) => setState(
      (draft) => (action as StoreAction<TState>)(draft, ...args),
      actionName,
    );
  }

  function subscribe(listener: StoreListener<TState>) {
    if (typeof listener !== "function") {
      return () => {};
    }
    listeners.add(listener);
    return () => {
      listeners.delete(listener);
    };
  }

  function reset(nextState: TState = initialState) {
    // 克隆一次，避免冻结/共享调用方持有的对象
    return setState(cloneMutable(nextState, warnNonCloneable), "reset");
  }

  function batch<TResult = TState>(
    callback: (api: StoreBatchApi<TState, TActions>) => TResult,
  ): TResult {
    if (typeof callback !== "function") {
      return getSnapshot() as unknown as TResult;
    }
    batchDepth += 1;
    try {
      return callback({
        actions: boundActions,
        getSnapshot,
        setState,
      });
    } finally {
      batchDepth -= 1;
      if (batchDepth === 0 && pendingNotification) {
        const notification = pendingNotification;
        pendingNotification = null;
        notify(notification.action, notification.previousState);
      }
    }
  }

  return Object.freeze({
    name,
    batch,
    getSnapshot,
    setState,
    subscribe,
    reset,
    actions: Object.freeze(boundActions),
  });
}
