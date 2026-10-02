//! The host a transport drives, without the transport: commands only, drags, gestures,
//! files, the events each call causes, and the change hook. A made-up object model, *Loom*.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rhizome_core::{IdSource, NodeType, Origin, Tree};
use rhizome_pom::*;
use serde_json::json;

struct Loom;

impl ObjectModel for Loom {
    const NAME: &'static str = "Loom";
    const EXTENSION: &'static str = "loom";
    /// How many threads there are, rebuilt on every change.
    type Projection = usize;

    fn kinds(k: &mut Kinds) {
        k.category("threads", Origin::Loaded);
        k.kind(
            NodeType::new("thread")
                .in_categories(&["threads"])
                .float("thread.tension", 0.0..=1.0, 0.5)
                .text("thread.colour", "white"),
        );
    }

    fn project(tree: &Tree, into: &mut usize, _: Option<&rhizome_core::Changeset>) {
        *into = tree
            .nodes()
            .iter()
            .filter(|n| n.type_name() == "thread")
            .count();
    }
}

const AT: &str = "/threads/warp";

/// A host on a shared in-memory store, with one thread, and a count of change-hook calls.
fn loom() -> (Arc<dyn Host>, Arc<Pom<Loom>>, MemoryStore, Arc<AtomicUsize>) {
    let store = MemoryStore::default();
    let s = store.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let pom = Arc::new(
        Pom::<Loom>::new(move || Box::new(s.clone()), IdSource::sequential)
            .unwrap()
            .on_change(move |_| {
                c.fetch_add(1, Ordering::SeqCst);
            }),
    );
    let h = host(pom.clone());
    h.run(
        "node.add",
        &json!({"parent": "/threads", "type": "thread", "name": "warp"}),
        None,
    )
    .unwrap();
    (h, pom, store, calls)
}

fn set(v: serde_json::Value) -> serde_json::Value {
    json!({"at": AT, "key": "thread.tension", "value": v})
}

fn tension(pom: &Pom<Loom>) -> Option<f64> {
    pom.read(|d| {
        d.tree()
            .at(AT)
            .unwrap()
            .get(rhizome_core::Key::<f64>::new("thread.tension"))
    })
}

fn commits(events: &[Event]) -> usize {
    events
        .iter()
        .filter(|e| matches!(e, Event::Commit(_)))
        .count()
}

fn last_status(events: &[Event]) -> Option<&Status> {
    events.iter().rev().find_map(|e| match e {
        Event::Status(s) => Some(s),
        _ => None,
    })
}

#[test]
fn a_command_commits_and_reports() {
    let (h, pom, _, calls) = loom();
    let before = calls.load(Ordering::SeqCst);
    let (ran, events) = h.run("value.set", &set(json!(0.8)), None).unwrap();
    let Ran::Committed(c) = &ran else {
        panic!("{ran:?}")
    };
    assert_eq!(c.label, "Set thread.tension");
    assert_eq!(commits(&events), 1);
    let s = last_status(&events).expect("unsaved and undo changed");
    assert_eq!(s.undo.as_deref(), Some("Set thread.tension"));
    assert!(s.unsaved);
    assert_eq!(tension(&pom), Some(0.8));
    assert_eq!(
        calls.load(Ordering::SeqCst),
        before + 1,
        "the hook ran once"
    );
    assert_eq!(pom.read(|d| *d.projection()), 1);

    // the same again: a commit, but the status didn't change, so it isn't sent
    let (_, events) = h.run("value.set", &set(json!(0.7)), None).unwrap();
    assert_eq!(events.len(), 1, "{events:?}");

    // value.set goes through the schema: a key the kind lacks, out of range, the wrong kind
    assert!(
        !h.commands(&json!({"at": AT, "key": "nope", "value": 1}))
            .iter()
            .find(|c| c.id == "value.set")
            .unwrap()
            .enabled
    );
    assert!(h.run("value.set", &set(json!(2.0)), None).is_err());
    assert!(h.run("value.set", &set(json!("tight")), None).is_err());
    assert_eq!(tension(&pom), Some(0.7), "refusals change nothing");
}

#[test]
fn a_drag_is_one_undo_step() {
    let (h, pom, _, _) = loom();
    let steps = pom.read(|d| d.tree().history_len());
    for v in [0.6, 0.7, 0.9] {
        let (ran, _) = h
            .run("value.set", &set(json!(v)), Some("drag:tension"))
            .unwrap();
        assert!(matches!(ran, Ran::Committed(_)), "each run still commits");
    }
    assert_eq!(pom.read(|d| d.tree().history_len()), steps + 1);
    h.run("edit.undo", &json!({}), None).unwrap();
    assert_eq!(tension(&pom), Some(0.5));
}

