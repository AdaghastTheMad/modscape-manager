//! Modrinth API client (https://docs.modrinth.com/api/). No key needed.
//! Full search UI arrives in milestone 2; this is the trait implementation.

use super::{Loader, ModSource, SearchHit, SearchPage, SearchQuery, Source};
use crate::error::{AppError, AppResult};
use serde::Deserialize;

const BASE: &str = "https://api.modrinth.com";

pub fn loader_slug(l: Loader) -> &'static str {
    match l {
        Loader::Neoforge => "neoforge",
        Loader::Fabric => "fabric",
        Loader::Forge => "forge",
        Loader::Quilt => "quilt",
    }
}

pub struct Modrinth {
    http: reqwest::Client,
}

impl Modrinth {
    pub fn new(http: reqwest::Client) -> Self {
        Self { http }
    }
}

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
    icon_url: Option<String>,
    date_modified: Option<String>,
    #[serde(default)]
    categories: Vec<String>,
}

/// Search facets: AND across groups, OR within a group.
pub fn facets(q: &SearchQuery) -> String {
    let mut groups: Vec<Vec<String>> = vec![vec!["project_type:mod".into()]];
    if let Some(v) = &q.game_version {
        groups.push(vec![format!("versions:{v}")]);
    }
    if let Some(l) = q.loader {
        groups.push(vec![format!("categories:{}", loader_slug(l))]);
    }
    serde_json::to_string(&groups).expect("facets serialise")
}

fn parse(body: &str) -> AppResult<SearchPage> {
    let r: Resp = serde_json::from_str(body)
        .map_err(|e| AppError::Decode { source_name: "Modrinth", detail: e.to_string() })?;
    let hits = r
        .hits
        .into_iter()
        .map(|h| SearchHit {
            source: Source::Modrinth,
            id: h.project_id,
            slug: h.slug,
            name: h.title,
            summary: h.description,
            authors: if h.author.is_empty() { vec![] } else { vec![h.author] },
            downloads: h.downloads,
            icon_url: h.icon_url.filter(|u| !u.is_empty()),
            updated: h.date_modified,
            // Loader names are mixed into Modrinth's categories; keep only real categories.
            categories: h
                .categories
                .into_iter()
                .filter(|c| !matches!(c.as_str(), "neoforge" | "fabric" | "forge" | "quilt" | "liteloader" | "rift" | "modloader"))
                .collect(),
        })
        .collect();
    Ok(SearchPage { source: Source::Modrinth, total: r.total_hits, hits })
}

#[async_trait::async_trait]
impl ModSource for Modrinth {
    fn source(&self) -> Source {
        Source::Modrinth
    }

    async fn search(&self, q: &SearchQuery) -> AppResult<SearchPage> {
        let resp = self
            .http
            .get(format!("{BASE}/v2/search"))
            .query(&[
                ("query", q.text.clone()),
                ("facets", facets(q)),
                ("limit", q.clamped_limit().to_string()),
                ("offset", q.offset.to_string()),
            ])
            .send()
            .await?;
        let status = resp.status();
        if !status.is_success() {
            let help = (status.as_u16() == 429)
                .then(|| "Modrinth is rate-limiting requests (300/min). Wait a minute and try again.".to_string());
            return Err(AppError::Api { source_name: "Modrinth", status: status.as_u16(), help });
        }
        parse(&resp.text().await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn facets_build() {
        let q = SearchQuery {
            text: "jei".into(),
            game_version: Some("1.21.1".into()),
            loader: Some(Loader::Neoforge),
            limit: 20,
            offset: 0,
        };
        assert_eq!(facets(&q), r#"[["project_type:mod"],["versions:1.21.1"],["categories:neoforge"]]"#);
    }

    #[test]
    fn parses_recorded_search_and_strips_loaders() {
        let body = include_str!("../../tests/fixtures/modrinth_search_sodium.json");
        let page = parse(body).unwrap();
        assert_eq!(page.total, 1);
        assert_eq!(page.hits[0].slug, "sodium");
        assert_eq!(page.hits[0].categories, vec!["optimization"]);
    }
}
