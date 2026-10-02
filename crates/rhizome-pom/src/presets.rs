//! Presets: an aggregate of getters and setters that the app's object model defines, and
//! the machinery around it that POM runs.
//!
//! The app writes an [`Aggregate`]: what state a preset holds, how to read it off a node
//! and how to write it back. POM supplies catalogues (built in, never saved), user presets
//! (saved in the document as nodes, so they undo, diff and save like anything else),
//! "which preset is current", and following a preset by reference with cascade.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::Arc;

use rhizome_core::{Edit, Node, NodeId, NodeType, On, Ref, Tree, Value};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::error::{Error, Result};
use crate::model::check_id;

/// POM's category for user presets, and their node type.
pub const PRESETS: &str = "presets";
pub const PRESET: &str = "preset";
const KIND: &str = "preset.kind";
const FOR: &str = "preset.for";
const LABEL: &str = "preset.label";
const STATE: &str = "preset.state";
const MAX_LABEL: usize = 60;

pub(crate) fn preset_node_type() -> NodeType {
    NodeType::new(PRESET)
        .in_categories(&[PRESETS])
        .text(KIND, "")
        .text(FOR, "")
        .text(LABEL, "")
        .text(STATE, "null")
}

/// The reference key a following kind gets for preset kind `kind`.
pub fn follow_key(kind: &str) -> String {
    format!("follow.{kind}")
}

fn catalogue_file(kind: &str, name: &str) -> String {
    format!("catalogue:{kind}/{name}")
}

/// What an apply did: how much it wrote, and what it couldn't. Surfaced, never swallowed,
/// so a preset saved before a rename doesn't look as if it applied cleanly.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub applied: usize,
    pub skipped: Vec<String>,
}

/// A preset by name: built into the app, or saved by the user in this document.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetRef {
    Catalogue(String),
    User(String),
}

/// The app's side of a preset kind: the state it holds and how to get and set it.
pub trait Aggregate: Send + Sync + 'static {
    type State: Serialize + DeserializeOwned + PartialEq + Clone;

    /// Reads the aggregate off a node.
    fn get(&self, node: Node<'_>) -> Self::State;

    /// Writes it back, inside one edit. Report what couldn't be applied rather than failing.
    fn set(
        &self,
        tx: &mut Edit<'_>,
        node: NodeId,
        state: &Self::State,
    ) -> rhizome_core::Result<Report>;

    /// Whether a node whose state is `current` counts as `preset`. Exact by default.
    fn matches(&self, current: &Self::State, preset: &Self::State) -> bool {
        current == preset
    }

    /// Which nodes this kind applies to. All by default.
    fn applies_to(&self, _node: Node<'_>) -> bool {
        true
    }
}

