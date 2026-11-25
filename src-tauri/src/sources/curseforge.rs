//! CurseForge API client (https://docs.curseforge.com/rest-api/).
//! Needs the user's API key, read from the OS keychain (and then kept in
//! memory until it changes). Inside the resolver CurseForge ids carry a `cf:`
//! prefix (`cf:238222` for a mod, `cf:5678901` for a file) so one graph can
//! hold mods from both sites; pack entries store the plain numbers.

use super::{Loader, ModSource, SearchHit, SearchPage, SearchQuery, Sort, Source};
use crate::error::{AppError, AppResult};
use crate::fetch::{status_help, Fetcher, DAY};
use crate::keychain;
use crate::resolve::{Catalog, Dep, DepType, ProjectInfo, VFile, Version};
use crate::sources::modrinth::{License, ProjectDetails};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const BASE: &str = "https://api.curseforge.com";
const NAME: &str = "CurseForge";
pub const MINECRAFT_GAME_ID: u32 = 432;
pub const CLASS_MODS: u32 = 6;
/// CurseForge class ids for the other content types (Minecraft game 432).
/// Same numbers as Prism Launcher's `g_classIDMappings` (FlameAPI.cpp); the
/// owner's searches of each type with CurseForge on matched too.
pub const CLASS_RESOURCE_PACKS: u32 = 12;
pub const CLASS_SHADERS: u32 = 6552;
pub const CLASS_DATAPACKS: u32 = 6945;

pub fn class_for(kind: crate::model::EntryKind) -> u32 {
    use crate::model::EntryKind::*;
    match kind {
        Mod => CLASS_MODS,
        Resourcepack => CLASS_RESOURCE_PACKS,
        Shader => CLASS_SHADERS,
        Datapack => CLASS_DATAPACKS,
    }
}
pub const PREFIX: &str = "cf:";

pub fn loader_type(l: Loader) -> u32 {
    match l {
        Loader::Forge => 1,
        Loader::Fabric => 4,
        Loader::Quilt => 5,
        Loader::Neoforge => 6,
    }
}

fn loader_from_type(t: u32) -> Option<&'static str> {
    match t {
        1 => Some("forge"),
        4 => Some("fabric"),
        5 => Some("quilt"),
        6 => Some("neoforge"),
        _ => None,
    }
}

fn sort_field(s: Sort) -> u32 {
    match s {
        Sort::Relevance | Sort::Follows => 2, // popularity
        Sort::Updated => 3,
        Sort::Downloads => 6,
        Sort::Newest => 11, // release date
    }
}

/// `cf:123` → 123
pub fn parse_id(s: &str) -> Option<u64> {
    s.strip_prefix(PREFIX)?.parse().ok()
}
pub fn cf_id(n: u64) -> String {
    format!("{PREFIX}{n}")
}

// ---------------------------------------------------------------- categories
/// Modrinth category slug ↔ CurseForge category names. The app's filters and
/// grouping speak Modrinth slugs; CurseForge names map onto them where they
/// mean the same thing, and otherwise become their own slug.
const CATEGORY_MAP: &[(&str, &[&str])] = &[
    ("adventure", &["Adventure and RPG"]),
    ("decoration", &["Cosmetic"]),
    ("equipment", &["Armor, Tools, and Weapons"]),
    ("food", &["Food"]),
    ("library", &["API and Library"]),
    ("magic", &["Magic"]),
    ("management", &["Server Utility"]),
    ("mobs", &["Mobs"]),
    ("optimization", &["Performance"]),
    ("storage", &["Storage"]),
    ("technology", &["Technology"]),
    ("transportation", &["Player Transport"]),
    ("utility", &["Utility & QoL", "Utility &amp; QoL"]),
    ("worldgen", &["World Gen"]),
];

pub fn slugify(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

/// CurseForge category name → the app's category slug.
pub fn category_slug(cf_name: &str) -> String {
    for (slug, names) in CATEGORY_MAP {
        if names.iter().any(|n| n.eq_ignore_ascii_case(cf_name)) {
            return (*slug).to_string();
        }
    }
    slugify(cf_name)
}

fn cf_names_for(slug: &str) -> &'static [&'static str] {
    CATEGORY_MAP.iter().find(|(s, _)| *s == slug).map(|(_, n)| *n).unwrap_or(&[])
}

