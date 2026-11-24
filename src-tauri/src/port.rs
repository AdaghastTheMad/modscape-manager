//! Duplicate a pack to another Minecraft version and/or loader, with a report.
//!
//! - Mods you added (and payload mods) carry over with their tags, notes and
//!   settings. Pins are dropped: file ids are specific to one version.
//! - Each one is checked for a build on the target. Without one it's reported
//!   (and left out, unless you ask to keep it flagged); if its other-site copy
//!   has a build, the report says so.
//! - Dependencies are worked out fresh for the target: old dependency entries
//!   are reused (keeping notes/tags) when still needed, dropped when not, and
//!   new ones are added.
//! - The report also lists mods whose required dependencies changed.

use crate::error::{AppError, AppResult};
use crate::model::{Pack, PackEntry, Reason};
use crate::resolve::{self, Catalog, Fix};
use crate::sources::{curseforge, Loader, Source};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortTarget {
    pub mc_version: String,
    pub loader: Loader,
    #[serde(default)]
    pub loader_version: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Keep mods without a target build in the new pack (they'll show as "no build").
    #[serde(default)]
    pub keep_missing: bool,
    /// Old entry keys to carry over as your own mods even though nothing on the
    /// target needs them (dependencies of mods that didn't make it, e.g.
    /// Sophisticated Storage when Sophisticated Inventory Interactions has no build).
    #[serde(default)]
    pub keep: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DroppedDep {
    pub key: String,
    pub name: String,
    /// Whether it has a build on the target (worth keeping?).
    pub has_build: bool,
    /// Old mods that pulled it in.
    pub was_needed_by: Vec<String>,
    /// Looks like a library rather than a mod you'd play with.
    pub library: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Missing {
    pub key: String,
    pub name: String,
    /// The mod's other-site copy has a build for the target.
    pub other_site: Option<Source>,
    /// Mods in the old pack that needed this one.
    pub needed_by: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepChange {
    pub key: String,
    pub name: String,
    pub added: Vec<String>,
    pub removed: Vec<String>,
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PortReport {
    /// Your mods that have a build on the target.
    pub ported: Vec<String>,
    pub missing: Vec<Missing>,
    pub changed_deps: Vec<DepChange>,
    /// Dependencies the target needs that the old pack didn't have.
    pub new_deps: Vec<String>,
    /// Old dependencies nothing needs on the target; tick them to keep them.
    pub dropped_deps: Vec<DroppedDep>,
    /// Dependencies you chose to keep, now carried as your own mods.
    pub kept: Vec<String>,
    pub pins_dropped: usize,
    pub notes: Vec<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortPlan {
    pub pack: Pack,
    pub report: PortReport,
}

fn project_of(e: &PackEntry) -> Option<String> {
    resolve::target(e).map(|(p, _)| p)
}

/// The other-site id of an entry, as a catalog id.
fn other_site(e: &PackEntry) -> Option<(Source, String)> {
    match e.mod_ref.preferred {
        Source::Modrinth => e.mod_ref.curseforge_id.map(|n| (Source::Curseforge, curseforge::cf_id(n))),
        Source::Curseforge => e.mod_ref.modrinth_id.clone().map(|id| (Source::Modrinth, id)),
    }
}

/// Required-dependency names per entry key, from an analysis.
fn required_names(a: &resolve::Analysis) -> HashMap<String, HashSet<String>> {
    a.entries.iter().map(|(k, r)| (k.clone(), r.required.iter().map(|d| d.project_id.clone()).collect())).collect()
}

async fn has_build(cat: &dyn Catalog, pid: &str, mc: &str, loader: Loader, kind: crate::model::EntryKind, policy: crate::model::ChannelPolicy) -> bool {
    let vs = cat.versions_of(pid, mc, loader, kind, false).await.unwrap_or_default();
    resolve::select_version_kind(&vs, mc, loader, kind, policy).is_some()
}

pub async fn plan_port(pack: &Pack, t: &PortTarget, cat: &dyn Catalog, now_iso: &str, mut new_key: impl FnMut() -> String) -> AppResult<PortPlan> {
    let mc = t.mc_version.trim().to_string();
    if mc.is_empty() {
        return Err(AppError::Invalid("Pick a Minecraft version to port to.".into()));
    }
    if mc == pack.mc_version && t.loader == pack.loader {
        return Err(AppError::Invalid("That's the version and loader the pack already uses.".into()));
    }
    let mut report = PortReport::default();
    // New keys must never collide with the old pack's (old entries keep theirs).
    let mut used: HashSet<String> = pack.entries.keys().cloned().collect();
    let mut new_key = move || loop {
        let k = new_key();
        if used.insert(k.clone()) {
            return k;
        }
    };

    // The old pack's dependency picture, for the "changed dependencies" report.
    let before = resolve::analyze(pack, cat, false, crate::store::now_secs()).await;
    let before_req = required_names(&before);

    // New pack shell: same settings, new target, no entries yet.
    let mut out = pack.clone();
    out.id = uuid::Uuid::new_v4().to_string();
    out.name = t.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| format!("{} ({mc})", pack.name.trim()));
    out.mc_version = mc.clone();
    out.loader = t.loader;
    out.loader_version = t.loader_version.trim().to_string();
    out.entries = BTreeMap::new();
    out.snapshots = vec![];
    out.created_at = now_iso.to_string();
    out.updated_at = now_iso.to_string();

    // 1. Which of your mods have a build on the target?
    let keep: HashSet<&String> = t.keep.iter().collect();
    let is_root = |k: &String, e: &PackEntry| e.reason != Reason::Dependency || keep.contains(k);
    let roots: Vec<(String, PackEntry)> = pack
        .entries
        .iter()
        .filter(|(k, e)| is_root(k, e))
        .map(|(k, e)| {
            let mut e = e.clone();
            if keep.contains(k) {
                e.reason = Reason::User; // kept on purpose: now one of your mods
                report.kept.push(e.display.name.clone());
            }
            (k.clone(), e)
        })
        .collect();
    let deps_old: Vec<(String, PackEntry)> = pack.entries.iter().filter(|(k, e)| !is_root(k, e)).map(|(k, e)| (k.clone(), e.clone())).collect();
    let (mc_ref, policy) = (mc.as_str(), pack.channel_policy);
    let checks: Vec<(String, PackEntry, bool, Option<Source>)> = stream::iter(roots)
        .map(|(k, e)| async move {
            let ok = match project_of(&e) {
                Some(p) => has_build(cat, &p, mc_ref, t.loader, e.kind, policy).await,
                None => false,
            };
            let mut alt = None;
            if !ok {
                if let Some((s, id)) = other_site(&e) {
                    if has_build(cat, &id, mc_ref, t.loader, e.kind, policy).await {
                        alt = Some(s);
                    }
                }
            }
            (k, e, ok, alt)
        })
        .buffer_unordered(8)
        .collect()
        .await;

    for (k, mut e, ok, alt) in checks {
        if e.file_id.take().is_some() {
            report.pins_dropped += 1;
        }
        e.pulled_in_by.clear();
        e.graph_pos = None; // layouts are per pack
        if ok {
            report.ported.push(e.display.name.clone());
            out.entries.insert(k, e);
        } else {
            let needed_by: Vec<String> = before.entries.get(&k).map(|r| r.required_by.iter().filter_map(|x| pack.entries.get(x)).map(|x| x.display.name.clone()).collect()).unwrap_or_default();
            report.missing.push(Missing { key: k.clone(), name: e.display.name.clone(), other_site: alt, needed_by });
            if let Some(s) = alt {
                // Switch to the copy that has a build.
                e = PackEntry { mod_ref: crate::model::ModRef { preferred: s, ..e.mod_ref.clone() }, ..e };
                out.entries.insert(k, e);
            } else if t.keep_missing {
                out.entries.insert(k, e);
            }
        }
    }

    // 2. Fill required dependencies for the target, reusing old dependency entries.
    let old_by_project: HashMap<String, (String, PackEntry)> =
        deps_old.into_iter().filter_map(|(k, e)| project_of(&e).map(|p| (p, (k, e)))).collect();
    for _round in 0..6 {
        let a = resolve::analyze(&out, cat, false, crate::store::now_secs()).await;
        let wanted: Vec<(String, Option<String>, String)> = a
            .issues
            .iter()
            .filter_map(|i| match &i.fix {
                Some(Fix::AddProject { project_id, pin_version, for_key: Some(k), .. }) if i.kind == "missing_dependency" => {
                    Some((project_id.clone(), pin_version.clone(), k.clone()))
                }
                _ => None,
            })
            .collect();
        if wanted.is_empty() {
            break;
        }
        for (pid, pin, parent) in wanted {
            if let Some((ok, oe)) = old_by_project.get(&pid) {
                if !out.entries.contains_key(ok) {
                    let mut e = oe.clone();
                    if e.file_id.take().is_some() {
                        report.pins_dropped += 1;
                    }
                    e.pulled_in_by = vec![parent.clone()];
                    e.graph_pos = None;
                    out.entries.insert(ok.clone(), e);
                    continue;
                }
            }
            let plan = resolve::plan_add(&out, &pid, pin.as_deref(), Some(&parent), None, cat, false, now_iso, &mut new_key).await?;
            for a in plan.additions {
                report.new_deps.push(a.entry.display.name.clone());
                out.entries.insert(a.key, a.entry);
            }
            for l in plan.links {
                if let Some(e) = out.entries.get_mut(&l.key) {
                    if !e.pulled_in_by.contains(&l.parent) {
                        e.pulled_in_by.push(l.parent);
                    }
                }
            }
            report.notes.extend(plan.notes);
        }
    }

    // 3. Sync "pulled in by", then report what changed.
    let after = resolve::analyze(&out, cat, false, crate::store::now_secs()).await;
    for (k, rep) in &after.entries {
        if let Some(e) = out.entries.get_mut(k) {
            if after.complete {
                e.pulled_in_by = rep.required_by.clone();
            }
        }
    }
    let dropped: Vec<(String, PackEntry)> =
        pack.entries.iter().filter(|(k, e)| e.reason == Reason::Dependency && !keep.contains(k) && !out.entries.contains_key(*k)).map(|(k, e)| (k.clone(), e.clone())).collect();
    let mut dropped_deps: Vec<DroppedDep> = stream::iter(dropped)
        .map(|(k, e)| {
            let before = &before;
            async move {
                let has_build = match project_of(&e) {
                    Some(p) => has_build(cat, &p, mc_ref, t.loader, e.kind, policy).await,
                    None => false,
                };
                let was_needed_by = before.entries.get(&k).map(|r| r.required_by.iter().filter_map(|x| pack.entries.get(x)).map(|x| x.display.name.clone()).collect()).unwrap_or_default();
                let library = crate::model::is_library(&e);
                DroppedDep { key: k, name: e.display.name.clone(), has_build, was_needed_by, library }
            }
        })
        .buffer_unordered(8)
        .collect()
        .await;
    dropped_deps.sort_by(|a, b| a.library.cmp(&b.library).then(b.has_build.cmp(&a.has_build)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    report.dropped_deps = dropped_deps;
    let after_req = required_names(&after);
    let name_of = |pid: &str, a: &resolve::Analysis| -> String {
        a.entries.values().flat_map(|r| r.required.iter()).find(|d| d.project_id == pid).map(|d| d.name.clone()).unwrap_or_else(|| pid.to_string())
    };
    for (k, e) in &out.entries {
        let (Some(old), Some(new)) = (before_req.get(k), after_req.get(k)) else { continue };
        if !before.entries.get(k).is_some_and(|r| r.checked) || !after.entries.get(k).is_some_and(|r| r.checked) {
            continue;
        }
        let added: Vec<String> = new.difference(old).map(|p| name_of(p, &after)).collect();
        let removed: Vec<String> = old.difference(new).map(|p| name_of(p, &before)).collect();
        if !added.is_empty() || !removed.is_empty() {
            report.changed_deps.push(DepChange { key: k.clone(), name: e.display.name.clone(), added, removed });
        }
    }

    let _ = crate::model::normalize(&mut out);
    for v in [&mut report.ported, &mut report.new_deps, &mut report.kept] {
        v.sort_by_key(|s| s.to_lowercase());
        v.dedup();
    }
    report.missing.sort_by_key(|m| m.name.to_lowercase());
    report.changed_deps.sort_by_key(|c| c.name.to_lowercase());
    if out.loader_version.is_empty() {
        report.notes.push("No loader version was set for the target; pick one in the new pack before exporting.".into());
    }
    let switched = report.missing.iter().filter(|m| m.other_site.is_some()).count();
    if switched > 0 {
        report.notes.push(format!("{switched} mod{} switched to the other site, which has a build.", if switched == 1 { " was" } else { "s were" }));
    }
    Ok(PortPlan { pack: out, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::tests::{add, block, counter, key_of, pack_for, Fixture};

    fn target(mc: &str, loader: Loader) -> PortTarget {
        PortTarget { mc_version: mc.into(), loader, loader_version: "0.20.1".into(), name: None, keep_missing: false, keep: vec![] }
    }

    #[test]
    fn port_reports_builds_missing_and_changed_deps() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "sodium-extra", &mut keys)); // + Sodium (dependency); both on 26.1.2 (recorded)
        block(add(&f, &mut pack, "port-mod", &mut keys)); // + Dep Old; on 26.1.2 it needs Dep New
        block(add(&f, &mut pack, "only-old", &mut keys)); // no 26.1.2 build on Modrinth…
        let only = key_of(&pack, &f, "only-old");
        pack.entries.get_mut(&only).unwrap().mod_ref.curseforge_id = Some(4444); // …but its CurseForge copy has one
        let extra = key_of(&pack, &f, "sodium-extra");
        let sodium = key_of(&pack, &f, "sodium");
        pack.entries.get_mut(&extra).unwrap().file_id = Some("pinned-1.21.1-file".into());
        pack.entries.get_mut(&extra).unwrap().tags = vec!["perf".into()];
        pack.entries.get_mut(&sodium).unwrap().notes = Some("keep me".into());

        let plan = block(plan_port(&pack, &target("26.1.2", Loader::Fabric), &f, "t", counter())).unwrap();
        let r = &plan.report;
        let out = &plan.pack;
        assert_eq!(out.mc_version, "26.1.2");
        assert_ne!(out.id, pack.id);
        assert_eq!(out.name, "Test pack (26.1.2)");
        assert!(r.ported.contains(&"Sodium Extra".to_string()) && r.ported.contains(&"Port Mod".to_string()), "{r:?}");

        // No Modrinth build, but the CurseForge copy has one: switched, not lost.
        let m = r.missing.iter().find(|m| m.name == "Only Old").unwrap();
        assert_eq!(m.other_site, Some(Source::Curseforge));
        assert_eq!(out.entries[&only].mod_ref.preferred, Source::Curseforge);

        // Dependencies recomputed for the target.
        assert_eq!(r.new_deps, vec!["Dep New"]);
        assert_eq!(r.dropped_deps.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), vec!["Dep Old"]);
        assert!(!r.dropped_deps[0].has_build, "Dep Old has no 26.1.2 build");
        assert_eq!(r.dropped_deps[0].was_needed_by, vec!["Port Mod"]);
        let change = r.changed_deps.iter().find(|c| c.name == "Port Mod").expect("changed deps");
        assert_eq!((change.added.clone(), change.removed.clone()), (vec!["Dep New".to_string()], vec!["Dep Old".to_string()]));

        // The old Sodium entry is reused with its notes; settings and tags carry over; pins don't.
        assert_eq!(out.entries[&sodium].notes.as_deref(), Some("keep me"));
        assert_eq!(out.entries[&sodium].reason, Reason::Dependency);
        assert_eq!(out.entries[&extra].tags, vec!["perf"]);
        assert!(r.pins_dropped >= 1);
        assert!(out.entries.values().all(|e| e.file_id.is_none()));

        // The new pack is clean on the target.
        let a = block(resolve::analyze(out, &f, false, crate::resolve::tests::NOW));
        assert!(!a.issues.iter().any(|i| i.kind == "missing_dependency" || i.kind == "no_build"), "{:?}", a.issues);
    }

    #[test]
    fn dropped_dependencies_can_be_kept_and_are_rechecked() {
        // The owner's case: a mod with no target build (Sophisticated Inventory
        // Interactions) drops its dependencies (Sophisticated Storage), which
        // are worth keeping on their own when they have builds.
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "sodium-extra", &mut keys)); // Sodium comes in as a dependency
        let extra = key_of(&pack, &f, "sodium-extra");
        let sodium = key_of(&pack, &f, "sodium");
        // Pretend Sodium Extra has no target build: swap it for one that doesn't.
        pack.entries.get_mut(&extra).unwrap().mod_ref.modrinth_id = Some("onlyOld0".into());
        pack.entries.get_mut(&sodium).unwrap().pulled_in_by = vec![extra.clone()];

        let plan = block(plan_port(&pack, &target("26.1.2", Loader::Fabric), &f, "t", counter())).unwrap();
        let d = plan.report.dropped_deps.iter().find(|d| d.key == sodium).expect("Sodium dropped");
        assert!(d.has_build, "Sodium has a 26.1.2 build, so it's worth keeping");
        assert!(!plan.pack.entries.contains_key(&sodium));

        let mut t = target("26.1.2", Loader::Fabric);
        t.keep = vec![sodium.clone()];
        let plan = block(plan_port(&pack, &t, &f, "t", counter())).unwrap();
        let kept = &plan.pack.entries[&sodium];
        assert_eq!(kept.reason, Reason::User, "kept on purpose → your own mod");
        assert_eq!(plan.report.kept, vec!["Sodium"]);
        assert!(plan.report.ported.contains(&"Sodium".to_string()));
        assert!(plan.report.dropped_deps.iter().all(|d| d.key != sodium));
    }

    #[test]
    fn keep_missing_keeps_flagged_mods_and_same_target_is_refused() {
        let f = Fixture::load();
        let mut pack = pack_for("1.21.1", Loader::Fabric);
        let mut keys = counter();
        block(add(&f, &mut pack, "only-old", &mut keys));
        let mut t = target("26.1.2", Loader::Fabric);
        let plan = block(plan_port(&pack, &t, &f, "t", counter())).unwrap();
        assert!(plan.pack.entries.is_empty(), "left out by default");
        t.keep_missing = true;
        let plan = block(plan_port(&pack, &t, &f, "t", counter())).unwrap();
        assert_eq!(plan.pack.entries.len(), 1);
        assert!(block(plan_port(&pack, &target("1.21.1", Loader::Fabric), &f, "t", counter())).is_err());
    }
}
