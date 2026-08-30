# Rust 化改进计划

> 记录时间：2026-08-27。基于 [14-Rust化实施状态.md](./14-Rust化实施状态.md) 的架构诊断提出后续改进。

## 现状架构诊断

当前迁移模式是"Python 宿主 + pyo3 被调"：渲染管线仍由 Python 进程编排（`run_render_only.py` → `workflow/render_only.py` → `book_renderer.py`），Rust 以 `rendering_bridge`（pyo3）被 Python 内嵌调用，靠 `_native.py` shim 运行时路由。

关键判断：**pyo3 bridge 是迁移脚手架，不是终态**。只要"Python 调 Rust"方向不变，Python 永远是宿主，运行时依赖删不掉。

结构性问题：

- **休眠移植 = 双实现漂移风险**。5 个 crate 只有 `rendering_writer::background` 接线；`rendering_output`（Typst）、写路径原语、`rendering_core` 都是"已移植 + 差分通过 + 未接线"。双实现并存时差分测试是唯一防线，不跑就漂。
- **fitz 是最大耦合点**。约百个 Python 模块引用 fitz；`rendering_reader`（mupdf-rs）仅 230 行，基本未接线。破不掉 fitz，Python 就断不了。
- **边界是手写 JSON**。Python↔Rust 靠 `page_specs_json`/`fill_map_json` 传递，两边手工维护 key 结构；`render_page_spec_to_bridge` 固定 key 序只是权宜之计。

## 目标架构（端态）

```
rust_api（服务壳，编排作业）
   └─ render orchestrator（Rust 编排渲染流程）   ← 新建，镜像 render_only.py
        ├─ rendering_reader   （mupdf-rs，唯一 PDF IO 入口）
        ├─ rendering_core     （纯逻辑：layout/profile/route/source_cleanup）
        ├─ rendering_output   （Typst 源码生成 + 编译）
        └─ rendering_writer   （写 PDF：save/overlay/background/redaction）
```

原则：**Rust 是编排者，Python 从"宿主"降级为可选**（仅 AI 翻译等外部服务需要时以子进程/接口调用）。分层单向依赖：core ← reader/writer/output ← orchestrator ← rust_api。

## 分阶段改进

### 阶段 A：收编休眠代码（先接线，风险最低）
架构意图：消除"已移植但闲置"的漂移源，双实现变单实现。

- **A1 接线 Typst 输出层**。`output/typst/_native.py` 已写好但无人 import；`book_renderer` 的 emit + compile 路径路由到 `_native.emit_typst_source`/`compile_typst_source`。差分（`emit_diff.rs`/`book_diff.rs`）已就绪。
- **A2 接线写路径原语**。`rendering_bridge` 已导出 `save_optimized`/`compress_images`/`extract_pages`/`overlay_page`/`strip_*` 但无调用点；把 `document_ops.save_optimized_pdf`、`legacy/pdf_compress`、叠加逻辑换成调 bridge。
- **A3 接线即收编**。每接一个子系统就删对应 Python 双实现（不要复制 `_build_clean_background_pdf_python` 这个保留模式）。
- 验收：native 命中率从 1/5 子系统提到 3/5，端到端输出与纯 Python 一致。

### 阶段 B：PDF IO 解耦（破 fitz 依赖，最大工作量）
架构意图：PDF 读写收敛到单一抽象，fitz 从生产路径消失。

- **B1 扩展 `rendering_reader`**：定义 `PdfDocument` trait（open/load_page/geometry/text/page 渲染），mupdf-rs 实现；其它 crate 只依赖 trait。
- **B2 替换 fitz 调用点**：page_specs 构建、prepare/首行缩进、颜色适配（像素采样）、visual profile 加载、source_cleanup planning 逐个换到 trait 实现。
- **B3 PDF IO 差分**：同一 PDF 两种实现的几何与像素比对（沿用 7R 的 corpus + diff 模式）。
- 验收：生产渲染路径不再 `import fitz`。

### 阶段 C：反转控制方向（关键转折）
架构意图：从"Python 调 Rust"反转为"Rust 编排"，是删掉 Python 运行时的分水岭。

- **C1 新建 render orchestrator（Rust）**：镜像 `render_only.py` 流程编排，串起 prepare → page_specs → background → typst → save。
- **C2 端到端差分**：Rust 编排输出 vs Python 编排输出整本对比（`stage_diff.rs` 已覆盖 background 单 stage，扩展到全流程）。
- **C3 渐进迁移**：orchestrator 按 stage 逐段接管，Python 保留 AI 翻译/外部 provider 桥。
- 验收：rust_api 跑通渲染不再 spawn `python3`（AI 服务除外）。
- 状态（2026-08-29）：C1·Ext 已把 overlay/dual 编排移入 `render_rs`（delegate 产出 `overlay_page_specs`，`stages/overlay.rs`/`stages/dual.rs` 全 native），C3·Ext 已把 typst/typst_visual/overlay/dual/auto 全部默认路由到 `render_rs`；`orchestrator_parity` 四模式全等、`smoke_zero_fitz` delegate 四模式零 fitz。

