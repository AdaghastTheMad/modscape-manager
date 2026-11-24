//! Records real CurseForge responses for the resolver tests, in the same
//! bundle format as `tests/fixtures/resolver/modrinth.json`.
//!
//!   cd src-tauri && cargo run --example record_cf_fixtures
//!
//! Uses the CurseForge key saved in the app (macOS may ask whether this tool
//! may read it: that's your call), or CURSEFORGE_API_KEY if set. Only talks to
//! api.curseforge.com. Writes tests/fixtures/resolver/curseforge.json.

use modpack_planner_lib::resolve::Catalog;
use modpack_planner_lib::sources::{curseforge::cf_id, Loader};
use modpack_planner_lib::{store, AppState};
use serde_json::{json, Map, Value};

// (CurseForge mod id, label) per scenario; required deps are followed one hop.
const SCENARIOS: &[(&str, Loader, &[(u64, &str)])] = &[
    ("1.21.1", Loader::Neoforge, &[(238222, "jei"), (419699, "architectury"), (348521, "cloth-config"), (328085, "create"), (248787, "appleskin")]),
    ("1.21.1", Loader::Fabric, &[(394468, "sodium"), (238222, "jei"), (306612, "fabric-api"), (348521, "cloth-config")]),
    ("26.1.2", Loader::Neoforge, &[(238222, "jei"), (419699, "architectury")]),
];

fn main() {
    let st = AppState::new(store::Db::memory());
    if let Ok(k) = std::env::var("CURSEFORGE_API_KEY") {
        st.curseforge.set_key_for_tools(k);
    }
    let cat = &st.curseforge;
    let mut versions = Map::new();
    let mut projects = Map::new();
    tauri::async_runtime::block_on(async {
        for (mc, loader, mods) in SCENARIOS {
            let mut todo: Vec<u64> = mods.iter().map(|(id, _)| *id).collect();
            let mut seen = std::collections::HashSet::new();
            while let Some(id) = todo.pop() {
                if !seen.insert(id) {
                    continue;
                }
                let pid = cf_id(id);
                let vs = match cat.versions(&pid, mc, *loader, false).await {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("  {pid} {mc}: {e}");
                        if matches!(e, modpack_planner_lib::error::AppError::NoCurseForgeKey | modpack_planner_lib::error::AppError::Api { status: 401 | 403, .. }) {
                            std::process::exit(1);
                        }
                        continue;
                    }
                };
                println!("  {mc} {loader:?} {pid}: {} files", vs.len());
                for v in vs.iter().take(3) {
                    for d in &v.dependencies {
                        if d.dependency_type == modpack_planner_lib::resolve::DepType::Required {
                            if let Some(n) = d.project_id.as_deref().and_then(modpack_planner_lib::sources::curseforge::parse_id) {
                                todo.push(n);
                            }
                        }
                    }
                }
                let key = format!("{pid}|{mc}");
                let mut list: Vec<Value> = versions.get(&key).and_then(|v| v.as_array().cloned()).unwrap_or_default();
                list.extend(vs.iter().take(40).map(|v| serde_json::to_value(v).unwrap()));
                versions.insert(key, Value::Array(list));
            }
            let ids: Vec<String> = seen.iter().map(|n| cf_id(*n)).collect();
            for p in cat.projects(&ids, false).await.unwrap_or_default() {
                projects.insert(p.id.clone(), json!({
                    "id": p.id, "slug": p.slug, "title": p.title, "description": p.description, "icon_url": p.icon_url,
                    "categories": p.categories, "downloads": p.downloads, "updated": p.updated
                }));
            }
        }
    });
    let out = json!({
        "_comment": "Recorded from api.curseforge.com by examples/record_cf_fixtures.rs.",
        "recordedAt": format!("{}", store::now_secs()),
        "versions": versions, "versionById": {}, "projects": projects
    });
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/resolver/curseforge.json");
    std::fs::write(&path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    println!("wrote {}", path.display());
}
