import { $ } from "../../../shared/dom/query.js";
import { resolveMarkedVendorUrl } from "../../../js/runtime/vendor-url.js";
import { parseMarkdownWithMath } from "./markdown-math.js";

let markedModulePromise = null;

function loadMarked() {
  if (!markedModulePromise) {
    markedModulePromise = import(resolveMarkedVendorUrl())
      .catch((error) => {
        markedModulePromise = null;
        throw error;
      });
  }
  return markedModulePromise;
}

// 渲染产物只来自本站管线,这里仍做一层基础清洗:
// 去掉脚本节点、内联事件与 javascript: 链接
function sanitizeRenderedMarkdown(container) {
  container.querySelectorAll("script, iframe, object, embed").forEach((node) => node.remove());
  container.querySelectorAll("*").forEach((node) => {
    for (const attribute of [...node.attributes]) {
      if (/^on/i.test(attribute.name)) {
        node.removeAttribute(attribute.name);
      }
    }
  });
  container.querySelectorAll("a[href]").forEach((anchor) => {
    if (/^\s*javascript:/i.test(anchor.getAttribute("href") || "")) {
      anchor.removeAttribute("href");
    }
    anchor.setAttribute("target", "_blank");
    anchor.setAttribute("rel", "noopener noreferrer");
  });
}

export function createReaderMarkdownPreview({
  jobId = "",
  loadMarkdownPayload = null,
  fetchProtected = null,
} = {}) {
  let loadPromise = null;
  const objectUrls = [];

  function statusEl() {
    return $("reader-markdown-status");
  }

  function contentEl() {
    return $("reader-markdown-content");
  }

  function setStatus(text) {
    const el = statusEl();
    if (el) {
      el.textContent = text || "";
      el.classList.toggle("hidden", !text);
    }
  }

  // 后端图片需要 X-API-Key,<img> 发不了请求头,换成 blob URL。
  // src 已在挂载前改存到 data-reader-md-src,避免浏览器先发一次裸请求
  async function hydrateImages(container) {
    const images = [...container.querySelectorAll("img[data-reader-md-src]")];
    await Promise.allSettled(images.map(async (img) => {
      const src = img.getAttribute("data-reader-md-src") || "";
      try {
        const response = await fetchProtected?.(src);
        if (!response?.ok) {
          throw new Error(`图片加载失败(${response?.status ?? "网络错误"})`);
        }
        const objectUrl = URL.createObjectURL(await response.blob());
        objectUrls.push(objectUrl);
        img.src = objectUrl;
      } catch (_err) {
        const fallback = img.ownerDocument.createElement("span");
        fallback.className = "reader-markdown-image-missing";
        fallback.textContent = `[图片暂不可用] ${img.getAttribute("alt") || src}`;
        fallback.title = src;
        img.replaceWith(fallback);
      }
    }));
  }

  async function load() {
    setStatus("正在加载 Markdown…");
    const payload = await loadMarkdownPayload?.(jobId);
    const content = `${payload?.content_with_absolute_image_urls || payload?.content || ""}`;
    const imagesBaseUrl = `${payload?.images_base_url || payload?.images_base_path || ""}`.trim();
    if (!content.trim()) {
      setStatus("该任务暂无 Markdown 产物");
      return false;
    }
    const { marked } = await loadMarked();
    const container = contentEl();
    if (!container) {
      return false;
    }
    // 先保护 $公式$ 再 marked，再 MathJax→SVG；图片在 template 内挂载前去掉 src
    const html = await parseMarkdownWithMath(content, (src) =>
      String(marked.parse(src, { async: false })),
    );
    const template = container.ownerDocument.createElement("template");
    template.innerHTML = html;
    sanitizeRenderedMarkdown(template.content);
    // 动态 import 避免 circular；resolveMarkdownAssetUrl 剥 images/ 双前缀
    const { resolveMarkdownAssetUrl } = await import("../../../js/job/artifacts.js");
    template.content.querySelectorAll("img[src]").forEach((img) => {
      const raw = img.getAttribute("src") || "";
      const resolved = resolveMarkdownAssetUrl(imagesBaseUrl, raw) || raw;
      img.setAttribute("data-reader-md-src", resolved);
      img.removeAttribute("src");
    });
    container.replaceChildren(template.content);
    container.classList.remove("hidden");
    setStatus("");
    await hydrateImages(container);
    return true;
  }

  function ensureLoaded() {
    if (!loadPromise) {
      loadPromise = load().catch((error) => {
        loadPromise = null;
        setStatus(error?.message || "Markdown 加载失败，重开抽屉可重试");
        return false;
      });
    }
    return loadPromise;
  }

  function destroy() {
    objectUrls.forEach((url) => URL.revokeObjectURL(url));
    objectUrls.length = 0;
  }

  return {
    destroy,
    ensureLoaded,
  };
}
