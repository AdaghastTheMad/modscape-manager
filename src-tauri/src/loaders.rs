//! Loader versions from each loader's own metadata service.

use crate::error::{AppError, AppResult};
use crate::fetch::{status_help, Fetcher, DAY};
use crate::sources::Loader;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct LoaderVersion {
    pub version: String,
    pub stable: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoaderVersions {
    pub versions: Vec<LoaderVersion>,
    /// Shown when the list may be incomplete or empty for a known reason.
    pub note: Option<String>,
}

/// Compares versions segment by segment, numbers numerically ("21.1.10" > "21.1.9").
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let seg = |s: &str| -> Vec<(u64, String)> {
        s.split(['.', '-', '+'])
            .map(|p| {
                let digits: String = p.chars().take_while(|c| c.is_ascii_digit()).collect();
                (digits.parse().unwrap_or(0), p[digits.len()..].to_string())
            })
            .collect()
    };
    let (sa, sb) = (seg(a), seg(b));
    for i in 0..sa.len().max(sb.len()) {
        match (sa.get(i), sb.get(i)) {
            (Some(x), Some(y)) => {
                let o = x.0.cmp(&y.0).then_with(|| match (x.1.is_empty(), y.1.is_empty()) {
                    (true, false) => Ordering::Greater, // "1" > "1beta"
                    (false, true) => Ordering::Less,
                    _ => x.1.cmp(&y.1),
                });
                if o != Ordering::Equal {
                    return o;
                }
            }
            // A release beats a longer pre-release tail ("21.1.0" vs "21.1.0-beta").
            (Some(x), None) => return if x.0 == 0 && !x.1.is_empty() { Ordering::Less } else { Ordering::Greater },
            (None, Some(y)) => return if y.0 == 0 && !y.1.is_empty() { Ordering::Greater } else { Ordering::Less },
            (None, None) => break,
        }
    }
    Ordering::Equal
}

fn is_prerelease(v: &str) -> bool {
    let l = v.to_ascii_lowercase();
    ["alpha", "beta", "pre", "rc", "snapshot"].iter().any(|t| l.contains(t))
}

/// NeoForge's version prefix for a Minecraft version:
/// 1.21.1 → "21.1.", 1.21 → "21.0.", 26.1.2 → "26.1.2.", 26.1 → "26.1.0.".
pub fn neoforge_prefix(mc: &str) -> Option<String> {
    if mc.contains(|c: char| !(c.is_ascii_digit() || c == '.')) {
        return None; // snapshots, pre-releases
    }
    let p: Vec<&str> = mc.split('.').collect();
    match p.as_slice() {
        ["1", minor] => Some(format!("{minor}.0.")),
        ["1", minor, patch] => Some(format!("{minor}.{patch}.")),
        [year, a] if year.len() == 2 => Some(format!("{year}.{a}.0.")),
        [year, a, b] if year.len() == 2 => Some(format!("{year}.{a}.{b}.")),
        _ => None,
    }
}

pub fn filter_neoforge(all: &[String], mc: &str) -> Vec<LoaderVersion> {
    let Some(prefix) = neoforge_prefix(mc) else { return vec![] };
    let mut v: Vec<LoaderVersion> = all
        .iter()
        .filter(|v| v.starts_with(&prefix))
        .map(|v| LoaderVersion { version: v.clone(), stable: !is_prerelease(v) })
        .collect();
    v.sort_by(|a, b| version_cmp(&b.version, &a.version));
    v
}

pub fn filter_forge(xml: &str, mc: &str) -> Vec<LoaderVersion> {
    let prefix = format!("{mc}-");
    let mut v: Vec<LoaderVersion> = xml
        .split("<version>")
        .skip(1)
        .filter_map(|s| s.split("</version>").next())
        .filter(|s| s.starts_with(&prefix))
        .map(|s| {
            let ver = s.to_string(); // Forge installs are named "mc-forge", keep the full id
            LoaderVersion { stable: !is_prerelease(&ver), version: ver }
        })
        .collect();
    v.sort_by(|a, b| version_cmp(&b.version[prefix.len()..], &a.version[prefix.len()..]));
    v.dedup();
    v
}

#[derive(Deserialize)]
struct MetaEntry {
    loader: MetaLoader,
}
#[derive(Deserialize)]
struct MetaLoader {
    version: String,
    stable: Option<bool>,
}

