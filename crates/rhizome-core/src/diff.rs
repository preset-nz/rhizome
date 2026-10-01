use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::id::NodeId;
use crate::path::Path;
use crate::state::{BindingData, On, Ref, State};
use crate::value::Value;

/// What changed between two states: a flat, path-ordered list. Computed, never stored.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Changeset {
    pub entries: Vec<Change>,
    /// The path of every node the entries mention, for display. The newer state wins.
    paths: BTreeMap<NodeId, Path>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub path: Path,
    pub id: NodeId,
    pub kind: ChangeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChangeKind {
    Added {
        type_name: String,
    },
    Removed,
    /// A rename or a reparent. Identity is the id, so this is never a remove plus an add.
    Moved {
        from: Path,
    },
    Value {
        key: String,
        from: Option<Value>,
        to: Option<Value>,
    },
    Ref {
        key: String,
        from: Option<Ref>,
        to: Option<Ref>,
    },
    /// A binding onto this node appeared, went, or changed its values.
    Bound {
        on: On,
        source: NodeId,
        from: Option<BTreeMap<String, Value>>,
        to: Option<BTreeMap<String, Value>>,
    },
    /// One entry for a whole reorder, never a run of moves.
    Reordered {
        order: String,
        from: Vec<NodeId>,
        to: Vec<NodeId>,
    },
    Membership {
        added: Vec<NodeId>,
        removed: Vec<NodeId>,
    },
}

impl ChangeKind {
    fn rank(&self) -> u8 {
        match self {
            ChangeKind::Removed => 0,
            ChangeKind::Added { .. } => 1,
            ChangeKind::Moved { .. } => 2,
            ChangeKind::Value { .. } => 3,
            ChangeKind::Ref { .. } => 4,
            ChangeKind::Bound { .. } => 5,
            ChangeKind::Reordered { .. } => 6,
            ChangeKind::Membership { .. } => 7,
        }
    }
}

impl Changeset {
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Change> {
        self.entries.iter()
    }

    /// The path of a node the changeset mentions.
    pub fn path_of(&self, id: NodeId) -> Option<&Path> {
        self.paths.get(&id)
    }

    fn name(&self, id: NodeId) -> String {
        self.paths
            .get(&id)
            .map(|p| p.to_string())
            .unwrap_or_else(|| format!("#{id}"))
    }

    fn short(&self, id: NodeId) -> String {
        self.paths
            .get(&id)
            .map(|p| p.name().to_string())
            .unwrap_or_else(|| format!("#{id}"))
    }

    fn reference(&self, r: &Option<Ref>) -> String {
        match r {
            None => "unset".into(),
            Some(r) => match (&r.file, r.node) {
                (Some(f), None) => format!("file:{f}"),
                (Some(f), Some(_)) => format!(
                    "file:{f}#{}",
                    r.path.as_ref().map(|p| p.to_string()).unwrap_or_default()
                ),
                (None, Some(id)) => self.name(id),
                (None, None) => "empty".into(),
            },
        }
    }

    /// One readable line per entry, as tests and logs show them.
    pub fn line(&self, c: &Change) -> String {
        let p = &c.path;
        let opt = |v: &Option<Value>| v.as_ref().map_or("unset".to_string(), |v| v.to_string());
        let bound = |v: &Option<BTreeMap<String, Value>>| match v {
            None => "unbound".to_string(),
            Some(m) if m.is_empty() => "bound".to_string(),
            Some(m) => {
                let parts: Vec<String> = m.iter().map(|(k, v)| format!("{k}={v}")).collect();
                format!("bound {{{}}}", parts.join(", "))
            }
        };
        let list = |ids: &[NodeId]| {
            let names: Vec<String> = ids.iter().map(|id| self.short(*id)).collect();
            format!("[{}]", names.join(", "))
        };
        match &c.kind {
            ChangeKind::Added { type_name } => format!("{p}  added {type_name}"),
            ChangeKind::Removed => format!("{p}  removed"),
            ChangeKind::Moved { from } => format!("{p}  moved from {from}"),
            ChangeKind::Value { key, from, to } => {
                format!("{p}  {key}  {} → {}", opt(from), opt(to))
            }
            ChangeKind::Ref { key, from, to } => {
                format!(
                    "{p}  {key}  {} → {}",
                    self.reference(from),
                    self.reference(to)
                )
            }
            ChangeKind::Bound {
                on,
                source,
                from,
                to,
            } => format!(
                "{p}  {on} ← {}  {} → {}",
                self.name(*source),
                bound(from),
                bound(to)
            ),
            ChangeKind::Reordered { order, from, to } => {
                format!("{p}  order {order}  {} → {}", list(from), list(to))
            }
            ChangeKind::Membership { added, removed } => {
                let mut parts: Vec<String> = added
                    .iter()
                    .map(|id| format!("+{}", self.name(*id)))
                    .collect();
                parts.extend(removed.iter().map(|id| format!("-{}", self.name(*id))));
                format!("{p}  members  {}", parts.join(" "))
            }
        }
    }
}

