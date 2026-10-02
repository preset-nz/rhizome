use std::collections::{BTreeMap, BTreeSet};

use crate::diff::Changeset;
use crate::error::{Error, Result};
use crate::file::{self, Fragment, PasteReport};
use crate::id::{IdSource, NodeId};
use crate::path::{Path, valid_name};
use crate::registry::{CATEGORY, GROUP, NodeType, Problem, ROOT, Registry, ValueSpec};
use crate::state::{BindingData, NodeData, On, Ref, State};
use crate::value::{Key, KeyName, Value, ValueType};
use crate::view::Node;

/// Where a verb points: a [`NodeId`], a [`Path`], or text that parses as either.
#[derive(Clone, Debug)]
pub enum At {
    Id(NodeId),
    Path(Path),
    Text(String),
}

impl From<NodeId> for At {
    fn from(id: NodeId) -> Self {
        At::Id(id)
    }
}

impl From<&NodeId> for At {
    fn from(id: &NodeId) -> Self {
        At::Id(*id)
    }
}

impl From<Path> for At {
    fn from(p: Path) -> Self {
        At::Path(p)
    }
}

impl From<&Path> for At {
    fn from(p: &Path) -> Self {
        At::Path(p.clone())
    }
}

impl From<&str> for At {
    fn from(s: &str) -> Self {
        At::Text(s.to_string())
    }
}

impl From<String> for At {
    fn from(s: String) -> Self {
        At::Text(s)
    }
}

impl From<&String> for At {
    fn from(s: &String) -> Self {
        At::Text(s.clone())
    }
}

impl From<Node<'_>> for At {
    fn from(n: Node<'_>) -> Self {
        At::Id(n.id())
    }
}

pub(crate) fn resolve(state: &State, at: At) -> Result<NodeId> {
    let found = match &at {
        At::Id(id) => state.contains(*id).then_some(*id),
        At::Path(p) => state.find(p),
        At::Text(s) if s.starts_with('/') => state.find(&Path::parse(s)?),
        At::Text(s) => {
            let id: NodeId = s.parse()?;
            state.contains(id).then_some(id)
        }
    };
    found.ok_or_else(|| {
        Error::NotFound(match at {
            At::Id(id) => id.to_string(),
            At::Path(p) => p.to_string(),
            At::Text(s) => s,
        })
    })
}

pub(crate) fn problem_error(p: Problem, path: String, spec: &ValueSpec, v: &Value) -> Error {
    let key = spec.key.clone();
    match p {
        Problem::WrongKind => Error::WrongKind {
            path,
            key,
            expected: spec.kind,
            got: format!("{:?}", v.kind()),
        },
        Problem::NotFinite => Error::NotFinite { path, key },
        Problem::OutOfRange { min, max } => Error::OutOfRange {
            path,
            key,
            value: v.to_string(),
            min,
            max,
        },
        Problem::NotAChoice => Error::NotAChoice {
            path,
            key,
            value: v.to_string(),
        },
    }
}

/// Runs every node type's `check`, then every tree rule, over the tree after an edit.
/// A failure rolls the edit back.
pub(crate) fn run_checks(state: &State, registry: &Registry, changes: &Changeset) -> Result<()> {
    for d in state.nodes.values() {
        if d.is_opaque() {
            continue;
        }
        let Some(check) = registry
            .node_type(&d.type_name)
            .and_then(|t| t.check.as_ref())
        else {
            continue;
        };
        let node = Node::new(state, registry, d.id).expect("live");
        check(&node).map_err(|message| Error::Check {
            path: node.path().to_string(),
            message,
        })?;
    }
    if registry.rules.is_empty() {
        return Ok(());
    }
    let root = Node::new(state, registry, state.root).expect("root");
    for rule in &registry.rules {
        rule(&root, changes).map_err(|v| Error::Check {
            path: v.path,
            message: v.message,
        })?;
    }
    Ok(())
}

/// The only thing with write verbs. Exists inside [`Tree::edit`](crate::Tree::edit) or an
/// open gesture. Writes apply as they are made; the edit is cut into one undo step.
pub struct Edit<'t> {
    pub(crate) state: &'t mut State,
    pub(crate) registry: &'t Registry,
    pub(crate) ids: &'t mut IdSource,
}

impl<'t> Edit<'t> {
    // ---- reads ----

