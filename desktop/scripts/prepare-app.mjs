import fs from "fs";
import path from "path";
import { spawnSync } from "child_process";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const desktopRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(desktopRoot, "..");
const versionFile = path.join(repoRoot, "VERSION");
const frontendRoot = path.join(repoRoot, "frontend");
const backendRoot = path.join(repoRoot, "backend");
const desktopSrcRoot = path.join(desktopRoot, "src");
const desktopRuntimeRoot = path.join(desktopSrcRoot, "runtime");
const targetPlatform = process.env.RETAIN_PDF_DESKTOP_PLATFORM || process.platform;
const allowBundledMacPython = process.env.RETAIN_PDF_BUNDLE_MAC_PYTHON === "1";
const skipBundledRuntimeVerification = process.env.RETAIN_PDF_SKIP_BUNDLED_RUNTIME_VERIFICATION === "1";
const frontendOnly = process.argv.includes("--frontend-only");
const appRoot = path.join(desktopRoot, "app");
const outputFrontendRoot = path.join(appRoot, "frontend");
const outputBackendRoot = path.join(appRoot, "backend");
const outputFrontendVendorRoot = path.join(outputFrontendRoot, "vendor");
const bundledFontsRoot = path.join(outputBackendRoot, "fonts");
const buildRoot = path.join(desktopRoot, "build");
const linuxIconsRoot = path.join(buildRoot, "icons");
const desktopIconSource = path.join(desktopRoot, "assets", "RetainPDF-logo.png");
const desktopPackagePath = path.join(desktopRoot, "package.json");
const desktopPackage = JSON.parse(fs.readFileSync(desktopPackagePath, "utf8"));

function normalizeTargetPlatformName(platform = targetPlatform) {
  if (platform === "darwin" || platform === "mac") {
    return "mac";
  }
  if (platform === "win32" || platform === "windows") {
    return "windows";
  }
  if (platform === "linux") {
    return "linux";
  }
  throw new Error(`unsupported desktop target platform: ${platform}`);
}

const targetPlatformName = normalizeTargetPlatformName();

function resolvePlatformRuntimeDir(platformName = targetPlatformName) {
  return path.join(desktopRuntimeRoot, platformName);
}

function resolveRuntimeCandidate(relativePath) {
  const platformRoot = resolvePlatformRuntimeDir();
  const desktopCandidate = path.join(platformRoot, relativePath);
  if (fs.existsSync(desktopCandidate)) {
    return desktopCandidate;
  }

  if (targetPlatformName === "mac" && relativePath === "python" && allowBundledMacPython) {
    return desktopCandidate;
  }

  const legacyCandidates = {
    "python": path.join(backendRoot, "python"),
    "typst": {
      win32: path.join(backendRoot, "typst-win32"),
      darwin: path.join(backendRoot, "typst-darwin"),
      linux: path.join(backendRoot, "typst-linux"),
    }[targetPlatform],
  };

  const legacyCandidate = legacyCandidates[relativePath];
  return legacyCandidate && fs.existsSync(legacyCandidate) ? legacyCandidate : desktopCandidate;
}

function resolveSharedRuntimePath(relativePath) {
  const desktopCandidate = path.join(desktopRuntimeRoot, "shared", relativePath);
  if (fs.existsSync(desktopCandidate)) {
    return desktopCandidate;
  }
  const legacyCandidates = {
    "typst-packages": path.join(backendRoot, "typst-packages"),
    "fonts": [
      path.join(backendRoot, "fonts"),
      path.join(desktopRoot, "assets", "fonts"),
    ],
  };
  const legacyCandidate = legacyCandidates[relativePath];
  if (Array.isArray(legacyCandidate)) {
    const match = legacyCandidate.find((candidate) => fs.existsSync(candidate));
    return match || desktopCandidate;
  }
  return legacyCandidate && fs.existsSync(legacyCandidate) ? legacyCandidate : desktopCandidate;
}

function resolveSharedRuntimePaths(relativePath) {
  const candidates = [];
  const desktopCandidate = path.join(desktopRuntimeRoot, "shared", relativePath);
  if (fs.existsSync(desktopCandidate)) {
    candidates.push(desktopCandidate);
  }
  if (relativePath === "fonts") {
    for (const candidate of [
      path.join(backendRoot, "fonts"),
      path.join(desktopRoot, "assets", "fonts"),
    ]) {
      if (fs.existsSync(candidate)) {
        candidates.push(candidate);
      }
    }
  } else {
    const legacyCandidate = resolveSharedRuntimePath(relativePath);
    if (legacyCandidate !== desktopCandidate && fs.existsSync(legacyCandidate)) {
      candidates.push(legacyCandidate);
    }
  }
  return [...new Set(candidates)];
}

