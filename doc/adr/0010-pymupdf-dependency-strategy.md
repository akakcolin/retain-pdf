# 0010 PyMuPDF 依赖策略

## 背景

`retainpdf-core` 将 `PyMuPDF==1.26.5` 列为硬依赖（`pyproject.toml` `dependencies`），`backend/scripts` 下 7 处 `import fitz`。PyMuPDF 由 Artifex 以 **AGPL-3.0 + 商业双许可**发布，项目自身为 **MIT**——「源码分发」场景下两者不兼容：Docker 镜像与桌面安装包等对外分发渠道会触发 AGPL 第 5/13 条强 copyleft 义务。桌面端已剪除（release 桌面包不再打包 pymupdf/pikepdf/lxml/PIL），服务端路径仍在用。详见 `doc/review/pymupdf-license-assessment-2026-09-01.md`。

## 决策

- **默认走 non-fitz 路径**：读取复用 Rust/mupdf-sys 与 `lopdf`；fitz 收敛为 `fallback_reference`，由 `FITZ_IMPORT_ALLOWLIST`（72 项，分类 `hard_boundary`/`fallback_reference`/`non_default_write`/`non_render_service`）限定边界。
- **中期移除硬依赖**：把 PyMuPDF 从 `retainpdf-core` `dependencies` 移出为可选 `extras`，用 Apache-2.0 替代（`pypdfium2` / 复用 Rust native / `lopdf` 覆盖写路径），长期目标是服务端路径也零 fitz。
- **修复路径加固（P0）**：`upload.rs` 的 fitz repair 回退加资源限制（超时 + 内存上限 + 输出大小上限），当前无任何限制，是已识别攻击面。
- **法律定论待法务**：分发渠道的最终合规口径（AGPL 整体开源 vs 商业许可）由法务/维护者确认后再定案。

## 后果

- 许可证风险从「常驻硬依赖」降为「可替换边界」，为 50 年传承尺度（第十律）与商业化路径留出空间。
- `FITZ_IMPORT_ALLOWLIST` 成为常驻门禁，新 fitz import 须先判定归属分类。
- 移除硬依赖前，服务端仍带 AGPL 组件，对外分发须按评估文档的选项 A/B/C 择一执行。

## 替代方案

- 选项 A：受 AGPL 约束的分发渠道整体按 AGPL 开源。合规零成本，但与闭源/商业化意图冲突。
- 选项 C：保留 PyMuPDF 为 `fallback_reference` 并向 Artifex 购买商业许可覆盖分发。保住能力与闭源空间，但持续授权成本 + 隔离改造。
