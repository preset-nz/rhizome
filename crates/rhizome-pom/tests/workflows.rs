//! Core workflows for POM: what a person does with any app built on it, driven only through
//! the stable surface. That is commands by id with JSON payloads (`Document::run`), `Op` JSON,
//! files, and preset reads. The workflows are data in `tests/workflows/*.json`; each
//! transcript is pinned beside it as `*.txt`. `RHIZOME_BLESS=1` rewrites them, and every
//! changed line gets read before it is committed.
//!
//! The object model below is made up and **frozen**. It belongs to these workflows;
//! changing it changes every transcript.

use std::path::PathBuf;

use rhizome_core::{Edit, Fragment, IdSource, Key, NodeId, NodeType, Op, Origin};
use rhizome_pom::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

struct Gazetteer;

const SIZE: Key<[f64; 2]> = Key::new("map.size");

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Palette {
    fill: String,
}

struct PaletteKind;

impl Aggregate for PaletteKind {
    type State = Palette;
    fn get(&self, _: rhizome_core::Node<'_>) -> Palette {
        Palette {
            fill: String::new(),
        }
    }
    fn set(&self, _: &mut Edit<'_>, _: NodeId, _: &Palette) -> rhizome_core::Result<Report> {
        Err(rhizome_core::Error::Structural(
            "palettes are chosen, not applied".into(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Aspect {
    ratio: f64,
}

struct AspectKind;

impl Aggregate for AspectKind {
    type State = Aspect;
    fn get(&self, n: rhizome_core::Node<'_>) -> Aspect {
        let [w, h] = n.get(SIZE).unwrap_or([1.0, 1.0]);
        Aspect { ratio: w / h }
    }
    fn set(&self, tx: &mut Edit<'_>, n: NodeId, s: &Aspect) -> rhizome_core::Result<Report> {
        let [w, h] = tx.at(n).unwrap().get(SIZE).unwrap();
        let long = w.max(h);
        tx.set(n, SIZE, [long, (long / s.ratio).round()])?;
        Ok(Report {
            applied: 1,
            skipped: vec![],
        })
    }
    fn matches(&self, a: &Aspect, b: &Aspect) -> bool {
        (a.ratio - b.ratio).abs() < 0.02
    }
    fn applies_to(&self, n: rhizome_core::Node<'_>) -> bool {
        n.type_name() == "map"
    }
}

#[derive(Deserialize)]
struct AddArgs {
    to: String,
    name: String,
}

fn add_map(tx: &mut Edit<'_>, to: &str, name: &str) -> rhizome_core::Result<NodeId> {
    let map = tx.add(to, "map", name)?;
    let paper = tx.add(map, "paper", "paper")?;
    let grid = tx.add(map, "grid", "grid")?;
    tx.set_order(map, "draw", [paper, grid])?;
    Ok(map)
}

fn add_layer(tx: &mut Edit<'_>, to: &str, name: &str) -> rhizome_core::Result<NodeId> {
    let l = tx.add(to, "layer", name)?;
    let mut draw: Vec<NodeId> = tx
        .at(to)
        .unwrap()
        .order("draw")
        .iter()
        .map(|n| n.id())
        .collect();
    draw.insert(draw.len().saturating_sub(1), l);
    tx.set_order(to, "draw", draw)?;
    Ok(l)
}

impl ObjectModel for Gazetteer {
    const NAME: &'static str = "Gazetteer";
    const EXTENSION: &'static str = "gaz";
    type Projection = ();

    fn kinds(k: &mut Kinds) {
        k.category("realms", Origin::Loaded);
        k.kind(NodeType::new("realm").in_categories(&["realms"]));
        k.kind(
            NodeType::new("map")
                .in_categories(&["realms"])
                .vec2(SIZE, [800.0, 600.0])
                .int("map.zoom", 1..=8, 1),
        );
        k.kind(NodeType::new("paper").in_categories(&["realms"]))
            .not_deletable()
            .not_duplicable()
            .max_per_parent(1)
            .pinned_first("draw");
        k.kind(NodeType::new("grid").in_categories(&["realms"]))
            .not_deletable()
            .not_duplicable()
            .max_per_parent(1)
            .pinned_last("draw");
        k.kind(
            NodeType::new("layer")
                .in_categories(&["realms"])
                .int("layer.seed", 0..=999, 0),
        );
    }

    fn presets(p: &mut Presets) {
        let pal = |f: &str| Palette { fill: f.into() };
        p.kind("palette", PaletteKind)
            .catalogue([
                ("ember", pal("#3a1c12")),
                ("night", pal("#0b1030")),
                ("tide", pal("#0a2a30")),
            ])
            .fallback("ember")
            .followed_by(&["realm", "map"]);
        p.kind("aspect", AspectKind).catalogue([
            ("4x3", Aspect { ratio: 4.0 / 3.0 }),
            ("16x9", Aspect { ratio: 16.0 / 9.0 }),
            ("square", Aspect { ratio: 1.0 }),
        ]);
        p.kind("style", NodeValues::new().skip(|k| k == "map.size"))
            .followed_by(&["map"]);
    }

    fn commands(c: &mut Commands<Self>) {
        let exists = |d: &Document<Gazetteer>, p: &Json| {
            payload::<AddArgs>(p).is_ok_and(|a| d.tree().at(a.to.as_str()).is_some())
        };
        c.add(
            "map.add",
            |_| "New Map".into(),
            exists,
            |d, p| {
                let a: AddArgs = payload(p)?;
                Ok(d.edit("New Map", |tx| add_map(tx, &a.to, &a.name))?
                    .1
                    .into())
            },
        );
        c.add(
            "layer.add",
            |_| "New Layer".into(),
            exists,
            |d, p| {
                let a: AddArgs = payload(p)?;
                Ok(d.edit("New Layer", |tx| add_layer(tx, &a.to, &a.name))?
                    .1
                    .into())
            },
        );
    }
}

// ---------------------------------------------------------------- the runner

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workflow {
    title: String,
    #[allow(dead_code)]
    why: String,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetQuery {
    kind: String,
    at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Menu {
    payload: Json,
    ids: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    New,
    Run {
        id: String,
        payload: Json,
    },
    Edit {
        label: String,
        ops: Vec<Op>,
    },
    /// Each command's label and whether it's enabled for one payload.
    Menu(Menu),
    Status,
    Names(PresetQuery),
    Current(PresetQuery),
    Resolve(PresetQuery),
    SaveAs(String),
    Reopen,
}

fn preset(r: &PresetRef) -> String {
    match r {
        PresetRef::Catalogue(n) => n.clone(),
        PresetRef::User(l) => format!("“{l}”"),
    }
}

struct Session {
    store: MemoryStore,
    doc: Option<Document<Gazetteer>>,
    clipboard: Option<String>,
}

impl Session {
    fn doc(&mut self) -> &mut Document<Gazetteer> {
        self.doc.as_mut().expect("a document")
    }

    fn node(&mut self, at: &str) -> NodeId {
        self.doc()
            .tree()
            .at(at)
            .unwrap_or_else(|| panic!("no node at {at}"))
            .id()
    }

    fn step(&mut self, s: &Step) -> (String, Vec<String>) {
        let committed = |r: Result<Option<rhizome_core::Commit>>| match r {
            Ok(Some(c)) => c.changes.to_string().lines().map(String::from).collect(),
            Ok(None) => vec!["(no change)".into()],
            Err(e) => vec![format!("refused: {e}")],
        };
        match s {
            Step::New => {
                self.doc = Some(
                    Document::new_with_ids(self.store.clone(), IdSource::sequential()).unwrap(),
                );
                ("new".into(), vec![])
            }
            Step::Run { id, payload } => {
                // a paste payload without a fragment takes the clipboard
                let mut payload = payload.clone();
                if id == "edit.paste" && payload.get("fragment").is_none() {
                    payload["fragment"] = json!(self.clipboard.clone().expect("copied first"));
                }
                let lines = match self.doc().run(id, &payload) {
                    Ok(Outcome::Committed(c)) => {
                        c.changes.to_string().lines().map(String::from).collect()
                    }
                    Ok(Outcome::Nothing) => vec!["(no change)".into()],
                    Ok(Outcome::Text(t)) => {
                        let n = Fragment::from_text(&t).map(|f| f.len()).unwrap_or(0);
                        self.clipboard = Some(t);
                        vec![format!("{n} nodes on the clipboard")]
                    }
                    Err(e) => vec![format!("refused: {e}")],
                };
                // the fragment is clipboard text; the step shows where it went, not what it was
                let mut shown = payload.clone();
                if let Some(o) = shown.as_object_mut() {
                    o.remove("fragment");
                }
                (format!("run {id} {shown}"), lines)
            }
            Step::Edit { label, ops } => (
                format!("edit {label}"),
                committed(self.doc().edit_ops(label, ops)),
            ),
            Step::Menu(m) => {
                let lines = m
                    .ids
                    .iter()
                    .map(|id| {
                        let d = self.doc();
                        let on = if d.is_enabled(id, &m.payload).unwrap() {
                            "on"
                        } else {
                            "off"
                        };
                        format!("{id}  “{}”  {on}", d.label(id).unwrap())
                    })
                    .collect();
                (format!("menu for {}", m.payload), lines)
            }
            Step::Status => {
                let d = self.doc();
                let line = format!(
                    "{}, undo: {}, redo: {}",
                    d.title(),
                    d.tree().undo_label().unwrap_or("-"),
                    d.tree().redo_label().unwrap_or("-")
                );
                ("status".into(), vec![line])
            }
            Step::Names(q) => {
                let n = self.node(&q.at);
                let names = self.doc().preset_names(&q.kind, n).unwrap();
                let line = names.iter().map(preset).collect::<Vec<_>>().join(", ");
                (
                    format!("names {} at {}", q.kind, q.at),
                    vec![if line.is_empty() {
                        "(none)".into()
                    } else {
                        line
                    }],
                )
            }
            Step::Current(q) => {
                let n = self.node(&q.at);
                let c = self.doc().current_preset(&q.kind, n).unwrap();
                (
                    format!("current {} at {}", q.kind, q.at),
                    vec![c.as_ref().map_or("(none)".into(), preset)],
                )
            }
            Step::Resolve(q) => {
                let n = self.node(&q.at);
                let r = self.doc().resolve_preset(&q.kind, n).unwrap();
                let line = match r {
                    None => "(nothing)".to_string(),
                    Some(r) => {
                        let from = r
                            .follower
                            .map(|f| self.doc().tree().get(f).unwrap().path().to_string())
                            .unwrap_or_else(|| "the fallback".into());
                        format!("{} from {from}  {}", preset(&r.preset), r.state)
                    }
                };
                (format!("resolve {} at {}", q.kind, q.at), vec![line])
            }
            Step::SaveAs(path) => {
                let r = self.doc().save_as(path);
                (
                    format!("save as {path}"),
                    r.err()
                        .map(|e| vec![format!("refused: {e}")])
                        .unwrap_or_default(),
                )
            }
            Step::Reopen => {
                let path = self.doc().path().expect("saved").to_path_buf();
                let (doc, report) = Document::open_with_ids(
                    self.store.clone(),
                    &path,
                    IdSource::Sequential { next: 1000 },
                )
                .unwrap();
                self.doc = Some(doc);
                let lines = report.issues.iter().map(|i| format!("issue {i}")).collect();
                ("reopen".into(), lines)
            }
        }
    }
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/workflows")
}

fn run(name: &str) -> Vec<(String, Vec<String>)> {
    let text = std::fs::read_to_string(dir().join(format!("{name}.json"))).unwrap();
    let wf: Workflow = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}.json: {e}"));
    let mut s = Session {
        store: MemoryStore::default(),
        doc: None,
        clipboard: None,
    };
    let out: Vec<(String, Vec<String>)> = wf.steps.iter().map(|st| s.step(st)).collect();
    let mut transcript = format!("# {}\n", wf.title);
    for (step, lines) in &out {
        transcript += &format!("> {step}\n");
        for l in lines {
            transcript += &format!("  {l}\n");
        }
    }
    let golden = dir().join(format!("{name}.txt"));
    if std::env::var_os("RHIZOME_BLESS").is_some() {
        std::fs::write(&golden, &transcript).unwrap();
    } else {
        let expected = std::fs::read_to_string(&golden)
            .unwrap_or_else(|_| panic!("no {name}.txt; bless and read it"));
        assert_eq!(expected, transcript, "{name} differs from its transcript");
    }
    out
}

fn lines<'a>(out: &'a [(String, Vec<String>)], step: &str) -> Vec<&'a str> {
    out.iter()
        .find(|(s, _)| s.starts_with(step))
        .unwrap_or_else(|| panic!("no step {step}"))
        .1
        .iter()
        .map(String::as_str)
        .collect()
}

#[test]
fn every_workflow_has_a_test() {
    let mut on_disk: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| {
            e.ok()?
                .file_name()
                .into_string()
                .ok()?
                .strip_suffix(".json")
                .map(String::from)
        })
        .collect();
    on_disk.sort();
    assert_eq!(
        on_disk,
        [
            "01-new-map-with-anchors",
            "02-copy-a-layer-to-another-map",
            "03-palette-by-cascade",
            "04-user-presets",
            "05-aspect-preset",
            "06-policy-refusals",
        ]
    );
}

#[test]
fn new_map_with_anchors() {
    let o = run("01-new-map-with-anchors");
    let menu = lines(&o, "menu for {\"at\":\"/realms/west/north/paper\"}");
    assert!(
        menu.contains(&"edit.delete  “Delete”  off")
            && menu.contains(&"edit.duplicate  “Duplicate”  off"),
        "{menu:?}"
    );
    let menu = lines(&o, "menu for {\"at\":\"/realms/west/north/hills\"}");
    assert!(menu.contains(&"edit.delete  “Delete”  on"), "{menu:?}");
}

#[test]
fn copy_a_layer_to_another_map() {
    let o = run("02-copy-a-layer-to-another-map");
    let paste = lines(&o, "run edit.paste");
    assert!(
        paste.contains(&"/realms/west/south  order draw  [paper, grid] → [paper, hills, grid]"),
        "{paste:?}"
    );
}

#[test]
fn palette_by_cascade() {
    let o = run("03-palette-by-cascade");
    assert!(o.iter().filter(|(s, _)| s.starts_with("resolve")).count() >= 4);
    assert_eq!(
        o.last().unwrap().1[0],
        "night from /realms/west  {\"fill\":\"#0b1030\"}",
        "the choice survives a save"
    );
}

#[test]
fn user_presets() {
    run("04-user-presets");
}

#[test]
fn aspect_preset() {
    run("05-aspect-preset");
}

#[test]
fn policy_refusals() {
    let o = run("06-policy-refusals");
    for (step, out) in o
        .iter()
        .filter(|(s, _)| s.starts_with("edit Bad") || s.starts_with("run edit.delete"))
    {
        assert!(
            out.len() == 1 && out[0].starts_with("refused:"),
            "{step}: {out:?}"
        );
    }
    let status: Vec<&String> = o
        .iter()
        .filter(|(s, _)| s == "status")
        .map(|(_, l)| &l[0])
        .collect();
    assert_eq!(status[0], status[1], "refusals moved nothing");
}
