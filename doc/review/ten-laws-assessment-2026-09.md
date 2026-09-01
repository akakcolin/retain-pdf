# RetainPDF 十律架构评估

评估日期：2026-09-01
评估对象：retain-pdf @ `19b2b3e9`（main）
代码规模：约 24 万行（Python 6.9 万 / Rust 10.6 万 / TypeScript 5.0 万 / Tauri Rust 0.16 万）
综合评分：**50 / 100（5.0）— 工具级，正卡在工业级门槛外**

---

## 零、核心诊断：内核清醒，外壳臃肿

在展开十律之前，先给出一句话结论，后面十条律都是它的注脚。

RetainPDF 拥有一个真正的「第一律级」内核：**`document.v1`**。

它符合顶级原语的几乎全部特征——单一、稳定、版本化（`schema + schema_version` 前缀强制校验）、有 JSON Schema 定义（`rendering_orchestrator/schemas/document.v1.schema.json`，493 行）、有唯一访问层（`services/document_schema/consumer_reader.py`）、有 ADR 记录它为什么存在（`doc/adr/0001-use-document-v1-as-ir.md`，含替代方案对比）。围绕它，6 个 stage spec 契约 + 三个架构检查脚本进 CI，把「内核不被污染」变成了机器可验证的事，而不是靠Code Review 自觉。

**这是本项目最值钱的资产，也是它能拿到 7 分的唯一原因。**

但内核之外长出了三层「壳」，每一层都在稀释内核的价值：

| 壳层 | 表现 | 代价 |
|---|---|---|
| 语言壳 | Python + Rust(5 crate) + TS + Tauri Rust | 贡献者需要 4 套工具链 |
| 前端壳 | `js/`(323 文件) + `pages/`(227 文件) + `pages/reader/legacy/` | 550 个前端文件，两套心智模型 |
| 状态壳 | `jobs` 表列 + `status_json` blob + `events` 表 + `pipeline_events.jsonl` | 同一语义四个真相 |

三层壳症状各异，病因相同：**同一语义存在多个真相**。而「单一真理源」恰是第五律的核心判据。

分数分布本身就印证了这个诊断：

- 第一律（抽象能力）**7** + 第九律（内核产生价值）**7** → 抽象能力强
- 第四律（认知收敛）**3** + 第八律（形态稳定）**4** → 收敛能力弱

**项目现在的问题不是「设计得不好」，而是「设计得很好但减法还没做完」。**

git 历史支持这个判断：582 次提交中，最近 200 次里有 29 次（14.5%）是在删东西——`剪除` `退役` `收编` `删除死阶段` `移除冗余迁移区`，而且每次都留了清晰的 commit 记录。这是健康信号，说明团队知道问题在哪，缺的是把减法做完的时间窗口。

---

## 一、底层抽象的穿透力 — **7 / 10**

### 判定：找到了原语，但原语本身不够锋利

**成立的证据（这是加分项）**

原语链 `page → block → line → segment`，最小完备集清晰。block 的 13 个必填字段里，有几个设计堪称精彩：

- `policy: {translate: bool, translate_reason: string}` —— **翻译决策编码进文档本身**，而不是散落在调用方的 if-else 里。这意味着「为什么这段没翻译」是可追溯的数据，不是需要考古的逻辑。
- `provenance` 保留 `raw_label/raw_sub_type/raw_bbox/raw_path` —— 归一化不丢血缘，出错能回溯到 provider 原始输出。
- `continuation_hint` 承载跨页续接（`{source, group_id, role: single|head|middle|tail, scope, reading_order, confidence}`）—— 把「跨页段落合并」这个排版难题变成了一个可声明的字段，而不是渲染期的启发式猜测。
- `source.provider` 保留血缘，ADR 0001 明令「下游不得为某个 provider 特判去读 raw 字段」。

唯一 accessor 层 `consumer_reader.py` 是真正的架构纪律：`markdown_serializer.py` 能从 normalized JSON 反向生成 `md/full.md`，正是因为 accessor 层隔离了存储细节。

**扣分的原因**

1. **原语偏宽**。block 有 13 个必填字段，其中三套 role 轴并存：`layout_role`（title/heading/paragraph/list_item/caption/header/footer/footnote/page_number/unknown）、`semantic_role`（body/abstract/reference/metadata/affiliation/acknowledgement/unknown）、`structure_role`。

   对比 Git 的 DAG（一个有向无环图 + 三种对象）、Redis 的 Dict —— 顶级原语的共同点是**可以用一句话说清，且能推导出其他概念**。document.v1 目前更像一个「领域模型的最大公约数」，三个 role 轴之间边界需要记忆，且高度怀疑是同一个信号的三种投影（见第十律的改进建议）。