/// An [`Aggregate`] with its state as JSON, so kinds of different state types sit together.
trait Erased: Send + Sync {
    fn get(&self, node: Node<'_>) -> Json;
    fn set(&self, tx: &mut Edit<'_>, node: NodeId, state: &Json) -> rhizome_core::Result<Report>;
    fn matches(&self, current: &Json, preset: &Json) -> bool;
    fn applies_to(&self, node: Node<'_>) -> bool;
}

impl<A: Aggregate> Erased for A {
    fn get(&self, node: Node<'_>) -> Json {
        serde_json::to_value(Aggregate::get(self, node)).expect("preset state serialises")
    }

    fn set(&self, tx: &mut Edit<'_>, node: NodeId, state: &Json) -> rhizome_core::Result<Report> {
        let s: A::State = serde_json::from_value(state.clone()).map_err(|e| {
            rhizome_core::Error::Structural(format!("preset state doesn't fit: {e}"))
        })?;
        Aggregate::set(self, tx, node, &s)
    }

    fn matches(&self, current: &Json, preset: &Json) -> bool {
        match (
            serde_json::from_value::<A::State>(current.clone()),
            serde_json::from_value::<A::State>(preset.clone()),
        ) {
            (Ok(c), Ok(p)) => Aggregate::matches(self, &c, &p),
            _ => false,
        }
    }

    fn applies_to(&self, node: Node<'_>) -> bool {
        Aggregate::applies_to(self, node)
    }
}

struct Entry {
    aggregate: Arc<dyn Erased>,
    catalogue: Vec<(String, Json)>,
    fallback: Option<String>,
    followers: Vec<String>,
}

/// The app's preset kinds, as [`ObjectModel::presets`](crate::ObjectModel::presets) declares them.
#[derive(Default)]
pub struct Presets {
    kinds: BTreeMap<String, Entry>,
}

/// One preset kind being declared.
pub struct PresetKindRef<'a, A: Aggregate> {
    entry: &'a mut Entry,
    _a: PhantomData<A>,
}

impl<A: Aggregate> PresetKindRef<'_, A> {
    /// Built-in presets, in menu order. Code, never saved.
    pub fn catalogue<'n>(self, entries: impl IntoIterator<Item = (&'n str, A::State)>) -> Self {
        for (name, state) in entries {
            let json = serde_json::to_value(state).expect("preset state serialises");
            self.entry.catalogue.push((name.to_string(), json));
        }
        self
    }

    /// The catalogue entry a node resolves to when neither it nor an ancestor follows one.
    pub fn fallback(self, name: &str) -> Self {
        self.entry.fallback = Some(name.to_string());
        self
    }

    /// Kinds that can follow a preset of this kind by reference. Their descendants
    /// resolve through them.
    pub fn followed_by(self, kinds: &[&str]) -> Self {
        self.entry
            .followers
            .extend(kinds.iter().map(|k| k.to_string()));
        self
    }
}

