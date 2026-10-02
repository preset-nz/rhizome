//! POM against two made-up object models of different shapes. Neither is an app's real one.
//!
//! - **Synth**: values and bindings on nodes, user presets of one node's sound that skip its
//!   on/off switch, a compiled projection.
//! - **Atlas**: a document of maps with anchored, singleton layers (policy), a palette theme
//!   built from rhizome's primitives (decision 39), a computed aspect preset, domain verbs.

use rhizome_core::{
    Changeset, Edit, IdSource, Key, NodeId, NodeType, On, Origin, Tree, Value, ValueSpec,
};
use rhizome_pom::*;
use serde::{Deserialize, Serialize};
use serde_json::json;

// ---------------------------------------------------------------- Synth

struct Synth;

const ON: Key<bool> = Key::new("osc.on");
const PITCH: Key<f64> = Key::new("osc.pitch");
const MIX: Key<f64> = Key::new("osc.mix");

#[derive(Default)]
struct Plan {
    playing: Vec<(String, f64)>,
    builds: usize,
}

impl ObjectModel for Synth {
    const NAME: &'static str = "Synth";
    const EXTENSION: &'static str = "synth";
    type Projection = Plan;

    fn kinds(k: &mut Kinds) {
        k.category("voices", Origin::Loaded)
            .category("mods", Origin::Loaded);
        k.kind(NodeType::new("voice").in_categories(&["voices"]).float(
            "voice.gain",
            0.0..=1.0,
            1.0,
        ));
        k.kind(
            NodeType::new("osc")
                .in_categories(&["voices"])
                .bool(ON, false)
                .float(MIX, 0.0..=1.0, 0.0)
                .float(PITCH, -24.0..=24.0, 0.0),
        )
        .presets(
            NodeValues::new()
                .skip(|k| k.ends_with(".on"))
                .with_bindings(),
        );
        k.kind(
            NodeType::new("lfo")
                .in_categories(&["mods"])
                .float("lfo.rate", 0.01..=20.0, 1.0)
                .bindable([ValueSpec::float("depth", -1.0..=1.0, 0.5)]),
        );
    }

    fn project(tree: &Tree, into: &mut Plan, _changes: Option<&Changeset>) {
        into.builds += 1;
        into.playing = tree
            .nodes()
            .iter()
            .filter(|n| n.type_name() == "osc" && n.get(ON) == Some(true))
            .map(|n| (n.path().to_string(), n.get(PITCH).unwrap()))
            .collect();
    }
}

fn synth() -> (Document<Synth>, MemoryStore, NodeId, NodeId) {
    let store = MemoryStore::default();
    let mut d = Document::<Synth>::new_with_ids(store.clone(), IdSource::sequential()).unwrap();
    let ((osc, lfo), _) = d
        .edit("Build", |tx| {
            let v = tx.add("/voices", "voice", "pad")?;
            let osc = tx.add(v, "osc", "a")?;
            let lfo = tx.add("/mods", "lfo", "wobble")?;
            Ok((osc, lfo))
        })
        .unwrap();
    (d, store, osc, lfo)
}

