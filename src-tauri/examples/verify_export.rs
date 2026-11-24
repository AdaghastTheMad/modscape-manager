//! Downloads every file an exported .mrpack lists, like a launcher would,
//! into test-env/downloads/<pack>/ and checks size, SHA-1 and SHA-512.
//! Only fetches from the hosts the .mrpack spec allows.
//!
//!   cd src-tauri && cargo run --example verify_export -- ../test-env/exports/neoforge-1.21.1.mrpack

use sha1::Digest;
use std::io::Read;

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn main() {
    let path = std::env::args().nth(1).expect("usage: verify_export FILE.mrpack");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("test-env");
    let bytes = std::fs::read(&path).expect("read pack");
    let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut s = String::new();
    z.by_name("modrinth.index.json").expect("index").read_to_string(&mut s).unwrap();
    let idx: serde_json::Value = serde_json::from_str(&s).unwrap();
    let stem = std::path::Path::new(&path).file_stem().unwrap().to_string_lossy().to_string();
    let dest = root.join("downloads").join(&stem);
    std::fs::create_dir_all(dest.join("mods")).unwrap();
    let http = reqwest::blocking::Client::builder().user_agent(modpack_planner_lib::USER_AGENT).timeout(std::time::Duration::from_secs(600)).build().unwrap();
    let files = idx["files"].as_array().unwrap();
    let (mut ok, mut bad) = (0, Vec::new());
    for f in files {
        let rel = f["path"].as_str().unwrap();
        let url = f["downloads"][0].as_str().unwrap();
        let host_ok = reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(|h| modpack_planner_lib::export::MRPACK_HOSTS.contains(&h))).unwrap_or(false);
        if !host_ok || rel.contains("..") {
            bad.push(format!("{rel}: refused URL {url}"));
            continue;
        }
        let out = dest.join(rel);
        if let Some(parent) = out.parent() { std::fs::create_dir_all(parent).unwrap(); }
        let data = if out.exists() { std::fs::read(&out).unwrap() } else {
            match http.get(url).send().and_then(|r| r.error_for_status()).and_then(|r| r.bytes()) {
                Ok(b) => { std::fs::write(&out, &b).unwrap(); b.to_vec() }
                Err(e) => { bad.push(format!("{rel}: download failed: {e}")); continue; }
            }
        };
        let sha1 = hex(&sha1::Sha1::digest(&data));
        let sha512 = hex(&sha2::Sha512::digest(&data));
        let size_ok = f["fileSize"].as_u64() == Some(data.len() as u64);
        if sha1 == f["hashes"]["sha1"].as_str().unwrap_or("") && sha512 == f["hashes"]["sha512"].as_str().unwrap_or("") && size_ok {
            ok += 1;
        } else {
            bad.push(format!("{rel}: hash/size mismatch"));
        }
    }
    println!("{stem}: {ok}/{} files downloaded and verified → {}", files.len(), dest.display());
    println!("dependencies: {}", idx["dependencies"]);
    for b in &bad {
        println!("  ✗ {b}");
    }
    std::process::exit(if bad.is_empty() { 0 } else { 1 });
}
