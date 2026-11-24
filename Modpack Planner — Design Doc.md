# Modpack Planner — Design Doc

Nov 23, 2026 · @AdaghastTheMad

## Overview

Modpack Planner is a local web app for designing Minecraft modpacks as metadata only: you pick mods, see how they depend on each other, and export a pack file a launcher can install. No mod jar is ever downloaded by the app.

**Goals**

- Plan packs for Minecraft **1.21.1** and **26.1.2** at launch, with any other version Modrinth or CurseForge lists selectable.
- Support NeoForge, Fabric, Forge and Quilt as the pack's loader; one loader per pack. Mods come first; resource packs, shader packs and datapacks are added the same way once the mod core is solid.
- Search Modrinth and CurseForge side by side, add mods in one click, and pull in required dependencies automatically.
- Show the pack as a dependency graph and as a categorized list, with problems (missing deps, incompatibilities, no build for this version) flagged inline.
- Save reusable **payloads** (bundles of mods) and drop them into any pack.
- Export to formats Prism, the Modrinth App and the CurseForge app can install, so the launcher does the downloading.
- Look and feel like Minecraft's Java UI, darkened, without giving up ease of use.

**Non-goals for v1**

- Downloading, launching or testing the pack. The launcher handles that.
- Editing configs, KubeJS scripts or datapacks (the export can carry an `overrides/` folder you fill in later).
- Accounts, cloud sync or sharing beyond exporting a file.
- Detecting runtime conflicts that metadata can't express (mixin clashes, worldgen overlap). The app can show community-flagged incompatibilities only.

## Platform and architecture

Modpack Planner is a standalone desktop app for Linux and macOS, built with Tauri: a Rust core with the web UI in a native window, installed and launched like any other program.

The Rust core makes every API call. That means no browser CORS limits, the CurseForge key never reaches the UI, and Modrinth gets a proper User-Agent. The key is stored in the OS keychain (Secret Service on Linux, Keychain on macOS).

&#91;embedded content: architecture · desktop app, Rust core, two APIs, Prism\]

Tauri rather than Electron: it uses the system webview, so the app is a small download, and the Rust side suits the resolver, caching and zip writing. Without a key, the app runs in Modrinth-only mode.

**Stack**

- Tauri 2 shell; Rust core for HTTP (reqwest), caching (SQLite via rusqlite), the resolver and export writers.
- UI: Vite + React + TypeScript, Cytoscape.js for the graph.
- UI talks to the core only through typed Tauri commands; no network access from the webview.
- Builds: `.deb` and `.AppImage` on Linux (the `.deb` adds a menu entry), `.dmg` on macOS.
- No telemetry, no accounts, no outside calls except Modrinth, CurseForge and loader version metadata.

**Setup scripts**

- `scripts/setup.sh` detects the OS, checks each dependency, prints what's missing, and asks before installing anything.
  - Linux (apt, Linux Mint/Ubuntu): the Tauri system libraries (WebKitGTK 4.1, build tools, OpenSSL, librsvg and the rest of Tauri's listed prerequisites, checked against Tauri's docs at build time), Rust via rustup, Node LTS and pnpm.
  - macOS (Homebrew): Xcode Command Line Tools check, Rust via rustup, Node LTS and pnpm.
- `scripts/build.sh` produces the installers above; `scripts/dev.sh` runs the app with hot reload.
- Scripts are re-runnable: a second run with everything present just reports "all good".

## Data sources

Both APIs are called from the Rust core; Modrinth needs no key, CurseForge needs yours.

