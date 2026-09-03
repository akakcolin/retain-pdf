# Scripts 总览

`scripts/` 是整套“PDF -> OCR -> 翻译 -> 保留排版渲染”的脚本工程目录。

现在顶层按职责分成五层：

- `runtime/`
  运行时编排层，只放 pipeline。
- `services/`
  OCR、MinerU、翻译、渲染等具体实现层。
- `foundation/`
  配置、共享工具和提示词资源。
- `entrypoints/`
  人工执行入口。
- `devtools/`
  实验、迁移、示例、测试探针、诊断脚本。

其中 `services/` 内部现在又明确分成两类：

- provider / translation / rendering 这类能力模块
- `services/pipeline_shared/` 这类跨阶段共享协议模块

## 主链路

核心流程可以概括成：

`PDF -> OCR provider -> document_schema -> services/translation -> render_rs(native) -> PDF`

更具体一点：

1. `normalize.stage.v1`
   OCR provider 原始结果进入 `document_schema`，产出 `ocr/normalized/document.v1.json` 和 `document.v1.report.json`
2. `translate.stage.v1`
   翻译链只读取 `document.v1.json`，抽取正文白名单 block，补 continuation / orchestration 元数据，输出 `translated/`
3. `render.stage.v1`
   渲染链只读取翻译产物和源 PDF，输出 `rendered/*.pdf`
4. `book.stage.v1`
   顶层整书流程，只负责编排 `normalize -> translate -> render`，不再让下游直接猜 provider 原始结构

现在的正式块级契约是：

- `geometry`
- `content`
- `layout_role`
- `semantic_role`
- `structure_role`
- `policy`
- `provenance`

说明：

- `type/sub_type/bbox/text/lines/segments` 仍保留，但已经降级为兼容字段
- translation / rendering 主线不应该再基于 raw OCR 字段或 `derived/sub_type` 重新猜正文
- 是否进入翻译，以 `policy.translate` 为唯一正式入口
- translation payload 的正式消费口径也已固定为 strict top-level contract，不再依赖 `metadata` 镜像

## 推荐入口

当前存活入口（整条主链路由 Rust API 驱动，本地直跑用 worker 入口）：

- `scripts/entrypoints/run_translate_only.py`
  顶层 translate worker。只接受已经标准化的 `document.v1.json`。
- `scripts/entrypoints/translate_book.py`
  只翻译，不渲染。
- `scripts/entrypoints/validate_document_schema.py`
  契约校验入口。只用于检查 `document.v1.json`，不是日常整链路入口。
- `scripts/entrypoints/diagnose_failure_with_ai.py`
  失败诊断入口。

render 阶段由 native `render_rs --spec render.spec.json` 执行（见下文「Stage Spec 约定」），不再有 Python 渲染入口。不要把测试脚本当主入口；正常整链路走 Rust API 提交 job，让 Rust 通过 spec 驱动各 worker。

如果要改翻译链路，推荐阅读顺序是：

1. `services/translation/README.md`
2. `services/translation/llm/README.md`
3. 再按需要进入 `services/translation/llm/providers/` 或 `services/translation/llm/shared/orchestration/`

## OCR 归一化

OCR 归一化由 native `render_rs --normalize-ocr` 完成（`rendering_orchestrator/src/normalize/`），Python 侧无归一化实现。新 provider 的 raw JSON 到 `document.v1` 的适配在 Rust 侧新增。Python `services/document_schema/` 只消费 `document.v1.json`（读取/校验/摘要），不接触 provider 原始 JSON。

## 顶层目录说明

- `services/mineru`
  仅保留 `contracts.py`（文件名单），provider 接入已 native 化。
- `services/pipeline_shared`
  provider / translate / render 共用的阶段协议、summary 和 JSON IO。
- `services/translation`
  OCR payload 到翻译 JSON。
- `services/README.md`
  具体能力实现层总说明。
