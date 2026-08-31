# Pipeline 目录说明

`scripts/runtime/pipeline/` 承载保留的 Python 阶段 worker。渲染已全量由 render_rs 接管（native-only），本目录不再含任何渲染模块。

## 模块

- `translation_stage.py`
  只负责翻译阶段。输入 `document.v1.json` 和输出目录，完成页范围裁剪、学术模式策略装配和全书翻译，输出逐页 translation payload 与 `translation-manifest.json`。
  由 `run_translate_only.py`（`entrypoints/translate_only_pipeline.py`）spawn。
- `__init__.py`
  仅包文档；旧 `is_editable_pdf` 等渲染辅助已随 Python 渲染树移除。

## 阶段边界

- OCR / Normalize → `document.v1.json`（document_schema）
- Translate → translation payload + `translation-manifest.json`（translation_stage）
- Render → render_rs 原生渲染（`rendering_orchestrator`），spec 协议 `render.stage.v1`

跨阶段共享的 stdout contract、summary、`pipeline_events.jsonl` 事件流与 JSON IO 在 `services/pipeline_shared/`。
