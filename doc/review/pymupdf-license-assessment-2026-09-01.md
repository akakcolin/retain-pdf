# PyMuPDF 许可证兼容性核实

核实日期：2026-09-01
关联：十律评估 P0-5 / 第六律（依赖自主性）/ 第十律（时间抗性）
状态：**待决 — 需法务或维护者确认分发策略**

---

## 结论摘要

`retainpdf-core` 的 `pyproject.toml` 将 **PyMuPDF==1.26.5** 列为硬依赖，且 `backend/scripts` 下仍存在 7 处 `import fitz`。PyMuPDF 由 Artifex 以 **AGPL-3.0 + 商业双许可**发布。

项目自身为 **MIT**。两者在「源码分发」场景下**不兼容**：MIT 的宽松条款不能覆盖 AGPL-3.0 的强 copyleft 义务。本项目存在 .dmg / .deb / Setup.exe 桌面安装包与 Docker 镜像等**向第三方分发二进制**的渠道，会触发 AGPL 第 13 条（远程网络交互视为分发）及第 5 条（分发须整体在 AGPL 下开源）的强 copyleft 义务。

这不构成「代码不能写」，但构成**分发合规风险**，需在 50 年传承尺度（第十律）与商业化路径上被正视。

---

## 事实依据

1. **PyMuPDF 许可**：GitHub 官方仓库（`pymupdf/PyMuPDF`）明确：
   > "All repositories in this organisation are available under the GNU AGPL v3 for open-source use, and under a commercial licence for commercial use."
   PyMuPDF 1.26.x 沿用 AGPL-3.0 + Artifex 商业许可双轨（自 1.19 起）。

2. **项目许可**：`LICENSE` 为 MIT（`Copyright (c) 2026 RetainPDF contributors`）。

3. **依赖面**：
   - `backend/packages/retainpdf-core/pyproject.toml`：`dependencies = ["PyMuPDF==1.26.5", "requests==2.32.5", "urllib3==2.5.0"]`（版本已 pin 死）。
   - `backend/scripts` 中 `import fitz` 命中 7 处：`devtools/tools/add_retainpdf_footer_typst.py:9`、`devtools/tests/document_schema/test_local_command_ocr_driver.py:6`、`test_local_paddlex_wrapper_integration.py:10`、`ocr_provider/paddle_normalize.py:91`、`paddle_runner.py:39`、`entrypoints/*`。
   - **桌面端已剪除**：`doc/core/rust_api/14-Rust化实施状态.md`（C4·R）记 release 桌面包不再打包 pymupdf/pikepdf/lxml/PIL，仅服务端 `backend/scripts` 路径仍在用。

4. **使用性质**：fitz 主要用于 PDF 解析、repair（`upload.rs:110-124` 在 lopdf 失败时调 PyMuPDF 做 repair）、文本层操作。其中 repair 路径**无任何资源限制**，是已识别的攻击面。

---

## 冲突判定

| 分发形态 | 是否触发 AGPL 义务 | 说明 |
|---|---|---|
| 纯内部部署（不向外部分发二进制） | 否 | AGPL 义务在「分发」时触发 |
| Docker 镜像 / 桌面安装包对外发布 | **是** | 分发二进制，须整体 AGPL 开源或购商业许可 |
| 仅作为 API 服务运行、不分发二进制 | 视解释 | AGPL §13 对「网络交互」有强约束力，谨慎起见视为触发 |

结论：当前 `.github/release-*` 流程与 README「下载发布包」指引明确对外分发，故**处于 AGPL 强 copyleft 约束范围内**。MIT 项目在 AGPL 约束下分发，除非满足二者之一：
- 向接收方提供**整个衍生作品**在 AGPL-3.0 下的源码；或
- 持有 **Artifex 商业许可**。

---

## 缓解选项（按推荐排序）

### 选项 A：开源合规路径（零成本）
将受 AGPL 约束的分发渠道（Docker 镜像 + 桌面端）在 AGPL-3.0 下发布，并向外提供完整对应源码。这与 MIT 主许可可并存（AGPL 仅约束包含 AGPL 组件的衍生作品）。
- 优点：合规、零授权成本、符合开源精神。
- 代价：整个分发产物（含非 PyMuPDF 部分）须按 AGPL 提供源码；若项目有闭源/商业化意图则冲突。

### 选项 B：移除 PyMuPDF 硬依赖，换 Apache-2.0 替代（推荐长期方案）
- **渲染/读**：`pypdfium2`（`pdfium` 绑定，Apache-2.0）或项目的 `mupdf-sys` Rust 侧已覆盖读取，优先复用 Rust 路径，Python 侧仅作 fallback_reference。
- **写 / 合并**：`lopdf`（已在 `upload.rs` 使用，MIT/Apache-2.0）扩展覆盖 PyMuPDF 的写路径；`pikepdf`（已用，已剪除但逻辑可复用）。
- 现状印证可行性：doc14「终态判据 4」已将 fitz import 收编为 72 项 `FITZ_IMPORT_ALLOWLIST`，分类 `hard_boundary`/`fallback_reference`/`non_default_write`/`non_render_service`。这说明 PyMuPDF 的真实必要性已被收窄到少数硬边界（words-clip / get_texttrace / pixmap 采样），其余可走 Rust/native 或 lopdf。

### 选项 C：隔离为可替换边界 + 购买商业许可（折中）
保留 PyMuPDF 作为 `fallback_reference`（参考 doc14 的 parity_reference 模式），默认走 lopdf/pikepdf/Rust-native；仅在硬边界回退时触及。若仍需对外分发且不愿开源全部产物，向 Artifex 购买商业许可覆盖分发。
- 优点：保住现有能力与闭源/商业化空间。
- 代价：持续授权成本 + 隔离改造工作量。

---

## 建议行动

1. **立专项**（P0）：两周内由维护者或法务确认分发策略——选「AGPL 开源分发」还是「商业许可」还是「移除硬依赖」。
2. **中期（P1）**：按选项 B 把 PyMuPDF 从 `retainpdf-core` 的 `dependencies` 移出，改为可选 `extras`（如 `pip install retainpdf-core[pdfium]`），并复用现有 72 项 allowlist 把硬边界收敛到 `fallback_reference`。
3. **加固（P0）**：`upload.rs:110-124` 的 PyMuPDF repair 路径立即加资源限制（超时 + 内存上限 + 输出大小上限），与「复杂度预算」同一套防护。
4. **记录（P0-6）**：补 ADR 明确 PyMuPDF 依赖策略，使决策可传承（见 `doc/adr/0006-architecture-decisions-backlog.md`）。

---

## 待核实清单

- [ ] Artifex 商业许可报价与条款（若选 C）
- [ ] `pypdfium2` / `lopdf` 对当前 7 处 `import fitz` 的覆盖度（pinpoint 替换成本）
- [ ] 维护者是否接受 AGPL 分发渠道（若选 A）
