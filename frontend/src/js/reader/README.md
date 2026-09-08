# `src/js/reader` — 旧阅读引擎子目录（顶层已迁出）

顶层命令式 pdf.js 管线（`pdf-controller` / `pdf-renderer` / `view` / `favorites` 端口…）已整体迁入
**`pages/reader/legacy/`**（ADR 0009）。本目录只剩仍被 legacy 链路引用的子目录。

## 分层（与 `pages/reader/README` 对齐）

| 用途 | 模块 | 谁 import |
|------|------|-----------|
| **favorites** | `favorites/*` | `pages/reader/legacy/selection-favorites` |
| **annotations** | `annotations/view-model.ts` | `pages/reader/legacy/region-*` |
| **legacy AI** | `ai/*`（`ask-answerer`、`chat-history-store`…） | `pages/reader/legacy/ai`、`hooks/use-reader-boot` |
| **downloads** | `downloads/resolve.ts` | `pages/reader/legacy` 下载链路 |

## 已迁出

| 文件 | 去向 |
|------|------|
| 顶层 `*.ts`（引擎 / 共享 ports / view / markdown-*） | `pages/reader/legacy/`（ADR 0009 第 ⑤ 批） |
| `chrome-controller.ts`、`column-resizer.ts`、`mode-controller.ts`、`panel-collapse.ts` | `pages/reader/legacy/controllers/`（ADR 0009） |

## 已删除

| 文件 | 说明 |
|------|------|
| `ai/remote-answerer.ts` | 旧 `/reader/ai/chat` payload 应答器；现网 `ask-answerer` |

## 不要

- 不要在这里加新代码（新 UI → `pages/reader` 非 legacy；引擎 → 不再回迁）  
- 不要批量删除本目录子目录（legacy 仍依赖内部图）  
- 不要假设 `pages/reader/components/*` 扁平存在（已迁 `legacy/components/`）

## 主路径

```text
默认: pages/reader/ReaderAppReactPdf + hooks/ + pdf/ + annotations/ + components/react-pdf/
      js 依赖 → pages/reader/external.ts → pages/reader/legacy（共享 ports）
回退: pages/reader/legacy/*（引擎主力）+ 本目录子目录
地图: src/FEATURES.md
```
