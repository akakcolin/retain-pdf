# RetainPDF AI Runtime 设计

**状态：** 设计草案 v0.1  
**日期：** 2026-07-21  
**范围：** `backend/rust_api/src/services/ai/`（agent 循环已并入 Rust）及与前端 / LLM 的契约  
**非范围：** OCR/翻译流水线；具体 LLM 供应商锁定

配套：

- [SESSION_AND_MEMORY.md](./SESSION_AND_MEMORY.md) — 多轮与压缩  
- [SKILLS.md](./SKILLS.md) — Skill 包  

---

## 1. 动机

当前 `RetrievalAgent` 足够支撑「整本检索 + 引用跳转」，但产品路线还需要：

| 能力 | 为什么现在就设计 |
|------|------------------|
| **Skills** | 文献问答 / 批注助手 / 多文对比… 不能全塞进一个 system prompt |
| **工具调用** | 已有 function calling；要版本化、权限域、预算、可观测 |
| **上下文压缩** | 多轮后 `history[-12:]` 会爆 token 且丢证据结构 |
| **多 Agent** | 检索与写作拆分、可选 critic；避免单循环无限膨胀 |

约束（不可破）：

1. **Rust 是数据面单写入者**（documents / FTS / conversations / favorites）。  
2. **AI 服务无状态优先**：可重启；会话持久化落 Rust。  
3. **本地单用户**优先：延迟与可控性 > 云端 agent 平台完整度。  
4. **工具 schema 与 OpenAI-compatible tools 同构**，便于换循环外壳。

---

## 2. 分层架构

```
┌─────────────────────────────────────────────────────────────┐
│  Frontend (reader AI panel)                                 │
│  SSE: tool / answer_delta / compress / handoff / done       │
└───────────────────────────┬─────────────────────────────────┘
                            │ POST /api/v1/ai/ask  (Rust 直调)
┌───────────────────────────▼─────────────────────────────────┐
│  Transport  routes/ai.rs                                    │
│  鉴权 · SSE · 请求校验 · conversation_id 透传               │
└───────────────────────────┬─────────────────────────────────┘
                            │
┌───────────────────────────▼─────────────────────────────────┐
│  Orchestrator  (后期；v0 可退化为「直接跑默认 skill」)        │
│  选 skill / 是否多 agent / 何时收尾                         │
└───────┬─────────────────────┬───────────────────────────────┘
        │                     │
┌───────▼───────┐   ┌─────────▼─────────┐
│ Session/Memory│   │ RunBudget         │
│ 窗口·摘要·证据 │   │ 轮数·token·墙钟   │
└───────┬───────┘   └─────────┬─────────┘
        │                     │
┌───────▼─────────────────────▼─────────┐
│  Agent Runtime(s)                     │
│  通用 tool loop · 事件发射 · 中止条件  │
└───────┬───────────────────────────────┘
        │
┌───────▼───────────────────────────────┐
│  Skills  →  Tools                     │
│  声明式能力包    原子动作               │
└───────┬───────────────────────────────┘
        │
┌───────▼───────────────────────────────┐
│  Data plane (只读自 AI 视角)            │
│  Rust HTTP · job 目录 md/ocr/translated│
└───────────────────────────────────────┘
```

| 层 | 职责 | 现状 | 目标 |
|----|------|------|------|
| Transport | HTTP/SSE、Key | `routes/ai.rs` | 保持薄；事件类型可扩展 |
| Session/Memory | 多轮、压缩 | `services/ai/memory.rs`（窗口 + 压缩已落地） | evidence 包跨轮 |
| Orchestrator | 路由/协作 | 无（单 agent） | skill 选择 → 可选 multi-agent |
| Runtime | 工具循环 | `services/ai/agent.rs` | 抽成可复用 loop |
| Skills | 策略+提示+工具子集 | 硬编码 SYSTEM_PROMPT（Rust 常量） | 目录化 skill 包 |
| Tools | 原子 I/O | `services/ai/tools.rs` | 加 scope/timeout/版本 |
| Evidence | 引用/图 | Rust citations 模型 | 统一协议，前端可跳可渲 |

---

## 3. 核心对象（逻辑模型）

### 3.1 Run

一次用户提问触发的执行单元（可多轮工具、可跨 agent）。

