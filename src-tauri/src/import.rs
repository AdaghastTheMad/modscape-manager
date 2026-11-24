//! Importers: `.mrpack`, CurseForge zip, and a Prism instance folder.
//! Everything is identified from metadata (hashes, ids, Prism's `.index`
//! files); jars are only ever *read* to hash them, never changed. Imported
//! mods are pinned to the exact file the pack used.

use crate::error::{AppError, AppResult};
use crate::model::{self, EntryDisplay, EntryKind, Pack, PackEntry, Reason};
use crate::resolve::{self, Catalog, Version};
use crate::sources::curseforge::{self, cf_id};
use crate::sources::Loader;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Read;
use std::path::Path;

/// Hash lookups the importers need on top of the resolver catalog.
#[async_trait::async_trait]
pub trait Lookup: Catalog {
    async fn modrinth_by_sha1(&self, hashes: &[String]) -> AppResult<HashMap<String, Version>>;
    async fn curseforge_by_fingerprint(&self, fps: &[u32]) -> AppResult<HashMap<u32, Version>>;
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub pack: Pack,
    /// What happened, for the import report.
    pub notes: Vec<String>,
    /// Files that couldn't be identified (shown to the user).
    pub unknown: Vec<String>,
    /// The archive or instance had config/override files (kept for override templates, M9).
    pub has_overrides: bool,
}

const MAX_ARCHIVE: u64 = 64 * 1024 * 1024;
const MAX_MANIFEST: u64 = 16 * 1024 * 1024;

fn zip_text(bytes: &[u8], name: &str) -> AppResult<(String, bool)> {
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| AppError::Invalid(format!("That isn't a readable zip: {e}")))?;
    let has_overrides = z.file_names().filter_map(|n| n.ok()).any(|n| (n.starts_with("overrides/") || n.starts_with("client-overrides/")) && !n.ends_with('/'));
    let mut f = z.by_name(name).map_err(|_| AppError::Invalid(format!("The archive has no {name}.")))?;
    if f.size() > MAX_MANIFEST {
        return Err(AppError::Invalid(format!("{name} is unreasonably large.")));
    }
    let mut s = String::new();
    f.read_to_string(&mut s).map_err(|e| AppError::Invalid(format!("Couldn't read {name}: {e}")))?;
    Ok((s, has_overrides))
}

/// A file pinned to a known project.
struct Found {
    project: String, // catalog id (cf: for CurseForge)
    file: String,    // catalog id (cf: for CurseForge)
}

fn new_pack(name: &str, mc: &str, loader: Loader, loader_version: &str) -> Pack {
    let now = crate::store::now_iso();
    serde_json::from_value(serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "name": if name.trim().is_empty() { "Imported pack" } else { name.trim() },
        "mcVersion": mc, "loader": loader, "loaderVersion": loader_version,
        "createdAt": now, "updatedAt": now
    }))
    .expect("pack shape")
}