    pub fn at(&self, at: impl Into<At>) -> Option<Node<'_>> {
        let id = resolve(self.state, at.into()).ok()?;
        Node::new(self.state, self.registry, id)
    }

    pub fn root(&self) -> Node<'_> {
        Node::new(self.state, self.registry, self.state.root).expect("root")
    }

    pub fn extract<A: Into<At>>(&self, ats: impl IntoIterator<Item = A>) -> Result<Fragment> {
        let ids = ats
            .into_iter()
            .map(|a| resolve(self.state, a.into()))
            .collect::<Result<Vec<_>>>()?;
        file::extract(self.state, ids)
    }

    // ---- helpers ----

    pub(crate) fn resolve(&self, at: impl Into<At>) -> Result<NodeId> {
        resolve(self.state, at.into())
    }

    fn path(&self, id: NodeId) -> String {
        self.state.path_of(id).to_string()
    }

    fn not_opaque(&self, id: NodeId) -> Result<()> {
        if self.state.node(id).is_opaque() {
            return Err(Error::Opaque(self.path(id)));
        }
        Ok(())
    }

    /// The declared type of a node that holds values: not opaque, not the root, a category or a group.
    fn user_type(&self, id: NodeId) -> Result<&'t NodeType> {
        self.not_opaque(id)?;
        let registry: &'t Registry = self.registry;
        let name = &self.state.node(id).type_name;
        registry.node_type(name).ok_or_else(|| {
            Error::Structural(format!("{} is a {name}; it holds no values", self.path(id)))
        })
    }

    /// Root and categories are made by the core and stay put. Opaque nodes don't change.
    fn movable(&self, id: NodeId) -> Result<()> {
        if id == self.state.root || self.state.is_category(id) {
            return Err(Error::Structural(format!(
                "{} is the root or a category; the core owns it",
                self.path(id)
            )));
        }
        self.not_opaque(id)
    }

    pub(crate) fn check_parent(&self, parent: NodeId) -> Result<()> {
        self.not_opaque(parent)?;
        if parent == self.state.root {
            return Err(Error::Structural(
                "only categories live at /; the registry declares them".into(),
            ));
        }
        if self.state.node(parent).type_name == GROUP {
            return Err(Error::Structural(format!(
                "{} is a group; groups hold members, not children",
                self.path(parent)
            )));
        }
        Ok(())
    }

    pub(crate) fn check_can_hold(&self, parent: NodeId, type_name: &str) -> Result<()> {
        self.check_parent(parent)?;
        if type_name == GROUP {
            return Ok(());
        }
        if type_name == ROOT || type_name == CATEGORY {
            return Err(Error::Structural(format!(
                "the core makes {type_name} nodes"
            )));
        }
        let t = self
            .registry
            .node_type(type_name)
            .ok_or_else(|| Error::UnknownType(type_name.into()))?;
        let category = self.state.category_of(parent).expect("below a category");
        let category = &self.state.node(category).name;
        if !t.categories.is_empty() && !t.categories.contains(category) {
            return Err(Error::NotAllowed {
                type_name: type_name.into(),
                category: category.clone(),
            });
        }
        Ok(())
    }

    /// `base` if it's free under `parent`, otherwise `stem-2`, `stem-3` …
    pub(crate) fn unique_name(&self, parent: NodeId, base: &str) -> String {
        let children = &self.state.node(parent).children;
        if !children.contains_key(base) {
            return base.to_string();
        }
        let stem = match base.rsplit_once('-') {
            Some((s, n))
                if !s.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) =>
            {
                s
            }
            _ => base,
        };
        (2..)
            .map(|n| format!("{stem}-{n}"))
            .find(|name| !children.contains_key(name))
            .expect("a free name")
    }

    pub(crate) fn insert_node(&mut self, parent: NodeId, type_name: &str, name: &str) -> NodeId {
        let id = self.ids.next();
        self.state
            .nodes
            .insert(id, NodeData::new(id, name, Some(parent), type_name));
        self.state
            .node_mut(parent)
            .children
            .insert(name.to_string(), id);
        id
    }

    fn detach_from_orders(&mut self, parent: NodeId, id: NodeId) {
        let p = self.state.node_mut(parent);
        for order in p.orders.values_mut() {
            order.retain(|x| *x != id);
        }
        p.orders.retain(|_, o| !o.is_empty());
    }

    // ---- nodes ----

    /// Adds a node of a declared type (or a `group`) under `parent`.
    pub fn add(&mut self, parent: impl Into<At>, type_name: &str, name: &str) -> Result<NodeId> {
        let parent = self.resolve(parent)?;
        if !valid_name(name) {
            return Err(Error::InvalidName(name.into()));
        }
        self.check_can_hold(parent, type_name)?;
        if self.state.node(parent).children.contains_key(name) {
            return Err(Error::NameTaken(
                self.state.path_of(parent).join(name).to_string(),
            ));
        }
        Ok(self.insert_node(parent, type_name, name))
    }

    /// As `add`, but names it `base-2`, `base-3` … when `base` is taken.
    pub fn add_unique(
        &mut self,
        parent: impl Into<At>,
        type_name: &str,
        base: &str,
    ) -> Result<NodeId> {
        let parent = self.resolve(parent)?;
        if !valid_name(base) {
            return Err(Error::InvalidName(base.into()));
        }
        let name = self.unique_name(parent, base);
        self.add(parent, type_name, &name)
    }

    /// Removes a node and everything under it. Drops them from every group and order, and
    /// unbinds every binding they are the source of. Here-references to them stay, unresolved.
    pub fn remove(&mut self, at: impl Into<At>) -> Result<()> {
        let id = self.resolve(at)?;
        self.movable(id)?;
        let gone: BTreeSet<NodeId> = self.state.subtree(id).into_iter().collect();
        let d = self.state.node(id);
        let (parent, name) = (d.parent.expect("not root"), d.name.clone());
        self.state.node_mut(parent).children.remove(&name);
        for g in &gone {
            self.state.nodes.remove(g);
        }
        self.state.forget(&gone);
        Ok(())
    }

    pub fn rename(&mut self, at: impl Into<At>, name: &str) -> Result<()> {
        let id = self.resolve(at)?;
        self.movable(id)?;
        if !valid_name(name) {
            return Err(Error::InvalidName(name.into()));
        }
        let d = self.state.node(id);
        if d.name == name {
            return Ok(());
        }
        let (parent, old) = (d.parent.expect("not root"), d.name.clone());
        if self.state.node(parent).children.contains_key(name) {
            return Err(Error::NameTaken(
                self.state.path_of(parent).join(name).to_string(),
            ));
        }
        let p = self.state.node_mut(parent);
        p.children.remove(&old);
        p.children.insert(name.to_string(), id);
        self.state.node_mut(id).name = name.to_string();
        Ok(())
    }

    /// Moves a node under a new parent in the same category. It leaves the old parent's orders.
    pub fn move_to(&mut self, at: impl Into<At>, parent: impl Into<At>) -> Result<()> {
        let id = self.resolve(at)?;
        self.movable(id)?;
        let to = self.resolve(parent)?;
        let d = self.state.node(id);
        let (from, name, type_name) = (
            d.parent.expect("not root"),
            d.name.clone(),
            d.type_name.clone(),
        );
        if from == to {
            return Ok(());
        }
        if self.state.subtree(id).contains(&to) {
            return Err(Error::Cycle(self.path(id)));
        }
        self.check_can_hold(to, &type_name)?;
        if self.state.category_of(id) != self.state.category_of(to) {
            return Err(Error::CrossesCategory {
                from: self.path(id),
                to: self.path(to),
            });
        }
        if self.state.node(to).children.contains_key(&name) {
            return Err(Error::NameTaken(
                self.state.path_of(to).join(&name).to_string(),
            ));
        }
        self.state.node_mut(from).children.remove(&name);
        self.detach_from_orders(from, id);
        self.state.node_mut(to).children.insert(name, id);
        self.state.node_mut(id).parent = Some(to);
        Ok(())
    }

    /// Copies a node and its subtree under `parent`. See [`Edit::paste`] for what the copy points at.
    pub fn copy(&mut self, at: impl Into<At>, parent: impl Into<At>) -> Result<NodeId> {
        let id = self.resolve(at)?;
        let fragment = file::extract(self.state, vec![id])?;
        let report = self.paste(parent, &fragment)?;
        Ok(report.nodes[0])
    }

    // ---- values ----

    pub fn set<T: ValueType>(&mut self, at: impl Into<At>, key: Key<T>, value: T) -> Result<()> {
        self.set_value(at, key.name(), value.into_value())
    }

    /// Writes a value. Refuses a key outside the schema, a wrong kind, a non-finite number,
    /// an unknown choice and anything out of range. The caller clamps.
    pub fn set_value(&mut self, at: impl Into<At>, key: &str, value: Value) -> Result<()> {
        let id = self.resolve(at)?;
        let t = self.user_type(id)?;
        let spec = t.spec(key).ok_or_else(|| Error::UnknownKey {
            path: self.path(id),
            key: key.into(),
        })?;
        if let Some(p) = spec.problem(&value) {
            return Err(problem_error(p, self.path(id), spec, &value));
        }
        self.state.node_mut(id).values.insert(key.into(), value);
        Ok(())
    }

    /// Unsets a value, so it reads as the schema default again.
    pub fn reset(&mut self, at: impl Into<At>, key: impl KeyName) -> Result<()> {
        let id = self.resolve(at)?;
        let key = key.key_name();
        if self.user_type(id)?.spec(key).is_none() {
            return Err(Error::UnknownKey {
                path: self.path(id),
                key: key.into(),
            });
        }
        self.state.node_mut(id).values.remove(key);
        Ok(())
    }

    fn ref_key(&self, id: NodeId, key: &str) -> Result<()> {
        if !self.user_type(id)?.refs.iter().any(|k| k == key) {
            return Err(Error::UnknownKey {
                path: self.path(id),
                key: key.into(),
            });
        }
        Ok(())
    }

    /// Points a reference key at a file or a node. A missing target is allowed and reads as unresolved.
    pub fn set_ref(&mut self, at: impl Into<At>, key: impl KeyName, mut r: Ref) -> Result<()> {
        let id = self.resolve(at)?;
        let key = key.key_name();
        self.ref_key(id, key)?;
        if r.file.is_none() && r.node.is_none() {
            return Err(Error::Structural(
                "a reference needs a file or a node".into(),
            ));
        }
        if r.is_here() {
            r.path = None;
        }
        self.state.node_mut(id).refs.insert(key.into(), r);
        Ok(())
    }

    pub fn clear_ref(&mut self, at: impl Into<At>, key: impl KeyName) -> Result<()> {
        let id = self.resolve(at)?;
        let key = key.key_name();
        self.ref_key(id, key)?;
        self.state.node_mut(id).refs.remove(key);
        Ok(())
    }

    // ---- bindings ----

    /// Binds `source` onto `target`'s slot or value key, carrying the values the source's type
    /// declares. A slot holds one source, so binding a slot replaces what was there.
    pub fn bind<K: KeyName>(
        &mut self,
        target: impl Into<At>,
        on: On,
        source: impl Into<At>,
        values: impl IntoIterator<Item = (K, Value)>,
    ) -> Result<()> {
        let t_id = self.resolve(target)?;
        let s_id = self.resolve(source)?;
        if t_id == s_id {
            return Err(Error::Structural("a node can't be bound to itself".into()));
        }
        let tt = self.user_type(t_id)?;
        match &on {
            On::Slot(s) if !tt.slots.contains(s) => {
                return Err(Error::UnknownSlot {
                    path: self.path(t_id),
                    slot: s.clone(),
                });
            }
            On::Value(k) if tt.spec(k).is_none() => {
                return Err(Error::UnknownKey {
                    path: self.path(t_id),
                    key: k.clone(),
                });
            }
            _ => {}
        }
        let specs = self
            .user_type(s_id)
            .ok()
            .and_then(|st| st.bindable.as_ref())
            .ok_or_else(|| Error::NotBindable(self.path(s_id)))?;
        let mut vals = BTreeMap::new();
        for (k, v) in values {
            let k = k.key_name();
            let spec = specs
                .iter()
                .find(|s| s.key == k)
                .ok_or_else(|| Error::UnknownKey {
                    path: self.path(s_id),
                    key: k.into(),
                })?;
            if let Some(p) = spec.problem(&v) {
                return Err(problem_error(p, self.path(s_id), spec, &v));
            }
            vals.insert(k.to_string(), v);
        }
        let d = self.state.node_mut(t_id);
        if matches!(on, On::Slot(_)) {
            d.bindings.retain(|(o, _), _| *o != on);
        }
        d.bindings.insert(
            (on, s_id),
            BindingData {
                values: vals,
                raw: None,
            },
        );
        Ok(())
    }

    pub fn unbind(&mut self, target: impl Into<At>, on: On, source: impl Into<At>) -> Result<()> {
        let t_id = self.resolve(target)?;
        let s_id = self.resolve(source)?;
        self.not_opaque(t_id)?;
        self.state.node_mut(t_id).bindings.remove(&(on, s_id));
        Ok(())
    }

    // ---- groups ----

    fn group(&self, at: At) -> Result<NodeId> {
        let g = self.resolve(at)?;
        self.not_opaque(g)?;
        if self.state.node(g).type_name != GROUP {
            return Err(Error::NotAGroup(self.path(g)));
        }
        Ok(g)
    }

    fn members<A: Into<At>>(
        &self,
        g: NodeId,
        ids: impl IntoIterator<Item = A>,
    ) -> Result<Vec<NodeId>> {
        let mut out = Vec::new();
        for a in ids {
            let id = self.resolve(a)?;
            self.not_opaque(id)?;
            if id == g || id == self.state.root {
                return Err(Error::Structural(format!(
                    "{} can't be a member here",
                    self.path(id)
                )));
            }
            out.push(id);
        }
        Ok(out)
    }

    pub fn join<A: Into<At>>(
        &mut self,
        group: impl Into<At>,
        ids: impl IntoIterator<Item = A>,
    ) -> Result<()> {
        let g = self.group(group.into())?;
        let ids = self.members(g, ids)?;
        self.state.node_mut(g).members.extend(ids);
        Ok(())
    }

    pub fn leave<A: Into<At>>(
        &mut self,
        group: impl Into<At>,
        ids: impl IntoIterator<Item = A>,
    ) -> Result<()> {
        let g = self.group(group.into())?;
        let ids = self.members(g, ids)?;
        let d = self.state.node_mut(g);
        for id in ids {
            d.members.remove(&id);
        }
        Ok(())
    }

    // ---- orders ----

    fn child_of(&self, owner: NodeId, at: At) -> Result<NodeId> {
        let id = self.resolve(at)?;
        if self.state.node(id).parent != Some(owner) {
            return Err(Error::NotAChild {
                owner: self.path(owner),
                child: self.path(id),
            });
        }
        Ok(id)
    }

    fn order_owner(&self, owner: At, name: &str) -> Result<NodeId> {
        let o = self.resolve(owner)?;
        self.not_opaque(o)?;
        if !valid_name(name) {
            return Err(Error::InvalidName(name.into()));
        }
        Ok(o)
    }

    /// Stores a named order of `owner`'s children. An empty list removes the order.
    pub fn set_order<A: Into<At>>(
        &mut self,
        owner: impl Into<At>,
        name: &str,
        ids: impl IntoIterator<Item = A>,
    ) -> Result<()> {
        let o = self.order_owner(owner.into(), name)?;
        let mut seen = BTreeSet::new();
        let mut list = Vec::new();
        for a in ids {
            let id = self.child_of(o, a.into())?;
            if !seen.insert(id) {
                return Err(Error::Duplicate(self.path(id)));
            }
            list.push(id);
        }
        let d = self.state.node_mut(o);
        if list.is_empty() {
            d.orders.remove(name);
        } else {
            d.orders.insert(name.to_string(), list);
        }
        Ok(())
    }

    pub fn append_to_order(
        &mut self,
        owner: impl Into<At>,
        name: &str,
        id: impl Into<At>,
    ) -> Result<()> {
        let o = self.order_owner(owner.into(), name)?;
        let id = self.child_of(o, id.into())?;
        let order = self
            .state
            .node_mut(o)
            .orders
            .entry(name.to_string())
            .or_default();
        if order.contains(&id) {
            return Err(Error::Duplicate(self.state.path_of(id).to_string()));
        }
        order.push(id);
        Ok(())
    }

    /// Pastes a fragment under `parent`. Returns the new top nodes and anything it couldn't take.
    pub fn paste(&mut self, parent: impl Into<At>, fragment: &Fragment) -> Result<PasteReport> {
        file::paste(self, parent.into(), fragment)
    }
}
