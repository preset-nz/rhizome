use std::collections::BTreeMap;
use std::fmt;

use crate::id::NodeId;
use crate::path::Path;
use crate::registry::{GROUP, NodeType, Origin, ROOT, Registry};
use crate::state::{BindingData, NodeData, On, Ref, State};
use crate::value::{Key, Value, ValueType};

/// A read-only view of one node. Borrowed from the tree, so it can't outlive the next edit.
#[derive(Clone, Copy)]
pub struct Node<'a> {
    pub(crate) state: &'a State,
    pub(crate) registry: &'a Registry,
    pub(crate) data: &'a NodeData,
}

impl fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Node({} {})", self.path(), self.data.type_name)
    }
}

impl<'a> Node<'a> {
    pub(crate) fn new(state: &'a State, registry: &'a Registry, id: NodeId) -> Option<Node<'a>> {
        let data = state.nodes.get(&id)?;
        Some(Node {
            state,
            registry,
            data,
        })
    }

    fn wrap(&self, id: NodeId) -> Node<'a> {
        Node::new(self.state, self.registry, id).expect("pointer to a live node")
    }

    fn by_path(&self, mut nodes: Vec<Node<'a>>) -> Vec<Node<'a>> {
        nodes.sort_by_key(|n| n.path());
        nodes
    }

    pub fn id(&self) -> NodeId {
        self.data.id
    }

    pub fn name(&self) -> &'a str {
        &self.data.name
    }

    pub fn path(&self) -> Path {
        self.state.path_of(self.data.id)
    }

    pub fn type_name(&self) -> &'a str {
        &self.data.type_name
    }

    /// The declared type. `None` for the root, categories, groups and opaque nodes.
    pub fn node_type(&self) -> Option<&'a NodeType> {
        if self.is_opaque() {
            return None;
        }
        self.registry.node_type(&self.data.type_name)
    }

    /// True for a node whose type this app doesn't know. It passes through unchanged.
    pub fn is_opaque(&self) -> bool {
        self.data.is_opaque()
    }

    pub fn is_root(&self) -> bool {
        self.data.type_name == ROOT && self.data.parent.is_none()
    }

    pub fn is_category(&self) -> bool {
        self.state.is_category(self.data.id)
    }

    pub fn is_group(&self) -> bool {
        self.data.type_name == GROUP
    }

    /// The category this node lives in. A category node is its own.
    pub fn category(&self) -> Option<Node<'a>> {
        self.state.category_of(self.data.id).map(|id| self.wrap(id))
    }

    pub fn origin(&self) -> Option<Origin> {
        self.registry.category(self.category()?.name())
    }

    pub fn parent(&self) -> Option<Node<'a>> {
        self.data.parent.map(|id| self.wrap(id))
    }

    /// Children by name. Hierarchy never implies order; use [`Node::order`] for that.
    pub fn children(&self) -> impl Iterator<Item = Node<'a>> + 'a {
        let (state, registry) = (self.state, self.registry);
        self.data
            .children
            .values()
            .map(move |id| Node::new(state, registry, *id).expect("child"))
    }

    pub fn child(&self, name: &str) -> Option<Node<'a>> {
        self.data.children.get(name).map(|id| self.wrap(*id))
    }

    /// A typed value, with the schema default when unset. `None` when the key isn't in the schema.
    pub fn get<T: ValueType>(&self, key: Key<T>) -> Option<T> {
        T::from_value(&self.value(key.name())?)
    }

    /// A value, with the schema default when unset.
    pub fn value(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.data.values.get(key) {
            return Some(v.clone());
        }
        Some(self.node_type()?.spec(key)?.default.clone())
    }

    /// Whether the value is stored, rather than read from the default.
    pub fn is_set(&self, key: &str) -> bool {
        self.data.values.contains_key(key)
    }

    pub fn stored_values(&self) -> impl Iterator<Item = (&'a str, &'a Value)> + 'a {
        self.data.values.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn reference(&self, key: &str) -> Option<&'a Ref> {
        self.data.refs.get(key)
    }

    /// The node a here-reference points at, if it still exists.
    pub fn resolve(&self, key: &str) -> Option<Node<'a>> {
        let r = self.reference(key)?;
        if !r.is_here() {
            return None;
        }
        Node::new(self.state, self.registry, r.node?)
    }

    pub fn order(&self, name: &str) -> Vec<Node<'a>> {
        self.data
            .orders
            .get(name)
            .map(|ids| ids.iter().map(|id| self.wrap(*id)).collect())
            .unwrap_or_default()
    }

    pub fn order_names(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.data.orders.keys().map(String::as_str)
    }

    /// A group's members, in path order.
    pub fn members(&self) -> Vec<Node<'a>> {
        self.by_path(self.data.members.iter().map(|id| self.wrap(*id)).collect())
    }

    /// The groups this node is in, in path order.
    pub fn groups(&self) -> Vec<Node<'a>> {
        let id = self.data.id;
        let groups = self
            .state
            .nodes
            .values()
            .filter(|d| d.members.contains(&id));
        self.by_path(groups.map(|d| self.wrap(d.id)).collect())
    }

    /// Bindings with this node as target.
    pub fn bindings(&self) -> Vec<Binding<'a>> {
        self.data
            .bindings
            .iter()
            .map(|((on, src), b)| self.binding(self.data.id, on, *src, b))
            .collect()
    }

    /// Bindings with this node as source, in target path order.
    pub fn bound_to(&self) -> Vec<Binding<'a>> {
        let id = self.data.id;
        let mut out: Vec<Binding<'a>> = Vec::new();
        for d in self.state.nodes.values() {
            for ((on, src), b) in &d.bindings {
                if *src == id {
                    out.push(self.binding(d.id, on, id, b));
                }
            }
        }
        out.sort_by_key(|b| b.target.path());
        out
    }

    fn binding(
        &self,
        target: NodeId,
        on: &'a On,
        source: NodeId,
        b: &'a BindingData,
    ) -> Binding<'a> {
        Binding {
            target: self.wrap(target),
            on,
            source: self.wrap(source),
            values: &b.values,
        }
    }

    /// Nodes holding a here-reference to this node, with the key, in path order.
    pub fn referrers(&self) -> Vec<(Node<'a>, &'a str)> {
        let id = self.data.id;
        let mut out = Vec::new();
        for d in self.state.nodes.values() {
            for (k, r) in &d.refs {
                if r.is_here() && r.node == Some(id) {
                    out.push((self.wrap(d.id), k.as_str()));
                }
            }
        }
        out.sort_by_key(|(n, _)| n.path());
        out
    }
}

/// A binding from a source node onto a target's slot or value key.
#[derive(Clone, Copy, Debug)]
pub struct Binding<'a> {
    pub target: Node<'a>,
    pub on: &'a On,
    pub source: Node<'a>,
    values: &'a BTreeMap<String, Value>,
}

impl Binding<'_> {
    /// A value the binding carries, with the source type's default when unset.
    pub fn value(&self, key: &str) -> Option<Value> {
        if let Some(v) = self.values.get(key) {
            return Some(v.clone());
        }
        let specs = self.source.node_type()?.binding_values()?;
        Some(specs.iter().find(|s| s.key == key)?.default.clone())
    }
}
