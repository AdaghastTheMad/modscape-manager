//! Modpack Planner core. Every network call, all storage and all secrets live
//! here; the webview only talks to these typed commands.

pub mod error;
pub mod export;
pub mod fetch;
pub mod import;
pub mod keychain;
pub mod loaders;
pub mod model;
pub mod payload;
pub mod port;
pub mod resolve;
pub mod sources;
pub mod store;
pub mod templates;

use base64::Engine;
use error::{AppError, AppResult};
use fetch::Fetcher;
use model::Pack;
use sources::{curseforge::CurseForge, modrinth, modrinth::Modrinth, Loader, ModSource, SearchPage, SearchQuery, Source};
use std::sync::Arc;
use store::Db;
use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

/// Generic, identifiable User-Agent (Modrinth asks for one). No personal
/// contact details: add the public project URL here once one exists,
/// e.g. "modpack-planner/0.1.0 (+https://github.com/…)".
pub const USER_AGENT: &str = concat!("modpack-planner/", env!("CARGO_PKG_VERSION"));

pub struct AppState {
    pub db: Arc<Db>,
    pub fetcher: Arc<Fetcher>,
    pub modrinth: Modrinth,
    pub curseforge: CurseForge,
    pub templates: templates::Library,
}

/// Catalog plus the hash lookups importers need.
pub struct Live<'a>(resolve::Multi<'a>, &'a AppState);

#[async_trait::async_trait]
impl resolve::Catalog for Live<'_> {
    async fn versions(&self, p: &str, mc: &str, l: Loader, force: bool) -> AppResult<Vec<resolve::Version>> {
        self.0.versions(p, mc, l, force).await
    }
    async fn version(&self, id: &str, force: bool) -> AppResult<Option<resolve::Version>> {
        self.0.version(id, force).await
    }
    async fn projects(&self, ids: &[String], force: bool) -> AppResult<Vec<resolve::ProjectInfo>> {
        self.0.projects(ids, force).await
    }
}

#[async_trait::async_trait]
impl import::Lookup for Live<'_> {
    async fn modrinth_by_sha1(&self, hashes: &[String]) -> AppResult<std::collections::HashMap<String, resolve::Version>> {
        self.1.modrinth.versions_by_sha1(hashes, false).await
    }
    async fn curseforge_by_fingerprint(&self, fps: &[u32]) -> AppResult<std::collections::HashMap<u32, resolve::Version>> {
        self.1.curseforge.by_fingerprints(fps).await
    }
}

impl AppState {
    pub fn live(&self) -> Live<'_> {
        Live(self.catalog(), self)
    }

    /// Both sites behind one catalog for the resolver.
    pub fn catalog(&self) -> resolve::Multi<'_> {
        resolve::Multi { modrinth: &self.modrinth, curseforge: &self.curseforge }
    }

    pub fn new(db: Db) -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(20))
            .connect_timeout(std::time::Duration::from_secs(10))
            .https_only(true)
            .build()
            .expect("HTTP client");
        let db = Arc::new(db);
        let fetcher = Arc::new(Fetcher::new(http, db.clone()));
        let templates = templates::Library::new(std::env::temp_dir().join(format!("modpack-planner-templates-{}", uuid::Uuid::new_v4())));
        Self { modrinth: Modrinth::new(fetcher.clone()), curseforge: CurseForge::new(fetcher.clone()), db, fetcher, templates }
    }

    /// Keeps override templates in `root` (the app data dir's `templates/`).
    pub fn with_templates(mut self, root: std::path::PathBuf) -> Self {
        self.templates = templates::Library::new(root);
        self
    }

    /// Export pre-flight including override templates.
    pub async fn export_plan(&self, pack: &Pack, format: export::Format) -> export::ExportPlan {
        let mut plan = export::plan_export(pack, &self.catalog(), format).await;
        export::add_overrides(&mut plan, pack, &self.templates, &store::now_iso());
        plan
    }

    pub fn source(&self, s: Source) -> &dyn ModSource {
        match s {
            Source::Modrinth => &self.modrinth,
            Source::Curseforge => &self.curseforge,
        }
    }
}

type St<'a> = tauri::State<'a, AppState>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::Invalid(e.to_string()))
}

// ---------------- app / keychain ----------------
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    user_agent: &'static str,
    os: &'static str,
    allowed_hosts: &'static [&'static str],
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), user_agent: USER_AGENT, os: std::env::consts::OS, allowed_hosts: fetch::ALLOWED_HOSTS }
}

#[tauri::command]
async fn key_status() -> AppResult<keychain::KeyStatus> {
    blocking(keychain::status).await
}

