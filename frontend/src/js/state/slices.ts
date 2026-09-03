import { createCredentialState } from "./credential-state.js";
import { createDesktopState } from "./desktop-state.js";
import { createDeveloperState } from "./developer-state.js";
import { createHomeState } from "./home-state.js";
import { createJobState } from "./job-state.js";
import { createRecentJobsState } from "./recent-jobs-state.js";
import { createTimerState } from "./timer-state.js";
import { createUploadState } from "./upload-state.js";

export {
  createCredentialState,
  createDesktopState,
  createDeveloperState,
  createHomeState,
  createJobState,
  createRecentJobsState,
  createTimerState,
  createUploadState,
};

export function createInitialState() {
  return {
    ...createTimerState(),
    ...createJobState(),
    ...createHomeState(),
    ...createUploadState(),
    ...createRecentJobsState(),
    ...createCredentialState(),
    ...createDeveloperState(),
    ...createDesktopState(),
  };
}

export type InitialState = ReturnType<typeof createInitialState>;
