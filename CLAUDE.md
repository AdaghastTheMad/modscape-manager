# Modpack Planner — CLAUDE.md

Source of truth: `Modpack Planner — Design Doc.md` (this folder). This file summarises it; when they disagree, the design doc wins unless a decision below says otherwise. Keep this file updated as milestones land and decisions are made.

Owner: **AdaghastTheMad**. In all project content (README, credits, metadata, changelogs, commit authors, User-Agent contact) refer to the owner only by that username. No real names or personal emails anywhere.

## What it is

A standalone **Tauri 2 desktop app** (Linux Mint + macOS, no Windows) for planning Minecraft modpacks as **metadata only**. Pick mods, see their dependency graph, export a pack file a launcher installs. **The app never downloads a mod jar.**

- Minecraft **1.21.1** and **26.1.2** first-class; any version Modrinth/CurseForge lists is selectable (open version picker).
- Loaders: NeoForge, Fabric, Forge, Quilt — one per pack.
- Content: mods first; resource packs, shaders, datapacks later (milestone 10) without changing mod behaviour.
- Primary install target: **Prism Launcher** (`.mrpack` and CurseForge zip exporters first).
- Look: Minecraft Java menus, darkened (bevelled buttons, inventory slots, purple-bordered tooltips, OFL pixel font for headings/buttons only), modern spacing, sans-serif body text, WCAG AA, full keyboard use.

**Non-goals v1:** downloading/launching/testing packs, editing configs/KubeJS/datapacks (beyond override templates), accounts/cloud sync, detecting runtime conflicts metadata can't express.

## Architecture

- **Rust core** makes every network call (reqwest), owns SQLite (rusqlite), the resolver and export writers. **Webview has no network access**; UI talks to the core only via typed Tauri commands.
- **UI:** Vite + React + TypeScript; Cytoscape.js for the graph.
- **CurseForge API key** lives in the OS keychain (macOS Keychain / Linux Secret Service), never reaches the UI. No key → Modrinth-only mode.
- Each API is a client module behind one **`ModSource` trait**; the UI never cares which site a mod came from.
- Modrinth calls carry a unique, generic User-Agent `modpack-planner/<version>` (no personal contact; add the public project URL once one exists, e.g. `modpack-planner/<version> (+<repo url>)`).
- **Cache:** every API response in SQLite with timestamp, 24 h TTL, manual refresh.
- **Storage:** SQLite in the app data dir (packs, payloads, API cache). Packs and payloads also save/load as plain JSON. Override templates are plain folders in the app data dir.
- Allowed outbound hosts: Modrinth, CurseForge, loader version metadata. Nothing else. **No telemetry.**

### API notes
| | Modrinth | CurseForge |
|---|---|---|
| Auth | none | `x-api-key` |
| Rate limit | 300 req/min/IP | unpublished, cache hard |
| Search | `/v2/search` (facets) | `/v1/mods/search` (`gameVersion`, `modLoaderType`, `categoryId`) |
| Deps | per version: required/optional/incompatible/embedded | per file `relationType`: 1 embedded, 2 optional, 3 required, 4 tool, 5 incompatible, 6 include |
| Categories | `/v2/tag/category` | `/v1/categories?gameId=432` |
| Bulk | `/v2/projects?ids=`, `/v2/versions?ids=` | `POST /v1/mods`, `POST /v1/mods/files` |

- Deps live on a **file/version**, not the project: pick the file for (MC version, loader) first, then read its deps. Unpinned deps → newest compatible file.
- Flag CF files with `allowModDistribution: false` (manual download in launchers).
- Cross-platform matching by slug + name, user confirms/links; store both IDs on the entry.

## Data model (revised Nov 24 — overrides the design doc's TS sketch)
Owner granted latitude to fix flawed model/design details so the whole vision works; harden against messy mod/dependency edge cases.

