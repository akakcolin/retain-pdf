import {
  appendResourceQuery,
  hasReadyManifestArtifact,
  resolveManifestArtifactUrl,
  resolveJobMarkdownContract,
  resolveResourceUrl,
} from "../job/artifacts.js";
import { firstNonEmpty } from "./core.js";

function artifactDisplayItem(job, ...keys) {
  const items = Array.isArray(job?.artifacts_display) ? job.artifacts_display : [];
  return items.find((item) => keys.includes(item?.key) || keys.includes(item?.kind)) || null;
}

function artifactDisplayReady(job, ...keys) {
  const item = artifactDisplayItem(job, ...keys);
  return Boolean(item?.ready);
}

function artifactDisplayUrl(job, ...keys) {
  const item = artifactDisplayItem(job, ...keys);
  return resolveResourceUrl(firstNonEmpty(item?.download_url, item?.url, item?.path));
}

function withIncludeJobDir(url) {
  return appendResourceQuery(url, { include_job_dir: "true" });
}

export function resolveJobActions(job) {
  const artifacts = job.artifacts || {};
  const links = job.links || {};
  const actions = job.actions || {};
  const artifactActions = artifacts.actions || {};
  const markdownContract = resolveJobMarkdownContract(job);
  const bundleEnabled = Boolean(
    actions.download_bundle?.enabled
    || artifactActions.download_bundle?.enabled
    || artifacts.bundle?.ready
    || artifacts.bundle_ready
    || job.bundle_ready
    || artifactDisplayReady(job, "bundle", "download_bundle", "archive")
  );
  const pdfEnabled = Boolean(
    actions.download_pdf?.enabled
    || artifactActions.download_pdf?.enabled
    || artifacts.pdf?.ready
    || artifacts.pdf_ready
    || job.pdf_ready
    || job.output_pdf_ready
    || artifactDisplayReady(job, "output_pdf", "pdf", "translated_pdf", "result_pdf")
  );
  const markdownJsonEnabled = Boolean(
    actions.open_markdown?.enabled
    || artifactActions.open_markdown?.enabled
    || markdownContract.ready
    || artifactDisplayReady(job, "markdown")
  );
  const markdownRawEnabled = Boolean(
    actions.open_markdown_raw?.enabled
    || artifactActions.open_markdown_raw?.enabled
    || markdownContract.ready
    || artifactDisplayReady(job, "markdown")
  );
  const rerunEnabled = Boolean(actions.rerun?.enabled ?? artifactActions.rerun?.enabled);
  return {
    cancelEnabled: Boolean(actions.cancel?.enabled ?? artifactActions.cancel?.enabled ?? (job.status === "queued" || job.status === "running")),
    rerunEnabled,
    bundleEnabled,
    pdfEnabled,
    markdownJsonEnabled,
    markdownRawEnabled,
    cancel: resolveResourceUrl(firstNonEmpty(
      actions.cancel?.url,
      artifactActions.cancel?.url,
      actions.cancel_url,
      links.cancel_url,
      links.cancel_path,
    )),
    rerun: resolveResourceUrl(firstNonEmpty(
      actions.rerun?.url,
      artifactActions.rerun?.url,
      actions.rerun?.path,
      artifactActions.rerun?.path,
      actions.rerun_url,
      links.rerun_url,
      links.rerun_path,
    )),
    bundle: resolveResourceUrl(firstNonEmpty(
      actions.download_bundle?.url,
      actions.download_bundle?.path,
      artifactActions.download_bundle?.url,
      artifactActions.download_bundle?.path,
      artifacts.bundle?.url,
      artifacts.bundle?.path,
      artifacts.bundle_url,
      artifacts.bundle_path,
      job.bundle_url,
      job.bundle_path,
      artifactDisplayUrl(job, "bundle", "download_bundle", "archive"),
    )),
    pdf: resolveResourceUrl(firstNonEmpty(
      actions.download_pdf?.url,
      actions.download_pdf?.path,
      artifactActions.download_pdf?.url,
      artifactActions.download_pdf?.path,
      artifacts.pdf?.url,
      artifacts.pdf?.path,
      artifacts.pdf_url,
      artifacts.pdf_path,
      job.pdf_url,
      job.pdf_path,
      artifactDisplayUrl(job, "output_pdf", "pdf", "translated_pdf", "result_pdf"),
    )),
    markdownJson: markdownContract.jsonUrl || artifactDisplayUrl(job, "markdown") || resolveResourceUrl(firstNonEmpty(
      actions.open_markdown?.url,
      actions.open_markdown?.path,
      artifactActions.open_markdown?.url,
      artifactActions.open_markdown?.path,
    )),
    markdownRaw: markdownContract.rawUrl || artifactDisplayUrl(job, "markdown") || resolveResourceUrl(firstNonEmpty(
      actions.open_markdown_raw?.url,
      actions.open_markdown_raw?.path,
      artifactActions.open_markdown_raw?.url,
      artifactActions.open_markdown_raw?.path,
    )),
  };
}

