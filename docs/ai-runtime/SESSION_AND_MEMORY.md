# Session 与上下文压缩（B 草案）

**状态：** API / 数据形状草案 v0.1 · **B1 + B2 已落地**  
**日期：** 2026-07-21  
**依赖：** [AI_RUNTIME.md](./AI_RUNTIME.md)  
**目标：** 多轮真正可用；长聊不爆上下文；证据（引用/图）跨轮可复用  

### B1 落地摘要（实现）

| 项 | 位置 |
|----|------|
| AI auto-create + `done.conversation_id` | `backend/rust_api/src/services/ai/ask.rs` |
| 读历史 / 追加消息 | `services/ai/ask.rs` + `services/library/conversations.rs` |
| 前端粘性存储 | `frontend/src/js/reader/ai/conversation-store.ts` |
| ask 传/收 conversationId | `api/ai.ts` + `ask-answerer.ts` |

### B2 落地摘要（实现）

| 项 | 位置 |
|----|------|
| 抽取式压缩 `extractive_v1` | `backend/rust_api/src/services/ai/memory.rs` |
| 窗口组装 | `backend/rust_api/src/services/ai/memory.rs` |
| SSE `compress` + `done.memory` | `services/ai/ask.rs` + `routes/ai.rs` |
| 配置 | `RETAIN_AI_MEMORY_WINDOW_TURNS` 等（见 `config/ai.rs`） |
| 摘要落库 | assistant 消息，正文以 `【对话摘要】` 开头 |


---

## 1. 现状与缺口

### 1.1 已有

| 能力 | 位置 |
|------|------|
| Rust 会话 CRUD | `/api/v1/ai/conversations` |
| 消息追加 | `.../messages`（user/assistant + citations_json + tool_trace_json） |
| AI 读历史 | `load_history` → **最近 12 条** `role+content` |
| AI 回写 | `persist_turn` 写 user + assistant |

### 1.2 缺口

1. 前端阅读器 **常不传 / 不创建 `conversation_id`** → 实际多轮无状态。  
2. 历史 **只塞原文**，无摘要、无 evidence 包 → 长了既贵又丢结构。  
3. `tool_trace` 落库但 **不回灌模型**（正确，但需要别的形式保留证据）。  
4. 无 **压缩事件**，用户不知道「早期轮次被摘要了」。  
5. 无统一 **memory 视图**（给 runtime 的 `messages[]` 与给存储的 transcript 未分层）。

---

## 2. 概念分层

```text
Transcript（持久化，Rust）
  = 用户可见的完整对话记录（可含 summary 消息）

MemoryView（运行时，AI 内存中拼装）
  = 本轮喂给 LLM 的 messages[]
  = f(Transcript, EvidenceStore, CompressPolicy)

EvidenceStore（运行时 + 可选快照落库）
  = 本会话累积的 EvidenceItem（按 ref 或按 content hash）
```

原则：

- **Transcript 求真**（可回放 UI）  
- **MemoryView 求省**（可截断、可替换为 summary）  
- **Evidence 求稳**（[ n ] 与锚点跨轮尽量稳定）

---

## 3. 数据形状

### 3.1 Conversation（Rust，已有可扩展）

```json
{
  "conversation_id": "conv_...",
  "document_id": "doc_...",
  "job_id": "2026...",
  "title": "可选自动标题",
  "skill_id": "literature-qa",
  "created_at": "...",
  "updated_at": "..."
}
```

扩展字段（建议）：

| 字段 | 说明 |
|------|------|
| `document_id` / `job_id` | 会话默认 scope（阅读器创建时写入） |
| `skill_id` | 默认 skill |
| `memory_json` | 可选：压缩状态 `{ "summary": "...", "through_message_id": "..." }` |

### 3.2 Message（Rust）

现有大致：`role`, `content`, `citations_json`, `tool_trace_json`, `model`, timestamps。

**建议扩展 `metadata_json`（对象序列化）**：