impl fmt::Display for Changeset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in &self.entries {
            writeln!(f, "{}", self.line(c))?;
        }
        Ok(())
    }
}

fn keys<'a, V>(a: &'a BTreeMap<String, V>, b: &'a BTreeMap<String, V>) -> BTreeSet<&'a String> {
    a.keys().chain(b.keys()).collect()
}

fn typed(b: Option<&BindingData>) -> Option<BTreeMap<String, Value>> {
    b.map(|b| b.values.clone())
}

pub(crate) fn diff(a: &State, b: &State) -> Changeset {
    let pa = a.all_paths();
    let pb = b.all_paths();
    let mut entries = Vec::new();
    let mut push = |path: &Path, id: NodeId, kind: ChangeKind| {
        entries.push(Change {
            path: path.clone(),
            id,
            kind,
        })
    };

    for (id, path) in &pa {
        if !b.contains(*id) {
            push(path, *id, ChangeKind::Removed);
        }
    }
    for (id, path) in &pb {
        let db = b.node(*id);
        let Some(da) = a.nodes.get(id) else {
            push(
                path,
                *id,
                ChangeKind::Added {
                    type_name: db.type_name.clone(),
                },
            );
            continue;
        };
        if da.parent != db.parent || da.name != db.name {
            push(
                path,
                *id,
                ChangeKind::Moved {
                    from: pa[id].clone(),
                },
            );
        }
        for key in keys(&da.values, &db.values) {
            let (from, to) = (da.values.get(key), db.values.get(key));
            if from != to {
                push(
                    path,
                    *id,
                    ChangeKind::Value {
                        key: key.clone(),
                        from: from.cloned(),
                        to: to.cloned(),
                    },
                );
            }
        }
        for key in keys(&da.refs, &db.refs) {
            let (from, to) = (da.refs.get(key), db.refs.get(key));
            if from != to {
                push(
                    path,
                    *id,
                    ChangeKind::Ref {
                        key: key.clone(),
                        from: from.cloned(),
                        to: to.cloned(),
                    },
                );
            }
        }
        let bindings: BTreeSet<&(On, NodeId)> =
            da.bindings.keys().chain(db.bindings.keys()).collect();
        for k in bindings {
            let (from, to) = (da.bindings.get(k), db.bindings.get(k));
            if from != to {
                push(
                    path,
                    *id,
                    ChangeKind::Bound {
                        on: k.0.clone(),
                        source: k.1,
                        from: typed(from),
                        to: typed(to),
                    },
                );
            }
        }
        for name in keys(&da.orders, &db.orders) {
            let (from, to) = (da.orders.get(name), db.orders.get(name));
            if from != to {
                push(
                    path,
                    *id,
                    ChangeKind::Reordered {
                        order: name.clone(),
                        from: from.cloned().unwrap_or_default(),
                        to: to.cloned().unwrap_or_default(),
                    },
                );
            }
        }
        let added: Vec<NodeId> = db.members.difference(&da.members).copied().collect();
        let removed: Vec<NodeId> = da.members.difference(&db.members).copied().collect();
        if !added.is_empty() || !removed.is_empty() {
            push(path, *id, ChangeKind::Membership { added, removed });
        }
        debug_assert_eq!(da.raw, db.raw, "opaque values never change");
    }

    entries.sort_by(|x, y| x.path.cmp(&y.path).then(x.kind.rank().cmp(&y.kind.rank())));
    let mut paths = pa;
    paths.extend(pb);
    Changeset { entries, paths }
}
