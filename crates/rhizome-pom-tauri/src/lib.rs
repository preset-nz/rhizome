//! rhizome-pom-tauri: one open POM document over Tauri.
//!
//! App-level commands and two events, the way `preset-preferences` does it: no plugin, so no
//! capability entries. **Commands only** (decision 46): the webview runs commands by id and
//! drives gestures; there is no command that takes a raw `Op`, so every edit goes through the
//! app's object model.
//!
//! ```ignore
//! let pom = Arc::new(Pom::<MyApp>::files()?.on_change(|doc| engine.load(doc.projection())));
//! tauri::Builder::default()
//!     .manage(PomHost::new(pom.clone()))
//!     .invoke_handler(tauri::generate_handler![
//!         rhizome_pom_tauri::commands::pom_status,
//!         rhizome_pom_tauri::commands::pom_view,
//!         rhizome_pom_tauri::commands::pom_tree,
//!         rhizome_pom_tauri::commands::pom_commands,
//!         rhizome_pom_tauri::commands::pom_run,
//!         rhizome_pom_tauri::commands::pom_begin,
//!         rhizome_pom_tauri::commands::pom_end,
//!         rhizome_pom_tauri::commands::pom_cancel,
//!         rhizome_pom_tauri::commands::pom_connect,
//!         rhizome_pom_tauri::commands::pom_new,
//!         rhizome_pom_tauri::commands::pom_open,
//!         rhizome_pom_tauri::commands::pom_save_as,
//!     ])
//!     .build(tauri::generate_context!())?
//!     .run(|app, event| {
//!         #[cfg(target_os = "macos")]
//!         if let tauri::RunEvent::Opened { urls } = &event {
//!             rhizome_pom_tauri::opened(app, urls);
//!         }
//!     });
//! ```
//!
//! **Events.** [`COMMIT`] carries every commit (undo, redo and cancel included) as an
//! [`Update`](rhizome_pom::Update): `{seq, label, changes, rows, removed, generation}`, the
//! fresh rows of every node it touched. [`STATUS`] carries the [`Status`] whenever it
//! changes; when its `generation` moves, the whole tree was replaced and the mirror re-reads
//! [`pom_view`](commands::pom_view).
//! [`OPEN_DOCUMENT`] carries a path macOS asked the app to open once the front end is
//! listening; the front end then calls [`pom_open`](commands::pom_open).
//!
//! Dialogs (choosing a file to open or save as) stay the front end's: these commands take
//! paths.

pub mod commands;
pub mod opened;

use std::sync::Arc;

use rhizome_pom::{Event, Host, ObjectModel, Pom};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub use opened::{Offer, Opened};

pub const COMMIT: &str = "pom://commit";
pub const STATUS: &str = "pom://status";
pub const OPEN_DOCUMENT: &str = "pom://open-document";

/// The open document, as managed state, and the opened-from-Finder hand-off.
pub struct PomHost {
    host: Arc<dyn Host>,
    pub(crate) opened: Opened,
}

impl PomHost {
    pub fn new<M: ObjectModel>(pom: Arc<Pom<M>>) -> Self {
        PomHost {
            host: rhizome_pom::host(pom),
            opened: Opened::default(),
        }
    }

    pub fn host(&self) -> &Arc<dyn Host> {
        &self.host
    }
}

/// Emits what a call caused. Called after the call returned, so the document isn't locked.
pub fn emit<R: Runtime>(app: &AppHandle<R>, events: Vec<Event>) {
    for e in events {
        let _ = match e {
            Event::Commit(c) => app.emit(COMMIT, c),
            Event::Status(s) => app.emit(STATUS, s),
        };
    }
}

/// Hands over the URLs of `RunEvent::Opened`: the last one with the document's extension is
/// emitted as [`OPEN_DOCUMENT`] if the front end is listening, else held for
/// [`pom_connect`](commands::pom_connect).
pub fn opened<R: Runtime>(app: &AppHandle<R>, urls: &[tauri::Url]) {
    let pom = app.state::<PomHost>();
    let Some(path) = opened::document_path(urls, pom.host.extension()) else {
        return;
    };
    if let Offer::Emit(path) = pom.opened.offer(path) {
        let _ = app.emit(OPEN_DOCUMENT, path);
    }
}
