//! The file format: canonical, versioned JSON. One record per node, in path order.
//!
//! Canonical means `serialise` is byte-stable: object keys sort, empty fields are left out,
//! and `load(serialise(t))` diffs empty against `t`. "Unchanged" for an opaque node means
//! unchanged relative to this canonical output.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as Json};

use crate::edit::{At, Edit};
use crate::error::{Error, Result};
use crate::id::{IdSource, NodeId};
use crate::path::{Path, valid_name};
use crate::registry::{CATEGORY, GROUP, NodeType, Problem, ROOT, Registry, ValueSpec};
use crate::state::{BindingData, NodeData, On, Ref, State};
use crate::value::Value;

/// The format version this build writes and reads.
pub const FORMAT_VERSION: u64 = 1;

/// Something load or paste could not take as written, with where it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

impl fmt::Display for Issue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path, self.message)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LoadReport {
    pub issues: Vec<Issue>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PasteReport {
    /// The new top nodes, in the fragment's path order.
    pub nodes: Vec<NodeId>,
    pub issues: Vec<Issue>,
}

/// Path order, so a report reads top to bottom. Stable within one path.
fn sort_issues(issues: &mut [Issue]) {
    issues.sort_by(|a, b| match (Path::parse(&a.path), Path::parse(&b.path)) {
        (Ok(x), Ok(y)) => x.cmp(&y),
        _ => a.path.cmp(&b.path),
    });
}

fn issue(issues: &mut Vec<Issue>, path: impl fmt::Display, message: impl Into<String>) {
    issues.push(Issue {
        path: path.to_string(),
        message: message.into(),
    });
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
struct Record {
    path: String,
    id: String,
    #[serde(rename = "type")]
    type_name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    values: BTreeMap<String, Json>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    refs: BTreeMap<String, Json>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    orders: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    members: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    bindings: Vec<Json>,
}

#[derive(Serialize)]
struct Doc<'a> {
    rhizome: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    fragment: Option<&'a FragmentInfo>,
    nodes: &'a [Record],
}

fn to_text(fragment: Option<&FragmentInfo>, nodes: &[Record]) -> String {
    let doc = Doc {
        rhizome: FORMAT_VERSION,
        fragment,
        nodes,
    };
    let mut s = serde_json::to_string_pretty(&doc).expect("serialisable");
    s.push('\n');
    s
}

fn values_json(values: &BTreeMap<String, Value>) -> BTreeMap<String, Json> {
    values
        .iter()
        .map(|(k, v)| (k.clone(), v.to_json()))
        .collect()
}

fn record_of(paths: &BTreeMap<NodeId, Path>, d: &NodeData) -> Record {
    let refs = d
        .refs
        .iter()
        .map(|(k, r)| {
            let mut r = r.clone();
            if r.is_here() {
                r.path = r.node.and_then(|n| paths.get(&n).cloned());
            }
            (k.clone(), serde_json::to_value(r).expect("ref"))
        })
        .collect();
    let bindings = d
        .bindings
        .iter()
        .map(|((on, src), b)| {
            let mut m = Map::new();
            m.insert("on".into(), serde_json::to_value(on).expect("on"));
            m.insert("source".into(), Json::String(src.to_string()));
            let values = b.raw.clone().unwrap_or_else(|| values_json(&b.values));
            if !values.is_empty() {
                m.insert("values".into(), Json::Object(values.into_iter().collect()));
            }
            Json::Object(m)
        })
        .collect();
    Record {
        path: paths[&d.id].to_string(),
        id: d.id.to_string(),
        type_name: d.type_name.clone(),
        values: d.raw.clone().unwrap_or_else(|| values_json(&d.values)),
        refs,
        orders: d
            .orders
            .iter()
            .map(|(k, ids)| (k.clone(), ids.iter().map(|i| i.to_string()).collect()))
            .collect(),
        members: d.members.iter().map(|i| i.to_string()).collect(),
        bindings,
    }
}

fn records(state: &State, ids: &BTreeSet<NodeId>) -> Vec<Record> {
    let paths = state.all_paths();
    let mut sorted: Vec<(&Path, NodeId)> = ids.iter().map(|id| (&paths[id], *id)).collect();
    sorted.sort();
    sorted
        .into_iter()
        .map(|(_, id)| record_of(&paths, state.node(id)))
        .collect()
}

pub(crate) fn serialise(state: &State) -> String {
    let all: BTreeSet<NodeId> = state.nodes.keys().copied().collect();
    to_text(None, &records(state, &all))
}

// ---- reading ----

struct Parsed {
    path: Path,
    id: NodeId,
    rec: Record,
}

fn read_doc(text: &str) -> Result<Json> {
    let doc: Json = serde_json::from_str(text).map_err(|e| Error::Format(e.to_string()))?;
    let v = doc
        .get("rhizome")
        .and_then(Json::as_u64)
        .ok_or_else(|| Error::Format("no `rhizome` version field".into()))?;
    if v != FORMAT_VERSION {
        return Err(Error::Version(v));
    }
    Ok(doc)
}

fn parse_records(nodes: &[Json], issues: &mut Vec<Issue>) -> Vec<Parsed> {
    let mut recs = Vec::new();
    for (i, j) in nodes.iter().enumerate() {
        let label = j
            .get("path")
            .and_then(Json::as_str)
            .map(String::from)
            .unwrap_or_else(|| format!("node #{i}"));
        match serde_json::from_value::<Record>(j.clone()) {
            Ok(r) => recs.push((label, r)),
            Err(e) => issue(issues, label, format!("unreadable record, dropped ({e})")),
        }
    }
    check_records(recs, issues)
}

fn check_records(recs: Vec<(String, Record)>, issues: &mut Vec<Issue>) -> Vec<Parsed> {
    let mut out: Vec<Parsed> = Vec::new();
    for (label, rec) in recs {
        let Ok(path) = Path::parse(&rec.path) else {
            issue(issues, label, "invalid path, dropped");
            continue;
        };
        let Ok(id) = rec.id.parse::<NodeId>() else {
            issue(issues, label, "invalid id, dropped");
            continue;
        };
        out.push(Parsed { path, id, rec });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    let (mut ids, mut paths) = (BTreeSet::new(), BTreeSet::new());
    out.retain(|p| {
        if !ids.insert(p.id) {
            issue(issues, &p.path, "duplicate id, dropped");
            return false;
        }
        if !paths.insert(p.path.clone()) {
            issue(issues, &p.path, "duplicate path, dropped");
            return false;
        }
        true
    });
    out
}

fn parse_id(s: &str, remap: &BTreeMap<NodeId, NodeId>) -> Option<NodeId> {
    let id: NodeId = s.parse().ok()?;
    remap.get(&id).copied()
}

fn parse_values(
    specs: &[ValueSpec],
    raw: &BTreeMap<String, Json>,
    path: &Path,
    issues: &mut Vec<Issue>,
) -> BTreeMap<String, Value> {
    let mut out = BTreeMap::new();
    for (k, j) in raw {
        let Some(spec) = specs.iter().find(|s| &s.key == k) else {
            issue(issues, path, format!("unknown key `{k}`, dropped"));
            continue;
        };
        let Some(v) = Value::from_json(spec.kind, j) else {
            issue(
                issues,
                path,
                format!("`{k}` should be {:?}, dropped", spec.kind),
            );
            continue;
        };
        match spec.problem(&v) {
            None => {
                out.insert(k.clone(), v);
            }
            Some(Problem::OutOfRange { .. }) => {
                let c = spec.clamp(&v);
                issue(
                    issues,
                    path,
                    format!("`{k}` = {v} is out of range, clamped to {c}"),
                );
                out.insert(k.clone(), c);
            }
            Some(Problem::NotAChoice) => {
                issue(issues, path, format!("`{k}` has no choice {v}, dropped"));
            }
            Some(_) => {
                issue(issues, path, format!("`{k}` = {v} doesn't fit, dropped"));
            }
        }
    }
    out
}

/// Fills a placed node's values, refs, orders, members and bindings from its record.
/// `remap` maps every id the record may point at to the id to store; anything absent is
/// missing. Missing here-references are kept (unresolved); other missing pointers drop.
fn fill(
    state: &mut State,
    registry: &Registry,
    id: NodeId,
    rec: &Record,
    remap: &BTreeMap<NodeId, NodeId>,
    issues: &mut Vec<Issue>,
) {
    let path = state.path_of(id);
    let d = state.node(id);
    let opaque = d.is_opaque();
    let t: Option<&NodeType> = if opaque {
        None
    } else {
        registry.node_type(&d.type_name)
    };
    let is_group = d.type_name == GROUP;

    let values = match t {
        Some(t) => parse_values(&t.values, &rec.values, &path, issues),
        None => {
            if !opaque && !rec.values.is_empty() {
                issue(issues, &path, "values on a node that holds none, dropped");
            }
            BTreeMap::new()
        }
    };

    let mut refs = BTreeMap::new();
    for (k, j) in &rec.refs {
        if let Some(t) = t
            && !t.refs.contains(k)
        {
            issue(
                issues,
                &path,
                format!("unknown reference key `{k}`, dropped"),
            );
            continue;
        }
        if t.is_none() && !opaque {
            issue(
                issues,
                &path,
                format!("reference `{k}` on a node that holds none, dropped"),
            );
            continue;
        }
        let Ok(mut r) = serde_json::from_value::<Ref>(j.clone()) else {
            issue(
                issues,
                &path,
                format!("unreadable reference `{k}`, dropped"),
            );
            continue;
        };
        if r.is_here() {
            r.path = None;
            if let Some(old) = r.node {
                r.node = Some(remap.get(&old).copied().unwrap_or(old));
            }
        }
        if r.file.is_none() && r.node.is_none() {
            issue(issues, &path, format!("empty reference `{k}`, dropped"));
            continue;
        }
        refs.insert(k.clone(), r);
    }

    let mut orders = BTreeMap::new();
    for (name, ids) in &rec.orders {
        if !valid_name(name) {
            issue(
                issues,
                &path,
                format!("invalid order name `{name}`, dropped"),
            );
            continue;
        }
        let mut list = Vec::new();
        for s in ids {
            match parse_id(s, remap) {
                Some(c) if state.node(c).parent == Some(id) && !list.contains(&c) => list.push(c),
                _ => issue(
                    issues,
                    &path,
                    format!("order `{name}`: `{s}` is not a child here, dropped"),
                ),
            }
        }
        if !list.is_empty() {
            orders.insert(name.clone(), list);
        }
    }

    let mut members = BTreeSet::new();
    if !rec.members.is_empty() && !is_group {
        issue(
            issues,
            &path,
            "members on a node that isn't a group, dropped",
        );
    } else {
        for s in &rec.members {
            match parse_id(s, remap) {
                Some(m) if m != id && m != state.root => {
                    members.insert(m);
                }
                _ => issue(issues, &path, format!("member `{s}` is missing, dropped")),
            }
        }
    }

    let mut bindings = BTreeMap::new();
    for j in &rec.bindings {
        let on = j
            .get("on")
            .and_then(|o| serde_json::from_value::<On>(o.clone()).ok());
        let source = j.get("source").and_then(Json::as_str);
        let (Some(on), Some(source)) = (on, source) else {
            issue(issues, &path, "unreadable binding, dropped");
            continue;
        };
        let Some(src) = parse_id(source, remap).filter(|s| *s != id) else {
            issue(
                issues,
                &path,
                format!("{on}: source `{source}` is missing, dropped"),
            );
            continue;
        };
        if t.is_none() && !opaque {
            issue(
                issues,
                &path,
                format!("{on}: binding on a node that takes none, dropped"),
            );
            continue;
        }
        if let Some(t) = t {
            let fits = match &on {
                On::Slot(s) => t.slots.contains(s),
                On::Value(k) => t.spec(k).is_some(),
            };
            let slot_taken = matches!(on, On::Slot(_)) && bindings.keys().any(|(o, _)| *o == on);
            if !fits || slot_taken {
                issue(
                    issues,
                    &path,
                    format!("{on}: not on this type or already bound, dropped"),
                );
                continue;
            }
        }
        let raw: BTreeMap<String, Json> = j
            .get("values")
            .and_then(Json::as_object)
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();
        let sd = state.node(src);
        let data = if sd.is_opaque() || opaque {
            BindingData {
                values: BTreeMap::new(),
                raw: Some(raw),
            }
        } else {
            let Some(specs) = registry
                .node_type(&sd.type_name)
                .and_then(|st| st.bindable.as_ref())
            else {
                issue(
                    issues,
                    &path,
                    format!("{on}: source can't be bound, dropped"),
                );
                continue;
            };
            BindingData {
                values: parse_values(specs, &raw, &path, issues),
                raw: None,
            }
        };
        bindings.insert((on, src), data);
    }

    let d = state.node_mut(id);
    if !opaque {
        d.values = values;
    }
    d.refs = refs;
    d.orders = orders;
    d.members = members;
    d.bindings = bindings;
}

fn unresolved(state: &State, ids: impl IntoIterator<Item = NodeId>, issues: &mut Vec<Issue>) {
    for id in ids {
        let d = state.node(id);
        for (k, r) in &d.refs {
            if r.is_here() && r.node.is_some_and(|n| !state.contains(n)) {
                issue(
                    issues,
                    state.path_of(id),
                    format!("reference `{k}` points at a missing node"),
                );
            }
        }
    }
}

pub(crate) fn load(
    text: &str,
    registry: &Registry,
    ids: &mut IdSource,
) -> Result<(State, LoadReport)> {
    let doc = read_doc(text)?;
    if doc.get("fragment").is_some() {
        return Err(Error::Format("this is a fragment, not a file".into()));
    }
    let nodes = doc
        .get("nodes")
        .and_then(Json::as_array)
        .ok_or_else(|| Error::Format("no `nodes` list".into()))?;
    let mut issues = Vec::new();
    let parsed = parse_records(nodes, &mut issues);
    for p in &parsed {
        ids.skip_past(p.id);
    }

    let root = match parsed.iter().find(|p| p.path.is_root()) {
        Some(p) if p.rec.type_name == ROOT => p.id,
        _ => {
            issue(&mut issues, "/", "no root node, made one");
            ids.next()
        }
    };
    let mut state = State::new(root);
    let mut by_path: BTreeMap<Path, NodeId> = BTreeMap::from([(Path::root(), root)]);
    let mut placed: Vec<&Parsed> = Vec::new();

    for p in parsed.iter().filter(|p| !p.path.is_root()) {
        let Some(&parent) = p.path.parent().and_then(|pp| by_path.get(&pp)) else {
            issue(&mut issues, &p.path, "parent is missing, dropped");
            continue;
        };
        if p.id == root || state.contains(p.id) {
            issue(&mut issues, &p.path, "duplicate id, dropped");
            continue;
        }
        let tn = p.rec.type_name.as_str();
        let name = p.path.name();
        let opaque = if parent == root {
            if tn != CATEGORY {
                issue(&mut issues, &p.path, "only categories live at /, dropped");
                continue;
            }
            let unknown = registry.category(name).is_none();
            if unknown {
                issue(&mut issues, &p.path, "unknown category, kept as is");
            }
            unknown
        } else if tn == ROOT || tn == CATEGORY {
            issue(
                &mut issues,
                &p.path,
                format!("a {tn} can't live here, dropped"),
            );
            continue;
        } else if state.node(parent).type_name == GROUP {
            issue(
                &mut issues,
                &p.path,
                "groups hold members, not children, dropped",
            );
            continue;
        } else if tn == GROUP {
            false
        } else if let Some(t) = registry.node_type(tn) {
            let cat = &state
                .node(state.category_of(parent).expect("below a category"))
                .name;
            let misplaced = !t.categories.is_empty() && !t.categories.contains(cat);
            if misplaced {
                issue(
                    &mut issues,
                    &p.path,
                    format!("`{tn}` can't live in `{cat}`, kept as is"),
                );
            }
            misplaced
        } else {
            issue(
                &mut issues,
                &p.path,
                format!("unknown type `{tn}`, kept as is"),
            );
            true
        };
        let mut d = NodeData::new(p.id, name, Some(parent), tn);
        if opaque {
            d.raw = Some(p.rec.values.clone());
        }
        state.nodes.insert(p.id, d);
        state
            .node_mut(parent)
            .children
            .insert(name.to_string(), p.id);
        by_path.insert(p.path.clone(), p.id);
        placed.push(p);
    }

    let identity: BTreeMap<NodeId, NodeId> = state.nodes.keys().map(|id| (*id, *id)).collect();
    for p in &placed {
        fill(&mut state, registry, p.id, &p.rec, &identity, &mut issues);
    }
    for (name, _) in registry.categories() {
        if !state.node(root).children.contains_key(name) {
            let id = ids.next();
            state
                .nodes
                .insert(id, NodeData::new(id, name, Some(root), CATEGORY));
            state.node_mut(root).children.insert(name.to_string(), id);
        }
    }
    unresolved(&state, placed.iter().map(|p| p.id), &mut issues);
    sort_issues(&mut issues);
    Ok((state, LoadReport { issues }))
}

// ---- fragments ----

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct FragmentInfo {
    /// The root id of the file it came from, so paste can tell "same file".
    source: NodeId,
    /// Each top node's parent in the source.
    parents: BTreeMap<NodeId, NodeId>,
    /// Groups outside the fragment that its nodes belong to.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    groups: BTreeMap<NodeId, Vec<NodeId>>,
    /// Paths of nodes outside the fragment that it points at, for lookup in another file.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    external: BTreeMap<NodeId, Path>,
}

/// A detached subtree in the file format: what copy, paste and the clipboard carry.
#[derive(Clone, Debug, PartialEq)]
pub struct Fragment {
    info: FragmentInfo,
    nodes: Vec<Record>,
}

impl PartialEq for Record {
    fn eq(&self, other: &Self) -> bool {
        serde_json::to_value(self).ok() == serde_json::to_value(other).ok()
    }
}

impl Fragment {
    pub fn to_text(&self) -> String {
        to_text(Some(&self.info), &self.nodes)
    }

    pub fn from_text(text: &str) -> Result<Fragment> {
        let doc = read_doc(text)?;
        let info = doc
            .get("fragment")
            .cloned()
            .ok_or_else(|| Error::Format("this is a file, not a fragment".into()))?;
        let info: FragmentInfo =
            serde_json::from_value(info).map_err(|e| Error::Format(e.to_string()))?;
        let nodes = doc
            .get("nodes")
            .cloned()
            .ok_or_else(|| Error::Format("no `nodes` list".into()))?;
        let nodes: Vec<Record> =
            serde_json::from_value(nodes).map_err(|e| Error::Format(e.to_string()))?;
        Ok(Fragment { info, nodes })
    }

    /// How many nodes it holds, top nodes and their subtrees together.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

pub(crate) fn extract(state: &State, tops: Vec<NodeId>) -> Result<Fragment> {
    let paths = state.all_paths();
    let mut tops = tops;
    tops.sort_by_key(|id| &paths[id]);
    tops.dedup();
    let mut kept: Vec<NodeId> = Vec::new();
    for t in tops {
        if t == state.root || state.is_category(t) {
            return Err(Error::Structural(format!(
                "{} is the root or a category; copy what's in it",
                paths[&t]
            )));
        }
        if !kept.iter().any(|k| paths[&t].is_within(&paths[k])) {
            kept.push(t);
        }
    }
    let inside: BTreeSet<NodeId> = kept.iter().flat_map(|t| state.subtree(*t)).collect();
    let parents = kept
        .iter()
        .map(|t| (*t, state.node(*t).parent.expect("not root")))
        .collect();
    let mut groups: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for d in state.nodes.values().filter(|d| !inside.contains(&d.id)) {
        for m in d.members.intersection(&inside) {
            groups.entry(*m).or_default().push(d.id);
        }
    }
    let mut external = BTreeMap::new();
    for id in &inside {
        for p in state.node(*id).pointers() {
            if !inside.contains(&p) && state.contains(p) {
                external.insert(p, paths[&p].clone());
            }
        }
    }
    for gs in groups.values() {
        for g in gs {
            external.insert(*g, paths[g].clone());
        }
    }
    Ok(Fragment {
        info: FragmentInfo {
            source: state.root,
            parents,
            groups,
            external,
        },
        nodes: records(state, &inside),
    })
}

pub(crate) fn paste(tx: &mut Edit<'_>, parent: At, fragment: &Fragment) -> Result<PasteReport> {
    let parent = tx.resolve(parent)?;
    let mut issues = Vec::new();
    let recs = fragment
        .nodes
        .iter()
        .map(|r| (r.path.clone(), r.clone()))
        .collect();
    let parsed = check_records(recs, &mut issues);
    let paths_in: BTreeSet<Path> = parsed.iter().map(|p| p.path.clone()).collect();
    let same_file = fragment.info.source == tx.state.root;

    let mut new_of: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    let mut by_old_path: BTreeMap<Path, NodeId> = BTreeMap::new();
    let mut tops: Vec<(NodeId, NodeId)> = Vec::new();
    let mut placed: Vec<(&Parsed, NodeId)> = Vec::new();

    for p in &parsed {
        let old_parent = p.path.parent().unwrap_or_else(Path::root);
        let (target, name, top) = match by_old_path.get(&old_parent) {
            Some(np) => (*np, p.path.name().to_string(), false),
            None if paths_in.contains(&old_parent) => {
                issue(&mut issues, &p.path, "parent was dropped, dropped");
                continue;
            }
            None => (parent, tx.unique_name(parent, p.path.name()), true),
        };
        let tn = p.rec.type_name.as_str();
        let known = tn == GROUP || tx.registry.node_type(tn).is_some();
        if known || tn == ROOT || tn == CATEGORY {
            tx.check_can_hold(target, tn)?;
        } else {
            tx.check_parent(target)?;
            issue(
                &mut issues,
                &p.path,
                format!("unknown type `{tn}`, kept as is"),
            );
        }
        let id = tx.insert_node(target, tn, &name);
        if !known {
            tx.state.node_mut(id).raw = Some(p.rec.values.clone());
        }
        new_of.insert(p.id, id);
        by_old_path.insert(p.path.clone(), id);
        if top {
            tops.push((p.id, id));
        }
        placed.push((p, id));
    }

    // Inside the fragment, pointers go to the copies. Outside, to the same node in this file,
    // or, from another file, to a node found by id and then by path.
    let mut remap = new_of.clone();
    for (p, _) in &placed {
        let r = &p.rec;
        let here_refs = r
            .refs
            .values()
            .filter_map(|j| serde_json::from_value::<Ref>(j.clone()).ok())
            .filter(|r| r.is_here())
            .filter_map(|r| r.node);
        let listed = r
            .members
            .iter()
            .map(String::as_str)
            .chain(
                r.bindings
                    .iter()
                    .filter_map(|b| b.get("source").and_then(Json::as_str)),
            )
            .filter_map(|s| s.parse::<NodeId>().ok());
        for old in here_refs.chain(listed) {
            if remap.contains_key(&old) {
                continue;
            }
            if tx.state.contains(old) {
                remap.insert(old, old);
            } else if !same_file
                && let Some(found) = fragment
                    .info
                    .external
                    .get(&old)
                    .and_then(|path| tx.state.find(path))
            {
                remap.insert(old, found);
            }
        }
    }
    for (p, id) in &placed {
        fill(tx.state, tx.registry, *id, &p.rec, &remap, &mut issues);
    }
    unresolved(tx.state, placed.iter().map(|(_, id)| *id), &mut issues);

    for (old, new) in &new_of {
        for g in fragment.info.groups.get(old).into_iter().flatten() {
            let joinable = same_file
                && tx
                    .state
                    .nodes
                    .get(g)
                    .is_some_and(|d| d.type_name == GROUP && !d.is_opaque());
            if joinable {
                tx.state.node_mut(*g).members.insert(*new);
            } else {
                let name = fragment
                    .info
                    .external
                    .get(g)
                    .map_or_else(|| g.to_string(), |p| p.to_string());
                issue(
                    &mut issues,
                    tx.state.path_of(*new),
                    format!("not added to group {name}"),
                );
            }
        }
    }

    for (old_top, new_top) in &tops {
        let beside = same_file && fragment.info.parents.get(old_top) == Some(&parent);
        let p = tx.state.node_mut(parent);
        for order in p.orders.values_mut() {
            match order.iter().position(|x| x == old_top) {
                Some(i) if beside => order.insert(i + 1, *new_top),
                _ => order.push(*new_top),
            }
        }
    }

    sort_issues(&mut issues);
    Ok(PasteReport {
        nodes: tops.into_iter().map(|(_, n)| n).collect(),
        issues,
    })
}
