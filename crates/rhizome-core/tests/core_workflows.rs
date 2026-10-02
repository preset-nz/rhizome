//! Core workflows: proof that what a person does with an app keeps working as the SDK changes.
//!
//! Each workflow in `tests/workflows/*.json` is data: steps of `Op` JSON, gestures, undo,
//! save, reopen, copy and paste. It drives only the stable surface: `Op`, the file format,
//! `Tree::load` / `serialise`, edits, gestures, undo and redo. Nothing here touches a Rust
//! verb, so refactoring the SDK can't break a workflow; changing what a workflow does can.
//!
//! Each workflow's transcript is pinned next to it as `*.txt`. A change to the SDK that
//! alters a transcript fails here. If the change is intended, `just bless` rewrites the
//! transcripts, and every changed line gets read before it is committed.
//!
//! The registry below is frozen. It belongs to these workflows, not to the acid test, and
//! changing it changes every transcript.

mod golden;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rhizome_core::{IdSource, NodeType, Op, Origin, Registry, Tree, ValueSpec};
use serde::Deserialize;

/// Frozen. A small studio: sounds and patches from Shard, images and masks from Oblique,
/// renders as the calculated case.
fn registry() -> Arc<Registry> {
    Registry::builder()
        .category("sounds", Origin::Loaded)
        .category("patches", Origin::Loaded)
        .category("modulators", Origin::Loaded)
        .category("images", Origin::Loaded)
        .category("masks", Origin::Loaded)
        .category("renders", Origin::Calculated)
        .node(
            NodeType::new("sample")
                .in_categories(&["sounds"])
                .float("gain", 0.0..=2.0, 1.0)
                .reference("file"),
        )
        .node(
            NodeType::new("patch")
                .in_categories(&["patches"])
                .float("amp.gain", 0.0..=1.0, 1.0)
                .reference("material"),
        )
        .node(
            NodeType::new("granular")
                .in_categories(&["patches"])
                .float("grain.size", 5.0..=500.0, 80.0)
                .float("grain.mix", 0.0..=1.0, 1.0)
                .slot("env"),
        )
        .node(
            NodeType::new("crush")
                .in_categories(&["patches"])
                .int("crush.bits", 1..=16, 8)
                .float("crush.mix", 0.0..=1.0, 0.0),
        )
        .node(
            NodeType::new("lfo")
                .in_categories(&["modulators"])
                .float("lfo.rate", 0.01..=20.0, 1.0)
                .choice("lfo.shape", &["sine", "triangle", "square"], "sine")
                .bindable([ValueSpec::float("depth", -1.0..=1.0, 0.5)]),
        )
        .node(
            NodeType::new("envelope")
                .in_categories(&["modulators"])
                .float("env.attack", 0.0..=5000.0, 10.0)
                .bindable([]),
        )
        .node(
            NodeType::new("image")
                .in_categories(&["images"])
                .float("opacity", 0.0..=1.0, 1.0)
                .choice("blend", &["normal", "multiply", "screen"], "normal")
                .reference("source")
                .slot("mask"),
        )
        .node(
            NodeType::new("blur")
                .in_categories(&["images"])
                .float("radius", 0.0..=200.0, 4.0),
        )
        .node(NodeType::new("mask").in_categories(&["masks"]).bindable([]))
        .node(
            NodeType::new("render")
                .in_categories(&["renders"])
                .reference("input"),
        )
        .build()
        .expect("registry")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Workflow {
    title: String,
    #[allow(dead_code)]
    why: String,
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum Step {
    /// Starts a new, empty file and makes it current.
    New(String),
    /// Opens a checked-in file from `tests/workflows/` and makes it current.
    Open(String),
    Switch(String),
    Edit {
        label: String,
        ops: Vec<Op>,
    },
    Begin(String),
    Apply(Vec<Op>),
    End,
    Cancel,
    /// A coalesced edit, as a knob or arrow key makes, `after_ms` after the last one.
    Nudge {
        label: String,
        key: String,
        after_ms: u64,
        ops: Vec<Op>,
    },
    Undo,
    Redo,
    Save,
    /// Throws the tree away and loads the last save.
    Reopen,
    Copy(Vec<String>),
    Paste(String),
    Status,
    /// Prints the current file.
    File,
}

/// What one step printed.
struct Out {
    step: String,
    lines: Vec<String>,
}

struct Session {
    files: BTreeMap<String, Tree>,
    saved: BTreeMap<String, String>,
    current: String,
    clipboard: Option<String>,
    now: Arc<Mutex<Instant>>,
    next_ids: u128,
    gesture: Option<rhizome_core::GestureId>,
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/workflows")
}

impl Session {
    fn new() -> Session {
        Session {
            files: BTreeMap::new(),
            saved: BTreeMap::new(),
            current: String::new(),
            clipboard: None,
            now: Arc::new(Mutex::new(Instant::now())),
            next_ids: 0,
            gesture: None,
        }
    }

    /// Each file draws ids from its own range, so two files never share one by accident.
    fn ids(&mut self) -> IdSource {
        self.next_ids += 1000;
        IdSource::Sequential {
            next: self.next_ids,
        }
    }

    fn adopt(&mut self, name: &str, mut tree: Tree) {
        let clock = self.now.clone();
        tree.set_clock(move || *clock.lock().unwrap());
        self.files.insert(name.to_string(), tree);
        self.current = name.to_string();
    }

    fn tree(&mut self) -> &mut Tree {
        self.files.get_mut(&self.current).expect("a current file")
    }

    fn committed(r: rhizome_core::Result<Option<rhizome_core::Commit>>) -> Vec<String> {
        match r {
            Ok(Some(c)) => c.changes.to_string().lines().map(String::from).collect(),
            Ok(None) => vec!["(no change)".into()],
            Err(e) => vec![format!("refused: {e}")],
        }
    }

    fn run(&mut self, step: &Step) -> Out {
        let (step, lines) = match step {
            Step::New(name) => {
                let ids = self.ids();
                self.adopt(name, Tree::with_ids(registry(), ids));
                (format!("new {name}"), vec![])
            }
            Step::Open(input) => {
                let text = std::fs::read_to_string(dir().join(input)).expect("input file");
                let ids = self.ids();
                let (tree, report) = Tree::load_with_ids(&text, registry(), ids).expect("loads");
                let mut lines: Vec<String> =
                    report.issues.iter().map(|i| format!("issue {i}")).collect();
                lines.push(format!(
                    "byte-identical on save: {}",
                    tree.serialise() == text
                ));
                self.saved.insert(input.clone(), text);
                self.adopt(input, tree);
                (format!("open {input}"), lines)
            }
            Step::Switch(name) => {
                self.current = name.clone();
                (format!("switch {name}"), vec![])
            }
            Step::Edit { label, ops } => (
                format!("edit {label}"),
                Self::committed(self.tree().edit_ops(label, ops)),
            ),
            Step::Begin(label) => {
                let g = self.tree().begin(label);
                let lines = match g {
                    Ok(g) => {
                        self.gesture = Some(g);
                        vec![]
                    }
                    Err(e) => vec![format!("refused: {e}")],
                };
                (format!("begin {label}"), lines)
            }
            Step::Apply(ops) => {
                let g = self.gesture.expect("an open gesture");
                ("apply".into(), Self::committed(self.tree().apply(g, ops)))
            }
            Step::End => {
                let g = self.gesture.take().expect("an open gesture");
                self.tree().end(g).expect("end");
                ("end".into(), vec![])
            }
            Step::Cancel => {
                let g = self.gesture.take().expect("an open gesture");
                ("cancel".into(), Self::committed(self.tree().cancel(g)))
            }
            Step::Nudge {
                label,
                key,
                after_ms,
                ops,
            } => {
                *self.now.lock().unwrap() += Duration::from_millis(*after_ms);
                let r = self
                    .tree()
                    .edit_coalesced(label, key, |tx| {
                        ops.iter().try_for_each(|op| tx.apply(op).map(drop))
                    })
                    .map(|(_, c)| c);
                (format!("nudge {label} +{after_ms}ms"), Self::committed(r))
            }
            Step::Undo => ("undo".into(), Self::committed(self.tree().undo())),
            Step::Redo => ("redo".into(), Self::committed(self.tree().redo())),
            Step::Save => {
                let text = self.tree().serialise();
                self.tree().mark_saved();
                let name = self.current.clone();
                self.saved.insert(name, text);
                ("save".into(), vec![])
            }
            Step::Reopen => {
                let text = self.saved.get(&self.current).expect("saved before").clone();
                let ids = self.ids();
                let (tree, report) = Tree::load_with_ids(&text, registry(), ids).expect("loads");
                let mut lines: Vec<String> =
                    report.issues.iter().map(|i| format!("issue {i}")).collect();
                lines.push(format!("byte-identical: {}", tree.serialise() == text));
                let name = self.current.clone();
                self.adopt(&name, tree);
                ("reopen".into(), lines)
            }
            Step::Copy(paths) => {
                let lines = match self.tree().extract(paths) {
                    Ok(f) => {
                        self.clipboard = Some(f.to_text());
                        vec![format!("{} nodes on the clipboard", f.len())]
                    }
                    Err(e) => vec![format!("refused: {e}")],
                };
                (format!("copy {}", paths.join(" ")), lines)
            }
            Step::Paste(parent) => {
                let op = Op::Paste {
                    parent: parent.clone(),
                    fragment: self.clipboard.clone().expect("something copied"),
                };
                let mut issues = Vec::new();
                let r = self
                    .tree()
                    .edit("Paste", |tx| {
                        issues = tx.apply(&op)?.issues;
                        Ok(())
                    })
                    .map(|(_, c)| c);
                let mut lines: Vec<String> = issues.iter().map(|i| format!("issue {i}")).collect();
                lines.extend(Self::committed(r));
                (format!("paste into {parent}"), lines)
            }
            Step::Status => {
                let t = self.tree();
                let line = format!(
                    "seq {}, {}, {} undo steps, undo: {}, redo: {}",
                    t.seq(),
                    if t.is_unsaved() { "unsaved" } else { "saved" },
                    t.history_len(),
                    t.undo_label().unwrap_or("-"),
                    t.redo_label().unwrap_or("-"),
                );
                ("status".into(), vec![line])
            }
            Step::File => {
                let text = self.tree().serialise();
                ("file".into(), text.lines().map(String::from).collect())
            }
        };
        Out { step, lines }
    }
}

fn run(name: &str) -> Vec<Out> {
    let text = std::fs::read_to_string(dir().join(format!("{name}.json"))).expect("workflow");
    let wf: Workflow = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}.json: {e}"));
    let mut session = Session::new();
    let outs: Vec<Out> = wf.steps.iter().map(|s| session.run(s)).collect();

    let mut transcript = format!("# {}\n", wf.title);
    for o in &outs {
        transcript += &format!("> {}\n", o.step);
        for l in &o.lines {
            transcript += &format!("  {l}\n");
        }
    }
    golden::check_file(&dir().join(format!("{name}.txt")), &transcript);
    outs
}

/// The lines a step printed, by its position in the workflow.
fn lines(outs: &[Out], i: usize) -> Vec<&str> {
    outs[i].lines.iter().map(String::as_str).collect()
}

#[test]
fn every_workflow_has_a_test() {
    let mut on_disk: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".json").map(String::from))
        .collect();
    on_disk.sort();
    let tested = [
        "01-build-save-reopen",
        "02-rename-is-a-move",
        "03-reorder-is-one-entry",
        "04-drag-is-one-undo-step",
        "05-cancel-a-drag",
        "06-delete-lets-go",
        "07-duplicate",
        "08-paste-into-another-file",
        "09-undo-to-saved-is-clean",
        "10-open-a-newer-file",
        "11-refusals-leave-no-trace",
        "12-knob-nudges-coalesce",
        "13-every-op",
    ];
    assert_eq!(on_disk, tested);
}

