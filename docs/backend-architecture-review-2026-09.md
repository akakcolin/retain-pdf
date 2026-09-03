# RetainPDF 后端架构审查报告

> 审查时间：2026-09-02
> 范围：`backend/rust_api/`（Rust API，约 5.4 万行）、`backend/scripts/`（Python 流水线，约 6 万行）、`backend/rendering_*`（渲染 crate 边界）
> 验证方式：通读关键模块 + 运行项目架构门禁 + Python 测试套件（610 通过）+ Rust 测试套件（420 通过 / 1 环境性失败）+ clippy（53 条告警）

> **修复进展（2026-09-03）**：第五节逻辑问题 2、3 已修复；第四节 P1 中"Python 内部 DEFAULT_BASE_URL 重复"已合并（transport.py 为唯一字面量）；P1"provider 识别靠 URL 嗅探"已完成端到端改造——`provider_family` 从 Rust `TranslationInput` 经 translate stage spec 显式传递至 Python `TranslationExecutionRequest`，执行计划优先使用显式值，URL 嗅探降级为兜底。P2"私有函数跨模块导入"已收敛：生产代码 59 处全部清零（AST 校验），涉及 11 个叶模块的 31 个函数提升为公开 API；删除零消费者的死 facade `batch_plan.py`；架构门禁的私有导入例外表已清空，后续任何新的跨模块私有导入将直接报错。P2"半途迁移"已收尾：删除纯透传层 `runtime/pipeline/translation_stage.py`（连同已清空的 `runtime/` 目录树），translate-only 入口直接经 `services.translation.public` 门面构造请求执行；`stages.py → phases/` 迁移完成，垫片删除，`book_flow.py` 与测试直指 phases；层规则与文档（6 处 README/架构文档）同步更新。验证：Python 613 测试全过、架构门禁通过、clippy 无新增告警。

---

## 一、总体评价

这是一个**架构纪律明显高于平均水平**的全栈后端。分层边界不仅有文档，还有可执行的门禁脚本（`check_pipeline_architecture.py`、`check_stage_specs_contract.py`），且当前全部通过。主要问题不在"混乱"，而在三处：**双语言配置重复、模块封装边界被穿透、以及半途迁移留下的过渡层**。

## 二、当前架构分层

```
前端 / 桌面端 (Tauri)
      │ HTTP
┌─────▼──────────────────────────────────────┐
│ rust_api (axum)                            │
│  routes → services → db (SQLite/WAL)       │
│  job_runner：进程编排、取消、队列、事件派生 │
└─────┬──────────────────────────────────────┘
      │ spawn 子进程 + stage spec (JSON, 版本化契约)
┌─────▼──────────────────────────────────────┐
│ Python scripts                             │
│  runtime/pipeline → services.translation   │
│  (workflow 编排 → llm 执行 → artifacts)    │
└─────┬──────────────────────────────────────┘
      │ render.stage.v1 spec
┌─────▼──────────────────────────────────────┐
│ render_rs (native) 渲染全量接管            │
└────────────────────────────────────────────┘
```

边界规则明确：OCR 原始产物先归一化为 `document.v1.json`；翻译只消费 normalized document；渲染无 Python 渲染树。

## 三、做得好的地方（值得保留）

1. **可执行的架构门禁**：`devtools/architecture_checks/` 把"跨层依赖规则"变成 CI 可跑的检查（fitz import 白名单、normalize 单引擎、翻译字段写入门禁），比写在 wiki 里的约定有效得多。
2. **错误处理规范**：Rust 侧 `AppError` 统一错误码（40100/40000/...），`thiserror` + `anyhow` 分层清晰；生产代码（排除测试模块）的 `unwrap/expect` 极少，残留的几乎都是正则编译和 mutex poisoning 这类可接受场景。
3. **并发与资源设计克制**：SQLite 用 WAL + `busy_timeout(5s)` + 每操作新建连接，注释解释了为什么；任务并发用 `Semaphore` + 取消注册表 + 轮询退出，没有过度设计。
4. **危险操作有防护**：retention 清理有 `documents` guard（防止误删"只入库未翻译"文档的源 PDF，注释明确写了 zombie 卡片场景）；PDF 修复先写 `*.repairing.pdf` 再 rename，避免中途失败毁掉原文件。
5. **契约版本化**：`translate.stage.v1`、`render.stage.v1` 等 schema version 显式声明，跨语言演进有锚点。
6. **测试真实可跑**：Python 610 个测试 29 秒全过；Rust 420 个测试通过。代码库零 TODO/FIXME 残留。
7. **文件规模克制**：最大 Rust 源文件 1142 行，最大 Python 生产文件 617 行，没有 god file。

