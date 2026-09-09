// app-framework/store → React 的适配 hook。
//
// store.getSnapshot() 返回写时冻结的内部状态引用(稳定),可直接作为
// useSyncExternalStore 的 getSnapshot。不要再在这里缓存一份:缓存只在
// subscribe 回调里刷新,组件卸载期间的写入会让缓存变陈旧,重新挂载时渲染
// 旧快照直到下一次 notify。
//
// selector 支持:对 selector 结果做浅比较缓存,高频轮询的大快照(recent-jobs)
// 只在所选切片真正变化时才触发该组件重渲染。

import { useCallback, useRef, useSyncExternalStore } from "react";

export function shallowEqual(a, b) {
  if (Object.is(a, b)) {
    return true;
  }
  if (!a || !b || typeof a !== "object" || typeof b !== "object") {
    return false;
  }
  const keysA = Object.keys(a);
  const keysB = Object.keys(b);
  if (keysA.length !== keysB.length) {
    return false;
  }
  return keysA.every((key) => Object.is(a[key], b[key]));
}

export function useStoreSnapshot(store, selector = null, isEqual = shallowEqual) {
  const selectionRef = useRef({ hasValue: false, value: null });

  const subscribe = useCallback(
    (onStoreChange) => store.subscribe(onStoreChange),
    [store],
  );

  const getSnapshot = useCallback(() => {
    const snapshot = store.getSnapshot();
    if (typeof selector !== "function") {
      return snapshot;
    }
    const next = selector(snapshot);
    const previous = selectionRef.current;
    if (previous.hasValue && isEqual(previous.value, next)) {
      return previous.value;
    }
    selectionRef.current = { hasValue: true, value: next };
    return next;
  }, [store, selector, isEqual]);

  return useSyncExternalStore(subscribe, getSnapshot);
}