#[test]
fn synth_presets_of_one_nodes_sound() {
    let (mut d, _, osc, lfo) = synth();
    d.edit("Shape", |tx| {
        tx.set(osc, ON, true)?;
        tx.set(osc, PITCH, 7.0)?;
        tx.set(osc, MIX, 0.5)?;
        tx.bind(osc, On::value(PITCH), lfo, [("depth", Value::Float(0.3))])
    })
    .unwrap();
    d.save_preset(osc, "Warm").unwrap();
    assert_eq!(
        d.current_preset(osc).unwrap(),
        Some(PresetRef::User("Warm".into()))
    );

    // change the sound and switch it off: the preset puts the sound back, not the switch
    d.edit("Change", |tx| {
        tx.set(osc, ON, false)?;
        tx.set(osc, PITCH, -12.0)?;
        tx.reset(osc, MIX)?;
        tx.unbind(osc, On::value(PITCH), lfo)
    })
    .unwrap();
    assert_eq!(d.current_preset(osc).unwrap(), None);
    let steps = d.tree().history_len();
    let (report, commit) = d
        .apply_preset(osc, &PresetRef::User("Warm".into()))
        .unwrap();
    assert!(commit.is_some());
    assert_eq!(
        d.tree().history_len(),
        steps + 1,
        "an apply is one undo step"
    );
    assert_eq!(report.applied, 3, "pitch, mix and the link: {report:?}");
    assert!(report.skipped.is_empty(), "{report:?}");
    let n = d.tree().get(osc).unwrap();
    assert_eq!(
        (n.get(PITCH), n.get(MIX), n.get(ON)),
        (Some(7.0), Some(0.5), Some(false))
    );
    assert_eq!(n.bindings()[0].value("depth"), Some(Value::Float(0.3)));
    assert_eq!(
        d.current_preset(osc).unwrap(),
        Some(PresetRef::User("Warm".into()))
    );
    d.undo().unwrap();
    assert_eq!(d.tree().get(osc).unwrap().get(PITCH), Some(-12.0));

    // Shard's rules: save refuses a taken name, update and the rest refuse a missing one
    let err = d.save_preset(osc, "Warm").unwrap_err().to_string();
    assert!(err.contains("already exists"), "{err}");
    assert!(d.update_preset(osc, "Cold").is_err());
    assert!(
        d.save_preset(osc, "   ").is_err(),
        "a name needs characters"
    );
    d.update_preset(osc, "Warm").unwrap();
    d.save_preset(osc, "Cold").unwrap();
    d.rename_preset(osc, "Cold", "Icy").unwrap();
    assert!(
        d.rename_preset(osc, "Icy", "Warm").is_err(),
        "rename refuses a taken name"
    );
    assert_eq!(
        d.preset_names(osc).unwrap(),
        [
            PresetRef::User("Icy".into()),
            PresetRef::User("Warm".into())
        ]
    );
    d.delete_preset(osc, "Icy").unwrap();
    assert_eq!(
        d.preset_names(osc).unwrap(),
        [PresetRef::User("Warm".into())]
    );

    // presets are document data: saved, and back after open
    d.save_as("/songs/one").unwrap();
    assert_eq!(d.path().unwrap().to_str(), Some("/songs/one.synth"));
    let store = MemoryStore::default();
    store.put("/x.synth", &d.tree().serialise());
    let (again, report) = Document::<Synth>::open(store, "/x.synth").unwrap();
    assert!(report.issues.is_empty());
    assert_eq!(
        again.preset_names(osc).unwrap(),
        [PresetRef::User("Warm".into())]
    );
}

/// A Synth with an osc and no lfo, its ids unlike the source's.
fn other_synth() -> (Document<Synth>, NodeId) {
    let mut d = Document::<Synth>::new(MemoryStore::default()).unwrap();
    let (osc, _) = d
        .edit("Build", |tx| {
            let v = tx.add("/voices", "voice", "lead")?;
            tx.add(v, "osc", "b")
        })
        .unwrap();
    (d, osc)
}