## 四、架构层面问题（按优先级）

### P1 — 双语言默认值重复定义，缺少单一事实源

同一默认值在两端、甚至同端多处硬编码：

| 配置项 | 位置 |
|---|---|
| `https://api.deepseek.com/v1` | Rust: `config/provider.rs:207`、`config/ai.rs:23`、`ai.rs:40`；Python: `deepseek/transport.py:19`、`deepseek/client.py:22`、`devtools/job_debug_runner.py:318`、`replay_translation_item.py:300` |
| `qwen2.5:7b` | Rust: `config/env_vars.rs:74`；Python: `llm/shared/provider_registry.py:110` |
| `http://localhost:11434/v1` | Rust: `env_vars.rs:80`；Python: `provider_registry.py:111` |

**风险**：任何一处漂移都会静默改变行为（比如改了 Python 默认值但 Rust 在请求里始终显式传值，Python 默认值成为死代码；或反之）。**建议**：由 Rust 侧（请求入口）作为唯一事实源，Python 侧的默认值仅作 standalone CLI 兜底，并加一个契约测试断言两端一致；Python 内部 `transport.py` 与 `client.py` 的重复定义应立即合并。

### P1 — Provider 识别靠 base_url 子串嗅探

`artifacts/aggregator.py:22` 和 `deepseek/client.py:134` 用 `"api.deepseek.com" in normalized_base` 判断 provider family。**风险**：用户走代理/网关/镜像（如 `https://gateway.corp.com/deepseek/v1`）时分类错误，诊断、限流、重试策略随之失效。**建议**：provider family 应从请求显式字段传递，URL 匹配只作 fallback。

### P2 — 私有函数跨模块导入（59 处，10 个生产文件）

典型如 `batch_runner.py` 导入了 4 个下划线私有函数（`_translate_batch_or_keep_origin`、`_failed_results_for_unhandled_batch_exception`、`_drain_translation_tail_queue` 等）。下划线前缀的"请勿跨模块使用"约定已实质失效。**风险**：重构时无法判断真实依赖面，改名即破坏隐式调用方。**建议**：把真正被跨模块使用的函数提升为公共 API（去掉下划线并进 `__all__`），其余的调用方内聚回所属模块。

### P2 — 半途迁移的过渡层

- `workflow/README.md` 自述"目标目录"：`stages.py → phases/`、`batch_runner.py → scheduling/` 迁移未完成，`legacy/page_translation.py`（206 行）仍为 debug-only 调用方保留。
- `runtime/pipeline/translation_stage.py` 是**纯透传层**：40+ 个参数原样转发给 `execute_translation_request`，不增加任何价值。调用方可直接构造 `TranslationExecutionRequest`。
- 同参数透传也解释了 clippy 的 12 条 "too many arguments"（最多 12 个参数）告警——两端都缺少"参数对象化"的收口。

**建议**：为这三个过渡点设截止时间或 issue 跟踪，否则"临时"会变成永久。

### P3 — 风格与性能小问题

- `#[path = "..."]` mod 声明（`db.rs`、`worker_command.rs` 等）：非标准布局，rust-analyzer 虽支持但新人困惑，建议迁移为标准 `mod.rs`。
- Rust 侧正则在**每次调用时重新编译**（`paddle_markdown.rs:124/162/179/183/192/201`、`agent.rs:427/435`），Python 侧反而都在模块级编译。建议用 `std::sync::LazyLock<Regex>`。
- `auth.rs` 用 `HashSet<String>::contains` 比对 API key，非常量时间。本地工具风险低，如需加固可用 `subtle::ConstantTimeEq`。
- `metrics.rs:238` 有未使用导入（编译告警）。

