//! Live Scan from the command line, on a folder of jars (dev tool):
//!   cargo run --example scan_folder -- <mods folder> <mc version> <neoforge|fabric|forge|quilt> [loader version]
use modscape_manager_lib::{model, scan, sources::Loader};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: scan_folder <mods folder> <mc version> <loader> [loader version]");
        std::process::exit(2);
    }
    let loader: Loader = serde_json::from_value(serde_json::json!(a[2])).expect("loader");
    let mut pack: model::Pack = serde_json::from_value(serde_json::json!({
        "id": "scan", "name": "scan", "mcVersion": a[1], "loader": loader, "loaderVersion": a.get(3).cloned().unwrap_or_default()
    })).unwrap();
    pack.entries.clear();
    let jars = scan::read_folder(std::path::Path::new(&a[0])).expect("folder");
    let r = scan::analyse(&pack, &jars, &Default::default());
    println!("{} jars, {} mod ids, {} issues", r.jars, r.mod_ids, r.issues.len());
    for i in &r.issues {
        println!("  {:?} {}: {}", i.severity, i.kind, i.message);
    }
}
