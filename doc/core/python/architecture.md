# Python 后端架构边界

这份文档描述 `backend/scripts` 的长期维护边界。目标不是减少文件数量，而是保证代码增长后仍能定位、测试和修改。

## 总体分层

```text
entrypoints
  -> runtime/pipeline
    -> services/*
      -> foundation
```

职责：

- `entrypoints/`
  命令行入口，只解析参数并调用稳定服务入口。
- `runtime/pipeline/`
  阶段编排层，负责 OCR、翻译、渲染的顺序、阶段 spec、事件和产物交接。
- `services/`
  具体能力层，包含 OCR provider、document schema、translation、rendering 等业务能力。
- `foundation/`
  配置、共享基础工具和跨服务底层能力。

## 稳定子系统

```text
services/document_schema
services/mineru
services/translation
services/pipeline_shared
runtime/pipeline
```

> 渲染与归一化已 native `render_rs` 接管；`services/rendering`、`services/ocr_provider`、`services/document_schema` 的 adapter/normalize 树均已退役删除。

核心规则：

- OCR raw payload 由 native `render_rs --normalize-ocr` 归一化为 `document.v1.json`，Python 侧无 adapter / normalize 实现。
- 翻译主链只消费 `document.v1` 和 translation stage spec。
- 渲染主链只消费源 PDF、translation manifest、逐页翻译 payload 和 render stage spec。
- `runtime/pipeline` 只负责编排，不吸收 provider、LLM、Typst、redaction 的细节。

## 渲染层边界

渲染由 native `render_rs` 全量接管（prepare / page_specs / typst / overlay / save 全 native），
Python 侧不存在渲染树（`backend/scripts/services/rendering` 已整体退役删除）。

渲染主链只消费：

- 源 PDF
- `translation-manifest.json` 与逐页翻译 payload
- `render.stage.v1` spec

渲染库代码：

- `backend/rendering_core`：layout / typography / payload 逻辑
- `backend/rendering_output`：Typst source 生成
- `backend/rendering_orchestrator`：stage 编排（`render_rs --spec`）

## 翻译层边界

```text
services/translation/workflow
  -> context
  -> policy
  -> memory
  -> llm
  -> payload
```

职责：

- `workflow/`
  翻译请求入口和执行门面。
- `context/`
  domain guidance、memory guidance 组合。
- `policy/`
  是否翻译、如何处理保留排版等策略。
- `memory/`
  job 级术语和保留排版记忆。
- `llm/`
  provider 调用、重试、校验和 fallback。
- `payload/`
  翻译产物协议。

禁止方向：

- `runtime/pipeline/translation_stage.py` 不直接 import `policy`、`llm`、`diagnostics` 内部细节。
- `translation` 不 import 任何渲染模块（渲染已 native `render_rs`，Python 侧无渲染树）。
- `translation` 不消费 provider raw JSON。

## OCR 边界

```text
native render_rs --normalize-ocr
  -> document.v1.json
  -> document_schema / translation
```

禁止方向：

- `document_schema` / `translation` 不消费 provider raw JSON（归一化由 native `render_rs --normalize-ocr` 完成，Python 侧无 `ocr_provider` / normalize 实现）。
- `translation` 不 import `services.mineru`（native `render_rs` 天然不 import Python 模块）。

## 公共入口

上层优先只调用这些入口：

- `render_rs --normalize-ocr`（native normalize，单一实现）
- `render_rs --spec <render.stage.v1>`（native 渲染，无 Python 渲染入口）
- `services.translation.public`（翻译公共门面）
- `runtime/pipeline/translation_stage.py`（翻译阶段编排）

如果新增入口，必须同时更新：

- 本文档。
- 对应目录 README。
- `backend/scripts/devtools/check_pipeline_architecture.py`。

## 什么时候才继续拆文件

满足下面任一条件再拆：

- 一个文件超过 300 行且包含 3 种以上职责。
- 改一个小功能需要跨 5 个以上目录。
- 出现循环依赖。
- 同一逻辑重复出现在多个模块。
- 测试很难写，因为 IO、策略、数据结构混在一个函数里。

不满足这些条件时，优先补测试、补文档、补架构检查，而不是继续拆文件。
