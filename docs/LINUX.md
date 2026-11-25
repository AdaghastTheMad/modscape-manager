# Building Modscape Manager on Linux

For **Linux Mint 21 / 22** and **Ubuntu 22.04 / 24.04** (anything apt-based with WebKitGTK 4.1). The result is a `.deb` you install like any other app, plus a portable `.AppImage`.

> [!NOTE]
> Linux installers have to be built on Linux; a Mac can't make them. Building takes about 10 minutes the first time (Rust compiles everything once) and 2–3 minutes after that.

## The short version

```bash
git clone https://github.com/AdaghastTheMad/modscape-manager.git
cd modscape-manager
scripts/linux-build.sh
```

The script checks everything first and **asks before it installs anything or uses sudo**. It:

1. Offers to install Node.js for your user (with nvm) if it's missing.
2. Runs `scripts/setup.sh` for the system libraries (one `sudo apt-get install`), Rust (rustup, user-local) and pnpm.
3. Runs the tests, builds, and puts the installers in `installers/`.
4. Offers to install the `.deb`.

If you'd rather do each step yourself, read on.

## Getting the code onto the Linux machine

- **From GitHub:** `git clone` as above (or download the ZIP from the repository page and unpack it).
- **From another computer (USB stick, network share):** copy the project folder **without** these, which are big and rebuilt anyway:
  `node_modules/`, `src-tauri/target/`, `dist/`, `installers/`, `test-env/`.
  Then make the scripts executable again if the copy lost that: `chmod +x scripts/*.sh`.

## Step by step

### 1. Node.js 20 or newer

Check with `node --version`. Mint's own `nodejs` package is too old, so if you don't have Node yet, install it for your user with nvm (no sudo):

```bash
curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
```

Open a new terminal, then:

```bash
nvm install --lts
```

### 2. Everything else

```bash
scripts/setup.sh
```

It lists what's there and what's missing, then asks before each install:

| What | How | Needs sudo |
|---|---|:-:|
| WebKitGTK 4.1, build tools, OpenSSL, AppIndicator, librsvg, D-Bus, gnome-keyring | one `apt-get install` | yes |
| Rust (stable) | the official rustup installer, into `~/.cargo` | no |
| pnpm | `corepack enable pnpm` | no (with nvm's Node) |
| JavaScript packages | `pnpm install` in the project | no |

Run it again until it says **All good**. After installing Rust, open a new terminal (or run `. ~/.cargo/env`) so `cargo` is found.

### 3. Build

```bash
scripts/build.sh            # tests first, then the .deb and .AppImage
scripts/build.sh --skip-tests
```

The first AppImage build has Tauri download its `linuxdeploy` tools into `~/.cache/tauri`. The script asks first; answering *no* builds just the `.deb`.

### 4. Install

```bash
sudo apt install ./installers/Modpack\ Planner_0.1.0_amd64.deb
```

Modscape Manager then appears in the applications menu under **Accessories / Utility**. To remove it: `sudo apt remove modscape-manager`.

Or skip installing and run the AppImage:

```bash
chmod +x installers/*.AppImage
./installers/Modpack\ Planner_0.1.0_amd64.AppImage
```

## Good to know

- **Your data** lives in `~/.local/share/io.github.adaghastthemad.modscapemanager/` (packs, payloads, override templates, cache). **Settings → Danger zone** resets any part of it.
- **CurseForge key:** saved in the Secret Service keyring (`gnome-keyring`, which Mint's Cinnamon, MATE and Xfce desktops run). If no keyring is running, the app tells you and works with Modrinth only; it never writes the key to a file.
- **Prism Launcher** (Flatpak or native) imports the exported `.mrpack` / CurseForge zip as usual: *Add Instance → Import*.

## Troubleshooting

| Problem | Fix |
|---|---|
| `Package libwebkit2gtk-4.1-dev has no installation candidate` | The distro is too old (Mint 20 / Ubuntu 20.04). Use Mint 21+ or Ubuntu 22.04+. |
| The AppImage won't start: *"AppImages require FUSE to run"* | `sudo apt install libfuse2` (Mint 21 / Ubuntu 22.04) or `sudo apt install libfuse2t64` (Mint 22 / Ubuntu 24.04). Or use the `.deb`. |
| `failed to bundle project: … linuxdeploy` | Re-run `scripts/build.sh`; if it persists, delete `~/.cache/tauri` and try again. The script already sets `APPIMAGE_EXTRACT_AND_RUN=1` and `NO_STRIP=true`, which fix the usual causes. |
| `cargo: command not found` after setup | Open a new terminal, or run `. ~/.cargo/env`. |
| `pnpm: command not found` | `corepack enable pnpm` (with nvm's Node it needs no sudo). |
| Blank window on start (some NVIDIA setups) | Start it once with `WEBKIT_DISABLE_DMABUF_RENDERER=1 modscape-manager`; if that helps, add the variable to the menu entry. |