/// Turns identified files into pinned entries, then lets a check pass mark
/// libraries that other mods require as dependencies (so they dim and can be
/// cleaned up like ones the app added itself).
async fn finish(mut pack: Pack, found: Vec<Found>, cat: &dyn Catalog, notes: &mut Vec<String>) -> Pack {
    let mut seen = HashSet::new();
    let found: Vec<Found> = found.into_iter().filter(|f| seen.insert(f.project.clone())).collect();
    let ids: Vec<String> = found.iter().map(|f| f.project.clone()).collect();
    let infos: HashMap<String, resolve::ProjectInfo> = cat.projects(&ids, false).await.unwrap_or_default().into_iter().map(|p| (p.id.clone(), p)).collect();
    let now = crate::store::now_iso();
    for f in found {
        let display = infos.get(&f.project).map(|p| p.display()).unwrap_or_else(|| EntryDisplay { name: f.project.clone(), ..Default::default() });
        pack.entries.insert(
            uuid::Uuid::new_v4().to_string(),
            PackEntry {
                mod_ref: resolve::mod_ref_for(&f.project, None),
                file_id: Some(f.file.strip_prefix(curseforge::PREFIX).unwrap_or(&f.file).to_string()),
                kind: EntryKind::Mod,
                reason: Reason::User,
                pulled_in_by: vec![],
                payload_ids: vec![],
                tags: vec![],
                notes: None,
                side: None,
                graph_pos: None,
                display,
                library_override: None,
                added_at: now.clone(),
                extra: Default::default(),
            },
        );
    }
    let a = resolve::analyze(&pack, cat, false, crate::store::now_secs()).await;
    let mut deps = 0;
    for (k, rep) in &a.entries {
        if let Some(e) = pack.entries.get_mut(k) {
            e.pulled_in_by = rep.required_by.clone();
            if !rep.required_by.is_empty() && e.display.categories.iter().any(|c| c == "library") {
                e.reason = Reason::Dependency;
                deps += 1;
            }
        }
    }
    if deps > 0 {
        notes.push(format!("Marked {deps} librar{} other mods need as dependencies.", if deps == 1 { "y" } else { "ies" }));
    }
    let _ = model::normalize(&mut pack);
    pack
}

fn loader_from_key(k: &str) -> Option<Loader> {
    match k {
        "neoforge" => Some(Loader::Neoforge),
        "forge" => Some(Loader::Forge),
        "fabric-loader" | "fabric" => Some(Loader::Fabric),
        "quilt-loader" | "quilt" => Some(Loader::Quilt),
        _ => None,
    }
}

/// CurseForge download URLs encode the file id: `/files/1234/567/name.jar` → 1234567.
pub fn cf_file_id_from_url(url: &str) -> Option<u64> {
    let u = reqwest::Url::parse(url).ok()?;
    if !u.host_str()?.ends_with("forgecdn.net") {
        return None;
    }
    let seg: Vec<&str> = u.path_segments()?.collect();
    let i = seg.iter().position(|s| *s == "files")?;
    // The id is split as its leading digits and its last three (leading zeros dropped).
    let a: u64 = seg.get(i + 1)?.parse().ok()?;
    let b: u64 = seg.get(i + 2)?.parse().ok()?;
    (b < 1000).then_some(a * 1000 + b)
}

/// Detects the format from the archive's contents.
pub async fn import_archive(bytes: &[u8], look: &dyn Lookup) -> AppResult<Imported> {
    let z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| AppError::Invalid(format!("That isn't a readable zip: {e}")))?;
    let names: Vec<String> = z.file_names().filter_map(|n| n.ok()).map(String::from).collect();
    if names.iter().any(|n| n == "modrinth.index.json") {
        import_mrpack(bytes, look).await
    } else if names.iter().any(|n| n == "manifest.json") {
        import_cf_zip(bytes, look).await
    } else {
        Err(AppError::Invalid("That archive isn't an .mrpack or a CurseForge modpack zip.".into()))
    }
}

