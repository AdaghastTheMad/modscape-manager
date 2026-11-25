//! Pack exporters (metadata only: launchers download the mods).
//!
//! - `.mrpack` (https://docs.modrinth.com/modpacks/format): needs sha1 +
//!   sha512 and a download URL on an allowed host per file, so only
//!   Modrinth-sourced files go in; CurseForge-only mods are listed, not added.
//! - CurseForge zip (`manifest.json`): project + file id per mod, so only
//!   CurseForge-sourced mods go in.
//!
//! `plan_export` is the pre-flight: exactly what goes in, what's left out and
//! why, and fixes. `write_*` turn a plan into the archive.

use crate::error::{AppError, AppResult};
use crate::model::{EntryKind, Pack};
use crate::resolve::{self, Catalog, Fix, Severity, Version};
use crate::sources::{curseforge, Loader, Source};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::io::Write;

/// Hosts the .mrpack spec allows for downloads.
pub const MRPACK_HOSTS: &[&str] = &["cdn.modrinth.com", "github.com", "raw.githubusercontent.com", "gitlab.com"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Format {
    Mrpack,
    CurseforgeZip,
    /// packwiz folder: each file from the site it's set to (mixed is fine).
    Packwiz,
    /// Mod lists for reading and sharing.
    Markdown,
    Csv,
}

impl Format {
    /// Formats that take every entry from the site it's set to, instead of one site only.
    pub fn mixed(self) -> bool {
        matches!(self, Format::Packwiz | Format::Markdown | Format::Csv)
    }
    pub fn is_list(self) -> bool {
        matches!(self, Format::Markdown | Format::Csv)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Env {
    pub client: String,
    pub server: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    pub key: String,
    pub name: String,
    pub source: Source,
    /// Path inside the instance, e.g. `mods/sodium-0.6.jar`.
    pub path: String,
    pub project_id: String,
    /// Plain id (no `cf:` prefix).
    pub file_id: String,
    pub version_number: String,
    pub url: String,
    pub sha1: Option<String>,
    pub sha512: Option<String>,
    pub size: u64,
    pub env: Option<Env>,
    pub distribution_blocked: bool,
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Excluded {
    pub key: String,
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    pub severity: Severity,
    pub message: String,
    /// Bulk fixes the pre-flight panel can apply in one click.
    pub fixes: Vec<Fix>,
    pub fix_label: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportPlan {
    pub format: Format,
    pub files: Vec<ExportFile>,
    pub excluded: Vec<Excluded>,
    pub problems: Vec<Problem>,
    /// Loader id as the format wants it (`neoforge-21.1.267` / `fabric-loader` key…).
    pub loader_label: String,
    /// False when something blocks writing a usable pack.
    pub can_export: bool,
    /// Files override templates put in `overrides/` (filled in by `add_overrides`).
    pub overrides: usize,
}

fn loader_name(l: Loader) -> &'static str {
    match l {
        Loader::Neoforge => "NeoForge",
        Loader::Fabric => "Fabric",
        Loader::Forge => "Forge",
        Loader::Quilt => "Quilt",
    }
}

/// Forge versions are stored as `1.21.1-52.1.21`; formats want `52.1.21`.
/// Where a file of this kind goes in the instance. Datapacks go to Paxi's
/// global folder when the pack has Paxi, else to `datapacks/` (copy them into
/// each world by hand).
pub fn folder_for(kind: EntryKind, has_paxi: bool) -> &'static str {
    match kind {
        EntryKind::Mod => "mods/",
        EntryKind::Resourcepack => "resourcepacks/",
        EntryKind::Shader => "shaderpacks/",
        EntryKind::Datapack if has_paxi => "config/paxi/datapacks/",
        EntryKind::Datapack => "datapacks/",
    }
}

pub fn plain_loader_version(pack: &Pack) -> String {
    let v = pack.loader_version.trim();
    match pack.loader {
        Loader::Forge => v.strip_prefix(&format!("{}-", pack.mc_version)).unwrap_or(v).to_string(),
        _ => v.to_string(),
    }
}

fn packwiz_loader_key(l: Loader) -> &'static str {
    match l {
        Loader::Neoforge => "neoforge",
        Loader::Forge => "forge",
        Loader::Fabric => "fabric",
        Loader::Quilt => "quilt",
    }
}

fn mrpack_loader_key(l: Loader) -> &'static str {
    match l {
        Loader::Neoforge => "neoforge",
        Loader::Forge => "forge",
        Loader::Fabric => "fabric-loader",
        Loader::Quilt => "quilt-loader",
    }
}

fn cf_loader_id(pack: &Pack) -> String {
    let prefix = match pack.loader {
        Loader::Neoforge => "neoforge",
        Loader::Forge => "forge",
        Loader::Fabric => "fabric",
        Loader::Quilt => "quilt",
    };
    format!("{prefix}-{}", plain_loader_version(pack))
}

/// Install sides for a file in the pack, learned the hard way in two Prism round-trips:
/// - Never "unsupported" on the client: singleplayer is an integrated server, so
///   server-side-only mods (YUNG's structures, Lithostitched…) must be installed too.
/// - Never "optional" either: launchers treat optional files as extras the
///   player opts into, and Prism leaves them unticked, so libraries went missing.
/// So every file is required on the client, and only mods that can't run on a
/// server are marked unsupported there (server exports skip them).
pub fn export_env(server_side: Option<&str>) -> Env {
    Env { client: "required".into(), server: if server_side == Some("unsupported") { "unsupported" } else { "required" }.into() }
}

fn host_ok(url: &str) -> bool {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| MRPACK_HOSTS.contains(&h))).unwrap_or(false) && url.starts_with("https://")
}

