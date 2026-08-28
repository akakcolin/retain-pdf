# Rust 化实施状态

> 记录时间：2026-08-28。基于 git 历史 + 代码勘察（含未提交的工作区改动）。
> 进度远超 [13-Rust化实施计划.md](./13-Rust化实施计划.md)（该文档只写到 Phase 5）。

## 阶段进度

| Phase | 计划内容 | 实际状态 | 提交 |
|---|---|---|---|
| 0 | Tauri v2 替换 Electron | 完成 | 2849236a |
| 1 | 纯逻辑模块（layout/fit/payload/route） | 完成 | 15991635 |
| 2 | 差分测试（golden replay） | 完成 | 908d5604 |
| 3 | profile 数据形状层 | 完成 | 5aca7fdf |
| 4 | mupdf-rs reader + golden replay | 完成 | 0d57d9db |
| 5 | 5A 逻辑 + 5B 渲染链 + 5C 写路径 | 完成 | f1095aca |
| 6-8 | Typst 输出层 + source/background + pyo3 接入 | 完成 | 10988cca |
| 7R-1..6 | background/redaction 完整移植 | 完成 | cc1bfef0..0bfef7d6 |
| 7R-7 | 移除 page_specs/visual_profile 门禁 | 完成 | b88e5e7d |
| B2 | save_optimized 字节压缩接 native（fitz 子集化 + native garbage=4/流压缩，失败回退） | 完成 | a74d9e84 |
| B2-Inc2 | pdf_structure_profile 表单 xobject/几何/文本 span 接 reader 原语 | 完成 | e768013e |
| B2-Inc3 | reader PageSnapshot 补全（image_rects 聚合 + form_xobjects 原语） | 完成 | e768013e |
| B2-Inc4 | 分析簇接线：`build_render_document_analysis` bridge → `analysis/document/builder.py` | 完成 | e768013e |
| B2-Inc5 | 热路径清扫：整本 overlay 默认路径零 fitz 调用 | 完成 | e768013e |
| B2-Inc6 | save_optimized 子集化接 native：C shim `subset_and_clean`（隔离 context 跑 mupdf `pdf_subset_fonts` + garbage=4/流压缩，异常转 `mupdf_error_t**`），bridge 直调 `subset_and_save_optimized_pdf`，生产 `doc.tobytes()` 直传 native，fitz subset_fonts 移入回退 | 完成 | e2a7e9e7 |
| D2 | rendering crates 差分 + 冒烟 parity 接入 CI 门禁（rendering-parity.yml） | 完成 | 6f9e35d9..c2ad4e4a |
| C1 | Rust 编排器骨架 `rendering_orchestrator`（`render_rs --spec`）：prepare/page_specs 委托 `run_render_delegate.py` 产出 bundle，background→typst→save 全 native；`RETAINPDF_RENDER_ORCHESTRATOR_RS=1` test-gated 切换（未接生产配置）；`orchestrator_parity` 双二进制差分接 CI | 完成 | 092bc885 |
| C3 | 生产接线 + 桌面打包：rust_api 按 mode 默认路由 typst/typst_visual→`render_rs`（auto/overlay/dual 仍走 python3），env 逃逸阀 `RETAINPDF_RENDER_ORCHESTRATOR_OFF/RS`；spawn 时给 delegate 透传 `RETAIN_PDF_PYTHON_BIN`/`RETAIN_PDF_RENDER_DELEGATE_SCRIPT`；`prepare-app.mjs` 打包 `bin/render_rs` + `build:render-rs` + release-desktop.yml 每平台构建 | 完成 | ccd4f5f3 |

## 子系统对照（已接线 / 休眠 / 未移植）

