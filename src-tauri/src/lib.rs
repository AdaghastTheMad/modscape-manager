//! Modpack Planner core. Every network call, all storage and all secrets live
//! here; the webview only talks to these typed commands.

pub mod error;
pub mod fetch;
pub mod keychain;
pub mod loaders;
pub mod model;
pub mod resolve;
pub mod sources;
pub mod store;

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
}

impl AppState {
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
        Self { modrinth: Modrinth::new(fetcher.clone()), curseforge: CurseForge::new(fetcher.clone()), db, fetcher }
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
async fn save_curseforge_key(key: String) -> AppResult<keychain::KeyStatus> {
    let key = keychain::validate(&key)?;
    blocking(move || keychain::set_key(&key)).await??;
    blocking(keychain::status).await
}

#[tauri::command]
async fn clear_curseforge_key() -> AppResult<keychain::KeyStatus> {
    blocking(keychain::clear_key).await??;
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
        Source::Curseforge => Err(AppError::Invalid("CurseForge mod pages arrive with full CurseForge support (milestone 4).".into())),
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

// ---------------- resolver ----------------
#[tauri::command]
async fn analyze_pack(state: St<'_>, pack: Pack, force: Option<bool>) -> AppResult<resolve::Analysis> {
    Ok(resolve::analyze(&pack, &state.modrinth, force.unwrap_or(false), store::now_secs()).await)
}

#[tauri::command]
async fn plan_add(state: St<'_>, pack: Pack, project_id: String, pin_version: Option<String>, for_key: Option<String>, added_at: String) -> AppResult<resolve::AddPlan> {
    resolve::plan_add(&pack, &project_id, pin_version.as_deref(), for_key.as_deref(), &state.modrinth, false, &added_at, || uuid::Uuid::new_v4().to_string()).await
}

#[tauri::command]
fn plan_removal(pack: Pack, keys: Vec<String>) -> Vec<String> {
    model::plan_removal(&pack, &keys)
}

#[tauri::command]
async fn entry_versions(state: St<'_>, project_id: String, mc_version: String, loader: Loader) -> AppResult<Vec<resolve::FileInfo>> {
    use resolve::Catalog;
    let vs = state.modrinth.versions(&project_id, &mc_version, loader, false).await?;
    Ok(resolve::compatible_files(&vs, &mc_version, loader))
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
            app.manage(AppState::new(db));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            key_status,
            save_curseforge_key,
            clear_curseforge_key,
            search,
            project_details,
            categories,
            game_versions,
            loader_versions,
            icon,
            analyze_pack,
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
            open_external
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
            let plan = resolve::plan_add(&pack, "PtjYWJkn", None, None, &st.modrinth, false, "t", || uuid::Uuid::new_v4().to_string()).await.expect("plan");
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
