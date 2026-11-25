//! Live Scan (M14): read the mods' own jar metadata and predict what the
//! loader will refuse at startup, which the sites' dependency data doesn't
//! always say.
//!
//! Opt-in. Jars come from either a folder (a Prism instance's `mods/`, read
//! only) or are downloaded from the Modrinth/CurseForge CDNs into a
//! content-addressed scan cache (sha1-checked). Only text inside the zips is
//! parsed (`META-INF/neoforge.mods.toml` / `mods.toml`, `fabric.mod.json`,
//! `quilt.mod.json`, nested jar-in-jar libraries); nothing is executed.
//! `scripts/check-jar-metadata.py` was the prototype.

use crate::error::{AppError, AppResult};
use crate::model::Pack;
use crate::resolve::{Fix, Issue, Severity};
use crate::sources::Loader;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Read;

// ---------------------------------------------------------------- versions
fn vkey(v: &str) -> Vec<(i64, String)> {
    // Build metadata after '+' (e.g. "1.0.82+mc1.21.1") never affects ordering.
    let v = v.split('+').next().unwrap_or(v).trim_end_matches('-');
    v.split(['.', '-'])
        .filter(|p| !p.is_empty())
        .map(|p| {
            let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
            if digits.is_empty() { (-1, p.to_string()) } else { (digits.parse().unwrap_or(0), p[digits.len()..].to_string()) }
        })
        .collect()
}
fn cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let (x, y) = (vkey(a), vkey(b));
    for i in 0..x.len().max(y.len()) {
        let p = x.get(i).cloned().unwrap_or((0, String::new()));
        let q = y.get(i).cloned().unwrap_or((0, String::new()));
        match p.0.cmp(&q.0).then_with(|| p.1.cmp(&q.1)) {
            std::cmp::Ordering::Equal => continue,
            o => return o,
        }
    }
    std::cmp::Ordering::Equal
}

