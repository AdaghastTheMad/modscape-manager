//! Modrinth API client (https://docs.modrinth.com/api/). No key needed.

use super::{Loader, ModSource, SearchHit, SearchPage, SearchQuery, SideFilter, Sort, Source, LOADER_SLUGS};
use crate::error::AppResult;
use crate::fetch::{status_help, Fetcher, Freshness, DAY};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const BASE: &str = "https://api.modrinth.com";
const NAME: &str = "Modrinth";

pub struct Modrinth {
    fetcher: Arc<Fetcher>,
}

impl Modrinth {
    pub fn new(fetcher: Arc<Fetcher>) -> Self {
        Self { fetcher }
    }
}

// ---------------- search ----------------
#[derive(Deserialize)]
struct Resp {
    #[serde(default)]
    hits: Vec<Hit>,
    #[serde(default)]
    total_hits: u64,
}
#[derive(Deserialize)]
struct Hit {
    project_id: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    author: String,
    #[serde(default)]
    downloads: u64,
    follows: Option<u64>,
    icon_url: Option<String>,
    date_modified: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    display_categories: Vec<String>,
    #[serde(default)]
    versions: Vec<String>,
    license: Option<String>,
    client_side: Option<String>,
    server_side: Option<String>,
}

/// Search facets: AND across groups, OR within a group.
pub fn facets(q: &SearchQuery) -> String {
    let ty = match q.kind {
        crate::model::EntryKind::Mod => "mod",
        crate::model::EntryKind::Resourcepack => "resourcepack",
        crate::model::EntryKind::Shader => "shader",
        crate::model::EntryKind::Datapack => "datapack",
    };
    let mut groups: Vec<Vec<String>> = vec![vec![format!("project_type:{ty}")]];
    if !q.include_incompatible {
        if let Some(v) = &q.game_version {
            groups.push(vec![format!("versions:{v}")]);
        }
        if let Some(l) = q.mod_loader() {
            groups.push(l.accepts().iter().map(|s| format!("categories:{s}")).collect());
        }
    }
    for c in &q.categories {
        groups.push(vec![format!("categories:{c}")]);
    }
    match q.side {
        Some(SideFilter::Client) => groups.push(vec!["client_side!=unsupported".into()]),
        Some(SideFilter::Server) => groups.push(vec!["server_side!=unsupported".into()]),
        None => {}
    }
    serde_json::to_string(&groups).expect("facets serialise")
}

fn sort_index(s: Sort) -> &'static str {
    match s {
        Sort::Relevance => "relevance",
        Sort::Downloads => "downloads",
        Sort::Follows => "follows",
        Sort::Updated => "updated",
        Sort::Newest => "newest",
    }
}

pub fn is_compatible(versions: &[String], loaders: &[String], mc: Option<&str>, loader: Option<Loader>) -> Option<bool> {
    if mc.is_none() && loader.is_none() {
        return None;
    }
    let v_ok = mc.map_or(true, |v| versions.iter().any(|x| x == v));
    let l_ok = loader.map_or(true, |l| l.accepts().iter().any(|a| loaders.iter().any(|x| x == a)));
    Some(v_ok && l_ok)
}

fn parse(body: Resp, q: &SearchQuery, freshness: Freshness) -> SearchPage {
    let hits = body
        .hits
        .into_iter()
        .map(|h| {
            let all_cats: Vec<String> = h.categories.iter().chain(h.display_categories.iter()).cloned().collect();
            let compatible = is_compatible(&h.versions, &all_cats, q.game_version.as_deref(), q.mod_loader());
            let mut cats: Vec<String> = all_cats.into_iter().filter(|c| !LOADER_SLUGS.contains(&c.as_str())).collect();
            cats.sort();
            cats.dedup();
            SearchHit {
                source: Source::Modrinth,
                id: h.project_id,
                slug: h.slug,
                name: h.title,
                summary: h.description,
                authors: if h.author.is_empty() { vec![] } else { vec![h.author] },
                downloads: h.downloads,
                follows: h.follows,
                icon_url: h.icon_url.filter(|u| !u.is_empty()),
                updated: h.date_modified,
                categories: cats,
                license: h.license.filter(|l| !l.is_empty()),
                client_side: h.client_side,
                server_side: h.server_side,
                compatible,
                distribution_blocked: false,
                also: None,
            }
        })
        .collect();
    SearchPage { source: Source::Modrinth, total: body.total_hits, hits, freshness: Some(freshness) }
}