function copyRuntimeTree(from, to, options = {}) {
  const dereference = options.dereference === true;
  fs.cpSync(from, to, {
    recursive: true,
    force: true,
    dereference,
  });
}

/// Per-component bundle sizes in bytes, keyed by the component names the
/// bundle-size CI report renders (all paths relative to `outputBackendRoot`,
/// the manifest's own directory). `total` is the plain sum of the components.
function buildSizesBytes() {
  const frontendRel = path.relative(outputBackendRoot, outputFrontendRoot);
  const sizes = {
    rustApi: dirSize(outputBackendRoot, path.join("bin", rustApiBinary.fileName)),
    renderRs: dirSize(outputBackendRoot, path.join("bin", renderRsBinary.fileName)),
    python: dirSize(outputBackendRoot, "python"),
    typst: dirSize(outputBackendRoot, "typst"),
    typstPackages: dirSize(outputBackendRoot, "typst-packages"),
    fonts: dirSize(outputBackendRoot, "fonts"),
    scripts: dirSize(outputBackendRoot, "scripts"),
    frontend: dirSize(outputBackendRoot, frontendRel),
  };
  sizes.total = Object.values(sizes).reduce((sum, value) => sum + value, 0);
  return sizes;
}

/// Sum of regular-file bytes under `root` resolved with `rel` (symlinks are
/// skipped to avoid double-counting linked-in files). Returns 0 for a missing
/// path so the manifest can report sizes unconditionally.
function dirSize(root, rel) {
  const target = path.resolve(root, rel);
  if (!fs.existsSync(target)) {
    return 0;
  }
  const targetStat = fs.statSync(target);
  if (targetStat.isFile()) {
    return targetStat.size;
  }
  if (!targetStat.isDirectory()) {
    return 0;
  }
  let total = 0;
  const stack = [target];
  while (stack.length) {
    const dir = stack.pop();
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
      const full = path.join(dir, entry.name);
      let isDirectory;
      let isFile;
      try {
        const stat = fs.lstatSync(full);
        isDirectory = stat.isDirectory();
        isFile = stat.isFile();
      } catch {
        continue;
      }
      if (isDirectory) {
        stack.push(full);
      } else if (isFile) {
        total += fs.statSync(full).size;
      }
    }
  }
  return total;
}

function rewriteAbsoluteSymlinksWithinRoot(root, sourceRoot) {
  if (!fs.existsSync(root) || !fs.existsSync(sourceRoot)) {
    return;
  }
  const normalizedRoot = path.resolve(root);
  const normalizedSourceRoot = path.resolve(sourceRoot);

  function visit(currentPath) {
    const entries = fs.readdirSync(currentPath, { withFileTypes: true });
    for (const entry of entries) {
      const entryPath = path.join(currentPath, entry.name);
      const stats = fs.lstatSync(entryPath);
      if (stats.isSymbolicLink()) {
        const target = fs.readlinkSync(entryPath);
        if (!path.isAbsolute(target)) {
          continue;
        }
        const normalizedTarget = path.normalize(target);
        if (!normalizedTarget.startsWith(normalizedSourceRoot + path.sep)
          && normalizedTarget !== normalizedSourceRoot) {
          continue;
        }
        const suffix = path.relative(normalizedSourceRoot, normalizedTarget);
        const replacementTarget = path.join(normalizedRoot, suffix);
        const relativeTarget = path.relative(path.dirname(entryPath), replacementTarget) || ".";
        fs.unlinkSync(entryPath);
        fs.symlinkSync(relativeTarget, entryPath);
        continue;
      }
      if (stats.isDirectory()) {
        visit(entryPath);
      }
    }
  }

  visit(normalizedRoot);
}

