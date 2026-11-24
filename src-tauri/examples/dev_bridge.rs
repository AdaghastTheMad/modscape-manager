//! DEV ONLY: serves the real Rust core over http://127.0.0.1:1421 so the UI
//! can be exercised in a plain browser (e.g. an agent's browser pane) when the
//! desktop window can't be driven. Never shipped: examples aren't bundled.
//!
//!   cargo run --example dev_bridge
//!
//! Then, in the browser console of the Vite page (http://127.0.0.1:1420):
//!   window.__TAURI_INTERNALS__ = { invoke: (cmd, args) => fetch("http://127.0.0.1:1421/invoke",
//!     { method: "POST", body: JSON.stringify({ cmd, args }) }).then(async r => { const j = await r.json(); if (!r.ok) throw j; return j; }) };
//!
//! Packs live in memory; keychain, dialogs and the opener are not available.

use modpack_planner_lib::{model, resolve, store, AppState};
use serde_json::{json, Value};

fn arg<T: serde::de::DeserializeOwned>(args: &Value, k: &str) -> Result<T, Value> {
    serde_json::from_value(args.get(k).cloned().unwrap_or(Value::Null)).map_err(|e| json!({"code": "invalid", "message": format!("bad arg {k}: {e}")}))
}

fn to_val<T: serde::Serialize, E: serde::Serialize>(r: Result<T, E>) -> Result<Value, Value> {
    r.map(|v| serde_json::to_value(v).unwrap()).map_err(|e| serde_json::to_value(e).unwrap())
}