#[tauri::command]
async fn save_curseforge_key(state: St<'_>, key: String) -> AppResult<keychain::KeyStatus> {
    let key = keychain::validate(&key)?;
    blocking(move || keychain::set_key(&key)).await??;
    state.curseforge.forget_key();
    blocking(keychain::status).await
}

#[tauri::command]
async fn clear_curseforge_key(state: St<'_>) -> AppResult<keychain::KeyStatus> {
    blocking(keychain::clear_key).await??;
    state.curseforge.forget_key();
    blocking(keychain::status).await
}

// ---------------- catalogue ----------------
#[tauri::command]
async fn search(state: St<'_>, source: Source, query: SearchQuery) -> AppResult<SearchPage> {
    state.source(source).search(&query).await
}

#[tauri::command]
async fn project_details(state: St<'_>, source: Source, id: String, force: Option<bool>) -> AppResult<modrinth::ProjectDetails> {
    match source {
        Source::Modrinth => state.modrinth.project(&id, force.unwrap_or(false)).await,
        Source::Curseforge => {
            let n: u64 = id.trim_start_matches(sources::curseforge::PREFIX).parse().map_err(|_| AppError::Invalid("Not a CurseForge id.".into()))?;
            state.curseforge.project(n, force.unwrap_or(false)).await
        }
    }
}

#[tauri::command]
async fn categories(state: St<'_>, force: Option<bool>) -> AppResult<Vec<modrinth::Category>> {
    state.modrinth.categories(force.unwrap_or(false)).await
}

#[tauri::command]
async fn game_versions(state: St<'_>, force: Option<bool>) -> AppResult<Vec<modrinth::GameVersion>> {
    state.modrinth.game_versions(force.unwrap_or(false)).await
}

#[tauri::command]
async fn loader_versions(state: St<'_>, loader: Loader, mc_version: String, force: Option<bool>) -> AppResult<loaders::LoaderVersions> {
    loaders::loader_versions(&state.fetcher, loader, &mc_version, force.unwrap_or(false)).await
}

#[tauri::command]
async fn icon(state: St<'_>, url: String) -> AppResult<String> {
    state.fetcher.icon_data_url(&url).await
}

/// Searches the chosen sites at once and merges the results.
pub async fn search_all_impl(
    st: &AppState,
    query: SearchQuery,
    sources: Vec<Source>,
    prefer: Source,
    mr_offset: u32,
    cf_offset: u32,
) -> AppResult<sources::merge::MergedPage> {
    use sources::merge::{merge, MergedPage, PerSource};
    let want_mr = sources.contains(&Source::Modrinth);
    let want_cf = sources.contains(&Source::Curseforge);
    let mut mq = query.clone();
    mq.offset = mr_offset;
    let mut cq = query.clone();
    cq.offset = cf_offset;
    let (mr, cf) = futures::join!(
        async { if want_mr { Some(st.modrinth.search(&mq).await) } else { None } },
        async { if want_cf { Some(st.curseforge.search_with_notes(&cq).await) } else { None } },
    );
    let mut notes = Vec::new();
    let mr = match mr {
        Some(Ok(p)) => Some(p),
        Some(Err(e)) if !want_cf => return Err(e),
        Some(Err(e)) => {
            notes.push(format!("Modrinth: {e}"));
            None
        }
        None => None,
    };
    let cf = match cf {
        Some(Ok((p, n))) => {
            notes.extend(n);
            Some(p)
        }
        Some(Err(AppError::NoCurseForgeKey)) => {
            notes.push("Add a CurseForge API key in Settings to search CurseForge too.".into());
            None
        }
        Some(Err(e)) if mr.is_none() => return Err(e),
        Some(Err(e)) => {
            notes.push(format!("CurseForge: {e}"));
            None
        }
        None => None,
    };
    let stale = [&mr, &cf].iter().any(|p| p.as_ref().and_then(|p| p.freshness.as_ref()).is_some_and(|f| f.stale));
    let next_of = |p: &Option<SearchPage>, off: u32, cap: u64| {
        p.as_ref().and_then(|p| {
            let n = off as u64 + p.hits.len() as u64;
            (!p.hits.is_empty() && n < p.total.min(cap)).then_some(n as u32)
        })
    };
    let hits = merge(mr.as_ref(), cf.as_ref(), query.sort, prefer);
    Ok(MergedPage {
        totals: PerSource { modrinth: mr.as_ref().map(|p| p.total), curseforge: cf.as_ref().map(|p| p.total) },
        next: PerSource { modrinth: next_of(&mr, mr_offset, u64::MAX), curseforge: next_of(&cf, cf_offset, 10_000) },
        hits,
        notes,
        stale,
    })
}