```json
{
  "kind": "turn | summary | system_note",
  "run_id": "run_...",
  "skill_id": "literature-qa",
  "evidence_refs": [1, 2, 5],
  "usage": { "prompt_tokens": 0, "completion_tokens": 0 },
  "compress": {
    "covers_message_ids": ["m1", "m2"],
    "policy": "extractive_v1"
  }
}
```

| kind | role 建议 | 用途 |
|------|-----------|------|
| `turn` | user / assistant | 正常问答（默认） |
| `summary` | `assistant` 或专用 `system` | 压缩后的历史摘要（UI 可折叠显示「已压缩 N 轮」） |
| `system_note` | system | 调试/策略说明，默认不对用户展示 |

**兼容：** 无 `metadata_json` 的旧消息视为 `kind=turn`。

### 3.3 EvidenceSnapshot（可嵌在 assistant metadata 或独立表）

```json
{
  "items": [
    {
      "ref": 1,
      "kind": "text",
      "document_id": "doc_x",
      "job_id": "job_y",
      "page_idx": 3,
      "block_id": "p004-b0012",
      "snippet": "……",
      "image_urls": ["/api/v1/jobs/job_y/markdown/images/page-4/imgs/..."]
    }
  ],
  "ref_counter": 6
}
```

同一会话内 **ref 单调递增不回收**（避免「[2] 上轮是 A 这轮是 B」）。  
若必须回收，UI 只展示本轮 `citations`，历史气泡绑定当时 snapshot。

---

## 4. Memory 组装算法（B2 核心）

### 4.1 输入

```text
assemble_memory(
  transcript: Message[],
  scope: { document_id, job_id },
  skill: Skill,
  budget: TokenBudget,
) -> { messages: ChatMessage[], evidence: EvidenceItem[], debug }
```

### 4.2 策略 `extractive_v1`（默认，不依赖 LLM 摘要）

```text
1. 分离：
   - summaries = kind==summary 的消息（按时间）
   - turns = kind==turn 的 user/assistant 对

2. 取「最新 summary」S（若有），它覆盖 through 某 message_id 之前的内容。

3. 近期窗口 W：
   - 取 S 之后的 turns，再截断为最近 K 轮（默认 K=6 轮 = 12 条消息）
   - 单条 content 超长则 clip（user 2k / assistant 3k 字符硬顶）

4. Evidence 包 E：
   - 合并 W 内 assistant 的 citations / evidence_snapshot
   - 上限 max_evidence_items（默认 24）
   - 优先保留：被最近一轮引用到的 ref > 较新 > 有 image_urls 的

5. 拼 messages：
   [ system = skill.system_prompt + scope_lock_text ]
   [ developer? = skill.developer ]
   if S: [ {role:user, content: "以下是更早对话的摘要，请当作已知背景：\n"+S.content } ]
          [ {role:assistant, content: "好的，我将基于摘要与新问题继续。" } ]  # 可选稳定前缀
   for m in W: append role/content
   if E:  append 一条隐藏/user 工具式上下文？ → 否；
          改为在 system 尾部附 "已知证据表"：
          "E1 [1] p.4 block … snippet"
          （控制在 ~2k 字符）

6. 若估算 tokens > budget：
   - 先减 K（窗口）
   - 再缩短 snippet
   - 再触发 compress_now() 生成新 summary（见 4.3）
```

### 4.3 何时压缩 `compress_now`

触发条件（任一）：

- `len(turns) > 2K`（例如 12 轮）  
- 估算 prompt tokens > `0.55 * context_window`  
- 显式请求 `force_compress: true`

**extractive 摘要内容模板：**

```text
【对话摘要】
- 用户关注：…
- 已确认结论：…（附 [n] 若有）
- 未解决问题：…
- 重要证据：
  [1] p.3 … 
  [2] p.7 …
```

生成方式 v1：

1. 从被折叠的 turns 抽取：所有 user 问题（截断）、所有带 [n] 的 assistant 句、全部 citations  
2. 规则拼接，**不调用 LLM**（稳、便宜、可测）  
3. v2 可选：LLM 摘要 skill，失败回退 v1  

