from __future__ import annotations

import re

from services.translation.llm.shared.response_parsing import PROMPT_ECHO_BATCH_LABEL_RE
from services.translation.llm.shared.response_parsing import PROMPT_ECHO_INSTRUCTION_FRAGMENTS
from services.translation.llm.shared.response_parsing import PROMPT_ECHO_LABEL_RE
from services.translation.llm.shared.response_parsing import PROMPT_ECHO_MARKER_LINES
from services.translation.llm.shared.response_parsing import is_prompt_echo_label_head


MODEL_REQUEST_PROMPT_RE = re.compile(
    r"^(?:"
    r"请\s*(?:提供|输入|给出|粘贴|发送)\s*(?:待翻译的?)?\s*(?:原文|文本|内容|source)(?:[。.!！?？\s]*)|"
    r"(?:please\s+)?(?:provide|send|enter|paste)\s+(?:the\s+)?(?:source\s+)?(?:text|content)(?:\s+to\s+translate)?(?:[。.!！?？\s]*)"
    r")$",
    re.I,
)
MODEL_REQUEST_PROMPT_MAX_CHARS = 48


def looks_like_protocol_shell_output(translated_text: str) -> bool:
    text = str(translated_text or "").strip()
    if not text:
        return False
    if len(text) <= MODEL_REQUEST_PROMPT_MAX_CHARS and MODEL_REQUEST_PROMPT_RE.fullmatch(text):
        return True
    if not text.startswith("{"):
        return False
    return (
        '"translated_text"' in text
        or '"translations"' in text
        or "“translated_text”" in text
        or "“translations”" in text
    )


def looks_like_prompt_echo_output(translated_text: str) -> bool:
    """弱模型把提示词定界符/标签/指令句回显进译文时的识别。

    解析层的 strip_prompt_echo_markers 能回收常见回显；这里作为兜底，凡是
    回显清理后仍残留可辨识的提示词痕迹（定界符、开头标签、指令片段）就报错，
    让上层走重试/降级，而不是把污染文本渲染进 PDF。
    """
    text = str(translated_text or "").strip()
    if not text:
        return False
    if any(marker in text for marker in PROMPT_ECHO_MARKER_LINES):
        return True
    lines = [line.strip() for line in text.splitlines() if line.strip()]
    if not lines:
        return False
    if PROMPT_ECHO_LABEL_RE.match(lines[0]) or PROMPT_ECHO_BATCH_LABEL_RE.match(lines[0]):
        return True
    return any(
        is_prompt_echo_label_head(line)
        or any(fragment in line for fragment in PROMPT_ECHO_INSTRUCTION_FRAGMENTS)
        for line in lines
    )


__all__ = [
    "MODEL_REQUEST_PROMPT_MAX_CHARS",
    "MODEL_REQUEST_PROMPT_RE",
    "looks_like_protocol_shell_output",
    "looks_like_prompt_echo_output",
]
