use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::id::NodeId;
use crate::path::Path;
use crate::registry::{CATEGORY, ROOT};
use crate::value::Value;

/// What a binding attaches to on its target: a named slot, or a value key.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum On {
    Slot(String),
    Value(String),
}

impl On {
    pub fn slot(name: &str) -> On {
        On::Slot(name.to_string())
    }

    pub fn value(key: impl crate::value::KeyName) -> On {
        On::Value(key.key_name().to_string())
    }
}

impl fmt::Display for On {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            On::Slot(s) => write!(f, "slot {s}"),
            On::Value(k) => write!(f, "value {k}"),
        }
    }
}

/// A reference: to a file (a sample, an image), or to a node.
///
/// A node reference with no `file` points into this file and resolves by id. Its path is
/// written to disk for people to read and ignored on load. A reference whose target is gone
/// stays in place and reads as unresolved.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct Ref {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<NodeId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<Path>,
}

impl Ref {
    /// A node in this file.
    pub fn here(id: NodeId) -> Ref {
        Ref {
            file: None,
            node: Some(id),
            path: None,
        }
    }

    /// A file on disk, relative to the referring file.
    pub fn file(path: impl Into<String>) -> Ref {
        Ref {
            file: Some(path.into()),
            node: None,
            path: None,
        }
    }

    /// A node in another file. Stored, not resolved, in the first slice.
    pub fn node_in(file: impl Into<String>, id: NodeId, path: Path) -> Ref {
        Ref {
            file: Some(file.into()),
            node: Some(id),
            path: Some(path),
        }
    }

    pub fn is_here(&self) -> bool {
        self.file.is_none()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BindingData {
    pub values: BTreeMap<String, Value>,
    /// Kept as read when the source's type is unknown.
    pub raw: Option<BTreeMap<String, Json>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NodeData {
    pub id: NodeId,
    pub name: String,
    pub parent: Option<NodeId>,
    pub type_name: String,
    pub children: BTreeMap<String, NodeId>,
    pub values: BTreeMap<String, Value>,
    /// `Some` marks an opaque node: its type is unknown and its values are kept as read.
    pub raw: Option<BTreeMap<String, Json>>,
    pub refs: BTreeMap<String, Ref>,
    pub orders: BTreeMap<String, Vec<NodeId>>,
    pub members: BTreeSet<NodeId>,
    /// Bindings with this node as target, keyed by what they attach to and their source.
    pub bindings: BTreeMap<(On, NodeId), BindingData>,
}

impl NodeData {
    pub fn new(id: NodeId, name: &str, parent: Option<NodeId>, type_name: &str) -> Self {
        NodeData {
            id,
            name: name.to_string(),
            parent,
            type_name: type_name.to_string(),
            children: BTreeMap::new(),
            values: BTreeMap::new(),
            raw: None,
            refs: BTreeMap::new(),
            orders: BTreeMap::new(),
            members: BTreeSet::new(),
            bindings: BTreeMap::new(),
        }
    }

    pub fn is_opaque(&self) -> bool {
        self.raw.is_some()
    }

    /// Every node id this node points at: here-refs, orders, members, binding sources.
    pub fn pointers(&self) -> impl Iterator<Item = NodeId> + '_ {
        let refs = self
            .refs
            .values()
            .filter(|r| r.is_here())
            .filter_map(|r| r.node);
        let orders = self.orders.values().flatten().copied();
        let members = self.members.iter().copied();
        let sources = self.bindings.keys().map(|(_, s)| *s);
        refs.chain(orders).chain(members).chain(sources)
    }
}

/// One file's nodes. Cloned for every undo step and every snapshot.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct State {
    pub root: NodeId,
    pub nodes: BTreeMap<NodeId, NodeData>,
}

impl State {
    pub fn new(root: NodeId) -> State {
        let mut nodes = BTreeMap::new();
        nodes.insert(root, NodeData::new(root, "", None, ROOT));
        State { root, nodes }
    }

    pub fn node(&self, id: NodeId) -> &NodeData {
        &self.nodes[&id]
    }

    pub fn node_mut(&mut self, id: NodeId) -> &mut NodeData {
        self.nodes.get_mut(&id).expect("node exists")
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn path_of(&self, id: NodeId) -> Path {
        let mut names = Vec::new();
        let mut at = self.node(id);
        while let Some(p) = at.parent {
            names.push(at.name.as_str());
            at = self.node(p);
        }
        let mut path = Path::root();
        for n in names.iter().rev() {
            path = path.join(n);
        }
        path
    }

    pub fn find(&self, path: &Path) -> Option<NodeId> {
        let mut at = self.root;
        for seg in path.segments() {
            at = *self.node(at).children.get(seg)?;
        }
        Some(at)
    }

    /// The node and everything under it, parents before children, siblings by name.
    pub fn subtree(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = vec![id];
        let mut i = 0;
        while i < out.len() {
            out.extend(self.node(out[i]).children.values().copied());
            i += 1;
        }
        out
    }

    /// The category node a node lives in: its top-level ancestor. `None` for the root.
    pub fn category_of(&self, id: NodeId) -> Option<NodeId> {
        let mut at = id;
        loop {
            let parent = self.node(at).parent?;
            if parent == self.root {
                return Some(at);
            }
            at = parent;
        }
    }

    pub fn is_category(&self, id: NodeId) -> bool {
        self.node(id).parent == Some(self.root) && self.node(id).type_name == CATEGORY
    }

    /// Every node's path, from one walk of the tree.
    pub fn all_paths(&self) -> BTreeMap<NodeId, Path> {
        let mut out = BTreeMap::new();
        let mut stack = vec![(self.root, Path::root())];
        while let Some((id, path)) = stack.pop() {
            for (name, child) in &self.node(id).children {
                stack.push((*child, path.join(name)));
            }
            out.insert(id, path);
        }
        out
    }

    /// Drops every pointer to the given ids: orders, members and bindings. Refs stay, unresolved.
    pub fn forget(&mut self, gone: &BTreeSet<NodeId>) {
        for d in self.nodes.values_mut() {
            for order in d.orders.values_mut() {
                order.retain(|id| !gone.contains(id));
            }
            d.orders.retain(|_, o| !o.is_empty());
            d.members.retain(|id| !gone.contains(id));
            d.bindings.retain(|(_, src), _| !gone.contains(src));
        }
    }
}
