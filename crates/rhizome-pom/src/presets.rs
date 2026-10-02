//! Node presets: ways to fill in a kind's template, declared on the kind.
//!
//! The app's object model gives a kind an [`Aggregate`]: what state a preset holds, how to
//! read it off a node and how to write it back. POM supplies the rest: built-in presets in
//! code, user presets saved in the document (they travel with the file), "which preset is
//! current", applying one, making a new node from one, and moving a user preset between
//! documents as text.
//!
//! Presets are copied into a node, never followed. A shared choice that nodes follow (a
//! theme) is the app's to build from rhizome's primitives, not POM's.

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::sync::Arc;

use rhizome_core::{Edit, Fragment, Node, NodeId, NodeType, On, Tree, Value};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::error::{Error, Result};

/// POM's category for user presets, and their node type.
pub const PRESETS: &str = "presets";
pub const PRESET: &str = "preset";
const FOR: &str = "preset.for";
const LABEL: &str = "preset.label";
const STATE: &str = "preset.state";
const MAX_LABEL: usize = 60;

pub(crate) fn preset_node_type() -> NodeType {
    NodeType::new(PRESET)
        .in_categories(&[PRESETS])
        .text(FOR, "")
        .text(LABEL, "")
        .text(STATE, "null")
}

/// What an apply did: how much it wrote, and what it couldn't. Surfaced, never swallowed,
/// so a preset saved before a rename doesn't look as if it applied cleanly.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub applied: usize,
    pub skipped: Vec<String>,
}

/// A preset by name: built into the kind, or saved by the user in this document.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresetRef {
    Catalogue(String),
    User(String),
}

/// The app's side of a kind's presets: the state a preset holds and how to get and set it.
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
}

/// An [`Aggregate`] with its state as JSON, so kinds of different state types sit together.
pub(crate) trait Erased: Send + Sync {
    fn get(&self, node: Node<'_>) -> Json;
    fn set(&self, tx: &mut Edit<'_>, node: NodeId, state: &Json) -> rhizome_core::Result<Report>;
    fn matches(&self, current: &Json, preset: &Json) -> bool;
    fn fits(&self, state: &Json) -> bool;
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

    fn fits(&self, state: &Json) -> bool {
        serde_json::from_value::<A::State>(state.clone()).is_ok()
    }
}

/// One kind's presets: its aggregate and its built-in catalogue.
pub(crate) struct KindPresets {
    pub aggregate: Arc<dyn Erased>,
    pub catalogue: Vec<(String, Json)>,
}

/// A kind's presets being declared. Chain the catalogue onto it.
pub struct KindPresetsRef<'a, A: Aggregate> {
    pub(crate) entry: &'a mut KindPresets,
    pub(crate) _a: PhantomData<A>,
}

impl<A: Aggregate> KindPresetsRef<'_, A> {
    /// Built-in presets, in menu order. Code, never saved.
    pub fn catalogue<'n>(self, entries: impl IntoIterator<Item = (&'n str, A::State)>) -> Self {
        for (name, state) in entries {
            let json = serde_json::to_value(state).expect("preset state serialises");
            self.entry.catalogue.push((name.to_string(), json));
        }
        self
    }
}

/// Every kind's presets, by node type.
#[derive(Default)]
pub(crate) struct Presets {
    pub by_type: BTreeMap<String, KindPresets>,
}

impl Presets {
    fn of(&self, type_name: &str) -> rhizome_core::Result<&KindPresets> {
        self.by_type
            .get(type_name)
            .ok_or_else(|| structural_msg(format!("a {type_name} has no presets")))
    }

