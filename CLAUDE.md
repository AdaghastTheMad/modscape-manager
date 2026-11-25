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
- **Override templates:** reusable `config/`, `kubejs/` etc. folders; library view; light text editor; ordered attachment (top of the list wins, like Minecraft resource packs; owner, M9 review) with clash display; versioning; exported to `overrides/`.

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
13. **Server variant** (nice-to-have). Includes a "for a hosted server" export that leaves client-only mods out.
14. **Live Scan** (extra stretch goal, owner idea Nov 24). Opt-in: fetch a pack's jars (or point at a Prism instance that's been imported but not run, read-only) into a local scan folder, read each jar's own metadata (mods.toml / neoforge.mods.toml / fabric.mod.json / quilt.mod.json, jar-in-jar libraries), and flag missing requirements and version-ranged incompatibilities the sites don't list, with fixes. No Minecraft install needed. `scripts/check-jar-metadata.py` is the working prototype.

**Status:** M1–M7 done (committed). M8 (payloads) done (committed). M9 (override templates) done (committed). M10 (polish) done (committed). M11 (other content types) done (committed; owner imported it in Prism). M12 (installers) in progress. Linux Mint check still pending.

## Running it
- `scripts/setup.sh` (or `--check`) → `scripts/dev.sh` (hot reload). Rust comes from rustup in `~/.cargo` (dev.sh sources `~/.cargo/env`).
- Rust tests: `cd src-tauri && cargo test`. Resolver tests run on recorded Modrinth responses in `src-tauri/tests/fixtures/resolver/` (`modrinth.json` from `python3 scripts/record-fixtures.py`, plus hand-written `synthetic.json` for cases real data lacks). Live API smoke test (not in the normal run): `cargo test live -- --ignored --nocapture`. Frontend: `pnpm typecheck`, `pnpm build`.
- App data (SQLite `planner.db`: packs + API cache) lives in the OS app-data dir for `io.github.adaghastthemad.modpackplanner`.

## Implementation notes
- Every outbound request goes through `src-tauri/src/fetch.rs`: host allow-list (`ALLOWED_HOSTS`), https only, SQLite cache (24 h; icons 30 d), stale-cache fallback when offline or on 5xx.
- Mod icons reach the webview only as data: URLs from the `icon` command; the CSP blocks all remote loads.
- Pack project files: `{ format: "modpack-planner-project", schemaVersion: 1, pack }`. Unknown fields are preserved (`extra`), newer schema versions are refused with a clear message, imports are normalised (duplicates, dangling links, missing ids) and re-id'd on collision.
- NeoForge versions: 1.X.Y → `X.Y.*`, year-based 26.A.B → `26.A.B.*`. Quilt accepts Fabric mods in search/compat checks.
- **Library detection:** user override wins; else a mod is a library if "library" is its only Modrinth category, or it has the tag and was pulled in as a dependency (Modrinth tags JEI/EMI as library too, so tags alone mislead).
- **Resolver (`src-tauri/src/resolve.rs`)**, all behind the `Catalog` trait:
  - File choice follows the pack's `channelPolicy`: `stable` (default) = newest *release* for (MC, loader), else newest beta, else alpha; `newest` = newest build of any channel. On Quilt a native Quilt build beats a Fabric one of the same rank. Toggle lives in the workspace bar. Pinned `fileId` wins; a pin for another version/loader or a deleted file is an error with an Unpin fix.
  - Dependency pins (`version_id`) are honoured only when that exact version fits the pack; otherwise unpinned deps float to the newest compatible file. A floating dep that differs from a compatible pin → warning with "Pin X" fix.
  - `plan_add` walks required deps breadth-first, cycle-safe, capped at 100 new entries; existing entries get links; deps that no longer exist are reported, never added; embedded deps ignored; optional deps become suggestions (dismissals stored in `pack.dismissedSuggestions`). Fixes for missing deps pass `forKey` so the new mod is recorded as that entry's dependency.
  - `PROVIDES` table: QSL/QFAPI satisfies Fabric API on Quilt.
  - Checks: missing dependency, incompatible (once per pair), no build, pin mismatch/missing, version mismatch, client-/server-only (info), stale >1 year (warning, skipped when pinned), unneeded dependency (info). Sorted error → warning → info.
  - `pulledInBy` is re-synced from each complete analysis; incomplete analyses (offline, CurseForge-only entries) never sync and never mark anything unneeded.
  - Orphan cleanup (`model::plan_removal`) is reachability-based: a dependency survives only if something kept still pulls it in, so chains and cycles are handled.
