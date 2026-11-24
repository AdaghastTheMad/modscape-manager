//! Pack data model (see CLAUDE.md "Data model (revised)"). Every struct keeps
//! unknown fields in `extra`, so files written by a newer build survive a
//! round trip through an older one instead of silently losing data.

use crate::error::{AppError, AppResult};
use crate::sources::{Loader, Source};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};

pub const PROJECT_FORMAT: &str = "modpack-planner-project";
pub const PROJECT_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SourcePreference {
    #[default]
    Modrinth,
    Curseforge,
    Mixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    #[default]
    Mod,
    Resourcepack,
    Shader,
    Datapack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Reason {
    #[default]
    User,
    Dependency,
    Payload,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModRef {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modrinth_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curseforge_id: Option<u64>,
    pub preferred: Source,
}

impl ModRef {
    fn has_preferred(&self) -> bool {
        match self.preferred {
            Source::Modrinth => self.modrinth_id.is_some(),
            Source::Curseforge => self.curseforge_id.is_some(),
        }
    }
}

/// Cached display info so a pack renders instantly and offline.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct EntryDisplay {
    pub name: String,
    pub slug: String,
    pub summary: String,
    pub icon_url: Option<String>,
    pub categories: Vec<String>,
    pub client_side: Option<String>,
    pub server_side: Option<String>,
    pub license: Option<String>,
    pub updated: Option<String>,
    pub downloads: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphPos {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PackEntry {
    #[serde(rename = "ref")]
    pub mod_ref: ModRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<String>,
    #[serde(default)]
    pub kind: EntryKind,
    #[serde(default)]
    pub reason: Reason,
    #[serde(default)]
    pub pulled_in_by: Vec<String>,
    #[serde(default)]
    pub payload_ids: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_pos: Option<GraphPos>,
    #[serde(default)]
    pub display: EntryDisplay,
    /// User override for library dimming; None = decide from metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library_override: Option<bool>,
    #[serde(default)]
    pub added_at: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AppliedRef {
    pub id: String,
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub mc_version: String,
    pub loader: Loader,
    #[serde(default)]
    pub loader_version: String,
    #[serde(default)]
    pub source_preference: SourcePreference,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub entries: BTreeMap<String, PackEntry>,
    #[serde(default)]
    pub choices: BTreeMap<String, Value>,
    #[serde(default)]
    pub applied_payloads: Vec<AppliedRef>,
    #[serde(default)]
    pub override_templates: Vec<String>,
    #[serde(default)]
    pub applied_templates: Vec<AppliedRef>,
    #[serde(default)]
    pub snapshots: Vec<Value>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub format: String,
    pub schema_version: u32,
    #[serde(default)]
    pub app_version: String,
    #[serde(default)]
    pub exported_at: String,
    pub pack: Pack,
}

pub const MAX_ICON_BYTES: usize = 512 * 1024;

/// Checks a pack is usable and repairs what can be repaired safely.
/// Returns human-readable notes about anything repaired.
pub fn normalize(pack: &mut Pack) -> AppResult<Vec<String>> {
    let mut notes = Vec::new();
    pack.name = pack.name.trim().to_string();
    if pack.name.is_empty() {
        return Err(AppError::Invalid("The pack needs a name.".into()));
    }
    if pack.name.chars().count() > 120 {
        return Err(AppError::Invalid("Pack names are limited to 120 characters.".into()));
    }
    pack.mc_version = pack.mc_version.trim().to_string();
    if pack.mc_version.is_empty() {
        return Err(AppError::Invalid("The pack needs a Minecraft version.".into()));
    }
    if pack.id.trim().is_empty() {
        return Err(AppError::Invalid("The pack is missing its id.".into()));
    }
    if let Some(icon) = &pack.icon {
        if !icon.starts_with("data:image/") || icon.len() > MAX_ICON_BYTES * 4 / 3 + 64 {
            pack.icon = None;
            notes.push("Dropped an invalid or oversized pack icon.".into());
        }
    }

    // Entries: drop ones with no usable id, fix a preferred source that has no id.
    let bad: Vec<String> = pack
        .entries
        .iter()
        .filter(|(k, e)| k.trim().is_empty() || (e.mod_ref.modrinth_id.is_none() && e.mod_ref.curseforge_id.is_none()))
        .map(|(k, _)| k.clone())
        .collect();
    for k in bad {
        let name = pack.entries.remove(&k).map(|e| e.display.name).unwrap_or_default();
        notes.push(format!("Removed an entry with no Modrinth or CurseForge id ({}).", if name.is_empty() { k } else { name }));
    }
    for e in pack.entries.values_mut() {
        if !e.mod_ref.has_preferred() {
            e.mod_ref.preferred = if e.mod_ref.modrinth_id.is_some() { Source::Modrinth } else { Source::Curseforge };
        }
    }

    // Duplicates (same Modrinth or CurseForge project twice): keep the earliest-added.
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut order: Vec<(&String, &PackEntry)> = pack.entries.iter().collect();
    order.sort_by(|a, b| a.1.added_at.cmp(&b.1.added_at).then(a.0.cmp(b.0)));
    let mut dupes = Vec::new();
    for (k, e) in order {
        let ids = [
            e.mod_ref.modrinth_id.as_ref().map(|i| format!("mr:{i}")),
            e.mod_ref.curseforge_id.map(|i| format!("cf:{i}")),
        ];
        if ids.iter().flatten().any(|id| seen.contains_key(id)) {
            dupes.push(k.clone());
        } else {
            for id in ids.into_iter().flatten() {
                seen.insert(id, k.clone());
            }
        }
    }
    for k in dupes {
        if let Some(e) = pack.entries.remove(&k) {
            notes.push(format!("Removed a duplicate of {}.", if e.display.name.is_empty() { k } else { e.display.name }));
        }
    }

    // Dangling pulled-in-by links.
    let keys: std::collections::HashSet<String> = pack.entries.keys().cloned().collect();
    let mut pruned = 0;
    for e in pack.entries.values_mut() {
        let before = e.pulled_in_by.len();
        e.pulled_in_by.retain(|k| keys.contains(k));
        e.pulled_in_by.dedup();
        pruned += before - e.pulled_in_by.len();
    }
    if pruned > 0 {
        notes.push(format!("Cleared {pruned} link(s) to mods no longer in the pack."));
    }
    Ok(notes)
}

pub fn parse_project(text: &str) -> AppResult<(Pack, Vec<String>)> {
    let v: Value = serde_json::from_str(text).map_err(|e| AppError::Invalid(format!("That file isn't valid JSON: {e}")))?;
    if v.get("format").and_then(Value::as_str) != Some(PROJECT_FORMAT) {
        return Err(AppError::Invalid(
            "That JSON file isn't a Modpack Planner project. (Pack formats like .mrpack are imported separately, in a later version.)".into(),
        ));
    }
    let schema = v.get("schemaVersion").and_then(Value::as_u64).unwrap_or(0) as u32;
    if schema > PROJECT_SCHEMA {
        return Err(AppError::Invalid(format!(
            "This project was saved by a newer Modpack Planner (format v{schema}; this build reads up to v{PROJECT_SCHEMA}). Update the app to open it."
        )));
    }
    let file: ProjectFile = serde_json::from_value(v).map_err(|e| AppError::Invalid(format!("The project file is damaged or incomplete: {e}")))?;
    let mut pack = file.pack;
    let notes = normalize(&mut pack)?;
    Ok((pack, notes))
}

pub fn to_project_json(pack: &Pack, exported_at: &str) -> AppResult<String> {
    let file = ProjectFile {
        format: PROJECT_FORMAT.into(),
        schema_version: PROJECT_SCHEMA,
        app_version: env!("CARGO_PKG_VERSION").into(),
        exported_at: exported_at.into(),
        pack: pack.clone(),
    };
    serde_json::to_string_pretty(&file).map_err(|e| AppError::Invalid(e.to_string()))
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn entry(mr: Option<&str>, cf: Option<u64>, name: &str, added: &str) -> PackEntry {
        PackEntry {
            mod_ref: ModRef { modrinth_id: mr.map(String::from), curseforge_id: cf, preferred: Source::Modrinth },
            file_id: None,
            kind: EntryKind::Mod,
            reason: Reason::User,
            pulled_in_by: vec![],
            payload_ids: vec![],
            tags: vec![],
            notes: None,
            side: None,
            graph_pos: None,
            display: EntryDisplay { name: name.into(), ..Default::default() },
            library_override: None,
            added_at: added.into(),
            extra: Map::new(),
        }
    }

    pub fn pack() -> Pack {
        Pack {
            id: "p1".into(),
            name: " Test pack ".into(),
            mc_version: "1.21.1".into(),
            loader: Loader::Neoforge,
            loader_version: "21.1.267".into(),
            source_preference: SourcePreference::Modrinth,
            icon: None,
            entries: BTreeMap::new(),
            choices: BTreeMap::new(),
            applied_payloads: vec![],
            override_templates: vec![],
            applied_templates: vec![],
            snapshots: vec![],
            created_at: "2026-11-24T00:00:00Z".into(),
            updated_at: "2026-11-24T00:00:00Z".into(),
            extra: Map::new(),
        }
    }

    #[test]
    fn normalize_repairs() {
        let mut p = pack();
        p.entries.insert("a".into(), entry(Some("AANobbMI"), None, "Sodium", "1"));
        p.entries.insert("b".into(), entry(Some("AANobbMI"), Some(394468), "Sodium again", "2"));
        p.entries.insert("c".into(), entry(None, None, "Nothing", "3"));
        let mut cf_only = entry(None, Some(238222), "JEI", "4");
        cf_only.pulled_in_by = vec!["a".into(), "gone".into()];
        p.entries.insert("d".into(), cf_only);
        let notes = normalize(&mut p).unwrap();
        assert_eq!(p.name, "Test pack");
        assert_eq!(p.entries.len(), 2, "{notes:?}");
        assert!(p.entries.contains_key("a"));
        assert_eq!(p.entries["d"].mod_ref.preferred, Source::Curseforge);
        assert_eq!(p.entries["d"].pulled_in_by, vec!["a"]);
        assert_eq!(notes.len(), 3);
    }

    #[test]
    fn normalize_rejects_unnamed() {
        let mut p = pack();
        p.name = "  ".into();
        assert!(normalize(&mut p).is_err());
    }

    #[test]
    fn project_roundtrip_keeps_unknown_fields() {
        let mut p = pack();
        p.entries.insert("a".into(), entry(Some("AANobbMI"), None, "Sodium", "1"));
        p.extra.insert("futureField".into(), Value::from(42));
        p.entries.get_mut("a").unwrap().extra.insert("futureEntryField".into(), Value::from("x"));
        let json = to_project_json(&p, "2026-11-24T00:00:00Z").unwrap();
        let (back, notes) = parse_project(&json).unwrap();
        assert!(notes.is_empty());
        assert_eq!(back.extra["futureField"], Value::from(42));
        assert_eq!(back.entries["a"].extra["futureEntryField"], Value::from("x"));
        assert_eq!(back.name, "Test pack");
    }

    #[test]
    fn project_rejects_foreign_and_newer() {
        assert!(parse_project("{}").is_err());
        assert!(parse_project("not json").is_err());
        let newer = format!(r#"{{"format":"{PROJECT_FORMAT}","schemaVersion":99,"pack":{{}}}}"#);
        let err = parse_project(&newer).unwrap_err().to_string();
        assert!(err.contains("newer"), "{err}");
    }
}
