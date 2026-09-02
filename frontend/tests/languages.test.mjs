import test from "node:test";
import assert from "node:assert/strict";

import {
  AUTO_SOURCE_CODE,
  DEFAULT_SOURCE_LANG,
  DEFAULT_TARGET_LANG,
  DEFAULT_TARGET_LANGUAGE_NAME,
  LANGUAGE_ENTRIES,
  lookupLanguage,
  normalizeSourceLang,
  normalizeTargetLang,
  sourceLanguageName,
  sourceOptions,
  targetFilePrefix,
  targetLanguageName,
  targetOptions,
  targetScriptFamily,
} from "../src/js/config/languages.js";

test("registry defaults reproduce legacy zh behavior", () => {
  assert.equal(DEFAULT_SOURCE_LANG, "auto");
  assert.equal(DEFAULT_TARGET_LANG, "zh-CN");
  assert.equal(DEFAULT_TARGET_LANGUAGE_NAME, "简体中文");
  assert.equal(normalizeSourceLang(), "auto");
  assert.equal(normalizeSourceLang(undefined), "auto");
  assert.equal(normalizeTargetLang(), "zh-CN");
  assert.equal(normalizeTargetLang(undefined), "zh-CN");
  assert.equal(targetLanguageName(), "简体中文");
  assert.equal(targetScriptFamily(), "zh");
  assert.equal(targetFilePrefix(), "zh");
});

test("normalize falls back for unknown or invalid codes", () => {
  assert.equal(normalizeSourceLang("xx"), "auto");
  assert.equal(normalizeSourceLang(null), "auto");
  assert.equal(normalizeTargetLang("xx"), "zh-CN");
  assert.equal(targetLanguageName("xx"), "简体中文");
  assert.equal(targetScriptFamily("xx"), "zh");
});

test("display names, families and prefixes map per language", () => {
  assert.equal(targetLanguageName("en"), "English");
  assert.equal(targetLanguageName("ja"), "日本語");
  assert.equal(targetScriptFamily("zh-CN"), "zh");
  assert.equal(targetScriptFamily("zh-TW"), "zh");
  assert.equal(targetScriptFamily("ja"), "cjk");
  assert.equal(targetScriptFamily("ko"), "cjk");
  assert.equal(targetScriptFamily("fr"), "latin");
  assert.equal(targetScriptFamily("en"), "latin");
  assert.equal(targetFilePrefix("zh-TW"), "zh-Hant");
  assert.equal(targetFilePrefix("ko"), "ko");
});

test("source name is empty for auto and provided otherwise", () => {
  assert.equal(sourceLanguageName("auto"), "");
  assert.equal(sourceLanguageName("ja"), "日本語");
  assert.equal(sourceLanguageName(""), "");
});

test("source options lead with auto; target options exclude auto", () => {
  const sources = sourceOptions();
  assert.equal(sources[0].code, AUTO_SOURCE_CODE);
  const targets = targetOptions();
  assert.ok(targets.every((option) => option.code !== AUTO_SOURCE_CODE));
  assert.equal(LANGUAGE_ENTRIES.length, 9);
  const codes = new Set(LANGUAGE_ENTRIES.map((entry) => entry.code));
  assert.equal(codes.size, LANGUAGE_ENTRIES.length);
  for (const entry of LANGUAGE_ENTRIES) {
    assert.equal(lookupLanguage(entry.code)?.code, entry.code);
    assert.equal(lookupLanguage(entry.code)?.displayName, entry.displayName);
  }
});