function pruneBundledMacPythonRuntime(root) {
  if (!fs.existsSync(root)) {
    return;
  }
  const frameworkVersionsRoot = path.join(root, "Frameworks", "Python.framework", "Versions");
  const libRoot = path.join(root, "lib");
  const pythonLibDir = fs.existsSync(libRoot)
    ? fs.readdirSync(libRoot).find((entry) => /^python\d+\.\d+$/.test(entry))
    : null;
  const expectedFrameworkVersion = pythonLibDir
    ? pythonLibDir.replace(/^python/, "")
    : "";
  const removalTargets = [
    path.join(root, "Frameworks", "Python.framework", "Headers"),
    path.join(root, "Frameworks", "Python.framework", "Versions", "Current", "Frameworks", "Tk.framework"),
    path.join(root, "Frameworks", "Python.framework", "Versions", "Current", "Frameworks", "Tcl.framework"),
    path.join(root, "Frameworks", "Python.framework", "Versions", "Current", "Headers"),
    path.join(root, "Frameworks", "Python.framework", "Versions", "Current", "share", "doc"),
  ];
  if (pythonLibDir) {
    const sitePackagesRoot = path.join(libRoot, pythonLibDir, "site-packages");
    removalTargets.push(
      path.join(libRoot, pythonLibDir, "ensurepip"),
      path.join(sitePackagesRoot, "pip"),
      path.join(sitePackagesRoot, "setuptools"),
      path.join(sitePackagesRoot, "pkg_resources"),
    );
    if (fs.existsSync(sitePackagesRoot)) {
      for (const entry of fs.readdirSync(sitePackagesRoot)) {
        if (/^(pip|setuptools)-.+\.dist-info$/.test(entry)) {
          removalTargets.push(path.join(sitePackagesRoot, entry));
        }
      }
      // pymupdf/pikepdf/lxml are unreachable from the desktop's worker flow:
      // render/normalize/extract run natively (render_rs), and the always-Python
      // translate worker lazy-imports fitz
      // (translation/llm/domain_context.py),
      // degrading sci domain inference instead of importing it.
      // PIL is not imported by any bundled script. rendering_bridge is deleted
      // (Python rendering tree retired), so no bridge ships into the bundle.
      const removableSitePackages = [
        "fitz", "pymupdf", "core", // pymupdf
        "lxml", // pikepdf dependency
        "pikepdf",
        "PIL", // pillow
      ];
      const removableDistInfo =
        /^(pymupdf|lxml|pikepdf|pillow)-.+\.dist-info$/;
      for (const packageName of removableSitePackages) {
        removalTargets.push(path.join(sitePackagesRoot, packageName));
      }
      for (const entry of fs.readdirSync(sitePackagesRoot)) {
        if (removableDistInfo.test(entry)) {
          removalTargets.push(path.join(sitePackagesRoot, entry));
        }
      }
    }
  }
  for (const target of removalTargets) {
    fs.rmSync(target, { recursive: true, force: true });
  }

  const removableFiles = [
    path.join(root, "bin", "2to3"),
    path.join(root, "bin", "idle3"),
    path.join(root, "bin", "pydoc3"),
    path.join(root, "bin", "python3-config"),
  ];
  for (const target of removableFiles) {
    fs.rmSync(target, { force: true });
  }

  if (fs.existsSync(frameworkVersionsRoot)) {
    for (const entry of fs.readdirSync(frameworkVersionsRoot, { withFileTypes: true })) {
      if (!entry.isDirectory()) {
        continue;
      }
      if (entry.name === "Current" || entry.name === expectedFrameworkVersion) {
        continue;
      }
      fs.rmSync(path.join(frameworkVersionsRoot, entry.name), { recursive: true, force: true });
    }
    if (expectedFrameworkVersion) {
      const currentLink = path.join(frameworkVersionsRoot, "Current");
      fs.rmSync(currentLink, { recursive: true, force: true });
      fs.symlinkSync(expectedFrameworkVersion, currentLink);
    }
  }

  function pruneTree(currentPath) {
    if (!fs.existsSync(currentPath)) {
      return;
    }
    const entries = fs.readdirSync(currentPath, { withFileTypes: true });
    for (const entry of entries) {
      const entryPath = path.join(currentPath, entry.name);
      if (entry.isDirectory()) {
        if (entry.name === "__pycache__" || entry.name === "test" || entry.name === "tests") {
          fs.rmSync(entryPath, { recursive: true, force: true });
          continue;
        }
        pruneTree(entryPath);
      }
    }
  }

  pruneTree(root);
}

const embeddedPythonRoot = resolveRuntimeCandidate("python");
const bundledTypstRoot = resolveRuntimeCandidate("typst");
const typstPackagesRoot = resolveSharedRuntimePath("typst-packages");

