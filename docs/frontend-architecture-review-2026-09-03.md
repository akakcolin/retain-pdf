# retain-pdf 前端架构评审

- 评审日期：2026-09-03
- 范围：`frontend/src`（约 5 万行，672 个文件），含 `js/` 命令式领域层、`pages/` React 层、构建与测试
- 方法：静态代码审查（未运行动态用例）

---

## 0. 总结论

这是一个**正在从命令式 DOM 架构向 React 架构迁移、且迁移纪律良好**的前端：有 `FEATURES.md` / `composition/README.md` 写明边界，有 `architecture-boundaries.test.mjs` 做门禁，82 个测试文件，XSS 有两层防御（`render-answer-html.ts`），异步流有 generation / AbortController 防护。整体工程质量在中上水平。

主要问题集中在三类：

1. **交互健壮性**：轮询无超时、任务计时状态跨任务残留、停止轮询不能作废在途渲染（详见 P0）。
2. **状态基础设施的性能与正确性风险**：自研 store 每次读写都全量 `structuredClone` + 深冻结（P1-1）。
3. **双轨架构的过渡成本**：stringly-typed DOM 契约、30+ 参数的 mount 工厂、`any` 兜底、模块级单例（P1/P2）。

---

## 1. 架构现状（确认事实）

| 维度 | 现状 |
|------|------|
| 入口 | `index.html` / `reader.html` / `detail.html` / `studio.html` 四个 MPA 入口，esbuild 打 `app.bundle.js` 等 |
| 双轨 | `js/`（命令式领域：`mountXxxFeature` + ports + DOM 契约）与 `pages/`（React 19 + hooks + store）并存，`pages/home/composition/external.ts` 是唯一接线口，有边界测试门禁 |
| 状态 | 自研 `app-framework/store.ts`（不可变 + subscribe）；另有近乎遗留的全局可变单例 `js/state/store.ts`（仅 `desktop/index.ts` 使用） |
| 阅读器 | 双引擎：默认 react-pdf（`pages/reader`），`?engine=legacy` 回退旧 pdf.js 命令式引擎（`js/reader` + `pages/reader/legacy`） |
| 测试 | 82 个 `tests/*.test.mjs`，含架构边界、事件名契约、CSS 命名空间、XSS 向量锁 |

值得肯定的实践：composition 层"只接线不写业务"、轮询 generation 防串会话、AI 回答 HTML 双层消毒、流式请求的 abort 链路完整。

---

## 2. 交互逻辑问题（按严重度）

### P0-1 轮询 fetch 无超时、无 AbortSignal —— 后端 hang 时界面静默假死

位置：[jobs-query.ts `fetchJobPayload`](/Volumes/data/Projects/retain-pdf/frontend/src/js/api/jobs-query.ts)（行 9-24）+ [runtime-polling-state.ts](/Volumes/data/Projects/retain-pdf/frontend/src/js/features/job-runtime/runtime-polling-state.ts)（`beginPoll` 行 172-179）。

链路：`startPolling` 每 1s 触发 `fetchJob` → `beginPoll()` 在 `pollInFlight=true` 时返回 `null`，**整拍直接丢弃**；而 `fetchJobPayload` 是裸 `fetch`，没有超时也没有 signal。一旦后端连接挂起（TCP 保持但无响应），`pollInFlight` 永远为 true，之后所有轮询拍全部被丢弃，UI 停在旧进度，**无任何错误提示**，用户只能刷新页面。

对比：提交流程有 `withTimeout`（submit-flow.ts:200），轮询路径却没有。

建议：`fetchJobPayload` 加 `AbortSignal.timeout(8000)`（或 `withTimeout` 包裹），`fetchJob` 的 catch 里区分"超时/网络错误"并计数，连续 N 次失败在 error-box 给出可见提示而不是默默丢拍。

### P0-2 `currentJobStartedAt` 跨任务残留 —— 第二个任务可能继承第一个任务的开始时间

位置：[runtime-polling-state.ts `startJob`](/Volumes/data/Projects/retain-pdf/frontend/src/js/features/job-runtime/runtime-polling-state.ts)（行 187-199）：`if (host && !host.currentJobStartedAt) { host.currentJobStartedAt = now(); }`——只在为空时才写入。

而清空 `currentJobStartedAt` 的 `resetJob()`（reset-state-port.ts:101）**只在 `returnToHome` 时调用**（runtime-reset.ts:34）。`startPolling` 自身只调 `resetSecondary()`，后者不碰 `currentJobStartedAt`。