impl Presets {
    pub fn kind<A: Aggregate>(&mut self, id: &str, aggregate: A) -> PresetKindRef<'_, A> {
        let entry = self.kinds.entry(id.to_string()).or_insert_with(|| Entry {
            aggregate: Arc::new(aggregate),
            catalogue: Vec::new(),
            fallback: None,
            followers: Vec::new(),
        });
        PresetKindRef {
            entry,
            _a: PhantomData,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        for (id, e) in &self.kinds {
            check_id(id)?;
            if let Some(f) = &e.fallback
                && !e.catalogue.iter().any(|(n, _)| n == f)
            {
                return Err(Error::Model(format!(
                    "preset kind `{id}`: fallback `{f}` isn't in its catalogue"
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn followers(&self) -> Vec<(String, Vec<String>)> {
        self.kinds
            .iter()
            .map(|(id, e)| (id.clone(), e.followers.clone()))
            .collect()
    }

    fn entry(&self, kind: &str) -> Result<&Entry> {
        self.kinds
            .get(kind)
            .ok_or_else(|| Error::UnknownPresetKind(kind.into()))
    }

    pub fn kind_ids(&self) -> impl Iterator<Item = &str> {
        self.kinds.keys().map(String::as_str)
    }

    // ---- reads ----

    fn user_presets<'t>(
        &self,
        tree: &'t Tree,
        kind: &str,
        for_type: &str,
    ) -> Vec<(String, Node<'t>)> {
        let Some(cat) = tree.at(format!("/{PRESETS}").as_str()) else {
            return Vec::new();
        };
        let text = |n: &Node<'_>, k: &str| match n.value(k) {
            Some(Value::Text(s)) => s,
            _ => String::new(),
        };
        let mut out: Vec<(String, Node<'t>)> = cat
            .children()
            .filter(|n| {
                n.type_name() == PRESET && text(n, KIND) == kind && text(n, FOR) == for_type
            })
            .map(|n| (text(&n, LABEL), n))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn user_state(node: &Node<'_>) -> Json {
        match node.value(STATE) {
            Some(Value::Text(s)) => serde_json::from_str(&s).unwrap_or(Json::Null),
            _ => Json::Null,
        }
    }

    fn state_of(&self, tree: &Tree, kind: &str, for_type: &str, r: &PresetRef) -> Result<Json> {
        let e = self.entry(kind)?;
        match r {
            PresetRef::Catalogue(name) => e
                .catalogue
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, s)| s.clone())
                .ok_or_else(|| Error::Preset(format!("no built-in {kind} preset “{name}”"))),
            PresetRef::User(label) => self
                .user_presets(tree, kind, for_type)
                .into_iter()
                .find(|(l, _)| l == label)
                .map(|(_, n)| Self::user_state(&n))
                .ok_or_else(|| Error::Preset(format!("no {kind} preset called “{label}” here"))),
        }
    }

    /// Built-in presets first, then the user's for this node's kind.
    pub fn names(&self, tree: &Tree, kind: &str, node: NodeId) -> Result<Vec<PresetRef>> {
        let e = self.entry(kind)?;
        let n = live(tree, node)?;
        if !e.aggregate.applies_to(n) {
            return Ok(Vec::new());
        }
        let mut out: Vec<PresetRef> = e
            .catalogue
            .iter()
            .map(|(name, _)| PresetRef::Catalogue(name.clone()))
            .collect();
        out.extend(
            self.user_presets(tree, kind, n.type_name())
                .into_iter()
                .map(|(l, _)| PresetRef::User(l)),
        );
        Ok(out)
    }

    /// The first preset, built-in then user, whose state matches the node's now.
    pub fn current(&self, tree: &Tree, kind: &str, node: NodeId) -> Result<Option<PresetRef>> {
        let e = self.entry(kind)?;
        let n = live(tree, node)?;
        if !e.aggregate.applies_to(n) {
            return Ok(None);
        }
        let now = e.aggregate.get(n);
        for r in self.names(tree, kind, node)? {
            let state = self.state_of(tree, kind, n.type_name(), &r)?;
            if e.aggregate.matches(&now, &state) {
                return Ok(Some(r));
            }
        }
        Ok(None)
    }

    /// What a node resolves to: its own followed preset, else the nearest ancestor's, else
    /// the kind's fallback. `None` when there is nothing to resolve to.
    pub fn resolve(&self, tree: &Tree, kind: &str, node: NodeId) -> Result<Option<Resolved>> {
        let e = self.entry(kind)?;
        let key = follow_key(kind);
        let mut at = Some(live(tree, node)?);
        while let Some(n) = at {
            if let Some(r) = n.reference(&key) {
                let found = match (&r.file, r.node) {
                    (Some(f), _) => f
                        .strip_prefix(&format!("catalogue:{kind}/"))
                        .and_then(|name| e.catalogue.iter().find(|(c, _)| c == name))
                        .map(|(name, s)| (PresetRef::Catalogue(name.clone()), s.clone())),
                    (None, Some(id)) => tree.get(id).filter(|p| p.type_name() == PRESET).map(|p| {
                        let label = match p.value(LABEL) {
                            Some(Value::Text(s)) => s,
                            _ => String::new(),
                        };
                        (PresetRef::User(label), Self::user_state(&p))
                    }),
                    _ => None,
                };
                if let Some((preset, state)) = found {
                    return Ok(Some(Resolved {
                        follower: Some(n.id()),
                        preset,
                        state,
                    }));
                }
            }
            at = n.parent();
        }
        Ok(e.fallback.as_ref().map(|name| Resolved {
            follower: None,
            preset: PresetRef::Catalogue(name.clone()),
            state: e
                .catalogue
                .iter()
                .find(|(c, _)| c == name)
                .map(|(_, s)| s.clone())
                .expect("validated"),
        }))
    }

    // ---- writes, inside an edit ----

    pub(crate) fn save(
        &self,
        tx: &mut Edit<'_>,
        kind: &str,
        node: NodeId,
        label: &str,
    ) -> rhizome_core::Result<()> {
        let e = self.entry(kind).map_err(structural)?;
        let label = valid_label(label)?;
        let (for_type, state) = {
            let n = tx
                .at(node)
                .ok_or_else(|| rhizome_core::Error::NotFound(node.to_string()))?;
            (n.type_name().to_string(), e.aggregate.get(n))
        };
        if self.user_id(tx, kind, &for_type, &label).is_some() {
            return Err(structural_msg(format!(
                "a preset called “{label}” already exists here; update it instead"
            )));
        }
        let id = tx.add_unique(format!("/{PRESETS}").as_str(), PRESET, kind)?;
        tx.set_value(id, KIND, Value::Text(kind.into()))?;
        tx.set_value(id, FOR, Value::Text(for_type))?;
        tx.set_value(id, LABEL, Value::Text(label))?;
        tx.set_value(id, STATE, Value::Text(state.to_string()))
    }

    pub(crate) fn update(
        &self,
        tx: &mut Edit<'_>,
        kind: &str,
        node: NodeId,
        label: &str,
    ) -> rhizome_core::Result<()> {
        let e = self.entry(kind).map_err(structural)?;
        let (for_type, state) = {
            let n = tx
                .at(node)
                .ok_or_else(|| rhizome_core::Error::NotFound(node.to_string()))?;
            (n.type_name().to_string(), e.aggregate.get(n))
        };
        let id = self
            .user_id(tx, kind, &for_type, label)
            .ok_or_else(|| structural_msg(format!("no preset called “{label}” here")))?;
        tx.set_value(id, STATE, Value::Text(state.to_string()))
    }

    pub(crate) fn rename(
        &self,
        tx: &mut Edit<'_>,
        kind: &str,
        node: NodeId,
        label: &str,
        to: &str,
    ) -> rhizome_core::Result<()> {
        self.entry(kind).map_err(structural)?;
        let to = valid_label(to)?;
        let for_type = node_type(tx, node)?;
        let id = self
            .user_id(tx, kind, &for_type, label)
            .ok_or_else(|| structural_msg(format!("no preset called “{label}” here")))?;
        if to != label && self.user_id(tx, kind, &for_type, &to).is_some() {
            return Err(structural_msg(format!(
                "a preset called “{to}” already exists here"
            )));
        }
        tx.set_value(id, LABEL, Value::Text(to))
    }

    pub(crate) fn delete(
        &self,
        tx: &mut Edit<'_>,
        kind: &str,
        node: NodeId,
        label: &str,
    ) -> rhizome_core::Result<()> {
        self.entry(kind).map_err(structural)?;
        let for_type = node_type(tx, node)?;
        let id = self
            .user_id(tx, kind, &for_type, label)
            .ok_or_else(|| structural_msg(format!("no preset called “{label}” here")))?;
        // anything following it stops, so nothing points at a preset that's gone
        let key = follow_key(kind);
        let followers: Vec<NodeId> = tx
            .at(id)
            .expect("live")
            .referrers()
            .into_iter()
            .filter(|(_, k)| *k == key)
            .map(|(n, _)| n.id())
            .collect();
        for f in followers {
            tx.clear_ref(f, key.as_str())?;
        }
        tx.remove(id)
    }

    pub(crate) fn apply(
        &self,
        tx: &mut Edit<'_>,
        kind: &str,
        node: NodeId,
        preset: &PresetRef,
    ) -> rhizome_core::Result<Report> {
        let e = self.entry(kind).map_err(structural)?;
        let for_type = node_type(tx, node)?;
        let state = match preset {
            PresetRef::Catalogue(name) => e
                .catalogue
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, s)| s.clone())
                .ok_or_else(|| structural_msg(format!("no built-in {kind} preset “{name}”")))?,
            PresetRef::User(label) => {
                let id = self.user_id(tx, kind, &for_type, label).ok_or_else(|| {
                    structural_msg(format!("no {kind} preset called “{label}” here"))
                })?;
                Self::user_state(&tx.at(id).expect("live"))
            }
        };
        if !e.aggregate.applies_to(tx.at(node).expect("live")) {
            return Err(structural_msg(format!("{kind} presets don't apply here")));
        }
        e.aggregate.set(tx, node, &state)
    }

    /// Makes `node` follow a preset by reference, or stop following with `None`.
    pub(crate) fn follow(
        &self,
        tx: &mut Edit<'_>,
        kind: &str,
        node: NodeId,
        preset: Option<&PresetRef>,
    ) -> rhizome_core::Result<()> {
        let e = self.entry(kind).map_err(structural)?;
        let key = follow_key(kind);
        let Some(preset) = preset else {
            return tx.clear_ref(node, key);
        };
        let r = match preset {
            PresetRef::Catalogue(name) => {
                if !e.catalogue.iter().any(|(n, _)| n == name) {
                    return Err(structural_msg(format!(
                        "no built-in {kind} preset “{name}”"
                    )));
                }
                Ref::file(catalogue_file(kind, name))
            }
            PresetRef::User(label) => {
                // a follower's user presets are the ones saved for its own kind
                let for_type = node_type(tx, node)?;
                let id = self.user_id(tx, kind, &for_type, label).ok_or_else(|| {
                    structural_msg(format!("no {kind} preset called “{label}” here"))
                })?;
                Ref::here(id)
            }
        };
        tx.set_ref(node, key, r)
    }

    fn user_id(&self, tx: &Edit<'_>, kind: &str, for_type: &str, label: &str) -> Option<NodeId> {
        let cat = tx.at(format!("/{PRESETS}").as_str())?;
        let text = |n: &Node<'_>, k: &str| match n.value(k) {
            Some(Value::Text(s)) => s,
            _ => String::new(),
        };
        cat.children()
            .find(|n| text(n, KIND) == kind && text(n, FOR) == for_type && text(n, LABEL) == label)
            .map(|n| n.id())
    }
}

/// A resolved preset: which node chose it (`None` for the fallback), which preset, its state.
#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub follower: Option<NodeId>,
    pub preset: PresetRef,
    pub state: Json,
}

