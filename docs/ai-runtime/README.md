# RetainPDF AI Runtime（设计文档索引）

**状态：** 设计草案（C 架构 + B Session/压缩）  
**日期：** 2026-07-21  
**代码现状：** agent 循环已并入 Rust（`backend/rust_api/src/services/ai/`，无独立服务）  
**产品入口：** 阅读器整本问答 → `POST /api/v1/ai/ask`（Rust 直调 LLM）

---

## 文档

| 文档 | 内容 |
|------|------|
| **[AI_RUNTIME.md](./AI_RUNTIME.md)** | 目标架构：Transport / Session / Orchestrator / Runtime / Skills / Evidence |
| **[SESSION_AND_MEMORY.md](./SESSION_AND_MEMORY.md)** | 多轮会话协议、上下文压缩、API 与数据形状（B 的详细草案） |
| **[SKILLS.md](./SKILLS.md)** | Skill 包格式、与 Tool 的边界、首个 `literature-qa` 示例 |

---

## 一句话目标

> **AI 服务只做编排；Rust 管数据与权限；工具形状与主流 SDK 同构；Skills / Memory / Multi-agent 可插拔挂上，不必推倒重写。**

---

## 与现状的关系

```
现状（MVP）
  POST /v1/ask → RetrievalAgent 裸循环 → 4 个 tools → answer + citations

目标（可扩展 runtime）
  POST /v1/runs  → Orchestrator
                    ├─ Session/Memory（窗口 + 摘要 + evidence 包）
                    ├─ Skill(s)（literature-qa / …）
                    ├─ Agent loop(s)（检索 / 分析 / 可选 critic）
                    └─ Evidence（锚点、图、可跳转引用）
```

迁移策略：默认 skill 仍是今天的整本检索问答；新能力以 skill/tool 增加，**不先绑死** LangGraph/Crew 等重框架。

---

## 实施顺序（建议）

1. **文档冻结接口** ✅（C + B 草案）  
2. **Session 贯通（B1）** ✅ auto-create + 前端粘性 + done 回传  
3. **Memory 压缩（B2）** ✅ 窗口 + extractive 摘要 + SSE `compress`  
4. Skill 加载器 + 收口 `literature-qa`  
5. Orchestrator + 第二 agent（可选）  

每步都应可单独合并、可回滚，不阻断现有 `/v1/ask`。