#[test]
fn user_presets_travel_to_another_document() {
    let (mut d, _, osc, lfo) = synth();
    d.edit("Shape", |tx| {
        tx.set(osc, PITCH, 7.0)?;
        tx.set(osc, MIX, 0.5)?;
        tx.bind(osc, On::value(PITCH), lfo, [("depth", Value::Float(0.3))])
    })
    .unwrap();
    d.save_preset(osc, "Warm").unwrap();
    d.edit("Shape", |tx| tx.set(osc, PITCH, -5.0)).unwrap();
    d.save_preset(osc, "Low").unwrap();
    let names = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    // a preset file: plain JSON, one kind, its presets; bindings stay behind
    let text = d.export_presets(osc, &names(&["Warm", "Low"])).unwrap();
    let file: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        file,
        json!({
            "preset": 1,
            "for": "osc",
            "presets": [
                {"label": "Warm", "state": {"values": {"osc.mix": 0.5, "osc.pitch": 7.0}}},
                {"label": "Low", "state": {"values": {"osc.mix": 0.5, "osc.pitch": -5.0}}},
            ]
        })
    );
    assert!(d.export_presets(osc, &names(&["Cold"])).is_err());
    assert!(d.export_presets(osc, &[]).is_err());

    let (mut e, b) = other_synth();
    let steps = e.tree().history_len();
    let (labels, commit) = e.import_presets(&text).unwrap();
    assert_eq!(labels, ["Warm", "Low"]);
    assert!(commit.is_some());
    assert_eq!(
        e.tree().history_len(),
        steps + 1,
        "an import is one undo step"
    );
    assert_eq!(
        e.preset_names(b).unwrap(),
        [
            PresetRef::User("Low".into()),
            PresetRef::User("Warm".into())
        ]
    );
    let (report, _) = e.apply_preset(b, &PresetRef::User("Warm".into())).unwrap();
    assert_eq!(report.applied, 2, "pitch and mix: {report:?}");
    assert!(report.skipped.is_empty(), "{report:?}");
    assert!(e.tree().get(b).unwrap().bindings().is_empty());
    assert_eq!(e.tree().get(b).unwrap().get(PITCH), Some(7.0));

    // all or nothing: one taken name refuses the whole file
    let before = e.tree().serialise();
    let err = e.import_presets(&text).unwrap_err().to_string();
    assert!(err.contains("already exists"), "{err}");
    assert_eq!(e.tree().serialise(), before);
    e.undo().unwrap();
    e.undo().unwrap();
    assert!(
        e.preset_names(b).unwrap().is_empty(),
        "undo takes them out again"
    );

    // refused: not a preset file, another version, no presets, a kind this model lacks,
    // a state that doesn't fit, a bad or repeated name
    let refuse = |e: &mut Document<Synth>, file: serde_json::Value, says: &str| {
        let err = e.import_presets(&file.to_string()).unwrap_err().to_string();
        assert!(err.contains(says), "{says}: {err}");
    };
    let ok = json!({"values": {"osc.pitch": 1.0}});
    assert!(e.import_presets("not json").is_err());
    let fragment = d.tree().extract([osc]).unwrap().to_text();
    assert!(e.import_presets(&fragment).is_err());
    refuse(
        &mut e,
        json!({"preset": 2, "for": "osc", "presets": [{"label": "A", "state": ok}]}),
        "version 2",
    );
    refuse(
        &mut e,
        json!({"preset": 1, "for": "osc", "presets": []}),
        "no presets",
    );
    refuse(
        &mut e,
        json!({"preset": 1, "for": "map", "presets": [{"label": "A", "state": ok}]}),
        "no map presets",
    );
    refuse(
        &mut e,
        json!({"preset": 1, "for": "lfo", "presets": [{"label": "A", "state": ok}]}),
        "no lfo presets",
    );
    refuse(
        &mut e,
        json!({"preset": 1, "for": "osc", "presets": [{"label": "A", "state": "nope"}]}),
        "doesn't fit",
    );
    refuse(
        &mut e,
        json!({"preset": 1, "for": "osc", "presets": [{"label": " ", "state": ok}]}),
        "1 to 60",
    );
    refuse(
        &mut e,
        json!({"preset": 1, "for": "osc", "presets": [{"label": "A", "state": ok}, {"label": "A", "state": ok}]}),
        "already exists",
    );
    assert!(e.preset_names(b).unwrap().is_empty());

    // and as commands
    let Outcome::Text(t) = d
        .run(
            "preset.export",
            &json!({"at": "/voices/pad/a", "labels": ["Warm"]}),
        )
        .unwrap()
    else {
        panic!()
    };
    let Outcome::Committed(_) = e.run("preset.import", &json!({"text": t})).unwrap() else {
        panic!()
    };
    assert!(
        !e.is_enabled("preset.import", &json!({"text": "nope"}))
            .unwrap()
    );
}

