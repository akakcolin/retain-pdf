# 0011 typst 选型与 vendored 路径

## 背景

翻译 overlay 渲染选型理由已记录于 ADR 0002：纯 PyMuPDF 写字能力有限，复杂 markdown、公式与自动 fit 表达力不足，故用 Typst 生成 overlay 再与清理后的 PDF 背景合成。

当前实现依赖**外部 `typst` CLI 二进制**：`rendering_output/src/compile.rs` spawn `typst compile` 子进程（`TYPST_BIN` 环境变量 → `which typst` → 默认 `/snap/bin/typst`），版本 0.14.2（见 `backend/typst-win32/.crates.toml`）。该外部二进制在打包体积中占 38.7 MiB。

## 决策

- **保持外部二进制 0.14.2，本项目不 vendored**：继续以子进程方式调用 `typst` CLI，与 ADR 0008 的 native 子进程范式一致。
- **vendored `typst` crate 为追踪中的未来路径**：typst 本身是 Rust crate，理论上可直接依赖进 `rendering_output`，消除外部二进制依赖并缩减体积；作为第六律（依赖自主性）提分项，暂不实施，另行评估编译成本与锁定工作量。

## 后果

- 外部二进制是版本漂移与部署脆弱点（环境缺 `typst` 时 overlay 不可用），由 `TYPST_BIN` 显式配置缓解。
- 38.7 MiB 体积成本保留到 vendored 落地为止。
- 新增 typst 相关依赖时按 crate 边界归属（ADR 0007），不能随手进 `rendering_core`。

## 替代方案

- 直接依赖 `typst` crate 替代外部二进制：消除外部依赖、缩减体积，但引入大体积 Rust 依赖、编译时间上升，且需锁定 typst API 版本；列为未来路径。
- 自研排版引擎：开发与维护成本远超收益，已在 0002 否决。
