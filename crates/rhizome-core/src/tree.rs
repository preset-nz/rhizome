use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::diff::{Changeset, diff};
use crate::edit::{At, Edit, resolve, run_checks};
use crate::error::{Error, Result};
use crate::file::{self, Fragment, LoadReport};
use crate::id::{IdSource, NodeId};
use crate::op::Op;
use crate::registry::{CATEGORY, Registry};
use crate::state::{NodeData, State};
use crate::view::Node;

/// How many undo steps a tree keeps.
pub const HISTORY: usize = 50;

/// What every write returns: a sequence number, a label and what changed. Serialises for a
/// transport: `{"seq", "label", "changes": [...]}`.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct Commit {
    pub seq: u64,
    pub label: String,
    pub changes: Changeset,
}

/// A copy of a tree's nodes at one moment, to diff against later.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot(pub(crate) State);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GestureId(u64);

struct Step {
    label: String,
    state: State,
}

struct Gesture {
    id: GestureId,
    label: String,
    base: State,
}

struct Coalescing {
    key: String,
    at: Instant,
}

/// One file's nodes, its undo history and its saved snapshot.
///
/// The tree changes only inside an edit: [`Tree::edit`], [`Tree::edit_coalesced`], or an open
/// gesture. `&Tree` has no write method.
///
/// ```compile_fail
/// use rhizome_core::Tree;
/// fn sneak(tree: &Tree) {
///     let _ = tree.edit("Remove", |tx| tx.remove("/images/sky"));
/// }
/// ```
pub struct Tree {
    registry: Arc<Registry>,
    state: State,
    saved: State,
    ids: IdSource,
    seq: u64,
    undo: Vec<Step>,
    redo: Vec<Step>,
    gesture: Option<Gesture>,
    next_gesture: u64,
    coalescing: Option<Coalescing>,
    window: Duration,
    clock: Box<dyn Fn() -> Instant + Send>,
}

impl Tree {
    pub fn new(registry: Arc<Registry>) -> Tree {
        Tree::with_ids(registry, IdSource::Ulid)
    }

    /// A new tree drawing ids from `ids`. Tests pass `IdSource::sequential()`.
    pub fn with_ids(registry: Arc<Registry>, mut ids: IdSource) -> Tree {
        let mut state = State::new(ids.next());
        for (name, _) in registry.categories() {
            let id = ids.next();
            state
                .nodes
                .insert(id, NodeData::new(id, name, Some(state.root), CATEGORY));
            state
                .node_mut(state.root)
                .children
                .insert(name.to_string(), id);
        }
        Tree::from_state(registry, state, ids)
    }

    /// A new tree with what every document of its kind starts with, made by `start`: no undo
    /// step, no commit, and nothing unsaved. What a new document is born with can't be undone.
    pub fn starting_with(
        registry: Arc<Registry>,
        ids: IdSource,
        start: impl FnOnce(&mut Edit<'_>) -> Result<()>,
    ) -> Result<Tree> {
        let mut tree = Tree::with_ids(registry, ids);
        tree.run(start)?;
        tree.saved = tree.state.clone();
        Ok(tree)
    }

    fn from_state(registry: Arc<Registry>, state: State, ids: IdSource) -> Tree {
        Tree {
            registry,
            saved: state.clone(),
            state,
            ids,
            seq: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            gesture: None,
            next_gesture: 1,
            coalescing: None,
            window: Duration::from_secs(1),
            clock: Box::new(Instant::now),
        }
    }

    /// Reads a file. Fails only when the text isn't a rhizome file this build reads; anything
    /// else it couldn't take as written is in the report. History starts empty and the tree
    /// starts saved.
    pub fn load(text: &str, registry: Arc<Registry>) -> Result<(Tree, LoadReport)> {
        Tree::load_with_ids(text, registry, IdSource::Ulid)
    }

    pub fn load_with_ids(
        text: &str,
        registry: Arc<Registry>,
        mut ids: IdSource,
    ) -> Result<(Tree, LoadReport)> {
        let (state, report) = file::load(text, &registry, &mut ids)?;
        Ok((Tree::from_state(registry, state, ids), report))
    }

    // ---- reads ----

    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    pub fn root(&self) -> Node<'_> {
        Node::new(&self.state, &self.registry, self.state.root).expect("root")
    }