#[test]
fn synth_document_lifecycle_and_projection() {
    let (mut d, store, osc, _) = synth();
    assert_eq!(d.title(), "Untitled — Edited");
    assert_eq!(
        d.projection().builds,
        2,
        "built on open, rebuilt after the first commit"
    );
    assert!(matches!(d.save(), Err(Error::NoPath)));
    d.save_as("/songs/a.synth").unwrap();
    assert_eq!(d.title(), "a.synth");
    assert!(store.get("/songs/a.synth").is_some());

    d.edit("On", |tx| tx.set(osc, ON, true)).unwrap();
    assert_eq!(d.title(), "a.synth — Edited");
    assert_eq!(d.projection().playing, [("/voices/pad/a".to_string(), 0.0)]);
    d.undo().unwrap();
    assert!(d.projection().playing.is_empty(), "undo re-projects");
    d.redo().unwrap();
    assert_eq!(d.projection().playing.len(), 1);

    let g = d.begin("Drag Pitch").unwrap();
    for v in [3, 5, 9] {
        d.apply(
            g,
            &[serde_json::from_value(
                json!({"op": "set", "at": "/voices/pad/a", "key": "osc.pitch", "value": v as f64}),
            )
            .unwrap()],
        )
        .unwrap();
    }
    assert_eq!(d.projection().playing[0].1, 9.0, "every apply re-projects");
    d.cancel(g).unwrap();
    assert_eq!(d.projection().playing[0].1, 0.0, "cancel re-projects");

    d.revert().unwrap();
    assert!(
        !d.is_unsaved() && d.projection().playing.is_empty(),
        "revert reloads the file"
    );
    assert_eq!(d.tree().history_len(), 0);
}

// ---------------------------------------------------------------- Atlas

struct Atlas;

const SIZE: Key<[f64; 2]> = Key::new("map.size");
const SEED: Key<i64> = Key::new("terrain.seed");

const FILL: &str = "palette.fill";
const LINE: &str = "palette.line";
const PALETTE: &str = "palette";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Aspect {
    ratio: f64,
}

/// The map kind's presets. The state is derived: a ratio, not the stored size. The
/// setter computes the size, keeping the long edge.
struct AspectKind;

impl Aggregate for AspectKind {
    type State = Aspect;
    fn get(&self, node: rhizome_core::Node<'_>) -> Aspect {
        let [w, h] = node.get(SIZE).unwrap_or([1.0, 1.0]);
        Aspect { ratio: w / h }
    }
    fn set(&self, tx: &mut Edit<'_>, node: NodeId, s: &Aspect) -> rhizome_core::Result<Report> {
        let [w, h] = tx.at(node).unwrap().get(SIZE).unwrap();
        let long = w.max(h);
        tx.set(node, SIZE, [long, (long / s.ratio).round()])?;
        Ok(Report {
            applied: 1,
            skipped: vec![],
        })
    }
    fn matches(&self, now: &Aspect, p: &Aspect) -> bool {
        (now.ratio - p.ratio).abs() < 0.02
    }
}

impl ObjectModel for Atlas {
    const NAME: &'static str = "Atlas";
    const EXTENSION: &'static str = "atlas";
    type Projection = Vec<String>;

    fn kinds(k: &mut Kinds) {
        k.category("campaigns", Origin::Loaded);
        k.category("themes", Origin::Loaded);
        // a theme is a node with values and no op; followers hold a reference to it
        k.kind(
            NodeType::new(PALETTE)
                .in_categories(&["themes"])
                .text(FILL, "#000000")
                .text(LINE, "#ffffff"),
        );
        k.kind(
            NodeType::new("campaign")
                .in_categories(&["campaigns"])
                .reference(PALETTE),
        );
        k.kind(
            NodeType::new("map")
                .in_categories(&["campaigns"])
                .vec2(SIZE, [800.0, 600.0])
                .reference(PALETTE),
        )
        .presets(AspectKind)
        .catalogue([
            ("4x3", Aspect { ratio: 4.0 / 3.0 }),
            ("16x9", Aspect { ratio: 16.0 / 9.0 }),
            ("square", Aspect { ratio: 1.0 }),
        ]);
        k.kind(NodeType::new("paper").in_categories(&["campaigns"]))
            .not_deletable()
            .not_duplicable()
            .max_per_parent(1)
            .pinned_first("draw");
        k.kind(NodeType::new("grid").in_categories(&["campaigns"]))
            .not_deletable()
            .not_duplicable()
            .max_per_parent(1)
            .pinned_last("draw");
        let seeded = |seed: i64| NodeValuesState {
            values: [("terrain.seed".to_string(), json!(seed))].into(),
            bindings: vec![],
        };
        k.kind(
            NodeType::new("terrain")
                .in_categories(&["campaigns"])
                .int(SEED, 0..=999, 0),
        )
        .presets(NodeValues::new())
        .catalogue([("rocky", seeded(7)), ("plains", seeded(42))]);
    }

