from __future__ import annotations

"""目标/源语言脚本族注册表（Python 侧）。

与前端 frontend/src/js/config/languages.ts 保持同一套 code/脚本族口径：
- zh-CN / zh-TW 属于 ``zh``（汉字目标，保留现有中文校验/渲染启发式）；
- ja / ko 属于 ``cjk``（汉字+假名/谚文，需放宽基于汉字占比的英文残留判定）；
- en / fr / de / es / ru 属于 ``latin``（输出本就是拉丁字母，英文残留族校验禁用）。

未知 code 回落到 ``zh``：任何没有显式携带目标语的任务都保持既有中文行为。
"""

DEFAULT_SOURCE_LANG = "auto"
DEFAULT_TARGET_LANG = "zh-CN"
DEFAULT_TARGET_LANGUAGE_NAME = "简体中文"

_TARGET_SCRIPT_FAMILY: dict[str, str] = {
    "zh-CN": "zh",
    "zh-TW": "zh",
    "en": "latin",
    "ja": "cjk",
    "ko": "cjk",
    "fr": "latin",
    "de": "latin",
    "es": "latin",
    "ru": "latin",
}


def script_family(target_lang: str | None) -> str:
    return _TARGET_SCRIPT_FAMILY.get((target_lang or "").strip() or DEFAULT_TARGET_LANG, "zh")


def is_zh_target(target_lang: str | None) -> bool:
    return script_family(target_lang) == "zh"


def is_cjk_target(target_lang: str | None) -> bool:
    return script_family(target_lang) == "cjk"


def is_latin_target(target_lang: str | None) -> bool:
    return script_family(target_lang) == "latin"


__all__ = [
    "DEFAULT_SOURCE_LANG",
    "DEFAULT_TARGET_LANG",
    "DEFAULT_TARGET_LANGUAGE_NAME",
    "is_cjk_target",
    "is_latin_target",
    "is_zh_target",
    "script_family",
]