#[test]
fn a_gesture_groups_commands() {
    let (h, pom, _, _) = loom();
    let steps = pom.read(|d| d.tree().history_len());
    let (token, events) = h.begin("Comb").unwrap();
    let s = last_status(&events).unwrap();
    assert!(s.gesture && s.undo.is_none(), "no undo mid-gesture: {s:?}");
    h.run("value.set", &set(json!(0.9)), None).unwrap();
    h.run(
        "value.set",
        &json!({"at": AT, "key": "thread.colour", "value": "red"}),
        None,
    )
    .unwrap();
    assert!(h.end(token + 1).is_err(), "only its own token ends it");
    let events = h.end(token).unwrap();
    assert!(!last_status(&events).unwrap().gesture);
    assert_eq!(pom.read(|d| d.tree().history_len()), steps + 1);

    // cancel takes it all back, as a commit
    let (token, _) = h.begin("Comb").unwrap();
    h.run("value.set", &set(json!(0.1)), None).unwrap();
    let events = h.cancel(token).unwrap();
    assert_eq!(commits(&events), 1);
    assert_eq!(tension(&pom), Some(0.9));

    // a gesture the front end lost is ended, keeping its edits: by a new one, or a reset
    let (stale, _) = h.begin("Comb").unwrap();
    h.run("value.set", &set(json!(0.2)), None).unwrap();
    let (fresh, _) = h.begin("Comb").unwrap();
    assert!(h.end(stale).is_err());
    h.end(fresh).unwrap();
    assert_eq!(tension(&pom), Some(0.2));
    h.begin("Comb").unwrap();
    let events = h.reset();
    assert!(
        !last_status(&events).unwrap().gesture,
        "reset always sends the status"
    );
    assert!(h.status().undo.is_some());
}

#[test]
fn files_replace_the_tree() {
    let (h, pom, store, calls) = loom();
    let events = h.save_as("/one".as_ref()).unwrap();
    let s = last_status(&events).unwrap();
    assert_eq!(s.path.as_deref(), Some("/one.loom"));
    assert_eq!((s.title.as_str(), s.unsaved), ("one.loom", false));
    assert!(store.get("/one.loom").is_some());

    // revert, new and open replace the whole tree: the generation moves, the hook runs
    h.run("value.set", &set(json!(0.9)), None).unwrap();
    let g = h.status().generation;
    let before = calls.load(Ordering::SeqCst);
    let (_, events) = h.run("file.revert", &json!({}), None).unwrap();
    assert_eq!(last_status(&events).unwrap().generation, g + 1);
    assert_eq!(calls.load(Ordering::SeqCst), before + 1);
    assert_eq!(tension(&pom), Some(0.5));

    h.begin("Comb").unwrap();
    let events = h.new_document().unwrap();
    let s = last_status(&events).unwrap();
    assert_eq!(
        (s.title.as_str(), s.gesture, s.generation),
        ("Untitled", false, g + 2)
    );
    assert_eq!(
        pom.read(|d| *d.projection()),
        0,
        "projected after the replace"
    );

    let (issues, events) = h.open("/one.loom".as_ref()).unwrap();
    assert!(issues.is_empty());
    assert_eq!(last_status(&events).unwrap().generation, g + 3);
    assert_eq!(tension(&pom), Some(0.5));
    assert!(h.tree().contains("\"warp\"") || h.tree().contains("/threads/warp"));

    // a file that isn't there leaves the document alone
    assert!(h.open("/nope.loom".as_ref()).is_err());
    assert_eq!(h.status().path.as_deref(), Some("/one.loom"));
    assert_eq!(h.extension(), "loom");
}

#[test]
fn the_wire_shapes() {
    let (h, _, _, _) = loom();
    assert_eq!(
        serde_json::to_value(h.status()).unwrap(),
        json!({
            "title": "Untitled — Edited",
            "path": null,
            "unsaved": true,
            "undo": "New thread",
            "redo": null,
            "gesture": false,
            "generation": 0
        })
    );
    let (ran, _) = h.run("value.set", &set(json!(0.8)), None).unwrap();
    let j = serde_json::to_value(&ran).unwrap();
    assert_eq!(j["committed"]["label"], "Set thread.tension");
    assert_eq!(j["committed"]["changes"][0]["change"], "value");
    assert_eq!(
        serde_json::to_value(Ran::Nothing).unwrap(),
        json!("nothing")
    );
    assert_eq!(
        serde_json::to_value(Ran::Text("x".into())).unwrap(),
        json!({"text": "x"})
    );
    let c = &h.commands(&json!({}))[0];
    assert_eq!(
        serde_json::to_value(c).unwrap(),
        json!({"id": "file.save", "label": "Save", "enabled": true})
    );
}

#[test]
fn a_host_goes_to_another_thread() {
    fn send_sync<T: Send + Sync + ?Sized>() {}
    send_sync::<dyn Host>();
    send_sync::<Pom<Loom>>();
}
