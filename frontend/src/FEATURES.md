# 前端功能树地图（`frontend/src`）

日常开发以 **`frontend/`** 为准。  
本文说明**两套「features」**、阅读器双引擎、以及共享 `js/*` 该往哪放。

## 总览

```text
frontend/src/
├── pages/
│   ├── home/          # 主页 SPA（React 壳 + 装配）
│   │   ├── composition/   # 只接线：external → js/features + home features
│   │   └── features/      # React UI / store / 主页专属编排
│   ├── reader/        # 阅读器 SPA（默认 react-pdf；legacy 回退）
│   └── detail/        # 任务详情 SPA
├── js/
│   ├── api/           # HTTP / 后端契约
│   ├── features/      # 命令式领域逻辑（mount*、ports、state）
│   ├── reader/        # 旧 pdf.js 引擎 + 少量被新引擎复用的 ports
│   ├── job/ status-detail/  # 任务展示纯逻辑（job-detail → pages/detail/legacy，job-status → pages/home/features/status）
│   ├── state/ config/ mock/ islands/ …
└── styles/ components/ shared/ partials/
```

| 层 | 路径 | 职责 | 新代码放哪 |
|----|------|------|------------|
| **页面 React** | `pages/*/features` 或 `pages/reader/*` | UI、hooks、page store | 新 UI / 新交互 |
| **命令式领域** | `js/features/*` | 挂载、轮询、表单、ports | 跨页可复用的非 UI 逻辑 |
| **共享 API** | `js/api/*` | fetch 封装 | 新后端端点客户端 |
| **装配** | `pages/home/composition/*` | 接线，不写业务 | 只改 wiring |

---

## 主页：双 features 树

主页同时存在：

1. **`src/js/features/*`** — 旧主路径抽出来的**命令式领域**（`mountXxxFeature`、ports、DOM contract、state）
2. **`src/pages/home/features/*`** — **React 侧**视图、store、对话框、书架 UI

它们**不是重复目录**，而是 **UI 层 vs 领域层**。接线规则见 `pages/home/composition/README.md`：

- 领域工厂应经 **`composition/external.ts`** 引用 `js/*`（避免在 features 里满天飞 `../../../js`）
- 例外：少量类型 / 纯函数已直接从 `pages/home/features` import `js/features`（历史债务，新增优先走 external）

### 对照表（名字相近 ≠ 同一模块）

| `js/features/` | `pages/home/features/` | 关系 |
|----------------|------------------------|------|
| `upload/` | `upload/` | 领域 mount + form ↔ React upload view store |
| `workflow/` + `translation-workflow-dialog/` | `workflow/` | 命令式 workflow + 对话框契约 ↔ React 工作流对话框 |
| `credentials/` | `credentials/` | 凭据 mount/DOM ↔ React 设置 UI |
| `glossaries/` | `glossaries/` | 术语表 controller ↔ React 术语表 |
| `app-update/` | `app-update/` | GitHub release / cache ↔ React 更新条 |
| `app-shell/` | `app-shell/` | idle reset / config ↔ 底栏等壳 UI |
| `app-actions/` | （无同名） | 提交任务；由 composition 挂到 status/upload |
| `job-runtime/` | （无同名） | 当前任务轮询；status / library 消费 |
| `recent-jobs/` + `documents-library/` | `library/` + `collections/` | 最近任务 + 文档资源 ↔ 书架卡片 / 合集 |
| `status-detail/` | `status/` + `status-detail/` | 详情逻辑 ↔ 状态卡 / 详情弹窗 React |
| `reader-dialog/` | `reader/` | 阅读入口路由/契约 ↔ 主页「打开阅读」对话框 store |
| `home/` | （散落） | home state port |
| `artifact-downloads/` | （经 library/status） | 产物下载 |
| （无） | `settings/` | 主页设置入口（多靠 credentials/update） |

### 主页改代码口诀

| 你要改… | 优先路径 |
|---------|----------|
| 书架卡片 / 详情弹窗 UI | `pages/home/features/library/**` |
| 上传表单 UI | `pages/home/features/upload/**` |
| 任务轮询、active job | `js/features/job-runtime/**` |
| 提交翻译任务 | `js/features/app-actions/**` + composition |
| 把新 `js` 依赖接进主页 | **只改** `composition/external.ts` + 对应 `create-*.ts` |

`library` 子目录约定见 `pages/home/features/library/README.md`。

---

## 阅读器：三层边界（与主页 features 无关）

| 层 | 入口 / 路径 | js 依赖方式 |
|----|-------------|-------------|
| **A 新引擎（默认）** | `ReaderAppReactPdf` + `hooks/` `pdf/` `annotations/` `components/react-pdf/` | 只经 **`pages/reader/external.ts`** |
| **B 共享 ports** | `pages/reader/legacy` 子集：data/config/resource/pdf-document/page-state… | 经 external 出口；勿塞进 pdf-controller |
| **C legacy** | `?engine=legacy` → **`pages/reader/legacy/**`**（引擎主力）+ `js/reader/{annotations,ai,downloads}` | legacy 可直接 import `js/reader/**` |

详情：`pages/reader/README.md`、`js/reader/README.md`。

**新功能不要写进 `legacy/` 或 `js/reader/annotations`。**

## 详情页

| 路径 | 规则 |
|------|------|
| `pages/detail/**` | js 只经 **`pages/detail/external.ts`** |
| `pages/detail/legacy/*` | overview / markdown / resume 命令式逻辑 |

---

## `js/` 其它目录（速查）

| 目录 | 用途 |
|------|------|
| `api/` | 后端 API 客户端 |
| `job/` | 任务阶段 / 产物逻辑（detail + home status 共用）；`job-detail/` → `pages/detail/legacy/`，`job-status/` → `pages/home/features/status/` |
| `status-detail/` | 状态详情 presenter（偏旧路径；与 `js/features/status-detail` 并存时以实际 import 为准） |
| `state/`、`config/` | 全局 store 切片、runtime 配置 |
| `islands/` | 可挂到旧 HTML 的小岛（如 library-search、reader-annotations） |
| `mock/` | 测试与本地 mock |
| `app-framework/` | 轻量 connector/store 原语 |
| `styles/` | **按页拆包** `dist/css/{home,detail,reader}.css`；见 **`styles/README.md`** |

---

## 死代码策略

- **先文档、再删**：`rg` 无 importer 仍可能是动态路径或测试专用。
- `js/reader` 顶层引擎与 `favorites/` 已迁 `pages/reader/legacy/`；余下 `{annotations,ai,downloads}` 子目录仍被 legacy 链路引用（含内部引用）。
- **`pages/home/features` → `src/js/*`**：经 `pages/home/composition/external.ts`。
- **`pages/detail` → `src/js/*`**：经 `pages/detail/external.ts`。
- **`pages/reader` 非 legacy → `src/js/*`**：经 `pages/reader/external.ts`；`legacy/**` 除外。
- **不要**批量删除 `pages/reader/legacy/favorites/*`——它们经 `selection-favorites` 服务 `?engine=legacy`。

---

## 相关 README

| 文件 | 内容 |
|------|------|
| `frontend/README.md` | 入口、命令、与桌面端 bundle 关系 |
| `pages/home/composition/README.md` | 主页装配规则 |
| `pages/home/features/README.md` | home React features 索引 |
| `pages/home/features/library/README.md` | 书架子目录 |
| `pages/reader/README.md` | 阅读器新/旧布局 |
| `js/reader/README.md` | 旧引擎边界与共享 ports |
