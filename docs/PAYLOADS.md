# Publishing starter payloads

The starter payloads every install begins with live in one file: [`src-tauri/data/starter_payloads.json`](../src-tauri/data/starter_payloads.json). Changing them is three steps: **design it in the app, export it, run one script**, then commit and push.

When people install a build that has your changes:

- **New starters** appear on their next start.
- **Starters they haven't edited** are updated to yours. It's a new payload version, so packs that used it show *updated → Review*.
- **Starters they edited** or deleted are left alone. *Restore starter payloads* brings deleted ones back.

## One-time setup

You need a copy of the repository on your computer, with git signing your commits as you.

```bash
git clone https://github.com/AdaghastTheMad/modscape-manager.git
cd modscape-manager
git config user.name "AdaghastTheMad"
git config user.email "355918907+AdaghastTheMad@users.noreply.github.com"
```

The two `git config` lines apply to this repository only, and keep your real email out of the history. (The copy on the Mac you built this on already has them.)

To try changes in the app, run it from source with `scripts/dev.sh`. The first time, run `scripts/setup.sh` first.

## Adding or changing a starter

1. **Make it in the app.** Open the **Payloads** tab and build or edit the payload: slots, fallbacks, loader and version limits, *one of* choices, exact-file pins. Save it.
2. **Export it.** In the payload editor, press **Export…** and save the `.payload.json` file anywhere (the Desktop is fine).
3. **Add it to the starters:**

   ```bash
   python3 scripts/starters.py add ~/Desktop/My\ Payload.payload.json
   ```

   - A starter with the **same name** (or `--id`) is **replaced** and keeps its id, so installs get it as an update.
   - Otherwise it's **added** with the id `starter-<name>`. Choose one with `--id starter-building-tools`.
   - The script checks the file before saving and says what to fix if something's off. For example, a payload can only *include* other payloads that are starters themselves.
4. **Look at the change** (optional, but handy):

   ```bash
   git diff src-tauri/data/starter_payloads.json
   ```

5. **Run the payload tests** (optional; about a minute the first time):

   ```bash
   cd src-tauri && cargo test payload && cd ..
   ```

6. **Commit and push:**

   ```bash
   git add src-tauri/data/starter_payloads.json
   git commit -m "Starter payloads: add Building Tools"
   git push
   ```

Done. It's on GitHub, and the next build anyone makes includes it.

### Other commands

```bash
python3 scripts/starters.py list                      # every starter, its id and version
python3 scripts/starters.py remove starter-maps       # drop one (also removes it from other starters' includes)
python3 scripts/starters.py check                     # validate the file after editing it by hand
```

## Editing the JSON by hand

You can also edit `starter_payloads.json` directly, including in the browser on GitHub (open the file, press the pencil, *Commit changes*). Each starter looks like this:

```json
{
  "id": "starter-distant",
  "name": "Distant Terrain",
  "description": "Far-away terrain as level-of-detail chunks.",
  "items": [ … ],
  "includes": [],
  "version": 1,
  "starter": true
}
```

An item is either a **mod slot**:

```json
{ "kind": "mod", "id": "qol-inventory", "label": "Inventory Essentials (Inventory Profiles Next as fallback)",
  "candidates": [
    { "name": "Inventory Essentials", "modrinthId": "Boon8xwi" },
    { "name": "Inventory Profiles Next", "modrinthId": "O7RBXm3n" }
  ] }
```

or a **choice** (`options`, each a list of slots, plus `common` slots that come with every option). Candidates are tried in order; the first that fits the pack and has a build wins. Optional fields on a candidate:

| Field | Meaning |
|---|---|
| `curseforgeId` | The CurseForge project id, for packs that prefer CurseForge (number). |
| `loaders` | Only on these loaders: `["neoforge"]`, `["fabric", "quilt"]`… |
| `mcVersions` | Only on these Minecraft versions: `["1.21.1"]`; `"26.*"` matches a prefix. |
| `kind` | `"resourcepack"`, `"shader"` or `"datapack"` (default: a mod). |
| `pinVersion`, `pinNumber` | Always this exact file (Modrinth version id) and its version number. |
| `forcePin` | Use the pinned file even if it isn't built for the pack's version/loader. Its own dependencies are skipped: the mod that needs it provides them (Roxy's exact Voxy file is the example). |

Run `python3 scripts/starters.py check` after editing by hand.

> [!TIP]
> Modrinth project ids are on each project's page (the *⋯* menu → *Copy ID*), or in the app: select a mod in a pack and look under *Sites*.

## Shipping a new installer (optional)

Starter changes reach people when they build or download a new version. To publish an installer on GitHub:

```bash
scripts/build.sh
gh release create v0.1.1 installers/*.dmg --title "Modscape Manager 0.1.1" --notes "New starter payloads."
```

Bump `"version"` in `src-tauri/tauri.conf.json` (and `package.json`, `src-tauri/Cargo.toml`) first, so the installer gets the new number. Attach the Linux `.deb` / `.AppImage` from your Linux machine to the same release with `gh release upload v0.1.1 installers/*.deb installers/*.AppImage`.
