import { $ } from "../../../shared/dom/query.js";
import { TRANSLATION_PROVIDER_DEFINITION, getOcrProviderDefinition } from "../../config/providers.js";
import {
  browserValidationIdForProvider,
  CREDENTIAL_DOM_IDS,
} from "./credentials-dom-contract.js";

const { browser: BROWSER_CREDENTIAL_IDS } = CREDENTIAL_DOM_IDS;

function validationIcon(tone = "", content = "") {
  if (!content) {
    return "";
  }
  if (tone === "valid") {
    return "✓";
  }
  if (tone === "error") {
    return "!";
  }
  return "…";
}

function setValidationBadge(el, {
  message = "",
  tone = "",
  idleMessage = "",
}: any = {}) {
  if (!el) {
    return;
  }
  const content = `${message || ""}`.trim();
  el.textContent = validationIcon(tone, content);
  el.title = content || idleMessage || "";
  el.classList.toggle("hidden", !content);
  el.classList.toggle("is-valid", tone === "valid");
  el.classList.toggle("is-error", tone === "error");
  el.classList.toggle("is-pending", !!content && !tone);
}

export function setOcrValidationMessage(message, tone = "", providerId = "") {
  const definition = getOcrProviderDefinition(providerId);
  setValidationBadge($(browserValidationIdForProvider(definition.id)), {
    message,
    tone,
    idleMessage: definition.validationIdleMessage,
  });
}

export function setDeepSeekValidationMessage(message, tone = "") {
  setValidationBadge($(BROWSER_CREDENTIAL_IDS.validations.deepseek), {
    message,
    tone,
    idleMessage: TRANSLATION_PROVIDER_DEFINITION.validationIdleMessage,
  });
}

export function setDeepSeekTopUpVisible(visible = false) {
  const link = $(BROWSER_CREDENTIAL_IDS.deepSeekTopUpLink);
  if (!link) {
    return;
  }
  link.classList.toggle("hidden", !visible);
}