|  | Modrinth | CurseForge |
| --- | --- | --- |
| Auth | None for reads | `x-api-key` header, your key |
| Browser access | CORS allowed (`Access-Control-Allow-Origin: *`) ([docs](https://docs.modrinth.com/api/)) | Not needed: calls come from the Rust core |
| Rate limit | 300 requests/min per IP | Not published; cache aggressively |
| Search | `/v2/search` with facets for version, loader, category | `/v1/mods/search` with `gameVersion`, `modLoaderType`, `categoryId` |
| Dependencies | Per version: `required`, `optional`, `incompatible`, `embedded` | Per file: `relationType` 1 embedded, 2 optional, 3 required, 4 tool, 5 incompatible, 6 include |
| Categories | `/v2/tag/category` | `/v1/categories?gameId=432` |
| Bulk lookup | `/v2/projects?ids=[…]`, `/v2/versions?ids=[…]` | `POST /v1/mods`, `POST /v1/mods/files` |

**Notes for implementation**

- Modrinth asks for a unique User-Agent; the core sends one like modpack-planner/\<version> (contact) on every call.
- Dependencies live on a specific **version/file**, not the project. The resolver must first pick the right file for (Minecraft version, loader), then read its dependency list.
- Some dependencies are declared only by project with no version pinned. Resolve those to the newest compatible file.
- CurseForge mods can disable third-party distribution (`allowModDistribution: false`). Flag these: Prism and similar launchers will ask you to download them by hand.
- **Cross-platform matching.** The same mod often exists on both sites. Match on slug and name first, then let the user confirm or link manually. Store both IDs on the pack entry so the user can switch source per mod without losing its place in the graph.
- Cache every API response in the core's SQLite cache with a timestamp (24 h default TTL, manual refresh button). This keeps the rate limits a non-issue and makes reopening a pack instant.

## Core features

The main screen is one pack workspace with three panes: browse on the left, the pack in the middle (list or graph), details on the right.

**Pack setup**

- New pack wizard: name, Minecraft version, loader, loader version (fetched from the loader's own metadata), optional icon.
- **Duplicate to another version.** Copy a pack from 1.21.1 to 26.1.2 and get a report: which mods have a build there, which don't, and which changed their dependencies. This is the feature most likely to save you hours, since you work in both versions.

**Browse and search**

- One search box, results merged from both sources with a small Modrinth/CurseForge badge. Filter by category, client/server side, and sort by downloads, follows or last updated. Tabs above the results switch content type: Mods, Resource packs, Shaders, Datapacks (the last three arrive in a later milestone).
- Results are pre-filtered to the pack's version and loader; a toggle shows incompatible results greyed out instead of hidden.
- Hover a result for a tooltip card (summary, downloads, last update, license). Click for the full page in the details pane, including the description rendered from the API.

**Adding and resolving**

- Add a mod → its required dependencies are added automatically and tagged "pulled in by X". Optional dependencies appear as suggestions you can accept.
- Remove a mod → any dependency nothing else needs is offered for removal too (orphan cleanup).
- Library mods (Architectury, Cloth Config, GeckoLib and similar) are recognised by category and shown dimmed, so the list stays focused on the content mods you picked.

**Checks, run continuously**

- Missing required dependency.
- Declared incompatibility between two mods in the pack.
- No file for this Minecraft version or loader.
- Mod is client-only or server-only (useful when the pack will also run on a server).
- CurseForge file blocks third-party distribution.
- Mod not updated in over a year (soft warning).

**Organizing**

- Group by: Modrinth/CurseForge category, your own custom tags, or "pulled in by".
- Custom tags and per-mod notes ("replace with X if it doesn't port to 26.1").
- Pin a specific file version, or let it float to the newest compatible build.
- Undo/redo on every change, plus named snapshots of the whole pack you can compare and roll back to.

## Graph view

The graph is a force-directed web where each mod is a node and each dependency is an arrow from the mod to what it needs.

- **Node look:** the mod's icon in an inventory-slot frame, name underneath. Size scales with how many mods depend on it, so big libraries stand out.
- **Edge styles:** solid for required, dashed for optional, red for incompatible. Embedded/included deps hidden by default.
- **Colour:** node border colour follows the selected grouping (category or custom tag). Red outline for any node with a failing check.
- **Clustering:** nodes in the same category pull together into soft labelled regions, so the graph reads as "tech cluster", "magic cluster" and so on.
- **Interaction:** click a node to select it and highlight its full dependency chain both ways; everything else fades. Double-click opens details. Drag to pin a node in place. Right-click for remove, pin version, tag.
- **Filters:** hide libraries, show only one category, or show only nodes with problems.
- **Drop to add:** drag a search result onto the canvas to add it; drag a payload in to add the whole bundle.
- Positions are saved with the pack so the layout doesn't reshuffle every time.

Suggested library: Cytoscape.js (good layouts, compound nodes for category regions, handles a few hundred nodes easily). A 300-mod pack is the realistic upper bound to test against.

## Payloads

A payload is a saved, version-agnostic list of mods that you add to a pack in one step; the app resolves each entry to the right file for that pack's version and loader.

**How they work**

- A payload stores project IDs (Modrinth and/or CurseForge), not files, so one "Performance" payload works for 1.21.1 and 26.1.2 alike.
- On drop-in, a preview shows what will be added, what's already in the pack, and what has no build for this version. You confirm, untick entries, then apply.
- **Choice slots:** an entry can be "one of" a set, such as JEI *or* EMI, or Xaero's *or* JourneyMap. Each pack remembers which you picked, and the app never adds both.
- **Loader variants:** an entry can name a different mod per loader (Sodium on Fabric, with a NeoForge alternative if that loader needs one).
- Payloads can nest ("Survival baseline" = Performance + QoL + Maps).
- Editing a payload doesn't silently change packs that used it. Packs show "payload updated, 2 new mods" and let you pull the change in.
- Import/export payloads as small JSON files so you can keep them in git or share them.

**Suggested starter payloads** (to be checked against live data at build time; anything without a 26.1.2 build gets flagged, not dropped)

| Payload | Contents |
| --- | --- |
| Performance core | Sodium, Lithium, FerriteCore, ModernFix, ImmediatelyFast, Particle Core, Crash Assistant, Packed Packs, C2ME, Extreme Sounds Muffler, Cubes Without Borders |
| Shaders | Iris (requires Sodium), Iris/Oculus Search |
| Recipe viewer | Choice slot: JEI, EMI (Standalone), EMI + JEI, or EMI + TooManyRecipeViewers (A JEI Emulation Layer). All options include Just Enough Effect Descriptions and Moderately Enough Effect Descriptions, EMI-Inclusive packs include EMIffect, EMI Ores, EMI Loot, EMI Enchanting, and EMI Extra Mod Integrations |
| QoL and HUD | Jade, AppleSkin, Mouse Tweaks, Controlling, Keyboard Atlas |
| Maps | Choice slot: Xaero's Minimap + World Map + XaeroPlus, JourneyMap, or Diorama (New and very useful isometric world map/minimap mod) |
| Distant terrain | Distant Horizons, Voxy, Roxy (Voxy Compat Mod that live-ports it to NeoForge 1.21.1) |
| Server-friendly | Spark (profiling), and a flag that marks client-only mods for removal on server export |
| Aesthetic Mods | Subtle Effects, Visuality Reforged, Particular Reforged, Wakes/Wavify, Windy: Configurable, Sound Physics Remastered, Sounds (+ Extra Sound on versions with it), Immersive UI, Smooth GUI, Drip Sounds, The Sound of Rain, Presence Footsteps ("Tread Lightly" on 26.1.2), AmbientSounds 6 |
| Animation Mods\
(Pending RP Support) | Punchy!, Fresh Animations (+ Extensions + Fresh Compat), a choice between Punchy! Moves (+Fluid and Foolish RP) or Fresh Animations: Player Extension + Just Expressions |

Genre payloads are left to you; the payload editor should make building them from old packs quick (import a pack, select mods, "Save as payload").

## Import and export

Every export is metadata only: launchers download the mods when they install the pack, so the app never touches a jar.

| Format | Sources it can hold | Installs in | Notes |
| --- | --- | --- | --- |
| `.mrpack` ([spec](https://docs.modrinth.com/modpacks/format)) | Modrinth only, in practice | Prism, Modrinth App, ATLauncher | Needs sha1 + sha512 and a download URL per file, all of which the Modrinth API supplies. CurseForge URLs aren't on the format's allowed host list, so CF-only mods are listed as a warning. |
| CurseForge zip (`manifest.json`) | CurseForge only | CurseForge app, Prism | Only needs project ID + file ID per mod. Modrinth-only mods are listed as a warning. |
| packwiz folder | Both, mixed | packwiz-installer, git-based workflows | Best fit for a mixed-source pack; one small `.pw.toml` per mod. |
| Mod list (Markdown / CSV) | Both | Reading, sharing, a pack page | Name, source, version, link, category. |
| Planner project (`.json`) | Everything | This app | Full save: graph layout, tags, notes, snapshots. |

- **Prism Launcher is the primary target.** It imports both `.mrpack` and CurseForge zips, so those two exporters come first and packwiz moves later. Every exporter is tested by importing its output into an isolated test copy of Prism (see Build milestones).
- Before export, a pre-flight panel lists every warning in one place and offers fixes ("switch these 4 mods to their Modrinth copies").
- **Source preference** per pack: "Prefer Modrinth", "Prefer CurseForge" or "Mixed". This decides which ID is used when a mod exists on both, and so which export formats come out clean. Default: Prefer Modrinth, since `.mrpack` is the cleanest import into Prism.
- **Other content types** in exports: resource packs go to `resourcepacks/`, shader packs to `shaderpacks/`.
- **Datapacks load globally through Paxi.** Adding any datapack to a pack also adds Paxi, and the export places datapacks in Paxi's datapack folder under `config/` (exact path and load-order file confirmed against Paxi's docs at build time). If Paxi has no build for the pack's version or loader, the check list flags it and offers a separate datapack folder instead.
- **Server variant** (nice-to-have, late milestone): a second export with client-only mods, shaders and resource packs removed, plus a server-side `.mrpack` or plain mod list.
- **Import** reads `.mrpack`, CurseForge zips and packwiz folders, so existing packs can be loaded, graphed and edited.
- **Import a Prism instance directly.** Prism keeps packwiz-style metadata for mods it installed in `mods/.index/`, so the app can read an instance without opening jars. Jars with no metadata fall back to a hash lookup against Modrinth and CurseForge. This only reads your instance; it never changes it. Imported `config/` and `kubejs/` folders can be saved as override templates (below).

## Override templates

Override templates are your own reusable `config/`, `kubejs/` and similar folders, kept in an editable library and attached to any pack; the export copies them into the pack's `overrides/` folder.

- **Import a template** from any folder on disk, a Prism instance, or an existing `.mrpack`/CurseForge zip. You name it and pick which subfolders to keep (`config/`, `defaultconfigs/`, `kubejs/`, `options.txt` and so on).
- **Library view:** a list of templates with name, folders included, file count and the packs that use it. Rename, duplicate, delete, or open the template's folder in your file manager to edit files with your own tools.
- **Light in-app editing:** add or remove files, and edit small text files (`.toml`, `.json`, `.js`, `.cfg`) in a plain text editor with syntax highlighting. Anything bigger opens in your own editor.
- **Attach to a pack** from the pack's Overrides tab, as an ordered list. When two templates contain the same file, the one lower in the list wins, and the app shows which files clash.
- **Mod awareness:** a template's config files are matched to mod IDs where the path makes it obvious (`config/jei/` → JEI). Files for mods that aren't in the pack are flagged as unused, never deleted.
- **Versioning:** editing a template bumps its version; packs that use it show "template updated" and let you pull the change in, the same way payloads do.
- Templates are stored as plain folders in the app's data directory, so they can be backed up or kept in git.

## Visual design

The look borrows Minecraft Java's menus (bevelled stone buttons, inventory slots, tooltip boxes, pixel font) on a dark palette, but keeps modern spacing, readable body text and keyboard control.

**Borrow from Minecraft**

- **Buttons:** the bevelled grey button shape (light top-left edge, dark bottom-right), dark variant. Hover gets the white outline the game uses; disabled buttons go flat and dim.
- **Panels:** dark recessed panels like the inventory background, with the same 2 px inner bevel.
- **Slots:** mod icons sit in 18 px-style slots (scaled up), as in an inventory grid.
- **Tooltips:** the game's tooltip box exactly: near-black fill, purple gradient border, name in white, secondary lines in grey, source badge in blue/orange.
- **Headings and buttons in a pixel font.** Minecraft's own font is Mojang's, so use an openly licensed lookalike such as Monocraft or a similar OFL pixel font.
- **Pixel-crisp rendering:** `image-rendering: pixelated` for icons and UI textures; borders drawn in CSS rather than copied game textures.
- Small touches: the click sound on buttons (off by default, toggle in settings), a dirt- or deepslate-tile backdrop behind the main panels.

**Keep modern**

- Body text (descriptions, notes, long lists) in a clean sans-serif. A whole app in pixel font tires the eyes quickly.
- Spacing on an 8 px grid; nothing cramped like the in-game mod list.
- Clear contrast: aim for WCAG AA on all text.
- Full keyboard use: `/` focuses search, `Enter` adds the highlighted result, `Del` removes the selected mod, `G` toggles graph/list, `Ctrl+Z` undo.
- Drag-and-drop everywhere it makes sense, with the same action always available from a right-click menu.
- Empty states that tell you what to do next ("Drop a payload here, or search on the left").

**Colour tokens (starting point)**

| Token | Use |
| --- | --- |
| Deepslate dark | App background |
| Stone grey | Buttons, panel bevels |
| Inventory dark | Panel fill |
| Tooltip purple | Tooltip border, selection |
| Emerald green | Success, "all checks pass" |
| Redstone red | Errors, incompatible edges |
| Gold | Warnings |
| Diamond cyan | Links, focus ring |

Original assets only: no copied Mojang textures or font files, so the app stays shareable.

## Data model

Five stored types cover everything: Pack, PackEntry, Payload, OverrideTemplate and a cache of API records.

```ts
type Source = 'modrinth' | 'curseforge';
type Loader = 'neoforge' | 'fabric' | 'forge' | 'quilt';

interface ModRef {            // one mod, possibly on both sites
  modrinthId?: string;        // project id
  curseforgeId?: number;      // mod id
  preferred: Source;
}

interface PackEntry {
  ref: ModRef;
  fileId?: string;            // pinned version/file; absent = float to newest compatible
  kind: 'mod' | 'resourcepack' | 'shader' | 'datapack';
  reason: 'user' | 'dependency' | 'payload';
  pulledInBy?: string[];      // entry keys that require this one
  payloadId?: string;
  tags: string[];
  notes?: string;
  side?: 'client' | 'server' | 'both';
  graphPos?: { x: number; y: number; pinned: boolean };
}

interface Pack {
  id: string;
  name: string;
  mcVersion: string;          // '1.21.1', '26.1.2', ...
  loader: Loader;
  loaderVersion: string;
  sourcePreference: Source | 'mixed';
  entries: Record<string, PackEntry>;
  choices: Record<string, string>;   // choice-slot id -> picked ModRef key
  overrideTemplates: string[];        // attached template ids, later ones win on clashes
  snapshots: { name: string; at: string; entries: Record<string, PackEntry> }[];
  createdAt: string; updatedAt: string;
}

interface OverrideTemplate {
  id: string; name: string;
  folder: string;             // path inside the app's data directory
  files: string[];            // relative paths, e.g. config/jei/jei-client.toml
  version: number;            // bumps on edit, so packs can see updates
}

interface PayloadItem {
  kind: 'mod' | 'choice';
  refs: ModRef[];             // one for 'mod', several for 'choice'
  perLoader?: Partial<Record<Loader, ModRef>>;
}

interface Payload {
  id: string; name: string; description?: string;
  items: PayloadItem[];
  includes: string[];         // nested payload ids
  version: number;            // bumps on edit, so packs can see updates
}
```

Storage: SQLite in the app's data folder, owned by the Rust core, one table each for packs, payloads and the API cache (keyed by request URL, with fetch time). Packs and payloads also save to and load from plain JSON files.

## Build milestones

Each milestone ends with something usable, so Claude Code can be stopped and reviewed between them.

1. **Skeleton and theme.** `scripts/setup.sh`, Tauri 2 + Vite + React + TypeScript project, the Minecraft-dark component kit (button, panel, slot, tooltip, input, tabs), settings screen with the CurseForge key saved to the OS keychain. Done when the app opens on Linux Mint and macOS and a test CurseForge search returns from the core.
2. **Modrinth search and pack list.** New-pack wizard with an open version picker, Modrinth search with version/loader/category filters, add/remove mods, list view grouped by category, SQLite cache, save/load project JSON.
3. **Dependency resolution and checks.** File selection per version and loader, auto-add required deps, optional-dep suggestions, orphan cleanup, the check list with inline warnings.
4. **CurseForge.** Search through the core, merged results, cross-platform matching, source preference per pack.
5. **Export to Prism.** `.mrpack` and CurseForge zip exporters, pre-flight warnings panel, importers for both, plus Prism instance import. Done when a 1.21.1 NeoForge pack and a 26.1.2 Fabric pack both import into the isolated test Prism and launch with no missing mods.
6. **Graph view.** Cytoscape graph with category regions, edge styles, chain highlighting, filters, saved layout, drag-to-add.
7. **Payloads.** Payload editor, choice slots, loader variants, nesting, drop-in preview, starter payloads, payload update notices, "Save selection as payload".
8. **Override templates.** Template library, import from folder/instance/pack, attach to packs with clash display, light text editor, template versioning, export into `overrides/`.
9. **Version migration and polish.** Duplicate-to-version report, snapshots and compare, keyboard shortcuts, undo/redo, optional click sound, empty states.
10. **Other content types.** Resource packs, shader packs and datapacks (with Paxi added automatically) in search, list, graph and export. packwiz exporter and Markdown/CSV mod lists.
11. **Installers.** `scripts/build.sh` producing `.deb`, `.AppImage` and `.dmg`.
12. **Server variant (nice-to-have).** Server-side export with client-only content stripped.

**Instructions for Claude Code**

- Write a `CLAUDE.md` first that summarises this doc and the milestone list, and keep it updated.
- **Mods first, and reliable.** Milestones 1 to 5 are the core. Don't start milestone 10 until the resolver tests pass and the Prism round-trip in milestone 5 works for both versions. Other content types may reuse the core, but never change its behaviour for mods.
- **Test only in an isolated Prism Launcher.** Approved: set up a dedicated Prism data directory inside the project (for example `test-env/prism/`, git-ignored), using Prism's portable build or its `--dir` launch option, plus a `scripts/test-prism.sh` that creates, starts and wipes it. Never read, write or launch the user's own Prism data folder (`~/.local/share/PrismLauncher`, the Flatpak equivalent, or `~/Library/Application Support/PrismLauncher`). Instance import is tested against fixture instances built in this test copy, not the user's instances.
- Launching Minecraft needs a signed-in account. The first time, pause and ask the user to sign in to the test Prism themselves; never handle account credentials. Until then, the round-trip test stops at "pack imports and every file downloads".
- Test downloads land only inside `test-env/`; `scripts/test-prism.sh --clean` deletes it to reclaim the space.
- Wrap each API in its own client module in the Rust core behind one `ModSource` trait, so the UI never cares which site a mod came from.
- Test the resolver against recorded API responses, not the live APIs: required, optional, incompatible and embedded deps; deps with no pinned version; a mod with no build for the pack's version; circular deps.
- Use built fixture packs: one NeoForge 1.21.1 pack around 150 mods, one Fabric 26.1.2 pack around 50.
- Setup and build scripts must check before installing, ask before using `sudo`, and work on Linux Mint (apt) and macOS (Homebrew). No Windows targets.
- No telemetry, analytics or third-party calls beyond the two mod APIs and loader version metadata.

## Open questions

**Decided** (Nov 23)

- Standalone desktop app (Tauri), with setup scripts for apt and Homebrew.
- Prism Launcher is the primary install target; `.mrpack` and CurseForge zip first, default source preference Modrinth.
- Resource packs, shaders and datapacks are in scope, after the mod core is proven.
- Prism instance import approved (read-only).
- Testing uses an isolated Prism inside the project, never the user's own.
- Starter payloads: your edited list in Payloads; no genre payloads in the app.
- Datapacks load globally through Paxi.
- Server variant export is a nice-to-have, last milestone.
- Config and KubeJS bundles become override templates with an editable library.
- Open version picker.

**Still open**

Nothing at the moment; new questions from the build go here.