2. **存在双实现漂移风险**。`rendering_orchestrator/src/normalize/contract.rs` 是 `contract_v1.py` 的 Rust 移植。同一个 role 判定逻辑两处实现，是典型的「单一真理源」破裂点。虽然 Schema 文件本身只有一份（已核实：`backend/rendering_orchestrator/schemas/document.v1.schema.json`，Python 侧引用同一份），但**语义实现是两份**。

**改进建议**

- 短期：为三套 role 轴写一个「正交性论证」ADR，明确每套轴回答什么互斥的问题。如果论证不出来，说明该合并。
- 中期：把 `contract.rs` 和 `contract_v1.py` 的 role 判定做成**共享的 golden 测试向量**（一份 JSON 用例，两侧都跑），把漂移变成 CI 失败。

---

## 二、正交性与组合性 — **6 / 10**

### 判定：三个扩展点，一个优秀、一个及格、一个不及格

**优秀：OCR provider（这是全项目的模板）**

```python
# services/document_schema/adapters.py:85
def register_ocr_adapter(*, provider: str, detector: Detector, builder: AdapterBuilder) -> None:
```

已注册 4 个（generic_flat_ocr / mineru / mineru_content_list_v2 / paddle），配 `provider_adapters/provider_adapter_template.py`。**新增 provider = 1 个新文件 + 1 次 register 调用，零核心改动。** 这是教科书级的开闭原则，也是 ADR 0001 的直接兑现。

**及格：Rust crate 拆分**

5 个 crate 的拆分理由是依赖隔离，不是随手分层：`rendering_core` 只有 serde/blake2，**故意不含 mupdf**（传递依赖仅 20 个包，对比 `desktop/src-tauri` 的 495 个），因此可被读写两侧共享而不拖入重依赖。这个决策是有想法的。

**不及格：翻译引擎**

有 `TranslationProviderRuntimeProtocol`（Protocol + 10 个 Callable 类型 + `TranslationProviderCapabilities`），看起来很规范。但 `resolve_active_provider_runtime()` 是硬编码二选一：

```python
if offline_mode() and os.environ.get("RETAIN_LOCAL_LLM_BASE_URL"):
    return LOCAL_LLM_RUNTIME
return DEEPSEEK_RUNTIME
```

更严重的是 **DeepSeek 语义泄漏到至少 4 处非正交点**：

| 位置 | 泄漏形式 |
|---|---|
| `artifacts/aggregator.py:22-25` | 按 `base_url` 字符串推断 `provider_family` |
| `shared/control_context.py:378-381` | 硬编码 `deepseek_balanced` profile |
| `orchestration/common.py:154` | `is_low_risk_deepseek_batch_item()` |
| `batched_plain_request.py:36` | `should_use_direct_deepseek_batch()` |

**换 Claude 至少改 5 个文件。** 按第二律的判定标准（「增加一个功能需要修改核心代码 = 正交性失败」），这一项不及格。

**不及格：输出格式**

`rendering_orchestrator/src/run.rs:61-77` 硬编码 match `overlay`/`dual`/`typst`/`typst_visual`，未知 mode 直接 `bail!`。新增输出 = 新增 `stages/*.rs` + 改 `run.rs` 的 match + 改 `bundle.rs` 的 mode 解析。**没有抽象层。**

**改进建议（按 ROI 排序）**

1. 把 `provider_family` 从「按 base_url 字符串推断」改为「Protocol 上的 capability 查询」：`capabilities().family`。这一处改动能同时消掉 `is_low_risk_deepseek_batch_item` 和 `should_use_direct_deepseek_batch` 两个特判。
2. `deepseek_balanced` → 通用 `balanced` profile，具体参数由 provider 自己提供。
3. 输出格式抽象：定义 `RenderMode` trait（`spec() -> Spec` / `run() -> Result`），`run.rs` 改为遍历注册表而非 match。参考 OCR adapter 的写法——**同一个项目里已经有正确答案了，照抄即可**。

---

## 三、对物理边界的忠实度 — **5 / 10**

### 判定：有度量意识，但覆盖不全，关键防护默认关闭

**做得好的（说明团队懂硬件）**

- **自适应并发 + warmup**：`artifacts/aggregator.py` 先只放行 1 条请求让 provider 建立前缀缓存，再放开并发——这是真正理解 LLM API 计费与冷启动的人写的（L258-263）。
- HTTP 连接池随并发自适应：`pool_size = min(pool_cap, max(1, min(adaptive_limit, per_thread_cap)))`。
- 重试尊重 `Retry-After` 头 + 指数退避 + 25% 抖动 + 累计等待统计。
- 诊断粒度极细：`phase_elapsed_ms` / `peak_inflight_*` / `slow_request_samples` / `token_usage`。
- `setpgid` 进程组隔离，让 kill 能作用于整个进程树而非单个 pid。
- D5 性能门禁：`render_elapsed_mean ≤ 60s`（实测 3.853s）、`native_hit_ratio ≥ 0.9`（实测全部 1.0）、`python_renderer` 计数必须为 0。

**扣分的（缺口明确）**

