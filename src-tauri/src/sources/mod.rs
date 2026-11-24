//! One trait over every mod site, so the UI never cares where a mod came from.

pub mod curseforge;
pub mod modrinth;

use crate::error::AppResult;
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

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQuery {
    pub text: String,
    pub game_version: Option<String>,
    pub loader: Option<Loader>,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
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
    pub icon_url: Option<String>,
    pub updated: Option<String>,
    pub categories: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchPage {
    pub source: Source,
    pub total: u64,
    pub hits: Vec<SearchHit>,
}

#[async_trait::async_trait]
pub trait ModSource: Send + Sync {
    fn source(&self) -> Source;
    async fn search(&self, q: &SearchQuery) -> AppResult<SearchPage>;
}