压缩后：

1. 向 Rust append `kind=summary` 消息  
2. 更新 `conversation.memory_json.through_message_id`  
3. SSE 发 `compress` 事件  

### 4.4 Token 估算

v1 使用廉价估算：`tokens ≈ chars / 3`（中英混合偏保守可 `/2.5`）。  
不强制 tiktoken，避免 AI 服务重依赖。

---

## 5. API 形状

### 5.1 保持兼容：`POST /v1/ai/ask`（Rust 直调）

```json
{
  "question": "……",
  "document_id": "doc_…",
  "job_id": "job_…",
  "conversation_id": "conv_…",
  "stream": true,
  "skill_id": "literature-qa",
  "force_compress": false,
  "llm_api_key": "",
  "llm_base_url": "",
  "llm_model": ""
}
```

| 字段 | 现状 | B 后 |
|------|------|------|
| `conversation_id` | 可选 | **阅读器应总是带**（无则后端可 auto-create 并在 done 返回） |
| `skill_id` | 无 | 可选，默认 `literature-qa` |
| `force_compress` | 无 | 可选 |
| `history` 客户端直传 | 无 | **不鼓励**；以服务端读 Rust 为准（防双源） |

### 5.2 `done` 扩展（可选字段）

```json
{
  "type": "done",
  "answer": "……",
  "citations": [ /* Evidence 子集 */ ],
  "tool_trace": [ /* 本 run */ ],
  "rounds": 3,
  "conversation_id": "conv_…",
  "run_id": "run_…",
  "memory": {
    "window_turns": 6,
    "had_summary": true,
    "evidence_count": 8,
    "compressed": false
  },
  "usage": {
    "prompt_tokens_est": 4200,
    "completion_tokens_est": 600
  }
}
```

### 5.3 新 SSE：`compress`

```json
{
  "type": "compress",
  "dropped_turns": 8,
  "summary_chars": 900,
  "kept_evidence": 12,
  "policy": "extractive_v1"
}
```

### 5.4 Rust：创建会话（阅读器打开 AI 或首问时）

```http
POST /api/v1/ai/conversations
{
  "document_id": "doc_…",
  "job_id": "job_…",
  "title": "",
  "skill_id": "literature-qa"
}
→ { "conversation_id": "conv_…" }
```

### 5.5 Rust：追加消息（扩展）

```http
POST /api/v1/ai/conversations/{id}/messages
{
  "role": "assistant",
  "content": "……",
  "citations_json": "[…]",
  "tool_trace_json": "[…]",
  "model": "…",
  "metadata_json": "{ \"kind\": \"turn\", \"run_id\": \"…\" }"
}
```

Summary 消息：

```json
{
  "role": "assistant",
  "content": "【对话摘要】…",
  "metadata_json": "{\"kind\":\"summary\",\"compress\":{\"policy\":\"extractive_v1\",\"covers_message_ids\":[…]}}"
}
```

### 5.6 前端阅读器流程（目标）

```text
open AI panel
  if !conversationId for (jobId|documentId):
      create conversation → store in memory/localStorage key
ask(question):
  POST ask with conversation_id + job_id + document_id
  on compress → 可选 toast「已压缩早期对话」
  on done → 渲染 answer + citations；记住 conversation_id
```

存储键建议：`retainpdf.reader.ai.conversation.v1:{jobId}`。

---

## 6. Runtime 伪代码

```python
def ask(question, *, conversation_id, scope, skill_id, budget, force_compress=False):
    skill = load_skill(skill_id)
    transcript = rust.list_messages(conversation_id, limit=200)

    if force_compress or should_compress(transcript, budget):
        summary_msg = build_extractive_summary(transcript, budget)
        rust.append_message(conversation_id, summary_msg)
        emit({"type": "compress", ...})
        transcript = rust.list_messages(conversation_id, limit=200)

    mem = assemble_memory(transcript, scope, skill, budget)
    result = run_tool_loop(
        messages=mem.messages,
        tools=skill.tools,
        budget=budget,
        evidence_seed=mem.evidence,
        on_event=emit,
    )
    rust.append_message(user)
    rust.append_message(assistant + citations + metadata)
    emit({"type": "done", **result, "conversation_id": conversation_id, "memory": mem.debug})
    return result
```