impl Resolved {
    /// The state as the aggregate's own type.
    pub fn state<S: DeserializeOwned>(&self) -> Result<S> {
        serde_json::from_value(self.state.clone()).map_err(|e| Error::Preset(e.to_string()))
    }
}

fn live(tree: &Tree, node: NodeId) -> Result<Node<'_>> {
    tree.get(node)
        .ok_or_else(|| Error::Rhizome(rhizome_core::Error::NotFound(node.to_string())))
}

fn node_type(tx: &Edit<'_>, node: NodeId) -> rhizome_core::Result<String> {
    tx.at(node)
        .map(|n| n.type_name().to_string())
        .ok_or_else(|| rhizome_core::Error::NotFound(node.to_string()))
}

fn valid_label(label: &str) -> rhizome_core::Result<String> {
    let l = label.trim();
    if l.is_empty() || l.chars().count() > MAX_LABEL || l.chars().any(char::is_control) {
        return Err(structural_msg(format!(
            "a preset name needs 1 to {MAX_LABEL} characters, no line breaks"
        )));
    }
    Ok(l.to_string())
}

fn structural(e: Error) -> rhizome_core::Error {
    rhizome_core::Error::Structural(e.to_string())
}

fn structural_msg(m: String) -> rhizome_core::Error {
    rhizome_core::Error::Structural(m)
}

