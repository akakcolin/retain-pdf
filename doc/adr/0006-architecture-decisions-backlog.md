# 0006 关键架构决策补遗（待补 ADR 索引）

## 背景

十律评估（2026-09-01）指出：24 万行代码仅对应 4 篇 ADR（183 行），**决策密度比约 1:1300**。大量影响多模块、长期维护成本、人员协作的结构性决策没有被记录。本文档把尚未写成独立 ADR、但影响重大的决策集中登记为「待补 ADR 队列」。

> 这不是一次性写完所有 ADR，而是先把「决策缺口」显性化——让团队知道哪些决定需要被追认，而不是让它们继续沉睡在 git 历史里。

## 已补实

2026-09-01 起，以下待补条目已各自独立成 ADR，并从本文档移除：

| 原待补条目 | 独立 ADR |
|---|---|
| 5 个 Rust crate 的拆分理由 | 0007-split-rendering-into-five-crates |
| pyo3 bridge → 子进程转向 | 0008-render-via-native-subprocess |
| 前端 `js/` 与 `pages/` 双轨处置 | 0009-frontend-dual-track-disposition |
| PyMuPDF 依赖策略 | 0010-pymupdf-dependency-strategy |
| typst 选型与 vendored 路径 | 0011-typst-selection-and-vendoring |

## 用法约定

- 本文件是「活索引」，每补实一篇独立 ADR，对应条目即从本文档划掉。
- 新增重大决策时，优先直接写独立 ADR；若暂时来不及，先在本索引登记。
