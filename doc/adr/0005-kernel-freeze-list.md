# 0005 内核冻结清单（Kernel Freeze List）

## 背景

十律架构评估（2026-09-01，`doc/review/ten-laws-assessment-2026-09.md`）指出：RetainPDF 的核心价值与护城河集中在少数「内核」抽象上，而非外围功能。这些内核是：

- `document.v1` 中间表示（schema + 版本常量 + 唯一访问层 `consumer_reader.py`）；
- 6 个 stage spec 契约（`normalize / extract_text_layer / translate / render / provider / book`）；
- `rendering_core` 的公开领域原语（typography / payload / text_flow / toc，零 mupdf 依赖）；
- 翻译单元缓存的 key 构造（`services/translation/llm/shared/cache.py`，覆盖 12 维）；
- 5 个 Rust crate 的对外二进制契约（`render_rs --spec` 输入输出形状）。

但全仓 24 万行代码仅对应 4 篇 ADR（183 行），**决策未被固化**。新人、甚至资深贡献者都无法从文档判断「哪些改动是高风险的、必须经过严格评审的」。结果：内核在不知不觉中被动漂移——例如 `aggregator.py:22-25` 仍按 `base_url` 字符串推断 `provider_family`，`contract.rs` 与 `contract_v1.py` 存在双实现。

第八律（维护不变性）要求「写完即永恒」，第十律（时间抗性）要求决策可传承。两者都依赖一个前提：**先明确「什么不该动」，再谈怎么动**。

## 决策

建立**内核冻结清单**，以下模块进入 `frozen` 状态：

| 冻结对象 | 范围 | 冻结强度 |
|---|---|---|
| `document.v1` schema | `document.v1.schema.json` 必填字段、版本号、校验规则 | 破环级：任何字段语义变更需 ADR + 全量回归 |
| `consumer_reader.py` accessor | 所有下游读取统一经此层，禁止绕过读 raw | 破环级：新增读取路径须经此层 |
| stage spec 契约 | 6 个 `*.stage.v1` 的字段集合与 stdout 标签格式 | 破环级：字段增删需 ADR |
| `rendering_core` 公开 API | crate 对外的 `pub` 领域原语 | 兼容级：可增不可改签名语义 |
| 翻译缓存 key 构造 | `cache.py` 的 12 维 key 组成 | 破环级：key 组成变更须保证旧缓存可识别失效 |
| `render_rs --spec` 形状 | 输入 spec 与输出 `pipeline_summary.json` | 兼容级：可增字段，不可删/改语义 |

**改动流程**：对 `frozen` 对象的任何改动，必须：

1. 先写 ADR（背景 / 决策 / 后果 / 替代方案），说明为何必须突破冻结；
2. 关联全量回归范围（Python 666 + Rust 914 + 前端 81 测试，外加 `rendering-parity.yml` 差分 replay）；
3. 在 PR 标题或标签标注 `kernel-freeze-break`，由至少一名维护者显式审批。

非 `frozen` 的外围模块（前端 UI、CLI 封装、provider 适配、devtools）按常规流程改动，不受此约束。

## 后果

- 内核获得「静默」特性：第八律要求的「写完即永恒」有了机制保障，而非靠个人纪律。
- 新人接手时有明确边界：改 `consumer_reader` 比改 `frontend/` 风险高，文档说清楚了。
- 评审成本可控：常规 PR 不被冻结约束，仅内核突破需额外论证，避免「处处是重点 = 没有重点」。
- 代价：内核演进变慢。这是刻意的权衡——内核稳定带来的复利远大于演进速度。

## 验证

- `rust-api-architecture.yml` 已强制 `check_architecture.py` / `check_pipeline_architecture.py`，冻结边界可由机器校验（契约级）。
- 后续可新增 CI 检查：扫描 `document.v1.schema.json` 与 `consumer_reader.py` 的 `git diff`，非 `kernel-freeze-break` 标记的改动若触及冻结区则阻断。
- 本清单本身纳入 `doc/adr/README.md` 索引，确保被发现。

## 替代方案

- **不冻结（当前状态）**：内核随业务需求随意漂移，短期快，长期侵蚀护城河与可传承性。已观察到 `contract.rs` 双实现、DeepSeek 语义泄漏等漂移症状。
- **全仓冻结**：所有改动都需 ADR，评审成本爆炸，不现实。故只冻结「极少数决定价值的内核」。
- **仅文档提示**：在 README 写「请勿改动内核」。无强制力，等同于不冻结。故采用「文档 + CI 门禁 + 评审标签」三重约束。
