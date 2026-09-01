from __future__ import annotations

import json
import re


_JSON_QUOTE_TRANSLATION = str.maketrans(
    {
        "“": '"',
        "”": '"',
        "„": '"',
        "‟": '"',
        "‘": '"',
        "’": '"',
        "‚": '"',
        "‛": '"',
        "：": ":",
    }
)
_JSON_KEY_PREFIX_RE = re.compile(r'^\s*"translations"\s*:', re.DOTALL)
_TAGGED_ITEM_BLOCK_RE = re.compile(
    r"<<<ITEM\s+item_id=(?P<item_id>[^\s>]+)(?:\s+decision=(?P<decision>[A-Za-z_-]+))?\s*>>>\s*"
    r"(?P<content>.*?)"
    r"\s*<<<END>>>",
    re.DOTALL,
)
_PROTOCOL_SHELL_HINT_RE = re.compile(
    r"(translated_text|translations|item_id|decision|```json|<<<ITEM)",
    re.IGNORECASE,
)
# 弱模型 / MT 专用模型（如 Hunyuan-MT-7B）不遵循"只输出译文"，会把提示词里的
# 定界符、指令句或"翻译结果"等标签也生成进译文。以下片段覆盖 prompt_protocols
# 与 foundation/prompts 中实际出现的指令文本，用于解析层回收 + 质检层识别。
PROMPT_ECHO_START_MARKER = "【当前原文开始】"
PROMPT_ECHO_END_MARKER = "【当前原文结束】"
PROMPT_ECHO_MARKER_LINES = {PROMPT_ECHO_START_MARKER, PROMPT_ECHO_END_MARKER}
PROMPT_ECHO_LABEL_RE = re.compile(r"^(?:翻译结果|翻译结果如下|翻译内容|译文|翻译)[：:\s]*$")
PROMPT_ECHO_PREFIX_RE = re.compile(r"^(?:翻译结果|翻译结果如下|翻译内容|译文|翻译)\s*[：:]")
PROMPT_ECHO_BATCH_LABEL_RE = re.compile(r"^原文\s+[A-Za-z0-9_.:-]+:?$")
PROMPT_ECHO_INSTRUCTION_FRAGMENTS = (
    "翻译成适合科研论文排版的",
    "请把用户给出的当前原文翻译为适合论文排版的",
    "下面是一段待翻译正文。",
    "只输出这一段的最终",
    "只输出最终译文",
    "只输出译文本身",
    "只返回译文本身",
    "只输出最终译文本身",
    "不要输出编号、决策字段",
    "不要返回结构化数据、代码块、标签、编号、决策字段或解释说明",
    "你正在翻译一个文本块。",
    "你是一名科研文献翻译助手",
    "只翻译当前原文中明确出现",
    "Output only the translation",
    "请把下面的",
    "不要机械照搬原文语序",
    "不要添加冗余解释",
    "不要翻译占位符",
    "不要添加原文中不存在的内容",
    "不要自行补全",
    "只能翻译当前原文本身",
    "只能用于术语消歧",
    "明显损坏的极短 OCR 片段",
    "不适用于正常英文虚词",
    "同等程度的不完整",
    "术语要求：",
    "风格提示：",
    "结构提示：",
    "前文上下文（仅供理解",
    "后文上下文（仅供理解",
    "这是跨栏或跨页续接正文的一部分",
    # translation_system_plain_text.txt / translation_system.txt
    "语言风格应当准确、客观、简洁",
    "符合学术论文常用表达",
    "避免口语化、闲聊式语气",
    "不要把方法、材料或结果段落无端扩写",
    "不要猜测缺失后文",
    "不要把相邻块内容并入当前块",
    "不得删除、改写、重排、解释或重编号",
    "不要补充背景说明、主观解释",
    "科研行文中优先使用",
    "不要把很短的片段扩写成完整句子",
    "翻译成简洁正式的学术标题",
    "作者、期刊、年份、卷期、页码等保持原样",
    "不要音译作者姓名",
    "保持原有引文顺序",
    # translation_task_plain_text.txt / translation_task.txt
    "在不损失信息和技术精度的前提下",
    "译文长度在可能时尽量接近原文",
    "混合了说明文字和字面量",
    "视为块级风格约束",
    # translation_direct_typst_guidance.txt
    "当前启用 direct_typst 公式直出模式",
    "不要把裸露的 LaTeX 风格数学片段直接留在正文里",
    # domain_inference_task.txt 生成的领域指导常见措辞(弱模型会逐字回显)
    "保持领域术语稳定一致",
    "避免过度意译或无依据扩写",
    "使用自然、准确、符合学术语体的中文",
    "优先采用该领域常见的中文术语",
    # system prompt 注入领域指导的英文标签
    "Document-specific translation guidance:",
)
# domain_guidance 实际回显中观察到的标签(弱 domain_inference 的转述)。这些
# 短语又短又通用,可能出现在真实译文里,不能按裸子串识别;只在"标签头"形态
# (行首 + 冒号,或整行就是短语)下由 is_prompt_echo_label_head 判定。
PROMPT_ECHO_LABEL_PHRASES = (
    "格式一致性",
    "避免不必要的翻译",
    "保持原文意图",
    "采用了符合学术论文格式",
    "保留了原样的数学表达式",
    "没有添加额外的解释",
    "章节标题或小节标题",
)


