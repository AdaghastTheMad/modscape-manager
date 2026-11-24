//! Payloads: saved, version-agnostic bundles of mods.
//!
//! A payload stores *projects*, never files, so one payload works on any
//! version and loader; the planner resolves each slot to the right build
//! when it's dropped into a pack.
//!
//! - **Slot**: one mod, as an ordered list of candidates. Each candidate may be
//!   limited to some loaders and/or Minecraft versions. The first candidate
//!   that fits the pack *and* has a build wins (e.g. Presence Footsteps, but
//!   Tread Lightly on 26.1.2; Voxy on Fabric, Roxy on NeoForge).
//! - **Choice**: "one of" several options; each option is a bundle of slots
//!   (Xaero's Minimap + World Map + XaeroPlus vs JourneyMap). `common` slots
//!   come with every option. The pack remembers the pick in `pack.choices`
//!   and only the picked option is ever added.
//! - **Nesting**: `includes` pulls in other payloads (cycle-safe).
//! - **Versioning**: `version` bumps when the contents change, so packs that
//!   applied an older version can show "payload updated".

use crate::error::{AppError, AppResult};
use crate::model::{EntryKind, Pack};
use crate::resolve::{self, Catalog};
use crate::sources::{curseforge, norm, Loader, Source};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

pub const PAYLOAD_FORMAT: &str = "modpack-planner-payload";
pub const PAYLOAD_SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modrinth_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curseforge_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_url: Option<String>,
    /// Only use this candidate on these loaders (empty = any).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub loaders: Vec<Loader>,
    /// Only use this candidate on these Minecraft versions (empty = any).
    /// An entry ending in "*" is a prefix (e.g. "26.*").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mc_versions: Vec<String>,
    /// Mods only for now; resource packs etc. arrive in milestone 11.
    #[serde(default)]
    pub kind: EntryKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Slot {
    pub id: String,
    #[serde(default)]
    pub label: String,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceOption {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub slots: Vec<Slot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PayloadItem {
    Mod(Slot),
    #[serde(rename_all = "camelCase")]
    Choice {
        id: String,
        label: String,
        options: Vec<ChoiceOption>,
        #[serde(default)]
        common: Vec<Slot>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Payload {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub items: Vec<PayloadItem>,
    /// Nested payload ids.
    #[serde(default)]
    pub includes: Vec<String>,
    #[serde(default = "one")]
    pub version: u32,
    /// Shipped with the app (editable copies are made when you change one).
    #[serde(default)]
    pub starter: bool,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
fn one() -> u32 {
    1
}

impl Candidate {
    fn fits(&self, mc: &str, loader: Loader) -> bool {
        let loader_ok = self.loaders.is_empty() || self.loaders.contains(&loader);
        let mc_ok = self.mc_versions.is_empty()
            || self.mc_versions.iter().any(|v| match v.strip_suffix('*') {
                Some(prefix) => mc.starts_with(prefix),
                None => v == mc,
            });
        loader_ok && mc_ok
    }
    /// Catalog ids in the order to try: the pack's preferred site first.
    fn ids(&self, prefer: Source) -> Vec<String> {
        let mr = self.modrinth_id.clone();
        let cf = self.curseforge_id.map(curseforge::cf_id);
        match prefer {
            Source::Modrinth => [mr, cf].into_iter().flatten().collect(),
            Source::Curseforge => [cf, mr].into_iter().flatten().collect(),
        }
    }
}

/// Content fingerprint: what counts as "the payload changed".
fn content_of(p: &Payload) -> Value {
    serde_json::json!({ "items": p.items, "includes": p.includes })
}

/// Normalises a payload before saving; bumps `version` when its contents
/// differ from `previous`. Returns notes about repairs.
pub fn prepare_for_save(p: &mut Payload, previous: Option<&Payload>, now: &str) -> AppResult<Vec<String>> {
    let mut notes = Vec::new();
    p.name = p.name.trim().to_string();
    if p.name.is_empty() {
        return Err(AppError::Invalid("The payload needs a name.".into()));
    }
    if p.id.trim().is_empty() {
        return Err(AppError::Invalid("The payload is missing its id.".into()));
    }
    // Every slot needs at least one candidate with an id.
    let fix_slots = |slots: &mut Vec<Slot>, notes: &mut Vec<String>| {
        slots.retain_mut(|s| {
            s.candidates.retain(|c| c.modrinth_id.is_some() || c.curseforge_id.is_some());
            if s.candidates.is_empty() {
                notes.push(format!("Removed an empty slot{}.", if s.label.is_empty() { String::new() } else { format!(" ({})", s.label) }));
                false
            } else {
                true
            }
        });
    };
    let mut items = std::mem::take(&mut p.items);
    items.retain_mut(|it| match it {
        PayloadItem::Mod(s) => {
            let mut v = vec![s.clone()];
            fix_slots(&mut v, &mut notes);
            match v.pop() {
                Some(x) => {
                    *s = x;
                    true
                }
                None => false,
            }
        }
        PayloadItem::Choice { options, common, label, .. } => {
            for o in options.iter_mut() {
                fix_slots(&mut o.slots, &mut notes);
            }
            fix_slots(common, &mut notes);
            options.retain(|o| !o.slots.is_empty());
            if options.is_empty() {
                notes.push(format!("Removed the choice \"{label}\", which had no options left."));
            }
            !options.is_empty()
        }
    });
    p.items = items;
    p.includes.retain(|i| i != &p.id);
    p.includes.dedup();
    if let Some(prev) = previous {
        p.version = prev.version.max(1);
        p.created_at = prev.created_at.clone();
        if content_of(prev) != content_of(p) {
            p.version += 1;
        }
    } else if p.created_at.is_empty() {
        p.created_at = now.to_string();
    }
    p.updated_at = now.to_string();
    Ok(notes)
}

pub fn to_file_json(p: &Payload) -> AppResult<String> {
    let mut clean = p.clone();
    clean.starter = false;
    serde_json::to_string_pretty(&serde_json::json!({ "format": PAYLOAD_FORMAT, "schemaVersion": PAYLOAD_SCHEMA, "payload": clean }))
        .map_err(|e| AppError::Invalid(e.to_string()))
}

pub fn parse_file(text: &str) -> AppResult<Payload> {
    let v: Value = serde_json::from_str(text).map_err(|e| AppError::Invalid(format!("That file isn't valid JSON: {e}")))?;
    if v.get("format").and_then(Value::as_str) != Some(PAYLOAD_FORMAT) {
        return Err(AppError::Invalid("That JSON file isn't a Modpack Planner payload.".into()));
    }
    if v.get("schemaVersion").and_then(Value::as_u64).unwrap_or(0) as u32 > PAYLOAD_SCHEMA {
        return Err(AppError::Invalid("This payload was saved by a newer Modpack Planner. Update the app to open it.".into()));
    }
    serde_json::from_value(v["payload"].clone()).map_err(|e| AppError::Invalid(format!("The payload file is damaged: {e}")))
}

// ================================================================ planning
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SlotStatus {
    /// One of the slot's candidates is already in the pack.
    #[serde(rename_all = "camelCase")]
    InPack { key: String, name: String },
    /// Will be added: catalog id of the winning candidate (+ its other-site id).
    #[serde(rename_all = "camelCase")]
    Add { project_id: String, alt_id: Option<String>, name: String, icon_url: Option<String>, fallback: bool },
    /// No candidate fits this version/loader or has a build.
    #[serde(rename_all = "camelCase")]
    NoBuild { tried: Vec<String> },
    /// Not a mod (resource packs, shaders, datapacks arrive in milestone 11).
    #[serde(rename_all = "camelCase")]
    NotYet { name: String, kind: EntryKind },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    pub slot_id: String,
    /// Which payload it came from (nested payloads show their own name).
    pub from: String,
    /// Choice it belongs to, if any.
    pub choice: Option<String>,
    pub label: String,
    #[serde(flatten)]
    pub status: SlotStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChoiceView {
    pub id: String,
    pub label: String,
    pub from: String,
    pub options: Vec<(String, String)>,
    pub chosen: String,
    /// Mods in the pack that belong to an option other than the chosen one.
    pub other_option_in_pack: Vec<String>,
}

/// A mod the pack has from this payload that the payload no longer contains.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedRow {
    pub key: String,
    pub name: String,
    /// Mods in the pack that require it (removing it would break them).
    pub needed_by: Vec<String>,
    /// Other payloads (still in the pack) that also include it.
    pub other_payloads: Vec<String>,
    /// It was in the pack before this payload covered it (you added it yourself).
    pub user_added: bool,
    /// Safe to remove without asking: nothing needs it, no other payload has it, you didn't add it.
    pub suggest_remove: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayloadPlan {
    pub payload_id: String,
    pub name: String,
    pub version: u32,
    /// Every payload pulled in (this one plus nested), with versions.
    pub applied: Vec<(String, u32)>,
    pub rows: Vec<Row>,
    pub choices: Vec<ChoiceView>,
    /// Mods the pack got from this payload that its current version no longer has.
    pub removed: Vec<RemovedRow>,
    pub notes: Vec<String>,
}

/// Flattens nested payloads depth-first, skipping cycles and missing ones.
pub fn flatten<'a>(root: &'a Payload, all: &'a HashMap<String, Payload>, notes: &mut Vec<String>) -> Vec<&'a Payload> {
    fn walk<'a>(p: &'a Payload, all: &'a HashMap<String, Payload>, seen: &mut HashSet<String>, out: &mut Vec<&'a Payload>, notes: &mut Vec<String>) {
        if !seen.insert(p.id.clone()) {
            return;
        }
        out.push(p);
        for inc in &p.includes {
            match all.get(inc) {
                Some(child) if seen.contains(&child.id) => notes.push(format!("\"{}\" includes \"{}\" in a loop; skipped the repeat.", p.name, child.name)),
                Some(child) => walk(child, all, seen, out, notes),
                None => notes.push(format!("\"{}\" includes a payload that no longer exists.", p.name)),
            }
        }
    }
    let mut out = Vec::new();
    walk(root, all, &mut HashSet::new(), &mut out, notes);
    out
}

/// Where each candidate already sits in the pack (any site), by id or by slug/name.
fn in_pack(pack: &Pack, c: &Candidate) -> Option<String> {
    pack.entries
        .iter()
        .find(|(_, e)| {
            (c.modrinth_id.is_some() && e.mod_ref.modrinth_id == c.modrinth_id)
                || (c.curseforge_id.is_some() && e.mod_ref.curseforge_id == c.curseforge_id)
        })
        .map(|(k, _)| k.clone())
        .or_else(|| {
            let n = norm(&c.name);
            (!n.is_empty()).then(|| pack.entries.iter().find(|(_, e)| norm(&e.display.name) == n).map(|(k, _)| k.clone())).flatten()
        })
}

async fn resolve_slot(pack: &Pack, s: &Slot, cat: &dyn Catalog) -> SlotStatus {
    for c in &s.candidates {
        if let Some(k) = in_pack(pack, c) {
            return SlotStatus::InPack { name: pack.entries[&k].display.name.clone(), key: k };
        }
    }
    let prefer = match pack.source_preference {
        crate::model::SourcePreference::Curseforge => Source::Curseforge,
        _ => Source::Modrinth,
    };
    let mut tried = Vec::new();
    for (i, c) in s.candidates.iter().enumerate() {
        if c.kind != EntryKind::Mod {
            return SlotStatus::NotYet { name: c.name.clone(), kind: c.kind };
        }
        if !c.fits(&pack.mc_version, pack.loader) {
            continue;
        }
        tried.push(c.name.clone());
        let ids = c.ids(prefer);
        for (j, pid) in ids.iter().enumerate() {
            let vs = cat.versions(pid, &pack.mc_version, pack.loader, false).await.unwrap_or_default();
            if resolve::select_version(&vs, &pack.mc_version, pack.loader, pack.channel_policy).is_some() {
                let alt = ids.iter().enumerate().find(|(x, _)| *x != j).map(|(_, id)| id.clone());
                return SlotStatus::Add { project_id: pid.clone(), alt_id: alt, name: c.name.clone(), icon_url: c.icon_url.clone(), fallback: i > 0 };
            }
        }
    }
    if tried.is_empty() {
        tried = s.candidates.iter().map(|c| format!("{} (not for this version/loader)", c.name)).collect();
    }
    SlotStatus::NoBuild { tried }
}

/// What dropping `payload` into `pack` would do. `choices` overrides the
/// pack's remembered picks.
pub async fn plan(pack: &Pack, payload: &Payload, all: &HashMap<String, Payload>, choices: &HashMap<String, String>, cat: &dyn Catalog) -> PayloadPlan {
    let mut notes = Vec::new();
    let parts = flatten(payload, all, &mut notes);
    let mut jobs: Vec<(String, String, Option<String>, Slot)> = Vec::new(); // (from, label, choice, slot)
    let mut choice_views = Vec::new();
    for p in &parts {
        for it in &p.items {
            match it {
                PayloadItem::Mod(s) => jobs.push((p.name.clone(), s.label.clone(), None, s.clone())),
                PayloadItem::Choice { id, label, options, common } => {
                    // Pick: explicit override → pack's memory → an option already in the pack → first.
                    let remembered = pack.choices.get(id).and_then(|v| v.as_str()).map(String::from);
                    // Options already (partly) in the pack, best first: mods you added
                    // yourself count; ones only present as dependencies (Sodium via
                    // Iris) don't mean you picked that option.
                    let score = |o: &ChoiceOption| -> (usize, usize) {
                        let keys: Vec<String> = o.slots.iter().filter_map(|s| s.candidates.iter().find_map(|c| in_pack(pack, c))).collect();
                        let own = keys.iter().filter(|k| pack.entries[*k].reason != crate::model::Reason::Dependency).count();
                        (own, keys.len())
                    };
                    let mut present: Vec<&ChoiceOption> = options.iter().filter(|o| score(o).1 > 0).collect();
                    present.sort_by(|x, y| score(y).cmp(&score(x)));
                    let chosen = choices
                        .get(id)
                        .cloned()
                        .or(remembered)
                        .filter(|c| options.iter().any(|o| &o.id == c))
                        .or_else(|| present.first().map(|o| o.id.clone()))
                        .unwrap_or_else(|| options[0].id.clone());
                    let other_option_in_pack = present
                        .iter()
                        .filter(|o| o.id != chosen)
                        .flat_map(|o| o.slots.iter().flat_map(|s| s.candidates.iter().filter_map(|c| in_pack(pack, c))))
                        .filter_map(|k| pack.entries.get(&k).map(|e| e.display.name.clone()))
                        .collect();
                    choice_views.push(ChoiceView {
                        id: id.clone(),
                        label: label.clone(),
                        from: p.name.clone(),
                        options: options.iter().map(|o| (o.id.clone(), o.label.clone())).collect(),
                        chosen: chosen.clone(),
                        other_option_in_pack,
                    });
                    for s in common {
                        jobs.push((p.name.clone(), s.label.clone(), Some(id.clone()), s.clone()));
                    }
                    if let Some(o) = options.iter().find(|o| o.id == chosen) {
                        for s in &o.slots {
                            jobs.push((p.name.clone(), s.label.clone(), Some(id.clone()), s.clone()));
                        }
                    }
                }
            }
        }
    }
    type Fut<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = (usize, SlotStatus)> + Send + 'a>>;
    let futs: Vec<Fut<'_>> = jobs.iter().enumerate().map(|(i, (_, _, _, s))| Box::pin(async move { (i, resolve_slot(pack, s, cat).await) }) as Fut<'_>).collect();
    let resolved: Vec<(usize, SlotStatus)> = stream::iter(futs).buffer_unordered(8).collect().await;
    let mut by_index: HashMap<usize, SlotStatus> = resolved.into_iter().collect();
    let mut rows = Vec::new();
    let mut adding: HashSet<String> = HashSet::new();
    for (i, (from, label, choice, s)) in jobs.into_iter().enumerate() {
        let status = by_index.remove(&i).unwrap_or(SlotStatus::NoBuild { tried: vec![] });
        // The same mod via two payloads (or common + option) is added once.
        if let SlotStatus::Add { project_id, .. } = &status {
            if !adding.insert(project_id.clone()) {
                continue;
            }
        }
        let label = if label.is_empty() { s.candidates.first().map(|c| c.name.clone()).unwrap_or_default() } else { label };
        rows.push(Row { slot_id: s.id.clone(), from, choice, label, status });
    }
    // Mods tagged with one of these payloads that no slot (any option) matches any more.
    let ids: HashSet<&String> = parts.iter().map(|p| &p.id).collect();
    let all_cands: Vec<&Candidate> = parts
        .iter()
        .flat_map(|p| p.items.iter())
        .flat_map(|it| match it {
            PayloadItem::Mod(s) => s.candidates.iter().collect::<Vec<_>>(),
            PayloadItem::Choice { options, common, .. } => options.iter().flat_map(|o| o.slots.iter()).chain(common.iter()).flat_map(|s| s.candidates.iter()).collect(),
        })
        .collect();
    let matches = |e: &crate::model::PackEntry| {
        all_cands.iter().any(|c| {
            (c.modrinth_id.is_some() && e.mod_ref.modrinth_id == c.modrinth_id) || (c.curseforge_id.is_some() && e.mod_ref.curseforge_id == c.curseforge_id)
        })
    };
    let mut removed: Vec<RemovedRow> = pack
        .entries
        .iter()
        .filter(|(_, e)| e.payload_ids.iter().any(|p| ids.contains(p)) && !matches(e))
        .map(|(k, e)| {
            let needed_by: Vec<String> = e.pulled_in_by.iter().filter_map(|x| pack.entries.get(x)).map(|x| x.display.name.clone()).collect();
            let other_payloads: Vec<String> =
                e.payload_ids.iter().filter(|p| !ids.contains(p)).map(|p| all.get(p).map(|x| x.name.clone()).unwrap_or_else(|| p.clone())).collect();
            let user_added = e.reason == crate::model::Reason::User;
            RemovedRow {
                key: k.clone(),
                name: e.display.name.clone(),
                suggest_remove: needed_by.is_empty() && other_payloads.is_empty() && !user_added,
                needed_by,
                other_payloads,
                user_added,
            }
        })
        .collect();
    removed.sort_by_key(|r| r.name.to_lowercase());

    PayloadPlan {
        payload_id: payload.id.clone(),
        name: payload.name.clone(),
        version: payload.version,
        applied: parts.iter().map(|p| (p.id.clone(), p.version)).collect(),
        rows,
        choices: choice_views,
        removed,
        notes,
    }
}

/// Builds a payload from pack entries ("Save selection as payload"): one slot
/// per mod, with both site ids where linked. Dependencies are left out; they
/// come back automatically when the payload is applied.
pub fn from_entries(pack: &Pack, keys: &[String], name: &str, now: &str) -> AppResult<Payload> {
    let mut items = Vec::new();
    for k in keys {
        let Some(e) = pack.entries.get(k) else { continue };
        if e.reason == crate::model::Reason::Dependency {
            continue;
        }
        items.push(PayloadItem::Mod(Slot {
            id: uuid::Uuid::new_v4().to_string(),
            label: String::new(),
            candidates: vec![Candidate {
                name: e.display.name.clone(),
                modrinth_id: e.mod_ref.modrinth_id.clone(),
                curseforge_id: e.mod_ref.curseforge_id,
                icon_url: e.display.icon_url.clone(),
                kind: e.kind,
                ..Default::default()
            }],
        }));
    }
    if items.is_empty() {
        return Err(AppError::Invalid("Pick at least one mod you added yourself (dependencies come along on their own).".into()));
    }
    Ok(Payload {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.trim().to_string(),
        description: format!("Saved from {}.", pack.name),
        items,
        includes: vec![],
        version: 1,
        starter: false,
        created_at: now.into(),
        updated_at: now.into(),
        extra: Map::new(),
    })
}

/// The payloads shipped with the app (from the design doc, resolved on Modrinth).
pub fn starters() -> Vec<Payload> {
    #[derive(Deserialize)]
    struct F {
        payloads: Vec<Payload>,
    }
    serde_json::from_str::<F>(include_str!("../data/starter_payloads.json")).expect("starter_payloads.json").payloads
}

/// Adds any starter payload that isn't in the store (first run, or "restore").
pub fn seed_starters(db: &crate::store::Db, now: &str, force: bool) -> AppResult<usize> {
    if !force && db.meta_get("starters_seeded")?.is_some() {
        return Ok(0);
    }
    let mut added = 0;
    for mut p in starters() {
        if db.get_payload(&p.id)?.is_none() {
            p.created_at = now.into();
            p.updated_at = now.into();
            db.put_payload(&p)?;
            added += 1;
        }
    }
    db.meta_set("starters_seeded", now)?;
    Ok(added)
}

/// Payloads this pack applied whose stored version is newer.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateNotice {
    pub payload_id: String,
    pub name: String,
    pub applied_version: u32,
    pub current_version: u32,
}

pub fn updates_for(pack: &Pack, all: &HashMap<String, Payload>) -> Vec<UpdateNotice> {
    pack.applied_payloads
        .iter()
        .filter_map(|a| {
            let p = all.get(&a.id)?;
            (p.version > a.version).then(|| UpdateNotice { payload_id: p.id.clone(), name: p.name.clone(), applied_version: a.version, current_version: p.version })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Reason;
    use crate::resolve::tests::{add, block, counter, key_of, pack_for, Fixture};

    fn cand(name: &str, mr: &str) -> Candidate {
        Candidate { name: name.into(), modrinth_id: Some(mr.into()), ..Default::default() }
    }
    fn slot(id: &str, cands: Vec<Candidate>) -> Slot {
        Slot { id: id.into(), label: String::new(), candidates: cands }
    }
    fn payload(id: &str, items: Vec<PayloadItem>, includes: Vec<&str>) -> Payload {
        Payload { id: id.into(), name: id.into(), description: String::new(), items, includes: includes.into_iter().map(String::from).collect(), version: 1, starter: false, created_at: String::new(), updated_at: String::new(), extra: Map::new() }
    }
    fn adds(p: &PayloadPlan) -> Vec<String> {
        p.rows.iter().filter_map(|r| match &r.status { SlotStatus::Add { project_id, .. } => Some(project_id.clone()), _ => None }).collect()
    }

    #[test]
    fn candidates_fall_back_by_loader_and_version() {
        let f = Fixture::load();
        let mut neo = cand("Neo copy", "xchk0000");
        neo.loaders = vec![Loader::Neoforge];
        let mut fab = cand("Fabric copy", "onlyOld0");
        fab.loaders = vec![Loader::Fabric, Loader::Quilt];
        let p = payload("p", vec![PayloadItem::Mod(slot("s1", vec![fab.clone(), neo.clone()]))], vec![]);
        let all = HashMap::new();
        let on_neo = block(plan(&pack_for("1.21.1", Loader::Neoforge), &p, &all, &HashMap::new(), &f));
        assert!(matches!(&on_neo.rows[0].status, SlotStatus::Add { project_id, fallback: true, .. } if project_id == "xchk0000"));
        let on_fab = block(plan(&pack_for("1.21.1", Loader::Fabric), &p, &all, &HashMap::new(), &f));
        assert!(matches!(&on_fab.rows[0].status, SlotStatus::Add { project_id, fallback: false, .. } if project_id == "onlyOld0"));

        // Presence Footsteps / Tread Lightly style: first has no build on 26.1.2, second does.
        let p = payload("p", vec![PayloadItem::Mod(slot("s1", vec![cand("Old dep", "depOld00"), cand("New dep", "depNew00")]))], vec![]);
        let r = block(plan(&pack_for("26.1.2", Loader::Fabric), &p, &all, &HashMap::new(), &f));
        assert!(matches!(&r.rows[0].status, SlotStatus::Add { project_id, fallback: true, .. } if project_id == "depNew00"));
        // Version-limited candidate with a prefix: skipped on 1.21.1.
        let mut only26 = cand("26 only", "depNew00");
        only26.mc_versions = vec!["26.*".into()];
        let p = payload("p", vec![PayloadItem::Mod(slot("s1", vec![only26]))], vec![]);
        let r = block(plan(&pack_for("1.21.1", Loader::Fabric), &p, &all, &HashMap::new(), &f));
        assert!(matches!(&r.rows[0].status, SlotStatus::NoBuild { tried } if tried[0].contains("not for this version")));
    }

    #[test]
    fn choices_pick_one_and_never_add_both() {
        let f = Fixture::load();
        let ch = PayloadItem::Choice {
            id: "viewer".into(),
            label: "Viewer".into(),
            options: vec![
                ChoiceOption { id: "a".into(), label: "Sodium".into(), slots: vec![slot("a1", vec![cand("Sodium", "AANobbMI")])] },
                ChoiceOption { id: "b".into(), label: "Iris".into(), slots: vec![slot("b1", vec![cand("Iris", "YL57xq9U")])] },
            ],
            common: vec![slot("c1", vec![cand("Mod Menu", "mOgUt4GM")])],
        };
        let p = payload("p", vec![ch], vec![]);
        let all = HashMap::new();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        // Nothing in the pack: first option + common.
        let r = block(plan(&pack, &p, &all, &HashMap::new(), &f));
        assert_eq!(r.choices[0].chosen, "a");
        assert_eq!(adds(&r), vec!["mOgUt4GM", "AANobbMI"]);
        // Pack already has option b's mod: that's the pick; nothing from a.
        let mut keys = counter();
        block(add(&f, &mut pack, "iris", &mut keys));
        let r = block(plan(&pack, &p, &all, &HashMap::new(), &f));
        assert_eq!(r.choices[0].chosen, "b");
        assert!(!adds(&r).contains(&"AANobbMI".to_string()), "never both options");
        assert!(r.rows.iter().any(|x| matches!(&x.status, SlotStatus::InPack { name, .. } if name.contains("Iris"))));
        // Explicit override to a: warns that b's mod is in the pack.
        let r = block(plan(&pack, &p, &all, &HashMap::from([("viewer".into(), "a".into())]), &f));
        assert_eq!(r.choices[0].chosen, "a");
        assert_eq!(r.choices[0].other_option_in_pack, vec!["Iris Shaders"]);
        // The pack's remembered pick wins over auto-detection.
        pack.choices.insert("viewer".into(), Value::from("a"));
        assert_eq!(block(plan(&pack, &p, &all, &HashMap::new(), &f)).choices[0].chosen, "a");
    }

    #[test]
    fn nesting_is_cycle_safe_and_dedupes() {
        let f = Fixture::load();
        let a = payload("a", vec![PayloadItem::Mod(slot("x", vec![cand("Sodium", "AANobbMI")]))], vec!["b"]);
        let b = payload("b", vec![PayloadItem::Mod(slot("y", vec![cand("Sodium again", "AANobbMI")])), PayloadItem::Mod(slot("z", vec![cand("Lithium", "gvQqBUqZ")]))], vec!["a"]);
        let all = HashMap::from([("a".to_string(), a.clone()), ("b".to_string(), b.clone())]);
        let r = block(plan(&pack_for("1.21.1", Loader::Fabric), &a, &all, &HashMap::new(), &f));
        assert_eq!(r.applied.iter().map(|x| x.0.as_str()).collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(adds(&r).iter().filter(|x| *x == "AANobbMI").count(), 1, "same mod via two payloads added once");
        assert!(r.notes.iter().any(|n| n.contains("loop")));
    }

    #[test]
    fn review_lists_mods_removed_from_the_payload() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "sodium-extra", &mut keys)); // + Sodium (dependency)
        block(add(&f, &mut pack, "modmenu", &mut keys));
        block(add(&f, &mut pack, "iris", &mut keys));
        let extra = key_of(&pack, &f, "sodium-extra");
        let sodium = key_of(&pack, &f, "sodium");
        let modmenu = key_of(&pack, &f, "modmenu");
        let iris = key_of(&pack, &f, "iris");
        for k in [&extra, &sodium, &modmenu] {
            pack.entries.get_mut(k).unwrap().payload_ids.push("p".into());
        }
        pack.entries.get_mut(&extra).unwrap().reason = Reason::Payload;
        pack.entries.get_mut(&modmenu).unwrap().reason = Reason::Payload;
        pack.entries.get_mut(&modmenu).unwrap().payload_ids.push("other".into());
        pack.entries.get_mut(&sodium).unwrap().reason = Reason::Payload;
        pack.entries.get_mut(&sodium).unwrap().pulled_in_by = vec![extra.clone(), iris.clone()];
        // v2 of "p" only keeps Lithium: Sodium Extra, Sodium and Mod Menu were removed from it.
        let p = payload("p", vec![PayloadItem::Mod(slot("s", vec![cand("Lithium", "gvQqBUqZ")]))], vec![]);
        let other = payload("other", vec![], vec![]);
        let all = HashMap::from([("p".to_string(), p.clone()), ("other".to_string(), Payload { name: "Other".into(), ..other })]);
        let r = block(plan(&pack, &p, &all, &HashMap::new(), &f));
        let by: HashMap<&str, &RemovedRow> = r.removed.iter().map(|x| (x.name.as_str(), x)).collect();
        assert_eq!(by.len(), 3, "{:?}", r.removed);
        assert!(by["Sodium Extra"].suggest_remove, "nothing needs it, no other payload, not user-added");
        assert!(!by["Sodium"].suggest_remove && by["Sodium"].needed_by.contains(&"Iris Shaders".to_string()), "Iris still needs Sodium");
        assert!(!by["Mod Menu"].suggest_remove && by["Mod Menu"].other_payloads == vec!["Other"]);
        assert!(!r.removed.iter().any(|x| x.key == iris), "Iris was never from this payload");
    }

    #[test]
    fn resource_packs_wait_for_m11() {
        let f = Fixture::load();
        let mut rp = cand("Fresh Animations", "50dA9Sha");
        rp.kind = EntryKind::Resourcepack;
        let p = payload("p", vec![PayloadItem::Mod(slot("s", vec![rp]))], vec![]);
        let r = block(plan(&pack_for("1.21.1", Loader::Fabric), &p, &HashMap::new(), &HashMap::new(), &f));
        assert!(matches!(&r.rows[0].status, SlotStatus::NotYet { kind: EntryKind::Resourcepack, .. }));
    }

    #[test]
    fn saving_bumps_version_only_on_content_change() {
        let mut p = payload("p", vec![PayloadItem::Mod(slot("s", vec![cand("Sodium", "AANobbMI")])), PayloadItem::Mod(slot("empty", vec![]))], vec!["p"]);
        p.name = "  Perf ".into();
        let notes = prepare_for_save(&mut p, None, "t1").unwrap();
        assert_eq!((p.name.as_str(), p.items.len(), p.includes.len(), p.version), ("Perf", 1, 0, 1));
        assert!(notes.iter().any(|n| n.contains("empty slot")));
        let prev = p.clone();
        let mut same = p.clone();
        same.description = "only the description changed".into();
        prepare_for_save(&mut same, Some(&prev), "t2").unwrap();
        assert_eq!(same.version, 1);
        let mut changed = p.clone();
        changed.items.push(PayloadItem::Mod(slot("s2", vec![cand("Lithium", "gvQqBUqZ")])));
        prepare_for_save(&mut changed, Some(&prev), "t3").unwrap();
        assert_eq!(changed.version, 2);
        let mut nameless = p.clone();
        nameless.name = " ".into();
        assert!(prepare_for_save(&mut nameless, None, "t").is_err());
    }

    #[test]
    fn selection_updates_and_files() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "sodium-extra", &mut keys));
        let all_keys: Vec<String> = pack.entries.keys().cloned().collect();
        let p = from_entries(&pack, &all_keys, "Mine", "t").unwrap();
        assert_eq!(p.items.len(), 1, "Sodium came in as a dependency, so it's left out");
        assert!(pack.entries.values().any(|e| e.reason == Reason::Dependency));
        let back = parse_file(&to_file_json(&p).unwrap()).unwrap();
        assert_eq!(back.items, p.items);
        assert!(parse_file("{}").is_err());

        let mut newer = p.clone();
        newer.version = 3;
        pack.applied_payloads.push(crate::model::AppliedRef { id: p.id.clone(), version: 1 });
        let u = updates_for(&pack, &HashMap::from([(p.id.clone(), newer)]));
        assert_eq!((u.len(), u[0].applied_version, u[0].current_version), (1, 1, 3));
        let sk = key_of(&pack, &f, "sodium");
        assert!(in_pack(&pack, &cand("whatever", "AANobbMI")).is_some_and(|k| k == sk));
    }

    #[test]
    fn starters_are_well_formed() {
        let s = starters();
        assert_eq!(s.len(), 10);
        let ids: HashSet<&str> = s.iter().map(|p| p.id.as_str()).collect();
        for p in &s {
            assert!(p.starter && !p.name.is_empty());
            assert!(p.includes.iter().all(|i| ids.contains(i.as_str())), "{} includes a missing payload", p.name);
            let slots: Vec<&Slot> = p.items.iter().flat_map(|it| match it {
                PayloadItem::Mod(s) => vec![s],
                PayloadItem::Choice { options, common, .. } => options.iter().flat_map(|o| o.slots.iter()).chain(common.iter()).collect(),
            }).collect();
            assert!(slots.iter().all(|s| !s.candidates.is_empty() && s.candidates.iter().all(|c| c.modrinth_id.is_some())), "{}", p.name);
        }
        let mut notes = vec![];
        let map: HashMap<String, Payload> = s.iter().map(|p| (p.id.clone(), p.clone())).collect();
        assert_eq!(flatten(&map["starter-survival"], &map, &mut notes).len(), 4);
    }
}
