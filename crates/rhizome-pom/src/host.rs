//! One open document behind a transport: what a Tauri shell (or later wasm) calls, with no
//! transport in it.
//!
//! **Every edit is a command** (decision 46). A transport offers [`Host::run`] and gestures,
//! never raw `Op`s, so whatever the webview does goes through the app's object model. A drag
//! is a command run with a coalesce key, or commands between [`Host::begin`] and
//! [`Host::end`].
//!
//! Calls return the [`Event`]s they caused instead of emitting them, so a shell emits them
//! after the document's lock is released and a listener that calls back in can't deadlock.
//!
//! [`Pom`] is typed, for the app's own Rust (its engine, its projection). [`Host`] is the same
//! thing with the object model erased, for transport commands that can't be generic.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use rhizome_core::{Changeset, Commit, GestureId, IdSource, NodeId, Row, Schema};
use serde::Serialize;
use serde_json::Value as Json;

use crate::command::Outcome;
use crate::document::{Document, FileStore, Store};
use crate::error::{Error, Result};
use crate::model::ObjectModel;

/// What a command did, as a transport carries it.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ran {
    Nothing,
    Committed(Commit),
    /// Text for the caller: a copied fragment, an exported preset file.
    Text(String),
}

impl From<Outcome> for Ran {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Nothing => Ran::Nothing,
            Outcome::Committed(c) => Ran::Committed(c),
            Outcome::Text(t) => Ran::Text(t),
        }
    }
}

/// The document as a window shows it. Sent whenever any of it changes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Status {
    pub title: String,
    pub path: Option<String>,
    pub unsaved: bool,
    /// What Undo and Redo would do, when they can.
    pub undo: Option<String>,
    pub redo: Option<String>,
    pub gesture: bool,
    /// Bumped whenever the whole tree is replaced (new, open, revert): re-read it.
    pub generation: u64,
}

/// A command as a menu shows it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CommandState {
    pub id: String,
    pub label: String,
    pub enabled: bool,
}

/// One thing a load couldn't take as written.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Issue {
    pub path: String,
    pub message: String,
}

/// The whole document for a mirror to start from (decision 48). A mirror drops any update
/// with `seq <= view.seq`, re-reads on a gap, and starts over when `generation` moves (a
/// replaced tree counts `seq` from 0 again).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct View {
    pub seq: u64,
    pub generation: u64,
    pub schema: Schema,
    pub rows: Vec<Row>,
}

/// A commit as a mirror takes it: what changed, and the fresh rows of every node it touched.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Update {
    pub seq: u64,
    pub label: String,
    /// For a UI that lists or animates what changed.
    pub changes: Changeset,
    pub rows: Vec<Row>,
    pub removed: Vec<NodeId>,
    /// The tree this belongs to; an update from another generation is stale.
    pub generation: u64,
}

/// What a call caused, for the shell to emit after the lock is released.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Every commit, in order: undo, redo and cancel included.
    Commit(Update),
    /// The status changed.
    Status(Status),
}

/// [`Pom`] with the object model erased. Object safe, so a transport holds `Arc<dyn Host>`.
pub trait Host: Send + Sync {
    /// The document's file extension, without the dot.
    fn extension(&self) -> &'static str;
    fn status(&self) -> Status;
    /// The whole tree in the file format.
    fn tree(&self) -> String;
    /// The whole document as rows and schema: what a mirror starts from.
    fn view(&self) -> View;
    /// Every command with its label and whether it can run with `payload`.
    fn commands(&self, payload: &Json) -> Vec<CommandState>;
    /// Runs a command. With `coalesce`, consecutive runs with that key are one undo step.
    fn run(&self, id: &str, payload: &Json, coalesce: Option<&str>) -> Result<(Ran, Vec<Event>)>;
    /// Opens a gesture: commands run until `end` are one undo step. Returns its token.
    fn begin(&self, label: &str) -> Result<(u64, Vec<Event>)>;
    fn end(&self, token: u64) -> Result<Vec<Event>>;
    /// Ends a gesture and takes back everything done in it.
    fn cancel(&self, token: u64) -> Result<Vec<Event>>;
    /// The front end (re)connected: a gesture it left open is ended, keeping its edits.
    fn reset(&self) -> Vec<Event>;
    /// Replaces the document with a new, untitled one.
    fn new_document(&self) -> Result<Vec<Event>>;
    /// Replaces the document with the file at `path`.
    fn open(&self, path: &Path) -> Result<(Vec<Issue>, Vec<Event>)>;
    fn save_as(&self, path: &Path) -> Result<Vec<Event>>;
}