_PROMPT_ECHO_LABEL_HEAD_PREFIX_RE = re.compile(
    r"^\s*(?:"
    r"[\[（(]?\s*\d+\s*[）)\]、.．]?"
    r"|[\[（(]?\s*[一二三四五六七八九十]+\s*[）)\]、.．]?"
    r"|[-*•·–—]"
    r")?\s*"
)


def is_prompt_echo_label_head(line: str) -> bool:
    """判断一行是否以"标签头"形态回显了短而通用的学术短语。

    格式一致性/避免不必要的翻译/保持原文意图等短语在真实译文里也常见,
    裸子串匹配必然误伤合法句子;这里只认回显形态:去掉可选的列表编号/圆点
    前缀后,行首是已知短语且紧跟冒号,或整行(含末尾句号)就是短语本身。
    """
    text = str(line or "").strip()
    if not text:
        return False
    headless = _PROMPT_ECHO_LABEL_HEAD_PREFIX_RE.sub("", text)
    for phrase in PROMPT_ECHO_LABEL_PHRASES:
        if not headless.startswith(phrase):
            continue
        remainder = headless[len(phrase):].strip()
        if not remainder:
            return True
        if remainder[0] in "：:":
            return True
        if len(remainder) <= 2 and remainder[0] in "。．.!！；;":
            return True
    return False


def is_prompt_echo_head_line(line: str) -> bool:
    if PROMPT_ECHO_LABEL_RE.match(line):
        return True
    if PROMPT_ECHO_BATCH_LABEL_RE.match(line):
        return True
    if is_prompt_echo_label_head(line):
        return True
    return any(fragment in line for fragment in PROMPT_ECHO_INSTRUCTION_FRAGMENTS)


def strip_prompt_echo_markers(text: str, source_text: str = "") -> str:
    """去掉模型回显的提示词定界符/标签/指令，回收真实译文。

    定界符包围的整块回显（【当前原文开始】…【当前原文结束】之间，含定界符本身）
    直接删除——真实译文中不可能出现这对定界符，删块是安全的；标签与指令回显
    只从头部剥离，遇首个内容行停止，避免误伤正文。传入 source_text 时再剥离
    译文开头整段回显的原文（见 _strip_leading_source_echo）。
    """
    if not text:
        return text
    lines = (text or "").splitlines()
    kept: list[str] = []
    in_echo_block = False
    for line in lines:
        stripped = line.strip()
        if in_echo_block:
            if stripped == PROMPT_ECHO_END_MARKER:
                in_echo_block = False
            continue
        if stripped == PROMPT_ECHO_START_MARKER:
            in_echo_block = True
            continue
        kept.append(line)
    result: list[str] = []
    head = True
    for line in kept:
        if not head:
            result.append(line)
            continue
        stripped = line.strip()
        if not stripped:
            continue
        if is_prompt_echo_head_line(stripped):
            continue
        prefix = PROMPT_ECHO_PREFIX_RE.match(stripped)
        if prefix:
            remainder = stripped[prefix.end() :].strip()
            if not remainder:
                continue
            if is_prompt_echo_head_line(remainder):
                continue
            result.append(remainder)
            head = False
            continue
        result.append(line)
        head = False
    cleaned = "\n".join(result).strip()
    if source_text:
        cleaned = _strip_leading_source_echo(cleaned, source_text)
    return cleaned


def _strip_leading_source_echo(text: str, source_text: str) -> str:
    """弱模型把 user 消息里的原文整段回显到译文开头时，回收其后真正的译文。

    极简兜底提示词没有定界符，解析层无从按块删除，只能拿原文本身做精确前缀
    比对。仅当译文以"原文整段（折叠空白后逐字）开头"时剥离，并要求原文至少
    4 个词，避免误伤译文开头合法保留的机构名/专名等短片段。剥离后为空说明
    模型只回显没翻译，交给空译文错误处理。
    """
    if not source_text or not text:
        return text
    normalized = re.sub(r"\s+", " ", str(text)).strip()
    normalized_source = re.sub(r"\s+", " ", str(source_text)).strip()
    if not normalized or not normalized_source:
        return text
    if len(normalized_source.split()) < 4:
        return text
    if not normalized.startswith(normalized_source):
        return text
    remainder = normalized[len(normalized_source) :].strip()
    if not remainder:
        return ""
    return remainder


