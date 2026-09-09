// 概念页正文 [[实体名]] → 可点击链接（点一下切到那个实体的面板）。
// 纯 DOM，与 injectCitationMarkers 同一套 text node 遍历；解析不到的还原成纯文本。

import type { EntityPageLink } from "./api.js";

const WIKILINK_RE = /\[\[([^\[\]\n]{1,80})\]\]/g;
const SKIP_SELECTOR = "code, pre, .reader-ai-citation-ref, button, a, .aui-msg-actions";

/** 把容器文本里的 [[X]] 换成按钮（X 命中 linkBySurface 时）或纯文本 X。 */
export function injectWikiLinks(
  container: ParentNode,
  linkBySurface: Map<string, EntityPageLink>,
  onOpen: ((link: EntityPageLink) => void) | null,
  documentRef: Document = globalThis.document,
): void {
  if (!container) return;
  // 正文没有 [[ 就直接跳过；注意不能按 linkBySurface.size 提前返回——
  // 一个都没解析到时仍要把 [[X]] 还原成纯文本，否则括号漏到界面上。
  if (!`${(container as Node).textContent || ""}`.includes("[[")) return;
  // 0x4 = SHOW_TEXT；避免依赖 NodeFilter 全局（jsdom/部分环境未挂）
  const walker = documentRef.createTreeWalker?.(container as Node, 0x4) || null;
  if (!walker) return;
  const textNodes: Text[] = [];
  let node = walker.nextNode();
  while (node) {
    if (!node.parentElement?.closest?.(SKIP_SELECTOR)) {
      textNodes.push(node as Text);
    }
    node = walker.nextNode();
  }
  for (const textNode of textNodes) {
    const text = `${textNode.textContent || ""}`;
    if (!text.includes("[[")) continue;
    WIKILINK_RE.lastIndex = 0;
    const fragment = documentRef.createDocumentFragment();
    let cursor = 0;
    let match: RegExpExecArray | null;
    while ((match = WIKILINK_RE.exec(text))) {
      fragment.appendChild(documentRef.createTextNode(text.slice(cursor, match.index)));
      cursor = match.index + match[0].length;
      const surface = match[1].trim();
      const link = linkBySurface.get(surface);
      if (!link) {
        fragment.appendChild(documentRef.createTextNode(surface));
        continue;
      }
      const button = documentRef.createElement("button");
      button.type = "button";
      button.className = "reader-entities-wikilink";
      button.textContent = surface;
      button.addEventListener("click", (event) => {
        event.preventDefault();
        event.stopPropagation();
        onOpen?.(link);
      });
      fragment.appendChild(button);
    }
    if (cursor === 0) continue;
    fragment.appendChild(documentRef.createTextNode(text.slice(cursor)));
    textNode.replaceWith(fragment);
  }
}