function resolveGitVersion() {
  const exactTag = spawnSync("git", ["describe", "--tags", "--exact-match", "HEAD"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (exactTag.status === 0) {
    return exactTag.stdout.trim();
  }
  const described = spawnSync("git", ["describe", "--tags", "--always", "--dirty"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (described.status === 0) {
    return described.stdout.trim();
  }
  return "";
}

const releaseVersion = (process.env.RETAIN_PDF_VERSION || "").trim()
  || (desktopPackage.version || "").trim()
  || resolveGitVersion()
  || (fs.existsSync(versionFile) ? fs.readFileSync(versionFile, "utf8").trim() : "");

if (!releaseVersion) {
  throw new Error(
    `Missing release version; fallback sources RETAIN_PDF_VERSION, git describe, ${versionFile}, and package.json are all empty`,
  );
}

function resolveRustApiBinary() {
  const overridePath = process.env.RUST_API_BINARY
    ? path.resolve(process.env.RUST_API_BINARY)
    : "";
  const candidates = [overridePath];

  if (targetPlatform === "win32") {
    candidates.push(
      path.join(
        backendRoot,
        "rust_api",
        "target",
        "x86_64-pc-windows-msvc",
        "release",
        "rust_api.exe",
      ),
      path.join(
        backendRoot,
        "rust_api",
        "target",
        "i686-pc-windows-msvc",
        "release",
        "rust_api.exe",
      ),
      path.join(
        backendRoot,
        "rust_api",
        "target",
        "i686-pc-windows-gnu",
        "release",
        "rust_api.exe",
      ),
    );
  } else if (targetPlatform === "darwin") {
    candidates.push(
      path.join(backendRoot, "rust_api", "target", "release", "rust_api"),
      path.join(backendRoot, "rust_api", "target", "x86_64-apple-darwin", "release", "rust_api"),
      path.join(backendRoot, "rust_api", "target", "aarch64-apple-darwin", "release", "rust_api"),
    );
  } else {
    candidates.push(path.join(backendRoot, "rust_api", "target", "release", "rust_api"));
  }

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return {
        path: candidate,
        fileName: path.basename(candidate),
      };
    }
  }

  return {
    path: candidates[0] || "",
    fileName: targetPlatform === "win32" ? "rust_api.exe" : "rust_api",
  };
}

function resolveRenderRsBinary() {
  const overridePath = process.env.RENDER_RS_BINARY
    ? path.resolve(process.env.RENDER_RS_BINARY)
    : "";
  const candidates = [overridePath];

  if (targetPlatform === "win32") {
    candidates.push(
      path.join(
        backendRoot,
        "rendering_orchestrator",
        "target",
        "x86_64-pc-windows-msvc",
        "release",
        "render_rs.exe",
      ),
    );
  } else if (targetPlatform === "darwin") {
    candidates.push(
      path.join(backendRoot, "rendering_orchestrator", "target", "release", "render_rs"),
      path.join(backendRoot, "rendering_orchestrator", "target", "x86_64-apple-darwin", "release", "render_rs"),
      path.join(backendRoot, "rendering_orchestrator", "target", "aarch64-apple-darwin", "release", "render_rs"),
    );
  } else {
    candidates.push(path.join(backendRoot, "rendering_orchestrator", "target", "release", "render_rs"));
  }

  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return {
        path: candidate,
        fileName: path.basename(candidate),
      };
    }
  }

  return {
    path: candidates[0] || "",
    fileName: targetPlatform === "win32" ? "render_rs.exe" : "render_rs",
  };
}

function hasBundledPosixPython(root) {
  return fs.existsSync(path.join(root, "bin", "python3"))
    || fs.existsSync(path.join(root, "bin", "python"));
}

function resolveBundledPythonCommand(root) {
  const candidates = targetPlatform === "win32"
    ? [path.join(root, "python.exe")]
    : [
        path.join(root, "bin", "python3"),
        path.join(root, "bin", "python"),
      ];
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
    }
  }
  return "";
}

function bundledPythonSitePackages(root) {
  if (!root || !fs.existsSync(root)) {
    return [];
  }
  if (targetPlatform === "win32") {
    const sitePackages = path.join(root, "Lib", "site-packages");
    return fs.existsSync(sitePackages) ? [sitePackages] : [];
  }
  const libRoot = path.join(root, "lib");
  if (!fs.existsSync(libRoot)) {
    return [];
  }
  const matches = [];
  for (const entry of fs.readdirSync(libRoot)) {
    if (!/^python\d+\.\d+$/.test(entry)) {
      continue;
    }
    const sitePackages = path.join(libRoot, entry, "site-packages");
    if (fs.existsSync(sitePackages)) {
      matches.push(sitePackages);
    }
  }
  return matches;
}

function bundledPythonLibDynload(root) {
  if (!root || !fs.existsSync(root) || targetPlatform !== "darwin") {
    return [];
  }
  const pythonHome = resolveBundledPythonHome(root);
  const libRoot = pythonHome ? path.join(pythonHome, "lib") : "";
  if (!libRoot || !fs.existsSync(libRoot)) {
    return [];
  }
  const matches = [];
  for (const entry of fs.readdirSync(libRoot)) {
    if (!/^python\d+\.\d+$/.test(entry)) {
      continue;
    }
    const libDynload = path.join(libRoot, entry, "lib-dynload");
    if (fs.existsSync(libDynload)) {
      matches.push(libDynload);
    }
  }
  return matches;
}

