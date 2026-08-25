import fs from "fs";
import path from "path";
import { spawnSync } from "child_process";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const desktopRoot = path.resolve(__dirname, "..");
const repoRoot = path.resolve(desktopRoot, "..");
const versionFile = path.join(repoRoot, "VERSION");
const tauriConfigPath = path.join(desktopRoot, "src-tauri", "tauri.conf.json");
const splashSource = path.join(desktopRoot, "splash.html");
const logoSource = path.join(desktopRoot, "assets", "RetainPDF-logo.png");
const outputFrontendRoot = path.join(desktopRoot, "app", "frontend");
const desktopPackagePath = path.join(desktopRoot, "package.json");
const desktopPackage = JSON.parse(fs.readFileSync(desktopPackagePath, "utf8"));

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

if (!fs.existsSync(outputFrontendRoot)) {
  throw new Error(
    `frontend root missing at ${outputFrontendRoot}; run "npm run prepare-app" before "npm run prepare-tauri"`,
  );
}

const tauriConfig = JSON.parse(fs.readFileSync(tauriConfigPath, "utf8"));
if (tauriConfig.version !== releaseVersion) {
  tauriConfig.version = releaseVersion;
  fs.writeFileSync(`${tauriConfigPath}.tmp`, `${JSON.stringify(tauriConfig, null, 2)}\n`, "utf8");
  fs.renameSync(`${tauriConfigPath}.tmp`, tauriConfigPath);
}

fs.copyFileSync(splashSource, path.join(outputFrontendRoot, "splash.html"));
const outputAssetsRoot = path.join(outputFrontendRoot, "assets");
fs.mkdirSync(outputAssetsRoot, { recursive: true });
fs.copyFileSync(logoSource, path.join(outputAssetsRoot, "RetainPDF-logo.png"));

if (process.platform === "darwin") {
  const iconsetPath = path.join(desktopRoot, "build", "icon.iconset");
  const icnsPath = path.join(desktopRoot, "build", "icon.icns");
  if (!fs.existsSync(icnsPath) && fs.existsSync(iconsetPath)) {
    const result = spawnSync("iconutil", ["-c", "icns", iconsetPath, "-o", icnsPath], {
      encoding: "utf8",
    });
    if (result.status !== 0) {
      console.warn(`[prepare-tauri] failed to generate icon.icns: ${(result.stderr || "").trim()}`);
    }
  }
}

console.log(`[prepare-tauri] version=${releaseVersion} splash=${path.join("app", "frontend", "splash.html")}`);
