//! CurseForge API client (https://docs.curseforge.com/rest-api/).
//! Needs the user's API key, read from the OS keychain on each call.

use super::{Loader, ModSource, SearchHit, SearchPage, SearchQuery, Source};
use crate::error::{AppError, AppResult};
use crate::keychain;
use serde::Deserialize;

const BASE: &str = "https://api.curseforge.com";
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

pub struct CurseForge {
    http: reqwest::Client,
}

impl CurseForge {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
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

fn parse(body: &str) -> AppResult<SearchPage> {
    let r: Resp = serde_json::from_str(body)
        .map_err(|e| AppError::Decode { source_name: "CurseForge", detail: e.to_string() })?;
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
            icon_url: m.logo.and_then(|l| l.thumbnail_url).filter(|u| !u.is_empty()),
            updated: m.date_modified,
            categories: m.categories.into_iter().map(|c| c.name).collect(),
        })
        .collect();
    let total = r.pagination.map(|p| p.total_count).unwrap_or(hits.len() as u64);
    Ok(SearchPage { source: Source::Curseforge, total, hits })
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
            ("searchFilter", q.text.clone()),
            ("sortField", "2".into()), // popularity
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

        let resp = self
            .http
            .get(format!("{BASE}/v1/mods/search"))
            .header("x-api-key", key)
            .header("Accept", "application/json")
            .query(&params)
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let help = match status.as_u16() {
                401 | 403 => Some(
                    "CurseForge rejected the API key. Check it at console.curseforge.com → API Keys, then save it again in Settings."
                        .to_string(),
                ),
                429 => Some("CurseForge is rate-limiting requests. Wait a minute and try again.".into()),
                _ => None,
            };
            return Err(AppError::Api { source_name: "CurseForge", status: status.as_u16(), help });
        }
        parse(&resp.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_recorded_search() {
        let body = include_str!("../../tests/fixtures/curseforge_search_jei.json");
        let page = parse(body).unwrap();
        assert_eq!(page.source, Source::Curseforge);
        assert_eq!(page.total, 2);
        assert_eq!(page.hits.len(), 2);
        let jei = &page.hits[0];
        assert_eq!(jei.id, "238222");
        assert_eq!(jei.slug, "jei");
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