### 阶段 D：架构收敛与治理
架构意图：一次性验证升级为常驻防线，防止回潮。

- **D1 双实现清零**：每子系统"接线 = 删 Python 版"，杜绝"差分过但生产没走"（当前 Typst 正是这个坑）。
- **D2 差分转 CI 门禁**：`differential/*.py` + `tests/*_diff.rs` 纳入 CI 常驻运行；golden corpus 版本化。
- **D3 边界契约化**：JSON 边界 schema 化（serde ↔ Python TypedDict）+ 契约测试，替代手写 key 序。
- **D4 统一路由与可观测**：`_native_eligible` 硬编码条件换成 feature flag + 回退原因日志 + native 命中率指标。
- **D5 目标度量**：桌面体积、Python 进程是否仍在、渲染耗时、native 命中率基线。

## 风险与注意事项

- 不要继续加固 pyo3 bridge 到终态；方向校准比再移植几个模块更重要。
- 不要无限期保留双实现；养两套代码的维护成本会吃掉收益。
- PDF IO 是最大风险点，需要真实样本多样性 + 像素级容差（对应原计划 Phase 5 判定）。
- 接线顺序：先 A（低风险收编）再 B（破 fitz）再 C（反转编排），C 依赖 B 的 IO 抽象。

## 推荐第一步

阶段 A1 接线 Typst 输出层——shim、差分、桥接函数都现成，能把第二个子系统接到 Rust，同时验证"接线 + 删 Python 版"收编流程，为后续定调。

## 阶段 B 执行批次（2026-08-28 更新）

> 依据 14 号状态文档现状，把「破 fitz + 收编休眠代码」拆为 8 个可接线批次。B-* 是阶段 B（PDF IO 解耦）的子批次，沿用 `_native.py` shim + 差分/parity 验证模式。

### 批次清单

| 批次 | 内容 | 风险 | 关键点 |
|---|---|---|---|
| B-A | `render_mode.py` + `analysis/profile`+`classifier` 收编 | 低 | 首个批次验证接线流程；仅需 bridge 薄函数，无新 reader 原语 |
| B-B | `source_cleanup/planning` 收尾 + 删 reference | 低-中 | 生产已 native，清 `fitz.Rect` + planner fitz 引用 |
| B-C | `source/cleanup` 文本读取补全 | 中 | `text_intrusion`/`margin_text` 接 spans/blocks 原语；words-clip 留 reference |
| B-D | `visual_profile`/`color_adapt` 纯几何清理 | 低 | 已 native，只剩 Rect 强转 |
| B-E | `source` 顶层散点收编 | 中 | `document_ops`/`compression`/`vector_profile` |
| B-F | `output/typst` 单页/dual-book `show_pdf_page` | 高 | 需新 writer 原语（display-list overlay + dual-page 组装） |
| B-G | 红批写入原语（破 `text_redaction`/`text_layer_only` 回退） | 高 | 泛化 background 红批+cover 为独立 bridge 原语 |
| B-H | 纯几何 Rect 强转清扫 | 低面广 | 40+ 文件机械替换，最后做避免 churn |

### 顺序

`B-A` → `B-B ∥ B-D` → `B-C ∥ B-E` → `B-F ∥ B-G`（建议先 G 后 F）→ `B-H`。读取侧批次（B-A..E）无硬依赖可并行，writer 侧两块（B-F/G）风险高宜串行。

### 终态判据

1. 默认 typst/typst_visual/overlay book + auto render-mode 采样：fitz 调用计数探针断言为 0。
2. `text_redaction`/`text_layer_only` 走 native（B-G 后），`_native_eligible` 不再回退这两个策略。
3. 单页/dual-book `show_pdf_page`：native 化（B-F）或明确列入 allowlist 标注非默认策略。
4. fitz import 收缩为可枚举 allowlist：不可复现分歧 / 回退网 / 非默认 write / devtools，每项带 reason。
5. 差分 smoke（新增 B-A/B-C/B-E/B-F/B-G）全部接入 `rendering-parity.yml` 常驻。

### 硬边界（勿强行 native）

`get_texttrace`、`get_text("words", clip)`、per-drawing zigzag rect、placement→xref 关联——mupdf-rs 无等价物，强行复刻会产生脆弱 quirk 代码（14 号文档 `collect_page_drawing_rects` 教训），一律留 reference 入 allowlist。

