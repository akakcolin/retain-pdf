import { resolveJobActions } from "./external.js";

export function buildStatusCardTaskActions({
  job = null,
}: any = {}) {
  const actions = resolveJobActions(job);
  return {
    cancelEnabled: actions.cancelEnabled && Boolean(actions.cancel),
  };
}
