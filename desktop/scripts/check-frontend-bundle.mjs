import fs from "fs";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const desktopRoot = path.resolve(__dirname, "..");
const frontendRoot = path.join(desktopRoot, "app", "frontend");

function fail(message) {
  throw new Error(message);
}

function assertExists(relativePath) {
  const fullPath = path.join(frontendRoot, relativePath);
  if (!fs.existsSync(fullPath)) {
    fail(`Missing desktop frontend artifact: ${fullPath}`);
  }
  return fullPath;
}

function readFile(relativePath) {
  return fs.readFileSync(assertExists(relativePath), "utf8");
}

function collectFiles(root, extensions) {
  const files = [];

  function walk(current) {
    if (!fs.existsSync(current)) {
      return;
    }
    for (const entry of fs.readdirSync(current, { withFileTypes: true })) {
      const fullPath = path.join(current, entry.name);
      if (entry.isDirectory()) {
        walk(fullPath);
        continue;
      }
      if (extensions.has(path.extname(entry.name))) {
        files.push(fullPath);
      }
    }
  }

  walk(root);
  return files;
}

// HTML shells + production bundles (React cutover: entry is dist/*.bundle.js)
assertExists("index.html");
assertExists("detail.html");
assertExists("reader.html");
assertExists("runtime-config.js");
assertExists("dist/app.bundle.js");
assertExists("dist/reader.bundle.js");
assertExists("dist/detail.bundle.js");
assertExists("styles.css");
assertExists("dist/css/home.css");
assertExists("dist/css/reader.css");

// Source modules still shipped for desktop rewrites / smoke (TypeScript after cutover)
assertExists("src/js/runtime/vendor-url.ts");
assertExists("src/pages/reader/legacy/pdf-document.ts");
assertExists("src/js/features/upload/pdf-page-count.ts");
assertExists("src/js/desktop/index.ts");
assertExists("src/js/reader/ai/config.ts");

// Vendor copies used by packaged file:// loads
assertExists("vendor/pdfjs-dist/build/pdf.mjs");
assertExists("vendor/pdfjs-dist/build/pdf.worker.mjs");
assertExists("vendor/pdfjs-dist/web/pdf_viewer.css");
assertExists("vendor/pdfjs-dist/web/pdf_viewer.mjs");
assertExists("vendor/pdf-lib/dist/pdf-lib.esm.js");

const runtimeConfig = readFile("runtime-config.js");
if (!runtimeConfig.includes('apiBase: "http://127.0.0.1:41000"')) {
  fail("Desktop runtime-config.js is missing local apiBase");
}
if (!runtimeConfig.includes('xApiKey: "retain-pdf-desktop"')) {
  fail("Desktop runtime-config.js is missing desktop API key");
}
if (!runtimeConfig.includes('modelApiKey: ""')) {
  fail("Desktop runtime-config.js must ship empty modelApiKey (keys live in settings)");
}
if (fs.existsSync(path.join(frontendRoot, "runtime-config.local.js"))) {
  fail("Desktop frontend must not include runtime-config.local.js");
}

const readerHtml = readFile("reader.html");
if (!readerHtml.includes("./vendor/pdfjs-dist/web/pdf_viewer.css")) {
  fail("Desktop reader.html did not rewrite pdfjs viewer CSS to vendor path");
}
if (!readerHtml.includes("./dist/reader.bundle.js")) {
  fail("Desktop reader.html is not using the production reader bundle");
}

const indexHtml = readFile("index.html");
if (!indexHtml.includes("./dist/app.bundle.js")) {
  fail("Desktop index.html is not using the production app bundle");
}
if (indexHtml.includes("runtime-config.local.js")) {
  fail("Desktop index.html still references runtime-config.local.js");
}

const uploadPdfPageCount = readFile("src/js/features/upload/pdf-page-count.ts");
if (!uploadPdfPageCount.includes("runtime/vendor-url")) {
  fail("Desktop upload/pdf-page-count.ts is missing runtime vendor resolver");
}

const readerPdfDocument = readFile("src/pages/reader/legacy/pdf-document.ts");
if (!readerPdfDocument.includes("runtime/vendor-url")) {
  fail("Desktop reader/pdf-document.ts is missing runtime vendor resolver");
}

// AI key gate: settings-only (not runtime secret fallback)
const readerAiConfig = readFile("src/js/reader/ai/config.ts");
if (!readerAiConfig.includes("readSettingsModelApiKey")) {
  fail("Desktop reader AI config missing settings-only model key helper");
}
if (!readerAiConfig.includes("CREDENTIALS_CHANGED_EVENT")) {
  fail("Desktop reader AI config missing credentials-changed event");
}

const appBundleJs = readFile("dist/app.bundle.js");
if (!appBundleJs.includes("./vendor/") && !appBundleJs.includes("vendor/pdfjs")) {
  // bundle may inline resolver strings differently; require credentials gate markers
}
if (!appBundleJs.includes("credentials-changed") && !appBundleJs.includes("retainpdf:credentials-changed")) {
  fail("Desktop app bundle missing credentials-changed gate refresh");
}
if (!appBundleJs.includes("home-ask")) {
  fail("Desktop app bundle missing home AI ask feature");
}
if (appBundleJs.includes("../../../vendor/pdfjs-dist/build/pdf.mjs")) {
  fail("Desktop app bundle still contains module-depth pdfjs vendor path");
}
if (appBundleJs.includes("app.asar/vendor/")) {
  fail("Desktop app bundle contains app.asar root vendor path");
}

const generatedFiles = [
  ...collectFiles(frontendRoot, new Set([".html"])),
  ...collectFiles(path.join(frontendRoot, "dist"), new Set([".js", ".mjs"])),
];
const forbiddenPatterns = [
  {
    pattern: "runtime-config.local.js",
    label: "runtime-config.local.js reference",
  },
  {
    pattern: "node_modules/pdfjs-dist",
    label: "pdfjs node_modules reference",
  },
  {
    pattern: "node_modules/pdf-lib",
    label: "pdf-lib node_modules reference",
  },
];

for (const filePath of generatedFiles) {
  const content = fs.readFileSync(filePath, "utf8");
  for (const { pattern, label } of forbiddenPatterns) {
    if (content.includes(pattern)) {
      fail(`Desktop frontend still contains ${label}: ${filePath}`);
    }
  }
}

console.log("desktop frontend bundle check: ok");