    fn user_presets<'t>(tree: &'t Tree, for_type: &str) -> Vec<(String, Node<'t>)> {
        let Some(cat) = tree.at(format!("/{PRESETS}").as_str()) else {
            return Vec::new();
        };
        let mut out: Vec<(String, Node<'t>)> = cat
            .children()
            .filter(|n| n.type_name() == PRESET && text(n, FOR) == for_type)
            .map(|n| (text(&n, LABEL), n))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    fn user_state(node: &Node<'_>) -> Json {
        serde_json::from_str(&text(node, STATE)).unwrap_or(Json::Null)
    }

    fn user_id(tx: &Edit<'_>, for_type: &str, label: &str) -> Option<NodeId> {
        let cat = tx.at(format!("/{PRESETS}").as_str())?;
        cat.children()
            .find(|n| {
                n.type_name() == PRESET && text(n, FOR) == for_type && text(n, LABEL) == label
            })
            .map(|n| n.id())
    }

    fn state_in(
        kp: &KindPresets,
        tx: &Edit<'_>,
        for_type: &str,
        r: &PresetRef,
    ) -> rhizome_core::Result<Json> {
        match r {
            PresetRef::Catalogue(name) => kp
                .catalogue
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, s)| s.clone())
                .ok_or_else(|| {
                    structural_msg(format!("a {for_type} has no built-in preset “{name}”"))
                }),
            PresetRef::User(label) => {
                let id = Self::user_id(tx, for_type, label).ok_or_else(|| {
                    structural_msg(format!("no {for_type} preset called “{label}”"))
                })?;
                Ok(Self::user_state(&tx.at(id).expect("live")))
            }
        }
    }

    // ---- reads ----

    /// Built-in presets first, then the user's for this node's kind.
    pub fn names(&self, tree: &Tree, node: NodeId) -> Result<Vec<PresetRef>> {
        let n = live(tree, node)?;
        let Some(kp) = self.by_type.get(n.type_name()) else {
            return Ok(Vec::new());
        };
        let mut out: Vec<PresetRef> = kp
            .catalogue
            .iter()
            .map(|(name, _)| PresetRef::Catalogue(name.clone()))
            .collect();
        out.extend(
            Self::user_presets(tree, n.type_name())
                .into_iter()
                .map(|(l, _)| PresetRef::User(l)),
        );
        Ok(out)
    }

    /// The first preset, built-in then user, whose state matches the node's now.
    pub fn current(&self, tree: &Tree, node: NodeId) -> Result<Option<PresetRef>> {
        let n = live(tree, node)?;
        let Some(kp) = self.by_type.get(n.type_name()) else {
            return Ok(None);
        };
        let now = kp.aggregate.get(n);
        for (name, state) in &kp.catalogue {
            if kp.aggregate.matches(&now, state) {
                return Ok(Some(PresetRef::Catalogue(name.clone())));
            }
        }
        for (label, p) in Self::user_presets(tree, n.type_name()) {
            if kp.aggregate.matches(&now, &Self::user_state(&p)) {
                return Ok(Some(PresetRef::User(label)));
            }
        }
        Ok(None)
    }

    // ---- writes, inside an edit ----

    pub(crate) fn save(
        &self,
        tx: &mut Edit<'_>,
        node: NodeId,
        label: &str,
    ) -> rhizome_core::Result<()> {
        let label = valid_label(label)?;
        let for_type = node_type(tx, node)?;
        let state = self
            .of(&for_type)?
            .aggregate
            .get(tx.at(node).expect("live"));
        if Self::user_id(tx, &for_type, &label).is_some() {
            return Err(structural_msg(format!(
                "a preset called “{label}” already exists here; update it instead"
            )));
        }
        let id = tx.add_unique(format!("/{PRESETS}").as_str(), PRESET, &for_type)?;
        tx.set_value(id, FOR, Value::Text(for_type))?;
        tx.set_value(id, LABEL, Value::Text(label))?;
        tx.set_value(id, STATE, Value::Text(state.to_string()))
    }

    pub(crate) fn update(
        &self,
        tx: &mut Edit<'_>,
        node: NodeId,
        label: &str,
    ) -> rhizome_core::Result<()> {
        let for_type = node_type(tx, node)?;
        let state = self
            .of(&for_type)?
            .aggregate
            .get(tx.at(node).expect("live"));
        let id = Self::user_id(tx, &for_type, label)
            .ok_or_else(|| structural_msg(format!("no preset called “{label}” here")))?;
        tx.set_value(id, STATE, Value::Text(state.to_string()))
    }

    pub(crate) fn rename(
        &self,
        tx: &mut Edit<'_>,
        node: NodeId,
        label: &str,
        to: &str,
    ) -> rhizome_core::Result<()> {
        let to = valid_label(to)?;
        let for_type = node_type(tx, node)?;
        self.of(&for_type)?;
        let id = Self::user_id(tx, &for_type, label)
            .ok_or_else(|| structural_msg(format!("no preset called “{label}” here")))?;
        if to != label && Self::user_id(tx, &for_type, &to).is_some() {
            return Err(structural_msg(format!(
                "a preset called “{to}” already exists here"
            )));
        }
        tx.set_value(id, LABEL, Value::Text(to))
    }

    pub(crate) fn delete(
        &self,
        tx: &mut Edit<'_>,
        node: NodeId,
        label: &str,
    ) -> rhizome_core::Result<()> {
        let for_type = node_type(tx, node)?;
        self.of(&for_type)?;
        let id = Self::user_id(tx, &for_type, label)
            .ok_or_else(|| structural_msg(format!("no preset called “{label}” here")))?;
        tx.remove(id)
    }

    pub(crate) fn apply(
        &self,
        tx: &mut Edit<'_>,
        node: NodeId,
        preset: &PresetRef,
    ) -> rhizome_core::Result<Report> {
        let for_type = node_type(tx, node)?;
        let kp = self.of(&for_type)?;
        let state = Self::state_in(kp, tx, &for_type, preset)?;
        kp.aggregate.set(tx, node, &state)
    }

    /// A user preset as text: its node as a rhizome fragment, so an export, a user preset in
    /// a document and an entry in a library rhizome are one shape.
    pub(crate) fn export(&self, tree: &Tree, node: NodeId, label: &str) -> Result<String> {
        let for_type = live(tree, node)?.type_name().to_string();
        self.of(&for_type)?;
        let (_, p) = Self::user_presets(tree, &for_type)
            .into_iter()
            .find(|(l, _)| l == label)
            .ok_or_else(|| structural_msg(format!("no preset called “{label}” here")))?;
        Ok(tree.extract([p.id()])?.to_text())
    }

    /// Takes in an exported user preset. Refused, and nothing changes, unless it's one preset
    /// for a kind of this model, its state fits that kind, and its name is free.
    pub(crate) fn import(&self, tx: &mut Edit<'_>, exported: &str) -> rhizome_core::Result<String> {
        let fragment = Fragment::from_text(exported)?;
        if fragment.len() != 1 {
            return Err(structural_msg(format!(
                "a preset export holds one preset, not {} nodes",
                fragment.len()
            )));
        }
        let pasted = tx.paste(format!("/{PRESETS}").as_str(), &fragment)?;
        let id = pasted.nodes[0];
        let n = tx.at(id).expect("just pasted");
        if n.type_name() != PRESET {
            return Err(structural_msg(format!(
                "this is a {}, not a preset",
                n.type_name()
            )));
        }
        let (for_type, label, state) = (text(&n, FOR), text(&n, LABEL), Self::user_state(&n));
        let kp = self
            .by_type
            .get(&for_type)
            .ok_or_else(|| structural_msg(format!("this document has no {for_type} presets")))?;
        if !kp.aggregate.fits(&state) {
            return Err(structural_msg(format!(
                "this preset doesn't fit a {for_type}"
            )));
        }
        let label = valid_label(&label)?;
        let cat = tx
            .at(format!("/{PRESETS}").as_str())
            .expect("POM's category");
        let taken = cat.children().any(|p| {
            p.id() != id
                && p.type_name() == PRESET
                && text(&p, FOR) == for_type
                && text(&p, LABEL) == label
        });
        if taken {
            return Err(structural_msg(format!(
                "a {for_type} preset called “{label}” already exists here"
            )));
        }
        Ok(label)
    }

    /// Instantiates a kind's template filled from a preset: add, then apply, in one edit.
    pub(crate) fn add_from(
        &self,
        tx: &mut Edit<'_>,
        parent: &str,
        type_name: &str,
        name: &str,
        preset: &PresetRef,
    ) -> rhizome_core::Result<(NodeId, Report)> {
        let kp = self.of(type_name)?;
        let state = Self::state_in(kp, tx, type_name, preset)?;
        let id = tx.add(parent, type_name, name)?;
        let report = kp.aggregate.set(tx, id, &state)?;
        Ok((id, report))
    }
}

fn text(n: &Node<'_>, key: &str) -> String {
    match n.value(key) {
        Some(Value::Text(s)) => s,
        _ => String::new(),
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
