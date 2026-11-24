//! Merged search: Modrinth and CurseForge side by side, the same mod shown once.

use super::{norm, AltRef, SearchHit, SearchPage, Sort, Source};
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Serialize, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PerSource<T> {
    pub modrinth: Option<T>,
    pub curseforge: Option<T>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedPage {
    pub hits: Vec<SearchHit>,
    pub totals: PerSource<u64>,
    /// Offsets for "Load more"; None when that site has nothing more.
    pub next: PerSource<u32>,
    pub notes: Vec<String>,
    /// Some results came from an expired cache because a site was unreachable.
    pub stale: bool,
}

fn alt(h: &SearchHit) -> AltRef {
    AltRef { source: h.source, id: h.id.clone(), slug: h.slug.clone(), name: h.name.clone(), downloads: h.downloads }
}

/// Same mod on both sites: identical slug *and* name once normalised.
/// Deliberately strict; looser matches go through the "find match" flow
/// where the user confirms.
pub fn same_mod(a: &SearchHit, b: &SearchHit) -> bool {
    let (sa, sb) = (norm(&a.slug), norm(&b.slug));
    !sa.is_empty() && sa == sb && norm(&a.name) == norm(&b.name)
}

/// Merges two result pages. `prefer` decides which site's copy leads a merged row.
pub fn merge(mr: Option<&SearchPage>, cf: Option<&SearchPage>, sort: Sort, prefer: Source) -> Vec<SearchHit> {
    let mr_hits: Vec<SearchHit> = mr.map(|p| p.hits.clone()).unwrap_or_default();
    let mut cf_hits: Vec<Option<SearchHit>> = cf.map(|p| p.hits.iter().cloned().map(Some).collect()).unwrap_or_default();
    let cf_index: HashMap<(String, String), usize> =
        cf_hits.iter().enumerate().filter_map(|(i, h)| h.as_ref().map(|h| ((norm(&h.slug), norm(&h.name)), i))).collect();

    // Modrinth rows, absorbing their CurseForge twins.
    let mut left: Vec<SearchHit> = Vec::new();
    for mut h in mr_hits {
        if let Some(&i) = cf_index.get(&(norm(&h.slug), norm(&h.name))) {
            if let Some(c) = cf_hits[i].take().filter(|c| same_mod(&h, c)) {
                let mut lead_cf = c.clone();
                if prefer == Source::Curseforge {
                    lead_cf.also = Some(alt(&h));
                    // Modrinth knows sides and licences; keep them on the merged row.
                    lead_cf.client_side = h.client_side.clone();
                    lead_cf.server_side = h.server_side.clone();
                    lead_cf.license = h.license.clone();
                    lead_cf.compatible = merge_compat(h.compatible, c.compatible);
                    left.push(lead_cf);
                } else {
                    h.also = Some(alt(&c));
                    h.compatible = merge_compat(h.compatible, c.compatible);
                    h.distribution_blocked = false; // the Modrinth copy is what gets used
                    left.push(h);
                }
                continue;
            }
        }
        left.push(h);
    }
    let right: Vec<SearchHit> = cf_hits.into_iter().flatten().collect();

    let total_downloads = |h: &SearchHit| h.downloads + h.also.as_ref().map_or(0, |a| a.downloads);
    match sort {
        Sort::Downloads => {
            let mut all: Vec<SearchHit> = left.into_iter().chain(right).collect();
            all.sort_by(|a, b| total_downloads(b).cmp(&total_downloads(a)));
            all
        }
        Sort::Updated => {
            let mut all: Vec<SearchHit> = left.into_iter().chain(right).collect();
            all.sort_by(|a, b| b.updated.cmp(&a.updated));
            all
        }
        // Each site ranks relevance its own way: interleave, preserving each site's order.
        _ => {
            let mut out = Vec::with_capacity(left.len() + right.len());
            let (mut l, mut r) = (left.into_iter(), right.into_iter());
            loop {
                match (l.next(), r.next()) {
                    (None, None) => break,
                    (a, b) => out.extend(a.into_iter().chain(b)),
                }
            }
            out
        }
    }
}

/// A merged row is compatible if either copy is.
fn merge_compat(a: Option<bool>, b: Option<bool>) -> Option<bool> {
    match (a, b) {
        (None, None) => None,
        (x, y) => Some(x.unwrap_or(false) || y.unwrap_or(false)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(source: Source, id: &str, slug: &str, name: &str, downloads: u64) -> SearchHit {
        SearchHit {
            source,
            id: id.into(),
            slug: slug.into(),
            name: name.into(),
            summary: String::new(),
            authors: vec![],
            downloads,
            follows: None,
            icon_url: None,
            updated: Some(format!("2026-0{}-01", downloads % 9 + 1)),
            categories: vec![],
            license: None,
            client_side: Some("required".into()),
            server_side: Some("unsupported".into()),
            compatible: Some(source == Source::Modrinth),
            distribution_blocked: source == Source::Curseforge,
            also: None,
        }
    }
    fn page(source: Source, hits: Vec<SearchHit>) -> SearchPage {
        SearchPage { source, total: hits.len() as u64, hits, freshness: None }
    }

    #[test]
    fn merges_twins_and_interleaves() {
        let mr = page(Source::Modrinth, vec![hit(Source::Modrinth, "AANobbMI", "sodium", "Sodium", 9), hit(Source::Modrinth, "m2", "mr-only", "MR Only", 5)]);
        let cf = page(Source::Curseforge, vec![
            hit(Source::Curseforge, "394468", "sodium", "Sodium", 7),
            hit(Source::Curseforge, "c2", "cf-only", "CF Only", 3),
            hit(Source::Curseforge, "c3", "sodium", "Sodium Extra Stuff", 1), // same slug, different name: not merged
        ]);
        let out = merge(Some(&mr), Some(&cf), Sort::Relevance, Source::Modrinth);
        let ids: Vec<&str> = out.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(ids, vec!["AANobbMI", "c2", "m2", "c3"]);
        let sodium = &out[0];
        assert_eq!(sodium.also.as_ref().unwrap().id, "394468");
        assert_eq!(sodium.compatible, Some(true));
        assert!(!sodium.distribution_blocked);
    }

    #[test]
    fn prefer_curseforge_leads_with_curseforge_copy() {
        let mr = page(Source::Modrinth, vec![hit(Source::Modrinth, "AANobbMI", "sodium", "Sodium", 9)]);
        let cf = page(Source::Curseforge, vec![hit(Source::Curseforge, "394468", "sodium", "Sodium", 7)]);
        let out = merge(Some(&mr), Some(&cf), Sort::Relevance, Source::Curseforge);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].source, Source::Curseforge);
        assert_eq!(out[0].also.as_ref().unwrap().source, Source::Modrinth);
        assert_eq!(out[0].client_side.as_deref(), Some("required"), "keeps Modrinth's side info");
    }

    #[test]
    fn download_sort_uses_both_sites_counts() {
        let mr = page(Source::Modrinth, vec![hit(Source::Modrinth, "a", "a", "A", 5), hit(Source::Modrinth, "b", "b", "B", 6)]);
        let cf = page(Source::Curseforge, vec![hit(Source::Curseforge, "a2", "a", "A", 5)]);
        let out = merge(Some(&mr), Some(&cf), Sort::Downloads, Source::Modrinth);
        assert_eq!(out.iter().map(|h| h.id.as_str()).collect::<Vec<_>>(), vec!["a", "b"], "A has 10 across both sites");
    }

    #[test]
    fn one_site_only() {
        let mr = page(Source::Modrinth, vec![hit(Source::Modrinth, "a", "a", "A", 5)]);
        assert_eq!(merge(Some(&mr), None, Sort::Relevance, Source::Modrinth).len(), 1);
        assert_eq!(merge(None, None, Sort::Relevance, Source::Modrinth).len(), 0);
    }
}
