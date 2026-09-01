# Python Pipeline Directory Map

这份文档只回答一个问题：

**现在要改 `backend/scripts`，应该先进哪个目录。**

## 主链现状

Rust API 创建 job，生成 `specs/*.spec.json` 并依次启动 worker。阶段执行：

- normalize：全量走 native `render_rs --normalize-ocr`（单一实现，Python normalize 引擎已退役）
- translate：`entrypoints/run_translate_only.py -> services/translation/entrypoints/translate_only_pipeline.py -> runtime/pipeline/translation_stage.py -> services/translation/*`
- render：native `render_rs --spec render.spec.json`，Python 侧无渲染实现

## 最常见入口

- 改人工执行入口：`entrypoints/`（console.py、diagnose_failure_with_ai.py、run_translate_only.py、translate_book.py、validate_document_schema.py）
- 改翻译阶段编排：`runtime/pipeline/`（仅剩 `translation_stage.py`）
- 改 OCR 归一化：native `rendering_orchestrator/src/normalize/`（`render_rs --normalize-ocr`），Python 侧无实现
- 改统一 OCR 契约：`services/document_schema/`（`consumer_reader.py`、`reporting.py`）
- 改翻译主链：`services/translation/`
- 渲染：Rust `rendering_orchestrator/`（render_rs），不在本目录

## 快速判断

- “这是入口参数或 worker 启动方式变化吗？” 先看 `entrypoints/`
- “这是翻译阶段顺序或输入输出协议变化吗？” 先看 `runtime/pipeline/`
- “这是 raw OCR 适配或 schema 变化吗？” 先看 `services/document_schema/`
- “这是 OCR 归一化或契约问题吗？” 先看 native `rendering_orchestrator/src/normalize/` 或 `services/document_schema/`
- “这是翻译结果不对吗？” 先看 `services/translation/`
- “这是 PDF 渲染不对吗？” 改 Rust `rendering_orchestrator/` 或 `rendering_writer/`

## 边界红线

- `services/translation/` 不消费 provider raw 结构，只消费 `document.v1.json` 稳定交接物。
- `entrypoints/` 只连稳定入口，不绕过 `*_pipeline.py` 或 `translation_stage.py` 直连深层实现。
- Python 侧不 import 任何 `services.rendering` 模块（已整体退役）。