- `foundation/config`
  路径、字体、版式和运行时默认配置。
- `foundation/shared`
  输入解析、job 目录、环境变量、提示词加载等共享能力。
- `foundation/prompts`
  可编辑提示词模板。
- `devtools/experiments`
  实验性流程，不属于稳定主链路。
- `devtools/tests`
  测试探针和排版实验。
- `devtools/tools`
  示例脚本、迁移工具和诊断脚本。

## 结构化输出

任务输出统一落到标准 job root 下。Rust API 默认是：

- `DATA_ROOT/jobs/<job-id>/source`
- `DATA_ROOT/jobs/<job-id>/ocr`
- `DATA_ROOT/jobs/<job-id>/translated`
- `DATA_ROOT/jobs/<job-id>/rendered`
- `DATA_ROOT/jobs/<job-id>/artifacts`
- `DATA_ROOT/jobs/<job-id>/logs`

其中：

- `ocr/unpacked/` 或 provider raw 目录保留 OCR provider 原始产物；MinerU 常见为 `layout.json`，Paddle 常见为 `paddle_result.json` / `paddle_raw`
- `ocr/normalized/document.v1.json` 是当前翻译/渲染主链路使用的统一 OCR 输入
- `ocr/normalized/document.v1.report.json` 记录 adapter/provider 探测、defaults 默认补齐和 schema 校验摘要
- `translated/translation-manifest.json` 与其引用的逐页 payload 是翻译阶段正式产物
- `rendered/*.pdf` 是最终输出 PDF
- `rendered/typst/` 保留 Typst 中间产物，便于查错和回溯
- `artifacts/` 放 summary、bundle 索引等下载产物
- `logs/` 放阶段日志和结构化事件输出

当前约定：

- 主链路优先消费 `document.v1.json`
- `document.v1.json` 的正式消费口径是 `geometry/content/layout_role/semantic_role/structure_role/policy/provenance`
- 如果入口给的是 raw `layout.json`，会先做一次显式规范化，再进入翻译主线
- raw MinerU 结构保留给 adapter、调试和回溯，不再作为主链路的隐式数据契约
- 如果只是做排错、状态展示或 API 输出摘要，优先消费 `document.v1.report.json`
- Python 侧统一通过 `services/document_schema/reporting.py` 读取 report 和生成 normalization summary
- `specs/` 保存阶段 spec JSON，当前已覆盖：
  - `normalize.spec.json` -> `normalize.stage.v1`
  - `translate.spec.json` -> `translate.stage.v1`
  - `render.spec.json` -> `render.stage.v1`
  - `provider.spec.json` -> `provider.stage.v1`
  - `book.spec.json` -> `book.stage.v1`

## Stage Spec 约定

当前 Rust API 到 Python worker 的稳定协议，已经固定为：

`python -u <entrypoint> --spec DATA_ROOT/jobs/<job-id>/specs/<stage>.spec.json`

约定如下：

- spec 只保存阶段输入、参数和 job 引用，不再把 Python 内部实现细节暴露给 Rust
- `job.job_root` 是路径推导锚点；各阶段内部通过 `job_dirs.py` 派生 `source/ocr/translated/rendered/artifacts/logs`
- 密钥不明文写入 spec
  - 翻译 key 通过 `credential_ref=env:RETAIN_TRANSLATION_API_KEY`
  - 如果 provider 是 `mineru`，对应 token 通过 `credential_ref=env:RETAIN_MINERU_API_TOKEN`
  - 运行时由 Rust 注入环境变量，Python 通过 `stage_specs.resolve_credential_ref(...)` 读取
- Rust 主工作流和本地入口都已切到 spec-only；存活入口：
  - `entrypoints/run_translate_only.py` -> `translate.stage.v1`
  - `entrypoints/translate_book.py`
  - `entrypoints/validate_document_schema.py`
  - `entrypoints/diagnose_failure_with_ai.py`