function bundledPythonImportPaths(root) {
  return [
    ...bundledPythonSitePackages(root),
    ...bundledPythonLibDynload(root),
  ];
}

function resolveBundledPythonHome(root) {
  if (!root || !fs.existsSync(root)) {
    return "";
  }
  if (targetPlatform === "darwin") {
    const frameworkVersionsRoot = path.join(
      root,
      "Frameworks",
      "Python.framework",
      "Versions",
    );
    const frameworkHome = path.join(frameworkVersionsRoot, "Current");
    if (fs.existsSync(frameworkHome)) {
      return frameworkHome;
    }
    if (fs.existsSync(frameworkVersionsRoot)) {
      const version = fs.readdirSync(frameworkVersionsRoot).find((entry) => /^\d+\.\d+$/.test(entry));
      if (version) {
        return path.join(frameworkVersionsRoot, version);
      }
    }
  }
  if (!fs.existsSync(path.join(root, "pyvenv.cfg"))) {
    return root;
  }
  return "";
}

function verifyBundledPythonRuntime(root) {
  const pythonCommand = resolveBundledPythonCommand(root);
  if (!pythonCommand) {
    throw new Error(`Bundled Python runtime missing executable under ${root}`);
  }
  const bundledPythonHome = resolveBundledPythonHome(root);
  const env = {
    ...process.env,
    PYTHONUNBUFFERED: "1",
    PYTHONUTF8: "1",
    PYTHONDONTWRITEBYTECODE: "1",
    PYTHONPATH: bundledPythonImportPaths(root).join(path.delimiter),
  };
  if (bundledPythonHome) {
    env.PYTHONHOME = bundledPythonHome;
  } else {
    delete env.PYTHONHOME;
  }
  const probe = spawnSync(
    pythonCommand,
    [
      "-c",
      [
        "import importlib, sys",
        "print(f'python_prefix={sys.prefix} python_exec_prefix={sys.exec_prefix}')",
        "for module_name in ['_socket', 'socket', 'ssl', 'requests', 'urllib3']:",
        "    importlib.import_module(module_name)",
        "print('python_bundle_import_check=ok')",
      ].join("\n"),
    ],
    {
      env,
      encoding: "utf8",
    },
  );
  if (probe.status !== 0) {
    const detail = [probe.stdout, probe.stderr].filter(Boolean).join("\n").trim();
    throw new Error(`Bundled Python runtime import check failed: ${detail || "unknown error"}`);
  }
  return {
    pythonCommand,
    pythonHome: bundledPythonHome,
    sitePackages: bundledPythonSitePackages(root),
    importPaths: bundledPythonImportPaths(root),
    importCheck: probe.stdout.trim() || "python_bundle_import_check=ok",
  };
}

const rustApiBinary = resolveRustApiBinary();
const renderRsBinary = resolveRenderRsBinary();
if (desktopPackage.version !== releaseVersion) {
  desktopPackage.version = releaseVersion;
  fs.writeFileSync(`${desktopPackagePath}.tmp`, `${JSON.stringify(desktopPackage, null, 2)}\n`, "utf8");
  fs.renameSync(`${desktopPackagePath}.tmp`, desktopPackagePath);
}

fs.mkdirSync(buildRoot, { recursive: true });
fs.rmSync(linuxIconsRoot, { recursive: true, force: true });
fs.mkdirSync(linuxIconsRoot, { recursive: true });
if (fs.existsSync(desktopIconSource)) {
  for (const size of [16, 24, 32, 48, 64, 96, 128, 256, 512]) {
    fs.cpSync(desktopIconSource, path.join(linuxIconsRoot, `${size}x${size}.png`), { force: true });
  }
}

if (frontendOnly) {
  fs.rmSync(outputFrontendRoot, { recursive: true, force: true });
  fs.mkdirSync(appRoot, { recursive: true });
  fs.mkdirSync(outputFrontendRoot, { recursive: true });
  fs.mkdirSync(outputFrontendVendorRoot, { recursive: true });
} else {
  fs.rmSync(appRoot, { recursive: true, force: true });
  fs.mkdirSync(outputFrontendRoot, { recursive: true });
  fs.mkdirSync(outputFrontendVendorRoot, { recursive: true });
  fs.mkdirSync(outputBackendRoot, { recursive: true });
  fs.mkdirSync(bundledFontsRoot, { recursive: true });
}

const excludedFrontendEntries = new Set([
  "node_modules",
  "runtime-config.local.js",
  ".codex",
  ".ipynb_checkpoints",
]);

function shouldExcludeFrontendPath(sourcePath) {
  const relativePath = path.relative(frontendRoot, sourcePath);
  if (!relativePath || relativePath.startsWith("..")) {
    return false;
  }
  const parts = relativePath.split(path.sep).filter(Boolean);
  return parts.some((part) => excludedFrontendEntries.has(part));
}

