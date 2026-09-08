import {
  findReadyManifestArtifact,
  resolveManifestArtifactUrl,
  resolveResourceUrl,
} from "../../../js/job/artifacts.js";
import { resolveJobActions } from "../../../js/job/actions.js";
import { resolveReaderArtifactUrl } from "./pdf-controller.js";

export function resolveReaderJobId(configPort) {
  return configPort?.readerJobId?.() || "";
}

export function resolveReaderSourcePdf(manifestPayload) {
  return resolveManifestArtifactUrl(manifestPayload, "source_pdf")
    || findReadyManifestArtifact(manifestPayload, "source_pdf");
}

export function resolveReaderTranslatedPdfUrl(jobPayload, manifestPayload) {
  const actions = jobPayload ? resolveJobActions(jobPayload) : null;
  if (actions?.pdfEnabled && actions?.pdf) {
    return actions.pdf;
  }
  const manifestCandidates = ["pdf", "translated_pdf", "result_pdf"];
  for (const artifactKey of manifestCandidates) {
    const item = findReadyManifestArtifact(manifestPayload, artifactKey);
    const url = resolveReaderArtifactUrl(item);
    if (url) {
      return url;
    }
  }
  const workflow = `${jobPayload?.workflow || jobPayload?.job_type || ""}`.trim().toLowerCase();
  const canUseTranslatedPdfRoute = actions?.pdfEnabled
    || (`${jobPayload?.status || ""}`.trim().toLowerCase() === "succeeded" && workflow !== "ocr");
  if (canUseTranslatedPdfRoute && jobPayload?.job_id) {
    return resolveResourceUrl(`/api/v1/jobs/${encodeURIComponent(jobPayload.job_id)}/pdf`);
  }
  return "";
}