复现路径：任务 A 跑完（终态只 `clearActiveJobId` + `stop`，不清 startedAt）→ 用户不点"返回主页"，直接从工作流再提交任务 B → `startJob(B)` 看到 `currentJobStartedAt` 非空，沿用 A 的开始时间 → 占位帧的 `created_at/started_at` 用的是 A 的时刻，计时显示偏大。

缓解因素：`publishSubmitSuccess` 的 `syncCurrentJobSnapshot(..., { startedAt: now() })` 会写一份正确的 snapshot startedAt，所以**实际计时是否出错取决于 UI 读哪份 startedAt**——这正是问题所在：startedAt 存在两份真相（host 平铺字段 vs snapshot），且有一份是脏的。

建议：`startJob` 里无条件刷新 `currentJobStartedAt = now()`（新任务就是新开始），或 `startPolling` 改调能清 startedAt 的重置；长期应收敛为单一数据源。

### P0-3 `stop()` 不作废在途渲染 —— 返回主页后旧任务的最后一帧仍可能写回 store

位置：[runtime-polling-state.ts `stop()`](/Volumes/data/Projects/retain-pdf/frontend/src/js/features/job-runtime/runtime-polling-state.ts)（行 165-171）只清 timer、置 `pollInFlight=false`，**不 bump generation**。

时序：`fetchJob` 已发出请求（在途）→ 用户点"返回主页"→ `returnToHome` → `stopPolling` + `resetJob` → 在途请求返回 → `isCurrentGeneration(jobId, generation)` 仍然通过（jobId 与 generation 都没变）→ `renderContextPort.applySnapshot(payload)` 把旧任务数据写回 statusCardStore，并触发 `notifyLibraryJobUpdated`。

虽然主页视图此时不一定读这些字段（所以多半只是"写了没人看"），但书架 notify 是真发出的，且这类"stop 不能取消语义"是竞态隐患温床。

建议：`stop()` 里 generation + 1（使所有在途 poll 失效），代价为零。

### P1-4 提交按钮防重只在视图层 —— 逻辑层无重入守卫

位置：[app-actions/controller.ts `submitForm`](/Volumes/data/Projects/retain-pdf/frontend/src/js/features/app-actions/controller.ts)（行 162-202）。防重复提交完全依赖 `viewPort.setSubmitBusyState(true)` 禁用按钮。若视图 port 未生效、或将来出现第二个触发入口（快捷键、命令面板），`runSubmitFlow` 会被重入，产生重复任务。

建议：`submitForm` 开头加逻辑层守卫：`if (readUploadState().submitBusy) return;`（`submitBusy` 字段在 upload state 里已存在，只差一个判断）。

### P1-5 AI 问答 502 静默降级为本地回答

位置：[use-reader-ask-runtime.ts](/Volumes/data/Projects/retain-pdf/frontend/src/pages/reader/components/react-pdf/assistant/use-reader-ask-runtime.ts)（行 95-99、719）。后端 502 时自动切换 local answerer，用户无感知——回答来源从服务端模型变成本地逻辑，回答质量/引用能力不同但 UI 无标识。建议降级时在消息里附加可见提示（"服务暂不可用，以下为本地简要回答"）。

---

## 3. 架构质量问题（按优先级）

### P1-1 自研 store：每次读写全量深拷贝 + 深冻结，是性能与正确性双重隐患 ✅ 已修（2026-09-03）

位置：[app-framework/store.ts](/Volumes/data/Projects/retain-pdf/frontend/src/js/app-framework/store.ts)。

**实施结果**：改为「写时深冻一次 + 读/通知零拷贝」——

- `getSnapshot()` / 订阅通知直接共享已冻结的内部状态，读路径 O(1)（原先每次读都 `structuredClone` + 深冻全树）；
- action 仍收到可变草稿（契约允许 `state.items.push(x)` 原地改，有测试锁定），草稿克隆优先 `structuredClone`，遇到函数/File/类实例等不可克隆值退化为「plain 数据递归拷贝 + 其余按引用共享」并告警一次——原先直接抛 `DataCloneError` 崩掉整个 store；
- 快照只读语义不变：调用方原地改快照在严格模式下抛 TypeError（测试锁定）。

**修复过程发现并顺带修复的回归**：status-detail 的翻译状态袋（`createTranslationState()` 可变袋）被浅拷贝镜像进 `statusDetailStore`，`query` 子对象引用共享——写时冻结会把状态袋的 `query` 一并冻住，导致 `applyQuery` 就地写 `query.finalStatus` 抛 TypeError。已在 [status-detail-controller.ts](/Volumes/data/Projects/retain-pdf/frontend/src/pages/home/features/status-detail/status-detail-controller.ts) `syncTranslation` 处断开 `query` 引用。教训：**可变状态袋镜像进不可变 store 时，就地改写的子对象必须拷贝断开**。