```text
Run
  run_id            运行时生成（日志/SSE 关联）
  conversation_id   可选，持久会话
  skill_id          默认 literature-qa
  scope             { document_id?, job_id? }
  budget            RunBudget
  status            running | done | error | cancelled
  events[]          可观测轨迹
  result            answer + evidence + usage
```

### 3.2 RunBudget

```text
RunBudget
  max_tool_rounds      默认 6（现有 RETAIN_AI_MAX_TOOL_ROUNDS）
  max_wall_time_s      建议 120
  max_input_tokens     建议按模型窗口 60%
  max_tool_calls       建议 24
  max_evidence_items   建议 32（压缩时保留上限）
```

耗尽时：强制收尾轮（现有「请基于已有证据回答」行为保留）。

### 3.3 EvidenceItem（统一证据）

前端跳转、插图、引用脚注都吃这一种形状：

```text
EvidenceItem
  ref               int          # 对用户可见的 [n]
  kind              text | image | page_preview | favorite
  document_id
  job_id
  page_idx          0-based
  block_id? 
  snippet?          短摘录
  image_url?        /api/v1/jobs/.../markdown/images/...
  preview_url?      /api/v1/jobs/.../preview/pages/{1-based}
  source_tool       search_fulltext | read_blocks | ...
  created_round     int
```

`citations[]` 是 `EvidenceItem` 中 `kind=text`（及被回答引用到的）子集的视图。

### 3.4 Transcript 消息（会话存储）

见 [SESSION_AND_MEMORY.md](./SESSION_AND_MEMORY.md)。关键点：除 `user`/`assistant` 外，允许 **`system_summary`** 与 **`evidence_snapshot`** 元数据字段（可落在 assistant 消息的 JSON 扩展里，或独立 message kind）。

---

## 4. 事件流（SSE 契约）

向后兼容现有类型；新增可选类型前端可忽略。

| type | 何时 | payload 要点 |
|------|------|----------------|
| `tool` | 工具调用前后 | `tool`, `round`, `arguments?`, `status?` |
| `answer_delta` | 最终回答流式 | `text` 增量或累积（**实现须固定一种**；现状为累积全文） |
| `compress` | 压缩发生 | `dropped_turns`, `summary_chars`, `kept_evidence` |
| `skill` | skill 切换/加载 | `skill_id`, `phase: start\|end` |
| `handoff` | agent 交接 | `from`, `to`, `reason` |
| `done` | 成功结束 | `answer`, `citations`, `tool_trace`, `rounds`, `usage?`, `memory?` |
| `error` | 失败 | `message`, `code?` |

**兼容规则：** 旧前端只认 `tool` / `answer_delta` / `done` / `error` 即可。

---

## 5. Skills 与 Tools（边界）

```text
Tool  = 原子动作（有 schema、可单测、可审计）
Skill = 工具子集 + system/developer 提示 + 策略（scope 锁、输出格式、是否允许 list_documents）
```

详见 [SKILLS.md](./SKILLS.md)。

首发 skill：

| skill_id | 用途 | 工具 |
|----------|------|------|
| `literature-qa` | 阅读器整本问答（当前行为） | search_fulltext, read_blocks, search_favorites（scoped） |

后续候选：`annotation-assist`、`paper-compare`、`glossary-extract`。

---

## 6. 多 Agent（Phase D，接口先占位）

**v0 / v1 不强制 multi-agent。** 接口预留：

```text
AgentRole
  id: retriever | analyst | critic
  skill_id or tool_allowlist
  model_override?

Handoff
  from_role → to_role
  payload: { evidence_refs[], question, notes }
```

推荐演进：

1. **单 Runtime + literature-qa**（现在）  
2. **流水线** Retriever → Analyst（同 evidence，不同 prompt）  
3. **可选 Critic** 检查「无 [n] 的断言」  
4. 再考虑并行 fan-out（多文档）

编排器用简单状态机即可，不必先上图执行引擎。

---

## 7. 包结构目标

**现状（2026-08-31）：** 循环已并入 Rust，Python 包结构不再成立。

```text
backend/rust_api/src/services/ai/
  mod.rs                 # 模块出口
  llm.rs                 # 直连 LLM chat/completions + Chat trait
  agent.rs               # 工具循环 + 引用编号
  tools.rs               # 原子工具注册表
  ask.rs                 # 编排：解析、会话落库、记忆窗口、SSE
  memory.rs              # 抽取式压缩 + 窗口组装
  blocks.rs              # 任务产物块级读取
  routes/ai.rs           # POST /api/v1/ai/ask SSE 路由
```