// ---------------------------------------------------------------- wire types
// Lenient: everything defaulted, the API leaves fields out freely.
#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
    pagination: Option<Pagination>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    #[serde(default)]
    total_count: u64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Links {
    website_url: Option<String>,
    wiki_url: Option<String>,
    issues_url: Option<String>,
    source_url: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct FileIndex {
    game_version: String,
    mod_loader: Option<u32>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Mod {
    id: u64,
    slug: String,
    name: String,
    summary: String,
    download_count: f64,
    logo: Option<Logo>,
    links: Links,
    date_modified: Option<String>,
    date_released: Option<String>,
    categories: Vec<Named>,
    authors: Vec<Named>,
    latest_files_indexes: Vec<FileIndex>,
    class_id: Option<u32>,
    allow_mod_distribution: Option<bool>,
    thumbs_up_count: u64,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Logo {
    thumbnail_url: Option<String>,
}
#[derive(Deserialize, Default)]
#[serde(default)]
struct Named {
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Hash {
    value: String,
    algo: u32,
}
#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct FileDep {
    mod_id: u64,
    relation_type: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct File {
    id: u64,
    mod_id: u64,
    #[serde(default = "yes")]
    is_available: bool,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    file_name: String,
    #[serde(default = "one")]
    release_type: u32,
    #[serde(default)]
    file_date: String,
    #[serde(default)]
    file_length: u64,
    #[serde(default)]
    download_url: Option<String>,
    #[serde(default)]
    hashes: Vec<Hash>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    dependencies: Vec<FileDep>,
    #[serde(default)]
    is_server_pack: bool,
    #[serde(default)]
    file_fingerprint: u32,
}
fn yes() -> bool {
    true
}
fn one() -> u32 {
    1
}

const LOADER_NAMES: &[&str] = &["forge", "neoforge", "fabric", "quilt"];

impl File {
    pub fn into_version(self) -> Version {
        let mut loaders = Vec::new();
        let mut games = Vec::new();
        for g in &self.game_versions {
            let l = g.to_lowercase();
            if LOADER_NAMES.contains(&l.as_str()) {
                loaders.push(l);
            } else if g.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                games.push(g.clone()); // skips "Client", "Server", "Java 21"…
            }
        }
        if loaders.is_empty() {
            loaders.push("forge".into()); // very old files carry no loader tag; they're Forge
        }
        let mut hashes = HashMap::new();
        for h in self.hashes {
            match h.algo {
                1 => hashes.insert("sha1".to_string(), h.value.to_lowercase()),
                2 => hashes.insert("md5".to_string(), h.value.to_lowercase()),
                _ => None,
            };
        }
        let dependencies = self
            .dependencies
            .into_iter()
            .filter(|d| d.mod_id != 0)
            .map(|d| Dep {
                project_id: Some(cf_id(d.mod_id)),
                version_id: None,
                file_name: None,
                dependency_type: match d.relation_type {
                    1 | 6 => DepType::Embedded, // embedded library / include
                    2 => DepType::Optional,
                    3 => DepType::Required,
                    5 => DepType::Incompatible,
                    _ => DepType::Other, // 4 = tool
                },
            })
            .collect();
        Version {
            id: cf_id(self.id),
            project_id: cf_id(self.mod_id),
            version_number: if self.display_name.is_empty() { self.file_name.clone() } else { self.display_name },
            version_type: match self.release_type {
                2 => "beta",
                3 => "alpha",
                _ => "release",
            }
            .into(),
            date_published: self.file_date,
            game_versions: games,
            loaders,
            dependencies,
            distribution_blocked: self.download_url.is_none(),
            files: vec![VFile { filename: self.file_name, url: self.download_url.unwrap_or_default(), size: self.file_length, hashes, primary: true }],
        }
    }
}

impl Mod {
    fn categories(&self) -> Vec<String> {
        let mut c: Vec<String> = self.categories.iter().map(|c| category_slug(&c.name)).collect();
        c.sort();
        c.dedup();
        c
    }

    fn supports(&self, mc: Option<&str>, loader: Option<Loader>) -> Option<bool> {
        if mc.is_none() && loader.is_none() {
            return None;
        }
        Some(self.latest_files_indexes.iter().any(|i| {
            mc.map_or(true, |v| i.game_version == v)
                && loader.map_or(true, |l| {
                    i.mod_loader.and_then(loader_from_type).is_some_and(|x| l.accepts().contains(&x))
                })
        }))
    }

    fn into_hit(self, mc: Option<&str>, loader: Option<Loader>) -> SearchHit {
        let compatible = self.supports(mc, loader);
        let categories = self.categories();
        SearchHit {
            source: Source::Curseforge,
            id: self.id.to_string(),
            slug: self.slug,
            name: self.name,
            summary: self.summary,
            authors: self.authors.into_iter().map(|a| a.name).collect(),
            downloads: self.download_count.max(0.0) as u64,
            follows: None,
            icon_url: self.logo.and_then(|l| l.thumbnail_url).filter(|u| !u.is_empty()),
            updated: self.date_modified,
            categories,
            license: None,
            client_side: None,
            server_side: None,
            compatible,
            distribution_blocked: self.allow_mod_distribution == Some(false),
            also: None,
        }
    }

    fn into_info(self) -> ProjectInfo {
        let categories = self.categories();
        ProjectInfo {
            id: cf_id(self.id),
            slug: self.slug,
            title: self.name,
            description: self.summary,
            icon_url: self.logo.and_then(|l| l.thumbnail_url).filter(|u| !u.is_empty()),
            categories,
            additional_categories: vec![],
            client_side: None,
            server_side: None,
            updated: self.date_modified,
            downloads: self.download_count.max(0.0) as u64,
            license: None,
            kind: match self.class_id {
                Some(CLASS_RESOURCE_PACKS) => Some(crate::model::EntryKind::Resourcepack),
                Some(CLASS_SHADERS) => Some(crate::model::EntryKind::Shader),
                Some(CLASS_DATAPACKS) => Some(crate::model::EntryKind::Datapack),
                Some(_) => Some(crate::model::EntryKind::Mod),
                None => None,
            },
        }
    }
}

#[derive(Deserialize)]
struct CategoryWire {
    id: u64,
    name: String,
}

// ---------------------------------------------------------------- client
pub struct CurseForge {
    fetcher: Arc<Fetcher>,
    key: Mutex<Option<String>>,
    keychain_disabled: std::sync::atomic::AtomicBool,
}

impl CurseForge {
    pub fn new(fetcher: Arc<Fetcher>) -> Self {
        Self { fetcher, key: Mutex::new(None), keychain_disabled: false.into() }
    }

    /// Dev tools only: never read the OS keychain (no surprise permission prompts).
    pub fn disable_keychain(&self) {
        self.keychain_disabled.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Dev tools only: use this key instead of the keychain.
    pub fn set_key_for_tools(&self, key: String) {
        *self.key.lock().unwrap() = Some(key);
    }

    /// Call after the key is saved or removed.
    pub fn forget_key(&self) {
        *self.key.lock().unwrap() = None;
    }

    pub fn has_cached_key(&self) -> bool {
        self.key.lock().unwrap().is_some()
    }

    /// Errors with NoCurseForgeKey when there's no usable key (respects the dev-tool guard).
    pub async fn ensure_key(&self) -> AppResult<()> {
        self.key().await.map(|_| ())
    }

    async fn key(&self) -> AppResult<String> {
        if let Some(k) = self.key.lock().unwrap().clone() {
            return Ok(k);
        }
        if self.keychain_disabled.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(AppError::NoCurseForgeKey);
        }
        let k = tauri::async_runtime::spawn_blocking(keychain::get_key)
            .await
            .map_err(|e| AppError::Invalid(e.to_string()))??
            .ok_or(AppError::NoCurseForgeKey)?;
        *self.key.lock().unwrap() = Some(k.clone());
        Ok(k)
    }

    async fn get(&self, path: &str) -> AppResult<reqwest::RequestBuilder> {
        Ok(self.fetcher.http().get(format!("{BASE}{path}")).header("x-api-key", self.key().await?).header("Accept", "application/json"))
    }

    async fn post(&self, path: &str, body: serde_json::Value) -> AppResult<reqwest::RequestBuilder> {
        Ok(self
            .fetcher
            .http()
            .post(format!("{BASE}{path}"))
            .header("x-api-key", self.key().await?)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .body(body.to_string()))
    }

    async fn category_ids(&self, slugs: &[String], class: u32, force: bool) -> AppResult<(Vec<u64>, Vec<String>)> {
        if slugs.is_empty() {
            return Ok((vec![], vec![]));
        }
        let req = self.get(&format!("/v1/categories?gameId={MINECRAFT_GAME_ID}&classId={class}")).await?;
        let (env, _) = self.fetcher.fetch_json::<Envelope<Vec<CategoryWire>>>(req, 7 * DAY, force, NAME, status_help(NAME)).await?;
        let mut ids = Vec::new();
        let mut unmapped = Vec::new();
        for s in slugs {
            let names = cf_names_for(s);
            match env.data.iter().find(|c| names.iter().any(|n| n.eq_ignore_ascii_case(&c.name))) {
                Some(c) => ids.push(c.id),
                None => unmapped.push(s.clone()),
            }
        }
        Ok((ids, unmapped))
    }

    /// Search, plus notes when a filter can't be expressed on CurseForge.
    pub async fn search_with_notes(&self, q: &SearchQuery) -> AppResult<(SearchPage, Vec<String>)> {
        let mut notes = Vec::new();
        let class = class_for(q.kind);
        let (cat_ids, unmapped) = self.category_ids(&q.categories, class, q.force).await?;
        if !unmapped.is_empty() {
            notes.push("CurseForge has no matching category, so it's left out of this filtered search.".into());
            return Ok((SearchPage { source: Source::Curseforge, total: 0, hits: vec![], freshness: None }, notes));
        }
        if q.side.is_some() {
            notes.push("CurseForge doesn't list client/server sides, so its results aren't filtered by side.".into());
        }
        let mut params: Vec<(&str, String)> = vec![
            ("gameId", MINECRAFT_GAME_ID.to_string()),
            ("classId", class.to_string()),
            ("searchFilter", q.text.trim().to_string()),
            ("sortField", sort_field(q.sort).to_string()),
            ("sortOrder", "desc".into()),
            ("pageSize", q.clamped_limit().to_string()),
            ("index", q.offset.min(9_950).to_string()), // CurseForge caps index + pageSize at 10 000
        ];
        if !q.include_incompatible {
            if let Some(v) = &q.game_version {
                params.push(("gameVersion", v.clone()));
            }
            if let Some(l) = q.mod_loader() {
                // Quilt also runs Fabric mods; CurseForge filters one loader at a time, so ask for both.
                if l == Loader::Quilt {
                    params.push(("modLoaderTypes", "[4,5]".into()));
                } else {
                    params.push(("modLoaderType", loader_type(l).to_string()));
                }
            }
        }
        if let Some(c) = cat_ids.first() {
            params.push(("categoryId", c.to_string()));
        }
        let req = self.get("/v1/mods/search").await?.query(&params);
        let (env, fr) = self.fetcher.fetch_json::<Envelope<Vec<Mod>>>(req, DAY, q.force, NAME, status_help(NAME)).await?;
        let total = env.pagination.map(|p| p.total_count).unwrap_or(env.data.len() as u64);
        let hits = env.data.into_iter().map(|m| m.into_hit(q.game_version.as_deref(), q.mod_loader())).collect();
        Ok((SearchPage { source: Source::Curseforge, total, hits, freshness: Some(fr) }, notes))
    }

    /// Exact slug lookup (used for cross-site matching).
    pub async fn by_slug(&self, slug: &str, force: bool) -> AppResult<Vec<SearchHit>> {
        let params = [("gameId", MINECRAFT_GAME_ID.to_string()), ("classId", CLASS_MODS.to_string()), ("slug", slug.to_string())];
        let req = self.get("/v1/mods/search").await?.query(&params);
        let (env, _) = self.fetcher.fetch_json::<Envelope<Vec<Mod>>>(req, DAY, force, NAME, status_help(NAME)).await?;
        Ok(env.data.into_iter().map(|m| m.into_hit(None, None)).collect())
    }

    pub async fn project(&self, id: u64, force: bool) -> AppResult<ProjectDetails> {
        let req = self.get(&format!("/v1/mods/{id}")).await?;
        let (env, freshness) = self.fetcher.fetch_json::<Envelope<Mod>>(req, DAY, force, NAME, status_help(NAME)).await?;
        let req = self.get(&format!("/v1/mods/{id}/description")).await?;
        let body = match self.fetcher.fetch_json::<Envelope<String>>(req, DAY, force, NAME, status_help(NAME)).await {
            Ok((d, _)) => d.data,
            Err(_) => String::new(),
        };
        let m = env.data;
        let mut game_versions: Vec<String> = m.latest_files_indexes.iter().map(|i| i.game_version.clone()).collect();
        game_versions.sort();
        game_versions.dedup();
        let mut loaders: Vec<String> = m.latest_files_indexes.iter().filter_map(|i| i.mod_loader.and_then(loader_from_type)).map(String::from).collect();
        loaders.sort();
        loaders.dedup();
        let mut links = Vec::new();
        for (label, url) in [("Source", &m.links.source_url), ("Issues", &m.links.issues_url), ("Wiki", &m.links.wiki_url)] {
            if let Some(u) = url.as_ref().filter(|u| u.starts_with("https://")) {
                links.push((label.to_string(), u.clone()));
            }
        }
        let categories = m.categories();
        Ok(ProjectDetails {
            source: Source::Curseforge,
            id: m.id.to_string(),
            page_url: m.links.website_url.clone().filter(|u| u.starts_with("https://")).unwrap_or_else(|| format!("https://www.curseforge.com/minecraft/mc-mods/{}", m.slug)),
            slug: m.slug,
            name: m.name,
            summary: m.summary,
            body,
            license: None::<License>,
            links,
            game_versions,
            loaders,
            categories,
            client_side: None,
            server_side: None,
            downloads: m.download_count.max(0.0) as u64,
            follows: m.thumbs_up_count,
            updated: m.date_modified,
            published: m.date_released,
            icon_url: m.logo.and_then(|l| l.thumbnail_url),
            freshness,
            distribution_blocked: m.allow_mod_distribution == Some(false),
        })
    }
}

/// CurseForge's file fingerprint: MurmurHash2 (seed 1) of the file with
/// whitespace bytes (tab, LF, CR, space) removed.
pub fn fingerprint(bytes: &[u8]) -> u32 {
    let data: Vec<u8> = bytes.iter().copied().filter(|b| !matches!(b, 9 | 10 | 13 | 32)).collect();
    murmur2(&data, 1)
}

pub fn murmur2(data: &[u8], seed: u32) -> u32 {
    const M: u32 = 0x5bd1_e995;
    let mut h: u32 = seed ^ data.len() as u32;
    let mut chunks = data.chunks_exact(4);
    for c in &mut chunks {
        let mut k = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        k = k.wrapping_mul(M);
        k ^= k >> 24;
        k = k.wrapping_mul(M);
        h = h.wrapping_mul(M);
        h ^= k;
    }
    let rest = chunks.remainder();
    if rest.len() >= 3 {
        h ^= (rest[2] as u32) << 16;
    }
    if rest.len() >= 2 {
        h ^= (rest[1] as u32) << 8;
    }
    if !rest.is_empty() {
        h ^= rest[0] as u32;
        h = h.wrapping_mul(M);
    }
    h ^= h >> 13;
    h = h.wrapping_mul(M);
    h ^= h >> 15;
    h
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FingerprintResult {
    #[serde(default)]
    exact_matches: Vec<FingerprintMatch>,
}
#[derive(Deserialize)]
struct FingerprintMatch {
    file: File,
}

impl CurseForge {
    /// Files whose fingerprint matches exactly (fingerprint → version).
    pub async fn by_fingerprints(&self, fps: &[u32]) -> AppResult<HashMap<u32, Version>> {
        let mut out = HashMap::new();
        for chunk in fps.chunks(200) {
            let req = self.post(&format!("/v1/fingerprints/{MINECRAFT_GAME_ID}"), serde_json::json!({ "fingerprints": chunk })).await?;
            let (env, _) = self.fetcher.fetch_json::<Envelope<FingerprintResult>>(req, 7 * DAY, false, NAME, status_help(NAME)).await?;
            for m in env.data.exact_matches {
                let fp = m.file.file_fingerprint;
                out.insert(fp, m.file.into_version());
            }
        }
        Ok(out)
    }
}

#[async_trait::async_trait]
impl ModSource for CurseForge {
    fn source(&self) -> Source {
        Source::Curseforge
    }
    async fn search(&self, q: &SearchQuery) -> AppResult<SearchPage> {
        Ok(self.search_with_notes(q).await?.0)
    }
}

#[async_trait::async_trait]
impl Catalog for CurseForge {
    async fn versions(&self, project_id: &str, mc: &str, loader: Loader, force: bool) -> AppResult<Vec<Version>> {
        let Some(id) = parse_id(project_id) else { return Ok(vec![]) };
        let mut out = Vec::new();
        for l in loader.accepts() {
            let t = match *l {
                "forge" => 1,
                "fabric" => 4,
                "quilt" => 5,
                "neoforge" => 6,
                _ => continue,
            };
            let req = self
                .get(&format!("/v1/mods/{id}/files"))
                .await?
                .query(&[("gameVersion", mc.to_string()), ("modLoaderType", t.to_string()), ("pageSize", "50".into())]);
            match self.fetcher.fetch_json::<Envelope<Vec<File>>>(req, DAY, force, NAME, status_help(NAME)).await {
                Ok((env, _)) => out.extend(env.data.into_iter().filter(|f| f.is_available && !f.is_server_pack).map(File::into_version)),
                Err(AppError::Api { status: 404, .. }) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(out)
    }

    /// Resource packs, shaders and datapacks have no loader on CurseForge; their
    /// files get the kind's Modrinth-style tag so selection treats both sites alike.
    async fn versions_of(&self, project_id: &str, mc: &str, loader: Loader, kind: crate::model::EntryKind, force: bool) -> AppResult<Vec<Version>> {
        if kind == crate::model::EntryKind::Mod {
            return self.versions(project_id, mc, loader, force).await;
        }
        let Some(id) = parse_id(project_id) else { return Ok(vec![]) };
        let mut q = vec![("pageSize", "50".to_string())];
        if !crate::resolve::kind_may_fall_back(kind) {
            q.push(("gameVersion", mc.to_string()));
        }
        let req = self.get(&format!("/v1/mods/{id}/files")).await?.query(&q);
        let files = match self.fetcher.fetch_json::<Envelope<Vec<File>>>(req, DAY, force, NAME, status_help(NAME)).await {
            Ok((env, _)) => env.data,
            Err(AppError::Api { status: 404, .. }) => vec![],
            Err(e) => return Err(e),
        };
        let tags = crate::resolve::kind_loaders(kind, loader);
        Ok(files
            .into_iter()
            .filter(|f| f.is_available && !f.is_server_pack)
            .map(|f| {
                let mut v = f.into_version();
                v.loaders = tags.iter().map(|s| s.to_string()).collect();
                v
            })
            .collect())
    }

    async fn version(&self, id: &str, force: bool) -> AppResult<Option<Version>> {
        let Some(fid) = parse_id(id) else { return Ok(None) };
        let req = self.post("/v1/mods/files", serde_json::json!({ "fileIds": [fid] })).await?;
        let (env, _) = self.fetcher.fetch_json::<Envelope<Vec<File>>>(req, 7 * DAY, force, NAME, status_help(NAME)).await?;
        Ok(env.data.into_iter().find(|f| f.id == fid).map(File::into_version))
    }

    async fn projects(&self, ids: &[String], force: bool) -> AppResult<Vec<ProjectInfo>> {
        let nums: Vec<u64> = ids.iter().filter_map(|i| parse_id(i)).collect();
        let mut out = Vec::new();
        for chunk in nums.chunks(100) {
            let req = self.post("/v1/mods", serde_json::json!({ "modIds": chunk, "filterPcOnly": true })).await?;
            let (env, _) = self.fetcher.fetch_json::<Envelope<Vec<Mod>>>(req, DAY, force, NAME, status_help(NAME)).await?;
            out.extend(env.data.into_iter().map(Mod::into_info));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recorded_search() {
        let env: Envelope<Vec<Mod>> = serde_json::from_str(include_str!("../../tests/fixtures/curseforge_search_jei.json")).unwrap();
        assert_eq!(env.pagination.unwrap().total_count, 2);
        let hits: Vec<SearchHit> = env.data.into_iter().map(|m| m.into_hit(Some("1.21.1"), Some(Loader::Neoforge))).collect();
        let jei = &hits[0];
        assert_eq!(jei.id, "238222");
        assert_eq!(jei.authors, vec!["mezz"]);
        assert_eq!(jei.downloads, 400_000_000);
        assert_eq!(jei.categories, vec!["library"], "API and Library maps to library");
        assert!(jei.icon_url.is_some());
        // Sparse entry: missing fields must not fail.
        assert_eq!(hits[1].name, "Sparse Mod");
        assert!(hits[1].icon_url.is_none());
    }

    #[test]
    fn categories_map_both_ways() {
        assert_eq!(category_slug("API and Library"), "library");
        assert_eq!(category_slug("Utility & QoL"), "utility");
        assert_eq!(category_slug("Map and Information"), "map-and-information");
        assert_eq!(cf_names_for("worldgen"), &["World Gen"]);
        assert!(cf_names_for("cursed").is_empty());
    }

    #[test]
    fn file_maps_to_version() {
        let f: File = serde_json::from_value(serde_json::json!({
            "id": 5_000_001, "modId": 238222, "displayName": "jei-1.21.1-neoforge-19.21.0.247", "fileName": "jei.jar",
            "releaseType": 2, "fileDate": "2026-10-01T00:00:00Z", "fileLength": 1234, "downloadUrl": null,
            "hashes": [{"value": "ABCDEF", "algo": 1}, {"value": "1234", "algo": 2}],
            "gameVersions": ["NeoForge", "1.21.1", "Client", "Server", "Java 21"],
            "dependencies": [{"modId": 1, "relationType": 3}, {"modId": 2, "relationType": 2}, {"modId": 3, "relationType": 5},
                             {"modId": 4, "relationType": 1}, {"modId": 5, "relationType": 6}, {"modId": 6, "relationType": 4}]
        }))
        .unwrap();
        let v = f.into_version();
        assert_eq!(v.id, "cf:5000001");
        assert_eq!(v.project_id, "cf:238222");
        assert_eq!(v.version_type, "beta");
        assert_eq!(v.loaders, vec!["neoforge"]);
        assert_eq!(v.game_versions, vec!["1.21.1"]);
        assert!(v.distribution_blocked, "no download URL means third-party downloads are blocked");
        assert_eq!(v.files[0].hashes["sha1"], "abcdef");
        let kinds: Vec<DepType> = v.dependencies.iter().map(|d| d.dependency_type).collect();
        assert_eq!(kinds, vec![DepType::Required, DepType::Optional, DepType::Incompatible, DepType::Embedded, DepType::Embedded, DepType::Other]);
        assert!(v.supports("1.21.1", Loader::Neoforge));
        assert!(!v.supports("1.21.1", Loader::Forge));
    }

    #[test]
    fn untagged_old_files_count_as_forge() {
        let f: File = serde_json::from_value(serde_json::json!({ "id": 1, "modId": 2, "gameVersions": ["1.12.2"] })).unwrap();
        assert_eq!(f.into_version().loaders, vec!["forge"]);
    }

    #[test]
    fn fingerprint_ignores_whitespace() {
        assert_eq!(fingerprint(b"a b\tc\r\n"), fingerprint(b"abc"));
        assert_ne!(fingerprint(b"abc"), fingerprint(b"abd"));
        // Reference MurmurHash2 values (32-bit, seed 1).
        assert_eq!(murmur2(b"", 1), 0x5bd1_5e36);
    }

    #[test]
    fn loader_ids() {
        assert_eq!(loader_type(Loader::Neoforge), 6);
        assert_eq!(loader_type(Loader::Fabric), 4);
        assert_eq!(parse_id("cf:42"), Some(42));
        assert_eq!(parse_id("AANobbMI"), None);
    }
}