/// Builds the pre-flight plan. Picks files exactly like the checks do (pins,
/// channel policy), so what you export is what you've been looking at.
pub async fn plan_export(pack: &Pack, cat: &dyn Catalog, format: Format) -> ExportPlan {
    let mc = pack.mc_version.as_str();
    let loader = pack.loader;
    let want = match format {
        Format::CurseforgeZip => Source::Curseforge,
        _ => Source::Modrinth,
    };
    let mut files = Vec::new();
    let mut excluded = Vec::new();
    let mut problems = Vec::new();
    let mut switchable = Vec::new(); // linked on both sites but set to the other one
    let mut one_site = 0usize;

    let mut todo = Vec::new();
    for (k, e) in &pack.entries {
        let has_want = match want {
            Source::Modrinth => e.mod_ref.modrinth_id.is_some(),
            Source::Curseforge => e.mod_ref.curseforge_id.is_some(),
        };
        if !format.mixed() && e.mod_ref.preferred != want {
            if has_want {
                switchable.push(Fix::UseSource { key: k.clone(), source: want });
            } else {
                one_site += 1;
            }
            let other = if want == Source::Modrinth { "CurseForge" } else { "Modrinth" };
            excluded.push(Excluded {
                key: k.clone(),
                name: e.display.name.clone(),
                reason: if has_want { format!("Set to use {other}; switch it to export it here.") } else { format!("Only linked on {other}.") },
            });
            continue;
        }
        if let Some((pid, pin)) = resolve::target(e) {
            todo.push((k.clone(), pid, pin, e.kind));
        }
    }

    let picked: Vec<(String, String, AppResult<Option<Version>>)> = stream::iter(todo)
        .map(|(k, pid, pin, kind)| async move {
            let r = async {
                if let Some(vid) = &pin {
                    return cat.version(vid, false).await;
                }
                let vs = cat.versions_of(&pid, mc, loader, kind, false).await?;
                Ok(resolve::select_version_kind(&vs, mc, loader, kind, pack.channel_policy).map(|x| x.0))
            }
            .await;
            (k, pid, r)
        })
        .buffer_unordered(8)
        .collect()
        .await;

    let mut failed = 0;
    let has_paxi = resolve::has_project(pack, resolve::PAXI, &["paxi"]);
    for (k, pid, r) in picked {
        let e = &pack.entries[&k];
        let name = e.display.name.clone();
        let v = match r {
            Ok(Some(v)) => v,
            Ok(None) => {
                excluded.push(Excluded {
                    key: k,
                    name,
                    reason: if e.kind == EntryKind::Mod { format!("No file for Minecraft {mc} · {}.", loader_name(loader)) } else { format!("No {} file for Minecraft {mc}.", resolve::kind_label(e.kind)) },
                });
                continue;
            }
            Err(err) => {
                failed += 1;
                excluded.push(Excluded { key: k, name, reason: format!("Couldn't look up its file: {err}") });
                continue;
            }
        };
        let f = v.main_file().cloned().unwrap_or_default();
        let sha1 = f.hashes.get("sha1").cloned();
        let sha512 = f.hashes.get("sha512").cloned();
        let source = if format.mixed() { e.mod_ref.preferred } else { want };
        if format == Format::Mrpack {
            if sha1.is_none() || sha512.is_none() || !host_ok(&f.url) {
                excluded.push(Excluded { key: k, name, reason: "Modrinth didn't give a usable download URL and hashes for this file.".into() });
                continue;
            }
        }
        if format == Format::Packwiz && sha1.is_none() && sha512.is_none() {
            excluded.push(Excluded { key: k, name, reason: "The site gave no hash for this file, which packwiz needs.".into() });
            continue;
        }
        let env = Some(export_env(e.display.server_side.as_deref()));
        files.push(ExportFile {
            key: k,
            name,
            source,
            kind: e.kind,
            // CurseForge manifests only need ids, so a missing file name is fine there.
            path: format!(
                "{}{}",
                folder_for(e.kind, has_paxi),
                if f.filename.is_empty() { format!("{}.{}", v.version_number, if e.kind == EntryKind::Mod { "jar" } else { "zip" }) } else { f.filename.replace(['/', '\\'], "_") }
            ),
            project_id: pid.strip_prefix(curseforge::PREFIX).unwrap_or(&pid).to_string(),
            file_id: v.id.strip_prefix(curseforge::PREFIX).unwrap_or(&v.id).to_string(),
            version_number: v.version_number.clone(),
            url: f.url.clone(),
            sha1,
            sha512,
            size: f.size,
            env,
            distribution_blocked: v.distribution_blocked,
        });
    }
    files.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    excluded.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));

    // Two jars with the same name would overwrite each other in mods/.
    let mut seen = std::collections::HashMap::new();
    for f in &files {
        if let Some(prev) = seen.insert(f.path.clone(), f.name.clone()) {
            problems.push(Problem {
                severity: Severity::Error,
                message: format!("{prev} and {} both ship {}; one would overwrite the other.", f.name, f.path),
                fixes: vec![],
                fix_label: None,
            });
        }
    }

    let (here, there) = if want == Source::Modrinth { ("Modrinth", "CurseForge") } else { ("CurseForge", "Modrinth") };
    if pack.loader_version.trim().is_empty() && !format.is_list() {
        problems.push(Problem {
            severity: Severity::Error,
            message: format!("The pack has no {} version set. Launchers need it to install the loader.", loader_name(loader)),
            fixes: vec![],
            fix_label: None,
        });
    }
    if !switchable.is_empty() {
        problems.push(Problem {
            severity: Severity::Warning,
            message: format!("{} mod{} {} linked on {here} but set to use {there}.", switchable.len(), if switchable.len() == 1 { "" } else { "s" }, if switchable.len() == 1 { "is" } else { "are" }),
            fix_label: Some(format!("Switch {} to {here}", switchable.len())),
            fixes: switchable,
        });
    }
    if one_site > 0 {
        problems.push(Problem {
            severity: Severity::Warning,
            message: format!(
                "{one_site} mod{} only linked on {there} and won't be in this {}. Try \"Match sites\" to find {here} copies{}.",
                if one_site == 1 { " is" } else { "s are" },
                if format == Format::Mrpack { ".mrpack" } else { "CurseForge zip" },
                if format == Format::Mrpack { ", or export a CurseForge zip (or packwiz, which mixes sites) instead" } else { ", or export an .mrpack (or packwiz) instead" }
            ),
            fixes: vec![],
            fix_label: None,
        });
    }
    let blocked = files.iter().filter(|f| f.distribution_blocked).count();
    // A CurseForge manifest lists only ids, so the launcher picks the folder. Prism
    // (FlameCreationTask::validateOtherResources) moves datapacks to datapacks/,
    // which Paxi 5.1.3+ also loads ("Load from base 'datapacks' directory", on by default).
    let cf_datapacks = files.iter().filter(|f| format == Format::CurseforgeZip && f.kind == EntryKind::Datapack).count();
    if cf_datapacks > 0 {
        problems.push(Problem {
            severity: Severity::Info,
            message: format!(
                "{cf_datapacks} datapack{}: launchers choose where CurseForge files go. Prism puts datapacks in datapacks/, which Paxi (5.1.3 and later) also loads in every world; other launchers may differ.",
                if cf_datapacks == 1 { "" } else { "s" }
            ),
            fixes: vec![],
            fix_label: None,
        });
    }
    if blocked > 0 && !format.is_list() {
        problems.push(Problem {
            severity: Severity::Warning,
            message: format!("{blocked} mod{} block third-party downloads; Prism will ask you to download {} by hand.", if blocked == 1 { "" } else { "s" }, if blocked == 1 { "it" } else { "them" }),
            fixes: vec![],
            fix_label: None,
        });
    }
    if failed > 0 {
        problems.push(Problem {
            severity: Severity::Error,
            message: format!("{failed} mod{} couldn't be looked up (network or API key). Try again before exporting.", if failed == 1 { "" } else { "s" }),
            fixes: vec![],
            fix_label: None,
        });
    }
    if files.is_empty() {
        problems.push(Problem { severity: Severity::Error, message: "Nothing to export in this format.".into(), fixes: vec![], fix_label: None });
    }
    problems.sort_by(|a, b| a.severity.cmp(&b.severity));
    let can_export = !problems.iter().any(|p| p.severity == Severity::Error);
    let loader_label = match format {
        Format::CurseforgeZip => cf_loader_id(pack),
        Format::Packwiz => format!("{} {}", packwiz_loader_key(loader), plain_loader_version(pack)),
        _ => format!("{} {}", mrpack_loader_key(loader), plain_loader_version(pack)),
    };
    ExportPlan { format, files, excluded, problems, loader_label, can_export, overrides: 0 }
}