#[tauri::command]
async fn search_all(
    state: St<'_>,
    query: SearchQuery,
    sources: Vec<Source>,
    prefer: Source,
    mr_offset: u32,
    cf_offset: u32,
) -> AppResult<sources::merge::MergedPage> {
    search_all_impl(&state, query, sources, prefer, mr_offset, cf_offset).await
}

#[tauri::command]
async fn find_matches(state: St<'_>, pack: Pack, key: String) -> AppResult<Vec<sources::matching::Candidate>> {
    let e = pack.entries.get(&key).ok_or_else(|| AppError::Invalid("That mod isn't in the pack.".into()))?;
    sources::matching::find_counterparts(&state.modrinth, &state.curseforge, e, &pack.mc_version, pack.loader).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryMatches {
    key: String,
    candidates: Vec<sources::matching::Candidate>,
}

/// Looks for the other site's copy of every mod that's only linked on one site.
pub async fn match_all_impl(st: &AppState, pack: &Pack) -> AppResult<Vec<EntryMatches>> {
    use futures::stream::{self, StreamExt};
    let todo: Vec<(String, model::PackEntry)> = pack
        .entries
        .iter()
        .filter(|(_, e)| e.mod_ref.modrinth_id.is_some() != e.mod_ref.curseforge_id.is_some())
        .map(|(k, e)| (k.clone(), e.clone()))
        .collect();
    if todo.iter().any(|(_, e)| e.mod_ref.curseforge_id.is_none()) {
        // Surface a missing key once instead of failing every lookup.
        st.curseforge.ensure_key().await?;
    }
    let (mc, loader) = (pack.mc_version.clone(), pack.loader);
    let (mr, cf) = (&st.modrinth, &st.curseforge);
    let futs = todo.into_iter().map(|(key, e)| {
        let mc = mc.clone();
        Box::pin(async move {
            let candidates = sources::matching::find_counterparts(mr, cf, &e, &mc, loader).await.unwrap_or_default();
            EntryMatches { key, candidates }
        }) as std::pin::Pin<Box<dyn std::future::Future<Output = EntryMatches> + Send + '_>>
    });
    let results: Vec<EntryMatches> = stream::iter(futs).buffer_unordered(4).collect().await;
    Ok(results.into_iter().filter(|m| !m.candidates.is_empty()).collect())
}

#[tauri::command]
async fn match_all(state: St<'_>, pack: Pack) -> AppResult<Vec<EntryMatches>> {
    match_all_impl(&state, &pack).await
}

// ---------------- resolver ----------------
#[tauri::command]
async fn analyze_pack(state: St<'_>, pack: Pack, force: Option<bool>) -> AppResult<resolve::Analysis> {
    Ok(resolve::analyze(&pack, &state.catalog(), force.unwrap_or(false), store::now_secs()).await)
}

#[tauri::command]
async fn plan_add(
    state: St<'_>,
    pack: Pack,
    project_id: String,
    pin_version: Option<String>,
    for_key: Option<String>,
    alt_id: Option<String>,
    added_at: String,
) -> AppResult<resolve::AddPlan> {
    resolve::plan_add(&pack, &project_id, pin_version.as_deref(), for_key.as_deref(), alt_id.as_deref(), &state.catalog(), false, &added_at, || uuid::Uuid::new_v4().to_string()).await
}

#[tauri::command]
fn plan_removal(pack: Pack, keys: Vec<String>) -> Vec<String> {
    model::plan_removal(&pack, &keys)
}

#[tauri::command]
async fn entry_versions(state: St<'_>, project_id: String, mc_version: String, loader: Loader) -> AppResult<Vec<resolve::FileInfo>> {
    use resolve::Catalog;
    let vs = state.catalog().versions(&project_id, &mc_version, loader, false).await?;
    Ok(resolve::compatible_files(&vs, &mc_version, loader))
}

// ---------------- payloads ----------------
fn payload_map(st: &AppState) -> AppResult<std::collections::HashMap<String, payload::Payload>> {
    Ok(st.db.all_payloads()?.into_iter().map(|p| (p.id.clone(), p)).collect())
}

#[tauri::command]
fn list_payloads(state: St<'_>) -> AppResult<Vec<payload::Payload>> {
    state.db.all_payloads()
}

#[derive(serde::Serialize)]
pub struct SavedPayload {
    payload: payload::Payload,
    notes: Vec<String>,
}

pub fn save_payload_impl(st: &AppState, mut p: payload::Payload) -> AppResult<SavedPayload> {
    let prev = st.db.get_payload(&p.id)?;
    let notes = payload::prepare_for_save(&mut p, prev.as_ref(), &store::now_iso())?;
    // Nesting must not form a loop through other payloads.
    let mut all = payload_map(st)?;
    all.insert(p.id.clone(), p.clone());
    let mut loop_notes = Vec::new();
    payload::flatten(&p, &all, &mut loop_notes);
    if loop_notes.iter().any(|n| n.contains("loop")) {
        return Err(AppError::Invalid("Those nested payloads would include each other in a loop.".into()));
    }
    st.db.put_payload(&p)?;
    Ok(SavedPayload { payload: p, notes })
}

#[tauri::command]
fn save_payload(state: St<'_>, payload: payload::Payload) -> AppResult<SavedPayload> {
    save_payload_impl(&state, payload)
}

#[tauri::command]
fn delete_payload(state: St<'_>, id: String) -> AppResult<()> {
    state.db.delete_payload(&id)
}

#[tauri::command]
fn restore_starter_payloads(state: St<'_>) -> AppResult<usize> {
    payload::seed_starters(&state.db, &store::now_iso(), true)
}

pub async fn plan_payload_impl(st: &AppState, pack: &Pack, payload_id: &str, choices: std::collections::HashMap<String, String>) -> AppResult<payload::PayloadPlan> {
    let all = payload_map(st)?;
    let p = all.get(payload_id).ok_or_else(|| AppError::Invalid("That payload no longer exists.".into()))?;
    Ok(payload::plan(pack, p, &all, &choices, &st.catalog()).await)
}

#[tauri::command]
async fn plan_payload(state: St<'_>, pack: Pack, payload_id: String, choices: Option<std::collections::HashMap<String, String>>) -> AppResult<payload::PayloadPlan> {
    plan_payload_impl(&state, &pack, &payload_id, choices.unwrap_or_default()).await
}

#[tauri::command]
fn payload_from_entries(state: St<'_>, pack: Pack, keys: Vec<String>, name: String) -> AppResult<SavedPayload> {
    let p = payload::from_entries(&pack, &keys, &name, &store::now_iso())?;
    save_payload_impl(&state, p)
}

#[tauri::command]
fn payload_updates(state: St<'_>, pack: Pack) -> AppResult<Vec<payload::UpdateNotice>> {
    Ok(payload::updates_for(&pack, &payload_map(&state)?))
}

#[tauri::command]
async fn export_payload(app: tauri::AppHandle, state: St<'_>, id: String) -> AppResult<Option<String>> {
    let p = state.db.get_payload(&id)?.ok_or_else(|| AppError::Invalid("That payload no longer exists.".into()))?;
    let json = payload::to_file_json(&p)?;
    let picked = app.dialog().file().set_title("Export payload").add_filter("Modpack Planner payload", &["json"]).set_file_name(format!("{}.payload.json", safe_file_name(&p.name))).blocking_save_file();
    let Some(fp) = picked else { return Ok(None) };
    let path = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    std::fs::write(&path, json).map_err(|e| AppError::Storage(format!("Couldn't write the file: {e}")))?;
    Ok(Some(path.display().to_string()))
}

pub fn import_payload_text(st: &AppState, text: &str) -> AppResult<SavedPayload> {
    let mut p = payload::parse_file(text)?;
    p.starter = false;
    let mut notes = vec![];
    if st.db.get_payload(&p.id)?.is_some() {
        p.id = uuid::Uuid::new_v4().to_string();
        p.name = format!("{} (imported)", p.name);
        notes.push("A payload with the same id already exists, so this was imported as a copy.".into());
    }
    let mut saved = save_payload_impl(st, p)?;
    saved.notes.splice(0..0, notes);
    Ok(saved)
}

#[tauri::command]
async fn import_payload(app: tauri::AppHandle, state: St<'_>) -> AppResult<Option<SavedPayload>> {
    let picked = app.dialog().file().set_title("Import payload").add_filter("Modpack Planner payload", &["json"]).blocking_pick_file();
    let Some(fp) = picked else { return Ok(None) };
    let path = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    let text = std::fs::read_to_string(&path).map_err(|e| AppError::Storage(format!("Couldn't read the file: {e}")))?;
    import_payload_text(&state, &text).map(Some)
}

// ---------------- duplicate to another version ----------------
#[tauri::command]
async fn plan_port(state: St<'_>, pack: Pack, target: port::PortTarget) -> AppResult<port::PortPlan> {
    port::plan_port(&pack, &target, &state.catalog(), &store::now_iso(), || uuid::Uuid::new_v4().to_string()).await
}

// ---------------- export / import ----------------
#[tauri::command]
async fn export_plan(state: St<'_>, pack: Pack, format: export::Format) -> AppResult<export::ExportPlan> {
    Ok(state.export_plan(&pack, format).await)
}

/// Re-plans (so the file matches the pack right now), asks where to save, writes.
#[tauri::command]
async fn export_pack(app: tauri::AppHandle, state: St<'_>, pack: Pack, format: export::Format) -> AppResult<Option<String>> {
    let plan = state.export_plan(&pack, format).await;
    if !plan.can_export {
        return Err(AppError::Invalid("Fix the errors in the pre-flight check before exporting.".into()));
    }
    let (label, ext) = match format {
        export::Format::Mrpack => ("Modrinth pack", "mrpack"),
        export::Format::CurseforgeZip => ("CurseForge pack", "zip"),
    };
    let picked = app.dialog().file().set_title("Export pack").add_filter(label, &[ext]).set_file_name(format!("{}.{ext}", safe_file_name(&pack.name))).blocking_save_file();
    let Some(fp) = picked else { return Ok(None) };
    let path = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    let overrides = state.templates.export_files(&pack)?;
    export::write_archive(&pack, &plan, &overrides, &path)?;
    Ok(Some(path.display().to_string()))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pack: Pack,
    notes: Vec<String>,
    unknown: Vec<String>,
    has_overrides: bool,
    /// What the source offers as an override template (when it has config files).
    override_source: Option<templates::SourceScan>,
}

fn store_import(st: &AppState, mut r: import::Imported, source: &std::path::Path) -> AppResult<ImportReport> {
    let mut notes = r.notes;
    notes.extend(model::normalize(&mut r.pack)?);
    st.db.put_pack(&r.pack)?;
    let override_source = if r.has_overrides { templates::scan_source(source).ok().filter(|s| !s.entries.is_empty()) } else { None };
    if r.has_overrides && override_source.is_none() {
        notes.push("It also has override files, but none that a template could keep.".into());
    }
    Ok(ImportReport { pack: r.pack, notes, unknown: r.unknown, has_overrides: r.has_overrides, override_source })
}

pub async fn import_file_impl(st: &AppState, path: &std::path::Path) -> AppResult<ImportReport> {
    let meta = std::fs::metadata(path).map_err(|e| AppError::Storage(e.to_string()))?;
    if meta.len() > 64 * 1024 * 1024 {
        return Err(AppError::Invalid("That file is too large to be a modpack.".into()));
    }
    let bytes = std::fs::read(path).map_err(|e| AppError::Storage(format!("Couldn't read the file: {e}")))?;
    let r = import::import_archive(&bytes, &st.live()).await?;
    store_import(st, r, path)
}

pub async fn import_instance_impl(st: &AppState, dir: &std::path::Path) -> AppResult<ImportReport> {
    let r = import::import_prism_instance(dir, &st.live()).await?;
    store_import(st, r, dir)
}

#[tauri::command]
async fn import_pack_file(app: tauri::AppHandle, state: St<'_>) -> AppResult<Option<ImportReport>> {
    let picked = app.dialog().file().set_title("Import a modpack").add_filter("Modpack", &["mrpack", "zip"]).blocking_pick_file();
    let Some(fp) = picked else { return Ok(None) };
    let path = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    import_file_impl(&state, &path).await.map(Some)
}

/// Read-only: picks an instance folder and reads it; nothing in it is changed.
#[tauri::command]
async fn import_prism_instance(app: tauri::AppHandle, state: St<'_>) -> AppResult<Option<ImportReport>> {
    let picked = app.dialog().file().set_title("Choose a Prism instance folder").blocking_pick_folder();
    let Some(fp) = picked else { return Ok(None) };
    let dir = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    import_instance_impl(&state, &dir).await.map(Some)
}

// ---------------- packs ----------------
#[derive(serde::Serialize)]
struct Saved {
    pack: Pack,
    notes: Vec<String>,
}

#[tauri::command]
fn list_packs(state: St<'_>) -> AppResult<Vec<store::PackSummary>> {
    state.db.list_packs()
}

#[tauri::command]
fn get_pack(state: St<'_>, id: String) -> AppResult<Pack> {
    state.db.get_pack(&id)?.ok_or_else(|| AppError::Invalid("That pack no longer exists.".into()))
}

#[tauri::command]
fn save_pack(state: St<'_>, mut pack: Pack) -> AppResult<Saved> {
    let notes = model::normalize(&mut pack)?;
    state.db.put_pack(&pack)?;
    Ok(Saved { pack, notes })
}

#[tauri::command]
fn delete_pack(state: St<'_>, id: String) -> AppResult<()> {
    state.db.delete_pack(&id)
}

fn safe_file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "pack".into() } else { s }
}