- **Entry keys** are app-generated stable IDs (not a Modrinth/CF ID), so linking a mod's other-site copy never re-keys it.
- `PackEntry.payloadIds: string[]` (a mod can come from several payloads); `pulledInBy: string[]` of entry keys.
- **Choice slots** have an `id`; `options: ModRef[][]` (each option may be a bundle) plus `common: ModRef[]` added whichever option is picked.
- **Variants:** an item may give an ordered fallback list per loader *and* per MC version; the first candidate with a build for (version, loader) wins; none → flagged, never silently dropped.
- Packs record `appliedPayloads` / `appliedTemplates: {id, version}[]` so "updated" notices can be computed.
- **Snapshots** store the whole pack state (entries, choices, loader version, source preference, templates…), minus snapshots themselves.
- **API cache key** = method + URL + request body (CF bulk lookups are POSTs).
- Side (client/server) comes from Modrinth; CF-only mods show side "unknown".
- Base shape otherwise as the design doc: `Pack`, `PackEntry` (ModRef with modrinthId/curseforgeId/preferred, optional pinned fileId, kind, reason, tags, notes, side, graphPos), `Payload` (nested `includes`, version), `OverrideTemplate` (folder, files, version), API cache.

## Feature summary
- **Workspace:** browse (left) · pack as list or graph (centre) · details (right).
- **Pack setup:** wizard (name, MC version, loader, loader version from loader metadata, icon); duplicate-to-version with a port report.
- **Search:** merged MR/CF results with badges, pre-filtered to pack version/loader (toggle to show incompatible greyed), filters, sorts, hover tooltip card, full description in details.
- **Resolve:** add → required deps auto-added ("pulled in by X"), optional deps suggested; remove → orphan cleanup offered; libraries dimmed.
- **Checks (continuous):** missing required dep, declared incompatibility, no file for version/loader, client/server-only, CF distribution blocked, not updated in >1 year (soft).
- **Organise:** group by category / custom tag / pulled-in-by; notes; pin or float file versions; undo/redo; named snapshots with compare/rollback.
- **Graph:** force-directed, icon-in-slot nodes sized by dependents, solid/dashed/red edges, category regions, chain highlighting, filters, drag-to-add, saved positions. Test up to ~300 mods.
- **Payloads:** version-agnostic mod bundles; choice slots; loader variants; nesting; drop-in preview; update notices; JSON import/export; starter payloads (list in design doc).
- **Export/import:** `.mrpack`, CurseForge zip, packwiz, Markdown/CSV, planner `.json`. Pre-flight warnings panel with fixes. Source preference per pack (default Prefer Modrinth). Datapacks via Paxi. Import mrpack/CF zip/packwiz and **read-only** Prism instance import.
- **Override templates:** reusable `config/`, `kubejs/` etc. folders; library view; light text editor; ordered attachment (later wins) with clash display; versioning; exported to `overrides/`.

## Milestones
Stop after each milestone and wait for AdaghastTheMad's review before starting the next. (Order revised Nov 24: duplicate-to-version moved up to M6; later milestones shifted by one.)