#[test]
fn build_save_reopen() {
    let o = run("01-build-save-reopen");
    assert_eq!(
        lines(&o, 2),
        ["seq 1, unsaved, 1 undo steps, undo: New Patch, redo: -"]
    );
    assert_eq!(
        lines(&o, 4),
        ["seq 1, saved, 1 undo steps, undo: New Patch, redo: -"]
    );
    assert_eq!(lines(&o, 5), ["byte-identical: true"]);
    assert_eq!(
        lines(&o, 6),
        ["seq 0, saved, 0 undo steps, undo: -, redo: -"],
        "a reopened file starts with no history"
    );
}

#[test]
fn rename_is_a_move() {
    let o = run("02-rename-is-a-move");
    assert_eq!(
        lines(&o, 2),
        ["/patches/hum  moved from /patches/drone"],
        "one entry, no children listed"
    );
    assert_eq!(lines(&o, 3), ["/patches/drone  moved from /patches/hum"]);
}

#[test]
fn reorder_is_one_entry() {
    let o = run("03-reorder-is-one-entry");
    assert_eq!(
        lines(&o, 2),
        ["/patches/drone  order chain  [crush, granular] → [granular, crush]"]
    );
}

#[test]
fn drag_is_one_undo_step() {
    let o = run("04-drag-is-one-undo-step");
    let status = o
        .iter()
        .filter(|x| x.step == "status")
        .map(|x| x.lines[0].as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        status[0],
        "seq 5, unsaved, 2 undo steps, undo: Drag Grain Size, redo: -"
    );
    let undo = o.iter().find(|x| x.step == "undo").unwrap();
    assert_eq!(
        undo.lines,
        ["/patches/drone/granular  grain.size  320.0 → unset"]
    );
}

