import { normalizeStageRetryActions } from "./stage-actions.js";

export function buildStatusCardRetryActions(stageActionsPayload = null) {
  return normalizeStageRetryActions(stageActionsPayload);
}