`skills` / `orchestrator` / multi-agent 仍为设计态（见 §6），未实现。

---

## 8. 与 Rust / 前端边界

### Rust

- 继续：`/api/v1/ai/ask` 代理、conversations CRUD、messages 追加  
- 扩展（B 需要）：消息可带 `metadata_json`（summary / evidence_snapshot / skill_id）  
- AI **不**直接写 SQLite  

### 前端

- 传：`question`, `document_id`/`job_id`, `conversation_id`, `stream`, LLM 凭据  
- 消费：SSE + `citations` + 图片 hydrate（已做）  
- 后期：展示 compress 提示、skill 名、多会话列表（可复用 Rust conversations）

### AI 服务

- 读：Rust search/documents/favorites + job 目录  
- 写：仅经 Rust append conversation messages  

---

## 9. 安全与策略

| 策略 | 说明 |
|------|------|
| Document scope | 阅读器会话强制 `document_id`；工具层注入（现有 `_scope_tool_arguments`） |
| 禁止隐式全库 | 有 job 无 document 时 fail closed（现有） |
| Tool 副作用 | v1 tools 全部只读；写操作（改收藏等）需显式 skill + 确认 |
| 密钥 | LLM key 可请求级下发；不写 job snapshot / 不回显 |
| 引用诚实 | 系统提示要求事实带 [n]；可选 critic 后置检查 |

---

## 10. 测试策略

| 层 | 测什么 |
|----|--------|
| tools | schema、handler 纯函数、image_urls 路径 |
| runtime loop | mock chat_fn：工具轮 → 收尾轮 → budget 耗尽 |
| memory | 窗口裁剪、摘要替换后 token 下降、evidence 保留 |
| app SSE | 事件顺序、done 含 citations |
| 契约 | OpenAPI/示例与前端 mock 一致 |

不强制 e2e 真打 DeepSeek；mock chat 即可。

---

## 11. 分阶段落地（PR 粒度）

| Phase | 交付 | 用户可见 |
|-------|------|----------|
| **C** | 本文档集 | 无 |
| **B1** | Session 协议 + 前端 `conversation_id` 贯通 | 多轮有记忆 |
| **B2** | Memory 压缩管道 + `compress` 事件 | 长聊不爆、可提示「已压缩」 |
| **S1** | Skill 加载 + literature-qa 外置 | 行为近似，可热加 skill |
| **D0** | Orchestrator 占位 + 可选 analyst 拆分 | 回答质量/结构提升 |

每阶段保持 `/v1/ask` 兼容；废弃路径给一个小版本窗口。

---

## 12. 刻意不做（本阶段）

- 绑定某一 agent 框架为唯一实现  
- AI 服务本地写业务库  
- 无限多 agent 无 budget 对话  
- 前端再实现一套 tool 协议  
- 云端多租户路由（非当前产品形态）

---

## 13. 决策记录（开放问题）

| ID | 问题 | 倾向 | 状态 |
|----|------|------|------|
| D1 | `answer_delta` 传增量还是累积？ | **固定累积全文**（与现实现一致），文档写死 | 建议批准 |
| D2 | summary 存在哪？ | assistant 旁路 `metadata_json` 或 kind=`summary` 消息 | 见 Session 文档 |
| D3 | 压缩用 LLM 还是抽取式？ | v1 **抽取式**（引用+问题关键词）；v2 可选 LLM 摘要 | 建议批准 |
| D4 | multi-agent 默认开吗？ | 默认关；feature flag / skill 配置 | 建议批准 |

---

## 14. 参考代码锚点

| 路径 | 角色 |
|------|------|
| `backend/rust_api/src/services/ai/agent.rs` | 现循环 / 引用编号 |
| `backend/rust_api/src/services/ai/tools.rs` | 原子工具 |
| `backend/rust_api/src/routes/ai.rs` + `services/ai/ask.rs` | SSE / history / persist |
| `backend/rust_api/src/services/ai/llm.rs` | 直连 LLM |
| `frontend/.../use-reader-ask-runtime.ts` | 前端消费 ask |
| `frontend/.../answer-enhance.ts` | 引用跳转与图 |
