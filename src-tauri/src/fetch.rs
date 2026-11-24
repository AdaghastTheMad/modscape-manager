//! Every outbound request goes through here: one HTTP client, a host
//! allow-list (no telemetry, no surprise calls), and the SQLite cache.

use crate::error::{AppError, AppResult};
use crate::store::Db;
use std::sync::Arc;

pub const DAY: i64 = 24 * 60 * 60;
pub const ICON_TTL: i64 = 30 * DAY;
const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_ICON: usize = 2 * 1024 * 1024;

/// The only hosts the app ever contacts.
pub const ALLOWED_HOSTS: &[&str] = &[
    "api.modrinth.com",
    "cdn.modrinth.com",
    "api.curseforge.com",
    "media.forgecdn.net",
    "meta.fabricmc.net",
    "meta.quiltmc.org",
    "maven.neoforged.net",
    "maven.minecraftforge.net",
];
const ICON_HOSTS: &[&str] = &["cdn.modrinth.com", "media.forgecdn.net"];

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Freshness {
    pub fetched_at: i64,
    pub from_cache: bool,
    /// True when the network failed and an expired cache entry was used.
    pub stale: bool,
}

pub struct Fetched {
    pub body: Vec<u8>,
    pub content_type: Option<String>,
    pub freshness: Freshness,
}

pub struct Fetcher {
    http: reqwest::Client,
    db: Arc<Db>,
}

pub fn host_allowed(url: &reqwest::Url, list: &[&str]) -> bool {
    url.scheme() == "https" && url.host_str().is_some_and(|h| list.contains(&h))
}

/// Cache key: method + final URL (with query) + body. Headers (API key) are excluded.
pub fn cache_key(method: &str, url: &reqwest::Url, body: Option<&[u8]>) -> String {
    match body {
        Some(b) => format!("{method} {url} {}", String::from_utf8_lossy(b)),
        None => format!("{method} {url}"),
    }
}

impl Fetcher {
    pub fn new(http: reqwest::Client, db: Arc<Db>) -> Self {
        Self { http, db }
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Sends `req` unless a fresh cached copy exists. `force` skips the cache
    /// read (manual refresh). Network failures fall back to an expired copy.
    pub async fn fetch(
        &self,
        req: reqwest::RequestBuilder,
        ttl: i64,
        force: bool,
        source_name: &'static str,
        help_for: impl Fn(u16) -> Option<String>,
    ) -> AppResult<Fetched> {
        let req = req.build()?;
        if !host_allowed(req.url(), ALLOWED_HOSTS) {
            return Err(AppError::Invalid(format!("Blocked a request to a host that isn't on the allow-list: {}", req.url().host_str().unwrap_or("?"))));
        }
        let key = cache_key(req.method().as_str(), req.url(), req.body().and_then(|b| b.as_bytes()));

        if !force {
            if let Some(hit) = self.db.cache_get(&key, Some(ttl))? {
                return Ok(Fetched {
                    body: hit.body,
                    content_type: hit.content_type,
                    freshness: Freshness { fetched_at: hit.fetched_at, from_cache: true, stale: false },
                });
            }
        }

        let result = async {
            let resp = self.http.execute(req).await?;
            let status = resp.status();
            if !status.is_success() {
                return Err(AppError::Api { source_name, status: status.as_u16(), help: help_for(status.as_u16()) });
            }
            let ct = resp.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(String::from);
            if resp.content_length().is_some_and(|l| l as usize > MAX_BODY) {
                return Err(AppError::Decode { source_name, detail: "response too large".into() });
            }
            let body = resp.bytes().await?.to_vec();
            if body.len() > MAX_BODY {
                return Err(AppError::Decode { source_name, detail: "response too large".into() });
            }
            Ok((body, ct))
        }
        .await;

        match result {
            Ok((body, ct)) => {
                let fetched_at = self.db.cache_put(&key, &body, ct.as_deref())?;
                Ok(Fetched { body, content_type: ct, freshness: Freshness { fetched_at, from_cache: false, stale: false } })
            }
            // Offline or server trouble: an old answer beats no answer. Auth/limit errors still surface.
            Err(e @ (AppError::Network(_) | AppError::Api { status: 500..=599, .. })) => match self.db.cache_get(&key, None)? {
                Some(hit) => Ok(Fetched {
                    body: hit.body,
                    content_type: hit.content_type,
                    freshness: Freshness { fetched_at: hit.fetched_at, from_cache: true, stale: true },
                }),
                None => Err(e),
            },
            Err(e) => Err(e),
        }
    }

    pub async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
        ttl: i64,
        force: bool,
        source_name: &'static str,
        help_for: impl Fn(u16) -> Option<String>,
    ) -> AppResult<(T, Freshness)> {
        let f = self.fetch(req, ttl, force, source_name, help_for).await?;
        let v = serde_json::from_slice(&f.body).map_err(|e| AppError::Decode { source_name, detail: e.to_string() })?;
        Ok((v, f.freshness))
    }

    /// Fetches a mod icon through the cache and returns it as a data: URL,
    /// so the webview never needs network access.
    pub async fn icon_data_url(&self, url: &str) -> AppResult<String> {
        use base64::Engine;
        let parsed = reqwest::Url::parse(url).map_err(|_| AppError::Invalid("bad icon URL".into()))?;
        if !host_allowed(&parsed, ICON_HOSTS) {
            return Err(AppError::Invalid("icon host not allowed".into()));
        }
        let f = self.fetch(self.http.get(parsed), ICON_TTL, false, "Icon CDN", |_| None).await?;
        let ct = f.content_type.unwrap_or_default();
        let ct = ct.split(';').next().unwrap_or("").trim().to_string();
        if !ct.starts_with("image/") || f.body.len() > MAX_ICON {
            return Err(AppError::Invalid("not an image".into()));
        }
        Ok(format!("data:{ct};base64,{}", base64::engine::general_purpose::STANDARD.encode(&f.body)))
    }
}

pub fn status_help(source: &'static str) -> impl Fn(u16) -> Option<String> {
    move |status| match status {
        401 | 403 if source == "CurseForge" => Some(
            "CurseForge rejected the API key. Check it at console.curseforge.com → API Keys, then save it again in Settings.".into(),
        ),
        429 => Some(format!("{source} is rate-limiting requests. Wait a minute and try again.")),
        500..=599 => Some(format!("{source} is having trouble right now. Try again shortly.")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_list() {
        let ok = reqwest::Url::parse("https://api.modrinth.com/v2/search").unwrap();
        let http = reqwest::Url::parse("http://api.modrinth.com/v2/search").unwrap();
        let other = reqwest::Url::parse("https://example.com/").unwrap();
        let sneaky = reqwest::Url::parse("https://api.modrinth.com.evil.test/").unwrap();
        assert!(host_allowed(&ok, ALLOWED_HOSTS));
        assert!(!host_allowed(&http, ALLOWED_HOSTS));
        assert!(!host_allowed(&other, ALLOWED_HOSTS));
        assert!(!host_allowed(&sneaky, ALLOWED_HOSTS));
        assert!(!host_allowed(&ok, ICON_HOSTS));
    }

    #[test]
    fn keys_include_query_and_body() {
        let a = reqwest::Url::parse("https://api.curseforge.com/v1/mods?x=1").unwrap();
        let b = reqwest::Url::parse("https://api.curseforge.com/v1/mods?x=2").unwrap();
        assert_ne!(cache_key("GET", &a, None), cache_key("GET", &b, None));
        assert_ne!(cache_key("POST", &a, Some(b"[1]")), cache_key("POST", &a, Some(b"[2]")));
    }
}