测试：`frontend/tests/app-framework.test.mjs` 新增 3 条（零拷贝读、不可克隆值容错、previousState 冻结）。

~~原建议~~：~~store 内部不再克隆……~~（已按上述方案落地，保留可变草稿契约以兼容既有 action 写法）

### P1-2 mount 工厂"参数汤" + `any` 兜底，类型契约名存实亡 ✅ 部分修复（2026-09-03）

`mountJobRuntimeFeature` 解构 30+ 个字段且整体 `: any`（controller.ts:26-73）；`js/` 下 `: any` 共 159 处、分布在 102 个文件。结果是：重命名 port 方法、传错函数签名，编译器都发现不了，只能靠运行时炸。`composition/types.ts`（542 行）已有成型类型，但 js/ 侧工厂签名没有对接。

**实施结果**：核查发现 upload / workflow / app-actions 工厂其实已有成型 Options interface，真正裸奔的只有 `mountJobRuntimeFeature`。已为其补齐 `MountJobRuntimeFeatureOptions`（含 `JobRuntimePresentationPort` / `JobRuntimeLibraryEventPort` 等子接口，复用各 port 模块的导出类型），typecheck 零新增错误。`: any` 存量 159 → 157，并落地增量预算卡口（见路线图第 6 条）。剩余 157 行的消化是长期工作，靠预算只降不升来收口。

建议：从最大的 3 个工厂（job-runtime、upload、workflow）开始，把 `: any` 参数包替换为已存在的 interface；并在 CI 对 `js/` 设 `: any` 增量预算（只许减少）。

### P1-3 stringly-typed DOM 契约与"事件注册顺序靠注释约定" ✅ 已修（2026-09-03）

- `setText("error-box", ...)`、`setText("runtime-current-stage", "-")` 这类字符串 id 遍布 js/features（idle-reset.ts 一次写 18 个 id），id 拼写错误、HTML 删了节点，都无编译期反馈。
- `composition/README.md` 第 5 条：`workflowDialog.bindEvents()` 必须先于 `mountRecentJobsFeature`——注册顺序是**口头契约**，顺序错了只在特定交互路径（关闭对话框时 recent-jobs 不刷新）才暴露，无测试守护。

**实施结果**：

- 盘点发现一个架构事实：这些字符串在主页已不是 DOM id，而是 **React text-store 的文案槽位 key**（`InlineErrorBox` 读 `texts["error-box"]`），仅详情页仍是真实 DOM id——因此"HTML 存在性校验"不成立，改为统一常量 + 调用点禁字面量。
- 新增 [text-keys.ts](/Volumes/data/Projects/retain-pdf/frontend/src/js/dom/text-keys.ts)：`TEXT_KEYS` 收编全部 51 个槽位 key，14 个 `src/js` 调用文件全部改为常量引用（React 侧的本地 `setText`/`texts` 键不在本次范围）。
- 新增 [dom-text-keys.test.mjs](/Volumes/data/Projects/retain-pdf/frontend/tests/dom-text-keys.test.mjs)：① key 唯一 + kebab-case 校验；② `src/js` 禁止 `setText("字面量")`；③ 静态断言 `composition.ts` 中 `workflowDialog.bindEvents()` 先于 `createRuntimeFeatures(`——口头顺序契约升级为测试门禁。

### P1-4 `js/reader` 模块级可变单例 ✅ 已冻结（2026-09-03）

`region-interactions.ts`、`pdf-document.ts`、`view.ts` 等 8 个文件用模块级 `let` 存绑定状态（`readerRegionBinding`、`selectedReaderRegion`…）。后果：无法同页双实例、测试间必须小心顺序隔离、HMR 后状态残留。这是 legacy 引擎的既定形态，FEATURES.md 已声明"新功能不要写进来"——方向正确，需要的是**冻结**：给这些文件加 lint 规则禁止新增模块级 `let`。

- 盘点：8 个文件共 16 处模块级 `let`（ui-interaction-lock.ts 4、region-interactions.ts 4、pdf-layout.ts 2、view.ts 2，markdown-math/markdown-preview/markdown-render/pdf-document 各 1）。
- 冻结方式：在 [architecture-boundaries.test.mjs](/Volumes/data/Projects/retain-pdf/frontend/tests/architecture-boundaries.test.mjs) 追加预算式门禁 "legacy js/reader 模块级可变单例只减不增"——逐文件统计 `^(?:export\s+)?let\s` 行数，对照只降不升的预算 Map；新增 `let` 或超预算直接测试失败。比 lint 规则更贴现状：允许存量逐步消除，禁止任何新增。

