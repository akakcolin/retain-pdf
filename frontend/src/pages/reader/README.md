# 阅读器目录（`pages/reader`）

默认引擎：**react-pdf**（`ReaderAppReactPdf`）。  
回退：`?engine=legacy`（`ReaderApp` 内分支 + `legacy/**` 命令式引擎，自 `js/reader` 迁入）。

## 三层边界

```text
┌─────────────────────────────────────────────────────────────┐
│  A. 新引擎 UI/逻辑（默认）                                    │
│     hooks/  pdf/  annotations/  components/react-pdf/         │
│     ReaderAppReactPdf.tsx                                     │
│     js 依赖 → 只经 ./external.ts                              │
└──────────────────────────┬──────────────────────────────────┘
                           │ 仅共享 ports
                           ▼
┌─────────────────────────────────────────────────────────────┐
│  B. 共享 ports（legacy/ 子集 + 少量 config/api）              │
│     data-port / config-port / resource-resolver /             │
│     pdf-document(resolve URL) / page-state(文案常量)          │
│     经 pages/reader/external.ts 出口                          │
└──────────────────────────┬──────────────────────────────────┘
                           │ legacy 可多用
                           ▼
┌─────────────────────────────────────────────────────────────┐
│  C. 旧命令式引擎（?engine=legacy）                            │
│     pages/reader/legacy/**（引擎主力）                          │
│     + js/reader/{annotations,ai,downloads}                    │
│     pdf-controller / pdf-renderer / regions…                  │
│     允许直接 import js/reader（不要塞进 external 冒充共享）    │
└─────────────────────────────────────────────────────────────┘
```

| 层 | 路径 | 新功能放哪 |
|----|------|------------|
| **A 新引擎** | `hooks/`、`pdf/`、`annotations/`、`components/react-pdf/` | 批注、缩放、对照、滚动锚点 |
| **B 共享** | `external.ts` → `legacy/{data-port,config-port,resource-resolver,…}` | 仅会话/资源/URL，不写 UI |
| **C legacy** | `legacy/**`（引擎主力）+ `js/reader/{annotations,ai,downloads}` | **不要**加新功能 |

## 布局

```text
pages/reader/
  entry.tsx / ReaderApp.tsx / ReaderAppReactPdf.tsx
  external.ts                # 新引擎对 js/* 唯一出口
  hooks/                     # 会话、缩放、锚点、批注、控制器
  pdf/                       # Document/Page、滚动、行高
  annotations/               # 新批注 + localStorage
  components/react-pdf/      # 新引擎 UI
  legacy/                    # 旧壳 UI + boot + 抽屉 AI + 引擎（自 js/reader 迁入）
    components/
    controllers/             # chrome/mode/column-resizer/panel-collapse（自 js/reader 迁入）
    hooks/use-reader-boot.ts
    state/
    ai/
```

## 入口

| 文件 | 作用 |
|------|------|
| `entry.tsx` | 挂载 `ReaderApp` |
| `ReaderApp.tsx` | `engine=legacy` → 旧壳，否则 `ReaderAppReactPdf` |
| `hooks/use-reader-react-controller.ts` | 新引擎逻辑总装 |
| `external.ts` | 新引擎共享 js 依赖 |

## 不要

- 新功能接到 `legacy/selection-favorites` / `legacy/favorites/*`  
- 把 `pdf-controller` 引进 `external.ts` 给新引擎用  
- 假设组件仍在扁平 `components/*`（旧 UI 已在 `legacy/components/`）

全站地图：`src/FEATURES.md` · 旧引擎细节：`src/js/reader/README.md`