for (const entry of fs.readdirSync(frontendRoot, { withFileTypes: true })) {
  const from = path.join(frontendRoot, entry.name);
  const to = path.join(outputFrontendRoot, entry.name);
  fs.cpSync(from, to, {
    recursive: true,
    force: true,
    filter: (sourcePath) => !shouldExcludeFrontendPath(sourcePath),
  });
}

function copyFrontendRuntimeDependency(packageName, entries, targetDirName = packageName) {
  const candidateRoots = [
    path.join(frontendRoot, "node_modules", packageName),
    path.join(outputFrontendRoot, "node_modules", packageName),
    path.join(desktopRoot, "node_modules", packageName),
  ];
  const packageRoot = candidateRoots.find((candidate) => fs.existsSync(candidate));
  if (!packageRoot) {
    throw new Error(
      `Missing frontend runtime dependency: ${candidateRoots.join(" | ")}`,
    );
  }
  const targetRoot = path.join(outputFrontendVendorRoot, targetDirName);
  for (const entry of entries) {
    const from = path.join(packageRoot, entry);
    if (!fs.existsSync(from)) {
      throw new Error(`Missing frontend runtime dependency asset: ${from}`);
    }
    fs.cpSync(from, path.join(targetRoot, entry), { recursive: true, force: true });
  }
}

copyFrontendRuntimeDependency("pdf-lib", [
  "dist/pdf-lib.esm.js",
]);

copyFrontendRuntimeDependency("pdfjs-dist", [
  "build/pdf.mjs",
  "build/pdf.worker.mjs",
  "cmaps",
  "standard_fonts",
  "web/images",
  "web/pdf_viewer.css",
  "web/pdf_viewer.mjs",
]);

function rewriteDesktopFrontendRuntimeImports() {
  for (const entry of fs.readdirSync(outputFrontendRoot, { withFileTypes: true })) {
    if (!entry.isFile() || !entry.name.endsWith(".html")) {
      continue;
    }
    const htmlPath = path.join(outputFrontendRoot, entry.name);
    let html = fs.readFileSync(htmlPath, "utf8");
    html = html.replace('\n    <script src="./runtime-config.local.js"></script>', "");
    if (entry.name === "reader.html") {
      html = html.replaceAll(
        "./node_modules/pdfjs-dist/web/pdf_viewer.css",
        "./vendor/pdfjs-dist/web/pdf_viewer.css",
      );
    }
    fs.writeFileSync(htmlPath, html, "utf8");
  }

  const readerJsPath = path.join(outputFrontendRoot, "src", "js", "reader.js");
  if (fs.existsSync(readerJsPath)) {
    let readerJs = fs.readFileSync(readerJsPath, "utf8");
    readerJs = readerJs.replaceAll(
      "../../node_modules/pdfjs-dist/",
      "../../vendor/pdfjs-dist/",
    );
    fs.writeFileSync(readerJsPath, readerJs, "utf8");
  }

  const readerDialogControllerPath = path.join(
    outputFrontendRoot,
    "src",
    "js",
    "features",
    "reader-dialog",
    "controller.js",
  );
  if (fs.existsSync(readerDialogControllerPath)) {
    let controllerJs = fs.readFileSync(readerDialogControllerPath, "utf8");
    controllerJs = controllerJs.replaceAll(
      "../../../../node_modules/pdf-lib/",
      "../../../../vendor/pdf-lib/",
    );
    fs.writeFileSync(readerDialogControllerPath, controllerJs, "utf8");
  }
}

rewriteDesktopFrontendRuntimeImports();

const desktopPartialsRoot = path.join(outputFrontendRoot, "src", "partials");
const desktopTemplatesPath = path.join(outputFrontendRoot, "src", "js", "templates.js");
const desktopMainContent = fs.readFileSync(
  path.join(desktopPartialsRoot, "main-content.html"),
  "utf8",
);
const desktopDialogs = fs.readFileSync(
  path.join(desktopPartialsRoot, "dialogs.html"),
  "utf8",
);
const desktopTemplatesSource = `const MAIN_CONTENT_HTML = ${JSON.stringify(desktopMainContent)};
const DIALOGS_HTML = ${JSON.stringify(desktopDialogs)};

export async function renderPageShell() {
  document.body.innerHTML = MAIN_CONTENT_HTML + DIALOGS_HTML;
}
`;

fs.writeFileSync(desktopTemplatesPath, desktopTemplatesSource, "utf8");

