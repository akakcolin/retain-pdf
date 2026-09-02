import { defaultArtifactRuntimePort } from "./artifact-runtime-port.js";
import { defaultArtifactUrlConfigPort } from "./artifact-url-config.js";
import { targetFilePrefix } from "../config/languages.js";
import type {
  ArtifactRuntimeState,
  ArtifactUrlQuery,
  ArtifactUrlResolveOptions,
  JobAction,
  JobArtifacts,
  JobLike,
  JobPayload,
  JobRequestPayload,
  ManifestArtifactItem,
  ManifestPayload,
  MarkdownContract,
} from "./types.js";

export type {
  ArtifactRuntimeState,
  ArtifactUrlQuery,
  ArtifactUrlResolveOptions,
  JobLike,
  JobPayload,
  ManifestArtifactItem,
  ManifestPayload,
  MarkdownContract,
} from "./types.js";

function trimString(value: unknown): string {
  return typeof value === "string" ? value.trim() : "";
}

function stripExtension(filename: unknown): string {
  const normalized = trimString(filename);
  if (!normalized) {
    return "";
  }
  const index = normalized.lastIndexOf(".");
  if (index <= 0) {
    return normalized;
  }
  return normalized.slice(0, index);
}

function sanitizeFilenamePart(value: unknown): string {
  return `${value || ""}`.replace(/[\\/:*?"<>|]+/g, "_").trim();
}

function defaultBaseHref(): string {
  return globalThis.window?.location?.href || "http://localhost/";
}

function basenameFromUrlLike(
  value: unknown,
  { baseHref = defaultBaseHref() }: { baseHref?: string } = {},
): string {
  const raw = trimString(value);
  if (!raw) {
    return "";
  }
  try {
    const parsed = new URL(raw, baseHref);
    const pathname = parsed.pathname || "";
    const candidate = pathname.split("/").filter(Boolean).pop() || "";
    return decodeURIComponent(candidate);
  } catch (_err) {
    const candidate = raw.split(/[/?#]/)[0]?.split("/").filter(Boolean).pop() || "";
    return candidate;
  }
}

export function resolveOriginalPdfBaseName(state: ArtifactRuntimeState = {}): string {
  const snapshot = (defaultArtifactRuntimePort.currentJobSnapshot(state) || {}) as JobLike;
  const jobId = `${snapshot.job_id || defaultArtifactRuntimePort.currentJobId(state) || ""}`.trim();
  const uploadState = defaultArtifactRuntimePort.uploadSnapshot(state) || {};
  const requestPayload = (snapshot.request_payload || {}) as JobRequestPayload;
  const rawResponse = (snapshot.raw_response || {}) as JobLike;
  const bookSummary = (rawResponse.book_summary || {}) as {
    source_file_name?: string;
    title?: string;
  };
  const sourceArtifact = findReadyManifestArtifact(
    defaultArtifactRuntimePort.cachedManifestFor(state, jobId),
    "source_pdf",
  );
  const candidates = [
    uploadState.uploadedFileName,
    rawResponse.filename,
    rawResponse.file_name,
    rawResponse.source_file_name,
    rawResponse.display_name,
    rawResponse.original_filename,
    rawResponse.original_file_name,
    bookSummary.source_file_name,
    bookSummary.title,
    requestPayload.filename,
    requestPayload.file_name,
    requestPayload.original_filename,
    requestPayload.original_file_name,
    requestPayload.source_filename,
    requestPayload.source_file_name,
    sourceArtifact?.filename,
    sourceArtifact?.file_name,
    sourceArtifact?.name,
    basenameFromUrlLike(sourceArtifact?.resource_path),
    basenameFromUrlLike(sourceArtifact?.resource_url),
  ];
  const originalName = candidates.find((value) => typeof value === "string" && value.trim()) || "";
  return sanitizeFilenamePart(stripExtension(originalName));
}

/** 从 job 快照解析译文目标语言代码；缺省空串（沿用旧 zh 前缀命名）。 */
function readTranslatedTargetLang(state: ArtifactRuntimeState = {}): string {
  const snapshot = (defaultArtifactRuntimePort.currentJobSnapshot(state) || {}) as JobLike;
  const requestPayload = (snapshot.request_payload || {}) as JobRequestPayload & Record<string, unknown>;
  const translation = (requestPayload.translation || {}) as Record<string, unknown>;
  const rawResponse = (snapshot.raw_response || {}) as JobLike & Record<string, unknown>;
  const rawRequestPayload = (rawResponse.request_payload || {}) as Record<string, unknown>;
  const rawTranslation = (rawRequestPayload.translation || {}) as Record<string, unknown>;
  const bookSummary = (rawResponse.book_summary || {}) as Record<string, unknown>;
  const candidates = [
    translation.target_lang,
    requestPayload.target_lang,
    rawTranslation.target_lang,
    rawRequestPayload.target_lang,
    rawResponse.target_lang,
    bookSummary.target_lang,
  ];
  const found = candidates.find((value) => typeof value === "string" && value.trim());
  return `${found || ""}`.trim();
}

export function resolveTranslatedPdfDownloadName(
  state: ArtifactRuntimeState = {},
  fallbackName = "",
): string {
  const originalName = resolveOriginalPdfBaseName(state);
  if (!originalName) {
    return fallbackName;
  }
  const prefix = targetFilePrefix(readTranslatedTargetLang(state));
  return `${prefix}_${originalName}.pdf`;
}

export function resolveSourcePdfDownloadName(
  state: ArtifactRuntimeState = {},
  fallbackName = "",
): string {
  const originalName = resolveOriginalPdfBaseName(state);
  return originalName ? `${originalName}.pdf` : fallbackName;
}

function ensureTrailingSlash(value: unknown): string {
  const trimmed = trimString(value);
  if (!trimmed) {
    return "";
  }
  return trimmed.endsWith("/") ? trimmed : `${trimmed}/`;
}

function normalizedApiBase(resolveApiBase?: (() => string) | null): string {
  return trimString(resolveApiBase?.())
    .replace(/\/+$/, "")
    .replace(/\/api\/v1$/i, "");
}

export function toAbsoluteApiUrl(value: unknown): string {
  return resolveResourceUrl(value);
}

export function appendResourceQuery(url: unknown, query: ArtifactUrlQuery = {}): string {
  const normalized = trimString(url);
  if (!normalized) {
    return "";
  }
  const entries = Object.entries(query || {})
    .filter(([, value]) => value !== undefined && value !== null && `${value}` !== "");
  if (!entries.length) {
    return normalized;
  }
  let nextUrl = normalized;
  entries.forEach(([key, value]) => {
    const encodedKey = encodeURIComponent(key);
    if (new RegExp(`(?:[?&])${encodedKey}=`).test(nextUrl)) {
      return;
    }
    const separator = nextUrl.includes("?") ? "&" : "?";
    nextUrl = `${nextUrl}${separator}${encodedKey}=${encodeURIComponent(`${value}`)}`;
  });
  return nextUrl;
}

export function createArtifactUrlResolver({
  resolveApiBase = defaultArtifactUrlConfigPort.resolveApiBase,
}: {
  resolveApiBase?: () => string;
} = {}) {
  function resolve(value: unknown, { query = null }: { query?: ArtifactUrlQuery | null } = {}): string {
    const trimmed = trimString(value);
    if (!trimmed) {
      return "";
    }
    let absolute = "";
    const base = normalizedApiBase(resolveApiBase);
    if (/^[a-z][a-z\d+\-.]*:/i.test(trimmed)) {
      absolute = trimmed;
    } else if (trimmed.startsWith("/")) {
      absolute = `${base}${trimmed}`;
    } else {
      absolute = `${base}/${trimmed.replace(/^\.?\//, "")}`;
    }
    return query ? appendResourceQuery(absolute, query) : absolute;
  }

  return Object.freeze({
    resolve,
    toAbsolute: resolve,
  });
}

export const defaultArtifactUrlResolver = createArtifactUrlResolver();

export function resolveResourceUrl(
  value: unknown,
  options: ArtifactUrlResolveOptions = {},
): string {
  const { resolver = defaultArtifactUrlResolver, ...resolveOptions } = options || {};
  if (resolver?.resolve) {
    return resolver.resolve(value, resolveOptions);
  }
  return defaultArtifactUrlResolver.resolve(value, resolveOptions);
}

export function findReadyManifestArtifact(
  manifestPayload: ManifestPayload | null | undefined,
  artifactKey: string,
): ManifestArtifactItem | null {
  const items = Array.isArray(manifestPayload?.items) ? manifestPayload.items : [];
  return items.find((entry) => entry?.artifact_key === artifactKey && entry?.ready) || null;
}

export function hasReadyManifestArtifact(
  manifestPayload: ManifestPayload | null | undefined,
  artifactKey: string,
): boolean {
  return Boolean(findReadyManifestArtifact(manifestPayload, artifactKey));
}

export function resolveManifestArtifactUrl(
  manifestPayload: ManifestPayload | null | undefined,
  artifactKey: string,
  { includeJobDir = false }: { includeJobDir?: boolean } = {},
): string {
  const item = findReadyManifestArtifact(manifestPayload, artifactKey);
  const raw = trimString(item?.resource_url || item?.resource_path);
  if (!raw) {
    return "";
  }
  const jobDirKeys = ["markdown_bundle_zip", "translated_markdown_bundle_zip"];
  return resolveResourceUrl(raw, {
    query: includeJobDir && jobDirKeys.includes(artifactKey)
      ? { include_job_dir: "true" }
      : null,
  });
}

export function resolveJobMarkdownContract(job: JobLike | JobPayload | null | undefined): MarkdownContract {
  const artifacts = (job?.artifacts || {}) as JobArtifacts;
  const markdown = artifacts.markdown || {};
  const actions = (job?.actions || {}) as Record<string, JobAction>;
  const ready = Boolean(
    markdown.ready
    ?? artifacts.markdown_ready
    ?? job?.markdown_ready
    ?? actions.open_markdown?.enabled
    ?? actions.open_markdown_raw?.enabled
  );
  return {
    ready,
    jsonUrl: resolveResourceUrl(markdown.json_url || markdown.json_path || actions.open_markdown?.url || actions.open_markdown?.path),
    rawUrl: resolveResourceUrl(markdown.raw_url || markdown.raw_path || actions.open_markdown_raw?.url || actions.open_markdown_raw?.path),
    imagesBaseUrl: ensureTrailingSlash(resolveResourceUrl(
      markdown.images_base_url || markdown.images_base_path || artifacts.markdown_images_base_url
    )),
    fileName: trimString(markdown.file_name),
    sizeBytes: Number.isFinite(Number(markdown.size_bytes)) ? Number(markdown.size_bytes) : null,
  };
}

export function resolveMarkdownAssetUrl(imagesBaseUrl: unknown, relativePath: unknown): string {
  const target = trimString(relativePath);
  if (!target) {
    return "";
  }
  if (/^(?:data:|blob:|https?:\/\/|#|mock:\/\/)/i.test(target)) {
    return target;
  }
  if (target.startsWith("/")) {
    return resolveResourceUrl(target);
  }
  const base = ensureTrailingSlash(imagesBaseUrl);
  if (!base) {
    return target;
  }
  // images_base 已是 .../markdown/images/，path 常为 images/page-1/...
  // 若直接 new URL 会变成 .../images/images/...（双前缀 404）
  let rel = target.replace(/\\/g, "/").replace(/^\.\//, "");
  while (rel.startsWith("images/")) {
    rel = rel.slice("images/".length);
  }
  try {
    return new URL(rel, base).toString();
  } catch {
    return `${base}${rel}`;
  }
}

function normalizeMarkdownImageTarget(rawTarget: unknown): string {
  const trimmed = trimString(rawTarget);
  if (!trimmed) {
    return "";
  }
  let normalized = trimmed;
  const titleIndex = normalized.search(/\s+["']/);
  if (titleIndex > 0) {
    normalized = normalized.slice(0, titleIndex);
  }
  if (normalized.startsWith("<") && normalized.endsWith(">")) {
    normalized = normalized.slice(1, -1).trim();
  }
  return normalized;
}

export function collectMarkdownImageRefs(content: unknown): string[] {
  const text = `${content || ""}`;
  if (!text.trim()) {
    return [];
  }
  const refs: string[] = [];
  const seen = new Set<string>();

  const pushRef = (candidate: unknown) => {
    const normalized = normalizeMarkdownImageTarget(candidate);
    if (!normalized || seen.has(normalized)) {
      return;
    }
    seen.add(normalized);
    refs.push(normalized);
  };

  const htmlImgPattern = /<img\b[^>]*\bsrc=(["'])(.*?)\1[^>]*>/gi;
  for (const match of text.matchAll(htmlImgPattern)) {
    pushRef(match[2]);
  }

  const markdownImgPattern = /!\[[^\]]*]\(([^)]+)\)/g;
  for (const match of text.matchAll(markdownImgPattern)) {
    pushRef(match[1]);
  }

  return refs;
}
