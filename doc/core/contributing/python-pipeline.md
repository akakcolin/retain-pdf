# Python 流水线贡献指南

## 分层方向

总体分层：

```text
entrypoints -> services/*（各子系统公开门面）-> foundation
```

基本规则：

- OCR raw payload 由 native `render_rs --normalize-ocr` 归一化为 `document.v1.json`，Python 侧无 adapter / normalize 实现。
- 翻译主链只消费 `document.v1` 和 translation stage spec。
- 渲染主链只消费源 PDF、translation manifest、逐页翻译 payload 和 render stage spec。
- 阶段编排在各子系统入口内完成，不吸收 provider、LLM、Typst、redaction 的细节（独立的 `runtime/pipeline` 编排层已于 2026-09 移除）。
- `translation` 不 import 渲染模块（渲染已 native `render_rs`，Python 侧无渲染树），也不消费 provider raw JSON。

更细规则见 [Python 后端架构边界](../python/architecture.md)。

## 改动规则

- 新逻辑优先放在已有分层目录，避免跨层 import。
- 翻译、渲染、OCR provider 的边界按 `doc/core/python/architecture.md` 执行。
- 新增规则类逻辑时优先补最小回归测试，尤其是公式、术语、bbox、payload 变换。
- 翻译一致性、术语表、公式保护、渲染策略应尽量通过稳定 manifest/spec 传递，不要靠跨模块读取内部临时文件。
- 渲染和 PDF 处理改动要说明是否改变输出 PDF 内容、体积、首屏预览体验或可复制文本。

## 常用检查

Python 翻译相关：

```bash
python3 -m compileall -q backend/scripts/services/translation
PYTHONPATH=backend/scripts python3 -m pytest backend/scripts/devtools/tests/translation -q
python3 backend/scripts/devtools/check_pipeline_architecture.py
```

Python document schema / provider 相关：

```bash
PYTHONPATH=backend/scripts python3 -m pytest backend/scripts/devtools/tests/document_schema -q
python3 backend/scripts/devtools/check_pipeline_architecture.py
```

渲染相关（native `render_rs`，无 Python 渲染测试套件）：

```bash
cargo test -p rendering_core -p rendering_output -p rendering_reader -p rendering_writer -p rendering_orchestrator
python3 backend/scripts/devtools/check_pipeline_architecture.py
```

## PR 说明

涉及 Python 流水线的 PR 至少说明：

- 影响 OCR、translation、rendering 中哪一段。
- 是否改变 `document.v1`、translation manifest、render payload 或阶段事件。
- 是否影响旧 job 的重新渲染、断点恢复或诊断。
- 使用了哪些样本验证，是否包含公式、图注、脚注、长段落或大页数 PDF。