    fn project(tree: &Tree, into: &mut Vec<String>, _changes: Option<&Changeset>) {
        *into = tree
            .nodes()
            .iter()
            .filter(|n| n.type_name() == "map")
            .flat_map(|m| {
                m.order("draw")
                    .into_iter()
                    .map(|l| l.path().to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
    }
}

/// Atlas's domain verbs: what makes a map a map.
trait AtlasEdit {
    fn add_map(&mut self, campaign: NodeId, name: &str) -> rhizome_core::Result<NodeId>;
    fn add_terrain(&mut self, map: NodeId, name: &str) -> rhizome_core::Result<NodeId>;
}

impl AtlasEdit for Edit<'_> {
    fn add_map(&mut self, campaign: NodeId, name: &str) -> rhizome_core::Result<NodeId> {
        let map = self.add(campaign, "map", name)?;
        let paper = self.add(map, "paper", "paper")?;
        let grid = self.add(map, "grid", "grid")?;
        self.set_order(map, "draw", [paper, grid])?;
        Ok(map)
    }

    fn add_terrain(&mut self, map: NodeId, name: &str) -> rhizome_core::Result<NodeId> {
        let t = self.add(map, "terrain", name)?;
        let mut draw: Vec<NodeId> = self
            .at(map)
            .unwrap()
            .order("draw")
            .iter()
            .map(|n| n.id())
            .collect();
        draw.insert(draw.len() - 1, t); // under the grid
        self.set_order(map, "draw", draw)?;
        Ok(t)
    }
}

fn atlas() -> (Document<Atlas>, NodeId, NodeId) {
    let mut d =
        Document::<Atlas>::new_with_ids(MemoryStore::default(), IdSource::sequential()).unwrap();
    let ((campaign, map), _) = d
        .edit("Add Map", |tx| {
            let c = tx.add("/campaigns", "campaign", "realm")?;
            let m = tx.add_map(c, "north")?;
            tx.add_terrain(m, "hills")?;
            Ok((c, m))
        })
        .unwrap();
    (d, campaign, map)
}

#[test]
fn atlas_policy_holds_however_the_tree_is_edited() {
    let (mut d, _, map) = atlas();
    assert_eq!(
        d.projection(),
        &[
            "/campaigns/realm/north/paper",
            "/campaigns/realm/north/hills",
            "/campaigns/realm/north/grid"
        ]
    );
    let refused = |d: &mut Document<Atlas>,
                   f: &dyn Fn(&mut Edit<'_>) -> rhizome_core::Result<()>| {
        let before = d.tree().snapshot();
        let e = d.edit("Bad", f).unwrap_err().to_string();
        assert_eq!(
            d.tree().snapshot(),
            before,
            "a policy breach leaves no trace"
        );
        e
    };
    let e = refused(&mut d, &|tx| tx.remove("/campaigns/realm/north/paper"));
    assert_eq!(
        e,
        "/campaigns/realm/north/paper: a paper can't be removed on its own"
    );
    let e = refused(&mut d, &|tx| {
        tx.add("/campaigns/realm/north", "grid", "grid2").map(drop)
    });
    assert_eq!(e, "/campaigns/realm/north: at most 1 grid here");
    let e = refused(&mut d, &|tx| {
        tx.set_order(
            "/campaigns/realm/north",
            "draw",
            [
                "/campaigns/realm/north/hills",
                "/campaigns/realm/north/paper",
                "/campaigns/realm/north/grid",
            ],
        )
    });
    assert_eq!(e, "/campaigns/realm/north/paper: must stay first in `draw`");
    let e = refused(&mut d, &|tx| {
        tx.copy("/campaigns/realm/north/grid", "/campaigns/realm/north")
            .map(drop)
    });
    assert!(e.contains("at most 1 grid"), "{e}");

    // removing the map takes its anchored layers with it
    d.edit("Remove Map", |tx| tx.remove(map)).unwrap();
    assert!(d.tree().at("/campaigns/realm/north").is_none());
    assert!(d.projection().is_empty());
}

/// The app's theme op: a node's palette is its own choice, else its nearest ancestor's.
/// App code over rhizome's primitives; POM has no themes (decision 39).
fn palette_of(d: &Document<Atlas>, node: NodeId) -> Option<(NodeId, String)> {
    let mut n = d.tree().get(node);
    while let Some(here) = n {
        if let Some(p) = here.resolve(PALETTE) {
            let Some(Value::Text(line)) = p.value(LINE) else {
                unreachable!()
            };
            return Some((here.id(), line));
        }
        n = here.parent();
    }
    None
}

#[test]
fn atlas_palette_theme_is_built_from_primitives() {
    let (mut d, campaign, map) = atlas();
    let hills = d.tree().at("/campaigns/realm/north/hills").unwrap().id();
    assert_eq!(palette_of(&d, hills), None, "no theme until one is chosen");

    let ((opera, waters), _) = d
        .edit("Add Palettes", |tx| {
            let o = tx.add("/themes", PALETTE, "space-opera")?;
            tx.set_value(o, LINE, Value::Text("#80a0ff".into()))?;
            let w = tx.add("/themes", PALETTE, "hostile-waters")?;
            tx.set_value(w, LINE, Value::Text("#40c0c0".into()))?;
            Ok((o, w))
        })
        .unwrap();
    let follow = |d: &mut Document<Atlas>, node, theme: Option<NodeId>| {
        d.edit("Choose Theme", |tx| match theme {
            Some(t) => tx.set_ref(node, PALETTE, rhizome_core::Ref::here(t)),
            None => tx.clear_ref(node, PALETTE),
        })
        .unwrap();
    };

    follow(&mut d, campaign, Some(opera));
    assert_eq!(palette_of(&d, hills), Some((campaign, "#80a0ff".into())));
    follow(&mut d, map, Some(waters));
    assert_eq!(
        palette_of(&d, hills),
        Some((map, "#40c0c0".into())),
        "the map overrides the campaign"
    );
    follow(&mut d, map, None);
    assert_eq!(
        palette_of(&d, hills).map(|p| p.0),
        Some(campaign),
        "unfollowing falls back to the campaign"
    );

    // a followed theme is read, never copied: edit it and every follower sees the change
    d.edit("Edit Palette", |tx| {
        tx.set_value(opera, LINE, Value::Text("#ffffff".into()))
    })
    .unwrap();
    assert_eq!(palette_of(&d, hills), Some((campaign, "#ffffff".into())));

    // themes and choices are document data: they save, reopen, and undo like anything else
    let store = MemoryStore::default();
    store.put("/r.atlas", &d.tree().serialise());
    let (again, _) = Document::<Atlas>::open(store, "/r.atlas").unwrap();
    assert_eq!(
        palette_of(&again, hills),
        Some((campaign, "#ffffff".into()))
    );
    d.undo().unwrap();
    assert_eq!(palette_of(&d, hills), Some((campaign, "#80a0ff".into())));

    // a terrain has no palette key, so it can't follow one
    assert!(
        d.edit("Choose Theme", |tx| tx.set_ref(
            hills,
            PALETTE,
            rhizome_core::Ref::here(opera)
        ))
        .is_err()
    );
}

#[test]
fn atlas_aspect_preset_is_computed_and_matched() {
    let (mut d, _, map) = atlas();
    assert_eq!(
        d.current_preset(map).unwrap(),
        Some(PresetRef::Catalogue("4x3".into()))
    );
    let (report, _) = d
        .apply_preset(map, &PresetRef::Catalogue("16x9".into()))
        .unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(
        d.tree().get(map).unwrap().get(SIZE),
        Some([800.0, 450.0]),
        "the long edge stays"
    );
    assert_eq!(
        d.current_preset(map).unwrap(),
        Some(PresetRef::Catalogue("16x9".into()))
    );
    d.edit("Nudge", |tx| tx.set(map, SIZE, [800.0, 452.0]))
        .unwrap();
    assert_eq!(
        d.current_preset(map).unwrap(),
        Some(PresetRef::Catalogue("16x9".into())),
        "close enough"
    );

    // a preset belongs to its kind: a terrain's are its own
    let hills = d.tree().at("/campaigns/realm/north/hills").unwrap().id();
    assert_eq!(
        d.preset_names(hills).unwrap(),
        [
            PresetRef::Catalogue("rocky".into()),
            PresetRef::Catalogue("plains".into())
        ]
    );
    let e = d
        .apply_preset(hills, &PresetRef::Catalogue("square".into()))
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("a terrain has no built-in preset “square”"),
        "{e}"
    );
    let campaign = d.tree().at("/campaigns/realm").unwrap().id();
    assert!(
        d.preset_names(campaign).unwrap().is_empty(),
        "a kind without presets has none"
    );
    assert!(d.save_preset(campaign, "x").is_err());

    // user presets of a kind travel with the file, and any node of that kind sees them
    d.save_preset(map, "Poster").unwrap();
    let names = d.preset_names(map).unwrap();
    assert_eq!(names.last(), Some(&PresetRef::User("Poster".into())));
}

#[test]
fn a_node_made_from_a_preset() {
    let (mut d, _, map) = atlas();
    let ((id, report), commit) = d
        .add_from_preset(
            "/campaigns/realm/north",
            "terrain",
            "dunes",
            &PresetRef::Catalogue("plains".into()),
        )
        .unwrap();
    assert_eq!(report.applied, 1);
    let c = commit.unwrap();
    assert_eq!(c.label, "New terrain");
    assert_eq!(d.tree().get(id).unwrap().get(SEED), Some(42));
    d.undo().unwrap();
    assert!(d.tree().get(id).is_none(), "one edit, one undo step");

    // a user preset fills the template too
    let hills = d.tree().at("/campaigns/realm/north/hills").unwrap().id();
    d.edit("Seed", |tx| tx.set(hills, SEED, 99)).unwrap();
    d.save_preset(hills, "Mine").unwrap();
    let ((dunes, _), _) = d
        .add_from_preset(
            "/campaigns/realm/north",
            "terrain",
            "dunes",
            &PresetRef::User("Mine".into()),
        )
        .unwrap();
    assert_eq!(d.tree().get(dunes).unwrap().get(SEED), Some(99));
    assert!(
        d.add_from_preset(
            "/campaigns/realm/north",
            "terrain",
            "x",
            &PresetRef::User("Nope".into())
        )
        .is_err()
    );
    assert!(
        d.add_from_preset(
            "/campaigns/realm",
            "campaign",
            "x",
            &PresetRef::Catalogue("a".into())
        )
        .is_err(),
        "no presets on that kind"
    );
    let _ = map;
}

#[test]
fn commands_every_app_gets() {
    let (mut d, _, _) = atlas();
    let at = |p: &str| json!({ "at": p });
    assert_eq!(d.label("edit.undo").unwrap(), "Undo Add Map");
    assert!(!d.is_enabled("edit.redo", &json!({})).unwrap());
    assert!(
        !d.is_enabled("edit.delete", &at("/campaigns/realm/north/paper"))
            .unwrap(),
        "policy disables Delete"
    );
    assert!(
        d.is_enabled("edit.delete", &at("/campaigns/realm/north/hills"))
            .unwrap()
    );
    assert!(
        !d.is_enabled("edit.duplicate", &at("/campaigns/realm/north/grid"))
            .unwrap(),
        "and Duplicate"
    );
    assert!(
        d.is_enabled("edit.duplicate", &at("/campaigns/realm/north/hills"))
            .unwrap()
    );
    assert!(
        !d.is_enabled("edit.delete", &at("/campaigns")).unwrap(),
        "categories stay"
    );
    assert!(matches!(
        d.run("edit.delete", &at("/campaigns/realm/north/paper")),
        Err(Error::Disabled(_))
    ));

    let Outcome::Text(clip) = d
        .run("edit.copy", &at("/campaigns/realm/north/hills"))
        .unwrap()
    else {
        panic!()
    };
    let Outcome::Committed(c) = d
        .run(
            "edit.paste",
            &json!({"parent": "/campaigns/realm/north", "fragment": clip}),
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(
        c.changes
            .to_string()
            .contains("/campaigns/realm/north/hills-2  added terrain")
    );
    assert!(matches!(
        d.run("edit.duplicate", &at("/campaigns/realm/north/hills"))
            .unwrap(),
        Outcome::Committed(_)
    ));
    assert!(matches!(
        d.run("edit.undo", &json!({})).unwrap(),
        Outcome::Committed(_)
    ));
    assert_eq!(d.label("edit.redo").unwrap(), "Redo Duplicate");

    let Outcome::Committed(_) = d
        .run(
            "preset.apply",
            &json!({"at": "/campaigns/realm/north", "preset": {"catalogue": "square"}}),
        )
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        d.tree().at("/campaigns/realm/north").unwrap().get(SIZE),
        Some([800.0, 800.0])
    );
    assert!(
        !d.is_enabled("preset.apply", &at("/campaigns/realm"))
            .unwrap(),
        "no presets on a campaign"
    );
    let Outcome::Committed(c) = d.run("node.add", &json!({"parent": "/campaigns/realm/north", "type": "terrain", "name": "dunes", "preset": {"catalogue": "rocky"}})).unwrap() else { panic!() };
    assert!(
        c.changes
            .to_string()
            .contains("/campaigns/realm/north/dunes  added terrain")
    );
    assert_eq!(
        d.tree()
            .at("/campaigns/realm/north/dunes")
            .unwrap()
            .get(SEED),
        Some(7)
    );
    assert!(matches!(
        d.run("nope", &json!({})),
        Err(Error::UnknownCommand(_))
    ));
    assert!(matches!(
        d.run("preset.save", &at("/campaigns/realm/north")),
        Err(Error::Payload(_))
    ));
    let ids: Vec<String> = d
        .commands(&json!({}))
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert!(ids.contains(&"file.save".to_string()) && ids.contains(&"preset.apply".to_string()));
}

// ---------------------------------------------------------------- bad models

struct TakesPresets;
impl ObjectModel for TakesPresets {
    const NAME: &'static str = "x";
    const EXTENSION: &'static str = "x";
    type Projection = ();
    fn kinds(k: &mut Kinds) {
        k.category("presets", Origin::Loaded);
    }
}

#[test]
fn a_model_that_cant_be_built_says_why() {
    let e = Document::<TakesPresets>::new(MemoryStore::default())
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("presets"), "POM holds the presets category: {e}");
}

