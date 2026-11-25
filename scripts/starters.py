#!/usr/bin/env python3
"""Manage the starter payloads that ship with Modscape Manager
(src-tauri/data/starter_payloads.json). See docs/PAYLOADS.md.

    python3 scripts/starters.py list
    python3 scripts/starters.py add  <exported.payload.json> [--id starter-name]
    python3 scripts/starters.py remove <starter-id>
    python3 scripts/starters.py check

`add` takes a payload you exported from the app (Payloads → Export…). If a
starter with the same id or name exists it's replaced (keeping its id, so
installs that have it get the update); otherwise it's added as a new starter.
Nested payloads (`includes`) must be starters too. `check` validates the file.
"""
import json
import re
import sys
from pathlib import Path

FILE = Path(__file__).resolve().parent.parent / "src-tauri" / "data" / "starter_payloads.json"
FORMATS = ("modscape-manager-payload", "modpack-planner-payload")


def load():
    return json.loads(FILE.read_text(encoding="utf-8"))


def save(d):
    FILE.write_text(json.dumps(d, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def slug(s):
    return re.sub(r"[^a-z0-9]+", "-", s.lower()).strip("-") or "payload"


def problems(d):
    out = []
    ids = [p.get("id") for p in d["payloads"]]
    if len(ids) != len(set(ids)):
        out.append("two starters share an id")
    for p in d["payloads"]:
        name = p.get("name", "?")
        if not str(p.get("id", "")).startswith("starter-"):
            out.append(f"{name}: id should start with 'starter-'")
        if not p.get("name", "").strip():
            out.append(f"{p.get('id')}: has no name")
        for inc in p.get("includes", []):
            if inc not in ids:
                out.append(f"{name}: includes '{inc}', which isn't a starter")
        def cands(slots):
            for s in slots:
                for c in s.get("candidates", []):
                    yield c
        slots = []
        for it in p.get("items", []):
            if it.get("kind") == "mod":
                slots.append(it)
            elif it.get("kind") == "choice":
                for o in it.get("options", []):
                    slots += o.get("slots", [])
                slots += it.get("common", [])
            else:
                out.append(f"{name}: unknown item kind {it.get('kind')!r}")
        for c in cands(slots):
            if not c.get("modrinthId") and c.get("curseforgeId") is None:
                out.append(f"{name}: candidate {c.get('name')!r} has neither a Modrinth nor a CurseForge id")
        if not p.get("items") and not p.get("includes"):
            out.append(f"{name}: is empty")
    return out


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(__doc__)
        return 0
    cmd, rest = argv[0], argv[1:]
    d = load()
    if cmd == "list":
        for p in d["payloads"]:
            n = len(p.get("items", []))
            inc = f" + includes {', '.join(p['includes'])}" if p.get("includes") else ""
            print(f"  {p['id']:28} v{p.get('version', 1):<3} {p['name']}  ({n} item{'s' if n != 1 else ''}{inc})")
        return 0
    if cmd == "check":
        ps = problems(d)
        for x in ps:
            print("  ✘", x)
        print("  ✔ starter_payloads.json looks right" if not ps else f"  {len(ps)} problem(s)")
        return 1 if ps else 0
    if cmd == "remove":
        if not rest:
            print("usage: starters.py remove <starter-id>")
            return 2
        before = len(d["payloads"])
        d["payloads"] = [p for p in d["payloads"] if p["id"] != rest[0]]
        if len(d["payloads"]) == before:
            print(f"No starter with id {rest[0]!r}. Run 'starters.py list'.")
            return 1
        for p in d["payloads"]:
            p["includes"] = [i for i in p.get("includes", []) if i != rest[0]]
        save(d)
        print(f"Removed {rest[0]}.")
        return 0
    if cmd == "add":
        if not rest:
            print("usage: starters.py add <exported.payload.json> [--id starter-name]")
            return 2
        src = json.loads(Path(rest[0]).expanduser().read_text(encoding="utf-8"))
        if src.get("format") not in FORMATS:
            print("That file isn't a payload exported from the app (Payloads → Export…).")
            return 1
        p = src["payload"]
        want_id = rest[rest.index("--id") + 1] if "--id" in rest else None
        existing = next((x for x in d["payloads"] if x["id"] == (want_id or p.get("id")) or x["name"].lower() == p["name"].lower()), None)
        new = {
            "id": existing["id"] if existing else (want_id or f"starter-{slug(p['name'])}"),
            "name": p["name"].strip(),
            "description": p.get("description", "").strip(),
            "items": p.get("items", []),
            "includes": p.get("includes", []),
            "version": existing.get("version", 1) if existing else 1,
            "starter": True,
        }
        if existing:
            d["payloads"][d["payloads"].index(existing)] = new
            print(f"Replaced starter {new['id']} ({new['name']}). Installs that haven't edited it update on their next start.")
        else:
            d["payloads"].append(new)
            print(f"Added starter {new['id']} ({new['name']}). Every install gets it on its next start.")
        ps = problems(d)
        for x in ps:
            print("  ✘", x)
        if ps:
            print("Not saved. Fix the above (e.g. export the nested payloads as starters first), then try again.")
            return 1
        save(d)
        return 0
    print(f"Unknown command {cmd!r}.")
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