1. **复杂度预算默认是关的。** `config/upload.rs` 的 `RUST_API_UPLOAD_MAX_BYTES / _PAGES / _COMPLEXITY` **默认值全是 0**，且 `unlimited()` 构造在 6 处被调用（`state.rs:123`、`worker_command.rs:56`、`process_runner.rs:129`、`glossaries.rs:510`、`api_tests/jobs_common.rs:43` 等）。

   `页数 × 对象数` 用 `u128` 防溢出，设计是对的——**但一个默认关闭的防护等于没有防护**。PDF 是出了名的恶意输入载体，这是当前最高危的暴露面。

2. **无沙箱、无资源配额、无渲染硬超时。** D5 baseline 只做**事后观测**（记录 `render_elapsed` 均值），不阻断。渲染进程跑飞了没有 wall-clock 上限。

3. **无流式处理。** 未发现 PDF 分页流式读取或 page-level 增量落盘；`streaming` 只出现在 HTTP 下载和 AI 流式输出。大 PDF（数百页）全量载入内存，OOM 风险真实存在。

4. **子进程冷启动未计量。** 每阶段 spawn 新进程（Python 解释器启动 + PyMuPDF 导入），无 worker pool / warm pool。D5 要求渲染不得 spawn python3 是对的，但 OCR/翻译阶段仍是纯 Python 冷启动。

5. **指标覆盖不全。** `/metrics` 只聚合 render 阶段；OCR 和翻译只有 per-job 的 JSON 产物，无法做全局观测。

**改进建议**

- **P0**：把 `RUST_API_UPLOAD_MAX_*` 的默认值从 0 改为合理上限（建议 bytes=200MB / pages=2000 / complexity=10_000_000），并给 `unlimited()` 加 `#[cfg(test)]` 或显式 opt-in 标记。
- **P0**：给渲染子进程加 wall-clock 硬超时（建议 300s），超时 SIGTERM → SIGKILL。
- **P1**：把 OCR/翻译阶段的 `elapsed_ms` 也导出到 Prometheus，让 D5 门禁覆盖全链路而非只有 render。
- **P2**：大 PDF 分页流式处理；Python worker 改长驻进程池（顺带解决冷启动）。

---

## 四、认知负载极小化 — **3 / 10**（最弱项）

### 判定：一个核心开发者无法在脑中运行这个系统

这是全项目最严重的问题。不是代码写得差，而是**规模与多样性超出了人脑可容纳的上限**。

**规模事实**

| 维度 | 数量 |
|---|---|
| 语言/运行时 | 4（Python、Rust、TypeScript、Tauri Rust） |
| Rust crate | 5 个 + Tauri host |
| 前端文件 | 550 个（`js/` 323 + `pages/` 227） |
| 前端双轨 | `js/`（传统 DOM 模块）+ `pages/`（React）+ `pages/reader/legacy/` |
| 文档 | 118 篇 / 12,375 行 |
| **ADR** | **4 篇 / 183 行** |
| 测试 | Python 666 + Rust 914 + 前端 81 |

24 万行代码对应 183 行 ADR——**决策密度比 1:1300**。新人接手时，面对的不是「一套设计」，而是「一套设计 + 三次重构的遗迹」。

**认知陷阱的具体证据**

1. **同一份代码，两种行为。** `services/ocr_provider/paddle_runner.py:39`：
   ```python
   import fitz  # deferred: desktop bundles no pymupdf; page-count progress degrades to None
   ```
   桌面端没有 pymupdf，所以页数进度「静默降级为 None」。这意味着**同一行代码在两种部署形态下行为不同，且差异只存在于注释里**。这是认知负载最阴险的形式——不是文档缺失，而是文档存在于某个文件的第 39 行。

2. **文档自相矛盾。** `doc/core/rust_api/14-Rust化实施状态.md:34` 称「锚点 4.1.10 darwin 实测 92.8MiB」，而 `bundle-manifest.json` 实测 **199.6 MiB**，差 2 倍。同文件 :36 又记录「剪包 293.7→189.9 MiB」。三处数字互相打架。

3. **代码注释与实现脱节。** `frontend/src/js/reader/ai/answer-enhance.ts:132`、`render-answer-html.ts:107` 注释仍写「桌面端 Electron setWindowOpenHandler」，测试断言文案也是「(Electron window.open 守卫)」——**实际外壳是 Tauri v2**。README 已修正，代码注释没有。

4. **过渡态标记泛滥。** 一级源码中：`fallback` 1182 处、`legacy` 442 处（`legacy` 大头是 CSS，但 `pages/reader/legacy/` 是整目录）、`兼容` 90 处、`过渡` 53 处、`兜底` 43 处、`shim` 12 处。每一个都需要读者判断「这段代码现在还生效吗」。

5. **测试无配置。** 无 `pytest.ini`、无 `[tool.pytest]` 段、无 coverage 配置。666 个测试能跑（已实测 666 passed / 28.5s），但**新写测试的人没有任何约定可循**。