export function resolveJobMarkdownBundleAction(job, manifestPayload = null) {
  return resolveMarkdownBundleAction(job, manifestPayload, "markdown");
}

export function resolveJobTranslatedMarkdownBundleAction(job, manifestPayload = null) {
  return resolveMarkdownBundleAction(job, manifestPayload, "translated_markdown");
}

// 原文（markdown）与译文（translated_markdown）两种 markdown zip 共用同一套
// 解析形状，只有 artifact key / 字段前缀不同；manifest 优先，job 载荷兜底。
function resolveMarkdownBundleAction(job, manifestPayload, prefix) {
  const artifacts = job?.artifacts || {};
  const actions = job?.actions || {};
  const artifactActions = artifacts.actions || {};
  const zipKey = `${prefix}_bundle_zip`;
  const bundleKey = `${prefix}_bundle`;
  const manifestUrl = resolveManifestArtifactUrl(manifestPayload, zipKey, {
    includeJobDir: true,
  });
  const url = withIncludeJobDir(resolveResourceUrl(firstNonEmpty(
    manifestUrl,
    actions[`download_${prefix}_bundle`]?.url,
    actions[`download_${prefix}_bundle`]?.path,
    actions[`download_${prefix}_zip`]?.url,
    actions[`download_${prefix}_zip`]?.path,
    artifactActions[`download_${prefix}_bundle`]?.url,
    artifactActions[`download_${prefix}_bundle`]?.path,
    artifactActions[`download_${prefix}_zip`]?.url,
    artifactActions[`download_${prefix}_zip`]?.path,
    artifacts[zipKey]?.url,
    artifacts[zipKey]?.path,
    artifacts[bundleKey]?.url,
    artifacts[bundleKey]?.path,
    artifacts[`${prefix}_zip`]?.url,
    artifacts[`${prefix}_zip`]?.path,
    artifacts[`${zipKey}_url`],
    artifacts[`${zipKey}_path`],
    artifacts[`${bundleKey}_url`],
    artifacts[`${bundleKey}_path`],
    job?.[`${zipKey}_url`],
    job?.[`${zipKey}_path`],
    job?.[`${bundleKey}_url`],
    job?.[`${bundleKey}_path`],
    artifactDisplayUrl(job, zipKey, bundleKey, `${prefix}_zip`),
  )));
  const ready = Boolean(
    hasReadyManifestArtifact(manifestPayload, zipKey)
    || actions[`download_${prefix}_bundle`]?.enabled
    || actions[`download_${prefix}_zip`]?.enabled
    || artifactActions[`download_${prefix}_bundle`]?.enabled
    || artifactActions[`download_${prefix}_zip`]?.enabled
    || artifacts[zipKey]?.ready
    || artifacts[bundleKey]?.ready
    || artifacts[`${prefix}_zip`]?.ready
    || artifacts[`${zipKey}_ready`]
    || artifacts[`${bundleKey}_ready`]
    || artifacts[`${prefix}_zip_ready`]
    || job?.[`${zipKey}_ready`]
    || job?.[`${bundleKey}_ready`]
    || artifactDisplayReady(job, zipKey, bundleKey, `${prefix}_zip`)
    || url
  );
  return {
    ready,
    url,
  };
}

export function resolveJobSourcePdfAction(job, manifestPayload = null) {
  const artifacts = job?.artifacts || {};
  const manifestUrl = resolveManifestArtifactUrl(manifestPayload, "source_pdf");
  const fallbackUrl = job?.job_id
    ? `/api/v1/jobs/${encodeURIComponent(job.job_id)}/artifacts/source_pdf`
    : "";
  const url = resolveResourceUrl(firstNonEmpty(
    manifestUrl,
    artifacts.source_pdf?.url,
    artifacts.source_pdf?.path,
    artifacts.source_pdf_url,
    artifacts.source_pdf_path,
    job?.source_pdf_url,
    job?.source_pdf_path,
    fallbackUrl,
  ));
  const ready = Boolean(
    hasReadyManifestArtifact(manifestPayload, "source_pdf")
    || artifacts.source_pdf?.ready
    || artifacts.source_pdf_ready
    || job?.source_pdf_ready
  );
  return {
    ready,
    url,
  };
}
