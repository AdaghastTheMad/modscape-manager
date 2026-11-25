#!/usr/bin/env node
// Regenerates the README screenshots in docs/images/ (dev tool, never shipped).
//
// Needs the dev bridge and Vite running:
//   (cd src-tauri && cargo run --example dev_bridge) & pnpm exec vite --port 1420
// and a Chromium-based browser (Brave or Chrome). It runs headless with a
// throwaway profile, seeds demo data through the bridge (fixture packs, the
// starter payloads, a sample template from test-env/), opens each screen,
// draws numbered callouts on the real elements and saves a PNG.
//
//   node scripts/capture-screens.mjs [only-this-scene]
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { fileURLToPath } from "node:url";
const ROOT = fileURLToPath(new URL("..", import.meta.url));
const OUT = join(ROOT, "docs/images");
const BRIDGE = "http://127.0.0.1:1421/invoke";
const PAGE = "http://127.0.0.1:1420/";
const BROWSERS = [
  "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
  "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
  "/usr/bin/chromium", "/usr/bin/google-chrome",
];
const W = 1440, H = 900;
const only = process.argv[2];

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function inv(cmd, args = {}) {
  const r = await fetch(BRIDGE, { method: "POST", body: JSON.stringify({ cmd, args }) });
  const j = await r.json();
  if (!r.ok) throw new Error(`${cmd}: ${JSON.stringify(j)}`);
  return j;
}

// ---------------------------------------------------------------- demo data
function applyPlan(pack, plan) {
  for (const a of plan.additions) pack.entries[a.key] = a.entry;
  for (const l of plan.links) { const e = pack.entries[l.key]; if (e && !e.pulledInBy.includes(l.parent)) e.pulledInBy.push(l.parent); }
}
async function addTo(pack, projectId, kind = "mod") {
  applyPlan(pack, await inv("plan_add", { pack, projectId, pinVersion: null, forKey: null, altId: null, addedAt: new Date().toISOString(), kind }));
}
async function seed() {
  const fab = JSON.parse(readFileSync(join(ROOT, "src-tauri/tests/fixtures/packs/fabric-26.1.2.planner.json"))).pack;
  const neo = JSON.parse(readFileSync(join(ROOT, "src-tauri/tests/fixtures/packs/neoforge-1.21.1.planner.json"))).pack;
  fab.name = "Cozy Fabric"; neo.name = "Tinker's Workshop";
  const before = structuredClone(fab);
  await addTo(fab, "50dA9Sha", "resourcepack"); // Fresh Animations
  await addTo(fab, "HVnmMxH1", "shader");       // Complementary Reimagined
  fab.snapshots = [{ id: "snap-1", name: "Before the visuals", at: "2026-11-20T18:00:00Z", state: { ...before, snapshots: [] } }];
  // A sample override template, imported from the fake instance in test-env/.
  let tpl = (await inv("list_templates")).find((t) => t.name === "Base configs");
  if (!tpl) {
    const scan = await inv("scan_template_source", { path: "template-src/FakeInstance" });
    tpl = await inv("import_template", { path: scan.path, name: "Base configs", keep: scan.entries.filter((e) => e.suggested).map((e) => e.name) });
  }
  fab.overrideTemplates = [tpl.id];
  fab.appliedTemplates = [await inv("template_applied_ref", { id: tpl.id })];
  await inv("save_pack", { pack: neo });
  await inv("save_pack", { pack: fab });
  return { fab: fab.id, tpl: tpl.id };
}

function fakeInstance() {
  const base = join(ROOT, "test-env/template-src/FakeInstance");
  if (existsSync(join(base, "mmc-pack.json"))) return;
  const files = {
    "mmc-pack.json": "{}",
    "minecraft/config/jei/jei-client.toml": "# JEI client\n[advanced]\n\tcheatMode = false\n\tmaxColumns = 12\n",
    "minecraft/config/sodium-options.json": '{\n  "quality": { "weather_quality": "FANCY" },\n  "performance": { "chunk_builder_threads": 0 }\n}\n',
    "minecraft/kubejs/server_scripts/recipes.js": "ServerEvents.recipes(event => {\n  // Sticks from bamboo, two at a time\n  event.shaped('2x minecraft:stick', ['B', 'B'], { B: 'minecraft:bamboo' })\n})\n",
    "minecraft/options.txt": "fov:0.25\nguiScale:3\n",
  };
  for (const [p, t] of Object.entries(files)) {
    const f = join(base, p);
    mkdirSync(join(f, ".."), { recursive: true });
    writeFileSync(f, t);
  }
}