| Python 子系统 | 对应 Rust | 状态 |
|---|---|---|
| `source/background/stage.py`（build_clean_background_pdf） | `rendering_writer::background`（stage/redaction/detect/image_route/toc/formula_guard/vector_text） | **已接线**：生产走 Rust |
| `output/typst/`（emitter/compiler/block_renderer） | `rendering_output` | **已接线**：`compiler.py` 经 `_native.emit_typst_source`/`emit_typst_book_overlay_source` 生成源码；typst CLI 编译 + 叠加仍在 Python |
| `document/pdf_ops.py`（save_optimized_pdf 子集化+字节压缩） | `rendering_writer::save::subset_and_clean`（C shim `c/save_clean.c`：mupdf `pdf_subset_fonts` + garbage=4/流压缩） | **已接线**：生产走 `source/_native.py::save_optimized` 全 native（含字体子集化），失败回退纯 fitz |
| `legacy/pdf_compress` | `rendering_writer`（save/strip/compress/extract_pages/overlay） | **休眠**：bridge 已导出，Python 无调用点 |
| `layout/`、`analysis/route/` | `rendering_core` | **未接线**：无 Python 引用，仅差分测试 |
| `analysis/profile/`、`classifier.py` | `rendering_core::profile/classifier` | **未接线** |
| `source_cleanup/` | `rendering_core::source_cleanup` | **部分**：被 Rust background 内部用；Python planning 仍跑 |
| `pdf_structure_profile/`（sampler） | `rendering_reader`（read_page_cleanup_contexts/form_xobjects/geometry/text_spans 原语 + `_native.py` 装配） | **已接线**：生产走 native，回退 Python |
| `analysis/document`（builder） | `rendering_bridge::build_render_document_analysis`（reader 原语装配） | **已接线**：生产走 `analysis/_native.py`，回退 Python |
| fitz 读取层 | `rendering_reader`（mupdf-rs） | **部分**：pdf_structure_profile / analysis 生产使用 reader 原语；其余仍用 fitz |
| `visual_profile`、`policy`、`workflow`、`document` | — | **未移植** |

## 生产接线现状

- **C1（骨架 + 委托）**：`backend/rendering_orchestrator/` 提供 `render_rs --spec <spec>`，镜像 `render_only.py` 编排。`delegate.rs` spawn `run_render_delegate.py` 产出 `render.bundle.v1`（prepare + page_specs + visual profile fill map）；`stages/background.rs`→`typst.rs`→`save.rs` 全 native 直调（`build_clean_background_pdf` + `compile_typst_source` + `copy_toc`/`save_optimized`）。仅支持 `typst`/`typst_visual` 背景模式，其余 mode 明确拒绝。差分门禁 `rendering_writer/differential/orchestrator_parity.py` 双二进制 subprocess，已接 `rendering-parity.yml` bridge loop。本地验证：fixture 双 mode 页 facts 全等、像素逐字节相同、体积比 0.94（Rust 更小）。
- **C3（生产接线 + 桌面打包）**：`rust_api` `render_only_command`（`entrypoints.rs`）按 `render.render_mode` 默认路由——typst/typst_visual 发 `render_rs --spec`，auto/overlay/dual 仍 spawn `python3 run_render_only.py`；逃逸阀 `RETAINPDF_RENDER_ORCHESTRATOR_RS=1` 强制 native、`RETAINPDF_RENDER_ORCHESTRATOR_OFF=1` 强制 Python。`render_rs_bin`/`render_rs_delegate_script` 进 `RuntimePathsConfig`（`paths.rs::resolve_render_rs_bin`：env `RETAIN_PDF_RENDER_RS_BIN` → 仓库 target → `app/backend/bin/render_rs` → PATH）；`spawn_worker_process` 识别 render_rs 时透传 `RETAIN_PDF_PYTHON_BIN`+`RETAIN_PDF_RENDER_DELEGATE_SCRIPT`（delegate 用 desktop bundled python）；worker 输出契约闭合：`process_contract.rs` 识别 `[render_rs, --spec, ...]` 为 Render contract，render_rs 写 `artifacts/pipeline_summary.json` 并打印 `summary:` 标签（`output pdf`/`summary` 两 artifact 校验齐全）。桌面：`prepare-app.mjs::resolveRenderRsBinary` 拷贝 `bin/render_rs` + manifest 字段，`package.json build:render-rs`，`release-desktop.yml` 每平台构建（linux apt clang/libclang/pkg-config、mac brew pkg-config、win LLVM）。
- native 入口：`build_clean_background_pdf` → `source/background/_native.py`；最终保存 `save_optimized_pdf` → `source/_native.py::save_optimized` 全 native（`doc.tobytes()` → bridge `subset_and_save_optimized_pdf` → `save::subset_and_clean`，C shim 在隔离 mupdf context 里跑 `pdf_subset_fonts` + garbage=4/流压缩，异常经 `mupdf_error_t**` 返回而非 exit；失败回退纯 fitz 子集化+save）。
- 新增 native 入口：`pdf_structure_profile` sampler → `_native.build_pdf_structure_profile`；`analysis/document/builder` → `_native.build_render_document_analysis`；Typst 源码生成 → `output/typst/_native.py::emit_typst_source`/`emit_typst_book_overlay_source`（均回退 Python）。
- 接线现状（7R-7 后）：auto / visual_cover / visual_cover_and_remove_text 全走 Rust；仅 `text_layer_only` / `text_redaction` 与 mock（instrumented）场景回退纯 Python。
- CI 门禁（D2）：`.github/workflows/rendering-parity.yml` 跑 rendering crates 差分 replay（writer+reader，含 form_xobjects）+ 17 个 native 冒烟桥（含 B3 整本 E2E + 像素 parity，CI 装 typst 0.14.2 + cmarker/mitex），锁 native==fitz 页 facts、体积与像素。
- 生产渲染流程其余环节：整本 overlay 默认路径（`build_book_typst_pdf` → pikepdf 合并）已零 fitz 调用（B2-Inc5，用调用计数探针验证）；`save_fast_pdf` 仍走 fitz `doc.save`；颜色适配（`apply_adaptive_overlay_colors_batch`）PDF 访问已 native（`sample_page_color_fills`/`extract_page_span_dicts`/`sample_title_visual_colors`），仅剩纯几何 `fitz.Rect` 强转；单页/dual-book 路径仍传 fitz doc。