#[async_trait::async_trait]
impl ModSource for Modrinth {
    fn source(&self) -> Source {
        Source::Modrinth
    }

    async fn search(&self, q: &SearchQuery) -> AppResult<SearchPage> {
        let req = self.fetcher.http().get(format!("{BASE}/v2/search")).query(&[
            ("query", q.text.trim().to_string()),
            ("facets", facets(q)),
            ("index", sort_index(q.sort).to_string()),
            ("limit", q.clamped_limit().to_string()),
            ("offset", q.offset.to_string()),
        ]);
        let (body, fr) = self.fetcher.fetch_json::<Resp>(req, DAY, q.force, NAME, status_help(NAME)).await?;
        Ok(parse(body, q, fr))
    }
}

// ---------------- project details ----------------
#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct License {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProjectWire {
    id: String,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    body: String,
    license: Option<License>,
    source_url: Option<String>,
    issues_url: Option<String>,
    wiki_url: Option<String>,
    discord_url: Option<String>,
    #[serde(default)]
    game_versions: Vec<String>,
    #[serde(default)]
    loaders: Vec<String>,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    additional_categories: Vec<String>,
    client_side: Option<String>,
    server_side: Option<String>,
    #[serde(default)]
    downloads: u64,
    #[serde(default)]
    followers: u64,
    updated: Option<String>,
    published: Option<String>,
    icon_url: Option<String>,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDetails {
    pub source: Source,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub summary: String,
    /// Markdown/HTML description as the author wrote it. The UI sanitises it.
    pub body: String,
    pub license: Option<License>,
    pub page_url: String,
    pub links: Vec<(String, String)>,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub categories: Vec<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    pub downloads: u64,
    pub follows: u64,
    pub updated: Option<String>,
    pub published: Option<String>,
    pub icon_url: Option<String>,
    pub freshness: Freshness,
    #[serde(default)]
    pub distribution_blocked: bool,
}

impl Modrinth {
    pub async fn project(&self, id: &str, force: bool) -> AppResult<ProjectDetails> {
        let id = urlencode_segment(id);
        let req = self.fetcher.http().get(format!("{BASE}/v2/project/{id}"));
        let (p, freshness) = self.fetcher.fetch_json::<ProjectWire>(req, DAY, force, NAME, status_help(NAME)).await?;
        let mut links = Vec::new();
        for (label, url) in [("Source", p.source_url), ("Issues", p.issues_url), ("Wiki", p.wiki_url), ("Discord", p.discord_url)] {
            if let Some(u) = url.filter(|u| u.starts_with("https://")) {
                links.push((label.to_string(), u));
            }
        }
        let mut categories: Vec<String> = p.categories.into_iter().chain(p.additional_categories).filter(|c| !LOADER_SLUGS.contains(&c.as_str())).collect();
        categories.dedup();
        Ok(ProjectDetails {
            source: Source::Modrinth,
            page_url: format!("https://modrinth.com/mod/{}", if p.slug.is_empty() { &p.id } else { &p.slug }),
            id: p.id,
            slug: p.slug,
            name: p.title,
            summary: p.description,
            body: p.body,
            license: p.license,
            links,
            game_versions: p.game_versions,
            loaders: p.loaders,
            categories,
            client_side: p.client_side,
            server_side: p.server_side,
            downloads: p.downloads,
            follows: p.followers,
            updated: p.updated,
            published: p.published,
            icon_url: p.icon_url.filter(|u| !u.is_empty()),
            freshness,
            distribution_blocked: false,
        })
    }

    pub async fn categories(&self, force: bool) -> AppResult<Vec<Category>> {
        self.categories_for(crate::model::EntryKind::Mod, force).await
    }