fn version_label(pack: &Pack) -> String {
    // A readable, increasing version from the last edit date.
    let d = pack.updated_at.get(0..10).unwrap_or("1.0.0").replace('-', ".");
    if d.is_empty() { "1.0.0".into() } else { d }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MrIndex<'a> {
    format_version: u32,
    game: &'a str,
    version_id: String,
    name: &'a str,
    files: Vec<MrFile<'a>>,
    dependencies: serde_json::Map<String, serde_json::Value>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MrFile<'a> {
    path: &'a str,
    hashes: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<&'a Env>,
    downloads: Vec<&'a str>,
    file_size: u64,
}

fn zip_opts() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn ensure_exportable(plan: &ExportPlan, want: Format) -> AppResult<()> {
    if plan.format != want {
        return Err(AppError::Invalid("Export plan is for a different format.".into()));
    }
    if !plan.can_export {
        return Err(AppError::Invalid("Fix the errors in the pre-flight check before exporting.".into()));
    }
    Ok(())
}

pub fn mrpack_index_json(pack: &Pack, plan: &ExportPlan) -> AppResult<String> {
    ensure_exportable(plan, Format::Mrpack)?;
    let mut deps = serde_json::Map::new();
    deps.insert("minecraft".into(), pack.mc_version.clone().into());
    deps.insert(mrpack_loader_key(pack.loader).into(), plain_loader_version(pack).into());
    let files = plan
        .files
        .iter()
        .map(|f| MrFile {
            path: &f.path,
            hashes: serde_json::json!({ "sha1": f.sha1, "sha512": f.sha512 }),
            env: f.env.as_ref(),
            downloads: vec![f.url.as_str()],
            file_size: f.size,
        })
        .collect();
    let idx = MrIndex { format_version: 1, game: "minecraft", version_id: version_label(pack), name: &pack.name, files, dependencies: deps };
    serde_json::to_string_pretty(&idx).map_err(|e| AppError::Invalid(e.to_string()))
}