## Python 依赖评估

- **运行时硬依赖部分下降**：typst/typst_visual 渲染已由 `render_rs` 生产接管（C3），background→typst→save 段不再 spawn python；但 prepare/page_specs 仍委托 `run_render_delegate.py`（Python 子进程），auto/overlay/dual 仍走 `python3 run_render_only.py`，渲染管线整体仍依赖 Python 运行时。
- fitz/PyMuPDF 仍被约百个模块引用。
- 仅在 `build_clean_background_pdf` stage 内部，PDF 读写由 mupdf-rs 替换 fitz。
- native `.so` 已构建并装入 `.venv`（Python 3.14），开发环境 `NATIVE=True`。
- 桌面发布版**不走 native**：`desktop/scripts/prepare-app.mjs` 只拷贝 `backend/scripts` Python 源码 + `rust_api` 二进制，无 maturin 构建步骤；`release-desktop.yml` 同样无 maturin/`rendering_bridge` 引用。`import rendering_bridge` 在发布包内必然 ImportError，所有 `_native.py` shim 回落纯 Python（`NATIVE=False`）。maturin 仅出现在 `backend/rendering_bridge/pyproject.toml` 与 parity CI（`maturin develop` 装进测试 venv）。发布版接入 native 需在 `prepare-app.mjs` 增加构建 + 拷贝 `.so` 步骤。

## 分歧台账（reader 原语 vs fitz）

- **text_traces 为空**：mupdf 无 `get_texttrace` 等价物（spans/drawings 无 opacity/type-3 信号）→ native `hidden_text=False`；影响仅限「隐藏文本且 <20 词」页，分类 parity 用 corpus kind 断言兜底。
- **image_rects xref tie**：mupdf 无 xref 关联 bbox，`page_snapshot` 取首个资源 xref，image_rects 聚合全部 placement rects（≤0.01pt 覆盖精确关联）。
- **drawing rect 分歧**：已有 golden 差分记录。
- **native 页索引推导边界**（B2-Inc5 doc=None 分支）：`layout._native.read_source_page_sizes` 会跳过不可读页，与 fitz `0 <= idx < len(doc)` 仅在「范围内但不可读」页有边角分歧；`overlay_pdf_size_mismatches` native 分支假设 overlay 页数==specs 数（fitz 用 `len(overlay_doc)` 实测），编译按 spec 生成、页数恒等，仅防御性检测有差异。
- **颜色适配**：batch 路径（`apply_adaptive_overlay_colors_batch`）PDF 访问已 native（3 个 source native 原语：`sample_page_color_fills`/`extract_page_span_dicts`/`sample_title_visual_colors`），共享决策树 `_apply_adaptive_overlay_colors_with_data` 纯 Python；仅参考实现（`PageTextColorSampler.build`/`title_text_color_from_visual_components`）与纯几何 `fitz.Rect` 强转仍走 fitz。
- **限定为 fallback/非默认策略**：`workflow/direct_overlay.py`、`overlay_ops`/`source_page_overlay`（单页/dual-book）、`fill.py`、`source_cleanup/pdf/document.py` —— NATIVE=False 时仍走 fitz，属参考实现。

## 结论

移植覆盖度高、每阶段带 corpus + 差分门禁（现已接 CI），但生产接入度仍偏低：真正跑 Rust 的热点是"背景涂改/红批"stage、最终保存（子集化+字节压缩，全 native）、pdf_structure_profile 采样、Typst 源码生成。B2 已把整本 overlay 默认路径拉成零 fitz（CI 门禁断言）；颜色适配 batch 已 native，仅参考实现与纯几何 `fitz.Rect` 残留；保存字节压缩已补上 mupdf 侧 `subset_fonts`（C shim 隔离 context + 异常转 `mupdf_error_t**`），不再需要 fitz 子集化，失败才回退纯 fitz。整体替代程度按代码量算中等，按运行时算偏低；Python 库的整体依赖尚未实质下降。