// ---- the ready-made aggregate ----

/// The common case, ready-made: every value in a node's schema, and optionally its
/// bindings. An app customises it with [`NodeValues::skip`] instead of writing an aggregate.
#[derive(Clone)]
pub struct NodeValues {
    skip: Arc<dyn Fn(&str) -> bool + Send + Sync>,
    bindings: bool,
}

impl Default for NodeValues {
    fn default() -> Self {
        NodeValues {
            skip: Arc::new(|_| false),
            bindings: false,
        }
    }
}

impl NodeValues {
    pub fn new() -> Self {
        Self::default()
    }

    /// Keys a preset never reads or writes, such as a node's on/off switch.
    pub fn skip(mut self, f: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        self.skip = Arc::new(f);
        self
    }

    /// Bindings onto the node come along, so applying replaces them.
    pub fn with_bindings(mut self) -> Self {
        self.bindings = true;
        self
    }

    fn skips(&self, on: &On) -> bool {
        match on {
            On::Slot(s) => (self.skip)(s),
            On::Value(k) => (self.skip)(k),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NodeValuesState {
    pub values: BTreeMap<String, Json>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<BindingState>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BindingState {
    pub on: On,
    pub source: NodeId,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<String, Json>,
}

impl Aggregate for NodeValues {
    type State = NodeValuesState;

    fn get(&self, node: Node<'_>) -> NodeValuesState {
        let mut state = NodeValuesState::default();
        let Some(t) = node.node_type() else {
            return state;
        };
        for spec in t.values() {
            if !(self.skip)(&spec.key)
                && let Some(v) = node.value(&spec.key)
            {
                state.values.insert(spec.key.clone(), v.to_json());
            }
        }
        if self.bindings {
            for b in node.bindings() {
                if self.skips(b.on) {
                    continue;
                }
                let specs = b
                    .source
                    .node_type()
                    .and_then(|t| t.binding_values())
                    .unwrap_or(&[]);
                let values = specs
                    .iter()
                    .filter_map(|s| Some((s.key.clone(), b.value(&s.key)?.to_json())))
                    .collect();
                state.bindings.push(BindingState {
                    on: b.on.clone(),
                    source: b.source.id(),
                    values,
                });
            }
        }
        state
    }

    fn set(
        &self,
        tx: &mut Edit<'_>,
        node: NodeId,
        state: &NodeValuesState,
    ) -> rhizome_core::Result<Report> {
        let mut report = Report::default();
        let (specs, existing) = {
            let n = tx
                .at(node)
                .ok_or_else(|| rhizome_core::Error::NotFound(node.to_string()))?;
            let t = n
                .node_type()
                .ok_or_else(|| structural_msg(format!("{} holds no values", n.path())))?;
            let existing: Vec<(On, NodeId)> = n
                .bindings()
                .iter()
                .map(|b| (b.on.clone(), b.source.id()))
                .collect();
            (t.values().to_vec(), existing)
        };
        for spec in specs.iter().filter(|s| !(self.skip)(&s.key)) {
            match state.values.get(&spec.key) {
                None => tx.reset(node, spec.key.as_str())?,
                Some(j) => {
                    match Value::from_json(spec.kind, j).map(|v| tx.set_value(node, &spec.key, v)) {
                        Some(Ok(())) => report.applied += 1,
                        _ => report.skipped.push(spec.key.clone()),
                    }
                }
            }
        }
        for key in state.values.keys() {
            if !specs.iter().any(|s| &s.key == key) || (self.skip)(key) {
                report.skipped.push(key.clone());
            }
        }
        if self.bindings {
            for (on, source) in existing.into_iter().filter(|(on, _)| !self.skips(on)) {
                tx.unbind(node, on, source)?;
            }
            for b in &state.bindings {
                let label = format!("{} ← {}", b.on, b.source);
                if self.skips(&b.on) {
                    report.skipped.push(label);
                    continue;
                }
                let specs = tx
                    .at(b.source)
                    .and_then(|s| s.node_type())
                    .and_then(|t| t.binding_values())
                    .map(|s| s.to_vec());
                let Some(specs) = specs else {
                    report.skipped.push(label);
                    continue;
                };
                let values: Vec<(String, Value)> = b
                    .values
                    .iter()
                    .filter_map(|(k, j)| {
                        let spec = specs.iter().find(|s| &s.key == k)?;
                        Some((k.clone(), Value::from_json(spec.kind, j)?))
                    })
                    .collect();
                match tx.bind(node, b.on.clone(), b.source, values) {
                    Ok(()) => report.applied += 1,
                    Err(_) => report.skipped.push(label),
                }
            }
        }
        Ok(report)
    }
}
