//! Dependency resolution and pack checks.
//!
//! Everything here works against the `Catalog` trait, so tests run on
//! recorded API responses and the app runs on the live Modrinth client.
//! Dependencies live on a *version*, never the project: we first pick the
//! right file for (Minecraft version, loader), then read its dependencies.

use crate::error::{AppError, AppResult};
use crate::model::{ChannelPolicy, EntryDisplay, EntryKind, ModRef, Pack, PackEntry, Reason};
use crate::sources::{curseforge, norm, Loader, Source, LOADER_SLUGS};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

const PARALLEL: usize = 8;

/// Conflicts declared inside mod jars that the sites don't list (see data/known_conflicts.json).
#[derive(Deserialize)]
struct KnownConflicts {
    conflicts: Vec<KnownConflict>,
}
#[derive(Deserialize)]
struct KnownConflict {
    a: ConflictSide,
    b: ConflictSide,
    source: String,
}
#[derive(Deserialize)]
struct ConflictSide {
    modrinth: Option<String>,
    curseforge: Option<u64>,
    slug: Option<String>,
}

fn known_conflicts() -> &'static KnownConflicts {
    static K: std::sync::OnceLock<KnownConflicts> = std::sync::OnceLock::new();
    K.get_or_init(|| serde_json::from_str(include_str!("../data/known_conflicts.json")).expect("known_conflicts.json"))
}

fn find_side(pack: &Pack, s: &ConflictSide) -> Option<String> {
    pack.entries
        .iter()
        .find(|(_, e)| {
            (s.modrinth.is_some() && e.mod_ref.modrinth_id == s.modrinth)
                || (s.curseforge.is_some() && e.mod_ref.curseforge_id == s.curseforge)
                || s.slug.as_ref().is_some_and(|sl| !e.display.slug.is_empty() && norm(&e.display.slug) == norm(sl))
        })
        .map(|(k, _)| k.clone())
}
/// Safety valve for a runaway dependency tree.
pub const MAX_NEW_ENTRIES: usize = 100;
const STALE_SECS: i64 = 365 * 24 * 60 * 60;

/// Projects that stand in for others (`provider` satisfies a dependency on any of `provides`).
const PROVIDES: &[(&str, &[&str])] = &[
    // Quilted Fabric API / QSL provides Fabric API on Quilt.
    ("qvIfYCYJ", &["P7dR8mSH"]),
];