**改进建议**

- **P0（最高 ROI）**：把 `fallback` / `legacy` / `兼容` / `过渡` / `兜底` 标记做一次全量审计，给每一处加标准注释头 `// TRANSITION(原因, 移除条件, 负责人)`，不能写明移除条件的一律删除。这能把 1182 处「需要读者猜」变成「答案写在旁边」。
- **P0**：修正文档数字打架（`doc/core/rust_api/14` vs `bundle-manifest.json`），把体积数字改为**脚本自动生成**，禁止手写。
- **P1**：前端双轨收敛。目标 `550 文件 → 250`。这不是重写，而是**给 `js/` 里的每个模块标注归属**（保留 / 迁移到 pages / 删除），然后按季度推进。参考已有先例 `7d49dd06 删除 frontend-react/ 冗余迁移区`——团队会做这件事，只是需要排期。
- **P1**：补 `pytest.ini` + coverage 门禁（建议阈值 60%，逐步提升）。
- **P2**：把「同一代码两种部署行为」的模式消灭——`paddle_runner.py` 的 fitz 延迟导入应改为显式 capability 检查，降级要在 UI 可见，不能是静默 None。

---

## 五、状态的确切性 — **5 / 10**

### 判定：翻译缓存堪称典范，核心 job 状态却是多真相

**亮点：翻译单元缓存（全项目最漂亮的一块代码）**

`services/translation/llm/shared/cache.py` 的设计值得单独表扬：

- **缓存 key 是 sha256，覆盖 12 个维度**：`model`、`base_url`（归一化）、`domain_guidance`、`mode`、`target_lang`、`target_language_name`、`prompt_hash`（8 个提示词文件内容的 sha256）、`translation_protocol_version`、`translation_policy_version`、`strategy_signature`、`translation_style_hint`、`translation_structure_kind`、`source_text`。
- **自动失效**：改提示词 / 改协议版本 / 改模型 / 改目标语言 → key 变化 → 自动失效。**不需要任何人工清理逻辑**，这是把失效策略编码进 key 的正确做法。
- **原子写**：`tmp-{pid}-{thread_ident}` + `os.replace`，「最后替换者胜」，无锁竞争。
- **读时自愈**：缓存文本被 sanitize 过会回写修正版；`math_mode=direct_typst` 且 `$` 定界符不配对则视为不命中。
- TTL 按 mtime 回收，默认 90 天，每进程最多清扫一次。

这一块单独拿出来可以打 9 分。它把「结果确定性」变成了**结构性的保证**而非约定。

**扣分：核心 job 状态四处分裂**

1. **`jobs` 表双写**：同时有顶层 `stage` / `progress_current` / `progress_total` 列 **和** `status_json` blob。同一语义两个真相，谁先谁后没有约束。
2. **事件双写**：DB `events` 表（PK `job_id,seq`，含 `elapsed_ms`）与文件侧 `logs/pipeline_events.jsonl`（`job_events/jsonl.rs::append_event_jsonl`）并存。
3. **产物双写**：`artifacts.artifacts_json` blob 与 `job_artifact_entries` 行级表并存。
4. **PID 复用竞态未消除**：`app/state_recovery.rs` 的注释**明确承认**「理论上的 PID 复用竞态（记录的 pid 可能已退出并被无关进程回收）」，靠 `setpgid` 把爆炸半径限制在被复用 pid 自身的进程组。这是**缓解，不是消除**——无 start-time 校验、无心跳、无租约。

5. **断点续跑无一致性门禁**：`stage_plan.rs::resume_plan()` 的判定依据是 `StageArtifactAvailability::from_job(job)`，即**目录/文件是否存在**。虽然 `job_artifact_entries` 表有 `checksum` 列，但**未用于 resume 时的一致性校验**。断电后残留半个 translated 目录 → resume 会认为「translations_available」→ 产出损坏结果。

   **这正是第五律要防的「状态漂移」**，而当前架构允许它发生。

6. **多实例部署不可能**：并发闸门是进程内 `tokio::sync::Semaphore`（`app/state.rs:42`），取消注册表是进程内 `HashSet`。状态是纯本地 SQLite，无外置接口。这意味着**水平扩展需要在架构层动刀**，不是加个 Redis 就能解决。

**改进建议**

- **P0**：`jobs` 表去双写。把 `stage` / `progress_*` 列变成 `status_json` 的**生成列或读取时投影**，物理上只存一份。
- **P0**：resume 加 checksum 门禁。`job_artifact_entries.checksum` 已经在了，只差在 `resume_plan()` 里校验。校验失败则降级到更早的 stage，而不是信任残缺产物。
- **P1**：PID 加 start-time 校验（`/proc/{pid}/stat` 的 field 22，或 `sysctl` KERN_PROC_PID）。这是消除竞态的标准做法，改动量不大。
- **P1**：事件流收敛到单一载体。建议保留 DB `events` 表（可查询、可聚合），让 jsonl 变成**导出产物**而非并行写入。
- **P2**：如果考虑多实例，先抽 `JobStore` trait（SQLite 实现 + Postgres 实现），把并发闸门从进程内 Semaphore 换成 DB 层的 `SELECT ... FOR UPDATE` 或 advisory lock。

