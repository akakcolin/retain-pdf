# Skills 设计（草案）

**状态：** 草案 v0.1（配合 [AI_RUNTIME.md](./AI_RUNTIME.md)）  
**日期：** 2026-07-21  
**现状：** 未实现——agent 循环已并入 Rust，skill 加载器仍为设计态。  

---

## 1. Skill vs Tool

| | Tool | Skill |
|--|------|--------|
| 粒度 | 原子 I/O | 面向任务的能力包 |
| 内容 | name + JSON Schema + handler | 工具子集 + 提示词 + 策略 |
| 测试 | handler 单测 | 场景/契约测 |
| 示例 | `search_fulltext` | `literature-qa` |

一句话：

> **Tool 是动词；Skill 是剧本。**

---

## 2. 包格式

```text
skills/literature_qa/
  skill.yaml      # 清单
  prompt.md       # system（可拆 system.md / developer.md）
  # 可选 policy.py  — 复杂策略时再加
```

### skill.yaml

```yaml
id: literature-qa
version: 1
display_name: 文献整本问答
description: >
  在单文档（或指定 job）范围内检索并回答，强制引用锚点。
tools:
  - search_fulltext
  - read_blocks
  - search_favorites
# list_documents 故意不放进阅读器 skill
policies:
  require_document_scope: true
  allow_global_search: false
  max_tool_rounds: 6
  output_locale: zh-CN
  require_citations: true
  allow_markdown_images: true
model:
  # 可选覆盖；空则用请求/全局配置
  temperature: 0.3
```

### prompt.md

- 现有 `SYSTEM_PROMPT` 主体迁入  
- 占位符（装配时替换）：

```text
{{document_id}}
{{job_id}}
{{evidence_table}}   # Memory 注入的已知证据表，可为空
```

---

## 3. 加载器接口

```python
class Skill(Protocol):
    id: str
    version: int
    tools: list[str]
    policies: dict
    def system_prompt(self, *, scope, evidence_table: str) -> str: ...

def load_skill(skill_id: str) -> Skill: ...
def list_skills() -> list[SkillMeta]: ...
```

错误：`unknown skill` → 400。

---

## 4. 首发：literature-qa

行为对齐今天的阅读器问答：

- scope 强制 document  
- 工具层注入 document_id / job_id  
- 引用 [n] + image_urls 可嵌入  
- 不暴露 list_documents  

验收：与现网回答质量同级；仅配置/提示外置，无功能回退。

---

## 5. 后续 Skill 候选

| id | 场景 | 可能工具 |
|----|------|----------|
| `annotation-assist` | 基于批注/选区解释 | read_blocks, search_favorites |
| `paper-compare` | 两篇文档对比 | search_fulltext×2, read_blocks |
| `figure-explain` | 专讲图/表 | read_blocks, list_page_images（可新增 tool） |

---

## 6. 与 Multi-agent

Skill 可声明：

```yaml
agents:
  - role: retriever
    tools: [search_fulltext, read_blocks]
  - role: analyst
    tools: []    # 只写
```

v0 忽略 `agents` 字段，单循环执行全部 tools。  
字段先写进 schema，避免以后改包格式。

---

## 7. 实施顺序

1. 目录 + loader + literature-qa 迁入（行为不变）  
2. ask 请求支持 `skill_id`  
3. 第二个 skill 再证明扩展性  