1. **Skeleton and theme** — `scripts/setup.sh`, Tauri 2 + Vite + React + TS project, Minecraft-dark component kit (button, panel, slot, tooltip, input, tabs), settings screen saving CF key to OS keychain. *Done when the app opens on macOS and a test CurseForge search returns from the core.* (Linux Mint check deferred until later milestones, by owner's decision.)
2. **Modrinth search and pack list** — wizard with open version picker, Modrinth search + filters, add/remove, list grouped by category, SQLite cache, project JSON save/load.
3. **Dependency resolution and checks** — file selection, auto-add required deps, optional suggestions, orphan cleanup, check list with inline warnings.
4. **CurseForge** — search via core, merged results, cross-platform matching, source preference.
5. **Export to Prism** — `.mrpack` + CF zip exporters, pre-flight panel, importers for both, Prism instance import. *Done when a 1.21.1 NeoForge pack and a 26.1.2 Fabric pack import into the isolated test Prism and launch with no missing mods.*
6. **Duplicate to another version** — copy a pack to another MC version/loader with the port report (has a build / no build / deps changed).
7. **Graph view.**
8. **Payloads.**
9. **Override templates.**
10. **Polish** — snapshots/compare, keyboard shortcuts, undo/redo, click sound, empty states.
11. **Other content types** — RP/shaders/datapacks (+Paxi), packwiz exporter, Markdown/CSV lists.
12. **Installers** — `scripts/build.sh` → `.deb`, `.AppImage`, `.dmg`.
13. **Server variant** (nice-to-have).

**Status:** M1 done (Nov 24, 2026, macOS): app opens, CF key saved to Keychain, CF test search returns via the core. Awaiting owner review before M2. Linux Mint check still pending.

## Running it
- `scripts/setup.sh` (or `--check`) → `scripts/dev.sh` (hot reload). Rust comes from rustup in `~/.cargo` (dev.sh sources `~/.cargo/env`).
- Rust tests: `cd src-tauri && cargo test`. Frontend: `pnpm typecheck`, `pnpm build`.

## Rules (from "Instructions for Claude Code" + owner's hard rules)
- **Mods first, and reliable.** Milestones 1–5 are the core. Don't start milestone 11 (other content types) until resolver tests pass and the M5 Prism round-trip works for both versions. Other content types may reuse the core but must never change its behaviour for mods.
- **Never touch the owner's own Prism data** — never read, write or launch `~/.local/share/PrismLauncher`, the Flatpak equivalent, or `~/Library/Application Support/PrismLauncher`. Test only in an isolated Prism data dir inside the project (`test-env/prism/`, git-ignored) via Prism's portable build or `--dir`, managed by `scripts/test-prism.sh` (create / start / `--clean` wipes). Instance import is tested on fixture instances built in that test copy.
- **Accounts:** launching Minecraft needs sign-in. First time, pause and ask the owner to sign in to the test Prism themselves; never handle credentials. Until then the round-trip stops at "pack imports and every file downloads".
- Test downloads land only inside `test-env/`.
- **Ask before any `sudo` or system install.** Setup/build scripts check first, report what's missing, ask before installing or using sudo; re-runnable ("all good" when complete). Linux Mint (apt) + macOS (Homebrew) only.
- Resolver tests use **recorded API responses**, not live APIs: required/optional/incompatible/embedded deps, unpinned deps, no build for version, circular deps.
- Fixture packs: NeoForge 1.21.1 (~150 mods), Fabric 26.1.2 (~50 mods).
- **No telemetry, analytics, or third-party calls** beyond the two mod APIs and loader version metadata.
- **Original assets only** — no Mojang textures, sounds or font files.
- Keychain unavailable (e.g. no Secret Service on Linux) → show a clear error with a fix-it guide and run Modrinth-only. **Never** fall back to storing the key in plaintext.
- Keyboard shortcuts use each platform's native modifier (Cmd on macOS, Ctrl on Linux); Backspace or Delete removes on macOS.
- Isolated Prism: launch the installed Prism Launcher binary with `--dir test-env/prism` (owner approved), so the owner's own instances never appear or get touched.
- Never enter the owner's CurseForge key or any credential on their behalf; the owner pastes it into the settings screen.

## Decisions log
- Nov 23, 2026: all items in the design doc's "Decided" list.
- Nov 24, 2026: it's a Tauri desktop app (the doc's "local web app" wording is stale). M1 is judged on macOS only; Mint later. setup.sh installs Rust via rustup (portable to other Macs). pnpm via corepack. git author AdaghastTheMad <355918907+AdaghastTheMad@users.noreply.github.com>. Data-model revisions above. Duplicate-to-version moved to M6. Starter payload names (Iris/Oculus Search, Roxy, Diorama, etc.) to be verified against live data at the payloads milestone.
- Pixel font: Pixelify Sans (OFL, bundled via `@fontsource/pixelify-sans`; no runtime font fetch). Monocraft isn't on npm and could replace it later.