---

## 六、依赖自主性与孤立性 — **4 / 10**

### 判定：Python 侧极其克制，Rust 侧和打包体积是负债

**加分：Python 依赖克制到令人尊敬**

```toml
# backend/packages/retainpdf-core/pyproject.toml
dependencies = ["PyMuPDF==1.26.5", "requests==2.32.5", "urllib3==2.5.0"]
```

**3 个直接依赖，全部 pin 死版本。** 对比同类项目的依赖地狱，这是真正的自律。而且：

- `paddleocr` 不是本地依赖——`paddle_api.py:21` 走远程 API `https://paddleocr.aistudio-app.com`。**用 HTTP 调用替代本地安装几百 MB 的 paddle 栈**，这是聪明的架构决策。
- 已剪除 `pikepdf` / `lxml` / `PIL` / `rendering_bridge`（`desktop/scripts/prepare-app.mjs:256-270`）。

**扣分**

1. **打包体积贴着天花板。** `bundle-manifest.json`（v4.1.10/darwin）：**199.6 MiB**，D5 门禁 `max_bytes=209715200`（200 MiB）——**只剩 5 MiB 余量**。构成：python 58.2 + fonts 51.6 + typst 38.7 + frontend 20.5 + rustApi 15.1 + renderRs 11.2。

   这意味着**任何一次依赖升级都可能撞墙**，而且字体占了 51.6 MiB——这是个需要专项优化的目标（子集化 / 按需下载）。

2. **Rust 传递依赖 495 个包**（`desktop/src-tauri/Cargo.lock`）。对比 `rendering_core` 的 20 个，说明重量集中在 Tauri 侧。

3. **重依赖本质未消除**：`PyMuPDF`（C 扩展）+ `mupdf-sys 0.8`（Rust 侧编译依赖）+ `typst` 外部二进制（`pyproject.toml:24` 声明 `required = ["typst"]`）。项目**离不开 mupdf 和 typst**，只是把 Python 侧的调用换成了 Rust 侧。

4. **许可证风险（需法务核实）**：项目自身是 **MIT**，但 `PyMuPDF==1.26.5` 采用 AGPL-3.0 / 商业双许可（Artifex）。**MIT 项目分发包含 AGPL 组件的二进制包，存在许可兼容性问题**。这一条我标注为「待核实」而非定论——但如果成立，它是第十律（50 年传承）和项目商业化的双重硬伤。

   注意：桌面包已剪除 pymupdf，但 `retainpdf-core` 的 `dependencies` 里仍是硬依赖，且 `backend/scripts` 中仍有 7 处 `import fitz`。**服务端路径仍在用**。

**改进建议**

- **P0**：核实 PyMuPDF 许可证对 MIT 项目分发的影响。若成立，评估切换到 Apache-2.0 的 PDF 库（`pdfium-render` / `lopdf`，注意 `upload.rs:110-124` 已经在用 lopdf 做解析，只有 repair 路径用 fitz）。
- **P1**：字体瘦身专项。51.6 MiB → 目标 20 MiB（子集化 + 按需下载 + 系统字体回退）。这能一次性释放 30 MiB 余量。
- **P1**：把 D5 的 200 MiB 门禁**下调**到 170 MiB，用收紧的门禁倒逼瘦身，而不是等撞墙。
- **P2**：`typst` 外部二进制依赖 → vendored crate（`typst` 本身是 Rust，可直接作为 crate 依赖）。能消掉一个运行时外部依赖，顺便减小体积。

---

## 七、对抗性韧性 — **5 / 10**

### 判定：架构防腐做得聪明，但测试覆盖有致命缺口

**亮点：把架构约束变成 CI 门禁（这是第七律的正确答案）**

三个检查脚本进入 CI 强制执行：

- `backend/rust_api/scripts/check_architecture.py`
- `backend/scripts/devtools/check_pipeline_architecture.py`
- `check_stage_specs_contract.py`

由 `rust-api-architecture.yml` 强制。这意味着**「Python 不许越过边界」「stage spec 契约不许漂移」不是写在文档里的倡议，而是会红的流水线**。第七律要求「安全和健壮性是架构的副产品」——用机器验证架构边界，就是这句话的最佳实践。全项目这一处设计值得打 9 分。

其他正面证据：

