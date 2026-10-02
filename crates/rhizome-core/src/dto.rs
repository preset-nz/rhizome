//! The tree as plain data for a mirror on the other side of a process boundary (decision 48):
//! a [`Row`] per node, the [`Schema`] once, and after each commit a [`Patch`] of the rows it
//! touched. A mirror holds rows by id, replaces what a patch sends and drops what it removes;
//! it never re-implements a tree mechanic. Pinned in `tests/golden/view.json`.
//!
//! A row holds resolved values (defaults filled in) and says which were actually set. It
//! names its parent by id and its references by id only, so a rename touches no other row.
//! Children are the rows whose `parent` is this one.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::Value as Json;

use crate::diff::{ChangeKind, Changeset};
use crate::id::NodeId;
use crate::path::Path;
use crate::registry::{Origin, Registry, ValueSpec};
use crate::shape::Shape;
use crate::state::{On, Ref};
use crate::tree::Tree;
use crate::value::ValueKind;
use crate::view::Node;

/// What a node is to the tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Root,
    Category,
    Group,
    Node,
    /// A type this build doesn't know: carried, never written.
    Opaque,
}

/// One node as plain data.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub id: NodeId,
    pub path: Path,
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub role: Role,
    pub parent: Option<NodeId>,
    /// Every value in the schema, resolved: what the node reads as now.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Json>,
    /// The keys actually stored; the rest are defaults.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub set: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub refs: BTreeMap<String, RefRow>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<BindingRow>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub orders: BTreeMap<String, Vec<NodeId>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<NodeId>,
}

/// A reference by id: to a node here, or to a file, or a node in another file.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RefRow {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeId>,
}

/// A binding onto this node, with the values it carries resolved.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BindingRow {
    pub on: On,
    pub source: NodeId,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Json>,
}

/// What a commit did to a mirror: the fresh rows of every node it touched, and the ids it
/// removed.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Patch {
    pub rows: Vec<Row>,
    pub removed: Vec<NodeId>,
}

/// The registry as plain data: what a mirror needs to show and check values.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Schema {
    pub categories: Vec<CategorySchema>,
    pub types: Vec<TypeSchema>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CategorySchema {
    pub name: String,
    pub origin: Origin,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TypeSchema {
    pub name: String,
    /// Where it may live; empty means any category.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub categories: Vec<String>,
    /// In declaration order, which an app may draw in.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<ValueSchema>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<String>,
    /// The values a binding from this type carries, when it can be bound.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bindable: Option<Vec<ValueSchema>>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ValueSchema {
    pub key: String,
    pub kind: ValueKind,
    pub default: Json,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<(f64, f64)>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub len: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<Shape>,
}

fn value_schema(s: &ValueSpec) -> ValueSchema {
    ValueSchema {
        key: s.key.clone(),
        kind: s.kind,
        default: s.default.to_json(),
        range: s.range,
        choices: s.choices.clone(),
        len: s.len,
        shape: s.shape.clone(),
    }
}

impl Registry {
    /// The registry as plain data, types in name order.
    pub fn schema(&self) -> Schema {
        let mut types: Vec<TypeSchema> = self
            .node_types()
            .map(|t| TypeSchema {
                name: t.name.clone(),
                categories: t.categories.clone(),
                values: t.values.iter().map(value_schema).collect(),
                refs: t.refs.clone(),
                slots: t.slots.clone(),
                bindable: t
                    .bindable
                    .as_ref()
                    .map(|b| b.iter().map(value_schema).collect()),
            })
            .collect();
        types.sort_by(|a, b| a.name.cmp(&b.name));
        Schema {
            categories: self
                .categories()
                .map(|(name, origin)| CategorySchema {
                    name: name.to_string(),
                    origin,
                })
                .collect(),
            types,
        }
    }
}

fn ref_row(r: &Ref) -> RefRow {
    RefRow {
        file: r.file.clone(),
        node: r.node,
    }
}

/// One node as a row.
pub fn row(n: Node<'_>) -> Row {
    let role = if n.is_root() {
        Role::Root
    } else if n.is_category() {
        Role::Category
    } else if n.is_group() {
        Role::Group
    } else if n.is_opaque() {
        Role::Opaque
    } else {
        Role::Node
    };
    let mut values = BTreeMap::new();
    let mut set = Vec::new();
    let mut refs = BTreeMap::new();
    if let Some(t) = n.node_type() {
        for spec in &t.values {
            if let Some(v) = n.value(&spec.key) {
                values.insert(spec.key.clone(), v.to_json());
            }
            if n.is_set(&spec.key) {
                set.push(spec.key.clone());
            }
        }
        for key in &t.refs {
            if let Some(r) = n.reference(key) {
                refs.insert(key.clone(), ref_row(r));
            }
        }
    }
    let bindings = n
        .bindings()
        .iter()
        .map(|b| {
            let specs = b
                .source
                .node_type()
                .and_then(|t| t.binding_values())
                .unwrap_or(&[]);
            BindingRow {
                on: b.on.clone(),
                source: b.source.id(),
                values: specs
                    .iter()
                    .filter_map(|s| Some((s.key.clone(), b.value(&s.key)?.to_json())))
                    .collect(),
            }
        })
        .collect();
    let orders = n
        .order_names()
        .map(|name| {
            (
                name.to_string(),
                n.order(name).iter().map(|o| o.id()).collect(),
            )
        })
        .collect();
    Row {
        id: n.id(),
        path: n.path(),
        name: n.name().to_string(),
        type_name: n.type_name().to_string(),
        role,
        parent: n.parent().map(|p| p.id()),
        values,
        set,
        refs,
        bindings,
        orders,
        members: n.members().iter().map(|m| m.id()).collect(),
    }
}

impl Tree {
    /// Every node as a row, in path order: what a mirror starts from.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = self.nodes().into_iter().map(row).collect();
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        rows
    }

    /// What `changes` (a commit of this tree's, just made) did to a mirror. A node that moved
    /// takes its descendants' paths with it, so they're sent too.
    pub fn patch(&self, changes: &Changeset) -> Patch {
        let mut touched: Vec<NodeId> = Vec::new();
        let mut removed: Vec<NodeId> = Vec::new();
        for c in &changes.entries {
            match &c.kind {
                ChangeKind::Removed { .. } => removed.push(c.id),
                ChangeKind::Moved { .. } => {
                    if let Some(n) = self.get(c.id) {
                        let mut stack = vec![n];
                        while let Some(n) = stack.pop() {
                            touched.push(n.id());
                            stack.extend(n.children());
                        }
                    }
                }
                _ => touched.push(c.id),
            }
        }
        touched.sort();
        touched.dedup();
        removed.sort();
        removed.dedup();
        let mut rows: Vec<Row> = touched
            .into_iter()
            .filter(|id| !removed.contains(id))
            .filter_map(|id| self.get(id).map(row))
            .collect();
        rows.sort_by(|a, b| a.path.cmp(&b.path));
        Patch { rows, removed }
    }
}