/// Opens a save dialog and writes the planner project JSON. Returns the path, or None if cancelled.
#[tauri::command]
async fn export_project(app: tauri::AppHandle, pack: Pack, exported_at: String) -> AppResult<Option<String>> {
    let json = model::to_project_json(&pack, &exported_at)?;
    let picked = app
        .dialog()
        .file()
        .set_title("Save planner project")
        .add_filter("Modpack Planner project", &["json"])
        .set_file_name(format!("{}.planner.json", safe_file_name(&pack.name)))
        .blocking_save_file();
    let Some(fp) = picked else { return Ok(None) };
    let path = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &path)).map_err(|e| AppError::Storage(format!("Couldn't write the file: {e}")))?;
    Ok(Some(path.display().to_string()))
}

/// Opens a file dialog, reads a planner project and stores it as a new pack.
#[tauri::command]
async fn import_project(app: tauri::AppHandle, state: St<'_>) -> AppResult<Option<Saved>> {
    let picked = app.dialog().file().set_title("Open planner project").add_filter("Modpack Planner project", &["json"]).blocking_pick_file();
    let Some(fp) = picked else { return Ok(None) };
    let path = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    let meta = std::fs::metadata(&path).map_err(|e| AppError::Storage(e.to_string()))?;
    if meta.len() > 32 * 1024 * 1024 {
        return Err(AppError::Invalid("That file is too large to be a planner project.".into()));
    }
    let text = std::fs::read_to_string(&path).map_err(|e| AppError::Storage(format!("Couldn't read the file: {e}")))?;
    let (mut pack, mut notes) = model::parse_project(&text)?;
    if state.db.pack_exists(&pack.id)? {
        pack.id = uuid::Uuid::new_v4().to_string();
        pack.name = format!("{} (imported)", pack.name);
        notes.push("A pack with the same id already exists, so this was imported as a copy.".into());
    }
    state.db.put_pack(&pack)?;
    Ok(Some(Saved { pack, notes }))
}