- 路径遍历防护已实装：`sanitize_upload_filename`（`upload.rs:26-43`）拒绝 `\0` / `/` `\` / `.` `..`；历史上确实出现过漏洞并已修复（`300e084c 修复上传文件名路径遍历漏洞`）。
- 复杂度预算设计正确（页数 × 对象数，u128 防溢出）——**但见第三律，默认关闭**。
- 前端硬编码上限：`FRONT_MAX_BYTES = 50MB`、`FRONT_MAX_PAGE_COUNT = 999`。
- 其他配额：`MAX_GLOSSARY_ENTRIES 200`、`MAX_CHUNK_CHARS 1600`、`MAX_ASSET_BYTES 20`、`LOG_TAIL_LIMIT 40`。
- 历史上主动修过 `def9625e 为外部二进制调用增加超时并移除 shell=True`。

**致命缺口**

1. **92 个翻译测试文件不在 CI 内。** Python 有 666 个测试（`devtools/tests/translation` 92 个文件 + `document_schema` 29 个），但 `python-pipeline-tests.yml` **只跑 `document_schema/` 一个目录**。翻译是本项目最复杂、最易回归、且涉及外部 API 的部分，却恰恰没有 CI 保护。

   **这是当前投入产出比最高的一项改进——改一行 yaml。**

2. **无 coverage 配置**，无法知道 666 个测试到底覆盖了什么。

3. **无沙箱 / 资源配额**（见第三律）。`upload.rs:110-124` 在 lopdf 解析失败时会调 PyMuPDF 做 repair，**这条路径无任何资源限制**——恶意构造的损坏 PDF 可以精准触发这条路径。

4. **`rendering_reader` 测试极薄**：Rust 侧 914 个 `#[test]` 中，reader 只有 **4 个**（对比 rendering_core 315 个）。而 reader 是解析不可信输入的第一道关卡。

**改进建议**

- **P0（一行改动）**：`python-pipeline-tests.yml` 把 `devtools/tests/translation/` 纳入。外部 API 依赖用 mock 或录制回放（项目已有 `translation-replay.yml` 的基础设施）。
- **P0**：给 PyMuPDF repair 路径加资源限制（超时 + 内存上限 + 输出大小上限）。这是当前最具体的攻击面。
- **P1**：补 coverage 配置，先设 50% 门禁，逐步提到 70%。
- **P1**：`rendering_reader` 补 fuzz 测试（`cargo-fuzz`）。PDF 解析器不做 fuzz 是行业公认的失职。

---

## 八、维护不变性 — **4 / 10**

### 判定：内核尚未定型，但减法是健康的

**现状**

582 次提交中，最近 200 次里：

- `feat` 68 / `docs` 53（26.5%）/ `refactor` 14 / `fix` 13 / `test` 5 / `ci` 3 / `chore` 3
- 含「剪除/退役/收编/删除/清理/移除/冗余/死/prune」关键词：**29/200 ≈ 14.5%**
- 窄口径（refactor+remove+cleanup+delete+prune）：**18/200 = 9%**
- 另有 40 条非规范前缀的中文游离格式 commit（如「前端:…」）——**commit 规范执行不严格**

全库口径：refactor 36 次、删除 13 次、移除 9 次、清理 7 次。

代表性清理提交（这些不是坏事，是团队能力的证明）：

```
7d49dd06  删除 frontend-react/ 冗余迁移区
a25c8536  C4·R 发布包去 pyo3 bridge 化 + 剪除渲染依赖至 189.9 MiB
78cd461d  剪除 pymupdf/pikepdf/lxml
552c1318  fitz 收编 45 项 native-only 收口 + delegate 退役
9847e04d  删除 Python 渲染树 + rendering_bridge，render 全量 native
08a52186  render 路由 100% render_rs，退役 command-provider OCR
```

**为什么只给 4 分**

第八律的判据是「写完即永恒」。RetainPDF 目前**明确不满足**——它正处于活跃的架构收敛期，内核还在移动。这不是缺陷，是阶段特征。但评分必须反映事实。

反过来，14.5% 的返工率有正面解读：**团队在主动做减法，且每次减法都有据可查**。很多项目的问题不是返工多，而是返工了却不记录、或者根本不敢删。RetainPDF 的记录是完整的。

**扣分点在 `docs` 占比 26.5%**：近 200 次提交里 53 次是文档，超过 feat 之外的所有类别。结合第四律「文档 12375 行但 ADR 只有 183 行」——说明**文档在膨胀，但决策记录没有跟上**。文档量增长不解决问题，ADR 才解决。

**改进建议**

- **P0**：制定「内核冻结清单」。明确哪些模块（document.v1 schema / stage spec 契约 / consumer_reader accessor / translation cache key 构造）进入 **frozen 状态**——改动需 ADR + 全量回归，而不是随手改。内核静默是第八律的前提。
- **P0**：ADR 从 4 篇扩到覆盖所有重大决策。建议最低清单：5 个 crate 的拆分理由、pyo3 → 子进程的转向、前端 js/pages 双轨的处置方案、PyMuPDF 依赖策略、typst 选型。这 5 篇写完，第十律能直接从 4 分跳到 6 分。
- **P1**：commit 规范强制化（`.githooks/pre-commit` 已存在，加 conventional commit 校验即可）。40 条游离格式 commit 会影响自动化 changelog 和 bisect。