type StoreFn = Box<dyn Fn() -> Box<dyn Store> + Send + Sync>;
type IdsFn = Box<dyn Fn() -> IdSource + Send + Sync>;
type ChangeFn<M> = Box<dyn Fn(&Document<M>) + Send + Sync>;

struct Inner<M: ObjectModel> {
    doc: Document<M>,
    gesture: Option<(u64, GestureId)>,
    tokens: u64,
    /// Replacements so far (documents swapped, and the reverts of swapped-out ones), added
    /// to the current document's own generation.
    replaced: u64,
    sent: Option<Status>,
}

/// One open document of `M`, shared between the transport and the app's own Rust.
pub struct Pom<M: ObjectModel> {
    inner: Mutex<Inner<M>>,
    cx: Arc<M::Context>,
    store: StoreFn,
    ids: IdsFn,
    on_change: Vec<ChangeFn<M>>,
}

impl<M: ObjectModel> Pom<M> {
    /// Documents on disk, starting untitled.
    pub fn files() -> Result<Self>
    where
        M::Context: Default,
    {
        Self::files_in(Arc::default())
    }

    /// Documents on disk of a model built with `cx` (decision 52), starting untitled.
    pub fn files_in(cx: Arc<M::Context>) -> Result<Self> {
        Self::new_in(cx, || Box::new(FileStore), || IdSource::Ulid)
    }

    /// Documents in `store`, made fresh for each document, with ids from `ids`.
    pub fn new(
        store: impl Fn() -> Box<dyn Store> + Send + Sync + 'static,
        ids: impl Fn() -> IdSource + Send + Sync + 'static,
    ) -> Result<Self>
    where
        M::Context: Default,
    {
        Self::new_in(Arc::default(), store, ids)
    }

    /// As [`Pom::new`], for a model built with `cx`.
    pub fn new_in(
        cx: Arc<M::Context>,
        store: impl Fn() -> Box<dyn Store> + Send + Sync + 'static,
        ids: impl Fn() -> IdSource + Send + Sync + 'static,
    ) -> Result<Self> {
        let doc = Document::new_in(cx.clone(), store(), ids())?;
        Ok(Pom {
            inner: Mutex::new(Inner {
                doc,
                gesture: None,
                tokens: 0,
                replaced: 0,
                sent: None,
            }),
            cx,
            store: Box::new(store),
            ids: Box::new(ids),
            on_change: Vec::new(),
        })
    }

    /// Called with the document after every change, the whole tree replaced included, while
    /// the document is locked: hand the projection to the engine here. Don't call back into
    /// this `Pom` from it.
    pub fn on_change(mut self, f: impl Fn(&Document<M>) + Send + Sync + 'static) -> Self {
        self.on_change.push(Box::new(f));
        self
    }

    /// Reads the document, locked, for the app's own Rust. Edits go through commands.
    pub fn read<T>(&self, f: impl FnOnce(&Document<M>) -> T) -> T {
        f(&self.lock().doc)
    }

    fn lock(&self) -> MutexGuard<'_, Inner<M>> {
        self.inner.lock().expect("POM document lock poisoned")
    }

    /// After a commit, or with `None` after the whole tree was replaced.
    fn changed(&self, inner: &mut Inner<M>, commit: Option<Commit>) -> Vec<Event> {
        for f in &self.on_change {
            f(&inner.doc);
        }
        let generation = status(inner).generation;
        let mut events: Vec<Event> = commit
            .into_iter()
            .map(|c| {
                let patch = inner.doc.tree().patch(&c.changes);
                Event::Commit(Update {
                    seq: c.seq,
                    label: c.label,
                    changes: c.changes,
                    rows: patch.rows,
                    removed: patch.removed,
                    generation,
                })
            })
            .collect();
        events.extend(self.status_event(inner));
        events
    }

    /// The status, if it differs from what was last sent.
    fn status_event(&self, inner: &mut Inner<M>) -> Option<Event> {
        let now = status(inner);
        if inner.sent.as_ref() == Some(&now) {
            return None;
        }
        inner.sent = Some(now.clone());
        Some(Event::Status(now))
    }

    fn replace(&self, inner: &mut Inner<M>, doc: Document<M>) -> Vec<Event> {
        inner.gesture = None;
        // the outgoing document's reverts count too, so the generation only ever grows
        inner.replaced += inner.doc.generation() + 1;
        inner.doc = doc;
        self.changed(inner, None)
    }

    fn token(&self, inner: &Inner<M>, token: u64) -> Result<GestureId> {
        match inner.gesture {
            Some((t, g)) if t == token => Ok(g),
            _ => Err(Error::Payload(format!("no open gesture {token}"))),
        }
    }
}

