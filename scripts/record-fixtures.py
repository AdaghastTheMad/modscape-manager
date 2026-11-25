#!/usr/bin/env python3
"""Record Modrinth responses for the resolver tests.

Writes src-tauri/tests/fixtures/resolver/modrinth.json: trimmed project info,
every version of each project for the listed Minecraft versions (all loaders),
and any versions that dependencies pin by id. Required dependencies are
followed transitively so the recorded world is self-contained.

Re-run when you want fresher data:  python3 scripts/record-fixtures.py
Only talks to api.modrinth.com, with the app's generic User-Agent.
"""
import json, sys, time, urllib.parse, urllib.request
from pathlib import Path

UA = {"User-Agent": "modscape-manager/fixture-recorder"}
API = "https://api.modrinth.com/v2"
SCENARIOS = {
    "1.21.1": ["sodium-extra", "sodium", "iris", "reeses-sodium-options", "modmenu", "fabric-api", "qsl",
               "embeddium", "rubidium", "textrues-embeddium-options", "xaeros-world-map", "continuity", "jei"],
    "26.1.2": ["jei", "embeddium", "sodium", "sodium-extra"],
}
MAX_VERSIONS = 40
OUT = Path(__file__).resolve().parent.parent / "src-tauri/tests/fixtures/resolver/modrinth.json"

def get(path, **params):
    q = ("?" + urllib.parse.urlencode(params)) if params else ""
    for attempt in range(3):
        try:
            with urllib.request.urlopen(urllib.request.Request(API + path + q, headers=UA), timeout=30) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 404:
                return None
            if e.code == 429:
                time.sleep(10); continue
            raise
    raise RuntimeError("rate limited")

def trim_version(v):
    primary = next((f for f in v["files"] if f.get("primary")), v["files"][0] if v["files"] else None)
    return {
        "id": v["id"], "project_id": v["project_id"], "version_number": v["version_number"],
        "version_type": v["version_type"], "date_published": v["date_published"],
        "game_versions": v["game_versions"], "loaders": v["loaders"],
        "dependencies": [{k: d.get(k) for k in ("project_id", "version_id", "file_name", "dependency_type")} for d in v["dependencies"]],
        "files": [{k: primary[k] for k in ("filename", "url", "size", "hashes")}] if primary else [],
    }

def trim_project(p):
    keys = ("id", "slug", "title", "description", "icon_url", "categories", "additional_categories",
            "client_side", "server_side", "updated", "downloads", "loaders", "game_versions")
    out = {k: p.get(k) for k in keys}
    out["license"] = {"id": (p.get("license") or {}).get("id")}
    return out

def main():
    bundle = {"recordedAt": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "versions": {}, "versionById": {}, "projects": {}, "slugs": {}}
    pending_versions = set()
    for mc, slugs in SCENARIOS.items():
        queue = list(slugs)
        seen = set()
        while queue:
            ref = queue.pop(0)
            if ref in seen:
                continue
            seen.add(ref)
            p = get(f"/project/{ref}")
            if not p:
                print(f"  missing project {ref}", file=sys.stderr); continue
            bundle["projects"][p["id"]] = trim_project(p)
            bundle["slugs"][p["slug"]] = p["id"]
            seen.add(p["id"])
            vs = get(f"/project/{p['id']}/version", game_versions=json.dumps([mc]), include_changelog="false") or []
            # Newest 40 per project keep the file small but still mix release/beta/alpha.
            vs = sorted(vs, key=lambda v: v["date_published"], reverse=True)[:MAX_VERSIONS]
            bundle["versions"][f"{p['id']}|{mc}"] = [trim_version(v) for v in vs]
            for v in vs[:3]:  # follow deps of the newest few builds
                for d in v["dependencies"]:
                    if d["dependency_type"] == "required" and d.get("project_id"):
                        queue.append(d["project_id"])
                    if d.get("version_id"):
                        pending_versions.add(d["version_id"])
            print(f"  {mc} {p['slug']}: {len(vs)} versions")
    for vid in sorted(pending_versions):
        v = get(f"/version/{vid}")
        if v:
            bundle["versionById"][vid] = trim_version(v)
    # Optional/incompatible targets only need names.
    others = {d["project_id"] for vs in bundle["versions"].values() for v in vs for d in v["dependencies"] if d.get("project_id")}
    missing = sorted(others - set(bundle["projects"]))
    for i in range(0, len(missing), 50):
        for p in get("/projects", ids=json.dumps(missing[i:i + 50])) or []:
            bundle["projects"][p["id"]] = trim_project(p)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(bundle, indent=1, sort_keys=True))
    print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KB, {len(bundle['projects'])} projects)")

if __name__ == "__main__":
    main()