---

## 九、非对称经济优势 — **7 / 10**

### 判定：护城河真实存在，但贡献者门槛过高

**说明**：十律原为框架设计，RetainPDF 是应用产品。此处换算为——**它是否让用户/团队获得了 10 倍级的替代优势？**

**成立的证据**

README 的竞争矩阵虽是自述，但技术差异是可验证的：

| 能力 | 证据 |
|---|---|
| 扫描型 PDF | 走 OCR provider pipeline，非文本层提取 |
| 复杂行内公式 | `rendering_core/src/inline_content/inline_math.rs`（946 行）+ `latex_normalizer.rs`（1711 行）——**这是真投入** |
| 代码不误翻 | document.v1 的 `semantic_role` + `policy.translate` 决策链 |
| 表格控制 | `structure_role` + 可开关 |
| 自定义翻译策略 | `policy_hints_v2` + `domain_guidance` 进 cache key |

其中 **1711 行的 LaTeX normalizer + 946 行的 inline math 处理**，是这个项目最难被复制的部分。竞品做不好行内公式，RetainPDF 用近 2700 行专门代码去解决——这就是护城河的物理形态。

**经济杠杆**

- **翻译单元缓存**直接省钱：重复内容、术语、重试、多语言版本都不重复调用 LLM。key 覆盖 12 维度意味着**缓存命中率高且不会返回错误结果**。这是把成本模型从「按页数付费」推向「按唯一内容付费」。
- **一人/小团队做出 24 万行全栈**（含自研 Rust 渲染引擎），本身就是非对称优势的体现。
- 交付形态覆盖完整：桌面端（Tauri）+ Docker + API 自动化。

**扣分**

- **贡献者门槛极高**：4 种语言 + 5 个 crate + 495 个 Rust 传递依赖 + 199.6 MiB 打包。新人从 clone 到跑通需要配置 Python 3.11 + Rust + Node + typst 二进制。**这直接限制了社区贡献的规模**，而社区贡献是开源项目第九律的放大器。
- **打包体积 199.6 MiB** 对用户也是成本（下载、磁盘）。

**改进建议**

- **P1**：做一份「15 分钟跑通」的一键开发环境脚本（`.venv` + cargo + npm + typst 自动下载）。这是**提升社区贡献率最便宜的投资**。
- **P1**：README 补一节「架构导览」——新人应该先读哪 3 个文件（`document.v1.schema.json` → `consumer_reader.py` → `pipeline_plan.rs`）。24 万行项目没有导览图，新人流失率会很高。
- **P2**：把 `rendering_core`（零 mupdf 依赖，仅 20 个传递依赖）单独发布为 crate。它是全项目最可复用、最干净的部分，独立发布能吸引外部贡献，也是第十律（文明资产）的实际起步。

---

## 十、时间抗性与文明传承 — **4 / 10**

### 判定：有文明资产的种子，但传承机制缺失

**正面**

`document.v1` 具备成为长期资产的潜质：

- 它是**问题的本质抽象**，不是某个库的封装。PDF 保留排版翻译的核心难题——「如何描述一页文档的语义与几何，使得翻译后能重建排版」——不会因为技术栈更迭而失效。
- ADR 0001 记录了它为什么存在，包含被否决的替代方案。**50 年后的人读到这篇 ADR，能理解当时的权衡**。这是传承的正确形式。
- 有 JSON Schema + 版本号 + 唯一 accessor。数据格式的可移植性远高于代码。
- `rendering_core` 零 mupdf 依赖、仅 20 个传递依赖，**这一层是真正可移植的**。

**负面**

1. **ADR 严重不足**。24 万行代码对 4 篇 ADR（183 行）。**50 年后的人能读懂 document.v1，但读不懂为什么有 5 个 crate、为什么从 pyo3 转向子进程、为什么前端有两套体系。** 代码能跑，决策失传——这就是「技术快餐」与「文明资产」的分界线。

2. **三重运行时负债**：Python 3.11 被打包进桌面端（58.2 MiB）+ Rust 二进制 + Node 前端。在 50 年尺度上，**依赖特定解释器版本的分发方式是负债**。TeX 之所以能活 50 年，部分原因在于它只依赖极少的运行时假设。

3. **外部二进制依赖 `typst`**（38.7 MiB）。外部依赖的生命周期不由本项目控制。

4. **许可证不确定性**（见第六律）。AGPL 组件与 MIT 项目的组合，如果成立，会直接影响 50 年尺度上的可自由再分发性。

**改进建议**

