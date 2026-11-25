//! Finding the same mod on the other site.
//!
//! Confidence levels:
//! - `verified`: a file is byte-identical on both sites (same SHA-1). From
//!   CurseForge we ask Modrinth directly by hash; from Modrinth we compare the
//!   candidate's files for this version and loader.
//! - `likely`: same slug and same name.
//! - `possible`: same name, or same slug, or the same name once tags like
//!   "[ETF]" and suffixes like " - [Fabric & Forge]" are dropped.
//! - `manual`: you linked these two yourself before (remembered in the app's
//!   database), so they're offered first and pre-ticked.
//! Only `verified` matches are ever linked without asking.

use super::curseforge::{cf_id, CurseForge};
use super::modrinth::Modrinth;
use super::{norm, Loader, SearchQuery, Source};
use crate::error::AppResult;
use crate::model::PackEntry;
use crate::resolve::{Catalog, Version};
use serde::Serialize;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Manual,
    Verified,
    Likely,
    Possible,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub source: Source,
    pub id: String,
    pub slug: String,
    pub name: String,
    pub icon_url: Option<String>,
    pub downloads: u64,
    pub confidence: Confidence,
    pub reason: String,
}

/// The name without decoration: bracketed tags, a " - …" / " | …" suffix
/// (usually loaders or a tagline) and loader words.
/// "[ETF] Entity Texture Features - [Fabric & Forge]" → "entitytexturefeatures".
pub fn core_name(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    for c in s.chars() {
        match c {
            '[' | '(' | '{' => depth += 1,
            ']' | ')' | '}' => depth = (depth - 1).max(0),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    for sep in [" - ", " | ", " – ", " — "] {
        if let Some(i) = out.find(sep) {
            out.truncate(i);
        }
    }
    let words: Vec<String> = out
        .split_whitespace()
        .map(norm)
        .filter(|w| !w.is_empty() && !["fabric", "forge", "neoforge", "quilt", "and", "mod"].contains(&w.as_str()))
        .collect();
    words.join("")
}

pub fn name_confidence(slug_a: &str, name_a: &str, slug_b: &str, name_b: &str) -> Option<Confidence> {
    let slug = !norm(slug_a).is_empty() && norm(slug_a) == norm(slug_b);
    let name = !norm(name_a).is_empty() && norm(name_a) == norm(name_b);
    match (slug, name) {
        (true, true) => Some(Confidence::Likely),
        (true, false) | (false, true) => Some(Confidence::Possible),
        _ => {
            let (a, b) = (core_name(name_a), core_name(name_b));
            (a.len() >= 4 && a == b).then_some(Confidence::Possible)
        }
    }
}

fn sha1s(vs: &[Version]) -> HashSet<String> {
    vs.iter().flat_map(|v| v.files.iter()).filter_map(|f| f.hashes.get("sha1")).map(|h| h.to_lowercase()).collect()
}

fn reason_for(c: Confidence) -> &'static str {
    match c {
        Confidence::Manual => "You linked these before",
        Confidence::Verified => "Same file on both sites",
        Confidence::Likely => "Same slug and name",
        Confidence::Possible => "Similar slug or name",
    }
}

/// Candidates for the other site's copy of `entry`, best first.
pub async fn find_counterparts(mr: &Modrinth, cf: &CurseForge, entry: &PackEntry, mc: &str, loader: Loader) -> AppResult<Vec<Candidate>> {
    find_counterparts_with(mr, cf, entry, mc, loader, &[]).await
}

/// Like `find_counterparts`, with links you made yourself (Modrinth id,
/// CurseForge id) offered first.
pub async fn find_counterparts_with(mr: &Modrinth, cf: &CurseForge, entry: &PackEntry, mc: &str, loader: Loader, manual: &[(String, u64)]) -> AppResult<Vec<Candidate>> {
    let d = &entry.display;
    let mut out: Vec<Candidate> = Vec::new();
    let mut seen = HashSet::new();
    let kind = entry.kind;
    let search = |text: &str| -> SearchQuery { serde_json::from_value(serde_json::json!({ "text": text, "limit": 6, "kind": kind })).expect("query") };
    match (&entry.mod_ref.modrinth_id, entry.mod_ref.curseforge_id) {
        (Some(m), None) => {
            if let Some((_, c)) = manual.iter().find(|(x, _)| x == m) {
                if let Some(p) = cf.projects(&[cf_id(*c)], false).await.unwrap_or_default().into_iter().next() {
                    seen.insert(c.to_string());
                    out.push(Candidate { source: Source::Curseforge, id: c.to_string(), slug: p.slug, name: p.title, icon_url: p.icon_url, downloads: p.downloads, confidence: Confidence::Manual, reason: reason_for(Confidence::Manual).into() });
                }
            }
        }
        (None, Some(c)) => {
            if let Some((m, _)) = manual.iter().find(|(_, x)| *x == c) {
                if let Some(p) = mr.projects(std::slice::from_ref(m), false).await.unwrap_or_default().into_iter().next() {
                    seen.insert(p.id.clone());
                    out.push(Candidate { source: Source::Modrinth, id: p.id, slug: p.slug, name: p.title, icon_url: p.icon_url, downloads: p.downloads, confidence: Confidence::Manual, reason: reason_for(Confidence::Manual).into() });
                }
            }
        }
        _ => {}
    }

    if let (Some(mr_id), None) = (&entry.mod_ref.modrinth_id, entry.mod_ref.curseforge_id) {
        // Modrinth → CurseForge
        let mut hits = cf.by_slug(&d.slug, false).await.unwrap_or_default();
        hits.extend(cf.search_with_notes(&search(&d.name)).await?.0.hits);
        // Searching the core name too finds "[ETF] Entity Texture Features - [Fabric & Forge]" from "Entity Texture Features".
        let core = d.name.split(" - ").next().unwrap_or(&d.name).replace(['[', ']'], " ");
        if core.trim() != d.name.trim() {
            hits.extend(cf.search_with_notes(&search(core.trim())).await.map(|r| r.0.hits).unwrap_or_default());
        }
        let mine = sha1s(&mr.versions_of(mr_id, mc, loader, kind, false).await.unwrap_or_default());
        for h in hits {
            if !seen.insert(h.id.clone()) {
                continue;
            }
            let Some(mut conf) = name_confidence(&d.slug, &d.name, &h.slug, &h.name) else { continue };
            if !mine.is_empty() && out.len() < 4 {
                let theirs = sha1s(&cf.versions_of(&cf_id(h.id.parse().unwrap_or(0)), mc, loader, kind, false).await.unwrap_or_default());
                if !mine.is_disjoint(&theirs) {
                    conf = Confidence::Verified;
                }
            }
            out.push(Candidate { source: Source::Curseforge, id: h.id, slug: h.slug, name: h.name, icon_url: h.icon_url, downloads: h.downloads, confidence: conf, reason: reason_for(conf).into() });
        }
    } else if let (None, Some(cf_num)) = (&entry.mod_ref.modrinth_id, entry.mod_ref.curseforge_id) {
        // CurseForge → Modrinth: ask Modrinth about the CurseForge files' hashes first.
        let files = cf.versions_of(&cf_id(cf_num), mc, loader, kind, false).await.unwrap_or_default();
        let hashes: Vec<String> = sha1s(&files).into_iter().collect();
        let by_hash = mr.versions_by_sha1(&hashes, false).await.unwrap_or_default();
        let verified: HashSet<String> = by_hash.values().map(|v| v.project_id.clone()).collect();
        let mut infos = Vec::new();
        if !verified.is_empty() {
            infos.extend(mr.projects(&verified.iter().cloned().collect::<Vec<_>>(), false).await.unwrap_or_default());
        }
        if let Some(p) = mr.project_info(&d.slug, false).await.unwrap_or(None) {
            infos.push(p);
        }
        for p in infos {
            if !seen.insert(p.id.clone()) {
                continue;
            }
            let conf = if verified.contains(&p.id) { Some(Confidence::Verified) } else { name_confidence(&d.slug, &d.name, &p.slug, &p.title) };
            if let Some(conf) = conf {
                out.push(Candidate { source: Source::Modrinth, icon_url: p.icon_url.clone(), downloads: p.downloads, slug: p.slug, name: p.title, id: p.id, confidence: conf, reason: reason_for(conf).into() });
            }
        }
        use super::ModSource;
        let mut hits = mr.search(&search(&d.name)).await.map(|p| p.hits).unwrap_or_default();
        let core = d.name.split(" - ").next().unwrap_or(&d.name).replace(['[', ']'], " ");
        if core.trim() != d.name.trim() {
            hits.extend(mr.search(&search(core.trim())).await.map(|p| p.hits).unwrap_or_default());
        }
        for h in hits {
            if !seen.insert(h.id.clone()) {
                continue;
            }
            if let Some(conf) = name_confidence(&d.slug, &d.name, &h.slug, &h.name) {
                out.push(Candidate { source: Source::Modrinth, id: h.id, slug: h.slug, name: h.name, icon_url: h.icon_url, downloads: h.downloads, confidence: conf, reason: reason_for(conf).into() });
            }
        }
    }
    out.sort_by(|a, b| a.confidence.cmp(&b.confidence).then(b.downloads.cmp(&a.downloads)));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn name_levels() {
        assert_eq!(name_confidence("jei", "Just Enough Items (JEI)", "jei", "Just Enough Items (JEI)"), Some(Confidence::Likely));
        assert_eq!(name_confidence("jei", "Just Enough Items", "just-enough-items", "Just Enough Items"), Some(Confidence::Possible));
        assert_eq!(name_confidence("sodium", "Sodium", "sodium-extra", "Sodium Extra"), None);
        assert_eq!(name_confidence("", "", "", ""), None);
        assert!(Confidence::Manual < Confidence::Verified && Confidence::Verified < Confidence::Likely, "sorts best first");
        assert_eq!(core_name("[ETF] Entity Texture Features - [Fabric & Forge]"), core_name("[ETF] Entity Texture Features"));
        assert_eq!(name_confidence("entitytexturefeatures", "[ETF] Entity Texture Features", "entity-texture-features-fabric", "[ETF] Entity Texture Features - [Fabric & Forge]"), Some(Confidence::Possible));
        assert_eq!(name_confidence("a", "Create", "b", "Create: Steam 'n' Rails"), None, "a longer name isn't the same mod");
    }
}