#[test]
fn cancel_a_drag() {
    let o = run("05-cancel-a-drag");
    let cancel = o.iter().find(|x| x.step == "cancel").unwrap();
    assert!(cancel.lines.iter().any(|l| l.ends_with("removed")));
    let status = o.iter().rfind(|x| x.step == "status").unwrap();
    assert_eq!(
        status.lines[0],
        "seq 4, unsaved, 1 undo steps, undo: Build, redo: -"
    );
}

#[test]
fn delete_lets_go() {
    let o = run("06-delete-lets-go");
    let removed = lines(&o, 2);
    for want in [
        "/images  order draw  [sky, sea] → [sea]",
        "/images/fav  members  -/images/sky",
        "/images/sea  slot mask ← /masks/vignette  bound → unbound",
        "/images/sky  removed",
        "/images/sky/blur  removed",
        "/masks/vignette  removed",
    ] {
        assert!(removed.contains(&want), "missing {want:?} in {removed:#?}");
    }
    assert!(
        lines(&o, 4).contains(&"issue /renders/final: reference `input` points at a missing node")
    );
}

#[test]
fn duplicate() {
    let o = run("07-duplicate");
    assert_eq!(
        lines(&o, 2),
        [
            "/images  order draw  [sky, sea] → [sky, sky-2, sea]",
            "/images/fav  members  +/images/sky-2",
            "/images/sky-2  added image",
            "/images/sky-2/blur  added blur",
        ]
    );
}

