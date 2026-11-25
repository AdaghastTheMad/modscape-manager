//! Override templates (M9): reusable `config/`, `kubejs/`… folders kept as
//! plain folders in the app data dir and copied into a pack's `overrides/`
//! on export.
//!
//! Layout: `<root>/<id>/template.json` (metadata) + `<root>/<id>/files/…`
//! (the files exactly as they land in `overrides/`). The files can be edited
//! with any tool; every scan compares them to the hashes in `template.json`
//! and bumps the version when anything changed, so packs see "updated".

use crate::error::{AppError, AppResult};
use crate::model::{AppliedRef, Pack};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Text files at most this big open in the in-app editor.
pub const EDIT_LIMIT: u64 = 256 * 1024;
/// Safety caps for imports.
const MAX_FILES: usize = 20_000;
const MAX_BYTES: u64 = 512 * 1024 * 1024;

const TEXT_EXT: &[&str] = &[
    "toml", "json", "json5", "jsonc", "js", "ts", "cfg", "conf", "properties", "txt", "md", "yml", "yaml", "ini", "snbt", "zs", "mcmeta", "mcfunction", "csv", "xml", "lang",
];
/// Top-level folders an instance has that never belong in a template.
const NEVER: &[&str] = &[
    "mods", "saves", "logs", "crash-reports", "screenshots", "libraries", "natives", ".fabric", ".cache", ".index", "versions", "assets", "backups", "local", ".mixin.out",
    "downloads", "debug", "command_history.txt", "usercache.json", "usernamecache.json",
];
/// Ticked by default when importing.
const SUGGESTED: &[&str] = &["config", "defaultconfigs", "kubejs", "scripts", "options.txt", "global_packs", "openloader", "paxi", "fancymenu", "packmenu"];
/// Loader/vanilla config names that are never "for a mod".
const NOT_A_MOD: &[&str] = &["fml", "neoforge", "forge", "fabric", "fabricloader", "quilt", "quiltloader", "minecraft", "options", "servers", "realms", "jvmargs"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateMeta {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub version: u32,
    /// path → sha1 as of the last scan.
    #[serde(default)]
    pub files: BTreeMap<String, String>,
    /// path → "size:modified-ns" when it was hashed, so unchanged files
    /// aren't re-read on every scan (a cache; safe to delete).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stamps: BTreeMap<String, String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub size: u64,
    /// Small text file: opens in the in-app editor.
    pub editable: bool,
    /// Mod id the path points at (`config/jei/…` → `jei`), if obvious.
    pub mod_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateView {
    pub id: String,
    pub name: String,
    pub description: String,
    pub version: u32,
    pub created_at: String,
    pub updated_at: String,
    pub files: Vec<FileInfo>,
    /// Top-level folders/files, e.g. `config/`, `options.txt`.
    pub folders: Vec<String>,
    pub bytes: u64,
    /// Absolute path of the files folder (shown so you can find it).
    pub folder: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileText {
    pub path: String,
    pub size: u64,
    /// None when the file is too big or not text: open it in your own editor.
    pub text: Option<String>,
}

pub struct Library {
    root: PathBuf,
}

fn io(e: std::io::Error) -> AppError {
    AppError::Storage(e.to_string())
}

/// A relative path with only normal components, `/`-separated. Rejects
/// absolute paths, `..`, and the metadata file's name at the top.
pub fn clean_rel(p: &str) -> AppResult<String> {
    let p = p.trim().replace('\\', "/");
    let parts: Vec<&str> = p.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    let bad = || AppError::Invalid(format!("\"{p}\" isn't a valid path inside a template."));
    if parts.is_empty() || p.starts_with('/') {
        return Err(bad());
    }
    for s in &parts {
        if *s == ".." || s.contains(':') || s.chars().any(|c| c.is_control()) {
            return Err(bad());
        }
    }
    let joined = parts.join("/");
    if Path::new(&joined).components().any(|c| !matches!(c, Component::Normal(_))) {
        return Err(bad());
    }
    Ok(joined)
}

pub fn is_text_path(p: &str) -> bool {
    let name = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
    match name.rsplit_once('.') {
        Some((_, ext)) => TEXT_EXT.contains(&ext),
        None => false,
    }
}

/// `config/jei/jei-client.toml` → `jei`; `config/sodium-extra-options.json`
/// → `sodium-extra-options`; `kubejs/…` → `kubejs`. None for loader configs
/// and anything not obviously one mod's.
pub fn mod_hint(path: &str) -> Option<String> {
    let mut parts = path.split('/');
    let top = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    if top == "kubejs" {
        return Some("kubejs".into());
    }
    if top != "config" && top != "defaultconfigs" || rest.is_empty() {
        return None;
    }
    let first = rest[0];
    let mut name = if rest.len() > 1 { first.to_string() } else { first.split('.').next().unwrap_or(first).to_string() };
    name = name.to_ascii_lowercase();
    for suf in ["-client", "-common", "-server", "_client", "_common", "_server", "-startup", "-config", "_config"] {
        if let Some(s) = name.strip_suffix(suf) {
            name = s.to_string();
        }
    }
    let norm = norm(&name);
    if norm.len() < 2 || NOT_A_MOD.contains(&norm.as_str()) || name.starts_with("neoforge") || name.starts_with("forge-") {
        return None;
    }
    Some(name)
}

pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase()
}

fn sha1_file(p: &Path) -> AppResult<String> {
    let mut f = std::fs::File::open(p).map_err(io)?;
    let mut h = Sha1::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(io)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Every file under `dir`, as `/`-separated relative paths (symlinks skipped).
fn walk(dir: &Path) -> AppResult<Vec<(String, PathBuf, u64)>> {
    let mut out = vec![];
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let rd = match std::fs::read_dir(&d) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(io(e)),
        };
        for ent in rd {
            let ent = ent.map_err(io)?;
            let ft = ent.file_type().map_err(io)?;
            let p = ent.path();
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file() {
                let rel = p.strip_prefix(dir).unwrap().components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect::<Vec<_>>().join("/");
                if rel == ".DS_Store" || rel.ends_with("/.DS_Store") {
                    continue;
                }
                let size = ent.metadata().map_err(io)?.len();
                out.push((rel, p, size));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

impl Library {
    pub fn new(root: PathBuf) -> Self {
        Library { root }
    }
    fn dir(&self, id: &str) -> AppResult<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
            return Err(AppError::Invalid("Bad template id.".into()));
        }
        Ok(self.root.join(id))
    }
    pub fn files_dir(&self, id: &str) -> AppResult<PathBuf> {
        Ok(self.dir(id)?.join("files"))
    }
    fn file_path(&self, id: &str, rel: &str) -> AppResult<PathBuf> {
        Ok(self.files_dir(id)?.join(clean_rel(rel)?))
    }
    fn read_meta(&self, id: &str) -> AppResult<TemplateMeta> {
        let p = self.dir(id)?.join("template.json");
        let s = std::fs::read_to_string(&p).map_err(|_| AppError::Invalid("That template no longer exists.".into()))?;
        serde_json::from_str(&s).map_err(|e| AppError::Storage(format!("template.json is damaged: {e}")))
    }
    fn write_meta(&self, m: &TemplateMeta) -> AppResult<()> {
        let d = self.dir(&m.id)?;
        std::fs::create_dir_all(d.join("files")).map_err(io)?;
        let tmp = d.join("template.json.partial");
        std::fs::write(&tmp, serde_json::to_string_pretty(m).unwrap()).map_err(io)?;
        std::fs::rename(&tmp, d.join("template.json")).map_err(io)
    }

    /// Re-hashes the files; bumps the version if anything changed.
    pub fn scan(&self, id: &str, now: &str) -> AppResult<TemplateView> {
        let mut m = self.read_meta(id)?;
        let dir = self.files_dir(id)?;
        let files = walk(&dir)?;
        let mut hashes = BTreeMap::new();
        let mut stamps = BTreeMap::new();
        for (rel, p, size) in &files {
            let modified = std::fs::metadata(p).ok().and_then(|md| md.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
            let stamp = format!("{size}:{modified}");
            let h = match (m.stamps.get(rel), m.files.get(rel)) {
                (Some(s), Some(h)) if *s == stamp && modified != 0 => h.clone(),
                _ => sha1_file(p)?,
            };
            hashes.insert(rel.clone(), h);
            stamps.insert(rel.clone(), stamp);
        }
        if hashes != m.files {
            m.files = hashes;
            m.stamps = stamps;
            m.version += 1;
            m.updated_at = now.to_string();
            self.write_meta(&m)?;
        } else if stamps != m.stamps {
            m.stamps = stamps; // same contents, new timestamps: just refresh the cache
            self.write_meta(&m)?;
        }
        let mut folders = BTreeSet::new();
        let infos = files
            .iter()
            .map(|(rel, _, size)| {
                folders.insert(match rel.split_once('/') {
                    Some((top, _)) => format!("{top}/"),
                    None => rel.clone(),
                });
                FileInfo { path: rel.clone(), size: *size, editable: is_text_path(rel) && *size <= EDIT_LIMIT, mod_hint: mod_hint(rel) }
            })
            .collect();
        Ok(TemplateView {
            id: m.id,
            name: m.name,
            description: m.description,
            version: m.version,
            created_at: m.created_at,
            updated_at: m.updated_at,
            bytes: files.iter().map(|f| f.2).sum(),
            files: infos,
            folders: folders.into_iter().collect(),
            folder: dir.display().to_string(),
        })
    }

    pub fn list(&self, now: &str) -> AppResult<Vec<TemplateView>> {
        let mut out = vec![];
        let rd = match std::fs::read_dir(&self.root) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(io(e)),
        };
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !ent.path().join("template.json").is_file() {
                continue;
            }
            if let Ok(v) = self.scan(&name, now) {
                out.push(v);
            }
        }
        out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(out)
    }

    pub fn create(&self, name: &str, description: &str, now: &str) -> AppResult<TemplateView> {
        let name = name.trim();
        if name.is_empty() {
            return Err(AppError::Invalid("Give the template a name.".into()));
        }
        let id = uuid::Uuid::new_v4().to_string();
        self.write_meta(&TemplateMeta {
            id: id.clone(),
            name: name.into(),
            description: description.trim().into(),
            version: 1,
            files: BTreeMap::new(),
            stamps: BTreeMap::new(),
            created_at: now.into(),
            updated_at: now.into(),
        })?;
        self.scan(&id, now)
    }

    /// Name and description only; never a new version.
    pub fn set_meta(&self, id: &str, name: &str, description: &str, now: &str) -> AppResult<TemplateView> {
        let mut m = self.read_meta(id)?;
        if name.trim().is_empty() {
            return Err(AppError::Invalid("Give the template a name.".into()));
        }
        m.name = name.trim().into();
        m.description = description.trim().into();
        self.write_meta(&m)?;
        self.scan(id, now)
    }

    pub fn duplicate(&self, id: &str, now: &str) -> AppResult<TemplateView> {
        let m = self.read_meta(id)?;
        let copy = self.create(&format!("{} (copy)", m.name), &m.description, now)?;
        let to = self.files_dir(&copy.id)?;
        for (rel, p, _) in walk(&self.files_dir(id)?)? {
            let dest = to.join(&rel);
            std::fs::create_dir_all(dest.parent().unwrap()).map_err(io)?;
            std::fs::copy(&p, &dest).map_err(io)?;
        }
        let mut cm = self.read_meta(&copy.id)?;
        cm.version = 0; // the scan below sees the copied files and makes it v1
        cm.files.clear();
        self.write_meta(&cm)?;
        self.scan(&copy.id, now)
    }

    /// Danger zone: removes every template, the trash included (permanent).
    pub fn reset(&self) -> AppResult<usize> {
        let n = std::fs::read_dir(&self.root).map(|rd| rd.flatten().filter(|e| e.path().join("template.json").is_file()).count()).unwrap_or(0);
        if self.root.exists() {
            std::fs::remove_dir_all(&self.root).map_err(io)?;
        }
        Ok(n)
    }

    /// Moves the template to `<root>/.trash/` (not a permanent delete).
    pub fn delete(&self, id: &str) -> AppResult<()> {
        let d = self.dir(id)?;
        if !d.is_dir() {
            return Ok(());
        }
        let trash = self.root.join(".trash");
        std::fs::create_dir_all(&trash).map_err(io)?;
        let dest = trash.join(format!("{id}-{}", uuid::Uuid::new_v4().simple()));
        std::fs::rename(&d, dest).map_err(io)
    }

    pub fn read_file(&self, id: &str, rel: &str) -> AppResult<FileText> {
        let p = self.file_path(id, rel)?;
        let size = std::fs::metadata(&p).map_err(io)?.len();
        let text = if is_text_path(rel) && size <= EDIT_LIMIT { String::from_utf8(std::fs::read(&p).map_err(io)?).ok() } else { None };
        Ok(FileText { path: clean_rel(rel)?, size, text })
    }

    pub fn write_file(&self, id: &str, rel: &str, text: &str, now: &str) -> AppResult<TemplateView> {
        self.read_meta(id)?;
        let p = self.file_path(id, rel)?;
        if p.is_dir() {
            return Err(AppError::Invalid("A folder with that name already exists.".into()));
        }
        std::fs::create_dir_all(p.parent().unwrap()).map_err(io)?;
        std::fs::write(&p, text).map_err(io)?;
        self.scan(id, now)
    }

    pub fn delete_file(&self, id: &str, rel: &str, now: &str) -> AppResult<TemplateView> {
        let p = self.file_path(id, rel)?;
        if p.is_file() {
            std::fs::remove_file(&p).map_err(io)?;
        }
        self.tidy(id, &p)?;
        self.scan(id, now)
    }

    /// Removes empty folders above `p`, staying inside files/.
    fn tidy(&self, id: &str, p: &Path) -> AppResult<()> {
        let base = self.files_dir(id)?;
        let mut d = p.parent().map(Path::to_path_buf);
        while let Some(dir) = d {
            if dir == base || !dir.starts_with(&base) || std::fs::remove_dir(&dir).is_err() {
                break;
            }
            d = dir.parent().map(Path::to_path_buf);
        }
        Ok(())
    }

    pub fn rename_file(&self, id: &str, from: &str, to: &str, now: &str) -> AppResult<TemplateView> {
        let a = self.file_path(id, from)?;
        let b = self.file_path(id, to)?;
        if b.exists() {
            return Err(AppError::Invalid(format!("{} already exists.", clean_rel(to)?)));
        }
        std::fs::create_dir_all(b.parent().unwrap()).map_err(io)?;
        std::fs::rename(&a, &b).map_err(io)?;
        self.tidy(id, &a)?;
        self.scan(id, now)
    }

    /// Copies files from disk into `dest_dir` inside the template.
    pub fn add_files(&self, id: &str, dest_dir: &str, sources: &[PathBuf], now: &str) -> AppResult<TemplateView> {
        let base = if dest_dir.trim().is_empty() { self.files_dir(id)? } else { self.file_path(id, dest_dir)? };
        for s in sources {
            let name = s.file_name().ok_or_else(|| AppError::Invalid("Not a file.".into()))?;
            if !s.is_file() {
                continue;
            }
            std::fs::create_dir_all(&base).map_err(io)?;
            std::fs::copy(s, base.join(name)).map_err(io)?;
        }
        self.scan(id, now)
    }

    /// Imports the kept top-level entries of a source as a new template.
    pub fn import(&self, src: &Path, name: &str, keep: &[String], now: &str) -> AppResult<TemplateView> {
        let (source, _) = Source::open(src)?;
        let keep: BTreeSet<&str> = keep.iter().map(|s| s.trim_end_matches('/')).collect();
        if keep.is_empty() {
            return Err(AppError::Invalid("Pick at least one folder or file to keep.".into()));
        }
        let t = self.create(name, "", now)?;
        let to = self.files_dir(&t.id)?;
        let res = source.copy_into(&to, &keep);
        if let Err(e) = res {
            let _ = std::fs::remove_dir_all(self.dir(&t.id)?);
            return Err(e);
        }
        let mut m = self.read_meta(&t.id)?;
        m.version = 0;
        m.files.clear();
        self.write_meta(&m)?;
        self.scan(&t.id, now)
    }
}

// ---------------------------------------------------------------- import sources
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceEntry {
    pub name: String,
    pub is_dir: bool,
    pub files: usize,
    pub bytes: u64,
    pub suggested: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceScan {
    /// folder | instance | mrpack | curseforge
    pub kind: String,
    pub path: String,
    pub suggested_name: String,
    pub entries: Vec<SourceEntry>,
    pub notes: Vec<String>,
}

enum Source {
    Dir(PathBuf),
    /// Archive bytes + the prefixes that map onto the game folder, later wins.
    Zip(Vec<u8>, Vec<&'static str>),
}

impl Source {
    fn open(p: &Path) -> AppResult<(Self, String)> {
        if p.is_dir() {
            // A Prism instance keeps the game folder in minecraft/ or .minecraft/.
            if p.join("mmc-pack.json").is_file() || p.join("instance.cfg").is_file() {
                for g in ["minecraft", ".minecraft"] {
                    if p.join(g).is_dir() {
                        return Ok((Source::Dir(p.join(g)), "instance".into()));
                    }
                }
                return Err(AppError::Invalid("That Prism instance has no game folder yet (it's never been run or installed).".into()));
            }
            return Ok((Source::Dir(p.to_path_buf()), "folder".into()));
        }
        let bytes = std::fs::read(p).map_err(io)?;
        let z = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).map_err(|_| AppError::Invalid("That file isn't a folder, .mrpack or CurseForge zip.".into()))?;
        let names: Vec<String> = z.file_names().filter_map(|n| n.ok()).map(|n| n.to_string()).collect();
        if names.iter().any(|n| n == "modrinth.index.json") {
            Ok((Source::Zip(bytes, vec!["overrides/", "client-overrides/"]), "mrpack".into()))
        } else if names.iter().any(|n| n == "manifest.json") {
            Ok((Source::Zip(bytes, vec!["overrides/"]), "curseforge".into()))
        } else {
            Err(AppError::Invalid("That zip isn't a Modrinth or CurseForge modpack.".into()))
        }
    }

    /// (relative path in the game folder, size). Later prefixes override earlier.
    fn files(&self) -> AppResult<Vec<(String, u64)>> {
        match self {
            Source::Dir(d) => Ok(walk(d)?.into_iter().map(|(r, _, s)| (r, s)).collect()),
            Source::Zip(bytes, prefixes) => {
                let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| AppError::Invalid(e.to_string()))?;
                let mut out = BTreeMap::new();
                for pre in prefixes {
                    for i in 0..z.len() {
                        let f = z.by_index(i).map_err(|e| AppError::Invalid(e.to_string()))?;
                        if f.is_dir() {
                            continue;
                        }
                        let name = f.name().map(|n| n.to_string()).unwrap_or_default();
                        if let Some(rel) = name.strip_prefix(pre) {
                            if let Ok(rel) = clean_rel(rel) {
                                out.insert(rel, f.size());
                            }
                        }
                    }
                }
                Ok(out.into_iter().collect())
            }
        }
    }

    fn copy_into(&self, to: &Path, keep: &BTreeSet<&str>) -> AppResult<()> {
        let kept = |rel: &str| keep.contains(rel.split('/').next().unwrap_or(rel));
        let files: Vec<(String, u64)> = self.files()?.into_iter().filter(|(r, _)| kept(r)).collect();
        if files.len() > MAX_FILES || files.iter().map(|f| f.1).sum::<u64>() > MAX_BYTES {
            return Err(AppError::Invalid(format!("That's too much for a template (over {MAX_FILES} files or {} MB). Keep fewer folders.", MAX_BYTES / 1024 / 1024)));
        }
        match self {
            Source::Dir(d) => {
                for (rel, _) in &files {
                    let dest = to.join(rel);
                    std::fs::create_dir_all(dest.parent().unwrap()).map_err(io)?;
                    std::fs::copy(d.join(rel), dest).map_err(io)?;
                }
            }
            Source::Zip(bytes, prefixes) => {
                let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| AppError::Invalid(e.to_string()))?;
                for pre in prefixes {
                    for i in 0..z.len() {
                        let mut f = z.by_index(i).map_err(|e| AppError::Invalid(e.to_string()))?;
                        if f.is_dir() {
                            continue;
                        }
                        let name = f.name().map(|n| n.to_string()).unwrap_or_default();
                        let Some(rel) = name.strip_prefix(pre).and_then(|r| clean_rel(r).ok()) else { continue };
                        if !kept(&rel) {
                            continue;
                        }
                        let dest = to.join(&rel);
                        std::fs::create_dir_all(dest.parent().unwrap()).map_err(io)?;
                        let mut out = std::fs::File::create(&dest).map_err(io)?;
                        std::io::copy(&mut f, &mut out).map_err(io)?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// What a folder / instance / pack archive offers, for the import dialog.
pub fn scan_source(p: &Path) -> AppResult<SourceScan> {
    let (src, kind) = Source::open(p)?;
    let mut tops: BTreeMap<String, (bool, usize, u64)> = BTreeMap::new();
    let mut notes = vec![];
    let mut skipped = BTreeSet::new();
    for (rel, size) in src.files()? {
        let (top, is_dir) = match rel.split_once('/') {
            Some((t, _)) => (t.to_string(), true),
            None => (rel.clone(), false),
        };
        if NEVER.contains(&top.as_str()) || top.ends_with(".jar") || top.ends_with(".log") || top.starts_with('.') {
            skipped.insert(top);
            continue;
        }
        let e = tops.entry(top).or_insert((is_dir, 0, 0));
        e.1 += 1;
        e.2 += size;
    }
    if !skipped.is_empty() {
        notes.push(format!("Left out: {} (mods, worlds and logs never go in a template).", skipped.into_iter().collect::<Vec<_>>().join(", ")));
    }
    if tops.contains_key("resourcepacks") || tops.contains_key("shaderpacks") {
        notes.push("Resource and shader packs are usually better added as content (milestone 11); they're unticked here.".into());
    }
    if tops.is_empty() {
        notes.push("There's nothing here that a template could keep.".into());
    }
    let suggested_name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Template".into());
    Ok(SourceScan {
        kind,
        path: p.display().to_string(),
        suggested_name: if suggested_name.is_empty() { "Template".into() } else { format!("{suggested_name} configs") },
        entries: tops
            .into_iter()
            .map(|(name, (is_dir, files, bytes))| SourceEntry { suggested: SUGGESTED.contains(&name.as_str()), name, is_dir, files, bytes })
            .collect(),
        notes,
    })
}

// ---------------------------------------------------------------- packs
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MergedFile {
    pub path: String,
    /// Template ids that have this file, lowest priority first: the last one
    /// wins (it's the one highest in the pack's list).
    pub from: Vec<String>,
    pub size: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateChange {
    pub id: String,
    pub name: String,
    pub from_version: u32,
    pub to_version: u32,
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModUse {
    pub hint: String,
    /// The pack entry it's for, if one matches.
    pub entry: Option<String>,
    pub entry_name: Option<String>,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub attached: Vec<TemplateView>,
    /// Attached ids that no longer exist.
    pub missing: Vec<String>,
    pub merged: Vec<MergedFile>,
    pub clashes: usize,
    pub updates: Vec<TemplateChange>,
    /// Config groups by mod; ones with no entry are "unused" (never deleted).
    pub mods: Vec<ModUse>,
}

/// Which pack entry a config hint belongs to (by slug or name), if any.
fn match_entry(pack: &Pack, hint: &str) -> Option<(String, String)> {
    let h = norm(hint);
    if h.len() < 2 {
        return None;
    }
    let mut best: Option<(usize, String, String)> = None;
    for (k, e) in &pack.entries {
        let cands = vec![norm(&e.display.name), norm(&e.display.slug)];
        for c in cands.into_iter().filter(|c| c.len() >= 2) {
            // Exact wins; otherwise one name starting with the other (jei / justenoughitems won't, but sodiumextra / sodiumextraoptions will).
            let score = if c == h { 3 } else if (c.starts_with(&h) && h.len() >= 4) || (h.starts_with(&c) && c.len() >= 4) { 2 } else { 0 };
            if score > 0 && best.as_ref().is_none_or(|b| score > b.0) {
                best = Some((score, k.clone(), e.display.name.clone()));
            }
        }
    }
    // A few well-known config names that aren't the slug.
    if best.is_none() {
        let alias = match h.as_str() {
            "jei" => Some("justenoughitems"),
            "emi" => Some("emi"),
            "rei" => Some("roughlyenoughitems"),
            "kubejs" => Some("kubejs"),
            _ => None,
        };
        if let Some(a) = alias {
            for (k, e) in &pack.entries {
                if norm(&e.display.name) == a || norm(&e.display.slug) == a {
                    return Some((k.clone(), e.display.name.clone()));
                }
            }
        }
    }
    best.map(|b| (b.1, b.2))
}

pub fn changes_since(applied: Option<&AppliedRef>, t: &TemplateMeta, name: &str) -> Option<TemplateChange> {
    let a = applied?;
    if a.version >= t.version {
        return None;
    }
    let (mut added, mut changed, mut removed) = (vec![], vec![], vec![]);
    for (p, h) in &t.files {
        match a.files.get(p) {
            None => added.push(p.clone()),
            Some(o) if o != h => changed.push(p.clone()),
            _ => {}
        }
    }
    for p in a.files.keys() {
        if !t.files.contains_key(p) {
            removed.push(p.clone());
        }
    }
    Some(TemplateChange { id: t.id.clone(), name: name.into(), from_version: a.version, to_version: t.version, added, changed, removed })
}

impl Library {
    pub fn meta(&self, id: &str) -> AppResult<TemplateMeta> {
        self.read_meta(id)
    }

    pub fn overview(&self, pack: &Pack, now: &str) -> AppResult<Overview> {
        let mut attached = vec![];
        let mut missing = vec![];
        let mut updates = vec![];
        for id in &pack.override_templates {
            match self.scan(id, now) {
                Ok(v) => {
                    let m = self.read_meta(id)?;
                    if let Some(c) = changes_since(pack.applied_templates.iter().find(|a| &a.id == id), &m, &v.name) {
                        updates.push(c);
                    }
                    attached.push(v);
                }
                Err(_) => missing.push(id.clone()),
            }
        }
        let mut merged: BTreeMap<String, MergedFile> = BTreeMap::new();
        // Top of the list wins (like resource packs in Minecraft), so apply bottom-up.
        for t in attached.iter().rev() {
            for f in &t.files {
                let m = merged.entry(f.path.clone()).or_insert(MergedFile { path: f.path.clone(), from: vec![], size: 0 });
                m.from.push(t.id.clone());
                m.size = f.size; // the winner's size
            }
        }
        let clashes = merged.values().filter(|m| m.from.len() > 1).count();
        let mut mods: BTreeMap<String, ModUse> = BTreeMap::new();
        for m in merged.values() {
            if let Some(h) = mod_hint(&m.path) {
                let u = mods.entry(h.clone()).or_insert_with(|| {
                    let e = match_entry(pack, &h);
                    ModUse { hint: h.clone(), entry: e.as_ref().map(|x| x.0.clone()), entry_name: e.map(|x| x.1), files: vec![] }
                });
                u.files.push(m.path.clone());
            }
        }
        Ok(Overview { attached, missing, merged: merged.into_values().collect(), clashes, updates, mods: mods.into_values().collect() })
    }

    /// Files for `overrides/`, winners only: (relative path, file on disk).
    pub fn export_files(&self, pack: &Pack) -> AppResult<Vec<(String, PathBuf)>> {
        let mut out: BTreeMap<String, PathBuf> = BTreeMap::new();
        // Bottom-up, so the template at the top of the list wins clashes.
        for id in pack.override_templates.iter().rev() {
            let Ok(dir) = self.files_dir(id) else { continue };
            if !self.dir(id)?.join("template.json").is_file() {
                continue;
            }
            for (rel, p, _) in walk(&dir)? {
                out.insert(rel, p);
            }
        }
        Ok(out.into_iter().collect())
    }

    /// The `appliedTemplates` record for "reviewed as of now".
    pub fn applied_ref(&self, id: &str) -> AppResult<AppliedRef> {
        let m = self.read_meta(id)?;
        Ok(AppliedRef { id: m.id, version: m.version, files: m.files })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> (Library, PathBuf) {
        let d = std::env::temp_dir().join(format!("mpp-tpl-{}", uuid::Uuid::new_v4()));
        (Library::new(d.join("templates")), d)
    }

    #[test]
    fn paths_are_kept_inside() {
        assert_eq!(clean_rel("config/jei/jei.toml").unwrap(), "config/jei/jei.toml");
        assert_eq!(clean_rel("./config//a.json").unwrap(), "config/a.json");
        for bad in ["../x", "/etc/passwd", "config/../../x", "", "C:/x", "a/\u{0}b"] {
            assert!(clean_rel(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn hints() {
        assert_eq!(mod_hint("config/jei/jei-client.toml").as_deref(), Some("jei"));
        assert_eq!(mod_hint("config/sodium-extra-options.json").as_deref(), Some("sodium-extra-options"));
        assert_eq!(mod_hint("config/create-client.toml").as_deref(), Some("create"));
        assert_eq!(mod_hint("kubejs/server_scripts/a.js").as_deref(), Some("kubejs"));
        assert_eq!(mod_hint("config/fml.toml"), None);
        assert_eq!(mod_hint("config/neoforge-client.toml"), None);
        assert_eq!(mod_hint("options.txt"), None);
    }

    #[test]
    fn edits_bump_the_version_and_files_round_trip() {
        let (l, d) = lib();
        let t = l.create("Base configs", "", "t1").unwrap();
        assert_eq!((t.version, t.files.len()), (1, 0));
        let t = l.write_file(&t.id, "config/jei/jei-client.toml", "a = 1\n", "t2").unwrap();
        assert_eq!(t.version, 2);
        assert!(t.files[0].editable);
        assert_eq!(t.folders, vec!["config/"]);
        // Rescanning with no change keeps the version.
        assert_eq!(l.scan(&t.id, "t3").unwrap().version, 2);
        // Editing outside the app is noticed too.
        std::fs::write(l.files_dir(&t.id).unwrap().join("config/jei/jei-client.toml"), "a = 2\n").unwrap();
        assert_eq!(l.scan(&t.id, "t4").unwrap().version, 3);
        assert_eq!(l.read_file(&t.id, "config/jei/jei-client.toml").unwrap().text.as_deref(), Some("a = 2\n"));
        // Renaming and metadata.
        let t = l.rename_file(&t.id, "config/jei/jei-client.toml", "config/jei.toml", "t5").unwrap();
        assert_eq!(t.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["config/jei.toml"]);
        assert!(!l.files_dir(&t.id).unwrap().join("config/jei").exists(), "empty folder tidied");
        let v = t.version;
        let t = l.set_meta(&t.id, "Renamed", "desc", "t6").unwrap();
        assert_eq!((t.name.as_str(), t.version), ("Renamed", v));
        assert!(l.write_file(&t.id, "../escape.txt", "x", "t").is_err());
        // Duplicate, delete (to trash).
        let c = l.duplicate(&t.id, "t7").unwrap();
        assert_eq!((c.version, c.files.len()), (1, 1));
        l.delete(&t.id).unwrap();
        assert_eq!(l.list("t8").unwrap().len(), 1);
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn imports_from_folder_and_archive() {
        let (l, d) = lib();
        // A Prism-like instance.
        let inst = d.join("Inst");
        for (p, s) in [("minecraft/config/jei/a.toml", "x"), ("minecraft/mods/a.jar", "jar"), ("minecraft/options.txt", "o"), ("minecraft/kubejs/s.js", "k"), ("minecraft/logs/latest.log", "l")] {
            let f = inst.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, s).unwrap();
        }
        std::fs::write(inst.join("mmc-pack.json"), "{}").unwrap();
        let s = scan_source(&inst).unwrap();
        assert_eq!(s.kind, "instance");
        let names: Vec<_> = s.entries.iter().map(|e| (e.name.as_str(), e.suggested)).collect();
        assert_eq!(names, vec![("config", true), ("kubejs", true), ("options.txt", true)]);
        assert!(s.notes[0].contains("mods"));
        let t = l.import(&inst, "From instance", &["config".into(), "options.txt".into()], "t1").unwrap();
        assert_eq!(t.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), vec!["config/jei/a.toml", "options.txt"]);
        assert_eq!(t.version, 1);
        assert!(inst.join("minecraft/config/jei/a.toml").is_file(), "source untouched");

        // An .mrpack: client-overrides win over overrides.
        let mut buf = std::io::Cursor::new(vec![]);
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let o = zip::write::SimpleFileOptions::default();
            for (n, s) in [("modrinth.index.json", "{}"), ("overrides/config/a.json", "1"), ("client-overrides/config/a.json", "2"), ("overrides/mods/x.jar", "j")] {
                z.start_file(n, o).unwrap();
                std::io::Write::write_all(&mut z, s.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        let mp = d.join("p.mrpack");
        std::fs::write(&mp, buf.into_inner()).unwrap();
        let s = scan_source(&mp).unwrap();
        assert_eq!((s.kind.as_str(), s.entries.len()), ("mrpack", 1));
        let t = l.import(&mp, "From pack", &["config".into()], "t2").unwrap();
        assert_eq!(l.read_file(&t.id, "config/a.json").unwrap().text.as_deref(), Some("2"));
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn overview_clashes_updates_and_mods() {
        let (l, d) = lib();
        let a = l.create("A", "", "t").unwrap();
        l.write_file(&a.id, "config/jei/x.toml", "a", "t").unwrap();
        l.write_file(&a.id, "config/ghostmod.toml", "a", "t").unwrap();
        let b = l.create("B", "", "t").unwrap();
        l.write_file(&b.id, "config/jei/x.toml", "b", "t").unwrap();
        let mut pack = crate::resolve::tests::pack_for("1.21.1", crate::sources::Loader::Fabric);
        let mut e = crate::model::tests::entry(Some("u6dRKJwZ"), None, "Just Enough Items", "t");
        e.display.slug = "jei".into();
        pack.entries.insert("k1".into(), e);
        // B is on top, so it wins the clash.
        pack.override_templates = vec![b.id.clone(), a.id.clone(), "gone".into()];
        pack.applied_templates = vec![l.applied_ref(&a.id).unwrap(), l.applied_ref(&b.id).unwrap()];
        let o = l.overview(&pack, "t").unwrap();
        assert_eq!((o.clashes, o.missing.clone(), o.updates.len()), (1, vec!["gone".to_string()], 0));
        let jei = o.mods.iter().find(|m| m.hint == "jei").unwrap();
        assert_eq!(jei.entry.as_deref(), Some("k1"));
        assert!(o.mods.iter().find(|m| m.hint == "ghostmod").unwrap().entry.is_none(), "unused config flagged");
        // The top template wins on export.
        let files = l.export_files(&pack).unwrap();
        let jx = files.iter().find(|f| f.0 == "config/jei/x.toml").unwrap();
        assert_eq!(std::fs::read_to_string(&jx.1).unwrap(), "b");
        // Edit A → an update with the changed file listed.
        l.write_file(&a.id, "config/jei/x.toml", "a2", "t").unwrap();
        l.write_file(&a.id, "config/new.json", "{}", "t").unwrap();
        let o = l.overview(&pack, "t").unwrap();
        let u = &o.updates[0];
        assert_eq!((u.changed.clone(), u.added.clone(), u.removed.len()), (vec!["config/jei/x.toml".to_string()], vec!["config/new.json".to_string()], 0));
        let _ = std::fs::remove_dir_all(d);
    }
}