    pub fn at(&self, at: impl Into<At>) -> Option<Node<'_>> {
        let id = resolve(&self.state, at.into()).ok()?;
        self.get(id)
    }

    pub fn get(&self, id: NodeId) -> Option<Node<'_>> {
        Node::new(&self.state, &self.registry, id)
    }

    /// Every node, in path order.
    pub fn nodes(&self) -> Vec<Node<'_>> {
        let mut all: Vec<_> = self.state.all_paths().into_iter().collect();
        all.sort_by(|a, b| a.1.cmp(&b.1));
        all.into_iter().filter_map(|(id, _)| self.get(id)).collect()
    }

    pub fn len(&self) -> usize {
        self.state.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        false
    }

    /// The file text: canonical, byte-stable JSON.
    pub fn serialise(&self) -> String {
        file::serialise(&self.state)
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot(self.state.clone())
    }

    /// What changed from `since` to now.
    pub fn diff(&self, since: &Snapshot) -> Changeset {
        diff(&since.0, &self.state)
    }

    /// Copies nodes and their subtrees out, for the clipboard or a paste.
    pub fn extract<A: Into<At>>(&self, ats: impl IntoIterator<Item = A>) -> Result<Fragment> {
        let ids = ats
            .into_iter()
            .map(|a| resolve(&self.state, a.into()))
            .collect::<Result<Vec<_>>>()?;
        file::extract(&self.state, ids)
    }

    pub fn seq(&self) -> u64 {
        self.seq
    }

    // ---- edits ----

    /// Runs `f` on the current state. On error, restores `before` and returns it. On success,
    /// returns the changes, or `None` when nothing changed.
    fn run<T>(
        &mut self,
        f: impl FnOnce(&mut Edit<'_>) -> Result<T>,
    ) -> Result<(T, Option<(State, Changeset)>)> {
        let before = self.state.clone();
        let result = {
            let mut tx = Edit {
                state: &mut self.state,
                registry: &self.registry,
                ids: &mut self.ids,
            };
            f(&mut tx)
        }
        .and_then(|v| {
            let changes = diff(&before, &self.state);
            run_checks(&self.state, &self.registry, &changes)?;
            Ok((v, changes))
        });
        match result {
            Err(e) => {
                self.state = before;
                Err(e)
            }
            Ok((v, changes)) => Ok((v, (!changes.is_empty()).then_some((before, changes)))),
        }
    }

    fn commit(&mut self, label: &str, changes: Changeset) -> Commit {
        self.seq += 1;
        Commit {
            seq: self.seq,
            label: label.to_string(),
            changes,
        }
    }

    fn push_step(&mut self, label: &str, before: State) {
        self.undo.push(Step {
            label: label.to_string(),
            state: before,
        });
        if self.undo.len() > HISTORY {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// One edit: one undo step and one commit, or nothing at all if `f` fails or changes nothing.
    ///
    /// While a gesture is open, the edit joins it: it commits, but its undo step is the gesture's.
    pub fn edit<T>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Edit<'_>) -> Result<T>,
    ) -> Result<(T, Option<Commit>)> {
        let (v, changed) = self.run(f)?;
        let Some((before, changes)) = changed else {
            return Ok((v, None));
        };
        if self.gesture.is_none() {
            self.push_step(label, before);
            self.coalescing = None;
        }
        Ok((v, Some(self.commit(label, changes))))
    }

    /// For input with no begin and end: a knob, a scroll wheel, arrow-key nudges. Consecutive
    /// edits with the same `key`, each within the coalescing window of the last, share one undo
    /// step. Each still returns its own commit.
    pub fn edit_coalesced<T>(
        &mut self,
        label: &str,
        key: &str,
        f: impl FnOnce(&mut Edit<'_>) -> Result<T>,
    ) -> Result<(T, Option<Commit>)> {
        let (v, changed) = self.run(f)?;
        let Some((before, changes)) = changed else {
            return Ok((v, None));
        };
        let now = (self.clock)();
        if self.gesture.is_none() {
            let joins = self
                .coalescing
                .as_ref()
                .is_some_and(|c| c.key == key && now.duration_since(c.at) <= self.window);
            if !joins {
                self.push_step(label, before);
            }
            self.coalescing = Some(Coalescing {
                key: key.to_string(),
                at: now,
            });
        }
        Ok((v, Some(self.commit(label, changes))))
    }

    /// Runs `ops` as one edit.
    pub fn edit_ops(&mut self, label: &str, ops: &[Op]) -> Result<Option<Commit>> {
        let (_, commit) = self.edit(label, |tx| {
            ops.iter().try_for_each(|op| tx.apply(op).map(|_| ()))
        })?;
        Ok(commit)
    }

    pub fn set_coalesce_window(&mut self, window: Duration) {
        self.window = window;
    }

    /// Replaces the clock coalescing reads. For tests.
    pub fn set_clock(&mut self, clock: impl Fn() -> Instant + Send + 'static) {
        self.clock = Box::new(clock);
    }

    // ---- gestures ----

    /// Opens a gesture: edits applied through it are visible at once and become one undo step
    /// at [`Tree::end`]. One gesture at a time.
    pub fn begin(&mut self, label: &str) -> Result<GestureId> {
        if self.gesture.is_some() {
            return Err(Error::GestureOpen);
        }
        let id = GestureId(self.next_gesture);
        self.next_gesture += 1;
        self.gesture = Some(Gesture {
            id,
            label: label.to_string(),
            base: self.state.clone(),
        });
        self.coalescing = None;
        Ok(id)
    }

    fn open(&self, g: GestureId) -> Result<&Gesture> {
        self.gesture
            .as_ref()
            .filter(|x| x.id == g)
            .ok_or(Error::NoGesture)
    }

    /// Edits inside an open gesture. A failure undoes only this call; the gesture stays open.
    pub fn within<T>(
        &mut self,
        g: GestureId,
        f: impl FnOnce(&mut Edit<'_>) -> Result<T>,
    ) -> Result<(T, Option<Commit>)> {
        let label = self.open(g)?.label.clone();
        self.edit(&label, f)
    }

    /// Applies `ops` inside an open gesture, as one call.
    pub fn apply(&mut self, g: GestureId, ops: &[Op]) -> Result<Option<Commit>> {
        let (_, commit) = self.within(g, |tx| {
            ops.iter().try_for_each(|op| tx.apply(op).map(|_| ()))
        })?;
        Ok(commit)
    }

    /// Closes the gesture and cuts its undo step, if it changed anything.
    pub fn end(&mut self, g: GestureId) -> Result<()> {
        self.open(g)?;
        let gesture = self.gesture.take().expect("open");
        if gesture.base != self.state {
            self.push_step(&gesture.label, gesture.base);
        }
        Ok(())
    }

    /// Closes the gesture and puts everything back as it was at `begin`.
    pub fn cancel(&mut self, g: GestureId) -> Result<Option<Commit>> {
        self.open(g)?;
        let gesture = self.gesture.take().expect("open");
        let changes = diff(&self.state, &gesture.base);
        self.state = gesture.base;
        Ok((!changes.is_empty())
            .then(|| self.commit(&format!("Cancel {}", gesture.label), changes)))
    }

    pub fn gesture_open(&self) -> bool {
        self.gesture.is_some()
    }

    // ---- history ----

    pub fn undo(&mut self) -> Result<Option<Commit>> {
        if self.gesture.is_some() {
            return Err(Error::GestureOpen);
        }
        let Some(step) = self.undo.pop() else {
            return Ok(None);
        };
        self.coalescing = None;
        let current = std::mem::replace(&mut self.state, step.state);
        let changes = diff(&current, &self.state);
        let label = format!("Undo {}", step.label);
        self.redo.push(Step {
            label: step.label,
            state: current,
        });
        Ok(Some(self.commit(&label, changes)))
    }

    pub fn redo(&mut self) -> Result<Option<Commit>> {
        if self.gesture.is_some() {
            return Err(Error::GestureOpen);
        }
        let Some(step) = self.redo.pop() else {
            return Ok(None);
        };
        self.coalescing = None;
        let current = std::mem::replace(&mut self.state, step.state);
        let changes = diff(&current, &self.state);
        let label = format!("Redo {}", step.label);
        self.undo.push(Step {
            label: step.label,
            state: current,
        });
        Ok(Some(self.commit(&label, changes)))
    }

    /// The label of the step undo would revert, for the native menu.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    pub fn history_len(&self) -> usize {
        self.undo.len()
    }

    // ---- saved ----

    /// Records the current state as what's on disk.
    pub fn mark_saved(&mut self) {
        self.saved = self.state.clone();
    }

    /// Derived, never a flag: the current state differs from the saved one.
    pub fn is_unsaved(&self) -> bool {
        !diff(&self.saved, &self.state).is_empty()
    }
}