async fn dispatch(st: &AppState, cmd: &str, a: &Value) -> Result<Value, Value> {
    use resolve::Catalog;
    let force = a.get("force").and_then(Value::as_bool).unwrap_or(false);
    match cmd {
        "app_info" => Ok(json!({"version": "dev-bridge", "userAgent": modpack_planner_lib::USER_AGENT, "os": std::env::consts::OS, "allowedHosts": []})),
        "key_status" => Ok(json!({"keychainAvailable": false, "hasKey": false, "backend": "dev bridge", "problem": {"message": "No keychain in the dev bridge", "help": ""}})),
        "search" => to_val(st.source(arg(a, "source")?).search(&arg(a, "query")?).await),
        "search_all" => to_val(
            modpack_planner_lib::search_all_impl(st, arg(a, "query")?, arg(a, "sources")?, arg(a, "prefer")?, arg(a, "mrOffset")?, arg(a, "cfOffset")?).await,
        ),
        "match_all" => to_val(modpack_planner_lib::match_all_impl(st, &arg(a, "pack")?).await),
        "find_matches" => {
            let pack: model::Pack = arg(a, "pack")?;
            let key: String = arg(a, "key")?;
            let e = pack.entries.get(&key).ok_or(json!({"code": "invalid", "message": "no such entry"}))?;
            to_val(modpack_planner_lib::sources::matching::find_counterparts(&st.modrinth, &st.curseforge, e, &pack.mc_version, pack.loader).await)
        }
        "project_details" => match arg::<modpack_planner_lib::sources::Source>(a, "source")? {
            modpack_planner_lib::sources::Source::Modrinth => to_val(st.modrinth.project(&arg::<String>(a, "id")?, force).await),
            modpack_planner_lib::sources::Source::Curseforge => {
                let id: String = arg(a, "id")?;
                to_val(st.curseforge.project(id.trim_start_matches("cf:").parse().unwrap_or(0), force).await)
            }
        },
        "categories" => to_val(st.modrinth.categories(force).await),
        "game_versions" => to_val(st.modrinth.game_versions(force).await),
        "loader_versions" => to_val(modpack_planner_lib::loaders::loader_versions(&st.fetcher, arg(a, "loader")?, &arg::<String>(a, "mcVersion")?, force).await),
        "icon" => to_val(st.fetcher.icon_data_url(&arg::<String>(a, "url")?).await),
        "list_packs" => to_val(st.db.list_packs()),
        "get_pack" => match st.db.get_pack(&arg::<String>(a, "id")?) {
            Ok(Some(p)) => Ok(serde_json::to_value(p).unwrap()),
            Ok(None) => Err(json!({"code": "invalid", "message": "That pack no longer exists."})),
            Err(e) => Err(serde_json::to_value(e).unwrap()),
        },
        "save_pack" => {
            let mut pack: model::Pack = arg(a, "pack")?;
            let notes = to_val(model::normalize(&mut pack))?;
            to_val(st.db.put_pack(&pack))?;
            Ok(json!({"pack": pack, "notes": notes}))
        }
        "delete_pack" => to_val(st.db.delete_pack(&arg::<String>(a, "id")?)),
        "analyze_pack" => Ok(serde_json::to_value(resolve::analyze(&arg(a, "pack")?, &st.catalog(), force, store::now_secs()).await).unwrap()),
        "plan_add" => {
            let pack: model::Pack = arg(a, "pack")?;
            let pin: Option<String> = arg(a, "pinVersion")?;
            let for_key: Option<String> = arg(a, "forKey")?;
            let alt: Option<String> = arg(a, "altId")?;
            to_val(resolve::plan_add(&pack, &arg::<String>(a, "projectId")?, pin.as_deref(), for_key.as_deref(), alt.as_deref(), &st.catalog(), false, &arg::<String>(a, "addedAt")?, || uuid::Uuid::new_v4().to_string()).await)
        }
        "export_plan" => Ok(serde_json::to_value(st.export_plan(&arg(a, "pack")?, arg(a, "format")?).await).unwrap()),
        // Dev only: write an export / import a file at a path inside test-env/.
        "dev_export_to" => {
            let pack: model::Pack = arg(a, "pack")?;
            let path = test_env_path(&arg::<String>(a, "path")?)?;
            let plan = st.export_plan(&pack, arg(a, "format")?).await;
            let overrides = st.templates.export_files(&pack).map_err(|e| serde_json::to_value(e).unwrap())?;
            to_val(modpack_planner_lib::export::write_archive(&pack, &plan, &overrides, &path).map(|_| path.display().to_string()))
        }
        "dev_import_from" => to_val(modpack_planner_lib::import_file_impl(st, &test_env_path(&arg::<String>(a, "path")?)?).await),
        "dev_import_instance" => to_val(modpack_planner_lib::import_instance_impl(st, &test_env_path(&arg::<String>(a, "path")?)?).await),
        "plan_port" => to_val(
            modpack_planner_lib::port::plan_port(&arg(a, "pack")?, &arg(a, "target")?, &st.catalog(), &store::now_iso(), || uuid::Uuid::new_v4().to_string()).await,
        ),
        "list_payloads" => to_val(st.db.all_payloads()),
        "save_payload" => to_val(modpack_planner_lib::save_payload_impl(st, arg(a, "payload")?)),
        "delete_payload" => to_val(st.db.delete_payload(&arg::<String>(a, "id")?)),
        "restore_starter_payloads" => to_val(modpack_planner_lib::payload::seed_starters(&st.db, &store::now_iso(), true)),
        "plan_payload" => {
            let ch: Option<std::collections::HashMap<String, String>> = arg(a, "choices")?;
            to_val(modpack_planner_lib::plan_payload_impl(st, &arg(a, "pack")?, &arg::<String>(a, "payloadId")?, ch.unwrap_or_default()).await)
        }
        "payload_from_entries" => to_val(
            modpack_planner_lib::payload::from_entries(&arg(a, "pack")?, &arg::<Vec<String>>(a, "keys")?, &arg::<String>(a, "name")?, &store::now_iso())
                .and_then(|p| modpack_planner_lib::save_payload_impl(st, p)),
        ),
        "payload_updates" => {
            let all = st.db.all_payloads().map_err(|e| serde_json::to_value(e).unwrap())?.into_iter().map(|p| (p.id.clone(), p)).collect();
            Ok(serde_json::to_value(modpack_planner_lib::payload::updates_for(&arg(a, "pack")?, &all)).unwrap())
        }
        "plan_removal" => Ok(serde_json::to_value(model::plan_removal(&arg(a, "pack")?, &arg::<Vec<String>>(a, "keys")?)).unwrap()),
        "entry_versions" => {
            let (mc, loader): (String, _) = (arg(a, "mcVersion")?, arg(a, "loader")?);
            let vs = st.catalog().versions(&arg::<String>(a, "projectId")?, &mc, loader, false).await.map_err(|e| serde_json::to_value(e).unwrap())?;
            Ok(serde_json::to_value(resolve::compatible_files(&vs, &mc, loader)).unwrap())
        }
        "cache_stats" => to_val(st.db.cache_stats()),
        // Override templates (library lives in test-env/bridge-templates; source paths are relative to test-env/).
        "list_templates" => to_val(st.templates.list(&store::now_iso())),
        "create_template" => to_val(st.templates.create(&arg::<String>(a, "name")?, "", &store::now_iso())),
        "set_template_meta" => to_val(st.templates.set_meta(&arg::<String>(a, "id")?, &arg::<String>(a, "name")?, &arg::<String>(a, "description")?, &store::now_iso())),
        "duplicate_template" => to_val(st.templates.duplicate(&arg::<String>(a, "id")?, &store::now_iso())),
        "delete_template" => to_val(st.templates.delete(&arg::<String>(a, "id")?)),
        "read_template_file" => to_val(st.templates.read_file(&arg::<String>(a, "id")?, &arg::<String>(a, "path")?)),
        "write_template_file" => to_val(st.templates.write_file(&arg::<String>(a, "id")?, &arg::<String>(a, "path")?, &arg::<String>(a, "text")?, &store::now_iso())),
        "delete_template_file" => to_val(st.templates.delete_file(&arg::<String>(a, "id")?, &arg::<String>(a, "path")?, &store::now_iso())),
        "rename_template_file" => to_val(st.templates.rename_file(&arg::<String>(a, "id")?, &arg::<String>(a, "from")?, &arg::<String>(a, "to")?, &store::now_iso())),
        "scan_template_source" => to_val(modpack_planner_lib::templates::scan_source(&test_env_path(&arg::<String>(a, "path")?)?)),
        "import_template" => {
            let p = std::path::PathBuf::from(arg::<String>(a, "path")?);
            let root = test_env_path("")?;
            if !p.starts_with(&root) {
                return Err(json!({"code": "invalid", "message": "dev bridge imports only from test-env/"}));
            }
            to_val(st.templates.import(&p, &arg::<String>(a, "name")?, &arg::<Vec<String>>(a, "keep")?, &store::now_iso()))
        }
        "template_overview" => to_val(st.templates.overview(&arg(a, "pack")?, &store::now_iso())),
        "template_applied_ref" => to_val(st.templates.applied_ref(&arg::<String>(a, "id")?)),
        "template_uses" => to_val(modpack_planner_lib::template_uses_impl(&st.db)),
        "pick_template_source" | "add_template_files" | "open_template_path" => Ok(Value::Null),
        "open_external" | "pick_icon" | "export_project" | "import_project" => Ok(Value::Null),
        other => Err(json!({"code": "invalid", "message": format!("dev bridge doesn't support {other}")})),
    }
}

