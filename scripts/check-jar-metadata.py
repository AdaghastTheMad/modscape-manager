#!/usr/bin/env python3
"""Predicts NeoForge/Forge load errors for a folder of downloaded mod jars,
from the jars' own metadata (META-INF/neoforge.mods.toml or mods.toml, plus
jar-in-jar libraries and fabric.mod.json ids for Connector). It only parses
text inside the zips; nothing is executed.

Use it on test downloads, e.g. after scripts/verify_export:
    python3 -I scripts/check-jar-metadata.py test-env/downloads/neoforge-1.21.1/mods

Reports required mods that are missing and incompatibilities whose version
range matches the version that's actually present (like the loader does).
"""
import io, json, os, re, sys, tomllib, zipfile

BUILTIN = {"minecraft", "neoforge", "forge", "java", "javafml", "fabricloader", "fabric-api"}


def vkey(v):
    parts = re.split(r"[.\-+]", str(v))
    out = []
    for p in parts:
        m = re.match(r"(\d+)(.*)", p)
        out.append((int(m.group(1)), m.group(2)) if m else (-1, p))
    return out


def in_range(version, rng):
    """Maven-style ranges: '[1.2,2.0)', '[,0.8)', '1.0' (= at least), comma-joined unions."""
    if not rng or rng.strip() in ("*", "[0,)", "[0.0.1,)"):
        return True
    if version is None:
        return True  # unknown version: assume the range applies (the loader would too, conservatively)
    for part in re.findall(r"[\[(][^\])]*[\])]|[^,\[\]()]+", rng):
        part = part.strip()
        if not part:
            continue
        if part[0] not in "[(":
            if vkey(version) >= vkey(part):
                return True
            continue
        lo_inc, hi_inc = part[0] == "[", part[-1] == "]"
        body = part[1:-1]
        if "," not in body:
            if vkey(version) == vkey(body):
                return True
            continue
        lo, hi = [x.strip() for x in body.split(",", 1)]
        ok = True
        if lo:
            ok &= vkey(version) >= vkey(lo) if lo_inc else vkey(version) > vkey(lo)
        if hi:
            ok &= vkey(version) <= vkey(hi) if hi_inc else vkey(version) < vkey(hi)
        if ok:
            return True
    return False


def read_jar(zf, label, provided, deps, depth=0):
    names = set(zf.namelist())
    manifest_version = None
    if "META-INF/MANIFEST.MF" in names:
        m = re.search(r"Implementation-Version: (.+)", zf.read("META-INF/MANIFEST.MF").decode("utf-8", "replace"))
        manifest_version = m.group(1).strip() if m else None
    for meta in ("META-INF/neoforge.mods.toml", "META-INF/mods.toml"):
        if meta in names:
            try:
                t = tomllib.loads(zf.read(meta).decode("utf-8", "replace"))
            except Exception as e:
                print(f"  ! unreadable {meta} in {label}: {e}")
                break
            for m in t.get("mods", []):
                v = m.get("version")
                if v is None or "${" in str(v):
                    v = manifest_version
                if m.get("modId"):
                    provided.setdefault(m["modId"], (v, label))
            for owner, lst in (t.get("dependencies") or {}).items():
                for d in lst if isinstance(lst, list) else []:
                    typ = str(d.get("type") or ("required" if d.get("mandatory", True) else "optional")).lower()
                    deps.append((owner, d.get("modId"), typ, d.get("versionRange", ""), label))
            break
    if "fabric.mod.json" in names:
        try:
            j = json.loads(zf.read("fabric.mod.json").decode("utf-8", "replace"), strict=False)
            for i in [j.get("id")] + list(j.get("provides") or []):
                if i:
                    provided.setdefault(i, (j.get("version"), label))
        except Exception:
            pass
    if depth < 3:
        nested = set(n for n in names if n.startswith("META-INF/jars/") and n.endswith(".jar"))
        if "META-INF/jarjar/metadata.json" in names:
            try:
                nested |= {x["path"] for x in json.loads(zf.read("META-INF/jarjar/metadata.json"))["jars"]}
            except Exception:
                pass
        for n in nested & names:
            try:
                read_jar(zipfile.ZipFile(io.BytesIO(zf.read(n))), label, provided, deps, depth + 1)
            except Exception:
                pass


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    folder = sys.argv[1]
    jars = sorted(f for f in os.listdir(folder) if f.endswith(".jar"))
    provided, deps = {}, []
    for f in jars:
        try:
            read_jar(zipfile.ZipFile(os.path.join(folder, f)), f, provided, deps)
        except Exception as e:
            print(f"  ! not a readable jar: {f} ({e})")
    missing = sorted({(o, m, l) for o, m, t, r, l in deps if t == "required" and m not in provided and m not in BUILTIN})
    clashes = sorted({(o, m, provided[m][0], r, l) for o, m, t, r, l in deps
                      if t == "incompatible" and m in provided and o in provided and in_range(provided[m][0], r)})
    print(f"{len(jars)} jars, {len(provided)} mod ids")
    for o, m, l in missing:
        print(f"  ✗ {o} ({l}) requires {m}, which is missing")
    for o, m, v, r, l in clashes:
        print(f"  ✗ {o} ({l}) is incompatible with {m} {r or 'any'} (present: {v})")
    if not missing and not clashes:
        print("  ✔ no missing requirements or live incompatibilities")
    sys.exit(1 if missing or clashes else 0)


if __name__ == "__main__":
    main()