#[test]
fn a_layer_pasted_into_another_map_keeps_the_anchors() {
    let (mut d, campaign, _) = atlas();
    d.edit("South", |tx| tx.add_map(campaign, "south")).unwrap();
    let Outcome::Text(clip) = d
        .run("edit.copy", &json!({"at": "/campaigns/realm/north/hills"}))
        .unwrap()
    else {
        panic!()
    };
    d.run(
        "edit.paste",
        &json!({"parent": "/campaigns/realm/south", "fragment": clip}),
    )
    .unwrap();
    let draw: Vec<String> = d
        .tree()
        .at("/campaigns/realm/south")
        .unwrap()
        .order("draw")
        .iter()
        .map(|n| n.name().to_string())
        .collect();
    assert_eq!(draw, ["paper", "hills", "grid"], "the grid stays on top");
}

#[test]
fn documents_go_to_another_thread() {
    fn send<T: Send>() {}
    send::<Document<Synth>>();
    send::<Document<Atlas>>();
}

#[test]
fn open_reports_policy_breaches() {
    let (d, _, _) = atlas();
    let text = d.tree().serialise();
    // a hand-edited file: the draw order lists the grid before the terrain
    let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    for n in doc["nodes"].as_array_mut().unwrap() {
        if n["path"] == "/campaigns/realm/north" {
            let draw = n["orders"]["draw"].as_array_mut().unwrap();
            draw.swap(1, 2);
        }
    }
    let store = MemoryStore::default();
    store.put("/bad.atlas", &serde_json::to_string_pretty(&doc).unwrap());
    let (_, report) = Document::<Atlas>::open(store, "/bad.atlas").unwrap();
    let issues: Vec<String> = report.issues.iter().map(|i| i.to_string()).collect();
    assert_eq!(
        issues,
        ["/campaigns/realm/north/grid: policy: must stay last in `draw`"]
    );
}
