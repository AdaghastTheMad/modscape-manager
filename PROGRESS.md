# Progress

What's done, what's broken, and what's next, in plain language. Claude Code updates this at the end of every work session, and a weekly check-in reads it. Newest log entry first. The owner is named only as AdaghastTheMad.

## Current state

- **Version:** 0.1.0. Public on GitHub, MIT.
- **Done:** all 14 milestones in CLAUDE.md, from the skeleton through Live Scan. Tested on macOS and built successfully as a .deb on Linux.
- **Windows:** an installer exists now (`Modscape Manager_0.1.0_x64-setup.exe`), built by GitHub on a Windows machine with all tests passing. It lives on the `windows` branch, not `master` yet. Not tried on a real Windows PC yet; a friend is about to test it.
- **Not Done yet:** the .AppImage hasn't been tested on Linux. The Windows build hasn't been run by a person yet. The app can't search the mods inside a pack. New request from private reviews: an "orphan config cleaner" that compares a pack's config folder with its installed mod jars and clears out config files and folders that don't belong to any installed mod.
- **Next Step:** read the README as a stranger would and list every place you'd get stuck or stop reading. That list is the start of the traction plan. Meanwhile, send the Windows installer to your friend for testing.

## Log

### 2026-11-28
- Windows build, on its own `windows` branch: the CurseForge key goes in the Windows Credential Manager, the app has a Windows icon, and an installer that needs no admin rights. GitHub builds it on a Windows machine on every push to that branch (tests first). Guide: docs/WINDOWS.md.
- Untested: nobody has opened it on a real Windows PC yet. The installer isn't signed, so Windows SmartScreen will warn (More info → Run anyway).
- Next: send the installer to your friend. If it works, merge `windows` into `master` so every platform builds from one place.

### 2026-11-27
- Added this file, plus the rule in CLAUDE.md to keep it updated. No app changes.
