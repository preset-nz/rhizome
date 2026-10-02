use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rhizome_core::{Commit, Edit, GestureId, IdSource, LoadReport, NodeId, Op, Tree};
use serde_json::Value as Json;

use crate::command::Outcome;
use crate::error::{Error, Result};
use crate::model::{Model, ObjectModel, Policy, breaches, repin};
use crate::presets::{PresetRef, Report};

/// Where a document's text lives. Single files today; a bundle folder and a database later.
pub trait Store: Send {
    fn read(&self, path: &Path) -> std::io::Result<String>;
    fn write(&mut self, path: &Path, text: &str) -> std::io::Result<()>;
}

/// Plain files, written atomically: a temporary file beside the target, then a rename.
#[derive(Clone, Copy, Debug, Default)]
pub struct FileStore;

impl Store for FileStore {
    fn read(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn write(&mut self, path: &Path, text: &str) -> std::io::Result<()> {
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".saving");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

/// Files in memory, shared between clones. For tests and previews.
#[derive(Clone, Debug, Default)]
pub struct MemoryStore(Arc<Mutex<BTreeMap<PathBuf, String>>>);

impl MemoryStore {
    pub fn get(&self, path: impl AsRef<Path>) -> Option<String> {
        self.0.lock().unwrap().get(path.as_ref()).cloned()
    }

    pub fn put(&self, path: impl AsRef<Path>, text: &str) {
        self.0
            .lock()
            .unwrap()
            .insert(path.as_ref().to_path_buf(), text.to_string());
    }
}

impl Store for Box<dyn Store> {
    fn read(&self, path: &Path) -> std::io::Result<String> {
        (**self).read(path)
    }

    fn write(&mut self, path: &Path, text: &str) -> std::io::Result<()> {
        (**self).write(path, text)
    }
}

impl Store for MemoryStore {
    fn read(&self, path: &Path) -> std::io::Result<String> {
        self.get(path)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no such file"))
    }

    fn write(&mut self, path: &Path, text: &str) -> std::io::Result<()> {
        self.put(path, text);
        Ok(())
    }
}

fn io(path: &Path, e: std::io::Error) -> Error {
    Error::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    }
}

/// One open document of an app's object model: its tree, where it's stored, and the app's
/// projection of it. What an app gets from POM without writing it.
pub struct Document<M: ObjectModel> {
    model: Arc<Model<M>>,
    tree: Tree,
    store: Box<dyn Store>,
    path: Option<PathBuf>,
    projection: M::Projection,
    /// While set, edits coalesce under this key: a command run as part of a drag.
    coalesce: Option<String>,
    /// Bumped whenever the whole tree is replaced (revert), so a mirror knows to re-read.
    generation: u64,
    _m: PhantomData<fn() -> M>,
}

impl<M: ObjectModel> Document<M> {
    /// A new, untitled document.
    pub fn new(store: impl Store + 'static) -> Result<Self>
    where
        M::Context: Default,
    {
        Self::new_with_ids(store, IdSource::Ulid)
    }

    pub fn new_with_ids(store: impl Store + 'static, ids: IdSource) -> Result<Self>
    where
        M::Context: Default,
    {
        Self::new_in(Arc::default(), store, ids)
    }

    /// A new, untitled document of a model built with `cx` (decision 52).
    pub fn new_in(cx: Arc<M::Context>, store: impl Store + 'static, ids: IdSource) -> Result<Self> {
        let model = Model::<M>::build(cx)?;
        let tree = Tree::with_ids(model.registry.clone(), ids);
        Ok(Self::from_tree(model, tree, Box::new(store), None))
    }

    /// Opens a document. Fails only when the file can't be read or isn't a rhizome file;
    /// everything else it couldn't take as written is in the report.
    pub fn open(store: impl Store + 'static, path: impl AsRef<Path>) -> Result<(Self, LoadReport)>
    where
        M::Context: Default,
    {
        Self::open_with_ids(store, path, IdSource::Ulid)
    }

    pub fn open_with_ids(
        store: impl Store + 'static,
        path: impl AsRef<Path>,
        ids: IdSource,
    ) -> Result<(Self, LoadReport)>
    where
        M::Context: Default,
    {
        Self::open_in(Arc::default(), store, path, ids)
    }

    /// Opens a document of a model built with `cx` (decision 52).
    pub fn open_in(
        cx: Arc<M::Context>,
        store: impl Store + 'static,
        path: impl AsRef<Path>,
        ids: IdSource,
    ) -> Result<(Self, LoadReport)> {
        let path = path.as_ref();
        let model = Model::<M>::build(cx)?;
        let text = store.read(path).map_err(|e| io(path, e))?;
        let (tree, mut report) = Tree::load_with_ids(&text, model.registry.clone(), ids)?;
        report_breaches(&model, &tree, &mut report);
        Ok((
            Self::from_tree(model, tree, Box::new(store), Some(path.to_path_buf())),
            report,
        ))
    }

    fn from_tree(
        model: Arc<Model<M>>,
        tree: Tree,
        store: Box<dyn Store>,
        path: Option<PathBuf>,
    ) -> Self {
        let mut projection = M::Projection::default();
        M::project(&tree, &mut projection, None);
        Document {
            model,
            tree,
            store,
            path,
            projection,
            coalesce: None,
            generation: 0,
            _m: PhantomData,
        }
    }

    // ---- reads ----

    pub fn tree(&self) -> &Tree {
        &self.tree
    }

    pub fn projection(&self) -> &M::Projection {
        &self.projection
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn is_unsaved(&self) -> bool {
        self.tree.is_unsaved()
    }

    /// The window title: the file name, or "Untitled", and "Edited" when unsaved.
    pub fn title(&self) -> String {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        if self.is_unsaved() {
            format!("{name} — Edited")
        } else {
            name
        }
    }

    /// How many times the whole tree has been replaced since this document was made.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// What the model was built with (decision 52).
    pub fn context(&self) -> &M::Context {
        &self.model.context
    }

    pub fn policy(&self, type_name: &str) -> Policy {
        self.model.policy(type_name)
    }

    // ---- files ----

    pub fn save(&mut self) -> Result<()> {
        let path = self.path.clone().ok_or(Error::NoPath)?;
        self.write(&path)
    }

    /// Saves to `path` and makes it the document's file. Adds the extension if it's missing.
    pub fn save_as(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let mut path = path.as_ref().to_path_buf();
        if path.extension().is_none() {
            path.set_extension(M::EXTENSION);
        }
        self.write(&path)?;
        self.path = Some(path);
        Ok(())
    }

    fn write(&mut self, path: &Path) -> Result<()> {
        let text = self.tree.serialise();
        self.store.write(path, &text).map_err(|e| io(path, e))?;
        self.tree.mark_saved();
        Ok(())
    }

    /// Throws away every change since the last save, and the history with it.
    pub fn revert(&mut self) -> Result<LoadReport> {
        let path = self.path.clone().ok_or(Error::NoPath)?;
        let text = self.store.read(&path).map_err(|e| io(&path, e))?;
        let (tree, mut report) = Tree::load(&text, self.model.registry.clone())?;
        report_breaches(&self.model, &tree, &mut report);
        self.tree = tree;
        self.generation += 1;
        self.projection = M::Projection::default();
        M::project(&self.tree, &mut self.projection, None);
        Ok(report)
    }

    // ---- edits, passed through to the tree and re-projected ----

    fn projected(&mut self, commit: Option<Commit>) -> Option<Commit> {
        if let Some(c) = &commit {
            M::project(&self.tree, &mut self.projection, Some(&c.changes));
        }
        commit
    }

    pub fn edit<T>(
        &mut self,
        label: &str,
        f: impl FnOnce(&mut Edit<'_>) -> rhizome_core::Result<T>,
    ) -> Result<(T, Option<Commit>)> {
        let (v, c) = match self.coalesce.clone() {
            Some(key) => self.tree.edit_coalesced(label, &key, f)?,
            None => self.tree.edit(label, f)?,
        };
        Ok((v, self.projected(c)))
    }

    pub fn edit_ops(&mut self, label: &str, ops: &[Op]) -> Result<Option<Commit>> {
        let c = self.tree.edit_ops(label, ops)?;
        Ok(self.projected(c))
    }

    pub fn edit_coalesced<T>(
        &mut self,
        label: &str,
        key: &str,
        f: impl FnOnce(&mut Edit<'_>) -> rhizome_core::Result<T>,
    ) -> Result<(T, Option<Commit>)> {
        let (v, c) = self.tree.edit_coalesced(label, key, f)?;
        Ok((v, self.projected(c)))
    }

    pub fn within<T>(
        &mut self,
        g: GestureId,
        f: impl FnOnce(&mut Edit<'_>) -> rhizome_core::Result<T>,
    ) -> Result<(T, Option<Commit>)> {
        let (v, c) = self.tree.within(g, f)?;
        Ok((v, self.projected(c)))
    }

    /// Pastes a fragment under `parent`, keeping pinned kinds at their ends.
    pub fn paste(&mut self, parent: &str, fragment: &str) -> Result<Option<Commit>> {
        let model = self.model.clone();
        let op = Op::Paste {
            parent: parent.to_string(),
            fragment: fragment.to_string(),
        };
        Ok(self
            .edit("Paste", |tx| {
                tx.apply(&op)?;
                let p = tx
                    .at(parent)
                    .map(|n| n.id())
                    .ok_or_else(|| rhizome_core::Error::NotFound(parent.into()))?;
                repin(&model.policies, tx, p)
            })?
            .1)
    }

    /// Copies a node beside itself, keeping pinned kinds at their ends.
    pub fn duplicate(&mut self, at: &str) -> Result<Option<Commit>> {
        let model = self.model.clone();
        Ok(self
            .edit("Duplicate", |tx| {
                let parent = tx
                    .at(at)
                    .and_then(|n| n.parent())
                    .map(|n| n.id())
                    .ok_or_else(|| rhizome_core::Error::NotFound(at.into()))?;
                tx.copy(at, parent)?;
                repin(&model.policies, tx, parent)
            })?
            .1)
    }

    pub fn begin(&mut self, label: &str) -> Result<GestureId> {
        Ok(self.tree.begin(label)?)
    }

    pub fn apply(&mut self, g: GestureId, ops: &[Op]) -> Result<Option<Commit>> {
        let c = self.tree.apply(g, ops)?;
        Ok(self.projected(c))
    }

    pub fn end(&mut self, g: GestureId) -> Result<()> {
        Ok(self.tree.end(g)?)
    }

    pub fn cancel(&mut self, g: GestureId) -> Result<Option<Commit>> {
        let c = self.tree.cancel(g)?;
        Ok(self.projected(c))
    }

    pub fn undo(&mut self) -> Result<Option<Commit>> {
        let c = self.tree.undo()?;
        Ok(self.projected(c))
    }

    pub fn redo(&mut self) -> Result<Option<Commit>> {
        let c = self.tree.redo()?;
        Ok(self.projected(c))
    }

    // ---- presets: on the kind, copied into a node ----

    /// The node's kind's built-in presets, then the user's saved for that kind.
    pub fn preset_names(&self, node: NodeId) -> Result<Vec<PresetRef>> {
        self.model.presets.names(&self.tree, node)
    }

    /// Whether a kind declares presets.
    pub fn has_presets(&self, type_name: &str) -> bool {
        self.model.presets.by_type.contains_key(type_name)
    }

    /// The first preset whose state matches the node now.
    pub fn current_preset(&self, node: NodeId) -> Result<Option<PresetRef>> {
        self.model.presets.current(&self.tree, node)
    }

    /// Saves the node's state as a user preset for its kind. It lives in the document.
    pub fn save_preset(&mut self, node: NodeId, label: &str) -> Result<Option<Commit>> {
        let model = self.model.clone();
        Ok(self
            .edit("Save Preset", |tx| model.presets.save(tx, node, label))?
            .1)
    }

    pub fn update_preset(&mut self, node: NodeId, label: &str) -> Result<Option<Commit>> {
        let model = self.model.clone();
        Ok(self
            .edit("Update Preset", |tx| model.presets.update(tx, node, label))?
            .1)
    }

    pub fn rename_preset(&mut self, node: NodeId, label: &str, to: &str) -> Result<Option<Commit>> {
        let model = self.model.clone();
        Ok(self
            .edit("Rename Preset", |tx| {
                model.presets.rename(tx, node, label, to)
            })?
            .1)
    }

    pub fn delete_preset(&mut self, node: NodeId, label: &str) -> Result<Option<Commit>> {
        let model = self.model.clone();
        Ok(self
            .edit("Delete Preset", |tx| model.presets.delete(tx, node, label))?
            .1)
    }

    /// User presets of the node's kind as a preset file, to import into another document.
    pub fn export_presets(&self, node: NodeId, labels: &[String]) -> Result<String> {
        self.model.presets.export(&self.tree, node, labels)
    }

    /// Takes in a preset file, in one edit, one undo step. Returns the names it added.
    pub fn import_presets(&mut self, text: &str) -> Result<(Vec<String>, Option<Commit>)> {
        let model = self.model.clone();
        self.edit("Import Presets", |tx| model.presets.import(tx, text))
    }

    /// Applies a preset to a node in one edit, one undo step.
    pub fn apply_preset(
        &mut self,
        node: NodeId,
        preset: &PresetRef,
    ) -> Result<(Report, Option<Commit>)> {
        let model = self.model.clone();
        self.edit("Apply Preset", |tx| model.presets.apply(tx, node, preset))
    }

    /// Makes a node of `type_name` from one of its presets, in one edit, one undo step.
    pub fn add_from_preset(
        &mut self,
        parent: &str,
        type_name: &str,
        name: &str,
        preset: &PresetRef,
    ) -> Result<((NodeId, Report), Option<Commit>)> {
        let model = self.model.clone();
        let label = format!("New {type_name}");
        self.edit(&label, |tx| {
            model.presets.add_from(tx, parent, type_name, name, preset)
        })
    }

    // ---- commands ----

    /// Every command, in registration order, with its label and whether it can run with `payload`.
    pub fn commands(&self, payload: &Json) -> Vec<(String, String, bool)> {
        self.model
            .commands
            .iter()
            .map(|c| (c.id.clone(), (c.label)(self), (c.enabled)(self, payload)))
            .collect()
    }

    pub fn label(&self, id: &str) -> Result<String> {
        Ok((self.model.commands.get(id)?.label)(self))
    }

    pub fn is_enabled(&self, id: &str, payload: &Json) -> Result<bool> {
        Ok((self.model.commands.get(id)?.enabled)(self, payload))
    }

    /// Runs a command, if it's enabled for `payload`.
    pub fn run(&mut self, id: &str, payload: &Json) -> Result<Outcome> {
        let model = self.model.clone();
        let c = model.commands.get(id)?;
        if !(c.enabled)(self, payload) {
            return Err(Error::Disabled(id.into()));
        }
        (c.run)(self, payload)
    }

    /// Runs a command as part of a drag: consecutive runs with the same `key`, each within
    /// the coalescing window of the last, are one undo step.
    pub fn run_coalesced(&mut self, id: &str, payload: &Json, key: &str) -> Result<Outcome> {
        self.coalesce = Some(key.to_string());
        let out = self.run(id, payload);
        self.coalesce = None;
        out
    }
}

fn report_breaches<M: ObjectModel>(model: &Model<M>, tree: &Tree, report: &mut LoadReport) {
    for v in breaches(&model.policies, &tree.root()) {
        report.issues.push(rhizome_core::Issue {
            path: v.path,
            message: format!("policy: {}", v.message),
        });
    }
}