/// Opens an image picker and returns the pack icon as a data URL.
#[tauri::command]
async fn pick_icon(app: tauri::AppHandle) -> AppResult<Option<String>> {
    let picked = app.dialog().file().set_title("Choose a pack icon").add_filter("Image", &["png", "jpg", "jpeg", "webp", "gif"]).blocking_pick_file();
    let Some(fp) = picked else { return Ok(None) };
    let path = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    let bytes = std::fs::read(&path).map_err(|e| AppError::Storage(e.to_string()))?;
    if bytes.len() > model::MAX_ICON_BYTES {
        return Err(AppError::Invalid("Icons are limited to 512 KB. A 128×128 PNG is plenty.".into()));
    }
    let mime = match &bytes[..bytes.len().min(12)] {
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [0xFF, 0xD8, 0xFF, ..] => "image/jpeg",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P'] => "image/webp",
        _ => return Err(AppError::Invalid("That file isn't a PNG, JPEG, WebP or GIF image.".into())),
    };
    Ok(Some(format!("data:{mime};base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes))))
}

// ---------------- override templates (M9) ----------------
type Tpl = templates::TemplateView;
fn now() -> String {
    store::now_iso()
}

/// Async (off the main thread): scanning re-checks every template's files.
#[tauri::command]
async fn list_templates(state: St<'_>) -> AppResult<Vec<Tpl>> {
    state.templates.list(&now())
}

#[tauri::command]
fn create_template(state: St<'_>, name: String) -> AppResult<Tpl> {
    state.templates.create(&name, "", &now())
}

#[tauri::command]
fn set_template_meta(state: St<'_>, id: String, name: String, description: String) -> AppResult<Tpl> {
    state.templates.set_meta(&id, &name, &description, &now())
}

#[tauri::command]
fn duplicate_template(state: St<'_>, id: String) -> AppResult<Tpl> {
    state.templates.duplicate(&id, &now())
}

#[tauri::command]
fn delete_template(state: St<'_>, id: String) -> AppResult<()> {
    state.templates.delete(&id)
}

#[tauri::command]
fn read_template_file(state: St<'_>, id: String, path: String) -> AppResult<templates::FileText> {
    state.templates.read_file(&id, &path)
}

#[tauri::command]
fn write_template_file(state: St<'_>, id: String, path: String, text: String) -> AppResult<Tpl> {
    state.templates.write_file(&id, &path, &text, &now())
}

#[tauri::command]
fn delete_template_file(state: St<'_>, id: String, path: String) -> AppResult<Tpl> {
    state.templates.delete_file(&id, &path, &now())
}

#[tauri::command]
fn rename_template_file(state: St<'_>, id: String, from: String, to: String) -> AppResult<Tpl> {
    state.templates.rename_file(&id, &from, &to, &now())
}

/// Copies files picked in a dialog into `dir` inside the template.
#[tauri::command]
async fn add_template_files(app: tauri::AppHandle, state: St<'_>, id: String, dir: String) -> AppResult<Option<Tpl>> {
    let Some(picked) = app.dialog().file().set_title("Add files to the template").blocking_pick_files() else { return Ok(None) };
    let paths: Vec<std::path::PathBuf> = picked.into_iter().filter_map(|f| f.into_path().ok()).collect();
    state.templates.add_files(&id, &dir, &paths, &now()).map(Some)
}

/// Opens the template's folder (or one of its files) with the system's default app.
#[tauri::command]
fn open_template_path(app: tauri::AppHandle, state: St<'_>, id: String, path: Option<String>) -> AppResult<()> {
    let base = state.templates.files_dir(&id)?;
    std::fs::create_dir_all(&base).map_err(|e| AppError::Storage(e.to_string()))?;
    let target = match path {
        Some(p) => base.join(templates::clean_rel(&p)?),
        None => base,
    };
    app.opener().open_path(target.display().to_string(), None::<&str>).map_err(|e| AppError::Invalid(e.to_string()))
}

/// Asks for a folder / Prism instance (`folder`) or a pack archive, then lists what it offers.
#[tauri::command]
async fn pick_template_source(app: tauri::AppHandle, folder: bool) -> AppResult<Option<templates::SourceScan>> {
    let d = app.dialog().file();
    let picked = if folder {
        d.set_title("Choose a folder or Prism instance").blocking_pick_folder()
    } else {
        d.set_title("Choose a modpack").add_filter("Modpack", &["mrpack", "zip"]).blocking_pick_file()
    };
    let Some(fp) = picked else { return Ok(None) };
    let p = fp.into_path().map_err(|e| AppError::Invalid(e.to_string()))?;
    blocking(move || templates::scan_source(&p)).await?.map(Some)
}

#[tauri::command]
async fn import_template(state: St<'_>, path: String, name: String, keep: Vec<String>) -> AppResult<Tpl> {
    state.templates.import(std::path::Path::new(&path), &name, &keep, &now())
}

#[tauri::command]
async fn template_overview(state: St<'_>, pack: Pack) -> AppResult<templates::Overview> {
    state.templates.overview(&pack, &now())
}

/// The "reviewed as of now" record a pack stores in `appliedTemplates`.
#[tauri::command]
fn template_applied_ref(state: St<'_>, id: String) -> AppResult<model::AppliedRef> {
    state.templates.applied_ref(&id)
}

/// Template id → names of the packs that use it.
#[tauri::command]
async fn template_uses(state: St<'_>) -> AppResult<std::collections::BTreeMap<String, Vec<String>>> {
    template_uses_impl(&state.db)
}

pub fn template_uses_impl(db: &Db) -> AppResult<std::collections::BTreeMap<String, Vec<String>>> {
    let mut out: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for s in db.list_packs()? {
        if let Some(p) = db.get_pack(&s.id)? {
            for t in p.override_templates {
                out.entry(t).or_default().push(p.name.clone());
            }
        }
    }
    Ok(out)
}

// ---------------- misc ----------------
#[tauri::command]
fn cache_stats(state: St<'_>) -> AppResult<store::CacheStats> {
    state.db.cache_stats()
}

#[tauri::command]
fn clear_cache(state: St<'_>) -> AppResult<store::CacheStats> {
    state.db.cache_clear()?;
    state.db.cache_stats()
}

/// Opens an https link in the user's default browser (the webview never navigates away).
#[tauri::command]
fn open_external(app: tauri::AppHandle, url: String) -> AppResult<()> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| AppError::Invalid("Not a valid link.".into()))?;
    if parsed.scheme() != "https" {
        return Err(AppError::Invalid("Only https links can be opened.".into()));
    }
    app.opener().open_url(parsed.as_str(), None::<&str>).map_err(|e| AppError::Invalid(e.to_string()))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = Db::open(&dir.join("planner.db")).map_err(|e| e.to_string())?;
            payload::seed_starters(&db, &store::now_iso(), false).map_err(|e| e.to_string())?;
            app.manage(AppState::new(db).with_templates(dir.join("templates")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            key_status,
            save_curseforge_key,
            clear_curseforge_key,
            search,
            search_all,
            find_matches,
            match_all,
            project_details,
            categories,
            game_versions,
            loader_versions,
            icon,
            analyze_pack,
            plan_port,
            list_payloads,
            save_payload,
            delete_payload,
            restore_starter_payloads,
            plan_payload,
            payload_from_entries,
            payload_updates,
            export_payload,
            import_payload,
            export_plan,
            export_pack,
            import_pack_file,
            import_prism_instance,
            plan_add,
            plan_removal,
            entry_versions,
            list_packs,
            get_pack,
            save_pack,
            delete_pack,
            export_project,
            import_project,
            pick_icon,
            cache_stats,
            clear_cache,
            open_external,
            list_templates,
            create_template,
            set_template_meta,
            duplicate_template,
            delete_template,
            read_template_file,
            write_template_file,
            delete_template_file,
            rename_template_file,
            add_template_files,
            open_template_path,
            pick_template_source,
            import_template,
            template_overview,
            template_applied_ref,
            template_uses
        ])
        .run(tauri::generate_context!())
        .expect("error while running Modpack Planner");
}

/// Live checks against the real APIs. Not part of the normal test run
/// (resolver tests use recorded responses); run with
/// `cargo test live -- --ignored --nocapture`.
#[cfg(test)]
mod live {
    use super::*;
    use sources::SearchQuery;

    fn state() -> AppState {
        AppState::new(Db::memory())
    }

    #[test]
    #[ignore]
    fn live_modrinth_and_loaders() {
        tauri::async_runtime::block_on(async {
            let st = state();
            let q: SearchQuery = serde_json::from_str(r#"{"text":"sodium","gameVersion":"1.21.1","loader":"neoforge","limit":5}"#).unwrap();
            let page = st.modrinth.search(&q).await.expect("search");
            println!("search: {} total, first = {:?}", page.total, page.hits.first().map(|h| (&h.name, h.compatible)));
            assert!(page.hits.iter().all(|h| h.compatible == Some(true)));
            let again = st.modrinth.search(&q).await.unwrap();
            assert!(again.freshness.unwrap().from_cache, "second search should come from cache");

            let mut loose = q.clone();
            loose.include_incompatible = true;
            loose.loader = Some(Loader::Forge);
            let p2 = st.modrinth.search(&loose).await.unwrap();
            println!("incompatible-included: {:?}", p2.hits.iter().map(|h| (&h.slug, h.compatible)).collect::<Vec<_>>());

            let d = st.modrinth.project("AANobbMI", false).await.expect("project");
            println!("project: {} · {} chars body · loaders {:?}", d.name, d.body.len(), d.loaders);

            let cats = st.modrinth.categories(false).await.unwrap();
            println!("categories: {}", cats.len());
            let gv = st.modrinth.game_versions(false).await.unwrap();
            assert!(gv.iter().any(|v| v.version == "26.1.2"));

            for (l, mc) in [(Loader::Neoforge, "1.21.1"), (Loader::Neoforge, "26.1.2"), (Loader::Fabric, "26.1.2"), (Loader::Quilt, "1.21.1"), (Loader::Forge, "1.21.1"), (Loader::Neoforge, "26.4-snapshot-1")] {
                let v = loaders::loader_versions(&st.fetcher, l, mc, false).await.expect("loader versions");
                println!("{l:?} {mc}: {} versions, newest {:?}, note {:?}", v.versions.len(), v.versions.first(), v.note);
            }

            let icon = st.fetcher.icon_data_url(d.icon_url.as_deref().unwrap()).await.expect("icon");
            println!("icon: {}… ({} chars)", &icon[..30], icon.len());
            assert!(st.fetcher.icon_data_url("https://example.com/x.png").await.is_err());
            // Resolver end to end against the live API.
            let mut pack: Pack = serde_json::from_value(serde_json::json!({
                "id": "live", "name": "Live", "mcVersion": "1.21.1", "loader": "fabric"
            })).unwrap();
            let plan = resolve::plan_add(&pack, "PtjYWJkn", None, None, None, &st.modrinth, false, "t", || uuid::Uuid::new_v4().to_string()).await.expect("plan");
            println!("plan_add sodium-extra: {} additions {:?}, {} suggestions, notes {:?}",
                plan.additions.len(), plan.additions.iter().map(|a| &a.entry.display.name).collect::<Vec<_>>(), plan.suggestions.len(), plan.notes);
            for a in plan.additions { pack.entries.insert(a.key, a.entry); }
            let an = resolve::analyze(&pack, &st.modrinth, false, store::now_secs()).await;
            println!("analyze: complete={} issues={:?}", an.complete, an.issues.iter().map(|i| (i.kind, &i.message)).collect::<Vec<_>>());
            assert!(an.complete);
            assert!(!an.issues.iter().any(|i| i.kind == "missing_dependency"));
            println!("cache: {:?}", st.db.cache_stats().unwrap());
        });
    }
}
