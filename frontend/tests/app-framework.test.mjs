import test from "node:test";
import assert from "node:assert/strict";

import {
  createCommandBus,
} from "../src/js/app-framework/commands.js";
import {
  createResource,
} from "../src/js/app-framework/resource.js";
import {
  createSelector,
} from "../src/js/app-framework/selector.js";
import {
  createStore,
} from "../src/js/app-framework/store.js";

test("createStore owns immutable snapshots and action updates", () => {
  const store = createStore({
    name: "library",
    initialState: {
      items: [],
      selectedId: "",
    },
    actions: {
      addBook(state, book) {
        state.items.push(book);
        return state;
      },
      selectBook(state, jobId) {
        return {
          ...state,
          selectedId: jobId,
        };
      },
    },
  });

  const events = [];
  const unsubscribe = store.subscribe((snapshot, meta) => {
    events.push({ snapshot, meta });
  });

  store.actions.addBook({ jobId: "job-1" });
  store.actions.selectBook("job-1");
  unsubscribe();
  store.actions.addBook({ jobId: "job-2" });

  assert.deepEqual(store.getSnapshot(), {
    items: [{ jobId: "job-1" }, { jobId: "job-2" }],
    selectedId: "job-1",
  });
  assert.equal(events.length, 2);
  assert.equal(events[0].meta.action, "addBook");
  assert.equal(events[0].meta.store, "library");
  assert.throws(() => {
    events[0].snapshot.items = [];
  }, TypeError);
});

test("createStore batches multiple updates into one subscriber notification", () => {
  const store = createStore({
    name: "status",
    initialState: {
      stage: "ocr",
      percent: 0,
    },
    actions: {
      setStage(state, stage) {
        return {
          ...state,
          stage,
        };
      },
      setPercent(state, percent) {
        return {
          ...state,
          percent,
        };
      },
    },
  });
  const events = [];
  store.subscribe((snapshot, meta) => {
    events.push({ snapshot, meta });
  });

  const result = store.batch(({ actions }) => {
    actions.setStage("translation");
    actions.setPercent(75);
    return "done";
  });

  assert.equal(result, "done");
  assert.deepEqual(store.getSnapshot(), {
    stage: "translation",
    percent: 75,
  });
  assert.equal(events.length, 1);
  assert.equal(events[0].meta.action, "setStage");
  assert.deepEqual(events[0].meta.previousState, {
    stage: "ocr",
    percent: 0,
  });
});

test("createCommandBus dispatches commands and can isolate handler errors", async () => {
  const errors = [];
  const commands = createCommandBus({
    onError: (error, meta) => errors.push({ error, meta }),
  });
  const calls = [];
  commands.on("reader:open", async (payload, meta) => {
    calls.push({ payload, meta });
    return payload.jobId;
  });
  commands.on("reader:open", () => {
    throw new Error("boom");
  });

  const results = await commands.dispatch("reader:open", { jobId: "job-1" });

  assert.deepEqual(results, ["job-1"]);
  assert.equal(calls[0].meta.command, "reader:open");
  assert.equal(errors.length, 1);
  assert.equal(errors[0].meta.command, "reader:open");
});

test("createResource owns loading success error and stale request guards", async () => {
  let releaseFirst;
  const firstLoad = new Promise((resolve) => {
    releaseFirst = () => resolve({ value: "first" });
  });
  const loads = [];
  const resource = createResource({
    name: "jobDetail",
    loader: async ({ jobId }) => {
      loads.push(jobId);
      if (jobId === "slow") {
        return firstLoad;
      }
      return { value: jobId };
    },
    cacheKey: ({ jobId }) => jobId,
  });

  const slowPromise = resource.load({ jobId: "slow" });
  const fastSnapshot = await resource.load({ jobId: "fast" });
  releaseFirst();
  await slowPromise;

  assert.equal(fastSnapshot.status, "success");
  assert.deepEqual(resource.getSnapshot().data, { value: "fast" });

  await resource.load({ jobId: "fast" });
  assert.deepEqual(loads, ["slow", "fast"]);

  resource.invalidate({ jobId: "fast" });
  await resource.load({ jobId: "fast" });
  assert.deepEqual(loads, ["slow", "fast", "fast"]);
});

test("createResource records loader errors without throwing by default", async () => {
  const resource = createResource({
    name: "failing",
    loader: async () => {
      throw new Error("network failed");
    },
  });

  const snapshot = await resource.load();

  assert.equal(snapshot.status, "error");
  assert.equal(snapshot.error.message, "network failed");
});

test("createSelector memoizes derived view models by input values", () => {
  let calls = 0;
  const selector = createSelector([
    (state) => state.count,
    (state) => state.label,
  ], (count, label) => {
    calls += 1;
    return { title: `${label}:${count}` };
  });

  const first = selector({ count: 1, label: "ocr", ignored: "a" });
  const second = selector({ count: 1, label: "ocr", ignored: "b" });
  const third = selector({ count: 2, label: "ocr", ignored: "b" });

  assert.equal(first, second);
  assert.notEqual(second, third);
  assert.deepEqual(third, { title: "ocr:2" });
  assert.equal(calls, 2);
});


test("createStore shares one frozen snapshot across reads (zero-copy read path)", () => {
  const store = createStore({
    name: "zero-copy",
    initialState: { count: 1, nested: { label: "a" } },
    actions: {
      bump(state) {
        return { ...state, count: state.count + 1 };
      },
    },
  });

  const first = store.getSnapshot();
  const second = store.getSnapshot();
  assert.equal(first, second, "读路径必须零拷贝：两次 getSnapshot 返回同一冻结引用");
  assert.equal(Object.isFrozen(first), true);
  assert.equal(Object.isFrozen(first.nested), true);

  const third = store.actions.bump();
  assert.notEqual(third, first, "写后产生新快照");
  // 草稿契约是可变深拷贝，未变更子树不保证引用共享；但值必须一致且被冻结
  assert.deepEqual(third.nested, { label: "a" });
  assert.equal(Object.isFrozen(third.nested), true);
  assert.equal(store.getSnapshot(), third);
});

test("createStore tolerates non-cloneable values by reference instead of crashing", () => {
  // 修复前：structuredClone 遇函数直接抛 DataCloneError，整个 store 不可用
  const handler = () => "called";
  const store = createStore({
    name: "non-cloneable",
    initialState: { handler, count: 0 },
    actions: {
      bump(state) {
        return { ...state, count: state.count + 1 };
      },
    },
  });

  assert.equal(store.getSnapshot().handler, handler, "函数按引用共享");
  const next = store.actions.bump();
  assert.equal(next.count, 1);
  assert.equal(next.handler, handler);
  assert.equal(Object.isFrozen(next), true);
});

test("createStore freezes previousState delivered to subscribers", () => {
  const store = createStore({
    name: "prev-frozen",
    initialState: { stage: "ocr" },
    actions: {
      setStage(state, stage) {
        return { ...state, stage };
      },
    },
  });
  const metas = [];
  store.subscribe((_snapshot, meta) => metas.push(meta));

  store.actions.setStage("translation");

  assert.equal(metas.length, 1);
  assert.equal(metas[0].previousState.stage, "ocr");
  assert.equal(Object.isFrozen(metas[0].previousState), true);
  assert.throws(() => {
    metas[0].previousState.stage = "render";
  }, TypeError);
});
