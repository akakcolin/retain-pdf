# 任务详情页（`pages/detail`）

独立 SPA：`detail.html` → `entry.tsx` → `DetailApp`。

## 布局

```text
pages/detail/
  entry.tsx / DetailApp.tsx
  external.ts              # 对 src/js/* 的唯一出口
  components/              # UI（Header / Artifacts / Events…）
  legacy/                  # 命令式 overview / markdown / resume（自 js/job-detail 迁入）
```

## 规则

| 层 | 规则 |
|----|------|
| `DetailApp` / `components/**` | **禁止**直接 `import … from "../../js/…"` |
| `external.ts` | 唯一允许 import `src/js/*` 的文件；缺符号只改这里 |
| `legacy/*` | 命令式 overview / markdown / resume 逻辑（自 `js/job-detail` 迁入；可直接 import `src/js/*`） |

门禁：`tests/architecture-boundaries.test.mjs`  
（`detail page must not import src/js/* directly`）

## 状态策略（摘要）

- 文案 / 链接：React state（`texts` / `links`），由 `legacy/` 回调写入  
- 产物清单、失败调试、Markdown 图片网格：命令式 innerHTML 孤岛（见各组件注释）  
- 模态 / 下载 toast：React（Radix Dialog + DownloadToastHost）
