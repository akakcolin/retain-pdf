import { isJobTerminal, isTerminalStatus } from "./external.js";
import {
  formatEventTimestamp,
  formatRuntimeDuration,
} from "./external.js";

export {
  clampPositiveMs,
  parseIsoTime,
  resolveLiveDurations,
} from "./external.js";
export {
  resolveStageHistory,
  resolveStageHistoryDuration,
  stageHistoryDisplay,
  summarizeStageName,
} from "./external.js";

export function escapeHtml(value) {
  return `${value ?? ""}`
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export { formatEventTimestamp, formatRuntimeDuration, isJobTerminal, isTerminalStatus };
