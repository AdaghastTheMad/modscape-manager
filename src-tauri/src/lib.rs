//! Modpack Planner core. Every network call, all storage and all secrets live
//! here; the webview only talks to these typed commands.

mod error;
mod keychain;
mod sources;

use error::{AppError, AppResult};
use sources::{curseforge::CurseForge, modrinth::Modrinth, ModSource, SearchPage, SearchQuery, Source};

/// Generic, identifiable User-Agent (Modrinth asks for one). No personal
/// contact details: add the public project URL here once one exists,
/// e.g. "modpack-planner/0.1.0 (+https://github.com/…)".
pub const USER_AGENT: &str = concat!("modpack-planner/", env!("CARGO_PKG_VERSION"));

pub struct AppState {
    modrinth: Modrinth,
    curseforge: CurseForge,
}

impl AppState {
    fn new() -> Self {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(20))
            .connect_timeout(std::time::Duration::from_secs(10))
            .https_only(true)
            .build()
            .expect("HTTP client");
        Self { modrinth: Modrinth::new(http.clone()), curseforge: CurseForge::new(http) }
    }

    fn source(&self, s: Source) -> &dyn ModSource {
        match s {
            Source::Modrinth => &self.modrinth,
            Source::Curseforge => &self.curseforge,
        }
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| AppError::Invalid(e.to_string()))
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

#[tauri::command]
async fn search(state: tauri::State<'_, AppState>, source: Source, query: SearchQuery) -> AppResult<SearchPage> {
    state.source(source).search(&query).await
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    user_agent: &'static str,
    os: &'static str,
}

#[tauri::command]
fn app_info() -> AppInfo {
    AppInfo { version: env!("CARGO_PKG_VERSION"), user_agent: USER_AGENT, os: std::env::consts::OS }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            app_info,
            key_status,
            save_curseforge_key,
            clear_curseforge_key,
            search
        ])
        .run(tauri::generate_context!())
        .expect("error while running Modpack Planner");
}