/// Maven-style ranges as mods.toml uses them: `[1.2,2.0)`, `[,0.8)`, `1.0`
/// (at least), unions joined by commas. Unknown version: the range applies.
pub fn in_maven_range(version: Option<&str>, range: &str) -> bool {
    let r = range.trim();
    if r.is_empty() || r == "*" {
        return true;
    }
    let Some(v) = version else { return true };
    let mut parts = vec![];
    let mut cur = String::new();
    let mut depth = 0;
    for c in r.chars() {
        match c {
            '[' | '(' => {
                depth += 1;
                cur.push(c)
            }
            ']' | ')' => {
                depth -= 1;
                cur.push(c);
                if depth == 0 {
                    parts.push(std::mem::take(&mut cur));
                }
            }
            ',' if depth == 0 => {
                if !cur.trim().is_empty() {
                    parts.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    if !cur.trim().is_empty() {
        parts.push(cur);
    }
    parts.iter().any(|p| {
        let p = p.trim();
        if !p.starts_with(['[', '(']) {
            return cmp(v, p) != std::cmp::Ordering::Less;
        }
        let (lo_inc, hi_inc) = (p.starts_with('['), p.ends_with(']'));
        let body = &p[1..p.len() - 1];
        match body.split_once(',') {
            None => cmp(v, body.trim()) == std::cmp::Ordering::Equal,
            Some((lo, hi)) => {
                let (lo, hi) = (lo.trim(), hi.trim());
                (lo.is_empty() || if lo_inc { cmp(v, lo) != std::cmp::Ordering::Less } else { cmp(v, lo) == std::cmp::Ordering::Greater })
                    && (hi.is_empty() || if hi_inc { cmp(v, hi) != std::cmp::Ordering::Greater } else { cmp(v, hi) == std::cmp::Ordering::Less })
            }
        }
    })
}

/// Fabric/Quilt version predicates: `*`, `>=1.2 <2`, `~1.2` (same minor),
/// `^1.2` (same major), `1.2.x` (prefix), plain `1.2` (exactly). A list is OR.
pub fn in_fabric_range(version: Option<&str>, preds: &[String]) -> bool {
    let Some(v) = version else { return true };
    if preds.is_empty() {
        return true;
    }
    preds.iter().any(|pred| {
        pred.split_whitespace().all(|c| {
            let c = c.trim();
            if c == "*" || c.is_empty() {
                return true;
            }
            let (op, rest) = ["<=", ">=", "<", ">", "=", "~", "^"].iter().find_map(|o| c.strip_prefix(o).map(|r| (*o, r))).unwrap_or(("", c));
            let o = cmp(v, rest);
            match op {
                ">=" => o != std::cmp::Ordering::Less,
                ">" => o == std::cmp::Ordering::Greater,
                "<=" => o != std::cmp::Ordering::Greater,
                "<" => o == std::cmp::Ordering::Less,
                "~" | "^" => {
                    // A trailing '-' ("~26.1-") just admits pre-releases.
                    let rest = rest.trim_end_matches('-');
                    let keep = if op == "^" { 1 } else { 2 };
                    let pre: Vec<&str> = rest.split('.').take(keep).collect();
                    cmp(v, rest) != std::cmp::Ordering::Less && v.split('.').take(keep).eq(pre.iter().copied())
                }
                _ if rest.contains(".x") || rest.ends_with('x') => v.starts_with(rest.trim_end_matches('x').trim_end_matches('.')),
                _ => o == std::cmp::Ordering::Equal,
            }
        })
    })
}

/// A Maven range in words: "[1.21,1.21.1)" → "1.21 up to, but not including, 1.21.1".
pub fn describe_maven(range: &str) -> String {
    let r = range.trim();
    if r.is_empty() || r == "*" {
        return "any version".into();
    }
    if !r.starts_with(['[', '(']) {
        return format!("{r} or newer");
    }
    if r.matches(['[', '(']).count() > 1 {
        return r.to_string(); // unions: leave as written
    }
    let (lo_inc, hi_inc) = (r.starts_with('['), r.ends_with(']'));
    let body = &r[1..r.len() - 1];
    match body.split_once(',') {
        None => format!("exactly {}", body.trim()),
        Some((lo, hi)) => {
            let (lo, hi) = (lo.trim(), hi.trim());
            match (lo.is_empty(), hi.is_empty()) {
                (true, true) => "any version".into(),
                (false, true) => if lo_inc { format!("{lo} or newer") } else { format!("newer than {lo}") },
                (true, false) => if hi_inc { format!("{hi} or older") } else { format!("anything older than {hi}") },
                (false, false) => format!("{lo}{} up to{} {hi}", if lo_inc { "" } else { " (exclusive)" }, if hi_inc { " and including" } else { ", but not including," }),
            }
        }
    }
}

// ---------------------------------------------------------------- reading jars
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Meta {
    Forge, // mods.toml / neoforge.mods.toml (Maven ranges)
    Fabric, // fabric.mod.json / quilt.mod.json (semver predicates)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DepKind {
    Required,
    Incompatible,
    Discouraged,
}

#[derive(Debug, Clone)]
struct Dep {
    owner: String,
    id: String,
    kind: DepKind,
    maven: String,
    fabric: Vec<String>,
    meta: Meta,
    jar: String,
}

#[derive(Debug, Default)]
struct Found {
    /// mod id → (version, jar file it came from, nested?)
    provided: BTreeMap<String, (Option<String>, String, bool)>,
    /// mod ids a top-level jar declares itself (for duplicate detection): id → jars
    top: BTreeMap<String, BTreeSet<String>>,
    deps: Vec<Dep>,
}

fn zip_text<R: Read + std::io::Seek>(z: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut f = z.by_name(name).ok()?;
    if f.size() > 4 * 1024 * 1024 {
        return None;
    }
    let mut s = String::new();
    f.read_to_string(&mut s).ok()?;
    Some(s)
}

fn fabric_preds(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(a) => a.iter().filter_map(|x| x.as_str().map(String::from)).collect(),
        _ => vec!["*".into()],
    }
}

fn read_jar(bytes: &[u8], jar: &str, out: &mut Found, depth: u8, loader: Loader) {
    let Ok(mut z) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { return };
    let names: BTreeSet<String> = z.file_names().filter_map(|n| n.ok()).map(|n| n.to_string()).collect();
    let manifest_version = zip_text(&mut z, "META-INF/MANIFEST.MF").and_then(|m| m.lines().find_map(|l| l.strip_prefix("Implementation-Version:").map(|v| v.trim().to_string())));
    let declare = |out: &mut Found, id: &str, v: Option<String>| {
        out.provided.entry(id.to_string()).or_insert((v, jar.to_string(), depth > 0));
        if depth == 0 {
            out.top.entry(id.to_string()).or_default().insert(jar.to_string());
        }
    };
    let forge_loader = matches!(loader, Loader::Neoforge | Loader::Forge);
    // NeoForge reads neoforge.mods.toml first; Forge reads mods.toml.
    for meta in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if !names.contains(meta) || (loader == Loader::Forge && meta.contains("neoforge")) {
            continue;
        }
        let Some(text) = zip_text(&mut z, meta) else { break };
        let Ok(t) = text.parse::<toml::Table>() else { break };
        for m in t.get("mods").and_then(|m| m.as_array()).into_iter().flatten() {
            let Some(id) = m.get("modId").and_then(|x| x.as_str()) else { continue };
            let v = m.get("version").and_then(|x| x.as_str()).filter(|v| !v.contains("${")).map(String::from).or_else(|| manifest_version.clone());
            declare(out, id, v);
        }
        if forge_loader {
            if let Some(deps) = t.get("dependencies").and_then(|d| d.as_table()) {
                for (owner, list) in deps {
                    for d in list.as_array().into_iter().flatten() {
                        let Some(id) = d.get("modId").and_then(|x| x.as_str()) else { continue };
                        let typ = d.get("type").and_then(|x| x.as_str()).map(str::to_ascii_lowercase).unwrap_or_else(|| {
                            if d.get("mandatory").and_then(|x| x.as_bool()).unwrap_or(true) { "required".into() } else { "optional".into() }
                        });
                        let kind = match typ.as_str() {
                            "required" => DepKind::Required,
                            "incompatible" => DepKind::Incompatible,
                            "discouraged" => DepKind::Discouraged,
                            _ => continue,
                        };
                        let range = d.get("versionRange").and_then(|x| x.as_str()).unwrap_or("").to_string();
                        out.deps.push(Dep { owner: owner.clone(), id: id.into(), kind, maven: range, fabric: vec![], meta: Meta::Forge, jar: jar.into() });
                    }
                }
            }
        }
        break;
    }
    for meta in ["fabric.mod.json", "quilt.mod.json"] {
        if !names.contains(meta) {
            continue;
        }
        let Some(text) = zip_text(&mut z, meta) else { continue };
        // Many fabric.mod.json files have stray control characters or comments; be lenient.
        let cleaned: String = text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
        let Ok(j) = serde_json::from_str::<serde_json::Value>(&cleaned) else { continue };
        let fabric_loader = matches!(loader, Loader::Fabric | Loader::Quilt);
        if meta == "fabric.mod.json" {
            let v = j.get("version").and_then(|x| x.as_str()).map(String::from);
            if let Some(id) = j.get("id").and_then(|x| x.as_str()) {
                declare(out, id, v.clone());
                for p in j.get("provides").and_then(|p| p.as_array()).into_iter().flatten().filter_map(|p| p.as_str()) {
                    declare(out, p, v.clone());
                }
                if fabric_loader {
                    for (field, kind) in [("depends", DepKind::Required), ("breaks", DepKind::Incompatible), ("conflicts", DepKind::Discouraged)] {
                        for (dep, pv) in j.get(field).and_then(|d| d.as_object()).into_iter().flatten() {
                            out.deps.push(Dep { owner: id.into(), id: dep.clone(), kind: kind.clone(), maven: String::new(), fabric: fabric_preds(pv), meta: Meta::Fabric, jar: jar.into() });
                        }
                    }
                }
            }
        } else {
            let ql = &j["quilt_loader"];
            let v = ql.get("version").and_then(|x| x.as_str()).map(String::from);
            if let Some(id) = ql.get("id").and_then(|x| x.as_str()) {
                declare(out, id, v.clone());
                for p in ql.get("provides").and_then(|p| p.as_array()).into_iter().flatten() {
                    if let Some(pid) = p.as_str().or_else(|| p.get("id").and_then(|x| x.as_str())) {
                        declare(out, pid, v.clone());
                    }
                }
                if loader == Loader::Quilt {
                    for (field, kind) in [("depends", DepKind::Required), ("breaks", DepKind::Incompatible)] {
                        for d in ql.get(field).and_then(|d| d.as_array()).into_iter().flatten() {
                            let (dep, preds) = match d {
                                serde_json::Value::String(s) => (s.clone(), vec!["*".to_string()]),
                                o => (o.get("id").and_then(|x| x.as_str()).unwrap_or("").to_string(), o.get("versions").map(fabric_preds).unwrap_or_default()),
                            };
                            if !dep.is_empty() && !d.get("optional").and_then(|x| x.as_bool()).unwrap_or(false) {
                                out.deps.push(Dep { owner: id.into(), id: dep, kind: kind.clone(), maven: String::new(), fabric: preds, meta: Meta::Fabric, jar: jar.into() });
                            }
                        }
                    }
                }
            }
        }
    }
    // Jar-in-jar libraries: Fabric's META-INF/jars/, NeoForge/Forge's jarjar metadata.
    if depth < 3 {
        let mut nested: BTreeSet<String> = names.iter().filter(|n| n.starts_with("META-INF/jars/") && n.ends_with(".jar")).cloned().collect();
        if let Some(meta) = zip_text(&mut z, "META-INF/jarjar/metadata.json") {
            if let Ok(j) = serde_json::from_str::<serde_json::Value>(&meta) {
                for p in j["jars"].as_array().into_iter().flatten().filter_map(|x| x["path"].as_str()) {
                    nested.insert(p.to_string());
                }
            }
        }
        for n in nested {
            let mut buf = vec![];
            if let Ok(mut f) = z.by_name(&n) {
                if f.size() > 64 * 1024 * 1024 || f.read_to_end(&mut buf).is_err() {
                    continue;
                }
            } else {
                continue;
            }
            read_jar(&buf, jar, out, depth + 1, loader);
        }
    }
}

// ---------------------------------------------------------------- report
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub jars: usize,
    pub mod_ids: usize,
    pub issues: Vec<Issue>,
    pub notes: Vec<String>,
    /// Files that couldn't be scanned (not downloadable, unreadable…), with why.
    pub skipped: Vec<String>,
}

/// Ids the game or loader provides (never "missing").
fn builtin(id: &str, loader: Loader) -> bool {
    matches!(id, "minecraft" | "java")
        || match loader {
            Loader::Neoforge => matches!(id, "neoforge" | "forge" | "javafml" | "lowcodefml" | "fml" | "mcp"),
            Loader::Forge => matches!(id, "forge" | "javafml" | "lowcodefml" | "fml" | "mcp"),
            Loader::Fabric => matches!(id, "fabricloader" | "fabric-loader"),
            Loader::Quilt => matches!(id, "fabricloader" | "fabric-loader" | "quilt_loader" | "quilt_base"),
        }
}

/// Which pack entry a scanned jar is (file name → key), for fixes and links.
pub type JarIndex = HashMap<String, String>;

/// Analyses a set of jars: (file name, bytes).
pub fn analyse(pack: &Pack, jars: &[(String, Vec<u8>)], index: &JarIndex) -> ScanReport {
    let loader = pack.loader;
    let mut f = Found::default();
    for (name, bytes) in jars {
        read_jar(bytes, name, &mut f, 0, loader);
    }
    let mut issues = vec![];
    let entry_of = |jar: &str| index.get(jar).cloned();
    let name_of = |jar: &str| index.get(jar).and_then(|k| pack.entries.get(k)).map(|e| e.display.name.clone()).unwrap_or_else(|| jar.to_string());
    let loader_version = crate::export::plain_loader_version(pack);
    let loader_ids: &[&str] = match loader {
        Loader::Neoforge => &["neoforge"],
        Loader::Forge => &["forge"],
        Loader::Fabric | Loader::Quilt => &["fabricloader", "fabric-loader"],
    };
    let mut seen = BTreeSet::new();
    for d in &f.deps {
        // Only rules of mods that are actually present apply (an optional nested module can't break anything).
        if !f.provided.contains_key(&d.owner) {
            continue;
        }
        let owner_name = name_of(&d.jar);
        let range_ok = |v: Option<&str>| match d.meta {
            Meta::Forge => in_maven_range(v, &d.maven),
            Meta::Fabric => in_fabric_range(v, &d.fabric),
        };
        let range_text = match d.meta {
            Meta::Forge => d.maven.clone(),
            Meta::Fabric => d.fabric.join(" || "),
        };
        // Said in words where we can (mods.toml ranges are easy to misread: ")" excludes the end).
        let range_words = match d.meta {
            Meta::Forge => format!("{} ({})", describe_maven(&d.maven), d.maven),
            Meta::Fabric => range_text.clone(),
        };
        let id = format!("{:?}:{}:{}:{}", d.kind, d.jar, d.id, range_text);
        if !seen.insert(id.clone()) {
            continue;
        }
        if d.id == "minecraft" {
            if d.kind == DepKind::Required && !range_ok(Some(&pack.mc_version)) {
                // Fabric/Quilt enforce this strictly. NeoForge/Forge in practice load jars whose
                // declared range is off by a patch (JEI 1.21.1 says [1.21, 1.21.1) and runs fine), so
                // there it's only a hint.
                let strict = d.meta == Meta::Fabric;
                issues.push(Issue {
                    id: format!("scan_minecraft:{}", d.jar),
                    severity: if strict { Severity::Error } else { Severity::Info },
                    kind: "scan_minecraft",
                    entry_key: entry_of(&d.jar),
                    other_key: None,
                    message: if strict {
                        format!("{owner_name} says it needs Minecraft {range_words}; the pack is {}. The loader won't start it.", pack.mc_version)
                    } else {
                        format!(
                            "{owner_name}'s jar says it's for Minecraft {range_words}, so {} is just outside it on paper. Mod authors often write the range this way by accident, and NeoForge usually loads it anyway; if the game refuses to start, this is why.",
                            pack.mc_version
                        )
                    },
                    fix: if strict { entry_of(&d.jar).map(|key| Fix::Remove { key }) } else { None },
                });
            }
            continue;
        }
        if loader_ids.contains(&d.id.as_str()) {
            if d.kind == DepKind::Required && !loader_version.is_empty() && !range_ok(Some(&loader_version)) {
                issues.push(Issue {
                    id: format!("scan_loader:{}", d.jar),
                    severity: Severity::Error,
                    kind: "scan_loader",
                    entry_key: entry_of(&d.jar),
                    other_key: None,
                    message: format!("{owner_name} needs {} {range_words}; the pack's loader version is {loader_version}. Pick a newer loader version when you create or duplicate the pack.", d.id),
                    fix: None,
                });
            }
            continue;
        }
        if builtin(&d.id, loader) {
            continue;
        }
        let present = f.provided.get(&d.id);
        match d.kind {
            DepKind::Required => match present {
                None => issues.push(Issue {
                    id: format!("scan_missing:{}:{}", d.jar, d.id),
                    severity: Severity::Error,
                    kind: "scan_missing",
                    entry_key: entry_of(&d.jar),
                    other_key: None,
                    message: format!("{owner_name} requires the mod \"{}\" ({}), which no jar in the pack provides. The game won't start.", d.id, if range_text.is_empty() { "any version".into() } else { range_text.clone() }),
                    // The id is often the Modrinth slug; the fix resolves it (or says it can't).
                    fix: Some(Fix::AddProject { project_id: d.id.clone(), name: d.id.clone(), pin_version: None, for_key: entry_of(&d.jar) }),
                }),
                Some((v, jar, _)) if !range_ok(v.as_deref()) => issues.push(Issue {
                    id: format!("scan_version:{}:{}", d.jar, d.id),
                    severity: Severity::Error,
                    kind: "scan_version",
                    entry_key: entry_of(&d.jar),
                    other_key: entry_of(jar),
                    message: format!("{owner_name} needs {} {range_words}, but the pack has {} ({}).", d.id, v.as_deref().unwrap_or("an unknown version"), name_of(jar)),
                    fix: None,
                }),
                _ => {}
            },
            DepKind::Incompatible | DepKind::Discouraged => {
                if let Some((v, jar, _)) = present {
                    if jar != &d.jar && range_ok(v.as_deref()) {
                        let hard = d.kind == DepKind::Incompatible;
                        issues.push(Issue {
                            id: format!("scan_conflict:{}:{}", d.jar, d.id),
                            severity: if hard { Severity::Error } else { Severity::Warning },
                            kind: if hard { "scan_incompatible" } else { "scan_discouraged" },
                            entry_key: entry_of(&d.jar),
                            other_key: entry_of(jar),
                            message: format!(
                                "{owner_name} {} {} {}{}.",
                                if hard { "refuses to load with" } else { "warns against" },
                                name_of(jar),
                                v.as_deref().unwrap_or(""),
                                if range_text.is_empty() || range_text == "*" { String::new() } else { format!(" (its rule: {} {range_text})", d.id) }
                            ),
                            fix: entry_of(jar).map(|key| Fix::Remove { key }),
                        });
                    }
                }
            }
        }
    }
    // The same mod id in two separate jars: the loader refuses to start.
    for (id, jars) in &f.top {
        if jars.len() > 1 {
            let list: Vec<String> = jars.iter().map(|j| name_of(j)).collect();
            issues.push(Issue {
                id: format!("scan_duplicate:{id}"),
                severity: Severity::Error,
                kind: "scan_duplicate",
                entry_key: jars.iter().next().and_then(|j| entry_of(j)),
                other_key: jars.iter().nth(1).and_then(|j| entry_of(j)),
                message: format!("Two jars both contain the mod \"{id}\": {}. Keep one.", list.join(" and ")),
                fix: jars.iter().nth(1).and_then(|j| entry_of(j)).map(|key| Fix::Remove { key }),
            });
        }
    }
    issues.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.message.cmp(&b.message)));
    ScanReport { jars: jars.len(), mod_ids: f.provided.len(), issues, notes: vec![], skipped: vec![] }
}