// ---------------------------------------------------------------- .mrpack
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrIndex {
    #[serde(default)]
    name: String,
    #[serde(default)]
    files: Vec<MrFile>,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MrFile {
    path: String,
    #[serde(default)]
    hashes: HashMap<String, String>,
    #[serde(default)]
    downloads: Vec<String>,
}

pub async fn import_mrpack(bytes: &[u8], look: &dyn Lookup) -> AppResult<Imported> {
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err(AppError::Invalid("That file is too large to be a modpack.".into()));
    }
    let (text, has_overrides) = zip_text(bytes, "modrinth.index.json")?;
    let idx: MrIndex = serde_json::from_str(&text).map_err(|e| AppError::Invalid(format!("modrinth.index.json is damaged: {e}")))?;
    let mc = idx.dependencies.get("minecraft").cloned().ok_or_else(|| AppError::Invalid("The pack doesn't say which Minecraft version it's for.".into()))?;
    let (loader, lv) = idx
        .dependencies
        .iter()
        .find_map(|(k, v)| loader_from_key(k).map(|l| (l, v.clone())))
        .ok_or_else(|| AppError::Invalid("The pack doesn't name a mod loader (vanilla packs aren't supported).".into()))?;
    let mut notes = Vec::new();
    let mut unknown = Vec::new();
    let mods: Vec<&MrFile> = idx.files.iter().filter(|f| f.path.starts_with("mods/")).collect();
    let skipped = idx.files.len() - mods.len();
    if skipped > 0 {
        notes.push(format!("Skipped {skipped} non-mod file{} (resource packs, shaders and the like arrive in milestone 11).", if skipped == 1 { "" } else { "s" }));
    }
    let hashes: Vec<String> = mods.iter().filter_map(|f| f.hashes.get("sha1").map(|h| h.to_lowercase())).collect();
    let by_hash = look.modrinth_by_sha1(&hashes).await?;
    let mut found = Vec::new();
    let mut cf_files = Vec::new();
    for f in mods {
        if let Some(v) = f.hashes.get("sha1").and_then(|h| by_hash.get(&h.to_lowercase())) {
            found.push(Found { project: v.project_id.clone(), file: v.id.clone() });
        } else if let Some(n) = f.downloads.iter().find_map(|u| cf_file_id_from_url(u)) {
            cf_files.push((n, f.path.clone()));
        } else {
            unknown.push(f.path.trim_start_matches("mods/").to_string());
        }
    }
    for (n, path) in cf_files {
        match look.version(&cf_id(n), false).await {
            Ok(Some(v)) => found.push(Found { project: v.project_id.clone(), file: v.id }),
            Ok(None) => unknown.push(path.trim_start_matches("mods/").to_string()),
            Err(AppError::NoCurseForgeKey) => {
                unknown.push(path.trim_start_matches("mods/").to_string());
                if !notes.iter().any(|n: &String| n.contains("CurseForge key")) {
                    notes.push("Some files come from CurseForge; add a CurseForge key in Settings to identify them.".into());
                }
            }
            Err(e) => return Err(e),
        }
    }
    notes.insert(0, format!("Identified {} of {} mods.", found.len(), found.len() + unknown.len()));
    let pack = finish(new_pack(&idx.name, &mc, loader, &lv), found, look, &mut notes).await;
    Ok(Imported { pack, notes, unknown, has_overrides })
}

