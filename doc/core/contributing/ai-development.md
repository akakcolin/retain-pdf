# AI 辅助开发指南

RetainPDF 鼓励使用 AI 辅助开发。推荐优先使用 Codex 或 Claude Code 这类能读写本地仓库、运行命令、执行测试的 coding agent，而不是只在聊天窗口里让模型凭空给方案。

AI 可以提高效率，但不能替代边界判断、测试验证和最终责任。提交 PR 的人需要确认改动符合项目架构、通过必要检查，并能解释风险。

## 推荐工具

- Codex：适合在本地仓库中做代码修改、重构、测试、文档整理和发布前检查。
- Claude Code：适合长上下文代码阅读、跨文件重构、生成测试和总结复杂改动。

不要求贡献者必须使用某一个工具，但如果使用 AI 参与开发，建议在 PR 描述中简单说明 AI 参与了哪些环节，例如“辅助生成测试”“辅助整理文档”“辅助重构 import 边界”。

## 建议的 AI Skills

可以把下面这些能力写成 Codex skill、Claude Code command，或项目内的 agent checklist。

### RetainPDF 项目上下文

用途：让 AI 先理解仓库边界再动手。

应包含：

- 项目根目录：`/home/wxyhgk/tmp/Code`
- 主要模块：`backend/rust_api/`、`backend/scripts/`、`frontend/`、`desktop/`、`docker/`、`doc/`
- 核心规则：不要回滚无关脏改；手动编辑用 patch；改前先读相邻代码；按模块跑测试。
- 文档入口：根目录 `CONTRIBUTING.md` 和 `doc/core/contributing/README.md`

### Rust API 边界检查

用途：防止 AI 把 route、service、runner、db 混在一起。

应提醒 AI：

- `routes/*` 只做 HTTP adapter。
- service 层做业务聚合和 view/projection。
- `job_runner/*` 做运行态执行。
- 数据库访问通过 `Db` facade，不在 route 里直接写 SQL。
- 新 API 字段要同步文档和测试。

常用检查：

```bash
cargo fmt --manifest-path backend/rust_api/Cargo.toml --check
cargo test --manifest-path backend/rust_api/Cargo.toml
cd backend/rust_api && python3 scripts/check_architecture.py
```

### Python 流水线边界检查

用途：防止 AI 引入跨层 import 或绕过稳定 manifest。

应提醒 AI：

- OCR raw payload 先进入 `document_schema`，产出 `document.v1`。
- translation 不 import rendering。
- rendering 只消费源 PDF、translation manifest、逐页 payload 和 render spec。
- 新增公式、术语、bbox、渲染策略时补最小回归测试。

常用检查：

```bash
python3 backend/scripts/devtools/check_pipeline_architecture.py
PYTHONPATH=backend/scripts python3 -m pytest backend/scripts/devtools/tests/translation -q
PYTHONPATH=backend/scripts python3 -m pytest backend/scripts/devtools/tests/rendering -q
```

### 前端与桌面端同步

用途：防止 AI 只改网页源码，忘记桌面端 bundle。

应提醒 AI：

- 改 `frontend/**` 后需要跑 `npm --prefix desktop run verify-frontend-sync`。
- 不要只改 `desktop/app/frontend/**`。
- 本地静态前端默认端口是 `40001`。

常用检查：

```bash
npm --prefix frontend run build
npm --prefix desktop run verify-frontend-sync
```

### 测试与回归生成

用途：让 AI 帮专业测试人员把问题转成可复现用例。

应提醒 AI 输出：

- 环境、版本、provider、workflow。
- 样本是否可公开。
- 页码、bbox、截图、job_id。
- 复现步骤、期望结果、实际结果。
- 最小 fixture 或自动化测试建议。

### 文档一致性检查

用途：避免改代码后文档落后。

应提醒 AI 检查：

- API 字段是否同步 `doc/core/api/`。
- Rust 边界是否同步 `doc/core/rust_api/`。
- Python 边界是否同步 `doc/core/python/`。
- 前端、Docker、桌面端端口和命令是否一致。
- 根目录 `CONTRIBUTING.md` 是否仍然只是短入口。

## 推荐工作流

1. 让 AI 先读相关子文档，不要直接改代码。
2. 要求 AI 给出影响范围和验证计划。
3. 让 AI 小步提交 patch，不做无关格式化。
4. 跑对应测试或检查。
5. 让 AI 做一次 review：重点查跨层依赖、旧兼容、测试缺口、文档缺口。
6. 人工确认输出、风险和 PR 描述。

## 提示词建议

可以直接对 Codex 或 Claude Code 这样说：

```text
你在 RetainPDF 仓库中工作。先阅读 CONTRIBUTING.md 和相关 doc/core/contributing 子文档。
只修改本任务相关文件，不要回滚无关脏改。
改动前说明影响范围，改动后运行对应测试。
如果不能运行测试，说明原因和剩余风险。
```

针对后端：

```text
检查这次 Rust API 改动是否违反 routes -> services -> job_runner/db 的边界。
重点看 route 是否拼业务 JSON、service 是否直接依赖 HTTP Response、job_runner 是否反向依赖 service。
给出文件和行号，必要时直接修复。
```

针对 Python：

```text
检查 translation、ocr_provider 是否存在跨层 import。
渲染已 native `render_rs`，Python 侧无渲染树，不存在 Python 渲染 import。
如果需要共享数据，通过 manifest/spec/document.v1 传递。
```

针对测试：

```text
把这个 bug 报告整理成可复现测试用例。
需要包含环境、样本、页码、bbox、复现步骤、期望结果、实际结果，以及建议自动化测试落点。
```

## 注意事项

- AI 生成的代码必须经过人工 review。
- AI 不应提交真实用户文件、私有 token、本地数据库或大体积运行产物。
- AI 做重构时必须说明替代了哪些重复或耦合，不能为了“看起来更通用”新增抽象。
- AI 修改发布、Docker、桌面端打包流程时，要额外说明回滚方式和验证方式。