    /// Categories for one content kind (datapacks use Modrinth's mod categories).
    pub async fn categories_for(&self, kind: crate::model::EntryKind, force: bool) -> AppResult<Vec<Category>> {
        use crate::model::EntryKind::*;
        let ty = match kind {
            Mod | Datapack => "mod",
            Resourcepack => "resourcepack",
            Shader => "shader",
        };
        let req = self.fetcher.http().get(format!("{BASE}/v2/tag/category"));
        let (all, _) = self.fetcher.fetch_json::<Vec<CategoryWire>>(req, 7 * DAY, force, NAME, status_help(NAME)).await?;
        let mut out: Vec<Category> = all
            .into_iter()
            .filter(|c| c.project_type == ty && (c.header == "categories" || kind != Mod && kind != Datapack))
            .map(|c| Category { name: c.name, icon_svg: c.icon })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub async fn game_versions(&self, force: bool) -> AppResult<Vec<GameVersion>> {
        let req = self.fetcher.http().get(format!("{BASE}/v2/tag/game_version"));
        let (mut all, _) = self.fetcher.fetch_json::<Vec<GameVersion>>(req, DAY, force, NAME, status_help(NAME)).await?;
        all.sort_by(|a, b| b.date.cmp(&a.date));
        Ok(all)
    }
}

/// Project ids/slugs are [A-Za-z0-9_-]; anything else is percent-encoded.
fn urlencode_segment(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

#[derive(Deserialize)]
struct CategoryWire {
    #[serde(default)]
    icon: String,
    name: String,
    project_type: String,
    #[serde(default)]
    header: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub name: String,
    pub icon_svg: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GameVersion {
    pub version: String,
    pub version_type: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub major: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> SearchQuery {
        serde_json::from_str(r#"{"text":"jei","gameVersion":"1.21.1","loader":"neoforge"}"#).unwrap()
    }

    #[test]
    fn facets_build() {
        assert_eq!(facets(&q()), r#"[["project_type:mod"],["versions:1.21.1"],["categories:neoforge"]]"#);
        let mut quilt = q();
        quilt.loader = Some(Loader::Quilt);
        quilt.categories = vec!["magic".into()];
        quilt.side = Some(SideFilter::Server);
        assert_eq!(
            facets(&quilt),
            r#"[["project_type:mod"],["versions:1.21.1"],["categories:quilt","categories:fabric"],["categories:magic"],["server_side!=unsupported"]]"#
        );
        let mut loose = q();
        loose.include_incompatible = true;
        assert_eq!(facets(&loose), r#"[["project_type:mod"]]"#);
    }

    #[test]
    fn compatibility() {
        let v = vec!["1.21.1".to_string(), "26.1.2".into()];
        let l = vec!["fabric".to_string(), "optimization".into()];
        assert_eq!(is_compatible(&v, &l, None, None), None);
        assert_eq!(is_compatible(&v, &l, Some("26.1.2"), Some(Loader::Fabric)), Some(true));
        assert_eq!(is_compatible(&v, &l, Some("26.1.2"), Some(Loader::Quilt)), Some(true));
        assert_eq!(is_compatible(&v, &l, Some("26.1.2"), Some(Loader::Neoforge)), Some(false));
        assert_eq!(is_compatible(&v, &l, Some("1.20.1"), Some(Loader::Fabric)), Some(false));
    }

    #[test]
    fn parses_recorded_search() {
        let body: Resp = serde_json::from_str(include_str!("../../tests/fixtures/modrinth_search_sodium.json")).unwrap();
        let fr = Freshness { fetched_at: 0, from_cache: false, stale: false };
        let mut query = q();
        query.loader = Some(Loader::Fabric);
        query.game_version = Some("1.21.1".into());
        let page = parse(body, &query, fr);
        let h = &page.hits[0];
        assert_eq!(h.slug, "sodium");
        assert_eq!(h.categories, vec!["optimization"]);
        assert_eq!(h.compatible, Some(true));
        assert_eq!(h.license.as_deref(), Some("LicenseRef-Polyform-Shield-1.0.0"));
    }

    #[test]
    fn encodes_ids() {
        assert_eq!(urlencode_segment("AANobbMI"), "AANobbMI");
        assert_eq!(urlencode_segment("../x"), "%2E%2E%2Fx");
    }
}

// ---------------- catalog for the resolver ----------------
use crate::error::AppError;
use crate::resolve::{Catalog, ProjectInfo, Version};

#[async_trait::async_trait]
impl Catalog for Modrinth {
    async fn versions(&self, project_id: &str, mc: &str, loader: Loader, force: bool) -> AppResult<Vec<Version>> {
        let loaders = serde_json::to_string(loader.accepts()).expect("loaders");
        let games = serde_json::to_string(&[mc]).expect("games");
        let req = self
            .fetcher
            .http()
            .get(format!("{BASE}/v2/project/{}/version", urlencode_segment(project_id)))
            .query(&[("loaders", loaders), ("game_versions", games), ("include_changelog", "false".into())]);
        match self.fetcher.fetch_json::<Vec<Version>>(req, DAY, force, NAME, status_help(NAME)).await {
            Ok((v, _)) => Ok(v),
            Err(AppError::Api { status: 404, .. }) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    async fn versions_of(&self, project_id: &str, mc: &str, loader: Loader, kind: crate::model::EntryKind, force: bool) -> AppResult<Vec<Version>> {
        if kind == crate::model::EntryKind::Mod {
            return self.versions(project_id, mc, loader, force).await;
        }
        let loaders = serde_json::to_string(crate::resolve::kind_loaders(kind, loader)).expect("loaders");
        let mut q = vec![("loaders", loaders), ("include_changelog", "false".into())];
        // Resource packs and shaders may fall back to another version's build, so fetch them all.
        if !crate::resolve::kind_may_fall_back(kind) {
            q.push(("game_versions", serde_json::to_string(&[mc]).expect("games")));
        }
        let req = self.fetcher.http().get(format!("{BASE}/v2/project/{}/version", urlencode_segment(project_id))).query(&q);
        match self.fetcher.fetch_json::<Vec<Version>>(req, DAY, force, NAME, status_help(NAME)).await {
            Ok((v, _)) => Ok(v),
            Err(AppError::Api { status: 404, .. }) => Ok(vec![]),
            Err(e) => Err(e),
        }
    }

    async fn version(&self, id: &str, force: bool) -> AppResult<Option<Version>> {
        let req = self.fetcher.http().get(format!("{BASE}/v2/version/{}", urlencode_segment(id)));
        match self.fetcher.fetch_json::<Version>(req, 7 * DAY, force, NAME, status_help(NAME)).await {
            Ok((v, _)) => Ok(Some(v)),
            Err(AppError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    async fn projects(&self, ids: &[String], force: bool) -> AppResult<Vec<ProjectInfo>> {
        let mut out = Vec::new();
        for chunk in ids.chunks(100) {
            let req = self
                .fetcher
                .http()
                .get(format!("{BASE}/v2/projects"))
                .query(&[("ids", serde_json::to_string(chunk).expect("ids"))]);
            let (mut v, _) = self.fetcher.fetch_json::<Vec<ProjectInfo>>(req, DAY, force, NAME, status_help(NAME)).await?;
            out.append(&mut v);
        }
        Ok(out)
    }
}

// ---------------- lookups used by cross-site matching ----------------
impl Modrinth {
    /// Project by slug or id; Ok(None) if there isn't one.
    pub async fn project_info(&self, slug_or_id: &str, force: bool) -> AppResult<Option<ProjectInfo>> {
        let req = self.fetcher.http().get(format!("{BASE}/v2/project/{}", urlencode_segment(slug_or_id)));
        match self.fetcher.fetch_json::<ProjectInfo>(req, DAY, force, NAME, status_help(NAME)).await {
            Ok((p, _)) => Ok(Some(p)),
            Err(AppError::Api { status: 404, .. }) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Modrinth versions whose file has one of these SHA-1 hashes (hash → version).
    pub async fn versions_by_sha1(&self, hashes: &[String], force: bool) -> AppResult<std::collections::HashMap<String, Version>> {
        if hashes.is_empty() {
            return Ok(Default::default());
        }
        let body = serde_json::json!({ "hashes": hashes, "algorithm": "sha1" }).to_string();
        let req = self.fetcher.http().post(format!("{BASE}/v2/version_files")).header("Content-Type", "application/json").body(body);
        let (m, _) = self.fetcher.fetch_json::<std::collections::HashMap<String, Version>>(req, 7 * DAY, force, NAME, status_help(NAME)).await?;
        Ok(m)
    }
}
