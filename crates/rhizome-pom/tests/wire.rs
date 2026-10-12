//! The wire shapes a transport carries from a `Host`: `View`, `Update`, `Status`, `Ran` and
//! `CommandState`, pinned in `tests/golden/*.json`. The TypeScript package (`src-ts/`) reads
//! these same files in its tests, so a change here that its types don't follow fails the
//! build. `RHIZOME_BLESS=1` rewrites them; read every changed line before committing.
//!
//! A made-up object model, *Loom*.

use std::path::PathBuf;
use std::sync::Arc;

use rhizome_core::{IdSource, NodeType, Origin, Tree};
use rhizome_pom::*;
use serde_json::{Value as Json, json};

struct Loom;

impl ObjectModel for Loom {
    const NAME: &'static str = "Loom";
    const EXTENSION: &'static str = "loom";
    type Projection = ();
    type Context = ();

    fn kinds(k: &mut Kinds, _: &()) {
        k.category("threads", Origin::Loaded);
        k.kind(
            NodeType::new("thread")
                .in_categories(&["threads"])
                .float("thread.tension", 0.0..=1.0, 0.5)
                .text("thread.colour", "white"),
        );
    }

    fn project(_: &Tree, _: &mut (), _: Option<&rhizome_core::Changeset>) {}
}

fn golden(name: &str, actual: &Json) {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    let text = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var_os("RHIZOME_BLESS").is_some() {
        std::fs::write(&file, text).expect("write golden");
        return;
    }
    let expected = std::fs::read_to_string(&file)
        .unwrap_or_else(|_| panic!("no golden {name}; run `just bless` and read it"));
    assert_eq!(expected, text, "{name} differs from its golden");
}

fn add(h: &Arc<dyn Host>, name: &str) -> Vec<Event> {
    h.run(
        "node.add",
        &json!({"parent": "/threads", "type": "thread", "name": name}),
        None,
    )
    .unwrap()
    .1
}

fn update(events: &[Event]) -> &Update {
    events
        .iter()
        .find_map(|e| match e {
            Event::Commit(u) => Some(u),
            _ => None,
        })
        .expect("a commit event")
}

fn status(events: &[Event]) -> &Status {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Status(s) => Some(s),
            _ => None,
        })
        .expect("a status event")
}

#[test]
fn every_wire_shape_of_the_host() {
    let store = MemoryStore::default();
    let pom =
        Arc::new(Pom::<Loom>::new(move || Box::new(store.clone()), IdSource::sequential).unwrap());
    let h = host(pom);
    let untitled = status(&add(&h, "warp")).clone();
    add(&h, "weft");

    golden("view.json", &serde_json::to_value(h.view()).unwrap());

    let (ran, set) = h
        .run(
            "value.set",
            &json!({"at": "/threads/warp", "key": "thread.tension", "value": 0.8}),
            None,
        )
        .unwrap();
    let (_, delete) = h
        .run("edit.delete", &json!({"at": "/threads/weft"}), None)
        .unwrap();
    golden(
        "update.json",
        &json!({
            "set": serde_json::to_value(update(&set)).unwrap(),
            "delete": serde_json::to_value(update(&delete)).unwrap(),
        }),
    );

    let (token, begun) = h.begin("Tangle").unwrap();
    let gesture = status(&begun).clone();
    h.end(token).unwrap();
    let saved = h.save_as("/looms/one.loom".as_ref()).unwrap();
    golden(
        "status.json",
        &json!({
            "untitled": serde_json::to_value(untitled).unwrap(),
            "gesture": serde_json::to_value(gesture).unwrap(),
            "saved": serde_json::to_value(status(&saved)).unwrap(),
        }),
    );

    golden(
        "ran.json",
        &json!({
            "nothing": serde_json::to_value(Ran::Nothing).unwrap(),
            "committed": serde_json::to_value(&ran).unwrap(),
            "text": serde_json::to_value(Ran::Text("a copied fragment".into())).unwrap(),
            "commands": serde_json::to_value(h.commands(&json!({}))).unwrap(),
        }),
    );
}
