import { resolveMarkedVendorUrl } from "../../../js/runtime/vendor-url.js";

// AI 回答专用的 Markdown 安全渲染:marked 惰性加载,且**转义原始 HTML**——
// 模型输出的 `**加粗**`/`## 标题`/列表会渲染,但 `<img>`/`<script>` 一律显示为
// 字面文本,绝不进入 DOM(防注入)。引用按钮注入与本模块解耦(见 chat.js)。

let markedPromise = null;

function escapeHtml(value = "") {
  return `${value}`
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

// marked 各版本 renderer.html 的入参不一(字符串或 token),统一取原文再转义
function rawHtmlText(input) {
  if (typeof input === "string") {
    return input;
  }
  return `${input?.raw ?? input?.text ?? ""}`;
}

function loadMarked() {
  if (!markedPromise) {
    markedPromise = import(resolveMarkedVendorUrl())
      .then(({ marked, Marked }) => {
        // 用独立实例,避免污染 markdown-preview 的全局 marked 配置
        const instance = typeof Marked === "function" ? new Marked() : marked;
        instance.use({
          renderer: {
            html: (input) => escapeHtml(rawHtmlText(input)),
          },
        });
        return instance;
      })
      .catch((error) => {
        markedPromise = null;
        throw error;
      });
  }
  return markedPromise;
}

// Markdown 文本 → 清洗后的 DocumentFragment。marked 不可用(如 node 测试)时抛错,
// 调用方负责回退为纯文本节点。
export async function renderAiMarkdownFragment(text, { documentRef = globalThis.document } = {}) {
  const marked = await loadMarked();
  const template = documentRef.createElement("template");
  template.innerHTML = marked.parse(`${text || ""}`, { async: false });
  // 双保险:即便 renderer.html 漏网,也移除脚本类节点与内联事件/危险链接
  const content = template.content;
  content.querySelectorAll("script, iframe, object, embed, img, svg").forEach((node) => node.remove());
  content.querySelectorAll("*").forEach((node) => {
    for (const attribute of [...node.attributes]) {
      if (/^on/i.test(attribute.name)) {
        node.removeAttribute(attribute.name);
      }
    }
  });
  content.querySelectorAll("a[href]").forEach((anchor) => {
    if (/^\s*javascript:/i.test(anchor.getAttribute("href") || "")) {
      anchor.removeAttribute("href");
    }
    anchor.setAttribute("target", "_blank");
    anchor.setAttribute("rel", "noopener noreferrer");
  });
  return content;
}
