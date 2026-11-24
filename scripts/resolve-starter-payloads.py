#!/usr/bin/env python3
"""Look up the design doc's starter-payload mod names on Modrinth and report
what each one resolves to, plus which versions/loaders it has builds for.
Used to (re)build src-tauri/data/starter_payloads.json; anything not found
or without builds is reported, never silently dropped.
    python3 -I scripts/resolve-starter-payloads.py [name ...]
"""
import json, re, sys, time, urllib.parse, urllib.request

UA = {"User-Agent": "modpack-planner/starter-resolver"}
API = "https://api.modrinth.com/v2"
NAMES = [
    "Sodium", "Lithium", "FerriteCore", "ModernFix", "ImmediatelyFast", "Particle Core", "Crash Assistant", "Packed Packs",
    "C2ME", "Extreme Sounds Muffler", "Cubes Without Borders",
    "Iris", "Oculus", "Iris/Oculus Search",
    "JEI", "EMI", "TooManyRecipeViewers", "Just Enough Effect Descriptions", "Moderately Enough Effect Descriptions",
    "EMIffect", "EMI Ores", "EMI Loot", "EMI Enchanting", "EMI Extra Mod Integrations",
    "Jade", "AppleSkin", "Mouse Tweaks", "Controlling", "Keyboard Atlas",
    "Xaero's Minimap", "Xaero's World Map", "XaeroPlus", "JourneyMap", "Diorama",
    "Distant Horizons", "Voxy", "Roxy",
    "Spark",
    "Subtle Effects", "Visuality Reforged", "Particular Reforged", "Wakes", "Wavify", "Windy: Configurable",
    "Sound Physics Remastered", "Sounds", "Extra Sounds", "Immersive UI", "Smooth GUI", "Drip Sounds", "The Sound of Rain",
    "Presence Footsteps", "Tread Lightly", "AmbientSounds",
    "Punchy!", "Fresh Animations", "Fresh Animations: Extensions", "Fresh Compat", "Punchy! Moves", "Fluid and Foolish",
    "Fresh Animations: Player Extension", "Just Expressions",
]
TARGETS = [("1.21.1", "neoforge"), ("1.21.1", "fabric"), ("26.1.2", "neoforge"), ("26.1.2", "fabric")]

def get(path, **q):
    url = API + path + ("?" + urllib.parse.urlencode(q) if q else "")
    for _ in range(3):
        try:
            with urllib.request.urlopen(urllib.request.Request(url, headers=UA), timeout=30) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code == 429: time.sleep(10); continue
            if e.code == 404: return None
            raise

norm = lambda s: re.sub(r"[^a-z0-9]", "", s.lower())

def resolve(name):
    hits = get("/search", query=name, limit=10, facets=json.dumps([["project_type:mod", "project_type:resourcepack"]]))["hits"]
    exact = [h for h in hits if norm(h["title"]) == norm(name) or norm(h["slug"]) == norm(name)]
    starts = [h for h in hits if norm(h["title"]).startswith(norm(name))]
    best = (exact or starts or hits or [None])[0]
    if not best: return {"name": name, "found": False}
    conf = "exact" if best in exact else "prefix" if best in starts else "loose"
    vs = get(f"/project/{best['project_id']}/version", include_changelog="false") or []
    have = {f"{mc}/{l}": any(mc in v["game_versions"] and (l in v["loaders"] or (l == "fabric" and "quilt" in v["loaders"] and False)) for v in vs) for mc, l in TARGETS}
    return {"name": name, "found": True, "match": conf, "title": best["title"], "slug": best["slug"], "id": best["project_id"],
            "type": best["project_type"], "icon": best.get("icon_url"), "builds": have}

for n in (sys.argv[1:] or NAMES):
    r = resolve(n)
    if not r["found"]:
        print(f"  ✗ {n:38} NOT FOUND"); continue
    b = " ".join(("●" if r["builds"][f"{mc}/{l}"] else "·") for mc, l in TARGETS)
    print(f"  {'✓' if r['match']=='exact' else '?'} {n:38} → {r['title'][:34]:34} {r['slug'][:24]:24} {r['id']} {r['type'][:4]} [{b}]")
    print("    JSON " + json.dumps(r))
