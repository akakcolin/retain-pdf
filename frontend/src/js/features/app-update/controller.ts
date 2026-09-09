import {
  fetchLatestGithubRelease,
  normalizeReleaseInfo,
} from "./github-release.js";
import {
  defaultUpdateCachePort,
} from "./state.js";

export function mountAppUpdateFeature({
  enabled = true,
  cachePort = defaultUpdateCachePort,
  fetchLatestRelease = fetchLatestGithubRelease,
  normalizeRelease = normalizeReleaseInfo,
  viewPort,
}: any = {}) {
  function applyUpdateInfo(info) {
    if (!info) {
      viewPort.setReady();
      return;
    }
    if (info.hasUpdate) {
      viewPort.setAvailable(info);
    } else {
      viewPort.setLatest(info);
    }
  }

  let inFlight = false;

  async function checkForUpdates({ manual = false }: any = {}) {
    if (!enabled) {
      return false;
    }
    // 挂载后 1200ms 的自动检查与用户点"重新检查"可能重叠,后到者会覆盖
    // 先前结果;同一时刻只允许一次。
    if (inFlight) {
      return false;
    }
    inFlight = true;
    if (manual) {
      viewPort.setChecking();
    }
    try {
      const release = await fetchLatestRelease();
      const info = normalizeRelease(release);
      cachePort.write(info);
      applyUpdateInfo(info);
    } catch (error) {
      if (manual) {
        viewPort.setError(error);
      }
    } finally {
      inFlight = false;
    }
    return true;
  }

  viewPort.bindButton({
    onCheck: () => {
      void checkForUpdates({ manual: true });
    },
  });

  if (!enabled) {
    viewPort.setReady();
    return {
      checkForUpdates,
    };
  }

  const cached = cachePort.read();
  applyUpdateInfo(cached.info);
  if (cached.fresh) {
    return {
      checkForUpdates,
    };
  }

  window.setTimeout(() => {
    void checkForUpdates({ manual: false });
  }, 1200);

  return {
    checkForUpdates,
  };
}