// ---------------------------------------------------------------- sources of jars
/// Hosts jars may be downloaded from (Modrinth and CurseForge CDNs only).
pub const DOWNLOAD_HOSTS: &[&str] = &["cdn.modrinth.com", "edge.forgecdn.net", "mediafilez.forgecdn.net", "media.forgecdn.net"];

/// Reads the `.jar` files in a folder (a Prism instance's `mods/`). Read-only.
pub fn read_folder(dir: &std::path::Path) -> AppResult<Vec<(String, Vec<u8>)>> {
    let mut out = vec![];
    let rd = std::fs::read_dir(dir).map_err(|e| AppError::Invalid(format!("Couldn't read {}: {e}", dir.display())))?;
    for ent in rd.flatten() {
        let p = ent.path();
        let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if p.is_file() && name.ends_with(".jar") {
            if let Ok(b) = std::fs::read(&p) {
                out.push((name, b));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// The scan cache: jars by sha1, so a re-scan only fetches what changed.
pub struct Cache {
    pub dir: std::path::PathBuf,
}

impl Cache {
    pub fn has(&self, sha1: &str) -> bool {
        !sha1.is_empty() && self.dir.join(format!("{sha1}.jar")).is_file()
    }
    pub fn get(&self, sha1: &str) -> Option<Vec<u8>> {
        let b = std::fs::read(self.dir.join(format!("{sha1}.jar"))).ok()?;
        (sha1_hex(&b) == sha1).then_some(b)
    }
    pub fn put(&self, sha1: &str, bytes: &[u8]) -> AppResult<()> {
        std::fs::create_dir_all(&self.dir).map_err(|e| AppError::Storage(e.to_string()))?;
        std::fs::write(self.dir.join(format!("{sha1}.jar")), bytes).map_err(|e| AppError::Storage(e.to_string()))
    }
    /// (files, bytes)
    pub fn stats(&self) -> (usize, u64) {
        std::fs::read_dir(&self.dir).map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).fold((0, 0), |(n, b), m| (n + 1, b + m.len()))).unwrap_or((0, 0))
    }
    pub fn clear(&self) -> AppResult<()> {
        if self.dir.exists() {
            std::fs::remove_dir_all(&self.dir).map_err(|e| AppError::Storage(e.to_string()))?;
        }
        Ok(())
    }
}

pub fn sha1_hex(b: &[u8]) -> String {
    use sha1::Digest;
    sha1::Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn jar(files: &[(&str, &str)]) -> Vec<u8> {
        let mut buf = std::io::Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            for (n, t) in files {
                z.start_file(*n, zip::write::SimpleFileOptions::default()).unwrap();
                z.write_all(t.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }

    #[test]
    fn ranges() {
        assert!(in_maven_range(Some("0.8.13"), "[0,)"));
        assert!(in_maven_range(Some("1.0.15"), "[0.0.1,)"));
        assert!(!in_maven_range(Some("0.5"), "[0.6,1.0)"));
        assert!(in_maven_range(Some("21.1.200"), "[21.1.90,)"));
        assert!(!in_maven_range(Some("21.1.77"), "[21.1.90,)"));
        assert!(in_maven_range(Some("1.21.1"), "[1.21.1,1.22)"));
        assert!(in_maven_range(Some("2.0"), "[1.0,1.5),[2.0,)"), "unions");
        assert!(in_fabric_range(Some("0.16.14"), &[">=0.15.0".into()]));
        assert!(!in_fabric_range(Some("0.14.0"), &[">=0.15.0".into()]));
        assert!(in_fabric_range(Some("1.21.1"), &["~1.21".into()]));
        assert!(!in_fabric_range(Some("1.22"), &["~1.21".into()]));
        assert!(in_fabric_range(Some("1.21.4"), &["1.21.x".into()]));
        assert!(in_fabric_range(Some("5.0"), &["<4".into(), ">=5".into()]), "list is OR");
        assert!(in_fabric_range(Some("26.1.2"), &["~26.1-".into()]), "pre-release marker");
        assert!(in_maven_range(Some("1.0.82+mc1.21.1"), "[1.0.82,)"), "build metadata ignored");
        assert_eq!(describe_maven("[1.21,1.21.1)"), "1.21 up to, but not including, 1.21.1");
        assert_eq!(describe_maven("[21.1.90,)"), "21.1.90 or newer");
        assert_eq!(describe_maven("[1.0,2.0]"), "1.0 up to and including 2.0");
    }

    #[test]
    fn finds_missing_conflicts_duplicates_and_loader_needs() {
        let mut pack = crate::resolve::tests::pack_for("1.21.1", Loader::Neoforge);
        pack.loader_version = "21.1.77".into();
        let a = jar(&[(
            "META-INF/neoforge.mods.toml",
            "modLoader=\"javafml\"\nloaderVersion=\"[1,)\"\n[[mods]]\nmodId=\"alpha\"\nversion=\"1.0\"\n\
             [[dependencies.alpha]]\nmodId=\"neoforge\"\ntype=\"required\"\nversionRange=\"[21.1.90,)\"\n\
             [[dependencies.alpha]]\nmodId=\"betalib\"\ntype=\"required\"\nversionRange=\"[2.0,)\"\n\
             [[dependencies.alpha]]\nmodId=\"gamma\"\ntype=\"incompatible\"\nversionRange=\"[0,)\"\n\
             [[dependencies.alpha]]\nmodId=\"ghost\"\ntype=\"required\"\nversionRange=\"[1,)\"\n",
        )]);
        let inner = jar(&[("META-INF/neoforge.mods.toml", "[[mods]]\nmodId=\"betalib\"\nversion=\"1.5\"\n")]);
        let b = {
            let mut buf = std::io::Cursor::new(vec![]);
            {
                let mut z = zip::ZipWriter::new(&mut buf);
                let o = zip::write::SimpleFileOptions::default();
                z.start_file("META-INF/neoforge.mods.toml", o).unwrap();
                z.write_all(b"[[mods]]\nmodId=\"beta\"\nversion=\"3.0\"\n").unwrap();
                z.start_file("META-INF/jarjar/metadata.json", o).unwrap();
                z.write_all(br#"{"jars":[{"path":"META-INF/jarjar/betalib.jar"}]}"#).unwrap();
                z.start_file("META-INF/jarjar/betalib.jar", o).unwrap();
                z.write_all(&inner).unwrap();
                z.finish().unwrap();
            }
            buf.into_inner()
        };
        let g = jar(&[("META-INF/neoforge.mods.toml", "[[mods]]\nmodId=\"gamma\"\nversion=\"1.0\"\n")]);
        let g2 = jar(&[("META-INF/neoforge.mods.toml", "[[mods]]\nmodId=\"gamma\"\nversion=\"1.1\"\n")]);
        let jars = vec![("a.jar".into(), a), ("b.jar".into(), b), ("g.jar".into(), g), ("g2.jar".into(), g2)];
        let idx: JarIndex = [("a.jar", "ka"), ("b.jar", "kb"), ("g.jar", "kg"), ("g2.jar", "kg2")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let r = analyse(&pack, &jars, &idx);
        let kinds: Vec<&str> = r.issues.iter().map(|i| i.kind).collect();
        for k in ["scan_loader", "scan_version", "scan_incompatible", "scan_missing", "scan_duplicate"] {
            assert!(kinds.contains(&k), "{k} in {kinds:?}");
        }
        let v = r.issues.iter().find(|i| i.kind == "scan_version").unwrap();
        assert!(v.message.contains("1.5"), "the nested betalib 1.5 is what's present: {}", v.message);
        let m = r.issues.iter().find(|i| i.kind == "scan_missing").unwrap();
        assert!(matches!(&m.fix, Some(Fix::AddProject { project_id, .. }) if project_id == "ghost"));
    }
}
