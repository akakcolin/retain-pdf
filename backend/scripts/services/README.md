# Services 说明

`scripts/services/` 是具体能力实现层。

这里放真正执行工作的模块，而不是流程编排：

- `ocr_provider/`
  OCR provider API 接入层的独立约定。这里只定义“第三方 OCR 服务怎么接进来”，不把 provider API 细节耦合到翻译/渲染工作流。
- `document_schema/`
  统一中间文档结构版本定义、adapter registry、defaults 收口、schema 校验与 normalization report。
- `mineru/`
  MinerU 这个 provider 的具体实现：提交、轮询、下载、解包、任务产物整理。
- `pipeline_shared/`
  provider / translate / render 主线共用的阶段协议、summary、统一 `pipeline_events.jsonl` 事件流和 JSON IO，不绑定任何单一 provider。
- `translation/`
  OCR 解析、翻译编排元数据、策略过滤、LLM 调用、结果回填。
- `rendering/`
  PDF 擦除、背景处理、Typst 生成、公式规整、最终渲染与压缩。

设计原则：

- `services/*` 负责把单项能力做完整
- OCR 归一化由 native `render_rs --normalize-ocr` 完成；`document_schema/` 只消费 `document.v1.json`，不承载归一化实现
- 需要排查 normalized 转化问题时，优先看 `document.v1.report.json` 或 `validate_document_schema.py`（校验模式）
- 如果只是消费 provider / defaults / validation 摘要，优先走 `document_schema/reporting.py`
- `mineru/` 仅保留 `contracts.py`（文件名单），provider 接入已 native 化
- `pipeline_shared/` 是中性共享层，不应该再放 provider 私有逻辑
- `translation/ocr` 主线优先读取 normalized document，而不是直接依赖某个 OCR provider 的原始 JSON
- 上层入口（`entrypoints/` 或 `services/translation/entrypoints/`）负责把这些能力串起来，不要直接跨服务拼流程时绕过各服务的公开门面
- 公共配置和共享工具继续下沉到 `foundation/`

## OCR 归一化

新 OCR provider 的 raw JSON → `document.v1` 适配在 native `render_rs --normalize-ocr`（`rendering_orchestrator/src/normalize/`）中新增；Python 侧不再有 adapter / normalize 实现，`document_schema/` 只负责读取、校验和摘要 `document.v1.json`。

## 协作规矩

现在可以按模块拆分负责人，但边界必须按协议来守：

- OCR / provider 负责人主要维护 native `rendering_orchestrator/src/normalize/`、`mineru/`、`document_schema/`
- 翻译负责人主要维护 `translation/`
- 渲染负责人主要维护 `rendering/`
- 编排负责人主要维护 `entrypoints/` 与 `services/translation/workflow/`

默认原则：

- 每个负责人优先在自己模块内解决问题，不把临时特判扩散到别的模块
- `document.v1.json`、`translation-manifest.json`、render-only 输入协议属于稳定交接点，不能单边修改
- 如果必须改交接协议，必须同时更新上下游 README、调用入口、兼容逻辑和测试
- translation / rendering 主线禁止重新依赖 provider raw JSON
- pipeline 只负责编排，不负责吸收 provider 特判、翻译细节或渲染补丁