### P1-5 巨型 hook：`use-reader-ask-runtime.ts` 1408 行 ✅ 已拆（2026-09-03）

单一文件承载了：消息分支树、会话 CRUD、本地快照持久化、流式取消、502 回退、assistant-ui 适配。内部其实已有清晰的段落注释，按段落拆成 `thread-tree.ts`（树结构纯函数）、`conversation-persistence.ts`（快照读写）、`use-ask-streaming.ts`（流式 + abort）三块即可，hook 本体只留编排。拆分后每块可独立单测（目前该文件的逻辑只能经 hook 间接测）。

**实施结果**：1408 行拆为三块——

- [thread-tree.ts](/Volumes/data/Projects/retain-pdf/frontend/src/pages/reader/components/react-pdf/assistant/thread-tree.ts)（272 行）：分支树/消息适配/citations 归一化等**纯函数**，逐行迁移、行为不变；新增 7 条直接单测 [reader-ask-thread-tree.test.mjs](/Volumes/data/Projects/retain-pdf/frontend/tests/reader-ask-thread-tree.test.mjs)（此前这些逻辑只能经 hook 间接测）——`pathForBranch` 的断链退化、`visibleMessages` 的环防护等微妙行为现在有锁。
- [session-operations.ts](/Volumes/data/Projects/retain-pdf/frontend/src/pages/reader/components/react-pdf/assistant/session-operations.ts)（485 行）：新建/切换/删除/重命名/分支五个会话操作的**纯工厂**，React 状态经 getter/setter、可变引用经 refs 袋显式注入；40/80ms 隔离、switchToken 防串、rAF 滚动等时序逐行保留。
- hook 本体 808 行：流式运行（rAF 旁路 + abort 闸）、hydrate/持久化 effect、assistant-ui 适配——编排集中在这里。
- 顺带修复一个存量类型 bug：`ask-answerer` 的 `ensureLoaded` 声明 0 参但调用方传 jobId（TS2554），已补 `_jobId?: string` 与 markdown-answerer 对齐；typecheck 存量错误 41 → 40。

流式段（`scheduleAnswerText` / `runAssistant` 的 rAF + abort 链路）留在 hook 内——它与 refs/setState 耦合最紧，再拆的收益/风险比不划算，后续如需可抽 `use-ask-stream.ts`。

### P2-6 全局 `js/state/store.ts` 单例近乎遗留 ✅ 已下线（2026-09-03）

仅 `desktop/index.ts` 真正消费。建议要么明确它只做 desktop 启动配置的容器（改名/挪位置，避免误导新人往里面塞状态），要么彻底并入 app-framework store。

- 核查确认：`state/store.ts` 的模块级 `export const state = createInitialState()` 全仓库仅 3 个消费方——`desktop/index.ts`（显式传入各 action）、`state/actions.ts`（`target = state` 默认值，生产无人依赖）、smoke 脚本（断言用）；`job-runtime.test.mjs` 的 import 是完全未用的死引用（测试内全部 shadow 为局部 `createInitialState()`）。
- 处理：**删除 `state/store.ts`**；`state/actions.ts` 17 个 action 的 `target` 改为必填显式参数（新增 `InitialState` 类型导出自 `slices.ts`）；`desktop/index.ts` 持有自己的 `export const desktopState = createInitialState()`（注释声明仅供 smoke 断言）；smoke 脚本改从 desktop 模块取实例；删除测试死 import。
- 门禁：`architecture-boundaries.test.mjs` 新增 "global state singleton stays retired"——`state/store.ts` 不得复活、actions.ts 不得再出现 `= state` 默认目标。

### P2-7 时序魔数 ✅ 已命名收编（2026-09-03）

`MODE_RESTORE_DELAYS_MS = [0, 48, 140, 320, 560]`、`GOTO_ALIGN_DELAYS_MS`、`publishSubmitSuccess` 里的 800ms 延迟刷新。滚动锚点的延时数组有注释解释且工程上难免，但建议集中到一个 `timing.ts` 并注明每个值的由来与失效后果；800ms 刷新这类"等后端落库"的延迟，应优先改为事件/轮询确认驱动。