const desktopConstantsPath = path.join(outputFrontendRoot, "src", "js", "constants.js");
if (fs.existsSync(desktopConstantsPath)) {
  let desktopConstants = fs.readFileSync(desktopConstantsPath, "utf8");
  desktopConstants = desktopConstants.replace(
    /export const DEFAULT_WORKERS = \d+;/,
    "export const DEFAULT_WORKERS = 100;",
  );
  fs.writeFileSync(desktopConstantsPath, desktopConstants, "utf8");
}

const desktopRuntimeConfig = `window.__FRONT_RUNTIME_CONFIG__ = {
  apiBase: "http://127.0.0.1:41000",
  xApiKey: "retain-pdf-desktop",
  ocrProvider: "mineru",
  mineruToken: "",
  modelApiKey: "",
  model: "deepseek-v4-flash",
  baseUrl: "https://api.deepseek.com/v1",
};
`;

fs.writeFileSync(
  path.join(outputFrontendRoot, "runtime-config.js"),
  desktopRuntimeConfig,
  "utf8",
);

const desktopIndexPath = path.join(outputFrontendRoot, "index.html");
let desktopIndexHtml = fs.readFileSync(desktopIndexPath, "utf8");
desktopIndexHtml = desktopIndexHtml.replace('\n    <script src="./runtime-config.local.js"></script>', "");
fs.writeFileSync(desktopIndexPath, desktopIndexHtml, "utf8");

const DEAD_ENTRYPOINTS = new Set([
  "run_extract_text_layer.py", // native extract-text-layer: no python worker
  "run_render_only.py", // python render retired: render_rs native
  "run_document_flow.py", // legacy book flow, deleted
  "run_book.py", // legacy: from_ocr_pipeline, deleted
  "translate_book.py", // legacy wrapper
  "translate_page.py", // legacy: services.rendering.legacy.*, deleted
  "build_book.py", // legacy: book_pipeline, deleted
  "build_page.py", // legacy: services.rendering.legacy.*, deleted
  "run_translate_from_ocr.py", // legacy: from_ocr_pipeline, deleted
  "diagnose_failure_with_ai.py", // dev diagnostic, never spawned by rust_api
]);

if (!frontendOnly) {
  fs.cpSync(path.join(backendRoot, "scripts"), path.join(outputBackendRoot, "scripts"), {
    recursive: true,
    force: true,
    filter: (sourcePath) => {
      const basename = path.basename(sourcePath);
      if (basename === "__pycache__" || basename.endsWith(".pyc")) {
        return false;
      }
      const relParts = path.relative(path.join(backendRoot, "scripts"), sourcePath).split(path.sep);
      // services/rendering is native-only in the desktop (render_rs); the Python
      // tree no longer has any live caller, so exclude it from the bundle.
      if (relParts[0] === "services" && relParts[1] === "rendering") {
        return false;
      }
      // Dead book-flow entrypoints would ImportError on services.rendering;
      // rust_api never spawns them in the desktop.
      if (relParts[0] === "entrypoints" && DEAD_ENTRYPOINTS.has(basename)) {
        return false;
      }
      return true;
    },
  });
}

if (!frontendOnly && fs.existsSync(rustApiBinary.path)) {
  fs.mkdirSync(path.join(outputBackendRoot, "bin"), { recursive: true });
  fs.cpSync(rustApiBinary.path, path.join(outputBackendRoot, "bin", rustApiBinary.fileName), {
    force: true,
  });
}

if (!frontendOnly && fs.existsSync(renderRsBinary.path)) {
  fs.mkdirSync(path.join(outputBackendRoot, "bin"), { recursive: true });
  fs.cpSync(renderRsBinary.path, path.join(outputBackendRoot, "bin", renderRsBinary.fileName), {
    force: true,
  });
}

if (!frontendOnly && targetPlatform === "win32" && fs.existsSync(path.join(embeddedPythonRoot, "python.exe"))) {
  copyRuntimeTree(embeddedPythonRoot, path.join(outputBackendRoot, "python"));
}

if (!frontendOnly && targetPlatform === "linux" && hasBundledPosixPython(embeddedPythonRoot)) {
  copyRuntimeTree(embeddedPythonRoot, path.join(outputBackendRoot, "python"));
}

if (!frontendOnly && targetPlatform === "darwin" && hasBundledPosixPython(embeddedPythonRoot)) {
  if (allowBundledMacPython) {
    const targetPythonRoot = path.join(outputBackendRoot, "python");
    copyRuntimeTree(embeddedPythonRoot, targetPythonRoot);
    rewriteAbsoluteSymlinksWithinRoot(targetPythonRoot, embeddedPythonRoot);
    pruneBundledMacPythonRuntime(targetPythonRoot);
  } else {
    console.warn(
      "[prepare-app] skip bundling backend/python for darwin because RETAIN_PDF_BUNDLE_MAC_PYTHON!=1",
    );
  }
}