#[test]
fn paste_into_another_file() {
    let o = run("08-paste-into-another-file");
    let paste = o.iter().find(|x| x.step.starts_with("paste")).unwrap();
    assert!(
        paste
            .lines
            .contains(&"issue /images/sky: not added to group /images/fav".to_string())
    );
}

#[test]
fn undo_to_saved_is_clean() {
    let o = run("09-undo-to-saved-is-clean");
    let status: Vec<&str> = o
        .iter()
        .filter(|x| x.step == "status")
        .map(|x| x.lines[0].as_str())
        .collect();
    assert!(
        status[1].contains("unsaved")
            && status[2].contains(", saved,")
            && status[3].contains("unsaved")
    );
}

#[test]
fn open_a_newer_file() {
    let o = run("10-open-a-newer-file");
    assert!(
        lines(&o, 0).contains(&"byte-identical on save: true"),
        "an unknown node passes through untouched"
    );
    assert!(
        lines(&o, 2)
            .iter()
            .any(|l| l.starts_with("refused:") && l.contains("doesn't know"))
    );
}

#[test]
fn refusals_leave_no_trace() {
    let o = run("11-refusals-leave-no-trace");
    for x in o.iter().filter(|x| x.step.starts_with("edit Bad")) {
        assert!(
            x.lines.len() == 1 && x.lines[0].starts_with("refused:"),
            "{}: {:?}",
            x.step,
            x.lines
        );
    }
    let status: Vec<&str> = o
        .iter()
        .filter(|x| x.step == "status")
        .map(|x| x.lines[0].as_str())
        .collect();
    assert_eq!(status[0], status[1], "refusals moved nothing");
}

#[test]
fn knob_nudges_coalesce() {
    let o = run("12-knob-nudges-coalesce");
    let status: Vec<&str> = o
        .iter()
        .filter(|x| x.step == "status")
        .map(|x| x.lines[0].as_str())
        .collect();
    assert_eq!(
        status,
        [
            "seq 6, unsaved, 3 undo steps, undo: Nudge Bits, redo: -",
            "seq 7, unsaved, 2 undo steps, undo: Nudge Bits, redo: Nudge Bits"
        ]
    );
}

#[test]
fn every_op() {
    let o = run("13-every-op");
    assert!(
        o.iter()
            .all(|x| !x.lines.iter().any(|l| l.starts_with("refused")))
    );
    let text = std::fs::read_to_string(dir().join("13-every-op.json")).unwrap();
    let mut seen: Vec<String> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("\"op\": \""))
        .map(|l| l.trim_end_matches(['"', ',']).to_string())
        .collect();
    seen.sort();
    seen.dedup();
    let every = [
        "add",
        "add_unique",
        "append_to_order",
        "bind",
        "clear_ref",
        "copy",
        "join",
        "leave",
        "move_to",
        "remove",
        "rename",
        "reset",
        "set",
        "set_order",
        "set_ref",
        "unbind",
    ];
    assert_eq!(
        seen, every,
        "every Op but paste, which the clipboard steps carry"
    );
    assert_eq!(
        lines(&o, 4),
        [
            "/patches/drone/granular  value grain.size ← /modulators/wobble  bound {depth=0.3} → unbound"
        ]
    );
}
