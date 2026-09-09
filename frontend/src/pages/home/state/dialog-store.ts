// 通用对话框开合状态工厂(蓝图 §0.3)——CredentialsDialog/GlossariesDialog/
// AppUpdate 详情/SettingsHubDialog 等常驻挂载的原生 <dialog> 共用同一套语义。
//
// 参照 src/pages/reader/legacy/state/drawer-store.js 的模式(open/subscribe 契约),
// 但对话框不是"多选一互斥"而是"单个开合 + 可选负载"(setupMode、初始 tab 等),
// 所以状态形状是 { open, payload } 而不是 drawer 的单一 active 字符串。
//
// getState() 返回的对象引用只在 open()/close() 调用时才更新(不是每次读取都
// 新建),可以直接喂 useSyncExternalStore 而不会触发无限重渲染
//（不存在 app-framework/store.js 的 getSnapshot 克隆雷点)。

export type DialogState<T = unknown> = {
  open: boolean;
  payload: T;
};

export type DialogStore<T = unknown> = {
  subscribe: (listener: (state: DialogState<T>) => void) => () => void;
  getState: () => DialogState<T>;
  open: (payload?: T | null) => DialogState<T>;
  close: () => DialogState<T>;
};

export function createDialogStore<T = unknown>(initialPayload: T | null = null): DialogStore<T> {
  let state: DialogState<T> = { open: false, payload: initialPayload as T };
  const listeners = new Set<(state: DialogState<T>) => void>();

  function notify() {
    listeners.forEach((listener) => listener(state));
  }

  return {
    // useSyncExternalStore 兼容:subscribe 返回退订函数
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    getState: () => state,
    open(payload = null) {
      // 传 null = 显式清空负载(合集"新建"模式);不再沿用旧 payload——否则
      // 管理过某合集后再点"新建合集",弹窗仍是"管理合集"并带着上次的勾选态。
      state = { open: true, payload: payload as T };
      notify();
      return state;
    },
    close() {
      if (!state.open) {
        return state;
      }
      state = { open: false, payload: state.payload };
      notify();
      return state;
    },
  };
}