const outputPythonRoot = path.join(outputBackendRoot, "python");
const pythonBundled = fs.existsSync(path.join(outputPythonRoot, "python.exe"))
  || fs.existsSync(path.join(outputPythonRoot, "bin", "python3"))
  || fs.existsSync(path.join(outputPythonRoot, "bin", "python"));
const bundledPythonRequired = targetPlatform === "win32"
  || targetPlatform === "linux"
  || (targetPlatform === "darwin" && allowBundledMacPython);
let bundledPythonDiagnostics = null;
if (!frontendOnly && targetPlatform === "darwin" && allowBundledMacPython && !hasBundledPosixPython(embeddedPythonRoot)) {
  throw new Error(
    `Bundled macOS Python runtime is missing. Expected ${path.join(resolvePlatformRuntimeDir("mac"), "python")} to contain bin/python3.`,
  );
}
if (!frontendOnly && bundledPythonRequired && !pythonBundled) {
  throw new Error(`Bundled Python runtime is required for ${targetPlatform} packaging but was not copied to ${outputPythonRoot}`);
}
if (!frontendOnly && pythonBundled && !skipBundledRuntimeVerification) {
  bundledPythonDiagnostics = verifyBundledPythonRuntime(outputPythonRoot);
}

if (!frontendOnly && fs.existsSync(bundledTypstRoot)) {
  fs.cpSync(bundledTypstRoot, path.join(outputBackendRoot, "typst"), {
    recursive: true,
    force: true,
  });
}

if (!frontendOnly && fs.existsSync(typstPackagesRoot)) {
  fs.cpSync(typstPackagesRoot, path.join(outputBackendRoot, "typst-packages"), {
    recursive: true,
    force: true,
  });
}

if (!frontendOnly) {
  for (const fontAssetsRoot of resolveSharedRuntimePaths("fonts")) {
    for (const entry of fs.readdirSync(fontAssetsRoot)) {
      const from = path.join(fontAssetsRoot, entry);
      const to = path.join(bundledFontsRoot, entry);
      if (fs.statSync(from).isFile()) {
        fs.cpSync(from, to, { force: true });
      }
    }
  }
}

const requiredBundledFonts = [
  "DroidSansFallbackFull.ttf",
  "SourceHanSerifSC-Regular.otf",
  "SourceHanSerifSC-Bold.otf",
];
if (!frontendOnly) {
  for (const fileName of requiredBundledFonts) {
    const expectedPath = path.join(bundledFontsRoot, fileName);
    if (!fs.existsSync(expectedPath)) {
      throw new Error(`Missing bundled font asset: ${expectedPath}`);
    }
  }

  const manifest = {
    generatedAt: new Date().toISOString(),
    version: releaseVersion,
    targetPlatform,
    targetPlatformName,
    rustApiBinaryBundled: fs.existsSync(path.join(outputBackendRoot, "bin", rustApiBinary.fileName)),
    rustApiBinaryName: rustApiBinary.fileName,
    renderRsBinaryBundled: fs.existsSync(path.join(outputBackendRoot, "bin", renderRsBinary.fileName)),
    renderRsBinaryName: renderRsBinary.fileName,
    pythonBundled,
    bundledPythonExecutable: bundledPythonDiagnostics ? path.relative(outputBackendRoot, bundledPythonDiagnostics.pythonCommand) : null,
    bundledPythonHome: bundledPythonDiagnostics && bundledPythonDiagnostics.pythonHome
      ? path.relative(outputBackendRoot, bundledPythonDiagnostics.pythonHome)
      : null,
    bundledPythonSitePackages: bundledPythonDiagnostics
      ? bundledPythonDiagnostics.sitePackages.map((entry) => path.relative(outputBackendRoot, entry))
      : [],
    bundledPythonImportPaths: bundledPythonDiagnostics
      ? bundledPythonDiagnostics.importPaths.map((entry) => path.relative(outputBackendRoot, entry))
      : [],
    bundledPythonImportCheck: bundledPythonDiagnostics ? bundledPythonDiagnostics.importCheck : null,
    typstBundled: fs.existsSync(path.join(outputBackendRoot, "typst")),
    typstPackagesBundled: fs.existsSync(path.join(outputBackendRoot, "typst-packages")),
    bundledFonts: fs.readdirSync(bundledFontsRoot).sort(),
    sizesBytes: buildSizesBytes(),
  };

  fs.writeFileSync(
    path.join(outputBackendRoot, "bundle-manifest.json"),
    JSON.stringify(manifest, null, 2),
    "utf8",
  );
}