- 全量盘点：`src/` 下共 15 处 `setTimeout` 数字字面量 + 5 个已命名常数，分布在 12 个文件。
- **为什么不建全局 `timing.ts`**：值分属 reader-pdf / reader-ai / home / js-features 四个层级，全局文件会迫使 reader/home 经 external 桶跨层 import 互不相关的常数，反而增加耦合。改为**按模块就近集中**：每个文件顶部设"时序常数"块，逐个注明由来与失效后果。
- 已收编：`useReadingAnchor.ts`（5 个存量常数补注释 + 新增 `ATTACH_RETRY_MS`）、`session-operations.ts`（40/80/200/900/1200/350 → `SESSION_SETTLE_MS` 等 6 个）、`submit-flow.ts`（800 → `POST_CREATE_PROJECTION_ALIGN_MS`）、`usePageRowSync.ts`（100/300/700）、`useCurrentPage.ts`（120）、`useHomeReturnRestore.ts`（80/320 → 数组）、`DecorStage`（5000）、`ReaderNotesPanel`（1800）、`ReaderAiPanel`（6000）、`InlineErrorBox`（1600）、`reader-annotations-app`（2000）、`downloads.ts`（240）。
- 800ms 未改轮询确认：`requestRefresh` 是 fire-and-forget 命令端口（`library-event-contract.ts`），缺"投影是否已含 jobId"的查回通道；改事件驱动需扩契约，已留 TODO 注释，列入后续候选。
- 行为零变化（纯命名 + 注释）；全量测试 778 通过，tsc 存量 40 不变，any 预算 157/157。

---

## 4. 改进路线图

**本周（交互正确性，改动小、收益直接）**
1. ✅ 轮询 fetch 加超时（P0-1，2026-09-03 已修）：`fetchJobPayload` 携带 `AbortSignal.timeout(10000)`，超时错误翻译为中文提示；测试见 `frontend/tests/jobs-query.test.mjs`
2. ✅ `startJob` 无条件刷新 `currentJobStartedAt`（P0-2，2026-09-03 已修）
3. ✅ 导航离开作废旧 poll（P0-3，2026-09-03 已修）：新增 `invalidate` 动作 / `invalidateJobPolls()`，`returnJobRuntimeToHome` 改调它；`stop()` 保持不动 generation（终态副资源调度依赖旧值，见测试 "stop() keeps generation…"）
4. ✅ `submitForm` 逻辑层重入守卫（P1-4，2026-09-03 已修）：入口检查 `submitBusy`

**本月（状态基础设施）**

5. ✅ store 去掉全量克隆/深冻结，加不可克隆值开发期检测（P1-1，2026-09-03 已修：写时深冻 + 读零拷贝 + 容错克隆告警）
6. ✅ 三大工厂的 `: any` 参数包接上类型（P1-2，2026-09-03 已修）：核查后实际只有 `mountJobRuntimeFeature` 仍是 `: any`（upload / workflow / app-actions 已有成型 Options interface）；已为其新增 `MountJobRuntimeFeatureOptions`（复用 `RuntimePollingStatePort` / `CurrentJobStatePort` / `SecondaryResourceStatePort` 等既有导出类型），`: any` 存量 159 → 157；并新增 `: any` 增量预算卡口 `frontend/scripts/check-any-budget.mjs`（`npm run check:any-budget`，预算 157 只降不升），已挂进 `.github/workflows/desktop-frontend-sync.yml`
7. ✅ DOM 槽位 key 常量化 + 装配顺序进边界测试（P1-3，2026-09-03 已修：`js/dom/text-keys.ts` + `tests/dom-text-keys.test.mjs`）

**本季度（结构收敛）**

8. ✅ 拆 `use-reader-ask-runtime.ts`（P1-5，2026-09-03 已拆：1408 → 808 + thread-tree 272 + session-operations 485，thread-tree 有 7 条直接单测）；✅ legacy `js/reader` 模块级 `let` 冻结门禁（P1-4，2026-09-03 已修：盘点 16 处存量进预算 Map，只减不增）
9. ✅ 收敛/下线全局 state 单例（P2-6，2026-09-03 已修：删除 `state/store.ts`，actions 改显式 target，desktop 持有 `desktopState`，含防复活门禁）；✅ 时序常数集中管理（P2-7，2026-09-03 已修：15 处字面量 + 5 个存量常数全部按模块就近命名并注由来/失效后果，800ms 事件化留 TODO 待契约扩展）
10. legacy 引擎功能对齐后制定 `js/reader` 退役标准（如：连续 N 版本无 `?engine=legacy` 回退工单）

---

## 5. 审查未覆盖项（诚实声明）

- 未运行测试套件与冒烟脚本，上述结论全部来自静态阅读；P0-2 的实际表现取决于 UI 读哪份 startedAt，建议用"连提两个任务看计时"的动态用例确认后再修。
- `pages/home/features/library/**`（书架）与 `pages/detail/**` 只做了抽查，未逐文件审查。
- 样式体系（styles/themes、CSS 命名空间测试）未评审。