// ---------------------------------------------------------------- CurseForge zip
#[derive(Deserialize)]
struct CfManifest {
    minecraft: CfMinecraft,
    #[serde(default)]
    name: String,
    #[serde(default)]
    files: Vec<CfFile>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfMinecraft {
    version: String,
    #[serde(default)]
    mod_loaders: Vec<CfLoader>,
}
#[derive(Deserialize)]
struct CfLoader {
    id: String,
    #[serde(default)]
    primary: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CfFile {
    #[serde(rename = "projectID")]
    project_id: u64,
    #[serde(rename = "fileID")]
    file_id: u64,
}

pub async fn import_cf_zip(bytes: &[u8], look: &dyn Lookup) -> AppResult<Imported> {
    if bytes.len() as u64 > MAX_ARCHIVE {
        return Err(AppError::Invalid("That file is too large to be a modpack.".into()));
    }
    let (text, has_overrides) = zip_text(bytes, "manifest.json")?;
    let m: CfManifest = serde_json::from_str(&text).map_err(|e| AppError::Invalid(format!("manifest.json is damaged: {e}")))?;
    let primary = m.minecraft.mod_loaders.iter().find(|l| l.primary).or(m.minecraft.mod_loaders.first());
    let (loader, lv) = primary
        .and_then(|l| l.id.split_once('-').and_then(|(k, v)| loader_from_key(k).map(|l| (l, v.to_string()))))
        .ok_or_else(|| AppError::Invalid("The pack doesn't name a mod loader.".into()))?;
    let lv = if loader == Loader::Forge { format!("{}-{lv}", m.minecraft.version) } else { lv };
    // Make sure there's a key before promising anything.
    look.projects(&[cf_id(m.files.first().map(|f| f.project_id).unwrap_or(0))], false).await?;
    let found = m.files.iter().map(|f| Found { project: cf_id(f.project_id), file: cf_id(f.file_id) }).collect();
    let mut notes = vec![format!("Read {} mods from the manifest.", m.files.len())];
    let pack = finish(new_pack(&m.name, &m.minecraft.version, loader, &lv), found, look, &mut notes).await;
    Ok(Imported { pack, notes, unknown: vec![], has_overrides })
}

// ---------------------------------------------------------------- Prism instance
#[derive(Deserialize)]
struct MmcPack {
    #[serde(default)]
    components: Vec<MmcComponent>,
}
#[derive(Deserialize)]
struct MmcComponent {
    uid: String,
    #[serde(default)]
    version: String,
}

fn sha1_hex(bytes: &[u8]) -> String {
    use sha1::Digest;
    let d = sha1::Sha1::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads (never writes) a Prism instance folder: `mmc-pack.json` for the
/// version and loader, `mods/.index/*.pw.toml` where Prism recorded where a
/// mod came from, and file hashes for jars without that record.
pub async fn import_prism_instance(dir: &Path, look: &dyn Lookup) -> AppResult<Imported> {
    let read = |p: &Path| std::fs::read_to_string(p).map_err(|e| AppError::Invalid(format!("Couldn't read {}: {e}", p.display())));
    let mmc_path = dir.join("mmc-pack.json");
    if !mmc_path.exists() {
        return Err(AppError::Invalid("That folder isn't a Prism instance (no mmc-pack.json). Pick the instance's own folder.".into()));
    }
    let mmc: MmcPack = serde_json::from_str(&read(&mmc_path)?).map_err(|e| AppError::Invalid(format!("mmc-pack.json is damaged: {e}")))?;
    let comp = |uid: &str| mmc.components.iter().find(|c| c.uid == uid).map(|c| c.version.clone());
    let mc = comp("net.minecraft").ok_or_else(|| AppError::Invalid("The instance has no Minecraft version.".into()))?;
    let (loader, lv) = [
        ("net.neoforged", Loader::Neoforge),
        ("net.minecraftforge", Loader::Forge),
        ("net.fabricmc.fabric-loader", Loader::Fabric),
        ("org.quiltmc.quilt-loader", Loader::Quilt),
    ]
    .iter()
    .find_map(|(uid, l)| comp(uid).map(|v| (*l, v)))
    .ok_or_else(|| AppError::Invalid("The instance doesn't use a mod loader.".into()))?;
    let lv = if loader == Loader::Forge && !lv.starts_with(&mc) { format!("{mc}-{lv}") } else { lv };
    let name = read(&dir.join("instance.cfg"))
        .ok()
        .and_then(|cfg| cfg.lines().find_map(|l| l.strip_prefix("name=").map(|s| s.trim().to_string())))
        .unwrap_or_else(|| dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default());
    let game = ["minecraft", ".minecraft"].iter().map(|d| dir.join(d)).find(|p| p.is_dir()).ok_or_else(|| AppError::Invalid("The instance has no minecraft folder.".into()))?;
    let mods_dir = game.join("mods");
    let has_overrides = game.join("config").is_dir() || game.join("kubejs").is_dir();

    let mut notes = Vec::new();
    let mut unknown = Vec::new();
    let mut found = Vec::new();
    let mut by_filename: HashSet<String> = HashSet::new();

    // 1. Prism's own records.
    let index = mods_dir.join(".index");
    if index.is_dir() {
        for ent in std::fs::read_dir(&index).map_err(|e| AppError::Invalid(e.to_string()))?.flatten() {
            let p = ent.path();
            if p.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }
            let Ok(t) = read(&p) else { continue };
            let Ok(v) = t.parse::<toml::Table>() else {
                notes.push(format!("Skipped an unreadable record: {}", p.file_name().unwrap_or_default().to_string_lossy()));
                continue;
            };
            let filename = v.get("filename").and_then(|x| x.as_str()).unwrap_or_default().to_string();
            let upd = v.get("update").and_then(|u| u.as_table());
            let mr = upd.and_then(|u| u.get("modrinth")).and_then(|m| m.as_table());
            let cf = upd.and_then(|u| u.get("curseforge")).and_then(|m| m.as_table());
            if let (Some(pid), Some(vid)) = (mr.and_then(|m| m.get("mod-id")).and_then(|x| x.as_str()), mr.and_then(|m| m.get("version")).and_then(|x| x.as_str())) {
                found.push(Found { project: pid.into(), file: vid.into() });
                by_filename.insert(filename);
            } else if let (Some(pid), Some(fid)) = (cf.and_then(|m| m.get("project-id")).and_then(|x| x.as_integer()), cf.and_then(|m| m.get("file-id")).and_then(|x| x.as_integer())) {
                found.push(Found { project: cf_id(pid as u64), file: cf_id(fid as u64) });
                by_filename.insert(filename);
            }
        }
    }

    // 2. Jars without a record: hash lookup on Modrinth, then CurseForge fingerprints.
    let mut loose: Vec<(String, Vec<u8>)> = Vec::new();
    let mut disabled = 0;
    if mods_dir.is_dir() {
        for ent in std::fs::read_dir(&mods_dir).map_err(|e| AppError::Invalid(e.to_string()))?.flatten() {
            let p = ent.path();
            let fname = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if fname.ends_with(".disabled") {
                disabled += 1;
                continue;
            }
            if !p.is_file() || !fname.ends_with(".jar") || by_filename.contains(&fname) {
                continue;
            }
            match std::fs::read(&p) {
                Ok(b) => loose.push((fname, b)),
                Err(e) => notes.push(format!("Couldn't read {fname}: {e}")),
            }
        }
    }
    if disabled > 0 {
        notes.push(format!("Left out {disabled} disabled mod{}.", if disabled == 1 { "" } else { "s" }));
    }
    if !loose.is_empty() {
        let sha: Vec<String> = loose.iter().map(|(_, b)| sha1_hex(b)).collect();
        let by_hash = look.modrinth_by_sha1(&sha).await.unwrap_or_default();
        let mut rest = Vec::new();
        for ((fname, bytes), h) in loose.into_iter().zip(sha) {
            match by_hash.get(&h) {
                Some(v) => found.push(Found { project: v.project_id.clone(), file: v.id.clone() }),
                None => rest.push((fname, curseforge::fingerprint(&bytes))),
            }
        }
        if !rest.is_empty() {
            let fps: Vec<u32> = rest.iter().map(|(_, f)| *f).collect();
            match look.curseforge_by_fingerprint(&fps).await {
                Ok(m) => {
                    for (fname, fp) in rest {
                        match m.get(&fp) {
                            Some(v) => found.push(Found { project: v.project_id.clone(), file: v.id.clone() }),
                            None => unknown.push(fname),
                        }
                    }
                }
                Err(AppError::NoCurseForgeKey) => {
                    notes.push("Some jars aren't on Modrinth; add a CurseForge key in Settings to identify them.".into());
                    unknown.extend(rest.into_iter().map(|(f, _)| f));
                }
                Err(_) => unknown.extend(rest.into_iter().map(|(f, _)| f)),
            }
        }
    }
    notes.insert(0, format!("Identified {} of {} mods.", found.len(), found.len() + unknown.len()));
    let pack = finish(new_pack(&name, &mc, loader, &lv), found, look, &mut notes).await;
    Ok(Imported { pack, notes, unknown, has_overrides })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cf_urls_decode_file_ids() {
        assert_eq!(cf_file_id_from_url("https://edge.forgecdn.net/files/5101/366/jei-1.21.1.jar"), Some(5_101_366));
        assert_eq!(cf_file_id_from_url("https://mediafilez.forgecdn.net/files/4925/42/x.jar"), Some(4_925_042));
        assert_eq!(cf_file_id_from_url("https://cdn.modrinth.com/data/x/versions/y/z.jar"), None);
    }
}
