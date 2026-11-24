//! CurseForge API client (https://docs.curseforge.com/rest-api/).
//! Needs the user's API key, read from the OS keychain on each call.
//! Full CurseForge support (merged results, details) is milestone 4.

use super::{Loader, ModSource, SearchHit, SearchPage, SearchQuery, Sort, Source};
use crate::error::{AppError, AppResult};
use crate::fetch::{status_help, Fetcher, DAY};
use crate::keychain;
use serde::Deserialize;
use std::sync::Arc;

const BASE: &str = "https://api.curseforge.com";
const NAME: &str = "CurseForge";
pub const MINECRAFT_GAME_ID: u32 = 432;
pub const CLASS_MODS: u32 = 6;

pub fn loader_type(l: Loader) -> u32 {
    match l {
        Loader::Forge => 1,
        Loader::Fabric => 4,
        Loader::Quilt => 5,
        Loader::Neoforge => 6,
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

pub struct CurseForge {
    fetcher: Arc<Fetcher>,
}

impl CurseForge {
    pub fn new(fetcher: Arc<Fetcher>) -> Self {
        Self { fetcher }
    }
}

// --- wire types (lenient: every field optional/defaulted, the API varies) ---
#[derive(Deserialize)]
struct Resp {
    #[serde(default)]
    data: Vec<Mod>,
    pagination: Option<Pagination>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pagination {
    #[serde(default)]
    total_count: u64,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Mod {
    id: u64,
    #[serde(default)]
    slug: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    download_count: f64,
    logo: Option<Logo>,
    date_modified: Option<String>,
    #[serde(default)]
    categories: Vec<Named>,
    #[serde(default)]
    authors: Vec<Named>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Logo {
    thumbnail_url: Option<String>,
}
#[derive(Deserialize)]
struct Named {
    #[serde(default)]
    name: String,
}

fn parse(r: Resp) -> SearchPage {
    let hits: Vec<SearchHit> = r
        .data
        .into_iter()
        .map(|m| SearchHit {
            source: Source::Curseforge,
            id: m.id.to_string(),
            slug: m.slug,
            name: m.name,
            summary: m.summary,
            authors: m.authors.into_iter().map(|a| a.name).collect(),
            downloads: m.download_count.max(0.0) as u64,
            follows: None,
            icon_url: m.logo.and_then(|l| l.thumbnail_url).filter(|u| !u.is_empty()),
            updated: m.date_modified,
            categories: m.categories.into_iter().map(|c| c.name).collect(),
            license: None,
            client_side: None,
            server_side: None,
            compatible: None,
        })
        .collect();
    let total = r.pagination.map(|p| p.total_count).unwrap_or(hits.len() as u64);
    SearchPage { source: Source::Curseforge, total, hits, freshness: None }
}

#[async_trait::async_trait]
impl ModSource for CurseForge {
    fn source(&self) -> Source {
        Source::Curseforge
    }

    async fn search(&self, q: &SearchQuery) -> AppResult<SearchPage> {
        let key = tauri::async_runtime::spawn_blocking(keychain::get_key)
            .await
            .map_err(|e| AppError::Invalid(e.to_string()))??
            .ok_or(AppError::NoCurseForgeKey)?;

        let mut params: Vec<(&str, String)> = vec![
            ("gameId", MINECRAFT_GAME_ID.to_string()),
            ("classId", CLASS_MODS.to_string()),
            ("searchFilter", q.text.trim().to_string()),
            ("sortField", sort_field(q.sort).to_string()),
            ("sortOrder", "desc".into()),
            ("pageSize", q.clamped_limit().to_string()),
            ("index", q.offset.to_string()),
        ];
        if let Some(v) = &q.game_version {
            params.push(("gameVersion", v.clone()));
        }
        if let Some(l) = q.loader {
            params.push(("modLoaderType", loader_type(l).to_string()));
        }
        let req = self
            .fetcher
            .http()
            .get(format!("{BASE}/v1/mods/search"))
            .header("x-api-key", key)
            .header("Accept", "application/json")
            .query(&params);
        let (body, fr) = self.fetcher.fetch_json::<Resp>(req, DAY, q.force, NAME, status_help(NAME)).await?;
        let mut page = parse(body);
        page.freshness = Some(fr);
        Ok(page)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recorded_search() {
        let body: Resp = serde_json::from_str(include_str!("../../tests/fixtures/curseforge_search_jei.json")).unwrap();
        let page = parse(body);
        assert_eq!(page.source, Source::Curseforge);
        assert_eq!(page.total, 2);
        let jei = &page.hits[0];
        assert_eq!(jei.id, "238222");
        assert_eq!(jei.authors, vec!["mezz"]);
        assert_eq!(jei.downloads, 400_000_000);
        assert!(jei.icon_url.is_some());
        // Second entry is deliberately sparse: missing fields must not fail.
        assert_eq!(page.hits[1].name, "Sparse Mod");
        assert!(page.hits[1].icon_url.is_none());
    }

    #[test]
    fn loader_ids() {
        assert_eq!(loader_type(Loader::Neoforge), 6);
        assert_eq!(loader_type(Loader::Fabric), 4);
    }
}
