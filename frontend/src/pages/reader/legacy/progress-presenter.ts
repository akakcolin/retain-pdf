import { READER_DIALOG_MESSAGES } from "../../../js/features/reader-dialog/contract.js";
import {
  defaultReaderPageConfigPort,
} from "./config-port.js";
import {
  computeReaderProgressSnapshot,
} from "./page-state.js";
import {
  animateReaderProgressValue,
  setReaderBootProgressText,
} from "./view.js";

export function createReaderProgressPresenter({
  animateProgressValue = animateReaderProgressValue,
  messageType = READER_DIALOG_MESSAGES.progress,
  messageTargetOrigin = defaultReaderPageConfigPort.messageTargetOrigin,
  parentWindow = () => window.parent,
  setProgressText = setReaderBootProgressText,
}: any = {}) {
  function apply({
    bootProgressBarState,
    percent,
    stage = "progress",
    text,
  }: any = {}) {
    setProgressText(text);
    animateProgressValue(bootProgressBarState, percent);
    try {
      parentWindow()?.postMessage({
        type: messageType,
        stage,
        percent,
        text,
      }, messageTargetOrigin());
    } catch (_err) {
      // Ignore cross-frame reporting failures.
    }
  }

  function sync(pageState) {
    const snapshot = computeReaderProgressSnapshot(pageState?.progress);
    apply({
      bootProgressBarState: pageState?.bootProgressBar,
      percent: snapshot.percent,
      stage: snapshot.stage,
      text: snapshot.text,
    });
    return snapshot;
  }

  return Object.freeze({
    apply,
    sync,
  });
}

export const defaultReaderProgressPresenter = createReaderProgressPresenter();