---

## 7. 与引用编号的关系

| 规则 | 说明 |
|------|------|
| 单 run 内 | 与现 ` _assign_refs` 相同，从 1 或从 `ref_counter+1` 起 |
| 跨 run | **继续递增**（读上次 snapshot 的 `ref_counter`） |
| 回答中的 [n] | 必须落在本 run 可见 evidence 或已知证据表 |
| 压缩后 | 摘要里保留 [n] 与 snippet；旧气泡 UI 仍显示当时 citations |

---

## 8. 测试计划（B）

| 用例 | 期望 |
|------|------|
| 无 conversation_id | 行为与现网一致（单轮）；或 auto-create 并在 done 返回 |
| 有 conversation_id 连问 2 轮 | 第二轮 memory 含第一轮 user/assistant |
| 15 轮后触发压缩 | 出现 summary 消息；assemble 不再含全部早期原文 |
| evidence 上限 | 超过 max 时丢最旧未引用项 |
| scope 锁 | memory 的 system 含 document_id；工具参数被注入 |
| 字符 clip | 超长 assistant 被截断且不炸 JSON |

---

## 9. 分阶段实现清单

### B1 — Session 贯通（小、优先）

- [ ] 前端：创建/复用 `conversation_id` 并随 ask 上传  
- [ ] 后端：done 回显 `conversation_id`  
- [ ] Rust：conversation 支持 `document_id`/`job_id`/`skill_id`（若尚无）  
- [ ] 文档 + 单测：history 注入条数  

### B2 — Memory 压缩

- [ ] `services/ai/memory.rs`（assemble + compress）  
- [ ] `metadata_json` 读写  
- [ ] SSE `compress`  
- [ ] 估算 token 与 budget 配置项  
- [ ] 单测：压缩前后 messages 长度  

### B3 — Evidence 跨轮

- [ ] snapshot 落库 / 回灌「已知证据表」  
- [ ] ref_counter 持久化  

---

## 10. 配置项（建议 env）

| 变量 | 默认 | 说明 |
|------|------|------|
| `RETAIN_AI_MEMORY_WINDOW_TURNS` | `6` | 近期保留轮数 |
| `RETAIN_AI_MEMORY_MAX_CHARS` | `24000` | MemoryView 粗上限 |
| `RETAIN_AI_MEMORY_COMPRESS_AFTER_TURNS` | `12` | 超过则压缩 |
| `RETAIN_AI_MEMORY_MAX_EVIDENCE` | `24` | 证据条数 |
| `RETAIN_AI_MEMORY_POLICY` | `extractive_v1` | 压缩策略名 |

---

## 11. 开放决策

| ID | 问题 | 建议 |
|----|------|------|
| M1 | 无 conversation_id 时 auto-create？ | **是**（减少前端状态机），done 必须回传 |
| M2 | summary 是否在 UI 展示？ | 默认折叠一行「已总结前 N 轮」 |
| M3 | tool_trace 是否进 memory？ | **否**；只进 evidence 与本 run trace |
| M4 | 是否允许客户端传 history？ | v1 **忽略**客户端 history，避免分叉 |

---

## 12. 验收标准（B 完成时）

1. 同一阅读任务连续追问 5 次，第 5 次回答能引用第 1 次结论或证据。  
2. 人为加长历史至 20 轮后，请求仍成功；SSE 至少出现一次 `compress` 或存在 summary 消息。  
3. 压缩后 citations 跳转仍正确（page_idx 0 基）。  
4. 旧前端不传新字段时行为不回退到 5xx。  
