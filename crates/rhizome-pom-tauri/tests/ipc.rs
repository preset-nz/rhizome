//! The commands over Tauri's mock runtime: real IPC in, real events out. A made-up object
//! model, *Loom*.

use std::sync::{Arc, Mutex};

use rhizome_core::{IdSource, NodeType, Origin};
use rhizome_pom::{Kinds, MemoryStore, ObjectModel, Pom};
use rhizome_pom_tauri::{COMMIT, OPEN_DOCUMENT, PomHost, STATUS, commands};
use serde_json::{Value as Json, json};
use tauri::test::{
    INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder, mock_context, noop_assets,
};
use tauri::webview::InvokeRequest;
use tauri::{Listener, WebviewWindow};

struct Loom;

impl ObjectModel for Loom {
    const NAME: &'static str = "Loom";
    const EXTENSION: &'static str = "loom";
    type Projection = ();

    type Context = ();

    fn kinds(k: &mut Kinds, _: &()) {
        k.category("threads", Origin::Loaded);
        k.kind(NodeType::new("thread").in_categories(&["threads"]).float(
            "thread.tension",
            0.0..=1.0,
            0.5,
        ));
    }
}

struct App {
    app: tauri::App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    events: Arc<Mutex<Vec<(String, Json)>>>,
}

fn app() -> App {
    let store = MemoryStore::default();
    let pom =
        Arc::new(Pom::<Loom>::new(move || Box::new(store.clone()), IdSource::sequential).unwrap());
    let app = mock_builder()
        .manage(PomHost::new(pom))
        .invoke_handler(tauri::generate_handler![
            commands::pom_status,
            commands::pom_view,
            commands::pom_tree,
            commands::pom_commands,
            commands::pom_run,
            commands::pom_begin,
            commands::pom_end,
            commands::pom_cancel,
            commands::pom_connect,
            commands::pom_new,
            commands::pom_open,
            commands::pom_save_as,
        ])
        .build(mock_context(noop_assets()))
        .unwrap();
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    for name in [COMMIT, STATUS, OPEN_DOCUMENT] {
        let e = events.clone();
        app.listen_any(name, move |ev| {
            let payload = serde_json::from_str(ev.payload()).unwrap();
            e.lock().unwrap().push((name.to_string(), payload));
        });
    }
    App {
        app,
        window,
        events,
    }
}

impl App {
    fn invoke(&self, cmd: &str, args: Json) -> Result<Json, Json> {
        get_ipc_response(
            &self.window,
            InvokeRequest {
                cmd: cmd.into(),
                callback: tauri::ipc::CallbackFn(0),
                error: tauri::ipc::CallbackFn(1),
                url: self.window.url().unwrap(),
                body: tauri::ipc::InvokeBody::Json(args),
                headers: Default::default(),
                invoke_key: INVOKE_KEY.to_string(),
            },
        )
        .map(|b| b.deserialize::<Json>().unwrap())
    }

    fn take_events(&self) -> Vec<(String, Json)> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}

fn set(v: f64) -> Json {
    json!({"at": "/threads/warp", "key": "thread.tension", "value": v})
}