## 五、逻辑错误与隐患（实测验证）

### 1. 测试环境耦合 —— 已复现

`services::jobs::creation::tests::store_pdf_upload_repairs_bad_xref_pdf` 在本机失败：

```
bad xref pdf should be repaired: BadRequest("invalid pdf: ... ModuleNotFoundError: No module named 'fitz'")
```

原因：测试经 `platform_python_bin()` 调用**系统 python3** 执行内联 PyMuPDF 脚本，环境缺 `fitz` 时直接失败，无 skip guard。用含 PyMuPDF 的解释器重跑则 420 个测试全部通过。**建议**：检测不到 `fitz` 时 `eprintln` + 提前返回（或标记 ignored），避免"环境缺依赖"被误报为"代码回归"。

### 2. `batch_runner.py:320-325` finally 中的 raise 会掩盖原始异常

```python
finally:
    for executor in executors:
        executor.shutdown(wait=True, cancel_futures=False)
    for future in worker_futures:
        if future.done() and future.exception() is not None:
            raise future.exception()   # ← 若主循环已在抛异常，此处会替换掉它
```

主循环第 290 行抛出 `RuntimeError("queues stopped early")` 后，`finally` 里的 `raise` 会用 worker 异常**覆盖**掉它，丢失"队列提前停止"这一更上层的诊断信息。**建议**：仅在没有异常传播时才 raise（用 `sys.exc_info()` 判断），或把 worker 异常链式附加（`raise ... from ...`）。

### 3. `page_range.py` 报错信息误导

```python
stop = total_pages - 1 if end_page < 0 else min(end_page, total_pages - 1)
if start > stop:
    raise RuntimeError(f"Invalid page range: start_page={start}, end_page={stop}")
```

报错里的 `end_page` 是 clamp 后的值，不是用户输入。用户传 `start=50, end=200`（共 10 页）会看到 `end_page=9`，排障时困惑。**建议**：错误信息同时保留原始输入与 clamp 结果。

### 4. 异常吞没点（7 处）

生产代码有 7 处 `except Exception: pass` 形态的静默吞没（数量少，但建议逐一确认是否至少应记日志）。

## 六、改进路线建议

**立即（低风险）**
1. 合并 Python 内部重复的 `DEFAULT_BASE_URL`；给双端默认值加一致性契约测试
2. 修复 `batch_runner.py` finally 异常屏蔽、`page_range.py` 报错信息
3. Rust 正则改 `LazyLock`；清理 53 条 clippy 告警并纳入 CI `-D warnings`

**近期（中风险）**

4. provider family 改显式传递，URL 嗅探降级为 fallback
5. 私有跨模块导入收敛：提升公共 API 或内聚调用方
6. 删除/收口 `translation_stage.py` 透传层；完成 `stages.py → phases/` 迁移或明确冻结现状

**持续**

7. 架构门禁脚本很好——建议把 clippy、双端默认值一致性也做成同级门禁
8. 测试环境依赖（fitz、typst、gs）做显式探测 + skip，而非硬失败

## 七、验证记录

| 检查 | 结果 |
|---|---|
| `check_pipeline_architecture.py` | ✅ passed |
| `check_stage_specs_contract.py data/jobs` | ⚠️ 本机无 stage spec 数据文件可验 |
| Python pytest（devtools/tests，610 项） | ✅ 610 passed / 29.6s |
| Rust `cargo check` | ✅ 通过 |
| Rust `cargo test --lib`（422 项） | ⚠️ 420 passed，1 失败为环境缺 fitz（换解释器后全过），2 ignored |
| `cargo clippy` | ⚠️ 53 条告警（too many arguments ×12、冗余引用、可 derive 的 impl 等） |
