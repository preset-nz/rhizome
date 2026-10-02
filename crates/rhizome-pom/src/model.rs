use std::collections::BTreeMap;
use std::sync::Arc;

use rhizome_core::{
    ChangeKind, Changeset, Edit, Node, NodeId, NodeType, Origin, Path, Registry, Tree, Violation,
};

use crate::command::Commands;
use crate::error::Result;
use std::marker::PhantomData;

use crate::presets::{
    Aggregate, KindPresets, KindPresetsRef, PRESET, PRESETS, Presets, preset_node_type,
};

/// The base every app's object model is built on. Implement it with only your parts;
/// [`Document`](crate::Document) supplies the rest.
pub trait ObjectModel: Sized + 'static {
    /// For window titles and logs.
    const NAME: &'static str;
    /// The file extension, without the dot.
    const EXTENSION: &'static str;

    /// Whatever the app's engine reads, rebuilt from the tree: a compiled plan, a render
    /// list. `()` when there is none.
    type Projection: Default + Send;

    /// What the app knows only at run time and its kinds depend on, such as a catalogue of
    /// operations a sidecar reports (decision 52). `()` when there is none.
    type Context: Send + Sync + 'static;

    /// The app's categories and kinds: node types plus their policy and presets.
    fn kinds(k: &mut Kinds, cx: &Self::Context);

    /// What every new document starts with, such as a document-settings node. Runs once in
    /// `Document::new`, outside history: it can't be undone, and a new document isn't unsaved.
    fn seed(_tx: &mut Edit<'_>, _cx: &Self::Context) -> rhizome_core::Result<()> {
        Ok(())
    }

    /// The app's own commands, next to the built-in ones.
    fn commands(_c: &mut Commands<Self>) {}

    /// Called after every commit, undo, redo and cancel with what changed, and after open
    /// with `None` for a full rebuild. Nothing made here goes back into the tree.
    fn project(_tree: &Tree, _into: &mut Self::Projection, _changes: Option<&Changeset>) {}
}

/// Where a pinned kind must sit in its parent's named order.
#[derive(Clone, Debug, PartialEq)]
pub enum Pin {
    First(String),
    Last(String),
}

/// What an app says about a kind beyond its schema. Compiled into commit-time rules, so
/// it holds however the tree is edited.
#[derive(Clone, Debug, PartialEq)]
pub struct Policy {
    pub deletable: bool,
    pub duplicable: bool,
    pub max_per_parent: Option<usize>,
    pub pinned: Option<Pin>,
}

impl Default for Policy {
    fn default() -> Self {
        Policy {
            deletable: true,
            duplicable: true,
            max_per_parent: None,
            pinned: None,
        }
    }
}

pub(crate) struct Kind {
    pub node_type: NodeType,
    pub policy: Policy,
    pub presets: Option<KindPresets>,
}

/// The app's categories and kinds, as [`ObjectModel::kinds`] declares them.
#[derive(Default)]
pub struct Kinds {
    categories: Vec<(String, Origin)>,
    kinds: Vec<Kind>,
}

/// One kind being declared. Chain policy onto it.
pub struct KindRef<'a>(&'a mut Kind);

impl<'a> KindRef<'a> {
    /// It can't be removed on its own; it leaves only with its parent.
    pub fn not_deletable(self) -> Self {
        self.0.policy.deletable = false;
        self
    }

    /// Duplicate is never offered for it.
    pub fn not_duplicable(self) -> Self {
        self.0.policy.duplicable = false;
        self
    }

    pub fn max_per_parent(self, n: usize) -> Self {
        self.0.policy.max_per_parent = Some(n);
        self
    }

    /// Always first in the parent's order `order`, whenever that order exists.
    pub fn pinned_first(self, order: &str) -> Self {
        self.0.policy.pinned = Some(Pin::First(order.into()));
        self
    }

    pub fn pinned_last(self, order: &str) -> Self {
        self.0.policy.pinned = Some(Pin::Last(order.into()));
        self
    }

    /// The kind's presets: how to read a preset off a node of this kind and write it back.
    /// Last in the chain; the catalogue follows it. Pass a clone to share one aggregate
    /// across kinds.
    pub fn presets<A: Aggregate>(self, aggregate: A) -> KindPresetsRef<'a, A> {
        let entry = self.0.presets.insert(KindPresets {
            aggregate: Arc::new(aggregate),
            catalogue: Vec::new(),
        });
        KindPresetsRef {
            entry,
            _a: PhantomData,
        }
    }
}

impl Kinds {
    pub fn category(&mut self, name: &str, origin: Origin) -> &mut Self {
        self.categories.push((name.to_string(), origin));
        self
    }

    pub fn kind(&mut self, t: NodeType) -> KindRef<'_> {
        self.kinds.push(Kind {
            node_type: t,
            policy: Policy::default(),
            presets: None,
        });
        KindRef(self.kinds.last_mut().expect("just pushed"))
    }
}

/// Everything POM built from an [`ObjectModel`]: the registry, the policies, the preset
/// kinds and the commands. Shared by every document of that model.
pub(crate) struct Model<M: ObjectModel> {
    pub registry: Arc<Registry>,
    pub policies: BTreeMap<String, Policy>,
    pub presets: Presets,
    pub commands: Commands<M>,
    pub context: Arc<M::Context>,
}

