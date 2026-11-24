//! One trait over every mod site, so the UI never cares where a mod came from.

pub mod curseforge;
pub mod modrinth;

use crate::error::AppResult;
use crate::fetch::Freshness;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    Modrinth,
    Curseforge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Loader {
    Neoforge,
    Fabric,
    Forge,
    Quilt,
}

impl Loader {
    /// Loaders whose mods this loader can run. Quilt runs most Fabric mods.
    pub fn accepts(self) -> &'static [&'static str] {
        match self {
            Loader::Quilt => &["quilt", "fabric"],
            Loader::Fabric => &["fabric"],
            Loader::Neoforge => &["neoforge"],
            Loader::Forge => &["forge"],
        }
    }
}

pub const LOADER_SLUGS: &[&str] = &["neoforge", "fabric", "forge", "quilt", "liteloader", "rift", "modloader", "babric", "bta-babric", "java-agent", "legacy-fabric", "nilloader", "ornithe"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Sort {
    #[default]
    Relevance,
    Downloads,
    Follows,
    Updated,
    Newest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SideFilter {
    /// Works on a client (client_side isn't "unsupported").
    Client,
    /// Works on a dedicated server (server_side isn't "unsupported").
    Server,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    #[serde(default)]
    pub text: String,
    pub game_version: Option<String>,
    pub loader: Option<Loader>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub side: Option<SideFilter>,
    #[serde(default)]
    pub sort: Sort,
    /// Return results for other versions/loaders too, marked `compatible: false`.
    #[serde(default)]
    pub include_incompatible: bool,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
    /// Bypass the cache (manual refresh).
    #[serde(default)]
    pub force: bool,
}
fn default_limit() -> u32 {
    20
}

impl SearchQuery {
    pub fn clamped_limit(&self) -> u32 {
        self.limit.clamp(1, 50)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub source: Source,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub authors: Vec<String>,
    pub downloads: u64,
    pub follows: Option<u64>,
    pub icon_url: Option<String>,
    pub updated: Option<String>,
    pub categories: Vec<String>,
    pub license: Option<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    /// None when compatibility wasn't checked (no version/loader given).
    pub compatible: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub source: Source,
    pub total: u64,
    pub hits: Vec<SearchHit>,
    pub freshness: Option<Freshness>,
}

#[async_trait::async_trait]
pub trait ModSource: Send + Sync {
    fn source(&self) -> Source;
    async fn search(&self, q: &SearchQuery) -> AppResult<SearchPage>;
}