- **CurseForge (M4):** inside the resolver CurseForge ids are `cf:<n>` (mods and files); pack entries store plain numbers (`curseforgeId`, and `fileId` pins may be stored either way; `resolve::target` normalises). `resolve::Multi` routes `cf:` ids to CurseForge, the rest to Modrinth. The key is read from the keychain once and kept in memory until saved/cleared (`forget_key`).
  - Categories: CurseForge names map onto Modrinth slugs via `CATEGORY_MAP` (e.g. "API and Library" → `library`); unknown ones are slugified. A category filter with no CurseForge equivalent leaves CurseForge out (with a note). CurseForge has no client/server side data, so side filters don't apply to it and side checks skip CF-only mods.
  - Files: releaseType 1/2/3 → release/beta/alpha; relationType 3 required, 2 optional, 5 incompatible, 1/6 embedded, 4 ignored. No `downloadUrl` ⇒ distribution blocked ⇒ warning, with a "Use Modrinth" fix when linked. Untagged old files count as Forge. Quilt packs query Fabric and Quilt files.
  - Merged search (`sources/merge.rs`): same normalised slug *and* name ⇒ one row with `also`; relevance interleaves, downloads/updated sort the combined list; no key or a CF error ⇒ Modrinth-only with a note.
  - Cross-site matching (`sources/matching.rs`): `verified` = identical SHA-1 file (CF→MR via Modrinth's hash lookup; MR→CF by comparing candidate files), `likely` = same slug and name, `possible` = one of them. Only `verified` is pre-ticked; the user confirms links. Dependencies from the other site link to an existing entry with the same slug+name (`IdLink`) instead of duplicating.
  - **Manual links** (owner, M11 review): Details → "Search <site> yourself…" picks the copy by hand. Every link you confirm (picked, matched or manual) is remembered app-wide in SQLite meta `manual_links` (`[modrinthId, curseforgeId]` pairs, one partner each; Unlink forgets it) and comes back first as confidence `manual`, pre-ticked in Match sites. Name matching also compares `matching::core_name` (drops `[tags]`, `(…)`, " - …" suffixes and loader words), and searches that core name too, so "[ETF] Entity Texture Features" finds "[ETF] Entity Texture Features - [Fabric & Forge]".
  - Source preference: Prefer Modrinth/CurseForge switches every entry linked on both sites (clearing pins, with Undo); Mixed leaves per-mod choices. Per-mod "Use …" / "Unlink" in the details pane.
  - Real CurseForge fixtures: `scripts/record-cf-fixtures.sh` (owner runs it; it reads the saved key, macOS may ask) → `tests/fixtures/resolver/curseforge.json`; `recorded_curseforge_jei_resolves` runs once data exists.
- **Export / import (M5, `export.rs`, `import.rs`):**
  - `plan_export` is the pre-flight and picks files exactly as the checks do. `.mrpack` holds Modrinth-sourced files only (sha1 + sha512 + allowed host required); CurseForge zip holds CurseForge-sourced only (ids). Entries set to the other site are excluded with a bulk "Switch N to …" fix; one-site-only entries point to "Match sites". Missing loader version, lookup failures or jar-name collisions block export. Distribution-blocked files warn (Prism asks for a manual download).
  - Exports carry no personal data (`author` is empty). `env` comes from Modrinth sides. Forge versions are exported without the MC prefix. `overrides/` is always written, filled from the pack's override templates (M9).
  - Imports identify by metadata: `.mrpack` by sha1 (Modrinth `version_files`) or CurseForge file id decoded from forgecdn URLs (`/files/AAAA/BBB/` → AAAA*1000+BBB); CF zip by project/file ids; Prism instance by `mmc-pack.json` + `mods/.index/*.pw.toml`, then sha1 → Modrinth, then CurseForge fingerprint (MurmurHash2 seed 1, whitespace stripped). Imported mods are pinned; libraries other mods require become dependencies. Instance import only reads (a test asserts the folder is unchanged).
  - Non-mod files in packs are skipped with a note (M11). When the source has config/override files, the import report offers "Save as template…", which imports them as a template already attached to the new pack.
- **Lessons from the M5 round-trip (keep these):**
  - `.mrpack` env rule (`export::export_env`, from two failed round-trips): **every file is `client: required`**; `server` is `unsupported` only when Modrinth says the mod can't run on a server, else `required`. Never `optional` (Prism treats those as opt-in extras and leaves them unticked) and never client `unsupported` (singleplayer is an integrated server, so server-only mods are needed).
  - Dependency data from Modrinth was complete in both failures; the cause was always the env flags. The jar checker proved it by reproducing NeoForge's error list from the installed jars.
  - Mods declare conflicts inside their jars that Modrinth/CurseForge don't list (e.g. Sodium/Iris vs Embeddium, Reese's Sodium Options vs Sodium Options API). The planner can't read jars, so `src-tauri/data/known_conflicts.json` (each entry with a source) feeds the incompatible check. Add to it when a launch log shows a new one.
  - `scripts/check-jar-metadata.py <mods folder>` predicts NeoForge/Forge load errors from downloaded test jars (missing requirements, version-ranged incompatibilities); it reproduced the loader's own error list exactly. Run it on `test-env/downloads/...` before asking the owner to launch.