#[test]
fn commands_in_events_out() {
    let a = app();
    let ran = a
        .invoke(
            "pom_run",
            json!({"id": "node.add", "payload": {"parent": "/threads", "type": "thread", "name": "warp"}}),
        )
        .unwrap();
    assert_eq!(ran["committed"]["label"], "New thread");
    let events = a.take_events();
    assert_eq!(events[0].0, COMMIT);
    assert_eq!(events[0].1["changes"][0]["change"], "added");
    assert_eq!(
        events[0].1["rows"][0]["path"], "/threads/warp",
        "the fresh row"
    );
    assert_eq!(
        events[0].1["rows"][0]["values"]["thread.tension"], 0.5,
        "resolved"
    );
    let view = a.invoke("pom_view", json!({})).unwrap();
    assert_eq!(view["seq"], 1);
    let types = view["schema"]["types"].as_array().unwrap();
    assert!(types.iter().any(|t| t["name"] == "thread"), "{types:?}");
    assert_eq!(events[1].0, STATUS);
    assert_eq!(events[1].1["undo"], "New thread");

    // a drag: three runs, one undo step
    for v in [0.6, 0.7, 0.8] {
        a.invoke(
            "pom_run",
            json!({"id": "value.set", "payload": set(v), "coalesce": "drag"}),
        )
        .unwrap();
    }
    a.invoke("pom_run", json!({"id": "edit.undo"})).unwrap();
    assert!(
        a.invoke("pom_tree", json!({}))
            .unwrap()
            .as_str()
            .unwrap()
            .contains("warp")
    );
    let status = a.invoke("pom_status", json!({})).unwrap();
    assert_eq!(status["undo"], "New thread", "the drag was one step");

    // a gesture by token
    let token = a.invoke("pom_begin", json!({"label": "Comb"})).unwrap();
    a.invoke("pom_run", json!({"id": "value.set", "payload": set(0.9)}))
        .unwrap();
    a.take_events();
    a.invoke("pom_cancel", json!({"token": token})).unwrap();
    let events = a.take_events();
    assert!(
        events.iter().any(|(n, _)| n == COMMIT),
        "cancel commits its revert"
    );

    // refusals come back as errors, and change nothing
    let e = a
        .invoke("pom_run", json!({"id": "value.set", "payload": set(3.0)}))
        .unwrap_err();
    assert!(e.as_str().unwrap().contains("thread.tension"), "{e}");
    assert!(a.invoke("pom_end", json!({"token": 99})).is_err());

    // the menu's view of the commands
    let cmds = a.invoke("pom_commands", json!({"payload": {}})).unwrap();
    let ids: Vec<&str> = cmds
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"file.save") && ids.contains(&"value.set"));
}

#[test]
fn files_and_finder() {
    let a = app();
    a.invoke(
        "pom_run",
        json!({"id": "node.add", "payload": {"parent": "/threads", "type": "thread", "name": "warp"}}),
    )
    .unwrap();
    a.invoke("pom_save_as", json!({"path": "/one"})).unwrap();
    let status = a.invoke("pom_status", json!({})).unwrap();
    assert_eq!(status["path"], "/one.loom");
    assert_eq!(status["unsaved"], false);

    a.invoke("pom_new", json!({})).unwrap();
    assert_eq!(a.invoke("pom_status", json!({})).unwrap()["generation"], 1);
    let issues = a.invoke("pom_open", json!({"path": "/one.loom"})).unwrap();
    assert_eq!(issues, json!([]));
    assert_eq!(a.invoke("pom_status", json!({})).unwrap()["generation"], 2);
    assert!(a.invoke("pom_open", json!({"path": "/nope.loom"})).is_err());

    // a file from Finder before the front end listens is held, then handed over once
    let handle = a.app.handle();
    let url = |s: &str| -> tauri::Url { s.parse().unwrap() };
    rhizome_pom_tauri::opened(
        handle,
        &[url("file:///tmp/a.loom"), url("file:///tmp/b.wav")],
    );
    a.take_events();
    assert_eq!(a.invoke("pom_connect", json!({})).unwrap(), "/tmp/a.loom");
    assert!(
        a.take_events().iter().any(|(n, _)| n == STATUS),
        "connect sends the status"
    );
    assert_eq!(a.invoke("pom_connect", json!({})).unwrap(), Json::Null);
    a.take_events();

    // once it listens, a file from Finder is an event
    rhizome_pom_tauri::opened(handle, &[url("file:///tmp/c.loom")]);
    let events = a.take_events();
    assert_eq!(events, [(OPEN_DOCUMENT.to_string(), json!("/tmp/c.loom"))]);
}