pub async fn loader_versions(f: &Fetcher, loader: Loader, mc: &str, force: bool) -> AppResult<LoaderVersions> {
    let mc = mc.trim();
    if mc.is_empty() {
        return Err(AppError::Invalid("Pick a Minecraft version first.".into()));
    }
    let enc: String = mc.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')).collect();
    let (versions, none_note) = match loader {
        Loader::Fabric | Loader::Quilt => {
            let (url, name) = if loader == Loader::Fabric {
                (format!("https://meta.fabricmc.net/v2/versions/loader/{enc}"), "Fabric meta")
            } else {
                (format!("https://meta.quiltmc.org/v3/versions/loader/{enc}"), "Quilt meta")
            };
            let res = f.fetch_json::<Vec<MetaEntry>>(f.http().get(url), DAY, force, name, status_help(name)).await;
            let list = match res {
                Ok((l, _)) => l,
                // Unknown game version → 400/404 from the meta service: treat as "none".
                Err(AppError::Api { status: 400 | 404, .. }) => vec![],
                Err(e) => return Err(e),
            };
            let mut v: Vec<LoaderVersion> = list
                .into_iter()
                .map(|e| LoaderVersion { stable: e.loader.stable.unwrap_or(!is_prerelease(&e.loader.version)), version: e.loader.version })
                .collect();
            v.sort_by(|a, b| version_cmp(&b.version, &a.version));
            (v, format!("{} has no loader builds listed for {mc}.", if loader == Loader::Fabric { "Fabric" } else { "Quilt" }))
        }
        Loader::Neoforge => {
            #[derive(Deserialize)]
            struct R {
                versions: Vec<String>,
            }
            let url = "https://maven.neoforged.net/api/maven/versions/releases/net/neoforged/neoforge";
            let (r, _) = f.fetch_json::<R>(f.http().get(url), DAY, force, "NeoForge maven", status_help("NeoForge maven")).await?;
            (filter_neoforge(&r.versions, mc), format!("NeoForge has no builds listed for {mc}."))
        }
        Loader::Forge => {
            let url = "https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml";
            let got = f.fetch(f.http().get(url), DAY, force, "Forge maven", status_help("Forge maven")).await?;
            (filter_forge(&String::from_utf8_lossy(&got.body), mc), format!("Forge has no builds listed for {mc}."))
        }
    };
    let note = versions.is_empty().then(|| format!("{none_note} You can still type a loader version by hand."));
    Ok(LoaderVersions { versions, note })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neoforge_prefixes() {
        assert_eq!(neoforge_prefix("1.21.1").as_deref(), Some("21.1."));
        assert_eq!(neoforge_prefix("1.21").as_deref(), Some("21.0."));
        assert_eq!(neoforge_prefix("26.1.2").as_deref(), Some("26.1.2."));
        assert_eq!(neoforge_prefix("26.1").as_deref(), Some("26.1.0."));
        assert_eq!(neoforge_prefix("26.4-snapshot-1"), None);
        assert_eq!(neoforge_prefix("25w14a"), None);
    }

    #[test]
    fn neoforge_filter_and_sort() {
        let all: Vec<String> = ["21.1.9", "21.1.10", "21.1.267", "21.10.1", "26.1.2.124", "26.1.2.129", "26.1.0.5", "26.3.0.75-beta"]
            .iter().map(|s| s.to_string()).collect();
        let v: Vec<String> = filter_neoforge(&all, "1.21.1").into_iter().map(|v| v.version).collect();
        assert_eq!(v, vec!["21.1.267", "21.1.10", "21.1.9"]);
        let v: Vec<String> = filter_neoforge(&all, "26.1.2").into_iter().map(|v| v.version).collect();
        assert_eq!(v, vec!["26.1.2.129", "26.1.2.124"]);
        let beta = filter_neoforge(&all, "26.3");
        assert_eq!(beta.len(), 1);
        assert!(!beta[0].stable);
    }

    #[test]
    fn forge_filter() {
        let xml = "<versions><version>1.21.1-52.1.21</version><version>1.21.1-52.1.3</version><version>1.21.10-60.0.1</version><version>26.3-66.1.9</version></versions>";
        let v: Vec<String> = filter_forge(xml, "1.21.1").into_iter().map(|v| v.version).collect();
        assert_eq!(v, vec!["1.21.1-52.1.21", "1.21.1-52.1.3"]);
    }

    #[test]
    fn version_ordering() {
        assert_eq!(version_cmp("0.20.1", "0.20.0"), Ordering::Greater);
        assert_eq!(version_cmp("0.20.0", "0.20.0-beta.9"), Ordering::Greater);
        assert_eq!(version_cmp("0.20.0-beta.10", "0.20.0-beta.9"), Ordering::Greater);
        assert_eq!(version_cmp("21.1.10", "21.1.9"), Ordering::Greater);
    }
}
