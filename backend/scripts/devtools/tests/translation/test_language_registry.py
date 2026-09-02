from services.translation.core.languages import (
    DEFAULT_SOURCE_LANG,
    DEFAULT_TARGET_LANG,
    DEFAULT_TARGET_LANGUAGE_NAME,
    is_cjk_target,
    is_latin_target,
    is_zh_target,
    script_family,
)


def test_defaults_reproduce_legacy_zh() -> None:
    assert DEFAULT_SOURCE_LANG == "auto"
    assert DEFAULT_TARGET_LANG == "zh-CN"
    assert DEFAULT_TARGET_LANGUAGE_NAME == "简体中文"
    assert script_family(None) == "zh"
    assert script_family("") == "zh"


def test_family_maps_per_language() -> None:
    assert script_family("zh-CN") == "zh"
    assert script_family("zh-TW") == "zh"
    assert script_family("ja") == "cjk"
    assert script_family("ko") == "cjk"
    assert script_family("en") == "latin"
    assert script_family("fr") == "latin"
    assert script_family("de") == "latin"
    assert script_family("es") == "latin"
    assert script_family("ru") == "latin"


def test_unknown_target_falls_back_to_zh() -> None:
    assert script_family("xx") == "zh"
    assert is_zh_target("xx")
    assert is_zh_target("zh-CN")
    assert is_cjk_target("ja")
    assert is_latin_target("en")
    assert not is_latin_target("zh-CN")