- **Dependency double-check (`pack.crossCheckDeps`, off by default):** for entries linked on both sites, the other copy's required deps are read too; ones not in the pack (also not matched by slug+name) become `missing_dependency_other_site` warnings with an Add fix.
- **No client-only / server-only notes** in the checks (owner, M6 review): singleplayer runs both sides and client exports install everything. Sides stay in the details pane and drive the server variant (M13), which is where "for a hosted server: leave out client-only mods" belongs.
- **Duplicate to version (M6, `port.rs`):** dropped dependencies come back as `DroppedDep {hasBuild, wasNeededBy, library}`; ticking them sets `keep` and re-runs the plan, carrying them as user mods (so they're checked for builds and pull in their own new deps, e.g. Iron's Lib for Iron's Spells). "Tick mods with builds" selects non-libraries that have a target build.
- **Duplicate to version (M6, details):** user-added and payload mods carry over with tags/notes/settings; pins and graph positions are dropped (version-specific). Each is checked for a target build; if only its other-site copy has one, the entry switches site. Without any build it's reported and left out (or kept, flagged, with "keep missing"). Dependencies are re-derived on the target by iterating missing-dependency fixes, reusing old dependency entries (and their notes) when still needed. The report lists ported / no build / changed deps (added, removed) / new deps / dropped deps / pins reset. New keys never collide with old ones. Live check: the 150-mod NeoForge 1.21.1 fixture ports to 26.1.2 in ~16 s.
- **Graph (M7, `src/screens/workspace/Graph.tsx`):** Cytoscape + fcose. Mods are nodes inside compound "region" nodes (category, or first tag); edges come from the analysis (required solid, optional dashed, incompatible red tee; embedded never). Node size grows with dependents; red border = error, gold double = warning, dashed = pinned position. Positions live in `entry.graphPos` and are saved after layouts and drags: saved nodes stay put, only new ones are laid out (fcose `fixedNodeConstraint`); Re-layout keeps pinned (dragged) nodes. Selection highlights the full dependency chain both ways. Right-click menu, filters (hide libraries / one group / only problems), drag a search result onto the canvas to add it there, `G` toggles list/graph. Layout params were tuned on the 150-mod fixture (strong compound gravity). Dev builds expose `window.__cy` for testing.
  - **Navigation (owner, M7 review):** Cytoscape's own wheel zoom is off. Two-finger trackpad scroll pans; pinch zooms (ctrl+wheel in Chromium/WebKitGTK, `gesturestart/gesturechange` in WKWebView, i.e. the macOS app); a real mouse wheel (whole-notch deltas) and ⌘/Ctrl+scroll zoom gently.
  - **Selection:** Cytoscape-native (click = one, ⌘/Ctrl/Shift-click adds, Shift-drag box). Region boxes have `events: no` so pans and boxes start anywhere. A bulk bar (2+ selected) pins/unpins positions, pins/floats versions, tags all. "Unpin all" releases every pinned position and re-lays out.
  - Cytoscape keeps the data object you pass to `cy.add`, so always pass a copy (a shared object let React's dev double-mount mark every node "placed" and stack them at 0,0). Packs with every saved position at 0,0 are treated as unplaced and re-laid out.
- **Grouping (`src/grouping.ts`):** list and graph share one vocabulary (category / first tag / pulled in by); independent by default, synced when Settings → Interface → "Keep … in sync" is on (stored in webview localStorage via `src/prefs.ts`).
- **Tags & notes** are edited in the details pane (tags autocomplete from the pack); graph "Add tag…" focuses it.
- **Payloads (M8, `src-tauri/src/payload.rs`, `src/screens/Payloads.tsx`, `workspace/PayloadDialog.tsx`):** stored in SQLite (migration v2 adds `payloads` + `meta`). Items are `mod` slots (ordered candidates, each optionally limited to loaders and/or MC versions, `26.*` = prefix; first that fits and has a build wins) or `choice` items (options are bundles of slots, plus `common`). The pick lives in `pack.choices`; auto-pick prefers the option whose mods you added yourself (not ones present only as dependencies); never both options. `includes` nest payloads (cycle-safe; loops refused on save). Saving bumps `version` only when items/includes change; packs record `appliedPayloads {id, version}` for every payload pulled in (nested too) and show "updated → Review". Payload-added mods get reason `payload` + `payloadIds`. Resource-pack candidates show "arrives in M11". "Save as payload" skips dependencies.
  - **Review after a payload changes:** `PayloadPlan.removed` lists pack mods tagged with the payload (or a nested one) that no candidate matches any more. Each row warns when other mods need it ("will break"), another payload also has it, or you added it yourself; only rows with none of those are pre-ticked. Ticked rows are removed with their orphans (Undo); unticked ones lose the payload tag and become `user` mods if no payload is left.
  - **Order:** items, options and slots move with ↑/↓ (candidates already did: that's fallback priority). The version fingerprint (`content_of`) sorts items/options/slots by id, so reordering alone never bumps the version or sends packs an "updated" notice; candidate order does count.
  - **Unsaved edits:** payloads change only on Save. Switching payload, New/Import, or leaving the Payloads tab with unsaved edits asks Keep editing / Discard / Save (`useUnsavedGuard` in `Payloads.tsx`).
  - Starter payloads: `src-tauri/data/starter_payloads.json`, built from the design doc by `scripts/resolve-starter-payloads.py` (Modrinth). Name fixes: C2ME → separate Fabric/NeoForge projects (loader-limited candidates), "Keyboard Atlas" → Keybind Atlas, "Windy: Configurable" → Windy, "Fresh Compat" → Fresh Compats, "Fluid and Foolish" → Fluid & Foolish. Seeded once (meta `starters_seeded`); "Restore starter payloads" re-adds deleted ones.
- **Drag and drop:** `tauri.conf.json` sets `dragDropEnabled: false`, otherwise Tauri's native file-drop handler swallows HTML5 drags in WKWebView (drags worked in the browser, never in the app). Dropping files onto the window would need it back on; the Import buttons cover that. In-app drags go through `src/dnd.ts`, which keeps the dragged item in memory as well as in `dataTransfer` (WKWebView doesn't reliably pass custom MIME data, e.g. when a drag starts on an icon image); icons are `draggable=false`. The list and the graph both accept mods and payloads.
- **Override templates (M9, `src-tauri/src/templates.rs`, `src/screens/Templates.tsx`, `workspace/Overrides.tsx`):**
  - Plain folders in the app data dir: `templates/<id>/template.json` (name, description, version, sha1 per file) + `templates/<id>/files/…` (laid out as in `overrides/`). Every scan re-hashes the files and bumps the version on any change, so edits made with your own tools count too. Renaming/describing never bumps. Delete moves the folder to `templates/.trash/` (recoverable by hand).
  - All paths go through `templates::clean_rel` (relative, no `..`, no absolute/drive paths); the export re-checks them.
  - Import (`scan_source` → `import`) from a folder, a Prism instance (its `minecraft/` or `.minecraft/`, read-only) or a pack archive (`.mrpack` `overrides/` then `client-overrides/`, later wins; CF zip `overrides/`). Lists top-level entries to keep: config, defaultconfigs, kubejs, scripts, options.txt… pre-ticked; mods, saves, logs, jars etc. never offered. Caps: 20k files / 512 MB.
  - The Templates screen opens with nothing selected (fast first load); the open template is remembered for the session (module-level, survives tab switches) and "✕ Close" goes back to the list. Scans reuse each file's sha1 when its size and modified time match `template.json`'s `stamps` cache, and the list/overview commands run off the main thread.
  - Editor: text files ≤ 256 KB with a known extension open in `components/CodeEditor.tsx` (textarea over a highlighted `<pre>`, no dependencies; Cmd/Ctrl+S saves, Tab indents, Esc leaves). Others: "Open externally" (opener plugin, Rust side). New file / Add files (dialog) / Rename / Delete. The unsaved-changes guard (`src/unsaved.tsx`, shared with payloads) covers the open file and the name/description.
  - Packs: `overrideTemplates` is the ordered list, highest priority first (the top one wins clashes; the core applies them bottom-up); `appliedTemplates` stores `{id, version, files}` (hashes as of attach/review). The Overrides tab (List | Graph | Overrides) shows attached templates with ↑/↓, clashes (who overrides whom), every file, configs matched to mods by path (`config/jei/…` → JEI, by slug/name, plus a few aliases) and unmatched ones as "not in this pack" (still exported, never deleted), plus "changed v2 → v4 → Review" (added/changed/removed files → Mark reviewed / Detach). Exports always use the current files; the pre-flight warns about unreviewed changes and missing templates and says how many files go into `overrides/`.
  - Dev bridge keeps its library in `test-env/bridge-templates/` and imports only from inside `test-env/`.
- **Polish (M10):**
  - **Undo/redo** (`Workspace.update(fn, {label, history})`): every pack change through `update()` is a step (last 100), labelled for the buttons' tooltips ("Undo remove AppleSkin"). Several updates before one render collapse into one step. Graph positions (`history: false`) and snapshots are never undone; `history.restore` keeps current positions and snapshots. Automatic "pulled in by" syncing bypasses history. ⌘/Ctrl+Z, ⇧⌘Z / Ctrl+Y, ignored while typing or with a dialog open.
  - **Snapshots** (`src/history.ts`, `workspace/SnapshotsDialog.tsx`): `pack.snapshots` holds `{id, name, at, state}` where `state` is the whole pack minus snapshots. Take / rename / delete (not undo steps), compare any two (or one with now: settings, added/removed split into yours and dependencies, per-mod changes), roll back (an undo step, with an optional "before rolling back" snapshot, ticked by default). Kept in the planner file, never in exports; cleared by Duplicate to version.
  - **Keyboard:** `?` (or the header ? button) opens the shortcut sheet (`components/Shortcuts.tsx`); ⌘/Ctrl+1–4 switch top tabs (through the unsaved-changes guard); workspace adds Esc (clear selection), ⌘S (save now), ⇧⌘S (snapshots), ⌘E (export).
  - **Click sound** (Settings → Interface, off by default): synthesised with Web Audio in `src/sound.ts`, no sound file. `uiClick()` plays it when the setting is on: on `click` (so Enter/Space on a button sounds too), on tab arrow keys, and on every shortcut that does what a button does (⌘1–4, ?, G, ⌘Z/⇧⌘Z, ⌘E, ⇧⌘S, Delete). A second toggle, "Also when picking items in lists" (`clickSoundLists`, off), covers `.entry`/`.hit`/`.tpl-file` picks by mouse or Enter/Space. **Backdrop:** CSS-drawn dark tiles behind the main area (reads as a soft monochrome plaid; owner likes it), on by default, toggle in the same place. **Component kit** tab is a dev aid: hidden unless Settings → Interface → "Developer: show the Component kit tab" (`prefs.showKit`, off by default).
- **Other content types (M11):** resource packs, shaders, datapacks are `PackEntry.kind`; the kind comes from how it was added (Browse's content picker, a payload candidate's kind, the import folder, or CurseForge's class), not the project: Modrinth lists datapacks as type `mod` with a `datapack` loader and many ship both (Terralith), so the same project can be a mod or a datapack.
  - Files: `resolve::kind_loaders` (mods: the pack's loader; resource packs `minecraft`; shaders `iris`/`optifine`; datapacks `datapack`). `Catalog::versions_of(.., kind)` (default = mods only); `select_version_kind` returns `(version, exact)`. Resource packs and shaders may fall back to the newest build for another Minecraft version (`version_fallback` warning); datapacks never do. Stale checks skip non-mods. CurseForge: class ids 6 / 12 / 6552 / 6945 (`curseforge::class_for`; same as Prism's `g_classIDMappings`, and the owner's searches matched), non-mod files get the kind's tag as loaders.
  - `plan_add_kind`: the root is the kind, dependencies are mods; the first datapack brings Paxi (`resolve::PAXI`, Modrinth `CU0PAyzb`) and its deps. Checks: `shader_loader_missing` (error, fix adds Iris, or Oculus on Forge), `datapack_without_paxi` (warning, fix adds Paxi).
  - Export folders (`export::folder_for`): `resourcepacks/`, `shaderpacks/`, `config/paxi/datapacks/` (Paxi's global folder, confirmed from Paxi's Modrinth page; no load-order file written, Paxi makes one) or `datapacks/` without Paxi. CurseForge zips keep datapacks: the manifest has only ids and the launcher picks folders. Prism (`FlameCreationTask::validateOtherResources`) identifies each file and moves datapacks to `datapacks/`, which Paxi 5.1.3+ also loads ("Load from base 'datapacks' directory", on by default); the pre-flight says so. Imports map those folders back to kinds (mrpack paths, Prism instance folders incl. their `.index`, CF class).
  - **Datapack that's also a mod** (Terralith…): `datapack_has_mod` warning when the project has a mod build for the pack's loader and version, with `Fix::SetKind { key, to: mod }` ("Use the mod": clears the pin; Paxi then shows as unneeded if nothing else uses it).
  - Search: `SearchQuery.kind` → Modrinth `project_type:` facet / CurseForge `classId`; the loader filter (`mod_loader()`) and side filter apply to mods only; categories per kind (`categories_for`). List and graph put each kind in its own group (`@resourcepack` etc. in `grouping.ts`).
  - **packwiz** (`export::packwiz_files` / `write_packwiz`): mixed sources (each entry from its preferred site). `pack.toml` (pack-format packwiz:1.1.0, `[versions]` minecraft + loader), `index.toml` (sha256 of every file, `metafile = true` for `.pw.toml`), one `<slug>.pw.toml` per file in the folder it installs to (Modrinth: url + sha512 + `[update.modrinth]`; CurseForge: sha1 + `mode = "metadata:curseforge"` + `[update.curseforge]`), override templates at the root. Written into a new `<pack> packwiz` folder inside the folder you pick. **Markdown/CSV lists** (`list_markdown`, `list_csv`): name, type, version, site, page link, category, reason.
  - Live check (Nov 24): Fresh Animations, Complementary Reimagined, Terralith as datapack (+Paxi, YUNG's API, Cloth Config, Fabric API) and Iris on 1.21.1 Fabric: `.mrpack` exported, all 9 files downloaded and verified into the right folders, re-import kept the kinds; packwiz hashes chain (test).
- **Prism round-trip testing:** `scripts/test-prism.sh create|start|import FILE|launch NAME|status|logs NAME|--clean` drives the installed Prism with `--dir test-env/prism` (refuses real Prism paths). The owner signs in to the test Prism themselves. Fixture packs: `cd src-tauri && cargo run --example build_fixture_packs` → `tests/fixtures/packs/*.planner.json` + `test-env/exports/*.mrpack`; `cargo run --example verify_export -- <file.mrpack>` downloads every file into `test-env/downloads/` and checks size/sha1/sha512.
- **Dev bridge (dev only, never shipped):** `cd src-tauri && cargo run --example dev_bridge` serves the real core on `127.0.0.1:1421`; point the Vite page's `window.__TAURI_INTERNALS__.invoke` at it (snippet in `examples/dev_bridge.rs`) to exercise the UI against real Rust in a plain browser. Packs live in memory; no keychain (CurseForge is deliberately off: `disable_keychain`) and no dialogs.
- React state updaters must stay pure (StrictMode runs them twice): generate ids/timestamps before calling `update()`.

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
- Nov 24, 2026 (M6 review): env rule fixed (never optional); side notes removed from checks; keepable dropped dependencies in Duplicate to version; Live Scan added as M14. Cross-check of linked mods' deps against their other-site copy: approved as a per-pack toggle, **off by default** (saves CurseForge API calls).
- Nov 24, 2026 (M5 review): owner approved committing M5 and starting M6 after the log review.
- Nov 24, 2026 (M4 review): optional suggestions are multi-select everywhere (tick any → "Add selected (N)", none ticked → "Add all"); picks are planned one after another against the growing pack and applied in one update.
- Nov 24, 2026 (M3 review): owner approved release-first default plus a per-pack "newest build" toggle, and optional-dependency suggestions.
- Nov 24, 2026: it's a Tauri desktop app (the doc's "local web app" wording is stale). M1 is judged on macOS only; Mint later. setup.sh installs Rust via rustup (portable to other Macs). pnpm via corepack. git author AdaghastTheMad <355918907+AdaghastTheMad@users.noreply.github.com>. Data-model revisions above. Duplicate-to-version moved to M6. Starter payload names (Iris/Oculus Search, Roxy, Diorama, etc.) to be verified against live data at the payloads milestone.
- Pixel font: Pixelify Sans (OFL, bundled via `@fontsource/pixelify-sans`; no runtime font fetch). Monocraft isn't on npm and could replace it later.
