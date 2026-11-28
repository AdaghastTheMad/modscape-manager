# Modscape Manager on Windows

Windows 10 or 11, 64-bit (x64).

## Install

1. Download `Modscape Manager_<version>_x64-setup.exe` from the repository's **Releases** page (or from the latest *Windows build* run under **Actions** → *Artifacts*).
2. Run it. It installs for your user account only, so it doesn't need administrator rights.
3. The installer isn't code-signed, so Windows SmartScreen may say *Windows protected your PC*. Press **More info → Run anyway**.
4. Open **Modscape Manager** from the Start menu.

The app needs Microsoft Edge WebView2, which Windows 10 and 11 normally already have. If it's missing, the installer downloads it from Microsoft.

To uninstall: **Settings → Apps → Installed apps → Modscape Manager → Uninstall**.

## Where things live

| What | Where |
|---|---|
| Packs, payloads, cache (`planner.db`) | `%APPDATA%\io.github.adaghastthemad.modscapemanager\` |
| CurseForge API key | Windows Credential Manager (*Windows Credentials*, entry `modscape-manager`). Never in a file. |

Prism Launcher's instances are usually in `%APPDATA%\PrismLauncher\instances\`. The app only reads an instance when you pick its folder (Import, or Live Scan → *Choose an instance…*).

Shortcuts use **Ctrl** (Ctrl+Z undo, Ctrl+Y or Ctrl+Shift+Z redo, Ctrl+1–4 switch tabs). Backspace or Delete removes the selected mod.

## Building it

Windows installers are built by GitHub Actions on a Windows runner: [`.github/workflows/windows.yml`](../.github/workflows/windows.yml). It runs the typecheck and the Rust tests, then `pnpm tauri build --bundles nsis`.

- **Every push to `master`** builds an installer. Download it from the run's *Artifacts* section.
- **By hand:** Actions → *Windows build* → *Run workflow*.
- **A release:** push a tag (`git tag v0.1.0 && git push origin v0.1.0`). The installer is attached to that tag's GitHub release.

To build on a Windows PC yourself, install [Rust](https://rustup.rs) (with the MSVC build tools it offers), [Node.js 20+](https://nodejs.org) and pnpm (`corepack enable`), then in the project folder:

```powershell
pnpm install --frozen-lockfile
pnpm tauri build --bundles nsis
```

The installer is in `src-tauri\target\release\bundle\nsis\`.
