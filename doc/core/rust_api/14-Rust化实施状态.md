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
| 7R-7 (WIP) | 移除 page_specs/visual_profile 门禁 | 未提交 | 工作区改动 |
| B2 | save_optimized 字节压缩接 native（fitz 子集化 + native garbage=4/流压缩，失败回退） | 完成 | a74d9e84 |
| B2-Inc2 | pdf_structure_profile 表单 xobject/几何/文本 span 接 reader 原语 | 未提交 | 工作区改动 |
| D2 | rendering crates 差分 + 冒烟 parity 接入 CI 门禁（rendering-parity.yml） | 完成 | 工作区改动 |

## 子系统对照（已接线 / 休眠 / 未移植）

| Python 子系统 | 对应 Rust | 状态 |
|---|---|---|
| `source/background/stage.py`（build_clean_background_pdf） | `rendering_writer::background`（stage/redaction/detect/image_route/toc/formula_guard/vector_text） | **已接线**：生产走 Rust |
| `output/typst/`（emitter/compiler/block_renderer） | `rendering_output` | **已接线**：`compiler.py` 经 `_native.emit_typst_source`/`emit_typst_book_overlay_source` 生成源码；typst CLI 编译 + 叠加仍在 Python |
| `document/pdf_ops.py`（save_optimized_pdf 字节压缩） | `rendering_writer::save::save_optimized` | **已接线**：生产走 `source/_native.py::save_optimized`（fitz 子集化 + native garbage=4/流压缩，失败回退纯 fitz） |
| `legacy/pdf_compress` | `rendering_writer`（save/strip/compress/extract_pages/overlay） | **休眠**：bridge 已导出，Python 无调用点 |
| `layout/`、`analysis/route/` | `rendering_core` | **未接线**：无 Python 引用，仅差分测试 |
| `analysis/profile/`、`classifier.py` | `rendering_core::profile/classifier` | **未接线** |
| `source_cleanup/` | `rendering_core::source_cleanup` | **部分**：被 Rust background 内部用；Python planning 仍跑 |
| `pdf_structure_profile/`（sampler） | `rendering_reader`（read_page_cleanup_contexts/form_xobjects/geometry/text_spans 原语 + `_native.py` 装配） | **已接线**（工作区未提交）：生产走 native，回退 Python |
| fitz 读取层 | `rendering_reader`（mupdf-rs） | **部分**：pdf_structure_profile 生产使用 reader 原语；其余仍用 fitz |
| `analysis/document`、`visual_profile`、`policy`、`workflow`、`document` | — | **未移植** |

## 生产接线现状

- native 入口：`build_clean_background_pdf` → `source/background/_native.py`；最终保存 `save_optimized_pdf` → `source/_native.py::save_optimized`（fitz `subset_fonts()`+`tobytes()`，native garbage=4+流压缩，失败回退 fitz）。
- 新增 native 入口：`pdf_structure_profile` sampler → `_native.build_pdf_structure_profile`（reader 原语装配，回退 Python）；Typst 源码生成 → `output/typst/_native.py::emit_typst_source`/`emit_typst_book_overlay_source`。
- WIP 状态：auto / visual_cover / visual_cover_and_remove_text 全走 Rust；仅 `text_layer_only` / `text_redaction` 与 mock（instrumented）场景回退纯 Python。
- CI 门禁（D2）：`.github/workflows/rendering-parity.yml` 跑 rendering crates 差分 replay + 13 个 native 冒烟桥，锁 native==fitz 页 facts 与体积。
- 生产渲染流程其余环节（page_specs 构建、prepare、颜色适配、visual profile 加载、typst CLI 编译、叠加）仍是 Python + fitz；`save_fast_pdf` 仍走 fitz `doc.save`。

## Python 依赖评估

- **运行时硬依赖未降**：`rust_api` 仍 spawn `python3 run_render_only.py`，渲染管线整体在 Python 进程内执行。
- fitz/PyMuPDF 仍被约百个模块引用。
- 仅在 `build_clean_background_pdf` stage 内部，PDF 读写由 mupdf-rs 替换 fitz。
- native `.so` 已构建并装入 `.venv`（Python 3.14），开发环境 `NATIVE=True`。
- 桌面发布包 bundle 的是 `desktop/app/backend` 副本，未发现 maturin 构建脚本；发布版是否走 native 取决于部署时是否执行构建。

## 结论

移植覆盖度高、每阶段带 corpus + 差分门禁（现已接 CI），但生产接入度仍偏低：真正跑 Rust 的热点是"背景涂改/红批"stage、最终保存字节压缩、pdf_structure_profile 采样、Typst 源码生成。整体替代程度按代码量算中等，按运行时算偏低；Python 库的整体依赖尚未实质下降。