def strip_prompt_prefix(content: str, *prefixes: str) -> str:
    """从输出开头剥离模型逐字回显的 prompt 片段，回收其后真正的译文。

    弱模型不遵循"只输出译文"时会把 system/user 消息（含动态领域指导）原样
    回显到输出开头。这里对每个候选片段做折叠空白后的精确前缀比对并剥离；
    只有与已知 prompt 逐字一致的片段才会被剥离，不会误伤正文。反复剥离直到
    无可剥离项（多个消息可能交错回显）。短片段（<20 字符）不剥离，避免误伤
    译文开头合法保留的专名。
    """
    text = str(content or "").strip()
    while True:
        normalized = re.sub(r"\s+", " ", text)
        matched: str | None = None
        for prefix in prefixes:
            expected = re.sub(r"\s+", " ", str(prefix or "")).strip()
            if len(expected) < 20:
                continue
            if normalized.startswith(expected):
                matched = expected
                break
        if matched is None:
            return text
        text = normalized[len(matched) :].strip()


def extract_json_text(content: str) -> str:
    text = (content or "").strip()
    if text.startswith("```"):
        lines = text.splitlines()
        if lines and lines[0].startswith("```"):
            lines = lines[1:]
        if lines and lines[-1].startswith("```"):
            lines = lines[:-1]
        text = "\n".join(lines).strip()
    text = _normalize_loose_json_text(text)
    start = text.find("{")
    end = text.rfind("}")
    if start == -1 or end == -1 or end < start:
        raise ValueError("Model response does not contain a JSON object.")
    return text[start : end + 1]


def _extract_single_item_translation_text(content: str, item_id: str) -> str:
    text = (content or "").strip()
    if not text:
        return ""

    tagged_matches = list(_TAGGED_ITEM_BLOCK_RE.finditer(text))
    if tagged_matches:
        for match in tagged_matches:
            if (match.group("item_id") or "").strip() == item_id:
                return (match.group("content") or "").strip()
        if len(tagged_matches) == 1:
            return (tagged_matches[0].group("content") or "").strip()

    try:
        payload = json.loads(extract_json_text(text))
    except Exception:
        if _PROTOCOL_SHELL_HINT_RE.search(text):
            raise
        return text

    if isinstance(payload, dict) and "translated_text" in payload:
        return unwrap_translation_shell(str(payload.get("translated_text", "") or "").strip(), item_id=item_id)

    translations = payload.get("translations", [])
    if not isinstance(translations, list):
        return text
    for item in translations:
        if str(item.get("item_id", "") or "").strip() == item_id:
            return unwrap_translation_shell(str(item.get("translated_text", "") or "").strip(), item_id=item_id)
    if len(translations) == 1:
        return unwrap_translation_shell(str(translations[0].get("translated_text", "") or "").strip(), item_id=item_id)
    return text


def extract_single_item_translation_text(content: str, item_id: str, source_text: str = "") -> str:
    raw = _extract_single_item_translation_text(content, item_id)
    return strip_prompt_echo_markers(raw, source_text=source_text).strip()


def _unwrap_translation_shell(text: str, item_id: str = "") -> str:
    current = str(text or "").strip()
    for _ in range(3):
        if not current or "translated_text" not in current or "{" not in current:
            return current
        try:
            payload = json.loads(extract_json_text(current))
        except Exception:
            return current
        if isinstance(payload, dict):
            if "translated_text" in payload:
                next_text = str(payload.get("translated_text", "") or "").strip()
                if next_text == current:
                    return current
                current = next_text
                continue
            translations = payload.get("translations", [])
            if isinstance(translations, list):
                for item in translations:
                    if not isinstance(item, dict):
                        continue
                    if item_id and str(item.get("item_id", "") or "").strip() == item_id:
                        next_text = str(item.get("translated_text", "") or "").strip()
                        if next_text == current:
                            return current
                        current = next_text
                        break
                else:
                    if len(translations) != 1 or not isinstance(translations[0], dict):
                        return current
                    next_text = str(translations[0].get("translated_text", "") or "").strip()
                    if next_text == current:
                        return current
                    current = next_text
                continue
        return current
    return current


def unwrap_translation_shell(text: str, item_id: str = "") -> str:
    current = _unwrap_translation_shell(text, item_id=item_id)
    return strip_prompt_echo_markers(current).strip()


def _normalize_loose_json_text(text: str) -> str:
    normalized = (text or "").strip().translate(_JSON_QUOTE_TRANSLATION).strip()
    if _JSON_KEY_PREFIX_RE.match(normalized):
        normalized = "{" + normalized + "}"
    return normalized
