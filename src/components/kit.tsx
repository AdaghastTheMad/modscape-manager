// Minecraft-dark component kit: Button, Panel, Slot, Tooltip, Input, Tabs (+ Badge, Notice, Modal, ModIcon).
import { uiClick } from "../sound";
import type { AppError } from "../api";
import { useIcon } from "../hooks";
import { useId, useRef, useState, type ButtonHTMLAttributes, type ComponentProps, type KeyboardEvent, type ReactNode, type SelectHTMLAttributes } from "react";

const cx = (...c: (string | false | null | undefined)[]) => c.filter(Boolean).join(" ");

// ---------- Button ----------
export function Button({ variant = "default", size = "normal", className, ...rest }:
  ButtonHTMLAttributes<HTMLButtonElement> & { variant?: "default" | "primary" | "danger"; size?: "normal" | "small" }) {
  return <button type="button" className={cx("mc-btn", variant !== "default" && `mc-btn--${variant}`, size === "small" && "mc-btn--small", className)} {...rest} />;
}

// ---------- Panel ----------
export function Panel({ title, actions, inset, className, children }:
  { title?: ReactNode; actions?: ReactNode; inset?: boolean; className?: string; children?: ReactNode }) {
  return (
    <section className={cx("mc-panel", inset && "mc-panel--inset", className)}>
      {(title || actions) && (
        <header className="mc-panel__title">
          {title && <h2>{title}</h2>}
          {actions && <div className="row">{actions}</div>}
        </header>
      )}
      {children}
    </section>
  );
}

// ---------- Slot ----------
export function Slot({ src, label, size = 48, selected, error }:
  { src?: string | null; label: string; size?: number; selected?: boolean; error?: boolean }) {
  const [broken, setBroken] = useState(false);
  const glyph = label.replace(/[^A-Za-z0-9]/g, "").slice(0, 2).toUpperCase() || "?";
  return (
    <div className={cx("mc-slot", selected && "mc-slot--selected", error && "mc-slot--error")} style={{ ["--size" as string]: `${size}px` }}
      role="img" aria-label={label}>
      {src && !broken ? <img src={src} alt="" draggable={false} onError={() => setBroken(true)} /> : <span className="mc-slot__glyph" aria-hidden>{glyph}</span>}
    </div>
  );
}

// ---------- Tooltip ----------
export function Tooltip({ name, lines = [], badge, placement = "right", children }:
  { name: ReactNode; lines?: ReactNode[]; badge?: ReactNode; placement?: "right" | "below"; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <span className="mc-tooltip-anchor" onMouseEnter={() => setOpen(true)} onMouseLeave={() => setOpen(false)}
      onFocus={() => setOpen(true)} onBlur={() => setOpen(false)} aria-describedby={open ? id : undefined}>
      {children}
      {open && (
        <span role="tooltip" id={id} className={cx("mc-tooltip", placement === "below" && "mc-tooltip--below")}>
          <span className="mc-tooltip__name">{name}</span>
          {lines.map((l, i) => <span key={i} className="mc-tooltip__line" style={{ display: "block" }}>{l}</span>)}
          {badge && <span style={{ display: "block", marginTop: 4 }}>{badge}</span>}
        </span>
      )}
    </span>
  );
}

export function SourceBadge({ source }: { source: "modrinth" | "curseforge" }) {
  return <span className={`mc-badge mc-badge--${source}`}>{source === "modrinth" ? "Modrinth" : "CurseForge"}</span>;
}

// ---------- Input ----------
export function Input({ label, hint, className, ...rest }: ComponentProps<"input"> & { label?: string; hint?: ReactNode }) {
  const id = useId();
  const input = <input id={id} className={cx("mc-input", className)} {...rest} />;
  if (!label) return input;
  return (
    <div className="mc-field">
      <label className="mc-field__label" htmlFor={id}>{label}</label>
      {input}
      {hint && <span className="mc-field__hint">{hint}</span>}
    </div>
  );
}

export function Select({ label, children, ...rest }: SelectHTMLAttributes<HTMLSelectElement> & { label?: string }) {
  const id = useId();
  const sel = <select id={id} className="mc-select" {...rest}>{children}</select>;
  if (!label) return sel;
  return <div className="mc-field"><label className="mc-field__label" htmlFor={id}>{label}</label>{sel}</div>;
}

// ---------- Tabs (WAI-ARIA tabs: arrow keys move between tabs) ----------
export function Tabs<T extends string>({ tabs, value, onChange, label, underline }:
  { tabs: { id: T; label: ReactNode; disabled?: boolean }[]; value: T; onChange: (v: T) => void; label: string; underline?: boolean }) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const enabled = tabs.map((t, i) => (t.disabled ? -1 : i)).filter((i) => i >= 0);
  const onKey = (e: KeyboardEvent, i: number) => {
    const pos = enabled.indexOf(i);
    let next: number | undefined;
    if (e.key === "ArrowRight") next = enabled[(pos + 1) % enabled.length];
    if (e.key === "ArrowLeft") next = enabled[(pos - 1 + enabled.length) % enabled.length];
    if (e.key === "Home") next = enabled[0];
    if (e.key === "End") next = enabled[enabled.length - 1];
    if (next !== undefined) { e.preventDefault(); if (tabs[next].id !== value) uiClick(); refs.current[next]?.focus(); onChange(tabs[next].id); }
  };
  return (
    <div role="tablist" aria-label={label} className={cx("mc-tabs", underline && "mc-tabs--under")}>
      {tabs.map((t, i) => (
        <button key={t.id} ref={(el) => { refs.current[i] = el; }} role="tab" type="button" className="mc-tab"
          aria-selected={t.id === value} tabIndex={t.id === value ? 0 : -1} disabled={t.disabled}
          onClick={() => onChange(t.id)} onKeyDown={(e) => onKey(e, i)}>
          {t.label}
        </button>
      ))}
    </div>
  );
}

// ---------- Notice ----------
export function Notice({ tone = "warn", title, children }: { tone?: "warn" | "error" | "ok"; title?: ReactNode; children?: ReactNode }) {
  return (
    <div className={cx("mc-notice", tone !== "warn" && `mc-notice--${tone}`)} role={tone === "error" ? "alert" : "status"}>
      {title && <strong className="pixel" style={{ display: "block" }}>{title}</strong>}
      {children}
    </div>
  );
}

// ---------- ModIcon: Slot whose image comes from the core's icon cache ----------
export function ModIcon({ url, ...rest }: { url?: string | null; label: string; size?: number; selected?: boolean; error?: boolean }) {
  const src = useIcon(url);
  return <Slot src={src} {...rest} />;
}

// ---------- Modal ----------
export function Modal({ title, onClose, children, width = 640 }: { title: ReactNode; onClose: () => void; children: ReactNode; width?: number }) {
  return (
    <div className="mc-modal-backdrop" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}
      onKeyDown={(e) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } }}>
      <div className="mc-modal" role="dialog" aria-modal="true" style={{ width }}>
        <Panel title={title} actions={<Button size="small" onClick={onClose} aria-label="Close">✕</Button>}>{children}</Panel>
      </div>
    </div>
  );
}

// ---------- ErrorNotice ----------
export function ErrorNotice({ error }: { error: AppError }) {
  return <Notice tone="error" title={error.message}>{error.help && <pre>{error.help}</pre>}</Notice>;
}