// ---------------------------------------------------------------- browser
async function launch() {
  const bin = BROWSERS.find((b) => existsSync(b));
  if (!bin) throw new Error("No Chromium-based browser found (Brave or Chrome).");
  const profile = mkdtempSync(join(tmpdir(), "mpp-capture-"));
  const port = 9300 + Math.floor(Math.random() * 500);
  const proc = spawn(bin, [`--headless=new`, `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, `--window-size=${W},${H}`,
    "--hide-scrollbars", "--no-first-run", "--no-default-browser-check", "--disable-extensions", "--force-color-profile=srgb", "about:blank"], { stdio: "ignore" });
  let target;
  for (let i = 0; i < 50 && !target; i++) {
    await sleep(200);
    try { target = (await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()).find((t) => t.type === "page"); } catch { /* not up yet */ }
  }
  if (!target) throw new Error("Browser didn't start.");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((r) => ws.addEventListener("open", r));
  let id = 0;
  const pending = new Map();
  ws.addEventListener("message", (m) => {
    const d = JSON.parse(m.data);
    if (d.id && pending.has(d.id)) { pending.get(d.id)(d); pending.delete(d.id); }
    // Surface the page's own errors (a crash would otherwise just look like missing elements).
    if (d.method === "Runtime.exceptionThrown") console.error("page exception:", d.params.exceptionDetails?.exception?.description?.slice(0, 500));
    if (d.method === "Runtime.consoleAPICalled" && d.params.type === "error") console.error("page error:", d.params.args.map((x) => x.value ?? x.description ?? "").join(" ").slice(0, 500));
  });
  const send = (method, params = {}) => new Promise((res, rej) => { const i = ++id; pending.set(i, (d) => (d.error ? rej(new Error(`${method}: ${d.error.message}`)) : res(d.result))); ws.send(JSON.stringify({ id: i, method, params })); });
  const close = () => { try { ws.close(); } catch { /* */ } proc.kill(); rmSync(profile, { recursive: true, force: true }); };
  return { send, close };
}

// Runs in the page: helpers to find things and draw callouts.
const PAGE_HELPERS = `
window.__mpp = {
  sleep: (ms) => new Promise((r) => setTimeout(r, ms)),
  find(sel, text) {
    const all = [...document.querySelectorAll(sel)];
    const el = text == null ? all[0] : all.find((e) => e.textContent.trim().startsWith(text)) ?? all.find((e) => e.textContent.includes(text));
    if (!el) throw new Error("not found: " + sel + " " + (text ?? ""));
    return el;
  },
  click(sel, text) { this.find(sel, text).click(); },
  setValue(el, v) {
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value").set.call(el, v);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? "change" : "input", { bubbles: true }));
  },
  // A numbered gold marker at the element's top-left, and a dashed box around it.
  /** Clicks the Open button of the pack card that mentions the name. */
  openPack(name) {
    for (const b of document.querySelectorAll("button")) {
      if (b.textContent !== "Open") continue;
      // The card: the largest ancestor that still holds just this one Open button.
      let card = b.parentElement;
      while (card.parentElement && [...card.parentElement.querySelectorAll("button")].filter((x) => x.textContent === "Open").length === 1) card = card.parentElement;
      if (card.textContent.includes(name)) { b.click(); return; }
    }
    throw new Error("no Open button for " + name);
  },
  try(fn) { try { fn(); } catch (e) { console.warn(String(e)); } },
  mark(n, el, opts = {}) {
    const r = (typeof el === "string" ? this.find(el, opts.text) : el).getBoundingClientRect();
    const pad = opts.pad ?? 4;
    const layer = document.getElementById("mpp-marks") ?? Object.assign(document.body.appendChild(document.createElement("div")), { id: "mpp-marks" });
    layer.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:2147483647";
    if (opts.box !== false) {
      const b = document.createElement("div");
      b.style.cssText = "position:fixed;border:2px dashed #ffc23d;border-radius:4px;box-shadow:0 0 0 1px #000a" +
        ";left:" + (r.left - pad) + "px;top:" + (r.top - pad) + "px;width:" + (r.width + 2 * pad) + "px;height:" + (r.height + 2 * pad) + "px";
      layer.appendChild(b);
    }
    const d = document.createElement("div");
    d.textContent = n;
    const x = opts.right ? r.right + pad - 14 : r.left - pad - 14, y = opts.bottom ? r.bottom + pad - 14 : r.top - pad - 14;
    d.style.cssText = "position:fixed;width:28px;height:28px;border-radius:50%;background:#ffc23d;color:#111;border:2px solid #111;" +
      "font:700 15px/24px system-ui,sans-serif;text-align:center;box-shadow:0 2px 6px #000c;left:" + Math.max(2, x) + "px;top:" + Math.max(2, y) + "px";
    layer.appendChild(d);
  },
  unmark() { document.getElementById("mpp-marks")?.remove(); },
};`;

// ---------------------------------------------------------------- scenes
// Each scene runs in the page (async function body) and draws its callouts.
const SCENES = {
  packs: `
    $.click('[role=tab]', 'Settings'); await $.sleep(200); $.click('[role=tab]', 'Packs'); await $.sleep(1500);
    $.mark(1, $.find('[role=tablist]'), { pad: 2 });
    $.mark(2, $.find('.app-help'), { right: true });
    $.mark(3, $.find('button', 'Open project'));
    $.mark(4, $.find('button', 'Import pack'));
    $.mark(5, $.find('button', 'Import Prism instance'));
    $.mark(6, $.find('button', 'New pack'));
    const open = [...document.querySelectorAll('button')].filter((b) => b.textContent === 'Open').pop();
    let card = open.parentElement; while ([...card.parentElement.querySelectorAll('button')].filter((x) => x.textContent === 'Open').length === 1) card = card.parentElement;
    $.mark(7, card);`,
  workspace: `
    $.click('[role=tab]', 'Packs'); await $.sleep(1200);
    $.openPack('Cozy Fabric');
    await $.sleep(6000);
    const row = [...document.querySelectorAll('.entry')].find((e) => e.textContent.includes('Sodium')) ?? document.querySelector('.entry');
    row.click(); await $.sleep(5000);
    $.mark(1, $.find('select[aria-label="Content type"]'));
    $.mark(2, $.find('input[aria-label^="Search"]'));
    $.mark(3, $.find('[role=tablist][aria-label=Browse]'));
    $.mark(4, $.find('.checks, .mc-notice, .pane__fixed .stack > div'));
    $.mark(5, $.find('[role=tablist][aria-label^="View"]'));
    $.mark(6, $.find('.group__title'));
    $.mark(7, $.find('.pane:last-child .mc-panel__title, .pane:last-child h2, .pane:last-child'), { pad: -6 });
    $.mark(8, $.find('button', '↶ Undo'));
    $.mark(9, $.find('button', 'Snapshots'));
    $.mark(10, $.find('button', 'Export'));
    $.mark(11, $.find('select[aria-label="Preferred site"]'));`,
  graph: `
    $.click('[role=tab]', 'Graph'); await $.sleep(6000);
    // Zoom in on Sodium's chain so nodes are readable, and select it (highlights the chain).
    const cy = window.__cy;
    $.try(() => $.click('.graph-bulk button', 'Clear')); await $.sleep(300);
    const sod = cy.nodes('node.mod').filter((n) => n.data('label') === 'Sodium');
    cy.$(':selected').unselect(); sod.select(); await $.sleep(2500);
    cy.zoom(0.85); cy.center(sod); await $.sleep(800);
    $.mark(1, $.find('select[aria-label="Group graph by"]'));
    $.mark(2, $.find('select[aria-label="Show one group"]'));
    $.mark(3, $.find('button', 'Re-layout'));
    $.mark(4, $.find('.graph-canvas'), { box: false, pad: -60 });
    $.mark(5, $.find('.graph-legend, .graph-canvas + *'), { pad: 2 });
`,
  overrides: `
    $.click('[role=tab]', 'Overrides'); await $.sleep(2500);
    $.mark(1, $.find('.tpl-order .entry'));
    $.mark(2, $.find('select[aria-label="Template to attach"]'));
    $.mark(3, $.find('.pixel', 'In overrides'));
    $.mark(4, $.find('.pixel', 'Configs by mod'));`,
  snapshots: `
    $.click('[role=tab]', 'List'); await $.sleep(500);
    $.click('button', 'Snapshots'); await $.sleep(1500);
    $.click('.mc-modal button', 'Compare'); await $.sleep(800);
    $.mark(1, $.find('.mc-modal form'));
    $.mark(2, $.find('.mc-modal .entry-list .entry'));
    $.mark(3, $.find('.mc-modal .diff'));`,
  export: `
    $.click('.mc-modal button', 'Done'); await $.sleep(400);
    $.click('button', 'Export…'); await $.sleep(8000);
    $.mark(1, $.find('.mc-modal [role=tablist]'));
    $.mark(2, $.find('.mc-modal .mc-notice'));
    $.try(() => $.mark(3, $.find('.mc-modal .issue-list')));
    $.mark(4, [...document.querySelectorAll('.mc-modal details')].pop(), { pad: 2 });`,
  preview: `
    $.click('.mc-modal button', 'Close'); await $.sleep(400);
    $.click('[role=tablist][aria-label=Browse] [role=tab]', 'Payloads'); await $.sleep(1200);
    const qol = [...document.querySelectorAll('.hit')].find((h) => h.textContent.includes('QoL and HUD'));
    [...qol.querySelectorAll('button')].find((b) => b.textContent.startsWith('Add')).click(); await $.sleep(9000);
    $.mark(1, $.find('.mc-modal .muted'));
    $.mark(2, $.find('.mc-modal .plan-rows'));
    $.mark(3, [...document.querySelectorAll('.mc-modal button')].pop(), { right: true });`,
  payloads: `
    $.click('.mc-modal button', 'Cancel'); await $.sleep(400);
    $.click('[role=tablist][aria-label=Browse] [role=tab]', 'Search'); await $.sleep(400);
    $.click('[role=tab]', 'Payloads'); await $.sleep(1500);
    $.click('.entry', 'Optimization'); await $.sleep(1200);
    $.mark(1, $.find('.entry-list'));
    $.mark(2, $.find('button', 'New'));
    $.mark(3, $.find('.slot-ed'));
    $.mark(4, $.find('.cand-list .cand'));
    $.mark(5, $.find('button', '+ Choice'));
    $.mark(6, $.find('button', 'Saved'), { right: true });`,
  templates: `
    $.click('[role=tab]', 'Templates'); await $.sleep(1500);
    $.click('.entry', 'Base configs'); await $.sleep(1200);
    $.click('.tpl-file', 'jei'); await $.sleep(1000);
    // Don't show this machine's folder path in a public screenshot.
    for (const c of document.querySelectorAll('code')) if (c.textContent.includes('/templates/') || c.textContent.includes('bridge-templates')) c.textContent = '~/Library/Application Support/io.github.adaghastthemad.modscapemanager/templates/<id>/files';
    $.mark(1, $.find('button', 'Import…'));
    $.mark(2, $.find('.tpl-files'));
    $.mark(3, $.find('.mini-badge'), { right: true });
    $.mark(4, $.find('.code-ed'));
    $.mark(5, $.find('button', 'Open externally'));
    $.mark(6, $.find('button', '✕ Close'), { right: true });`,
  settings: `
    $.click('[role=tab]', 'Settings'); await $.sleep(1500);
    document.querySelector('.danger-zone').open = true; await $.sleep(300);
    document.querySelector('.danger-zone').scrollIntoView({ block: 'end' }); await $.sleep(300);
    $.mark(1, $.find('.danger-zone'));`,
  shortcuts: `
    window.dispatchEvent(new KeyboardEvent('keydown', { key: '?', bubbles: true })); await $.sleep(600);`,
  livescan: `
    $.click('.mc-modal button', 'Close'); await $.sleep(300);
    $.click('[role=tab]', 'Workspace'); await $.sleep(1500);
    $.click('button', '← Packs'); await $.sleep(1200);
    $.openPack("Tinker's Workshop"); await $.sleep(7000);
    $.click('button', 'Live Scan'); await $.sleep(4000);
    // The browser has no folder dialog: point "choose an instance" at the old NeoForge test jars in test-env/.
    const real = window.__TAURI_INTERNALS__.invoke;
    window.__TAURI_INTERNALS__.invoke = (cmd, args) => cmd === 'scan_instance' ? real('dev_scan_folder', { ...args, path: 'downloads/neoforge-1.21.1' }) : real(cmd, args);
    $.click('.mc-modal button', 'Choose an instance'); await $.sleep(12000);
    window.__TAURI_INTERNALS__.invoke = real;
    // Don't show this machine's folders in a public screenshot.
    for (const d of document.querySelectorAll('.mc-modal .mc-notice div')) if (d.textContent.startsWith('Read ')) d.textContent = d.textContent.split(' from ')[0] + " from ~/Library/Application Support/PrismLauncher/instances/Tinker's Workshop/minecraft/mods (nothing was changed).";
    $.mark(1, $.find('.mc-modal .mc-notice'));
    $.mark(2, $.find('.mc-modal .issue-list .issue'));
    $.mark(3, $.find('.mc-modal .issue-list .issue button.mc-btn, .mc-modal .issue-list .mc-btn'), { right: true });`,
};

// Crops for scenes that are mostly empty space.
const CLIPS = {
  packs: { x: 0, y: 0, width: W, height: 320 },
  shortcuts: { x: 410, y: 140, width: 620, height: 660 },
};

async function main() {
  fakeInstance();
  await sleep(300);
  const seeded = await seed();
  console.log("seeded", seeded);
  const b = await launch();
  try {
    await b.send("Page.enable");
    await b.send("Runtime.enable");
    await b.send("Emulation.setDeviceMetricsOverride", { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    const bridge = `window.__TAURI_INTERNALS__ = { invoke: (cmd, args) => fetch(${JSON.stringify(BRIDGE)}, { method: "POST", body: JSON.stringify({ cmd, args }) }).then(async (r) => { const j = await r.json(); if (!r.ok) throw j; return j; }) };`;
    await b.send("Page.addScriptToEvaluateOnNewDocument", { source: bridge + PAGE_HELPERS });
    await b.send("Page.navigate", { url: PAGE });
    await sleep(2500);
    for (const [name, body] of Object.entries(SCENES)) {
      const run = async () => {
        const r = await b.send("Runtime.evaluate", { expression: `(async () => { const $ = window.__mpp; $.unmark(); ${body} })()`, awaitPromise: true, returnByValue: true });
        if (r.exceptionDetails) throw new Error(`${name}: ${r.exceptionDetails.exception?.description ?? r.exceptionDetails.text}`);
      };
      try { await run(); } catch (e) { console.error(String(e)); if (only === name) throw e; }
      if (only && only !== name) continue;
      await sleep(400);
      const clip = CLIPS[name] && { ...CLIPS[name], scale: 1 };
      const shot = await b.send("Page.captureScreenshot", { format: "png", ...(clip ? { clip } : {}) });
      writeFileSync(join(OUT, `${name}.png`), Buffer.from(shot.data, "base64"));
      console.log("saved", name);
    }
  } finally { b.close(); }
}
main().catch((e) => { console.error(e); process.exit(1); });
