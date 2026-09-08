import {
  collectStageProgressByKey,
} from "./job-stage-progress-records.js";
import {
  resolvePublicStagePresentation,
} from "./public-stage-engine.js";

export { collectStageProgressByKey };

export function resolveDisplayedStagePresentation(job, eventsPayload) {
  return resolvePublicStagePresentation(job || {}, eventsPayload || {});
}