pub fn cf_manifest_json(pack: &Pack, plan: &ExportPlan) -> AppResult<String> {
    ensure_exportable(plan, Format::CurseforgeZip)?;
    let files: Vec<serde_json::Value> = plan
        .files
        .iter()
        .map(|f| {
            serde_json::json!({
                "projectID": f.project_id.parse::<u64>().unwrap_or(0),
                "fileID": f.file_id.parse::<u64>().unwrap_or(0),
                "required": true
            })
        })
        .collect();
    let manifest = serde_json::json!({
        "minecraft": { "version": pack.mc_version, "modLoaders": [{ "id": cf_loader_id(pack), "primary": true }] },
        "manifestType": "minecraftModpack",
        "manifestVersion": 1,
        "name": pack.name,
        "version": version_label(pack),
        "author": "",
        "files": files,
        "overrides": "overrides"
    });
    serde_json::to_string_pretty(&manifest).map_err(|e| AppError::Invalid(e.to_string()))
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Adds the override-template checks to a plan: missing templates, ones
/// changed since the pack last reviewed them, and what goes in `overrides/`.
pub fn add_overrides(plan: &mut ExportPlan, pack: &Pack, lib: &crate::templates::Library, now: &str) {
    if pack.override_templates.is_empty() {
        return;
    }
    let warn = |message: String| Problem { severity: Severity::Warning, message, fixes: vec![], fix_label: None };
    match lib.overview(pack, now) {
        Ok(o) => {
            for _ in &o.missing {
                plan.problems.push(warn("An override template attached to this pack no longer exists. Detach it on the Overrides tab.".into()));
            }
            for u in &o.updates {
                plan.problems.push(warn(format!(
                    "Template {} changed since you reviewed it (v{} → v{}). The export uses its current files; review the changes on the Overrides tab.",
                    u.name, u.from_version, u.to_version
                )));
            }
            plan.overrides = o.merged.len();
            plan.problems.push(Problem {
                severity: Severity::Info,
                message: format!(
                    "{} file{} from {} template{} go into overrides/{}.",
                    o.merged.len(),
                    if o.merged.len() == 1 { "" } else { "s" },
                    o.attached.len(),
                    if o.attached.len() == 1 { "" } else { "s" },
                    if o.clashes > 0 { format!(" ({} clash{}: the template higher in the list wins)", o.clashes, if o.clashes == 1 { "" } else { "es" }) } else { String::new() }
                ),
                fixes: vec![],
                fix_label: None,
            });
        }
        Err(e) => {
            plan.problems.push(Problem { severity: Severity::Error, message: format!("Couldn't read the override templates: {e}"), fixes: vec![], fix_label: None });
            plan.can_export = false;
        }
    }
}

/// Writes the archive for `plan`, with `overrides` (path in `overrides/`,
/// file on disk) from the pack's override templates.
pub fn write_archive(pack: &Pack, plan: &ExportPlan, overrides: &[(String, std::path::PathBuf)], out: &std::path::Path) -> AppResult<()> {
    let io = |e: std::io::Error| AppError::Storage(format!("Couldn't write the file: {e}"));
    let zerr = |e: zip::result::ZipError| AppError::Storage(format!("Couldn't write the archive: {e}"));
    let tmp = out.with_extension("partial");
    let file = std::fs::File::create(&tmp).map_err(io)?;
    let mut z = zip::ZipWriter::new(file);
    match plan.format {
        Format::Mrpack => {
            let json = mrpack_index_json(pack, plan)?;
            z.start_file("modrinth.index.json", zip_opts()).map_err(zerr)?;
            z.write_all(json.as_bytes()).map_err(io)?;
        }
        Format::Packwiz | Format::Markdown | Format::Csv => {
            return Err(AppError::Invalid("That format isn't an archive.".into()));
        }
        Format::CurseforgeZip => {
            let json = cf_manifest_json(pack, plan)?;
            z.start_file("manifest.json", zip_opts()).map_err(zerr)?;
            z.write_all(json.as_bytes()).map_err(io)?;
            let mut html = String::from("<ul>\n");
            for f in &plan.files {
                html.push_str(&format!(
                    "<li><a href=\"https://www.curseforge.com/projects/{}\">{}</a></li>\n",
                    html_escape(&f.project_id),
                    html_escape(&f.name)
                ));
            }
            html.push_str("</ul>\n");
            z.start_file("modlist.html", zip_opts()).map_err(zerr)?;
            z.write_all(html.as_bytes()).map_err(io)?;
        }
    }
    z.add_directory("overrides/", zip_opts()).map_err(zerr)?;
    for (rel, src) in overrides {
        let rel = crate::templates::clean_rel(rel)?;
        z.start_file(format!("overrides/{rel}"), zip_opts()).map_err(zerr)?;
        let mut f = std::fs::File::open(src).map_err(io)?;
        std::io::copy(&mut f, &mut z).map_err(io)?;
    }
    z.finish().map_err(zerr)?;
    std::fs::rename(&tmp, out).map_err(io)?;
    Ok(())
}

// ---------------------------------------------------------------- packwiz (M11)
fn toml_str(s: &str) -> String {
    toml::Value::String(s.to_string()).to_string()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// A safe, unique metafile stem from a slug or name.
fn stem(raw: &str, used: &mut std::collections::HashSet<String>) -> String {
    let mut base: String = raw.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect();
    base = base.trim_matches('-').to_string();
    if base.is_empty() {
        base = "file".into();
    }
    let mut s = base.clone();
    let mut n = 2;
    while !used.insert(s.clone()) {
        s = format!("{base}-{n}");
        n += 1;
    }
    s
}

/// The files of a packwiz pack, as (path, contents): `pack.toml`, `index.toml`,
/// one `.pw.toml` per file (in the folder the file installs to), and the
/// override templates' files at the pack root.
pub fn packwiz_files(pack: &Pack, plan: &ExportPlan, overrides: &[(String, std::path::PathBuf)]) -> AppResult<Vec<(String, Vec<u8>)>> {
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut used = std::collections::HashSet::new();
    let mut index: Vec<(String, String, bool)> = Vec::new();
    for f in &plan.files {
        let e = pack.entries.get(&f.key);
        let slug = e.map(|e| if e.display.slug.is_empty() { e.display.name.clone() } else { e.display.slug.clone() }).unwrap_or_else(|| f.name.clone());
        let folder = f.path.rsplit_once('/').map(|(d, _)| d).unwrap_or("mods");
        let filename = f.path.rsplit('/').next().unwrap_or(&f.path);
        let side = f.env.as_ref().map(|e| if e.server == "unsupported" { "client" } else { "both" }).unwrap_or("both");
        let mut t = format!("name = {}\nfilename = {}\nside = {}\n\n[download]\n", toml_str(&f.name), toml_str(filename), toml_str(side));
        match f.source {
            Source::Modrinth => {
                let (fmt, h) = match (&f.sha512, &f.sha1) {
                    (Some(h), _) => ("sha512", h),
                    (None, Some(h)) => ("sha1", h),
                    _ => continue,
                };
                t.push_str(&format!("url = {}\nhash-format = \"{fmt}\"\nhash = {}\n\n[update]\n[update.modrinth]\nmod-id = {}\nversion = {}\n", toml_str(&f.url), toml_str(h), toml_str(&f.project_id), toml_str(&f.file_id)));
            }
            Source::Curseforge => {
                let Some(h) = &f.sha1 else { continue };
                t.push_str(&format!(
                    "hash-format = \"sha1\"\nhash = {}\nmode = \"metadata:curseforge\"\n\n[update]\n[update.curseforge]\nfile-id = {}\nproject-id = {}\n",
                    toml_str(h),
                    f.file_id.parse::<u64>().unwrap_or(0),
                    f.project_id.parse::<u64>().unwrap_or(0)
                ));
            }
        }
        let path = format!("{folder}/{}.pw.toml", stem(&slug, &mut used));
        index.push((path.clone(), sha256_hex(t.as_bytes()), true));
        out.push((path, t.into_bytes()));
    }
    for (rel, src) in overrides {
        let rel = crate::templates::clean_rel(rel)?;
        if rel == "pack.toml" || rel == "index.toml" {
            continue; // packwiz's own files win
        }
        let bytes = std::fs::read(src).map_err(|e| AppError::Storage(format!("Couldn't read {rel}: {e}")))?;
        index.push((rel.clone(), sha256_hex(&bytes), false));
        out.push((rel, bytes));
    }
    index.sort_by(|a, b| a.0.cmp(&b.0));
    let mut idx = String::from("hash-format = \"sha256\"\n");
    for (file, hash, meta) in &index {
        idx.push_str(&format!("\n[[files]]\nfile = {}\nhash = {}\n{}", toml_str(file), toml_str(hash), if *meta { "metafile = true\n" } else { "" }));
    }
    let lv = plain_loader_version(pack);
    let packtoml = format!(
        "name = {}\nauthor = \"\"\nversion = \"1.0.0\"\npack-format = \"packwiz:1.1.0\"\n\n[index]\nfile = \"index.toml\"\nhash-format = \"sha256\"\nhash = {}\n\n[versions]\nminecraft = {}\n{} = {}\n",
        toml_str(&pack.name),
        toml_str(&sha256_hex(idx.as_bytes())),
        toml_str(&pack.mc_version),
        packwiz_loader_key(pack.loader),
        toml_str(&lv)
    );
    out.push(("index.toml".into(), idx.into_bytes()));
    out.push(("pack.toml".into(), packtoml.into_bytes()));
    Ok(out)
}

/// Writes a packwiz pack into `dir`, which must not exist yet (or be empty).
pub fn write_packwiz(pack: &Pack, plan: &ExportPlan, overrides: &[(String, std::path::PathBuf)], dir: &std::path::Path) -> AppResult<()> {
    let io = |e: std::io::Error| AppError::Storage(format!("Couldn't write the folder: {e}"));
    if dir.exists() && std::fs::read_dir(dir).map_err(io)?.next().is_some() {
        return Err(AppError::Invalid(format!("{} isn't empty; pick an empty or new folder.", dir.display())));
    }
    for (rel, bytes) in packwiz_files(pack, plan, overrides)? {
        let rel = crate::templates::clean_rel(&rel)?;
        let p = dir.join(&rel);
        std::fs::create_dir_all(p.parent().unwrap()).map_err(io)?;
        std::fs::write(p, bytes).map_err(io)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- mod lists (M11)
fn page_url(f: &ExportFile, slug: &str) -> String {
    let id = if slug.is_empty() { f.project_id.as_str() } else { slug };
    match f.source {
        Source::Modrinth => format!(
            "https://modrinth.com/{}/{id}",
            match f.kind {
                EntryKind::Mod => "mod",
                EntryKind::Resourcepack => "resourcepack",
                EntryKind::Shader => "shader",
                EntryKind::Datapack => "datapack",
            }
        ),
        Source::Curseforge => format!(
            "https://www.curseforge.com/minecraft/{}/{id}",
            match f.kind {
                EntryKind::Mod => "mc-mods",
                EntryKind::Resourcepack => "texture-packs",
                EntryKind::Shader => "shaders",
                EntryKind::Datapack => "data-packs",
            }
        ),
    }
}

struct Row {
    name: String,
    kind: &'static str,
    source: &'static str,
    version: String,
    link: String,
    category: String,
    reason: &'static str,
}

fn rows(pack: &Pack, plan: &ExportPlan) -> Vec<Row> {
    plan.files
        .iter()
        .map(|f| {
            let e = pack.entries.get(&f.key);
            Row {
                name: f.name.clone(),
                kind: resolve::kind_label(f.kind),
                source: if f.source == Source::Modrinth { "Modrinth" } else { "CurseForge" },
                version: f.version_number.clone(),
                link: page_url(f, e.map(|e| e.display.slug.as_str()).unwrap_or("")),
                category: e.and_then(|e| e.display.categories.iter().find(|c| *c != "library").cloned()).unwrap_or_default(),
                reason: match e.map(|e| e.reason) {
                    Some(crate::model::Reason::Dependency) => "dependency",
                    Some(crate::model::Reason::Payload) => "payload",
                    _ => "added",
                },
            }
        })
        .collect()
}

/// A Markdown mod list: a table per content kind, your picks before dependencies.
pub fn list_markdown(pack: &Pack, plan: &ExportPlan) -> String {
    let esc = |s: &str| s.replace('|', "\\|").replace('\n', " ");
    let mut out = format!("# {}\n\nMinecraft {} · {} {}\n", esc(&pack.name), pack.mc_version, loader_name(pack.loader), plain_loader_version(pack));
    let mut rows = rows(pack, plan);
    rows.sort_by(|a, b| (a.reason == "dependency").cmp(&(b.reason == "dependency")).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    for (kind, title) in [("mod", "Mods"), ("resource pack", "Resource packs"), ("shader", "Shaders"), ("datapack", "Datapacks")] {
        let rs: Vec<&Row> = rows.iter().filter(|r| r.kind == kind).collect();
        if rs.is_empty() {
            continue;
        }
        out.push_str(&format!("\n## {title} ({})\n\n| Name | Version | Source | Category | |\n|---|---|---|---|---|\n", rs.len()));
        for r in rs {
            out.push_str(&format!(
                "| [{}]({}) | {} | {} | {} | {} |\n",
                esc(&r.name),
                r.link,
                esc(&r.version),
                r.source,
                esc(&r.category),
                if r.reason == "dependency" { "dependency" } else { "" }
            ));
        }
    }
    if !plan.excluded.is_empty() {
        out.push_str(&format!("\n## Not included ({})\n\n", plan.excluded.len()));
        for x in &plan.excluded {
            out.push_str(&format!("- {}: {}\n", esc(&x.name), esc(&x.reason)));
        }
    }
    out
}

/// CSV: name, type, version, source, link, category, why it's in the pack.
pub fn list_csv(pack: &Pack, plan: &ExportPlan) -> String {
    let q = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut out = String::from("name,type,version,source,link,category,reason\n");
    let mut rows = rows(pack, plan);
    rows.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    for r in rows {
        out.push_str(&[q(&r.name), q(r.kind), q(&r.version), q(r.source), q(&r.link), q(&r.category), q(r.reason)].join(","));
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;
    use crate::resolve::tests::{add, block, key_of, pack_for, Fixture};

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("mpp-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&d).unwrap();
        d.join(name)
    }

    fn fabric_pack(f: &Fixture) -> Pack {
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        pack.loader_version = "0.20.1".into();
        let mut keys = crate::resolve::tests::counter();
        block(add(f, &mut pack, "sodium-extra", &mut keys));
        block(add(f, &mut pack, "modmenu", &mut keys));
        pack
    }

    #[test]
    fn other_content_goes_to_its_own_folder() {
        use crate::resolve::{plan_add_kind, tests::{apply, counter, KindCat}};
        let c = KindCat::new();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        pack.loader_version = "0.20.1".into();
        let mut keys = counter();
        for (pid, kind) in [("rpFresh", EntryKind::Resourcepack), ("shComp", EntryKind::Shader), ("dpTerra", EntryKind::Datapack)] {
            let plan = block(plan_add_kind(&pack, pid, kind, None, false, None, None, &c, false, "t", &mut keys)).unwrap();
            apply(&mut pack, plan);
        }
        let plan = block(plan_export(&pack, &c, Format::Mrpack));
        let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
        for want in ["resourcepacks/rpNew.zip", "shaderpacks/shExact.zip", "config/paxi/datapacks/dpZip.zip", "mods/paxiF.zip"] {
            assert!(paths.contains(&want), "{want} not in {paths:?}");
        }
        // CurseForge zips keep datapacks (the launcher places them); only Modrinth-linked ones drop out here.
        let cf = block(plan_export(&pack, &c, Format::CurseforgeZip));
        assert!(cf.excluded.iter().any(|x| x.name == "Terralith" && x.reason.contains("Only linked on Modrinth")), "{:?}", cf.excluded);
    }

    #[test]
    fn packwiz_and_lists() {
        use crate::resolve::{plan_add_kind, tests::{apply, counter, KindCat}};
        let c = KindCat::new();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        pack.loader_version = "0.20.1".into();
        let mut keys = counter();
        for (pid, kind) in [("rpFresh", EntryKind::Resourcepack), ("dpTerra", EntryKind::Datapack)] {
            let plan = block(plan_add_kind(&pack, pid, kind, None, false, None, None, &c, false, "t", &mut keys)).unwrap();
            apply(&mut pack, plan);
        }
        let plan = block(plan_export(&pack, &c, Format::Packwiz));
        assert!(plan.can_export, "{:?}", plan.problems);
        let cfg = tmp("cfg.json");
        std::fs::write(&cfg, "{}").unwrap();
        let files: std::collections::HashMap<String, Vec<u8>> = packwiz_files(&pack, &plan, &[("config/x.json".into(), cfg)]).unwrap().into_iter().collect();
        let text = |p: &str| String::from_utf8(files[p].clone()).unwrap();
        let rp = text("resourcepacks/fresh-animations.pw.toml").parse::<toml::Table>().unwrap();
        assert_eq!(rp["filename"].as_str(), Some("rpNew.zip"));
        assert_eq!(rp["update"]["modrinth"]["mod-id"].as_str(), Some("rpFresh"));
        assert!(files.contains_key("config/paxi/datapacks/terralith.pw.toml"));
        assert!(files.contains_key("mods/paxi.pw.toml"));
        // pack.toml → index.toml → each file: the hashes chain.
        let packt = text("pack.toml").parse::<toml::Table>().unwrap();
        assert_eq!(packt["index"]["hash"].as_str().unwrap(), sha256_hex(&files["index.toml"]));
        assert_eq!(packt["versions"]["fabric"].as_str(), Some("0.20.1"));
        let idx = text("index.toml").parse::<toml::Table>().unwrap();
        for f in idx["files"].as_array().unwrap() {
            let p = f["file"].as_str().unwrap();
            assert_eq!(f["hash"].as_str().unwrap(), sha256_hex(&files[p]), "{p}");
            assert_eq!(f.get("metafile").and_then(|m| m.as_bool()).unwrap_or(false), p.ends_with(".pw.toml"));
        }
        let md = list_markdown(&pack, &block(plan_export(&pack, &c, Format::Markdown)));
        assert!(md.contains("## Resource packs (1)") && md.contains("[Fresh Animations](https://modrinth.com/resourcepack/fresh animations)"), "{md}");
        let csv = list_csv(&pack, &block(plan_export(&pack, &c, Format::Csv)));
        assert!(csv.starts_with("name,type,version,source,link,category,reason\n") && csv.contains("Paxi,mod,paxiF,Modrinth"), "{csv}");
    }

    #[test]
    fn override_files_land_in_overrides() {
        let f = Fixture::load();
        let pack = fabric_pack(&f);
        let plan = block(plan_export(&pack, &f, Format::Mrpack));
        let out = tmp("o.mrpack");
        let src = out.with_file_name("a.json");
        std::fs::write(&src, "{\"x\":1}").unwrap();
        write_archive(&pack, &plan, &[("config/a.json".into(), src)], &out).unwrap();
        let mut z = zip::ZipArchive::new(std::fs::File::open(&out).unwrap()).unwrap();
        let mut s = String::new();
        std::io::Read::read_to_string(&mut z.by_name("overrides/config/a.json").unwrap(), &mut s).unwrap();
        assert_eq!(s, "{\"x\":1}");
        assert!(write_archive(&pack, &plan, &[("../evil".into(), out.clone())], &out.with_file_name("b.mrpack")).is_err());
    }

    #[test]
    fn mrpack_plan_index_and_roundtrip() {
        let f = Fixture::load();
        let pack = fabric_pack(&f);
        let plan = block(plan_export(&pack, &f, Format::Mrpack));
        assert!(plan.can_export, "{:?}", plan.problems);
        assert_eq!(plan.files.len(), pack.entries.len(), "excluded: {:?}", plan.excluded);
        assert!(plan.files.iter().all(|x| x.sha1.is_some() && x.sha512.is_some() && x.url.starts_with("https://cdn.modrinth.com/")));
        let json: serde_json::Value = serde_json::from_str(&mrpack_index_json(&pack, &plan).unwrap()).unwrap();
        assert_eq!(json["formatVersion"], 1);
        assert_eq!(json["dependencies"]["minecraft"], "1.21.1");
        assert_eq!(json["dependencies"]["fabric-loader"], "0.20.1");
        let sodium = json["files"].as_array().unwrap().iter().find(|x| x["path"].as_str().unwrap().contains("sodium-fabric")).expect("sodium file");
        assert_eq!(sodium["env"]["server"], "unsupported", "Sodium is client-only");
        assert!(json["files"].as_array().unwrap().iter().all(|x| x["env"]["client"] == "required"), "every file installs on clients");
        assert!(json["files"].as_array().unwrap().iter().all(|x| x["env"]["server"] != "optional"), "no opt-in files");

        // Write, then import it back: same projects, pinned to the same files.
        let out = tmp("pack.mrpack");
        write_archive(&pack, &plan, &[], &out).unwrap();
        let back = block(import::import_mrpack(&std::fs::read(&out).unwrap(), &f)).unwrap();
        assert!(back.unknown.is_empty(), "{:?}", back.unknown);
        assert_eq!((back.pack.mc_version.as_str(), back.pack.loader, back.pack.loader_version.as_str()), ("1.21.1", Loader::Fabric, "0.20.1"));
        let mut a: Vec<(Option<String>, Option<String>)> = pack.entries.values().map(|e| (e.mod_ref.modrinth_id.clone(), None)).collect();
        let mut b: Vec<(Option<String>, Option<String>)> = back.pack.entries.values().map(|e| (e.mod_ref.modrinth_id.clone(), None)).collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
        let files: HashSet<String> = plan.files.iter().map(|x| x.file_id.clone()).collect();
        assert!(back.pack.entries.values().all(|e| e.file_id.as_ref().is_some_and(|id| files.contains(id))), "imports are pinned to the exported files");
        // Fabric API is required by Mod Menu and tagged library → becomes a dependency again.
        let fapi = back.pack.entries.values().find(|e| e.display.name == "Fabric API").unwrap();
        assert_eq!(fapi.reason, Reason::Dependency);
    }
    use crate::model::Reason;
    use std::collections::HashSet;

    #[test]
    fn formats_exclude_the_other_site_with_fixes() {
        let f = Fixture::load();
        let mut pack = fabric_pack(&f);
        let sk = key_of(&pack, &f, "sodium");
        // Sodium linked on both sites but set to CurseForge; plus a CurseForge-only mod.
        let e = pack.entries.get_mut(&sk).unwrap();
        e.mod_ref.curseforge_id = Some(394468);
        e.mod_ref.preferred = Source::Curseforge;
        // Cloth Config's real recorded CurseForge files.
        let mut cf_only = crate::model::tests::entry(None, Some(348521), "Cloth Config API", "9");
        cf_only.mod_ref.preferred = Source::Curseforge;
        pack.entries.insert("cfonly".into(), cf_only);
        let plan = block(plan_export(&pack, &f, Format::Mrpack));
        assert!(plan.excluded.iter().any(|x| x.key == sk));
        assert!(plan.excluded.iter().any(|x| x.key == "cfonly"));
        let switch = plan.problems.iter().find(|p| !p.fixes.is_empty()).expect("bulk switch fix");
        assert_eq!(switch.fixes, vec![Fix::UseSource { key: sk.clone(), source: Source::Modrinth }]);
        assert!(plan.can_export, "warnings don't block");

        // The CurseForge zip takes exactly the CurseForge side.
        let cf = block(plan_export(&pack, &f, Format::CurseforgeZip));
        let keys: HashSet<&str> = cf.files.iter().map(|x| x.key.as_str()).collect();
        assert!(keys.contains("cfonly"), "{:?} / {:?}", cf.files, cf.excluded);
        let m: serde_json::Value = serde_json::from_str(&cf_manifest_json(&pack, &cf).unwrap()).unwrap();
        assert_eq!(m["minecraft"]["modLoaders"][0]["id"], "fabric-0.20.1");
        let cloth = cf.files.iter().find(|x| x.key == "cfonly").unwrap();
        assert!(m["files"].as_array().unwrap().iter().any(|x| x["projectID"] == 348521 && x["fileID"].to_string() == cloth.file_id));
        assert_eq!(m["author"], "", "no personal details in exports");
    }

    #[test]
    fn env_is_never_optional_or_client_unsupported() {
        // Round-trip 1: client "unsupported" was skipped by Prism. Round-trip 2:
        // "optional" files were left unticked in Prism's import dialog.
        for (server, want) in [(Some("unsupported"), "unsupported"), (Some("optional"), "required"), (Some("required"), "required"), (None, "required")] {
            let e = export_env(server);
            assert_eq!(e.client, "required");
            assert_eq!(e.server, want, "{server:?}");
        }
    }

    #[test]
    fn server_only_mods_still_install_on_clients() {
        // Found in the Prism round-trip: YUNG's structures, Lithostitched etc. are
        // client "unsupported" on Modrinth; exported as-is, Prism skipped them and
        // singleplayer worlds (an integrated server) crashed on missing deps.
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        pack.loader_version = "0.20.1".into();
        let mut keys = crate::resolve::tests::counter();
        block(add(&f, &mut pack, "old-mod", &mut keys)); // client unsupported, server required
        let plan = block(plan_export(&pack, &f, Format::Mrpack));
        // Synthetic files have no hashes, so check the env logic on the plan's files where present,
        // and directly otherwise.
        for x in &plan.files {
            assert_ne!(x.env.as_ref().unwrap().client, "unsupported");
        }
        let e = pack.entries.values().next().unwrap();
        assert_eq!(e.display.client_side.as_deref(), Some("unsupported"));
    }

    #[test]
    fn missing_loader_version_blocks_export() {
        let f = Fixture::load();
        let mut pack = fabric_pack(&f);
        pack.loader_version = " ".into();
        let plan = block(plan_export(&pack, &f, Format::Mrpack));
        assert!(!plan.can_export);
        assert!(mrpack_index_json(&pack, &plan).is_err());
    }

    #[test]
    fn forge_versions_are_plain_in_exports() {
        let mut pack = pack_for("1.21.1", Loader::Forge);
        pack.loader_version = "1.21.1-52.1.21".into();
        assert_eq!(plain_loader_version(&pack), "52.1.21");
        assert_eq!(cf_loader_id(&pack), "forge-52.1.21");
    }

    #[test]
    fn prism_instance_import_reads_index_and_hashes() {
        let f = Fixture::load();
        let dir = tmp("Inst").with_file_name("Inst");
        let mods = dir.join("minecraft/mods");
        std::fs::create_dir_all(mods.join(".index")).unwrap();
        std::fs::write(dir.join("instance.cfg"), "[General]\nname=My Test Instance\n").unwrap();
        std::fs::write(dir.join("mmc-pack.json"), r#"{"components":[{"uid":"net.minecraft","version":"1.21.1"},{"uid":"net.fabricmc.fabric-loader","version":"0.20.1"}],"formatVersion":1}"#).unwrap();
        let sodium = crate::resolve::select_version(
            &f.versions.get(&format!("{}|1.21.1", f.id("sodium"))).unwrap().clone(),
            "1.21.1",
            Loader::Fabric,
            crate::model::ChannelPolicy::Stable,
        )
        .unwrap();
        std::fs::write(
            mods.join(".index/sodium.pw.toml"),
            format!("name = \"Sodium\"\nfilename = \"sodium.jar\"\nside = \"client\"\n\n[update.modrinth]\nmod-id = \"{}\"\nversion = \"{}\"\n", sodium.project_id, sodium.id),
        )
        .unwrap();
        std::fs::write(mods.join("sodium.jar"), b"pretend jar").unwrap();
        std::fs::write(mods.join("mystery.jar"), b"unknown bytes").unwrap();
        std::fs::write(mods.join("old.jar.disabled"), b"x").unwrap();
        let before: Vec<_> = walk(&dir);
        let r = block(import::import_prism_instance(&dir, &f)).unwrap();
        assert_eq!(r.pack.name, "My Test Instance");
        assert_eq!((r.pack.mc_version.as_str(), r.pack.loader), ("1.21.1", Loader::Fabric));
        assert_eq!(r.pack.entries.len(), 1);
        assert_eq!(r.pack.entries.values().next().unwrap().file_id.as_deref(), Some(sodium.id.as_str()));
        assert_eq!(r.unknown, vec!["mystery.jar"]);
        assert!(r.notes.iter().any(|n| n.contains("disabled")));
        assert_eq!(before, walk(&dir), "import must never change the instance");
    }

    fn walk(d: &std::path::Path) -> Vec<(String, u64)> {
        let mut out = Vec::new();
        for e in std::fs::read_dir(d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk(&p));
            } else {
                out.push((p.display().to_string(), std::fs::metadata(&p).unwrap().len()));
            }
        }
        out.sort();
        out
    }
}