- **P0**：ADR 补全（见第八律清单）。这既是第八律的改进项，也是第十律的改进项——**一份投入，两条律同时提分**。
- **P1**：把 document.v1 的 JSON Schema **独立发布**（独立仓库 / 独立版本号 / 独立 semver）。数据格式的生命远长于实现它的代码。Git 的对象格式、SQLite 的文件格式都是这样活下来的。
- **P2**：`rendering_core` 独立发布为 crate（见第九律）。
- **P2**：明确「最小可移植核心」的边界——哪部分代码不依赖 mupdf / typst / 特定 Python 版本？把这个边界写进 ADR 0005，并让架构检查脚本守护它。

---

## 改进路线图

### P0 — 立即（1–2 周，低风险高收益）

| # | 动作 | 文件 | 验收判据 | 提分 |
|---|---|---|---|---|
| 1 | 翻译测试进 CI | `.github/workflows/python-pipeline-tests.yml` | 666 个测试全在 CI 内 | 七 5→7 |
| 2 | 复杂度预算默认值改为启用 | `rust_api/src/config/upload.rs` | 三个 `MAX_*` 非 0，`unlimited()` 仅测试可见 | 三 5→6 |
| 3 | 渲染子进程加硬超时 | `worker_process.rs` | 300s wall-clock，SIGTERM→SIGKILL | 三 →6 |
| 4 | 修正文档数字打架 | `doc/core/rust_api/14-*.md` | 体积数字改为脚本生成 | 四 3→4 |
| 5 | 核实 PyMuPDF 许可证 | 全项目 | 出具结论文档 | 六/十 风险闭环 |
| 6 | 制定内核冻结清单 | 新增 ADR 0005 | 4 个模块进入 frozen | 八 4→5 |

### P1 — 本迭代（1–2 月）

| # | 动作 | 文件 | 验收判据 | 提分 |
|---|---|---|---|---|
| 7 | 消灭 DeepSeek 语义泄漏 | `aggregator.py` / `control_context.py` / `common.py` / `batched_plain_request.py` | `provider_family` 改 capability 查询，4 处特判清零 | 二 6→7 |
| 8 | jobs 表去双写 | `rust_api/src/db.rs` | `stage`/`progress_*` 改为投影 | 五 5→6 |
| 9 | resume 加 checksum 门禁 | `stage_plan.rs` | 残缺产物降级而非信任 | 五 →6 |
| 10 | 补 5 篇关键 ADR | `doc/adr/` | crate 拆分 / pyo3 转向 / 前端双轨 / PyMuPDF / typst | 八→6 十 4→6 |
| 11 | 输出格式 trait 化 | `rendering_orchestrator/src/run.rs` | match 改注册表遍历 | 二 →7 |
| 12 | 前端双轨收敛启动 | `frontend/src/` | 每个模块标注归属，550→目标 250 | 四 →5 |
| 13 | 字体瘦身专项 | `backend/fonts` | 51.6 MiB → 20 MiB | 六 4→5 |
| 14 | 一键开发环境脚本 | 新增 | 15 分钟跑通 | 九 7→8 |
| 15 | reader 补 fuzz 测试 | `rendering_reader` | cargo-fuzz 接入 CI | 七 →7 |

### P2 — 战略（3–6 月）

| # | 动作 | 验收判据 | 提分 |
|---|---|---|---|
| 16 | document.v1 三套 role 轴正交性论证（或合并） | ADR + schema 演进 | 一 7→8 |
| 17 | role 判定 golden 测试向量（Python/Rust 共享） | CI 守护双实现不漂移 | 一→8 五→7 |
| 18 | document.v1 schema 独立发布 | 独立仓库 + semver | 十 →7 |
| 19 | `rendering_core` 独立发布为 crate | 发布到 crates.io | 九→8 十→7 |
| 20 | typst 从外部二进制改为 vendored crate | 消掉外部依赖 | 六→6 十→7 |
| 21 | 大 PDF 分页流式 + Python worker 长驻池 | 内存曲线持平 | 三 →7 |
| 22 | `JobStore` trait 抽象（多实例准备） | SQLite + Postgres 双实现 | 五 →7 |

### 预期收益

| 阶段 | 综合分 | 定位 |
|---|---|---|
| 当前 | **5.0** | 工具级 |
| P0 完成 | 5.6 | 工具级偏上 |
| P0+P1 完成 | **6.4** | **工业级** |
| P2 完成 | **7.3** | 工业级上游，接近神作 |

**关键提醒**：从 5.0 到 6.4 的路径上，**没有一项是加功能，全部是减法与收敛**。这符合第八律的精神——顶级框架的内核是静默的。RetainPDF 的种子（document.v1）已经足够好，现在需要的是停止在外壳上继续生长，把养分还给内核。

---

## 附：评估方法说明

- 评分基于代码实证，非主观印象。所有数字来自实际文件统计、git 历史分析、测试实跑（666 Python + 914 Rust + 81 前端测试全绿）。
- 「待核实」项（PyMuPDF 许可证）已明确标注，未经法务确认前不作为定论。
- 第九律针对应用产品做了换算说明，未直接套用框架标准。
- 本报告为一次性快照，建议每季度重评一次，用分数变化验证改进是否真实落地。