### 最大风险

writer 原语语义等价性（B-F/B-G）：现有 `overlay_page` 是 pikepdf 风格 Form XObject，非 fitz `show_pdf_page` 等价物；红批 `apply_redactions`（text-remove/graphics-none/image-none）与 cover fill 采样无独立原语。必须像素级 parity + 真实样本 corpus，不能以单 fixture 全等验收。

## 阶段 C 续：非 AI worker 子进程清零（C5 批次）

> 阶段 C 判据「渲染不再 spawn python3」已在 C3-N11f 达成；C5 把同一接管原则推广到其余非 AI 的 Python worker 子进程（`run_*.py` entrypoint，非 AI 翻译/OCR provider 服务），逐个 `render_rs` 子命令化。每个批次沿用 `_native.py` shim + 差分/parity 验证模式。

| 批次 | 内容 | 关键点 |
|---|---|---|
| C5-N1 | `run_extract_text_layer.py`（skip-OCR 文本层提取）→ `render_rs --extract-text-layer --spec` | reader `page_rect`/`page_text_blocks` → `generic_flat_ocr`；`extract_text_layer_command` 默认路由 native，`RETAINPDF_RENDER_ORCHESTRATOR_OFF=1` 回退；`[render_rs,--extract-text-layer,...]` 落 `WorkerContract::Unknown`（同 python 路径，无新契约臂）；差分 `extract_text_layer_parity.py`（完成，2aba4919） |
| C5-N2a | `run_normalize_ocr.py`（mineru 默认 provider）→ `render_rs --normalize-ocr --spec` | `normalize/` 11 模块移植 `normalize_pipeline.py::main`（adapter_mineru → defaults → contract → validate → rescale → paddle_rebuild → refresh → save）；`normalize_ocr_command` 增 provider 参数，mineru 默认路由 native、`RETAINPDF_RENDER_ORCHESTRATOR_OFF=1` 回退，mineru_content_list_v2/paddle 留 python（C5-N2b/c）；差分 `normalize_ocr_parity.py`（完成，091f1cff） |
| C5-N2b | `run_normalize_ocr.py` 的 mineru_content_list_v2 provider → `render_rs --normalize-ocr --spec` 双 provider | `normalize/adapter_content_list_v2.rs` 移植 `mineru_content_list_v2_adapter.py` + `provider_adapters/common` 共享 builder（block/page/document/normalize），text_flow 复用 `rendering_core::text_flow`（C3-N5）；`should_route_normalize_native` 扩 `matches!(mineru\|mineru_content_list_v2)`，paddle 留 python（C5-N2c）；`normalize_ocr_parity.py` 重构 provider 参数化，同一 harness 双 provider 全绿（完成，9c4dcb46） |
| C5-N2c | `run_normalize_ocr.py` 的 paddle provider → `render_rs --normalize-ocr --spec` 三 provider | `normalize/paddle/` 13 模块移植 `provider_adapters/paddle/*.py`（block_reader/block_labels/body_repair/column_signals/content_extract/continuation/page_reader/page_trace/relations/rich_content/trace）；共享 builder 提升 `common.rs`，content_list_v2 adapter 改复用；`should_route_normalize_native` 扩 `matches!(mineru\|mineru_content_list_v2\|paddle)`；同一 harness 三 provider 全绿（完成，964e9636） |
| C5-N2d | `run_normalize_ocr.py` 的 generic_flat_ocr provider → `render_rs --normalize-ocr --spec` 四 provider | `normalize/generic_flat_ocr.rs` 移植 `provider_adapters/generic_flat_ocr_adapter.py`（flat passthrough：block 重索引 + derived 角色/翻译策略 + front-matter author-gap 判定，无几何改写）；`should_route_normalize_native` 扩 `matches!(mineru\|mineru_content_list_v2\|paddle\|generic_flat_ocr)`；rescale `if metadata:` 真值门控对齐 Python 参考；`normalize_ocr_parity.py` 同一 harness 四 provider 全绿（完成，74c4e954） |
| C5-N2.. | 其余非 AI worker（外部 provider 桥除外）按依赖序逐个接管 | 每个 batch 对齐：spec serde 镜像 + 子命令 dispatch + rust_api 默认路由 + 差分 smoke + CI |

### 终态判据（阶段 C 续）

- skip-OCR 作业的文本层提取不再 spawn python3（`render_rs --extract-text-layer` 进程内完成）。
- `_routing.ALLOWLIST` 不新增 routed fn（native 路径是 Rust 而非 Python shim）；`smoke_d1_mandate` 常绿。
- 逃逸阀不变：`RETAINPDF_RENDER_ORCHESTRATOR_OFF=1` 逐个 worker 回退 python reference。