/// The bridge only reads and writes inside the project's test-env/ folder.
fn test_env_path(rel: &str) -> Result<std::path::PathBuf, Value> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("test-env");
    let p = root.join(rel);
    let ok = !rel.contains("..") && p.starts_with(&root);
    if !ok {
        return Err(json!({"code": "invalid", "message": "dev bridge paths must stay inside test-env/"}));
    }
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    Ok(p)
}

fn main() {
    let st = AppState::new(store::Db::memory()).with_templates(test_env_path("bridge-templates/x").unwrap().parent().unwrap().to_path_buf());
    modpack_planner_lib::payload::seed_starters(&st.db, &store::now_iso(), false).unwrap();
    // Never touch the OS keychain from a dev tool: CurseForge stays off here.
    st.curseforge.disable_keychain();
    let server = tiny_http::Server::http("127.0.0.1:1421").expect("bind 127.0.0.1:1421");
    println!("dev bridge listening on http://127.0.0.1:1421");
    let cors = |r: tiny_http::Response<std::io::Cursor<Vec<u8>>>| {
        r.with_header("Access-Control-Allow-Origin: http://127.0.0.1:1420".parse::<tiny_http::Header>().unwrap())
            .with_header("Access-Control-Allow-Headers: content-type".parse::<tiny_http::Header>().unwrap())
            .with_header("Content-Type: application/json".parse::<tiny_http::Header>().unwrap())
    };
    for mut req in server.incoming_requests() {
        if req.method() == &tiny_http::Method::Options {
            let _ = req.respond(cors(tiny_http::Response::from_data(Vec::new())));
            continue;
        }
        let mut body = String::new();
        let _ = req.as_reader().read_to_string(&mut body);
        let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        let cmd = parsed.get("cmd").and_then(Value::as_str).unwrap_or("").to_string();
        let args = parsed.get("args").cloned().unwrap_or(json!({}));
        let t = std::time::Instant::now();
        let result = tauri::async_runtime::block_on(dispatch(&st, &cmd, &args));
        println!("{cmd} {} in {:?}", if result.is_ok() { "ok" } else { "ERR" }, t.elapsed());
        let (code, v) = match result { Ok(v) => (200, v), Err(e) => (400, e) };
        let _ = req.respond(cors(tiny_http::Response::from_data(serde_json::to_vec(&v).unwrap()).with_status_code(code)));
    }
}
