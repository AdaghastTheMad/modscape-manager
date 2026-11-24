//! Builds the two fixture packs the design doc asks for, from live Modrinth
//! data, through the same code the app uses (search → plan_add → checks):
//!   - NeoForge 1.21.1, ~150 mods
//!   - Fabric 26.1.2, ~50 mods
//! Mods the checks flag (incompatible, no build, missing deps) are removed
//! until the pack is clean. Writes planner projects to
//! tests/fixtures/packs/ and .mrpack exports to test-env/exports/.
//! Modrinth only: never reads the keychain.
//!
//!   cd src-tauri && cargo run --example build_fixture_packs [-- neoforge-1.21.1|fabric-26.1.2]

use modpack_planner_lib::resolve::{self, Fix, Severity};
use modpack_planner_lib::sources::{Loader, ModSource, SearchQuery, Source};
use modpack_planner_lib::{export, loaders, model, store, AppState};

fn main() {
    let st = AppState::new(store::Db::memory());
    st.curseforge.disable_keychain();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let packs_dir = root.join("tests/fixtures/packs");
    let exports = root.parent().unwrap().join("test-env/exports");
    std::fs::create_dir_all(&packs_dir).unwrap();
    std::fs::create_dir_all(&exports).unwrap();
    for (name, file, mc, loader, target) in [
        ("Fixture NeoForge 1.21.1", "neoforge-1.21.1", "1.21.1", Loader::Neoforge, 150usize),
        ("Fixture Fabric 26.1.2", "fabric-26.1.2", "26.1.2", Loader::Fabric, 50usize),
    ] {
        // Optional filter: `cargo run --example build_fixture_packs -- neoforge-1.21.1`
        if let Some(only) = std::env::args().skip(1).find(|a| !a.starts_with("--")) {
            if only != file {
                continue;
            }
        }
        // `--export-only`: re-export the saved fixture instead of rebuilding it.
        let saved = packs_dir.join(format!("{file}.planner.json"));
        let pack = if std::env::args().any(|a| a == "--export-only") && saved.exists() {
            model::parse_project(&std::fs::read_to_string(&saved).unwrap()).unwrap().0
        } else {
            tauri::async_runtime::block_on(build(&st, name, mc, loader, target))
        };
        let json = model::to_project_json(&pack, &store::now_iso()).unwrap();
        std::fs::write(packs_dir.join(format!("{file}.planner.json")), json).unwrap();
        let plan = tauri::async_runtime::block_on(export::plan_export(&pack, &st.catalog(), export::Format::Mrpack));
        let out = exports.join(format!("{file}.mrpack"));
        match export::write_archive(&pack, &plan, &out) {
            Ok(()) => println!("{name}: {} mods → {} (excluded {})", pack.entries.len(), out.display(), plan.excluded.len()),
            Err(e) => println!("{name}: export failed: {e} {:?}", plan.problems.iter().map(|p| &p.message).collect::<Vec<_>>()),
        }
    }
}

async fn build(st: &AppState, name: &str, mc: &str, loader: Loader, target: usize) -> model::Pack {
    let lv = loaders::loader_versions(&st.fetcher, loader, mc, false).await.unwrap();
    let loader_version = lv.versions.iter().find(|v| v.stable).or(lv.versions.first()).map(|v| v.version.clone()).unwrap_or_default();
    let mut pack: model::Pack = serde_json::from_value(serde_json::json!({
        "id": format!("fixture-{mc}-{loader:?}").to_lowercase(), "name": name, "mcVersion": mc, "loader": loader,
        "loaderVersion": loader_version, "createdAt": store::now_iso(), "updatedAt": store::now_iso()
    }))
    .unwrap();
    let cat = st.catalog();
    let mut banned = std::collections::HashSet::new();
    let mut offset = 0;
    let mut n = 0u32;
    while pack.entries.len() < target && offset < 1000 {
        let q: SearchQuery = serde_json::from_value(serde_json::json!({
            "text": "", "gameVersion": mc, "loader": loader, "sort": "downloads", "limit": 50, "offset": offset
        }))
        .unwrap();
        let page = st.modrinth.search(&q).await.unwrap();
        if page.hits.is_empty() {
            break;
        }
        offset += 50;
        for h in page.hits {
            if pack.entries.len() >= target {
                break;
            }
            if banned.contains(&h.id) || pack.entries.values().any(|e| e.mod_ref.modrinth_id.as_deref() == Some(&h.id)) {
                continue;
            }
            let plan = match resolve::plan_add(&pack, &h.id, None, None, None, &cat, false, &store::now_iso(), || {
                n += 1;
                format!("e{n:04}")
            })
            .await
            {
                Ok(p) => p,
                Err(_) => continue,
            };
            if !plan.notes.is_empty() {
                banned.insert(h.id.clone()); // no build / deleted deps: skip
                continue;
            }
            let mut trial = pack.clone();
            for a in plan.additions {
                trial.entries.insert(a.key, a.entry);
            }
            for l in plan.links {
                if let Some(e) = trial.entries.get_mut(&l.key) {
                    e.pulled_in_by.push(l.parent);
                }
            }
            // Keep the add only if the pack stays free of errors.
            let a = resolve::analyze(&trial, &cat, false, store::now_secs()).await;
            if a.issues.iter().any(|i| i.severity == Severity::Error) {
                banned.insert(h.id.clone());
                continue;
            }
            pack = trial;
            println!("  {name}: {} mods (+{})", pack.entries.len(), h.name);
        }
    }
    // Final tidy: drop anything still flagged (defensive; adds were checked one by one).
    for _ in 0..5 {
        let a = resolve::analyze(&pack, &cat, false, store::now_secs()).await;
        let bad: Vec<String> = a
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .filter_map(|i| match &i.fix {
                Some(Fix::Remove { key }) => Some(key.clone()),
                _ => i.entry_key.clone(),
            })
            .collect();
        if bad.is_empty() {
            break;
        }
        for k in bad {
            pack.entries.remove(&k);
        }
        let _ = model::normalize(&mut pack);
    }
    let _ = Source::Modrinth;
    pack
}