fn status<M: ObjectModel>(inner: &Inner<M>) -> Status {
    let d = &inner.doc;
    let t = d.tree();
    Status {
        title: d.title(),
        path: d.path().map(|p| p.display().to_string()),
        unsaved: d.is_unsaved(),
        undo: t
            .undo_label()
            .filter(|_| !t.gesture_open())
            .map(String::from),
        redo: t
            .redo_label()
            .filter(|_| !t.gesture_open())
            .map(String::from),
        gesture: t.gesture_open(),
        generation: inner.replaced + d.generation(),
    }
}

impl<M: ObjectModel> Host for Pom<M> {
    fn extension(&self) -> &'static str {
        M::EXTENSION
    }

    fn status(&self) -> Status {
        status(&self.lock())
    }

    fn tree(&self) -> String {
        self.lock().doc.tree().serialise()
    }

    fn view(&self) -> View {
        let inner = self.lock();
        let t = inner.doc.tree();
        View {
            seq: t.seq(),
            generation: status(&inner).generation,
            schema: t.registry().schema(),
            rows: t.rows(),
        }
    }

    fn commands(&self, payload: &Json) -> Vec<CommandState> {
        self.lock()
            .doc
            .commands(payload)
            .into_iter()
            .map(|(id, label, enabled)| CommandState { id, label, enabled })
            .collect()
    }

    fn run(&self, id: &str, payload: &Json, coalesce: Option<&str>) -> Result<(Ran, Vec<Event>)> {
        let mut inner = self.lock();
        let generation = inner.doc.generation();
        let out = match coalesce {
            Some(key) => inner.doc.run_coalesced(id, payload, key)?,
            None => inner.doc.run(id, payload)?,
        };
        let ran = Ran::from(out);
        let events = match &ran {
            Ran::Committed(c) => self.changed(&mut inner, Some(c.clone())),
            _ if inner.doc.generation() != generation => self.changed(&mut inner, None),
            _ => self.status_event(&mut inner).into_iter().collect(),
        };
        Ok((ran, events))
    }

    fn begin(&self, label: &str) -> Result<(u64, Vec<Event>)> {
        let mut inner = self.lock();
        let mut events = Vec::new();
        if let Some((_, g)) = inner.gesture.take() {
            inner.doc.end(g)?;
        }
        let g = inner.doc.begin(label)?;
        inner.tokens += 1;
        let token = inner.tokens;
        inner.gesture = Some((token, g));
        events.extend(self.status_event(&mut inner));
        Ok((token, events))
    }

    fn end(&self, token: u64) -> Result<Vec<Event>> {
        let mut inner = self.lock();
        let g = self.token(&inner, token)?;
        inner.doc.end(g)?;
        inner.gesture = None;
        Ok(self.status_event(&mut inner).into_iter().collect())
    }

    fn cancel(&self, token: u64) -> Result<Vec<Event>> {
        let mut inner = self.lock();
        let g = self.token(&inner, token)?;
        let commit = inner.doc.cancel(g)?;
        inner.gesture = None;
        Ok(match commit {
            Some(c) => self.changed(&mut inner, Some(c)),
            None => self.status_event(&mut inner).into_iter().collect(),
        })
    }

    fn reset(&self) -> Vec<Event> {
        let mut inner = self.lock();
        if let Some((_, g)) = inner.gesture.take() {
            let _ = inner.doc.end(g);
        }
        inner.sent = None;
        self.status_event(&mut inner).into_iter().collect()
    }

    fn new_document(&self) -> Result<Vec<Event>> {
        let doc = Document::new_in(self.cx.clone(), (self.store)(), (self.ids)())?;
        let mut inner = self.lock();
        Ok(self.replace(&mut inner, doc))
    }

    fn open(&self, path: &Path) -> Result<(Vec<Issue>, Vec<Event>)> {
        let (doc, report) = Document::open_in(self.cx.clone(), (self.store)(), path, (self.ids)())?;
        let issues = report
            .issues
            .into_iter()
            .map(|i| Issue {
                path: i.path,
                message: i.message,
            })
            .collect();
        let mut inner = self.lock();
        Ok((issues, self.replace(&mut inner, doc)))
    }

    fn save_as(&self, path: &Path) -> Result<Vec<Event>> {
        let mut inner = self.lock();
        inner.doc.save_as(path)?;
        Ok(self.status_event(&mut inner).into_iter().collect())
    }
}

/// A [`Pom`] as a transport holds it.
pub fn host<M: ObjectModel>(pom: Arc<Pom<M>>) -> Arc<dyn Host> {
    pom
}
