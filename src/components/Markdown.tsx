import DOMPurify from "dompurify";
import { marked, Renderer } from "marked";
import { useMemo } from "react";
import { api } from "../api";

// Mod descriptions are author-written Markdown/HTML. They're sanitised, remote
// images are replaced with a label (the webview has no network access), and
// links open in the system browser instead of navigating the app.
const renderer = new Renderer();
renderer.image = ({ text }) => `<span class="md-img">[image${text ? `: ${escapeHtml(text)}` : ""}]</span>`;

function escapeHtml(s: string) {
  return s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

DOMPurify.addHook("uponSanitizeElement", (node, data) => {
  if (data.tagName === "img" && node instanceof Element) {
    const alt = node.getAttribute("alt");
    const span = node.ownerDocument.createElement("span");
    span.className = "md-img";
    span.textContent = `[image${alt ? `: ${alt}` : ""}]`;
    node.replaceWith(span);
  }
});

export function Markdown({ source }: { source: string }) {
  const html = useMemo(() => {
    const raw = marked.parse(source, { renderer, async: false, gfm: true, breaks: false }) as string;
    return DOMPurify.sanitize(raw, {
      FORBID_TAGS: ["style", "iframe", "video", "audio", "source", "object", "embed", "form", "input", "button", "picture"],
      FORBID_ATTR: ["style", "srcset"],
    });
  }, [source]);
  return (
    <div className="markdown" dangerouslySetInnerHTML={{ __html: html }}
      onClick={(e) => {
        const a = (e.target as HTMLElement).closest("a");
        if (!a) return;
        e.preventDefault();
        const href = a.getAttribute("href") ?? "";
        if (href.startsWith("https://")) api.openExternal(href).catch(() => {});
      }} />
  );
}
