import { useMemo, useRef } from "react";

/**
 * A plain-text editor with light syntax highlighting for config files
 * (.toml, .json, .js, .cfg, .properties…): a transparent textarea over a
 * highlighted <pre>. No dependencies, no network.
 */
export function CodeEditor({ value, onChange, path, onSave }: { value: string; onChange: (v: string) => void; path: string; onSave?: () => void }) {
  const pre = useRef<HTMLPreElement>(null);
  const lang = langOf(path);
  const html = useMemo(() => highlight(value, lang) + "\n", [value, lang]);
  return (
    <div className="code-ed">
      <pre ref={pre} className="code-ed__hl" aria-hidden dangerouslySetInnerHTML={{ __html: html }} />
      <textarea className="code-ed__input" value={value} spellCheck={false} aria-label={`Contents of ${path}`}
        onChange={(e) => onChange(e.target.value)}
        onScroll={(e) => { if (pre.current) { pre.current.scrollTop = e.currentTarget.scrollTop; pre.current.scrollLeft = e.currentTarget.scrollLeft; } }}
        onKeyDown={(e) => {
          if ((e.metaKey || e.ctrlKey) && e.key === "s") { e.preventDefault(); onSave?.(); return; }
          if (e.key === "Tab" && !e.shiftKey) {
            // Tab indents (two spaces) instead of leaving the editor; Esc then Tab moves focus on.
            e.preventDefault();
            const t = e.currentTarget, a = t.selectionStart, b = t.selectionEnd;
            onChange(value.slice(0, a) + "  " + value.slice(b));
            requestAnimationFrame(() => { t.selectionStart = t.selectionEnd = a + 2; });
          }
          if (e.key === "Escape") e.currentTarget.blur();
        }} />
    </div>
  );
}

type Lang = "toml" | "json" | "js" | "cfg" | "plain";
function langOf(path: string): Lang {
  const ext = path.toLowerCase().split(".").pop() ?? "";
  if (ext === "toml") return "toml";
  if (["json", "json5", "jsonc", "mcmeta"].includes(ext)) return "json";
  if (["js", "ts", "zs"].includes(ext)) return "js";
  if (["cfg", "conf", "properties", "ini", "txt", "yml", "yaml", "snbt"].includes(ext)) return "cfg";
  return "plain";
}

const esc = (s: string) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

/** One pass over the text with a sticky regex per language; unknown text stays plain. */
function highlight(src: string, lang: Lang): string {
  if (lang === "plain" || src.length > 200_000) return esc(src);
  const comment = lang === "js" || lang === "json" ? String.raw`\/\/[^\n]*|\/\*[\s\S]*?\*\/` : String.raw`#[^\n]*|^[ \t]*;[^\n]*`;
  const parts = [
    `(?<c>${comment})`,
    String.raw`(?<s>"(?:[^"\\\n]|\\.)*"?|'(?:[^'\\\n]|\\.)*'?|\x60[^\x60]*\x60?)`,
    String.raw`(?<h>^[ \t]*\[[^\]\n]*\])`,
    String.raw`(?<k>^[ \t]*[A-Za-z0-9_.\-]+(?=[ \t]*[=:]))`,
    String.raw`(?<n>\b-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b)`,
    String.raw`(?<b>\b(?:true|false|null|undefined)\b)`,
    lang === "js" ? String.raw`(?<w>\b(?:const|let|var|function|return|if|else|for|while|new|import|from|export|=>|onEvent|ServerEvents|StartupEvents|ClientEvents)\b)` : "(?<w>(?!))",
  ];
  const re = new RegExp(parts.join("|"), "gm");
  let out = "", last = 0;
  for (const m of src.matchAll(re)) {
    const i = m.index ?? 0;
    if (m[0].length === 0) continue;
    out += esc(src.slice(last, i));
    const g = m.groups ?? {};
    const cls = g.c ? "c" : g.s ? "s" : g.h ? "h" : g.k ? "k" : g.n ? "n" : g.b ? "b" : "w";
    // Lang-specific: in JSON a string followed by ":" is a key.
    const key = lang === "json" && cls === "s" && /^\s*:/.test(src.slice(i + m[0].length, i + m[0].length + 4));
    out += `<span class="hl-${key ? "k" : cls}">${esc(m[0])}</span>`;
    last = i + m[0].length;
  }
  return out + esc(src.slice(last));
}