// =============================================================== wire types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DepType {
    Required,
    Optional,
    Incompatible,
    Embedded,
    #[serde(other)]
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dep {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    pub dependency_type: DepType,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct VFile {
    pub filename: String,
    pub url: String,
    pub size: u64,
    pub hashes: HashMap<String, String>,
    /// Modrinth marks the main jar; extra files (sources, dev jars) aren't installed.
    pub primary: bool,
}

impl Version {
    /// The jar a launcher installs: Modrinth's primary file, else the first.
    pub fn main_file(&self) -> Option<&VFile> {
        self.files.iter().find(|f| f.primary).or_else(|| self.files.first())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Version {
    pub id: String,
    pub project_id: String,
    #[serde(default)]
    pub version_number: String,
    #[serde(default = "release")]
    pub version_type: String,
    #[serde(default)]
    pub date_published: String,
    #[serde(default)]
    pub game_versions: Vec<String>,
    #[serde(default)]
    pub loaders: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<Dep>,
    #[serde(default)]
    pub files: Vec<VFile>,
    /// CurseForge file whose author blocks third-party downloads.
    #[serde(default)]
    pub distribution_blocked: bool,
}
fn release() -> String {
    "release".into()
}

impl Version {
    pub fn supports(&self, mc: &str, loader: Loader) -> bool {
        self.game_versions.iter().any(|v| v == mc) && loader.accepts().iter().any(|l| self.loaders.iter().any(|x| x == l))
    }
    fn native_for(&self, loader: Loader) -> bool {
        self.loaders.iter().any(|l| l == loader.accepts()[0])
    }
    fn file_info(&self, pinned: bool) -> FileInfo {
        FileInfo {
            version_id: self.id.clone(),
            version_number: self.version_number.clone(),
            version_type: self.version_type.clone(),
            date_published: self.date_published.clone(),
            filename: self.files.first().map(|f| f.filename.clone()),
            loaders: self.loaders.clone(),
            pinned,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LicenseRef {
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct ProjectInfo {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
    pub additional_categories: Vec<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    pub updated: Option<String>,
    pub downloads: u64,
    pub license: Option<LicenseRef>,
}

impl ProjectInfo {
    pub fn display(&self) -> EntryDisplay {
        let mut cats: Vec<String> = self
            .categories
            .iter()
            .chain(self.additional_categories.iter())
            .filter(|c| !LOADER_SLUGS.contains(&c.as_str()))
            .cloned()
            .collect();
        cats.sort();
        cats.dedup();
        EntryDisplay {
            name: if self.title.is_empty() { self.id.clone() } else { self.title.clone() },
            slug: self.slug.clone(),
            summary: self.description.clone(),
            icon_url: self.icon_url.clone().filter(|u| !u.is_empty()),
            categories: cats,
            client_side: self.client_side.clone(),
            server_side: self.server_side.clone(),
            license: self.license.as_ref().and_then(|l| l.id.clone()),
            updated: self.updated.clone(),
            downloads: Some(self.downloads),
        }
    }
}

/// Picks the file a floating mod uses.
/// `Stable`: newest release, else newest beta, else alpha.
/// `Newest`: newest build of any channel.
/// Either way, for Quilt packs a native Quilt build beats a Fabric one of the same rank.
pub fn select_version(versions: &[Version], mc: &str, loader: Loader, policy: ChannelPolicy) -> Option<Version> {
    let rank = |t: &str| match (policy, t) {
        (ChannelPolicy::Newest, _) => 0,
        (_, "release") => 0,
        (_, "beta") => 1,
        (_, "alpha") => 2,
        _ => 3,
    };
    versions
        .iter()
        .filter(|v| v.supports(mc, loader))
        .min_by(|a, b| {
            rank(&a.version_type)
                .cmp(&rank(&b.version_type))
                .then(b.native_for(loader).cmp(&a.native_for(loader)))
                .then(b.date_published.cmp(&a.date_published))
        })
        .cloned()
}

#[async_trait::async_trait]
pub trait Catalog: Send + Sync {
    /// Versions of a project usable with (mc, loader). May return extra
    /// versions; callers re-check with `Version::supports`.
    async fn versions(&self, project_id: &str, mc: &str, loader: Loader, force: bool) -> AppResult<Vec<Version>>;
    /// A single version by id; Ok(None) if it doesn't exist.
    async fn version(&self, id: &str, force: bool) -> AppResult<Option<Version>>;
    /// Project info for many ids; unknown ids are simply absent.
    async fn projects(&self, ids: &[String], force: bool) -> AppResult<Vec<ProjectInfo>>;
}

/// Routes `cf:` ids to CurseForge and everything else to Modrinth.
pub struct Multi<'a> {
    pub modrinth: &'a dyn Catalog,
    pub curseforge: &'a dyn Catalog,
}

impl Multi<'_> {
    fn pick(&self, id: &str) -> &dyn Catalog {
        if id.starts_with(curseforge::PREFIX) { self.curseforge } else { self.modrinth }
    }
}

#[async_trait::async_trait]
impl Catalog for Multi<'_> {
    async fn versions(&self, project_id: &str, mc: &str, loader: Loader, force: bool) -> AppResult<Vec<Version>> {
        self.pick(project_id).versions(project_id, mc, loader, force).await
    }
    async fn version(&self, id: &str, force: bool) -> AppResult<Option<Version>> {
        self.pick(id).version(id, force).await
    }
    async fn projects(&self, ids: &[String], force: bool) -> AppResult<Vec<ProjectInfo>> {
        let (cf, mr): (Vec<String>, Vec<String>) = ids.iter().cloned().partition(|i| i.starts_with(curseforge::PREFIX));
        let mut out = Vec::new();
        if !mr.is_empty() {
            out.extend(self.modrinth.projects(&mr, force).await?);
        }
        if !cf.is_empty() {
            // Without a CurseForge key, CurseForge names just stay unknown.
            out.extend(self.curseforge.projects(&cf, force).await.unwrap_or_default());
        }
        Ok(out)
    }
}

// ================================================================= reports
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub version_id: String,
    pub version_number: String,
    pub version_type: String,
    pub date_published: String,
    pub filename: Option<String>,
    pub loaders: Vec<String>,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepLink {
    pub project_id: String,
    pub name: String,
    pub in_pack: Option<String>,
    pub version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct EntryReport {
    pub file: Option<FileInfo>,
    pub required: Vec<DepLink>,
    pub optional: Vec<DepLink>,
    pub incompatible: Vec<DepLink>,
    pub required_by: Vec<String>,
    /// False when this entry couldn't be checked (network, CurseForge-only for now).
    pub checked: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fix {
    #[serde(rename_all = "camelCase")]
    /// `for_key`: the entry that needs it, so the new mod is recorded as its dependency.
    AddProject { project_id: String, name: String, pin_version: Option<String>, for_key: Option<String> },
    #[serde(rename_all = "camelCase")]
    Remove { key: String },
    #[serde(rename_all = "camelCase")]
    Unpin { key: String },
    #[serde(rename_all = "camelCase")]
    PinTo { key: String, version_id: String, version_number: String },
    /// Switch an entry that's linked on both sites to the other one.
    #[serde(rename_all = "camelCase")]
    UseSource { key: String, source: Source },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Stable id so the UI can key and dismiss issues.
    pub id: String,
    pub severity: Severity,
    pub kind: &'static str,
    pub entry_key: Option<String>,
    pub other_key: Option<String>,
    pub message: String,
    pub fix: Option<Fix>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Suggestion {
    pub project_id: String,
    pub name: String,
    pub summary: String,
    pub icon_url: Option<String>,
    pub suggested_by: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Analysis {
    pub entries: BTreeMap<String, EntryReport>,
    pub issues: Vec<Issue>,
    pub suggestions: Vec<Suggestion>,
    /// True when every entry was checked; the UI only syncs "pulled in by"
    /// links (and offers orphan cleanup) from complete analyses.
    pub complete: bool,
}

// ================================================================ helpers
fn loader_name(l: Loader) -> &'static str {
    match l {
        Loader::Neoforge => "NeoForge",
        Loader::Fabric => "Fabric",
        Loader::Forge => "Forge",
        Loader::Quilt => "Quilt",
    }
}

/// Catalog id the resolver uses for an entry (preferred site) and its pin.
/// CurseForge ids are `cf:`-prefixed; pins may be stored with or without it.
pub fn target(e: &PackEntry) -> Option<(String, Option<String>)> {
    match e.mod_ref.preferred {
        Source::Modrinth => e.mod_ref.modrinth_id.clone().map(|id| (id, e.file_id.clone())),
        Source::Curseforge => e.mod_ref.curseforge_id.map(|id| {
            let pin = e.file_id.as_ref().map(|f| if f.starts_with(curseforge::PREFIX) { f.clone() } else { format!("{}{f}", curseforge::PREFIX) });
            (curseforge::cf_id(id), pin)
        }),
    }
}

/// project id (either site) → entry key, including ids provided by stand-in projects.
fn pack_index(pack: &Pack) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for (k, e) in &pack.entries {
        if let Some(id) = e.mod_ref.curseforge_id {
            m.insert(curseforge::cf_id(id), k.clone());
        }
        if let Some(id) = &e.mod_ref.modrinth_id {
            m.insert(id.clone(), k.clone());
            for (provider, provides) in PROVIDES {
                if provider == id {
                    for p in *provides {
                        m.entry((*p).to_string()).or_insert_with(|| k.clone());
                    }
                }
            }
        }
    }
    m
}

fn iso_age_secs(iso: &str, now: i64) -> Option<i64> {
    // Minimal RFC 3339 date parse (YYYY-MM-DD…); precision of a day is plenty here.
    let y: i64 = iso.get(0..4)?.parse().ok()?;
    let m: i64 = iso.get(5..7)?.parse().ok()?;
    let d: i64 = iso.get(8..10)?.parse().ok()?;
    // Days from civil (Howard Hinnant's algorithm).
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(now - days * 86_400)
}

struct Selected {
    version: Option<Version>,
    pinned: bool,
    /// Pinned file missing or not for this version/loader.
    pin_problem: Option<&'static str>,
}

async fn select_for(cat: &dyn Catalog, project_id: &str, pin: Option<&str>, mc: &str, loader: Loader, policy: ChannelPolicy, force: bool) -> AppResult<Selected> {
    if let Some(vid) = pin {
        return Ok(match cat.version(vid, force).await? {
            None => Selected { version: None, pinned: true, pin_problem: Some("missing") },
            Some(v) if v.project_id != project_id => Selected { version: None, pinned: true, pin_problem: Some("missing") },
            Some(v) => {
                let ok = v.supports(mc, loader);
                Selected { version: Some(v), pinned: true, pin_problem: (!ok).then_some("mismatch") }
            }
        });
    }
    let versions = cat.versions(project_id, mc, loader, force).await?;
    Ok(Selected { version: select_version(&versions, mc, loader, policy), pinned: false, pin_problem: None })
}

/// Fills in `project_id` for dependencies that only name a version.
async fn resolve_dep_projects(cat: &dyn Catalog, deps: &mut [Dep], force: bool, cache: &mut HashMap<String, Option<Version>>) {
    for d in deps.iter_mut() {
        if d.project_id.is_none() {
            if let Some(vid) = d.version_id.clone() {
                if !cache.contains_key(&vid) {
                    let v = cat.version(&vid, force).await.ok().flatten();
                    cache.insert(vid.clone(), v);
                }
                d.project_id = cache.get(&vid).and_then(|v| v.as_ref()).map(|v| v.project_id.clone());
            }
        }
    }
}

// ================================================================ analyse
pub async fn analyze(pack: &Pack, cat: &dyn Catalog, force: bool, now: i64) -> Analysis {
    let mc = pack.mc_version.as_str();
    let loader = pack.loader;
    let index = pack_index(pack);
    let mut entries: BTreeMap<String, EntryReport> = pack.entries.keys().map(|k| (k.clone(), EntryReport::default())).collect();
    let mut issues: Vec<Issue> = Vec::new();
    let mut complete = true;

    // 1. Pick a file for every entry (on its preferred site), in parallel.
    let targets: Vec<(String, String, Option<String>)> =
        pack.entries.iter().filter_map(|(k, e)| target(e).map(|(pid, pin)| (k.clone(), pid, pin))).collect();
    if targets.len() < pack.entries.len() {
        complete = false;
    }
    let selected: Vec<(String, String, AppResult<Selected>)> = stream::iter(targets)
        .map(|(k, pid, pin)| async move {
            let r = select_for(cat, &pid, pin.as_deref(), mc, loader, pack.channel_policy, force).await;
            (k, pid, r)
        })
        .buffer_unordered(PARALLEL)
        .collect()
        .await;

    // 2. Normalise dependency lists.
    let mut vcache: HashMap<String, Option<Version>> = HashMap::new();
    let mut chosen: BTreeMap<String, (String, Version)> = BTreeMap::new();
    for (key, pid, res) in selected {
        let e = &pack.entries[&key];
        let name = &e.display.name;
        match res {
            Err(err) => {
                complete = false;
                issues.push(Issue {
                    id: format!("unchecked:{key}"),
                    severity: Severity::Warning,
                    kind: "unchecked",
                    entry_key: Some(key.clone()),
                    other_key: None,
                    message: format!("Couldn't check {name}: {err}"),
                    fix: None,
                });
            }
            Ok(sel) => {
                let rep = entries.get_mut(&key).unwrap();
                rep.checked = true;
                match sel.pin_problem {
                    Some("missing") => issues.push(Issue {
                        id: format!("pin_missing:{key}"),
                        severity: Severity::Error,
                        kind: "pin_missing",
                        entry_key: Some(key.clone()),
                        other_key: None,
                        message: format!("{name} is pinned to a file that no longer exists on Modrinth."),
                        fix: Some(Fix::Unpin { key: key.clone() }),
                    }),
                    Some(_) => issues.push(Issue {
                        id: format!("pin_mismatch:{key}"),
                        severity: Severity::Error,
                        kind: "pin_mismatch",
                        entry_key: Some(key.clone()),
                        other_key: None,
                        message: format!(
                            "{name} is pinned to {} which isn't for Minecraft {mc} · {}.",
                            sel.version.as_ref().map(|v| v.version_number.as_str()).unwrap_or("?"),
                            loader_name(loader)
                        ),
                        fix: Some(Fix::Unpin { key: key.clone() }),
                    }),
                    None => {}
                }
                match sel.version {
                    None if sel.pin_problem.is_none() => issues.push(Issue {
                        id: format!("no_build:{key}"),
                        severity: Severity::Error,
                        kind: "no_build",
                        entry_key: Some(key.clone()),
                        other_key: None,
                        message: format!("{name} has no build for Minecraft {mc} · {}.", loader_name(loader)),
                        fix: Some(Fix::Remove { key: key.clone() }),
                    }),
                    None => {}
                    Some(mut v) => {
                        rep.file = Some(v.file_info(sel.pinned));
                        if v.distribution_blocked {
                            let other = e.mod_ref.modrinth_id.is_some();
                            issues.push(Issue {
                                id: format!("cf_distribution:{key}"),
                                severity: Severity::Warning,
                                kind: "cf_distribution",
                                entry_key: Some(key.clone()),
                                other_key: None,
                                message: format!(
                                    "{name}'s author blocks third-party downloads on CurseForge, so launchers will ask you to download it by hand.{}",
                                    if other { " It's also on Modrinth." } else { "" }
                                ),
                                fix: other.then(|| Fix::UseSource { key: key.clone(), source: Source::Modrinth }),
                            });
                        }
                        resolve_dep_projects(cat, &mut v.dependencies, force, &mut vcache).await;
                        if !sel.pinned {
                            if let Some(age) = iso_age_secs(&v.date_published, now) {
                                if age > STALE_SECS {
                                    issues.push(Issue {
                                        id: format!("stale:{key}"),
                                        severity: Severity::Warning,
                                        kind: "stale",
                                        entry_key: Some(key.clone()),
                                        other_key: None,
                                        message: format!(
                                            "{name}'s newest {mc} build is from {}, over a year ago.",
                                            v.date_published.get(0..10).unwrap_or("?")
                                        ),
                                        fix: None,
                                    });
                                }
                            }
                        }
                        chosen.insert(key.clone(), (pid, v));
                    }
                }
            }
        }
    }

    // 3. Names for everything a dependency points at that isn't in the pack.
    let mut wanted: HashSet<String> = HashSet::new();
    for (_, v) in chosen.values() {
        for d in &v.dependencies {
            if let Some(p) = &d.project_id {
                if !index.contains_key(p) {
                    wanted.insert(p.clone());
                }
            }
        }
    }
    let mut wanted: Vec<String> = wanted.into_iter().collect();
    wanted.sort();
    let infos: HashMap<String, ProjectInfo> = cat.projects(&wanted, force).await.unwrap_or_default().into_iter().map(|p| (p.id.clone(), p)).collect();
    let name_of = |pid: &str| -> String {
        if let Some(k) = index.get(pid) {
            return pack.entries[k].display.name.clone();
        }
        infos.get(pid).map(|p| p.display().name).unwrap_or_else(|| pid.to_string())
    };

    // Pinned versions that dependencies ask for (for mismatch warnings).
    let pin_ids: Vec<String> = chosen
        .values()
        .flat_map(|(_, v)| v.dependencies.iter())
        .filter(|d| d.dependency_type == DepType::Required)
        .filter_map(|d| d.version_id.clone())
        .filter(|id| !vcache.contains_key(id))
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    for id in pin_ids {
        let v = cat.version(&id, force).await.ok().flatten();
        vcache.insert(id, v);
    }

    // 4. Walk dependencies.
    let mut suggestions: BTreeMap<String, Suggestion> = BTreeMap::new();
    let dismissed: HashSet<&String> = pack.dismissed_suggestions.iter().collect();
    let mut seen_incompat: HashSet<(String, String)> = HashSet::new();
    let mut required_by: HashMap<String, Vec<String>> = HashMap::new();

    for (key, (own_pid, v)) in &chosen {
        let a_name = &pack.entries[key].display.name;
        let mut seen_deps: HashSet<(String, DepType)> = HashSet::new();
        for d in &v.dependencies {
            let Some(dpid) = d.project_id.clone() else { continue };
            if &dpid == own_pid || !seen_deps.insert((dpid.clone(), d.dependency_type)) {
                continue;
            }
            let in_pack = index.get(&dpid).cloned();
            let link = DepLink { project_id: dpid.clone(), name: name_of(&dpid), in_pack: in_pack.clone(), version_id: d.version_id.clone() };
            match d.dependency_type {
                DepType::Required => {
                    match &in_pack {
                        Some(dk) => {
                            required_by.entry(dk.clone()).or_default().push(key.clone());
                            // A pin to an exact version we can actually use, but the pack uses another.
                            if let (Some(want), Some((_, have))) = (&d.version_id, chosen.get(dk)) {
                                if &have.id != want {
                                    if let Some(Some(wv)) = vcache.get(want) {
                                        if wv.supports(mc, loader) {
                                            issues.push(Issue {
                                                id: format!("version_mismatch:{key}:{dk}"),
                                                severity: Severity::Warning,
                                                kind: "version_mismatch",
                                                entry_key: Some(dk.clone()),
                                                other_key: Some(key.clone()),
                                                message: format!(
                                                    "{a_name} asks for {} {}; the pack uses {}.",
                                                    link.name, wv.version_number, have.version_number
                                                ),
                                                fix: Some(Fix::PinTo {
                                                    key: dk.clone(),
                                                    version_id: wv.id.clone(),
                                                    version_number: wv.version_number.clone(),
                                                }),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                        None if !infos.contains_key(&dpid) => issues.push(Issue {
                            id: format!("missing:{key}:{dpid}"),
                            severity: Severity::Error,
                            kind: "missing_dependency",
                            entry_key: Some(key.clone()),
                            other_key: None,
                            message: format!("{a_name} requires a project that no longer exists on Modrinth ({dpid}). Ask its author, or look for an update."),
                            fix: None,
                        }),
                        None => issues.push(Issue {
                            id: format!("missing:{key}:{dpid}"),
                            severity: Severity::Error,
                            kind: "missing_dependency",
                            entry_key: Some(key.clone()),
                            other_key: None,
                            message: format!("{a_name} requires {}, which isn't in the pack.", link.name),
                            fix: Some(Fix::AddProject { project_id: dpid.clone(), name: link.name.clone(), pin_version: d.version_id.clone(), for_key: Some(key.clone()) }),
                        }),
                    }
                    entries.get_mut(key).unwrap().required.push(link);
                }
                DepType::Optional => {
                    if in_pack.is_none() && !dismissed.contains(&dpid) {
                        let info = infos.get(&dpid);
                        suggestions
                            .entry(dpid.clone())
                            .or_insert_with(|| Suggestion {
                                project_id: dpid.clone(),
                                name: link.name.clone(),
                                summary: info.map(|i| i.description.clone()).unwrap_or_default(),
                                icon_url: info.and_then(|i| i.icon_url.clone()),
                                suggested_by: vec![],
                            })
                            .suggested_by
                            .push(key.clone());
                    }
                    entries.get_mut(key).unwrap().optional.push(link);
                }
                DepType::Incompatible => {
                    if let Some(other) = &in_pack {
                        let pair = if key < other { (key.clone(), other.clone()) } else { (other.clone(), key.clone()) };
                        if seen_incompat.insert(pair.clone()) {
                            issues.push(Issue {
                                id: format!("incompatible:{}:{}", pair.0, pair.1),
                                severity: Severity::Error,
                                kind: "incompatible",
                                entry_key: Some(key.clone()),
                                other_key: Some(other.clone()),
                                message: format!("{a_name} is marked incompatible with {}.", link.name),
                                fix: Some(Fix::Remove { key: other.clone() }),
                            });
                        }
                    }
                    entries.get_mut(key).unwrap().incompatible.push(link);
                }
                DepType::Embedded | DepType::Other => {}
            }
        }
    }

    // 4b. Conflicts declared in the mods' own files (known list).
    for c in &known_conflicts().conflicts {
        if let (Some(ka), Some(kb)) = (find_side(pack, &c.a), find_side(pack, &c.b)) {
            if ka == kb {
                continue;
            }
            let pair = if ka < kb { (ka.clone(), kb.clone()) } else { (kb.clone(), ka.clone()) };
            if seen_incompat.insert(pair.clone()) {
                issues.push(Issue {
                    id: format!("incompatible:{}:{}", pair.0, pair.1),
                    severity: Severity::Error,
                    kind: "incompatible",
                    entry_key: Some(ka.clone()),
                    other_key: Some(kb.clone()),
                    message: format!(
                        "{} and {} won't load together ({}; not listed on the mod sites).",
                        pack.entries[&ka].display.name, pack.entries[&kb].display.name, c.source
                    ),
                    fix: Some(Fix::Remove { key: kb.clone() }),
                });
            }
        }
    }

    // 5. Per-entry checks that don't need the network.
    for (key, e) in &pack.entries {
        let rb = required_by.remove(key).unwrap_or_default();
        let name = &e.display.name;
        let client = e.display.client_side.as_deref();
        let server = e.display.server_side.as_deref();
        if server == Some("unsupported") && client != Some("unsupported") {
            issues.push(Issue {
                id: format!("client_only:{key}"),
                severity: Severity::Info,
                kind: "client_only",
                entry_key: Some(key.clone()),
                other_key: None,
                message: format!("{name} is client-only. Leave it out of a server install."),
                fix: None,
            });
        } else if client == Some("unsupported") && server != Some("unsupported") {
            issues.push(Issue {
                id: format!("server_only:{key}"),
                severity: Severity::Info,
                kind: "server_only",
                entry_key: Some(key.clone()),
                other_key: None,
                message: format!("{name} runs on the server side only: needed for singleplayer and on servers, but not to join a server."),
                fix: None,
            });
        }
        if complete && e.reason == Reason::Dependency && rb.is_empty() {
            issues.push(Issue {
                id: format!("unneeded:{key}"),
                severity: Severity::Info,
                kind: "unneeded",
                entry_key: Some(key.clone()),
                other_key: None,
                message: format!("{name} was added as a dependency, but nothing in the pack needs it now."),
                fix: Some(Fix::Remove { key: key.clone() }),
            });
        }
        let mut rb = rb;
        rb.sort();
        rb.dedup();
        entries.get_mut(key).unwrap().required_by = rb;
    }

    issues.sort_by(|a, b| a.severity.cmp(&b.severity).then(a.message.cmp(&b.message)));
    Analysis { entries, issues, suggestions: suggestions.into_values().collect(), complete }
}

/// Every file usable with (mc, loader), newest first — for the pin picker.
pub fn compatible_files(versions: &[Version], mc: &str, loader: Loader) -> Vec<FileInfo> {
    let mut v: Vec<&Version> = versions.iter().filter(|v| v.supports(mc, loader)).collect();
    v.sort_by(|a, b| b.date_published.cmp(&a.date_published));
    v.into_iter().map(|v| v.file_info(false)).collect()
}

// ================================================================ planning
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Addition {
    pub key: String,
    pub entry: PackEntry,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    /// Existing entry that gains a dependent.
    pub key: String,
    pub parent: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddPlan {
    /// Key of the requested mod, new or existing.
    pub root_key: String,
    pub additions: Vec<Addition>,
    pub links: Vec<Link>,
    /// Existing dependency entry the user has now added on purpose.
    pub promote: Option<String>,
    /// Existing entries that turned out to be the same mod as a dependency on
    /// the other site: record that site's id on them instead of adding a duplicate.
    pub id_links: Vec<IdLink>,
    pub suggestions: Vec<Suggestion>,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IdLink {
    pub key: String,
    pub modrinth_id: Option<String>,
    pub curseforge_id: Option<u64>,
}

/// Builds a ModRef for a catalog id (`cf:`-prefixed → CurseForge), plus an optional other-site id.
pub fn mod_ref_for(pid: &str, alt: Option<&str>) -> ModRef {
    let mut r = match curseforge::parse_id(pid) {
        Some(n) => ModRef { modrinth_id: None, curseforge_id: Some(n), preferred: Source::Curseforge },
        None => ModRef { modrinth_id: Some(pid.to_string()), curseforge_id: None, preferred: Source::Modrinth },
    };
    if let Some(a) = alt {
        match curseforge::parse_id(a) {
            Some(n) if r.curseforge_id.is_none() => r.curseforge_id = Some(n),
            None if r.modrinth_id.is_none() && !a.is_empty() => r.modrinth_id = Some(a.to_string()),
            _ => {}
        }
    }
    r
}

/// An existing entry that is this project under its other-site id: same slug
/// and same name, and no id of its own on this project's site.
fn same_mod_elsewhere(pack: &Pack, pid: &str, info: &ProjectInfo) -> Option<String> {
    let is_cf = curseforge::parse_id(pid).is_some();
    let (slug, name) = (norm(&info.slug), norm(&info.title));
    if slug.is_empty() || name.is_empty() {
        return None;
    }
    pack.entries
        .iter()
        .find(|(_, e)| {
            let free = if is_cf { e.mod_ref.curseforge_id.is_none() } else { e.mod_ref.modrinth_id.is_none() };
            free && norm(&e.display.slug) == slug && norm(&e.display.name) == name
        })
        .map(|(k, _)| k.clone())
}

struct Node {
    key: String,
    project_id: String,
    parents: Vec<String>,
    file_id: Option<String>,
    is_root: bool,
}

/// Works out everything that adding `project_id` brings with it: the mod,
/// its required dependencies (transitively, cycle-safe), links to mods
/// already in the pack, and optional suggestions. Nothing is changed here;
/// the UI applies the plan.
pub async fn plan_add(
    pack: &Pack,
    project_id: &str,
    pin_version: Option<&str>,
    for_parent: Option<&str>,
    alt_id: Option<&str>,
    cat: &dyn Catalog,
    force: bool,
    now_iso: &str,
    mut new_key: impl FnMut() -> String,
) -> AppResult<AddPlan> {
    let mc = pack.mc_version.as_str();
    let loader = pack.loader;
    let index = pack_index(pack);
    let mut notes = Vec::new();

    // Only a parent that's really in the pack counts.
    let for_parent = for_parent.filter(|k| pack.entries.contains_key(*k)).map(String::from);
    if let Some(k) = index.get(project_id).or_else(|| alt_id.and_then(|a| index.get(a))) {
        if let Some(parent) = for_parent {
            let links = if pack.entries[k].pulled_in_by.contains(&parent) { vec![] } else { vec![Link { key: k.clone(), parent }] };
            return Ok(AddPlan { root_key: k.clone(), additions: vec![], links, promote: None, id_links: vec![], suggestions: vec![], notes });
        }
        let promote = (pack.entries[k].reason != Reason::User).then(|| k.clone());
        return Ok(AddPlan { root_key: k.clone(), additions: vec![], links: vec![], promote, id_links: vec![], suggestions: vec![], notes });
    }

    let mut queue: VecDeque<(String, Option<String>, Option<String>)> = VecDeque::from([(project_id.to_string(), for_parent.clone(), pin_version.map(String::from))]);
    let mut planned: HashMap<String, usize> = HashMap::new();
    let mut nodes: Vec<Node> = Vec::new();
    let mut links: Vec<Link> = Vec::new();
    let mut optional: Vec<(String, String)> = Vec::new();
    let mut vcache: HashMap<String, Option<Version>> = HashMap::new();
    let mut hit_cap = false;
    let mut no_build: Vec<String> = Vec::new();
    let mut infos: HashMap<String, ProjectInfo> = HashMap::new();
    let mut id_links: Vec<IdLink> = Vec::new();
    let mut index = index;

    while let Some((pid, parent, pin)) = queue.pop_front() {
        // A dependency from the other site may already be in the pack under its other id.
        if !index.contains_key(&pid) && !planned.contains_key(&pid) && parent.is_some() {
            if let Some(info) = cat.projects(std::slice::from_ref(&pid), force).await?.into_iter().next() {
                if let Some(k) = same_mod_elsewhere(pack, &pid, &info) {
                    let (m, c) = match curseforge::parse_id(&pid) {
                        Some(n) => (None, Some(n)),
                        None => (Some(pid.clone()), None),
                    };
                    id_links.push(IdLink { key: k.clone(), modrinth_id: m, curseforge_id: c });
                    index.insert(pid.clone(), k);
                }
                infos.insert(pid.clone(), info);
            }
        }
        if let Some(existing) = index.get(&pid) {
            if let Some(p) = parent {
                let l = Link { key: existing.clone(), parent: p };
                if !links.contains(&l) {
                    links.push(l);
                }
            }
            continue;
        }
        if let Some(&i) = planned.get(&pid) {
            if let Some(p) = parent {
                if !nodes[i].parents.contains(&p) {
                    nodes[i].parents.push(p);
                }
            }
            continue;
        }
        if nodes.len() >= MAX_NEW_ENTRIES {
            hit_cap = true;
            continue;
        }
        let key = new_key();
        planned.insert(pid.clone(), nodes.len());
        nodes.push(Node { key: key.clone(), project_id: pid.clone(), parents: parent.into_iter().collect(), file_id: None, is_root: nodes.is_empty() });
        let idx = nodes.len() - 1;

        // Honour a dependency's exact-version pin only when that version fits the pack.
        let mut version = None;
        if let Some(vid) = &pin {
            if let Some(v) = cat.version(vid, force).await? {
                if v.project_id == pid && v.supports(mc, loader) {
                    nodes[idx].file_id = Some(v.id.clone());
                    version = Some(v);
                }
            }
        }
        if version.is_none() {
            let vs = cat.versions(&pid, mc, loader, force).await?;
            version = select_version(&vs, mc, loader, pack.channel_policy);
        }
        let Some(mut v) = version else {
            no_build.push(pid.clone());
            continue;
        };
        resolve_dep_projects(cat, &mut v.dependencies, force, &mut vcache).await;
        for d in &v.dependencies {
            let Some(dpid) = d.project_id.clone() else { continue };
            if dpid == pid {
                continue;
            }
            match d.dependency_type {
                DepType::Required => queue.push_back((dpid, Some(key.clone()), d.version_id.clone())),
                DepType::Optional => optional.push((dpid, key.clone())),
                _ => {}
            }
        }
    }
    if hit_cap {
        notes.push(format!("Stopped after adding {MAX_NEW_ENTRIES} mods; the rest show up as missing dependencies."));
    }

    // Display info for everything new plus suggestion targets (some already fetched above).
    let mut ids: Vec<String> = nodes.iter().map(|n| n.project_id.clone()).filter(|id| !infos.contains_key(id)).collect();
    let dismissed: HashSet<&String> = pack.dismissed_suggestions.iter().collect();
    for (o, _) in &optional {
        if !index.contains_key(o) && !planned.contains_key(o) && !dismissed.contains(o) && !ids.contains(o) && !infos.contains_key(o) {
            ids.push(o.clone());
        }
    }
    if !ids.is_empty() {
        infos.extend(cat.projects(&ids, force).await?.into_iter().map(|p| (p.id.clone(), p)));
    }
    if !infos.contains_key(project_id) {
        return Err(AppError::Invalid("That project couldn't be found.".into()));
    }

    for pid in &no_build {
        let name = infos.get(pid).map(|p| p.display().name).unwrap_or_else(|| pid.clone());
        notes.push(format!("{name} has no build for Minecraft {mc} · {}.", loader_name(loader)));
    }

    // Dependencies that don't exist on Modrinth any more are reported, not added.
    let ghosts: HashSet<String> = nodes.iter().filter(|n| !n.is_root && !infos.contains_key(&n.project_id)).map(|n| n.key.clone()).collect();
    for n in nodes.iter().filter(|n| ghosts.contains(&n.key)) {
        notes.push(format!("A required dependency ({}) no longer exists.", n.project_id));
    }
    let mut additions = Vec::new();
    for n in nodes.iter().filter(|n| !ghosts.contains(&n.key)) {
        let display = infos.get(&n.project_id).map(|p| p.display()).unwrap_or_else(|| EntryDisplay { name: n.project_id.clone(), ..Default::default() });
        additions.push(Addition {
            key: n.key.clone(),
            entry: PackEntry {
                mod_ref: mod_ref_for(&n.project_id, if n.is_root { alt_id } else { None }),
                // Pins are stored without the `cf:` prefix.
                file_id: n.file_id.as_ref().map(|f| f.strip_prefix(curseforge::PREFIX).unwrap_or(f).to_string()),
                kind: EntryKind::Mod,
                reason: if n.is_root && for_parent.is_none() { Reason::User } else { Reason::Dependency },
                pulled_in_by: n.parents.iter().filter(|p| !ghosts.contains(*p)).cloned().collect(),
                payload_ids: vec![],
                tags: vec![],
                notes: None,
                side: None,
                graph_pos: None,
                display,
                library_override: None,
                added_at: now_iso.to_string(),
                extra: Default::default(),
            },
        });
    }

    let mut suggestions: BTreeMap<String, Suggestion> = BTreeMap::new();
    for (o, by) in optional {
        if index.contains_key(&o) || planned.contains_key(&o) || dismissed.contains(&o) {
            continue;
        }
        let Some(info) = infos.get(&o) else { continue };
        suggestions
            .entry(o.clone())
            .or_insert_with(|| Suggestion {
                project_id: o.clone(),
                name: info.display().name,
                summary: info.description.clone(),
                icon_url: info.icon_url.clone(),
                suggested_by: vec![],
            })
            .suggested_by
            .push(by);
    }

    Ok(AddPlan { root_key: nodes[0].key.clone(), additions, links, promote: None, id_links, suggestions: suggestions.into_values().collect(), notes })
}

// ================================================================== tests
#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::model::tests::pack as base_pack;
    use std::sync::Mutex;

    /// Catalog backed by recorded Modrinth responses plus synthetic cases.
    pub(crate) struct Fixture {
        pub(crate) versions: HashMap<String, Vec<Version>>,
        by_id: HashMap<String, Version>,
        projects: HashMap<String, ProjectInfo>,
        slugs: HashMap<String, String>,
        calls: Mutex<usize>,
    }

    #[derive(Deserialize)]
    struct Bundle {
        #[serde(default)]
        versions: HashMap<String, Vec<Version>>,
        #[serde(default, rename = "versionById")]
        version_by_id: HashMap<String, Version>,
        #[serde(default)]
        projects: HashMap<String, ProjectInfo>,
        #[serde(default)]
        slugs: HashMap<String, String>,
    }

    impl Fixture {
        pub(crate) fn load() -> Self {
            let mut f = Fixture { versions: HashMap::new(), by_id: HashMap::new(), projects: HashMap::new(), slugs: HashMap::new(), calls: Mutex::new(0) };
            for text in [
                include_str!("../tests/fixtures/resolver/modrinth.json"),
                include_str!("../tests/fixtures/resolver/synthetic.json"),
                include_str!("../tests/fixtures/resolver/curseforge.json"),
            ] {
                let b: Bundle = serde_json::from_str(text).expect("fixture");
                for (k, vs) in b.versions {
                    for v in &vs {
                        f.by_id.insert(v.id.clone(), v.clone());
                    }
                    f.versions.entry(k).or_default().extend(vs);
                }
                f.by_id.extend(b.version_by_id);
                for (id, mut p) in b.projects {
                    if p.id.is_empty() {
                        p.id = id.clone();
                    }
                    f.projects.insert(id, p);
                }
                f.slugs.extend(b.slugs);
            }
            // Slugs resolve to the Modrinth copy when both sites share one.
            for (id, p) in &f.projects {
                if id.starts_with(curseforge::PREFIX) {
                    f.slugs.entry(p.slug.clone()).or_insert_with(|| id.clone());
                } else {
                    f.slugs.insert(p.slug.clone(), id.clone());
                }
            }
            f
        }
        pub(crate) fn id(&self, slug: &str) -> String {
            self.slugs.get(slug).cloned().unwrap_or_else(|| panic!("no fixture slug {slug}"))
        }
        fn versions_for(&self, slug: &str, mc: &str, loader: Loader) -> Vec<Version> {
            self.versions.get(&format!("{}|{mc}", self.id(slug))).cloned().unwrap_or_default().into_iter().filter(|v| v.supports(mc, loader)).collect()
        }
    }

    #[async_trait::async_trait]
    impl crate::import::Lookup for Fixture {
        async fn modrinth_by_sha1(&self, hashes: &[String]) -> AppResult<HashMap<String, Version>> {
            let want: HashSet<&String> = hashes.iter().collect();
            let mut out = HashMap::new();
            for v in self.by_id.values().filter(|v| !v.id.starts_with(curseforge::PREFIX)) {
                for f in &v.files {
                    if let Some(h) = f.hashes.get("sha1").filter(|h| want.contains(h)) {
                        out.insert(h.clone(), v.clone());
                    }
                }
            }
            Ok(out)
        }
        async fn curseforge_by_fingerprint(&self, _fps: &[u32]) -> AppResult<HashMap<u32, Version>> {
            Ok(HashMap::new())
        }
    }

    #[async_trait::async_trait]
    impl Catalog for Fixture {
        async fn versions(&self, project_id: &str, mc: &str, _loader: Loader, _force: bool) -> AppResult<Vec<Version>> {
            *self.calls.lock().unwrap() += 1;
            Ok(self.versions.get(&format!("{project_id}|{mc}")).cloned().unwrap_or_default())
        }
        async fn version(&self, id: &str, _force: bool) -> AppResult<Option<Version>> {
            Ok(self.by_id.get(id).cloned())
        }
        async fn projects(&self, ids: &[String], _force: bool) -> AppResult<Vec<ProjectInfo>> {
            Ok(ids.iter().filter_map(|i| self.projects.get(i).cloned()).collect())
        }
    }

    pub(crate) const NOW: i64 = 1_795_478_400; // 2026-11-24

    pub(crate) fn pack_for(mc: &str, loader: Loader) -> Pack {
        let mut p = base_pack();
        p.mc_version = mc.into();
        p.loader = loader;
        p
    }

    pub(crate) fn counter() -> impl FnMut() -> String {
        let mut n = 0;
        move || {
            n += 1;
            format!("k{n}")
        }
    }

    /// Applies a plan the way the UI does, so tests can chain operations.
    pub(crate) fn apply(pack: &mut Pack, plan: AddPlan) {
        for a in plan.additions {
            pack.entries.insert(a.key, a.entry);
        }
        for l in plan.links {
            if let Some(e) = pack.entries.get_mut(&l.key) {
                if !e.pulled_in_by.contains(&l.parent) {
                    e.pulled_in_by.push(l.parent);
                }
            }
        }
        if let Some(k) = plan.promote {
            pack.entries.get_mut(&k).unwrap().reason = Reason::User;
        }
        for l in plan.id_links {
            let e = pack.entries.get_mut(&l.key).unwrap();
            if l.modrinth_id.is_some() {
                e.mod_ref.modrinth_id = l.modrinth_id;
            }
            if l.curseforge_id.is_some() {
                e.mod_ref.curseforge_id = l.curseforge_id;
            }
        }
    }

    pub(crate) async fn add(f: &Fixture, pack: &mut Pack, slug: &str, keys: &mut impl FnMut() -> String) -> Vec<String> {
        let plan = plan_add(pack, &f.id(slug), None, None, None, f, false, "2026-11-24T00:00:00Z", keys).await.unwrap();
        let notes = plan.notes.clone();
        apply(pack, plan);
        notes
    }

    pub(crate) fn key_of(pack: &Pack, f: &Fixture, slug: &str) -> String {
        let id = f.id(slug);
        pack.entries.iter().find(|(_, e)| e.mod_ref.modrinth_id.as_deref() == Some(&id)).map(|(k, _)| k.clone()).unwrap_or_else(|| panic!("{slug} not in pack"))
    }

    fn kinds(a: &Analysis) -> Vec<&'static str> {
        a.issues.iter().map(|i| i.kind).collect()
    }

    pub(crate) fn block<T>(f: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(f)
    }

    // ---------------------------------------------------------- selection
    #[test]
    fn picks_newest_release_then_beta() {
        let f = Fixture::load();
        // JEI on 1.21.1 publishes betas only: the newest beta wins.
        let jei = f.versions_for("jei", "1.21.1", Loader::Neoforge);
        assert!(!jei.is_empty() && jei.iter().all(|v| v.version_type == "beta"));
        let newest = jei.iter().max_by(|a, b| a.date_published.cmp(&b.date_published)).unwrap();
        assert_eq!(select_version(&jei, "1.21.1", Loader::Neoforge, ChannelPolicy::Stable).unwrap().id, newest.id);
        // JEI on 26.1.2 has one release and newer betas (recorded): the release wins.
        let jei = f.versions_for("jei", "26.1.2", Loader::Neoforge);
        let release = jei.iter().filter(|v| v.version_type == "release").max_by(|a, b| a.date_published.cmp(&b.date_published));
        let picked = select_version(&jei, "26.1.2", Loader::Neoforge, ChannelPolicy::Stable).unwrap();
        match release {
            Some(r) => assert_eq!(picked.id, r.id),
            None => assert_eq!(picked.version_type, "beta"),
        }
        // A release beats a newer beta and a newer alpha.
        let ch = f.versions_for("channel-test", "1.21.1", Loader::Fabric);
        assert_eq!(select_version(&ch, "1.21.1", Loader::Fabric, ChannelPolicy::Stable).unwrap().id, "chRelFabric");
        // On Quilt the native Quilt release beats a newer Fabric release.
        let chq = f.versions_for("channel-test", "1.21.1", Loader::Quilt);
        assert_eq!(select_version(&chq, "1.21.1", Loader::Quilt, ChannelPolicy::Stable).unwrap().id, "chRelQuilt");
        // Never a build for another loader or version.
        let sodium = f.versions.get(&format!("{}|1.21.1", f.id("sodium"))).unwrap();
        let s = select_version(sodium, "1.21.1", Loader::Neoforge, ChannelPolicy::Stable).unwrap();
        assert!(s.loaders.contains(&"neoforge".to_string()) && s.game_versions.contains(&"1.21.1".to_string()));
        assert!(select_version(sodium, "1.20.1", Loader::Neoforge, ChannelPolicy::Stable).is_none());
    }

    #[test]
    fn newest_policy_ignores_channel() {
        let f = Fixture::load();
        let ch = f.versions_for("channel-test", "1.21.1", Loader::Fabric);
        assert_eq!(select_version(&ch, "1.21.1", Loader::Fabric, ChannelPolicy::Newest).unwrap().id, "chAlpha");
        assert_eq!(select_version(&ch, "1.21.1", Loader::Fabric, ChannelPolicy::Stable).unwrap().id, "chRelFabric");
        // Recorded JEI 26.1.2: Stable keeps the release, Newest takes the newest beta.
        let jei = f.versions_for("jei", "26.1.2", Loader::Neoforge);
        let newest = jei.iter().max_by(|a, b| a.date_published.cmp(&b.date_published)).unwrap();
        assert_eq!(select_version(&jei, "26.1.2", Loader::Neoforge, ChannelPolicy::Newest).unwrap().id, newest.id);
        // The pack setting reaches analysis, and a stale-looking old release isn't picked over a fresh beta.
        let mut pack = pack_for("26.1.2", Loader::Neoforge);
        let mut keys = counter();
        block(add(&f, &mut pack, "jei", &mut keys));
        let jk = key_of(&pack, &f, "jei");
        let stable = block(analyze(&pack, &f, false, NOW)).entries[&jk].file.clone().unwrap();
        pack.channel_policy = ChannelPolicy::Newest;
        let latest = block(analyze(&pack, &f, false, NOW)).entries[&jk].file.clone().unwrap();
        assert_eq!(latest.version_id, newest.id);
        if jei.iter().any(|v| v.version_type == "release") {
            assert_eq!(stable.version_type, "release");
            assert_ne!(stable.version_id, latest.version_id);
        }
    }

    // ---------------------------------------------------------- planning
    #[test]
    fn adding_pulls_in_required_and_suggests_optional() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let plan = block(plan_add(&pack, &f.id("sodium-extra"), None, None, None, &f, false, "t", counter())).unwrap();
        let root = &plan.additions[0];
        assert_eq!(root.entry.reason, Reason::User);
        let sodium = plan.additions.iter().find(|a| a.entry.mod_ref.modrinth_id.as_deref() == Some(f.id("sodium").as_str())).expect("sodium pulled in");
        assert_eq!(sodium.entry.reason, Reason::Dependency);
        assert_eq!(sodium.entry.pulled_in_by, vec![root.key.clone()]);
        assert_eq!(sodium.entry.display.name, "Sodium");
        let sugg: Vec<&str> = plan.suggestions.iter().map(|s| s.name.as_str()).collect();
        assert!(sugg.iter().any(|n| n.contains("Iris")), "{sugg:?}");
        apply(&mut pack, plan);
        // Adding again is a no-op.
        let again = block(plan_add(&pack, &f.id("sodium-extra"), None, None, None, &f, false, "t", counter())).unwrap();
        assert!(again.additions.is_empty() && again.promote.is_none());
    }

    #[test]
    fn existing_dependency_links_instead_of_duplicating() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "sodium-extra", &mut keys));
        let n = pack.entries.len();
        block(add(&f, &mut pack, "reeses-sodium-options", &mut keys));
        assert_eq!(pack.entries.len(), n + 1, "Sodium must not be added twice");
        let sodium = &pack.entries[&key_of(&pack, &f, "sodium")];
        assert_eq!(sodium.pulled_in_by.len(), 2);
        // Adding a mod that's already a dependency promotes it to user-added.
        let plan = block(plan_add(&pack, &f.id("sodium"), None, None, None, &f, false, "t", counter())).unwrap();
        assert_eq!(plan.promote.as_deref(), Some(key_of(&pack, &f, "sodium").as_str()));
    }

    #[test]
    fn honours_compatible_exact_version_pins() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        // Iris pins an exact Sodium build; when that build fits the pack, the new entry is pinned to it.
        let iris = select_version(&f.versions_for("iris", "1.21.1", Loader::Fabric), "1.21.1", Loader::Fabric, ChannelPolicy::Stable).unwrap();
        let pin = iris.dependencies.iter().find(|d| d.project_id.as_deref() == Some(f.id("sodium").as_str())).and_then(|d| d.version_id.clone());
        block(add(&f, &mut pack, "iris", &mut keys));
        let sodium = &pack.entries[&key_of(&pack, &f, "sodium")];
        match pin {
            Some(vid) if f.by_id.get(&vid).is_some_and(|v| v.supports("1.21.1", Loader::Fabric)) => assert_eq!(sodium.file_id.as_deref(), Some(vid.as_str())),
            _ => assert_eq!(sodium.file_id, None),
        }
        // A dependency named only by version id resolves to its project (and pins).
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        block(add(&f, &mut pack, "version-only-host", &mut keys));
        let s = &pack.entries[&key_of(&pack, &f, "sodium")];
        assert_eq!(s.file_id.as_deref(), Some("SMxNOGZ6"));
    }

    #[test]
    fn transitive_deps_resolve_unpinned_to_newest() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "modmenu", &mut keys));
        let names: Vec<&str> = pack.entries.values().map(|e| e.display.name.as_str()).collect();
        assert!(names.contains(&"Fabric API"), "{names:?}");
        assert!(pack.entries.values().filter(|e| e.reason == Reason::Dependency).all(|e| e.file_id.is_none()));
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(!kinds(&a).contains(&"missing_dependency"), "{:?}", a.issues);
        assert!(a.complete);
    }

    #[test]
    fn circular_required_deps_terminate() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let plan = block(plan_add(&pack, "cycA0000", None, None, None, &f, false, "t", counter())).unwrap();
        assert_eq!(plan.additions.len(), 2);
        let a = &plan.additions[0];
        let b = &plan.additions[1];
        assert_eq!(b.entry.pulled_in_by, vec![a.key.clone()]);
        assert_eq!(a.entry.pulled_in_by, vec![b.key.clone()]);
        apply(&mut pack, plan);
        let an = block(analyze(&pack, &f, false, NOW));
        assert!(!kinds(&an).contains(&"missing_dependency"));
        // Removing the user-added mod orphans its cycle partner.
        let ka = key_of(&pack, &f, "cycle-a");
        assert_eq!(crate::model::plan_removal(&pack, &[ka]), vec![key_of(&pack, &f, "cycle-b")]);
    }

    #[test]
    fn embedded_deps_are_not_added_or_flagged() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "embed-host", &mut keys));
        assert!(pack.entries.values().all(|e| e.mod_ref.modrinth_id.as_deref() != Some("embLib00")));
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(!kinds(&a).contains(&"missing_dependency"), "{:?}", a.issues);
    }

    #[test]
    fn no_build_is_noted_and_flagged() {
        let f = Fixture::load();
        let mut pack = pack_for("26.1.2", Loader::Neoforge);
        let mut keys = counter();
        let notes = block(add(&f, &mut pack, "embeddium", &mut keys));
        assert!(notes.iter().any(|n| n.contains("no build")), "{notes:?}");
        let a = block(analyze(&pack, &f, false, NOW));
        let issue = a.issues.iter().find(|i| i.kind == "no_build").expect("no_build issue");
        assert_eq!(issue.severity, Severity::Error);
        assert!(issue.message.contains("26.1.2"));
    }

    #[test]
    fn deleted_dependency_is_reported_not_added() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        let notes = block(add(&f, &mut pack, "ghost-host", &mut keys));
        assert_eq!(pack.entries.len(), 1, "a project that doesn't exist must not become an entry");
        assert!(notes.iter().any(|n| n.contains("no longer exists")), "{notes:?}");
        let a = block(analyze(&pack, &f, false, NOW));
        let i = a.issues.iter().find(|i| i.kind == "missing_dependency").unwrap();
        assert!(i.message.contains("no longer exists") && i.fix.is_none(), "{i:?}");
    }

    // ---------------------------------------------------------- checks
    #[test]
    fn missing_dependency_has_add_fix() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "sodium-extra", &mut keys));
        let sk = key_of(&pack, &f, "sodium");
        pack.entries.remove(&sk);
        let a = block(analyze(&pack, &f, false, NOW));
        let i = a.issues.iter().find(|i| i.kind == "missing_dependency").expect("missing");
        assert_eq!(i.fix, Some(Fix::AddProject { project_id: f.id("sodium"), name: "Sodium".into(), pin_version: None, for_key: Some(key_of(&pack, &f, "sodium-extra")) }));
        // Applying that fix records Sodium as Sodium Extra's dependency, not a user pick.
        let ek = key_of(&pack, &f, "sodium-extra");
        let plan = block(plan_add(&pack, &f.id("sodium"), None, Some(&ek), None, &f, false, "t", counter())).unwrap();
        assert_eq!(plan.additions[0].entry.reason, Reason::Dependency);
        assert_eq!(plan.additions[0].entry.pulled_in_by, vec![ek.clone()]);
        assert!(a.suggestions.iter().any(|s| s.name.contains("Iris")));
        // Dismissed suggestions stay dismissed.
        pack.dismissed_suggestions.push(f.id("iris"));
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(!a.suggestions.iter().any(|s| s.name.contains("Iris")));
    }

    #[test]
    fn incompatibility_is_flagged_once() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Neoforge);
        let mut keys = counter();
        block(add(&f, &mut pack, "embeddium", &mut keys));
        block(add(&f, &mut pack, "rubidium", &mut keys));
        let a = block(analyze(&pack, &f, false, NOW));
        let inc: Vec<&Issue> = a.issues.iter().filter(|i| i.kind == "incompatible").collect();
        assert_eq!(inc.len(), 1, "{:?}", a.issues);
        assert_eq!(inc[0].other_key.as_deref(), Some(key_of(&pack, &f, "rubidium").as_str()));
        assert!(kinds(&a).contains(&"no_build"), "Rubidium has no 1.21.1 build");
    }

    #[test]
    fn version_mismatch_offers_pin() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "version-only-host", &mut keys));
        let sk = key_of(&pack, &f, "sodium");
        // The user floated Sodium to the newest build; the host still asks for SMxNOGZ6.
        pack.entries.get_mut(&sk).unwrap().file_id = None;
        let newest = select_version(&f.versions_for("sodium", "1.21.1", Loader::Fabric), "1.21.1", Loader::Fabric, ChannelPolicy::Stable).unwrap();
        let a = block(analyze(&pack, &f, false, NOW));
        let mm = a.issues.iter().find(|i| i.kind == "version_mismatch");
        if newest.id == "SMxNOGZ6" {
            assert!(mm.is_none());
        } else {
            let mm = mm.expect("mismatch warning");
            assert!(matches!(&mm.fix, Some(Fix::PinTo { version_id, .. }) if version_id == "SMxNOGZ6"));
        }
    }

    #[test]
    fn pinned_file_for_another_version_is_an_error() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Neoforge);
        let mut keys = counter();
        block(add(&f, &mut pack, "jei", &mut keys));
        let jk = key_of(&pack, &f, "jei");
        let other = &f.versions_for("jei", "26.1.2", Loader::Neoforge)[0];
        pack.entries.get_mut(&jk).unwrap().file_id = Some(other.id.clone());
        let a = block(analyze(&pack, &f, false, NOW));
        let i = a.issues.iter().find(|i| i.kind == "pin_mismatch").expect("pin mismatch");
        assert_eq!(i.fix, Some(Fix::Unpin { key: jk.clone() }));
        pack.entries.get_mut(&jk).unwrap().file_id = Some("doesNotExist".into());
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(kinds(&a).contains(&"pin_missing"));
    }

    #[test]
    fn quilt_standard_libraries_provide_fabric_api() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Quilt);
        let mut keys = counter();
        block(add(&f, &mut pack, "qsl", &mut keys));
        block(add(&f, &mut pack, "quilt-mod", &mut keys));
        assert!(pack.entries.values().all(|e| e.display.name != "Fabric API"), "QSL already provides Fabric API");
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(!kinds(&a).contains(&"missing_dependency"), "{:?}", a.issues);
        let qk = key_of(&pack, &f, "qsl");
        assert_eq!(a.entries[&qk].required_by, vec![key_of(&pack, &f, "quilt-mod")]);
    }

    #[test]
    fn soft_checks_side_stale_unneeded() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "old-mod", &mut keys));
        block(add(&f, &mut pack, "sodium-extra", &mut keys));
        let a = block(analyze(&pack, &f, false, NOW));
        let k = kinds(&a);
        assert!(k.contains(&"stale"), "{k:?}");
        assert!(k.contains(&"server_only"), "{k:?}");
        assert!(k.contains(&"client_only"), "Sodium is client-only: {k:?}");
        assert!(!k.contains(&"unneeded"));
        // Remove Sodium Extra but keep Sodium: Sodium is now unneeded.
        let ek = key_of(&pack, &f, "sodium-extra");
        pack.entries.remove(&ek);
        crate::model::normalize(&mut pack).unwrap();
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(kinds(&a).contains(&"unneeded"));
        // Errors sort before warnings before info.
        let sev: Vec<Severity> = a.issues.iter().map(|i| i.severity).collect();
        let mut sorted = sev.clone();
        sorted.sort();
        assert_eq!(sev, sorted);
    }

    #[test]
    fn curseforge_mods_resolve_and_link_across_sites() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Neoforge);
        let mut keys = counter();
        block(add(&f, &mut pack, "architectury-api", &mut keys)); // Modrinth copy
        let arch = key_of(&pack, &f, "architectury-api");
        assert_eq!(pack.entries[&arch].mod_ref.curseforge_id, None);

        let plan = block(plan_add(&pack, "cf:1001", None, None, None, &f, false, "t", &mut keys)).unwrap();
        let names: Vec<&str> = plan.additions.iter().map(|a| a.entry.display.name.as_str()).collect();
        assert_eq!(names, vec!["CF Tech Mod", "CF Only Lib"], "Architectury must not be added twice");
        let root = &plan.additions[0];
        assert_eq!(root.entry.mod_ref.curseforge_id, Some(1001));
        assert_eq!(root.entry.mod_ref.preferred, Source::Curseforge);
        assert_eq!(plan.id_links, vec![IdLink { key: arch.clone(), modrinth_id: None, curseforge_id: Some(2001) }]);
        assert!(plan.links.contains(&Link { key: arch.clone(), parent: root.key.clone() }));
        let root_key = root.key.clone();
        apply(&mut pack, plan);
        assert_eq!(pack.entries[&arch].mod_ref.curseforge_id, Some(2001));

        let a = block(analyze(&pack, &f, false, NOW));
        assert!(a.complete, "{:?}", a.issues);
        assert!(!kinds(&a).contains(&"missing_dependency"), "{:?}", a.issues);
        assert_eq!(a.entries[&arch].required_by, vec![root_key.clone()]);
        let dist = a.issues.iter().find(|i| i.kind == "cf_distribution").expect("distribution warning");
        assert_eq!(dist.fix, None, "no Modrinth copy to switch to");
        assert_eq!(a.entries[&root_key].file.as_ref().unwrap().version_id, "cf:9001");

        // Once a Modrinth copy is linked, the warning offers to switch.
        pack.entries.get_mut(&root_key).unwrap().mod_ref.modrinth_id = Some("someMRid".into());
        let a = block(analyze(&pack, &f, false, NOW));
        let dist = a.issues.iter().find(|i| i.kind == "cf_distribution").unwrap();
        assert_eq!(dist.fix, Some(Fix::UseSource { key: root_key.clone(), source: Source::Modrinth }));

        // A CurseForge pin stored without the prefix still resolves.
        pack.entries.get_mut(&root_key).unwrap().file_id = Some("9001".into());
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(a.entries[&root_key].file.as_ref().unwrap().pinned);
        assert!(!kinds(&a).contains(&"pin_missing"));
    }

    /// Runs on real CurseForge data once `cargo run --example record_cf_fixtures` has recorded it.
    #[test]
    fn recorded_curseforge_jei_resolves() {
        let f = Fixture::load();
        let Some(vs) = f.versions.get("cf:238222|1.21.1") else {
            eprintln!("(no recorded CurseForge data yet; skipping)");
            return;
        };
        assert!(!vs.is_empty());
        let mut pack = pack_for("1.21.1", Loader::Neoforge);
        let mut keys = counter();
        let plan = block(plan_add(&pack, "cf:238222", None, None, None, &f, false, "t", &mut keys)).unwrap();
        assert_eq!(plan.additions[0].entry.mod_ref.curseforge_id, Some(238222));
        apply(&mut pack, plan);
        let a = block(analyze(&pack, &f, false, NOW));
        assert!(a.complete, "{:?}", a.issues);
        assert!(!kinds(&a).contains(&"no_build"), "{:?}", a.issues);
        assert!(!kinds(&a).contains(&"missing_dependency"), "{:?}", a.issues);
    }

    #[test]
    fn alt_id_links_both_sites_on_add() {
        let f = Fixture::load();
        let pack = pack_for("1.21.1", Loader::Neoforge);
        let plan = block(plan_add(&pack, "lhGA9TYQ", None, None, Some("cf:2001"), &f, false, "t", counter())).unwrap();
        let r = &plan.additions[0].entry.mod_ref;
        assert_eq!((r.modrinth_id.as_deref(), r.curseforge_id, r.preferred), (Some("lhGA9TYQ"), Some(2001), Source::Modrinth));
        // Adding the CurseForge copy of something already in the pack is a no-op.
        let mut pack = pack;
        apply(&mut pack, plan);
        let again = block(plan_add(&pack, "cf:2001", None, None, None, &f, false, "t", counter())).unwrap();
        assert!(again.additions.is_empty());
    }

    #[test]
    fn known_conflicts_from_jar_metadata_are_flagged() {
        // Found in the Prism round-trip: Sodium + Embeddium both load, Modrinth lists no conflict.
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Neoforge);
        let mut keys = counter();
        block(add(&f, &mut pack, "embeddium", &mut keys));
        block(add(&f, &mut pack, "sodium", &mut keys));
        let a = block(analyze(&pack, &f, false, NOW));
        let inc: Vec<&Issue> = a.issues.iter().filter(|i| i.kind == "incompatible").collect();
        assert_eq!(inc.len(), 1, "{:?}", a.issues);
        assert!(inc[0].message.contains("won't load together"), "{}", inc[0].message);
        assert!(known_conflicts().conflicts.iter().all(|c| !c.source.is_empty()));
    }

    #[test]
    fn age_parsing() {
        assert_eq!(iso_age_secs("2026-11-24T00:00:00Z", NOW), Some(0));
        assert_eq!(iso_age_secs("2025-11-24T00:00:00Z", NOW), Some(365 * 86_400));
        assert_eq!(iso_age_secs("garbage", NOW), None);
    }
}
