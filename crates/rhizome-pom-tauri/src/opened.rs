//! Documents macOS asks the app to open: a double-click in Finder, `open some.doc`, a file
//! dropped on the Dock icon. Lifted from Shard's `opened.rs`, with the extension a parameter.
//!
//! They arrive as `RunEvent::Opened`, both at a cold start and while the app is running. At
//! a cold start the event can come before the webview has mounted, when nothing is listening
//! yet, so the path is held here until the front end asks for it. Once it has asked, later
//! paths are emitted straight away. Deciding both under one lock is what stops a path falling
//! between the two.

use std::path::PathBuf;
use std::sync::Mutex;

/// What to do with a path macOS handed over.
#[derive(Debug, PartialEq, Eq)]
pub enum Offer {
    /// The front end is listening: emit it now.
    Emit(String),
    /// The front end has not mounted: it will ask.
    Held,
}

#[derive(Default)]
struct Inner {
    ready: bool,
    pending: Option<String>,
}

/// The hand-off between `RunEvent::Opened` and the front end.
#[derive(Default)]
pub struct Opened(Mutex<Inner>);

impl Opened {
    /// A path macOS opened. Held until the front end is ready. A second path before then
    /// replaces the first, since one document is open at a time and the newer request is the
    /// one the person meant.
    pub fn offer(&self, path: String) -> Offer {
        let mut inner = self.0.lock().expect("opened poisoned");
        if inner.ready {
            Offer::Emit(path)
        } else {
            inner.pending = Some(path);
            Offer::Held
        }
    }

    /// The front end has mounted and is listening. Returns whatever arrived before it did,
    /// once.
    pub fn take(&self) -> Option<String> {
        let mut inner = self.0.lock().expect("opened poisoned");
        inner.ready = true;
        inner.pending.take()
    }
}

/// The last document with `extension` among the URLs macOS sent. Anything else (a folder, a
/// web link, another app's file) is not ours to open.
pub fn document_path<'a>(
    urls: impl IntoIterator<Item = &'a tauri::Url>,
    extension: &str,
) -> Option<String> {
    urls.into_iter()
        .filter_map(|u| u.to_file_path().ok())
        .filter(|p: &PathBuf| {
            p.extension()
                .is_some_and(|e| e.eq_ignore_ascii_case(extension))
        })
        .last()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_before_the_front_end_is_held_and_delivered_once() {
        let o = Opened::default();
        assert_eq!(o.offer("/a.doc".into()), Offer::Held);
        assert_eq!(o.take().as_deref(), Some("/a.doc"));
        assert_eq!(o.take(), None);
    }

    #[test]
    fn a_path_after_the_front_end_is_emitted_not_held() {
        let o = Opened::default();
        assert_eq!(o.take(), None);
        assert_eq!(o.offer("/b.doc".into()), Offer::Emit("/b.doc".into()));
        assert_eq!(o.take(), None);
    }

    #[test]
    fn the_newest_path_before_the_front_end_wins() {
        let o = Opened::default();
        o.offer("/old.doc".into());
        o.offer("/new.doc".into());
        assert_eq!(o.take().as_deref(), Some("/new.doc"));
    }

    #[test]
    fn only_files_with_the_extension_are_documents() {
        let urls: Vec<tauri::Url> = [
            "file:///tmp/one.doc",
            "file:///tmp/two.DOC",
            "file:///tmp/sound.wav",
            "https://example.com/x.doc",
        ]
        .iter()
        .map(|s| s.parse().unwrap())
        .collect();
        assert_eq!(document_path(&urls, "doc").as_deref(), Some("/tmp/two.DOC"));
        assert_eq!(document_path(&urls[2..], "doc"), None);
    }
}