impl<M: ObjectModel> Model<M> {
    pub fn build(cx: Arc<M::Context>) -> Result<Arc<Model<M>>> {
        let mut kinds = Kinds::default();
        M::kinds(&mut kinds, &cx);

        // POM's own category and type first, so an app can't take their names
        let mut b = Registry::builder();
        b.category(PRESETS, Origin::Loaded).node(preset_node_type());
        for (name, origin) in &kinds.categories {
            b.category(name, *origin);
        }
        let mut policies = BTreeMap::new();
        let mut presets = Presets::default();
        for mut k in kinds.kinds {
            if let Some(p) = k.presets.take() {
                presets.by_type.insert(k.node_type.name().to_string(), p);
            }
            if k.policy != Policy::default() {
                policies.insert(k.node_type.name().to_string(), k.policy.clone());
            }
            b.node(k.node_type);
        }
        if !policies.is_empty() {
            let rule_policies = policies.clone();
            b.rule(move |root, changes| enforce(&rule_policies, root, changes));
        }
        let registry = b.build()?;
        debug_assert!(registry.node_type(PRESET).is_some());

        let mut commands = Commands::builtin();
        M::commands(&mut commands);
        Ok(Arc::new(Model {
            registry,
            policies,
            presets,
            commands,
            context: cx,
        }))
    }

    pub fn policy(&self, type_name: &str) -> Policy {
        self.policies.get(type_name).cloned().unwrap_or_default()
    }
}

fn violation(path: impl ToString, message: impl Into<String>) -> Violation {
    Violation {
        path: path.to_string(),
        message: message.into(),
    }
}

fn find<'a>(root: &Node<'a>, path: &Path) -> Option<Node<'a>> {
    let mut at = *root;
    for seg in path.segments() {
        at = at.child(seg)?;
    }
    Some(at)
}

/// Every policy, over the tree after an edit.
fn enforce(
    policies: &BTreeMap<String, Policy>,
    root: &Node<'_>,
    changes: &Changeset,
) -> Result<(), Violation> {
    for c in changes.iter() {
        if let ChangeKind::Removed { type_name } = &c.kind
            && policies.get(type_name).is_some_and(|p| !p.deletable)
            && c.path.parent().is_some_and(|p| find(root, &p).is_some())
        {
            return Err(violation(
                &c.path,
                format!("a {type_name} can't be removed on its own"),
            ));
        }
    }
    match breaches(policies, root).into_iter().next() {
        Some(v) => Err(v),
        None => Ok(()),
    }
}

/// Every policy breach in the tree, in walk order: counts per parent, then pins. Load
/// reports these; a commit refuses on the first.
pub(crate) fn breaches(policies: &BTreeMap<String, Policy>, root: &Node<'_>) -> Vec<Violation> {
    let mut out = Vec::new();
    let mut stack = vec![*root];
    while let Some(parent) = stack.pop() {
        let children: Vec<Node<'_>> = parent.children().collect();
        stack.extend(children.iter().copied());
        // counts first: "at most one" says more than where the extra one ended up
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for child in &children {
            *counts.entry(child.type_name()).or_default() += 1;
        }
        for (type_name, n) in &counts {
            if let Some(max) = policies.get(*type_name).and_then(|p| p.max_per_parent)
                && *n > max
            {
                out.push(violation(
                    parent.path(),
                    format!("at most {max} {type_name} here"),
                ));
            }
        }
        for child in &children {
            let Some(Pin::First(order) | Pin::Last(order)) = policies
                .get(child.type_name())
                .and_then(|p| p.pinned.as_ref())
            else {
                continue;
            };
            let first = matches!(policies[child.type_name()].pinned, Some(Pin::First(_)));
            let list = parent.order(order);
            if list.is_empty() {
                continue;
            }
            let at = if first { list.first() } else { list.last() };
            if at.map(|n| n.id()) != Some(child.id()) {
                let place = if first { "first" } else { "last" };
                out.push(violation(
                    child.path(),
                    format!("must stay {place} in `{order}`"),
                ));
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Puts `parent`'s pinned children back at their ends of every order that holds them.
/// For the generic verbs (paste, duplicate), which append without knowing about pins.
pub(crate) fn repin(
    policies: &BTreeMap<String, Policy>,
    tx: &mut Edit<'_>,
    parent: NodeId,
) -> rhizome_core::Result<()> {
    let plan: Vec<(String, Vec<NodeId>)> = {
        let Some(p) = tx.at(parent) else {
            return Ok(());
        };
        let pin_of = |n: &Node<'_>, order: &str| match policies
            .get(n.type_name())
            .and_then(|p| p.pinned.as_ref())
        {
            Some(Pin::First(o)) if o == order => Some(true),
            Some(Pin::Last(o)) if o == order => Some(false),
            _ => None,
        };
        let mut plan = Vec::new();
        for name in p.order_names() {
            let list = p.order(name);
            let (mut first, mut middle, mut last) = (Vec::new(), Vec::new(), Vec::new());
            for n in &list {
                match pin_of(n, name) {
                    Some(true) => first.push(n.id()),
                    Some(false) => last.push(n.id()),
                    None => middle.push(n.id()),
                }
            }
            let new: Vec<NodeId> = first.into_iter().chain(middle).chain(last).collect();
            if new != list.iter().map(|n| n.id()).collect::<Vec<_>>() {
                plan.push((name.to_string(), new));
            }
        }
        plan
    };
    for (name, ids) in plan {
        tx.set_order(parent, &name, ids)?;
    }
    Ok(())
}