- normalize 阶段由 native `render_rs --normalize-ocr` 执行（单一实现，Python normalize 引擎已退役）
- render 阶段由 native `render_rs --spec render.spec.json` 执行，不再有 Python 渲染入口

也就是说，当前“最上层整个流程”的真实执行口径是：

- 本地：`translate_book.py` / 直接跑 worker 入口（`run_translate_only.py`）
- Rust API：创建 job，由 Rust 生成 `specs/*.spec.json` 并依次启动 worker
- 测试脚本：只做回归，不代表主执行路径

## Python 依赖真相源

当前 Python 依赖已经收敛到仓库根目录的 [`pyproject.toml`](/home/wxyhgk/tmp/Code/pyproject.toml)。

不要直接手改这些 requirements 文件：

- [`docker/requirements-app.txt`](/home/wxyhgk/tmp/Code/docker/requirements-app.txt)
- [`docker/requirements-test.txt`](/home/wxyhgk/tmp/Code/docker/requirements-test.txt)
- [`desktop/requirements-desktop-posix.txt`](/home/wxyhgk/tmp/Code/desktop/requirements-desktop-posix.txt)
- [`desktop/requirements-desktop-windows.txt`](/home/wxyhgk/tmp/Code/desktop/requirements-desktop-windows.txt)
- [`desktop/requirements-desktop-macos.txt`](/home/wxyhgk/tmp/Code/desktop/requirements-desktop-macos.txt)

修改依赖后统一执行：

```bash
python backend/scripts/devtools/sync_python_requirements.py --repo-root .
```

只检查是否漂移：

```bash
python backend/scripts/devtools/sync_python_requirements.py --repo-root . --check
```

兼容说明：

- 旧任务目录如果还是 `originPDF/jsonPDF/transPDF/typstPDF`，当前后端会直接拒绝详情/下载接口，请重新跑任务生成标准 schema
- 旧的逐页 translation JSON 直扫模式已经退出主线；render-only 必须提供 `translation-manifest.json`

## 子目录文档

- [PIPELINE_DIRECTORY_MAP.md](./PIPELINE_DIRECTORY_MAP.md)
- [foundation/config/README.md](./foundation/config/README.md)
- [foundation/shared/README.md](./foundation/shared/README.md)
- [services/README.md](./services/README.md)
- [services/translation/README.md](./services/translation/README.md)
- [services/translation/llm/README.md](./services/translation/llm/README.md)
- [services/translation/core/orchestration/README.md](./services/translation/core/orchestration/README.md)
- [services/translation/services/continuation/README.md](./services/translation/services/continuation/README.md)
- [services/translation/services/policy/README.md](./services/translation/services/policy/README.md)

## 设计边界

- `services/translation` 不直接操作 PDF
- `render_rs`（native）不直接决定翻译策略
- 翻译阶段由 `services/translation/entrypoints/translate_only_pipeline.py` 经 `services.translation.public` 门面执行，不再有独立编排层
- `foundation/` 不承载具体业务流程
- `entrypoints/` 只做入口，不承载核心实现
- `devtools/` 不能反向成为主链路依赖

## 架构检查

日常改动建议至少跑这两条：

- `python3 backend/rust_api/scripts/check_architecture.py`
- `python3 backend/scripts/devtools/check_pipeline_architecture.py`

第二条负责卡住 Python 主链最容易回退的边界：

- 任何模块重新引入 `runtime/pipeline` 编排层并 import `services.mineru` / 已退役的 `services.ocr_provider`
- 任何模块重新理解 provider raw token，例如 `layoutParsingResults`
- 任何模块重新依赖 `document_schema` provider adapters（翻译链路外）
- `services/translation` 重新碰 provider raw 结构
- `entrypoints/*` 绕过稳定入口，直接连深层实现
- 非 devtools 模块重新 import fitz / 重新出现第二个 normalize 实现
