//! POM against two made-up object models of different shapes. Neither is an app's real one.
//!
//! - **Synth**: values and bindings on nodes, user presets of one node's sound that skip its
//!   on/off switch, a compiled projection.
//! - **Atlas**: a document of maps with anchored, singleton layers (policy), a built-in
//!   palette followed by cascade, a computed aspect preset, domain verbs.

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
        );
        k.kind(
            NodeType::new("lfo")
                .in_categories(&["mods"])
                .float("lfo.rate", 0.01..=20.0, 1.0)
                .bindable([ValueSpec::float("depth", -1.0..=1.0, 0.5)]),
        );
    }

    fn presets(p: &mut Presets) {
        p.kind(
            "sound",
            NodeValues::new()
                .skip(|k| k.ends_with(".on"))
                .with_bindings(),
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
    d.save_preset("sound", osc, "Warm").unwrap();
    assert_eq!(
        d.current_preset("sound", osc).unwrap(),
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
    assert_eq!(d.current_preset("sound", osc).unwrap(), None);
    let steps = d.tree().history_len();
    let (report, commit) = d
        .apply_preset("sound", osc, &PresetRef::User("Warm".into()))
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
        d.current_preset("sound", osc).unwrap(),
        Some(PresetRef::User("Warm".into()))
    );
    d.undo().unwrap();
    assert_eq!(d.tree().get(osc).unwrap().get(PITCH), Some(-12.0));

    // Shard's rules: save refuses a taken name, update and the rest refuse a missing one
    let err = d.save_preset("sound", osc, "Warm").unwrap_err().to_string();
    assert!(err.contains("already exists"), "{err}");
    assert!(d.update_preset("sound", osc, "Cold").is_err());
    assert!(
        d.save_preset("sound", osc, "   ").is_err(),
        "a name needs characters"
    );
    d.update_preset("sound", osc, "Warm").unwrap();
    d.save_preset("sound", osc, "Cold").unwrap();
    d.rename_preset("sound", osc, "Cold", "Icy").unwrap();
    assert!(
        d.rename_preset("sound", osc, "Icy", "Warm").is_err(),
        "rename refuses a taken name"
    );
    assert_eq!(
        d.preset_names("sound", osc).unwrap(),
        [
            PresetRef::User("Icy".into()),
            PresetRef::User("Warm".into())
        ]
    );
    d.delete_preset("sound", osc, "Icy").unwrap();
    assert_eq!(
        d.preset_names("sound", osc).unwrap(),
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
        again.preset_names("sound", osc).unwrap(),
        [PresetRef::User("Warm".into())]
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Palette {
    fill: String,
    line: String,
}

/// Palettes are followed, never written onto a node.
struct PaletteKind;

impl Aggregate for PaletteKind {
    type State = Palette;
    fn get(&self, _node: rhizome_core::Node<'_>) -> Palette {
        Palette {
            fill: String::new(),
            line: String::new(),
        }
    }
    fn set(&self, _tx: &mut Edit<'_>, _node: NodeId, _s: &Palette) -> rhizome_core::Result<Report> {
        Err(rhizome_core::Error::Structural(
            "palettes are chosen, not applied".into(),
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Aspect {
    ratio: f64,
}

/// The state is derived: a ratio, not the stored size. The setter computes the size,
/// keeping the long edge.
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
    fn applies_to(&self, node: rhizome_core::Node<'_>) -> bool {
        node.type_name() == "map"
    }
}

impl ObjectModel for Atlas {
    const NAME: &'static str = "Atlas";
    const EXTENSION: &'static str = "atlas";
    type Projection = Vec<String>;

    fn kinds(k: &mut Kinds) {
        k.category("campaigns", Origin::Loaded);
        k.kind(NodeType::new("campaign").in_categories(&["campaigns"]));
        k.kind(
            NodeType::new("map")
                .in_categories(&["campaigns"])
                .vec2(SIZE, [800.0, 600.0]),
        );
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
        k.kind(NodeType::new("terrain").in_categories(&["campaigns"]).int(
            "terrain.seed",
            0..=999,
            0,
        ));
    }

    fn presets(p: &mut Presets) {
        let pal = |fill: &str, line: &str| Palette {
            fill: fill.into(),
            line: line.into(),
        };
        p.kind("palette", PaletteKind)
            .catalogue([
                ("doom-forge", pal("#3a1c12", "#f0a040")),
                ("space-opera", pal("#0b1030", "#80a0ff")),
                ("hostile-waters", pal("#0a2a30", "#40c0c0")),
            ])
            .fallback("doom-forge")
            .followed_by(&["campaign", "map"]);
        p.kind("aspect", AspectKind).catalogue([
            ("4x3", Aspect { ratio: 4.0 / 3.0 }),
            ("16x9", Aspect { ratio: 16.0 / 9.0 }),
            ("square", Aspect { ratio: 1.0 }),
        ]);
        p.kind("map-style", NodeValues::new()).followed_by(&["map"]);
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

#[test]
fn atlas_palette_follows_by_cascade() {
    let (mut d, campaign, map) = atlas();
    let hills = d.tree().at("/campaigns/realm/north/hills").unwrap().id();
    let palette = |d: &Document<Atlas>, n| d.resolve_preset("palette", n).unwrap().unwrap();

    let r = palette(&d, hills);
    assert_eq!(
        (r.follower, &r.preset),
        (None, &PresetRef::Catalogue("doom-forge".into())),
        "the fallback"
    );
    assert_eq!(r.state::<Palette>().unwrap().line, "#f0a040");

    d.follow_preset(
        "palette",
        campaign,
        Some(&PresetRef::Catalogue("space-opera".into())),
    )
    .unwrap();
    let r = palette(&d, hills);
    assert_eq!(
        (r.follower, r.preset),
        (Some(campaign), PresetRef::Catalogue("space-opera".into())),
        "from the campaign"
    );

    d.follow_preset(
        "palette",
        map,
        Some(&PresetRef::Catalogue("hostile-waters".into())),
    )
    .unwrap();
    assert_eq!(
        palette(&d, hills).follower,
        Some(map),
        "the map overrides the campaign"
    );

    d.follow_preset("palette", map, None).unwrap();
    assert_eq!(
        palette(&d, hills).follower,
        Some(campaign),
        "unfollowing falls back to the campaign"
    );

    // the choice is document data: it saves, and a catalogue ref is readable on disk
    let text = d.tree().serialise();
    assert!(
        text.contains("\"file\": \"catalogue:palette/space-opera\""),
        "{text}"
    );
    let store = MemoryStore::default();
    store.put("/r.atlas", &text);
    let (again, _) = Document::<Atlas>::open(store, "/r.atlas").unwrap();
    assert_eq!(
        again
            .resolve_preset("palette", hills)
            .unwrap()
            .unwrap()
            .follower,
        Some(campaign)
    );

    assert!(
        d.follow_preset(
            "palette",
            campaign,
            Some(&PresetRef::Catalogue("nope".into()))
        )
        .is_err()
    );
    assert!(
        d.follow_preset(
            "palette",
            hills,
            Some(&PresetRef::Catalogue("space-opera".into()))
        )
        .is_err(),
        "a terrain can't follow"
    );
    assert!(
        d.apply_preset(
            "palette",
            campaign,
            &PresetRef::Catalogue("space-opera".into())
        )
        .is_err(),
        "the aggregate says so"
    );
}

#[test]
fn atlas_follows_a_user_preset_by_id() {
    let (mut d, campaign, north) = atlas();
    let (south, _) = d.edit("South", |tx| tx.add_map(campaign, "south")).unwrap();
    d.edit("Big", |tx| tx.set(north, SIZE, [2000.0, 1000.0]))
        .unwrap();
    d.save_preset("map-style", north, "Big").unwrap();
    d.follow_preset("map-style", south, Some(&PresetRef::User("Big".into())))
        .unwrap();
    let r = d.resolve_preset("map-style", south).unwrap().unwrap();
    assert_eq!(
        r.state::<NodeValuesState>().unwrap().values["map.size"],
        json!([2000.0, 1000.0])
    );
    d.rename_preset("map-style", north, "Big", "Huge").unwrap();
    assert_eq!(
        d.resolve_preset("map-style", south)
            .unwrap()
            .unwrap()
            .preset,
        PresetRef::User("Huge".into()),
        "a follower holds the preset by id, so a rename keeps it"
    );
    d.delete_preset("map-style", north, "Huge").unwrap();
    assert_eq!(
        d.resolve_preset("map-style", south).unwrap(),
        None,
        "no fallback, nothing to resolve"
    );
}

#[test]
fn atlas_aspect_is_computed_and_matched() {
    let (mut d, _, map) = atlas();
    assert_eq!(
        d.current_preset("aspect", map).unwrap(),
        Some(PresetRef::Catalogue("4x3".into()))
    );
    let (report, _) = d
        .apply_preset("aspect", map, &PresetRef::Catalogue("16x9".into()))
        .unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(
        d.tree().get(map).unwrap().get(SIZE),
        Some([800.0, 450.0]),
        "the long edge stays"
    );
    assert_eq!(
        d.current_preset("aspect", map).unwrap(),
        Some(PresetRef::Catalogue("16x9".into()))
    );
    d.edit("Nudge", |tx| tx.set(map, SIZE, [800.0, 452.0]))
        .unwrap();
    assert_eq!(
        d.current_preset("aspect", map).unwrap(),
        Some(PresetRef::Catalogue("16x9".into())),
        "close enough"
    );
    let hills = d.tree().at("/campaigns/realm/north/hills").unwrap().id();
    assert!(
        d.preset_names("aspect", hills).unwrap().is_empty(),
        "aspect applies to maps only"
    );
    assert!(
        d.apply_preset("aspect", hills, &PresetRef::Catalogue("square".into()))
            .is_err()
    );
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

    let Outcome::Committed(_) = d.run("preset.apply", &json!({"kind": "aspect", "at": "/campaigns/realm/north", "preset": {"catalogue": "square"}})).unwrap() else { panic!() };
    assert_eq!(
        d.tree().at("/campaigns/realm/north").unwrap().get(SIZE),
        Some([800.0, 800.0])
    );
    let Outcome::Committed(_) = d.run("preset.follow", &json!({"kind": "palette", "at": "/campaigns/realm", "preset": {"catalogue": "space-opera"}})).unwrap() else { panic!() };
    assert!(matches!(
        d.run("nope", &json!({})),
        Err(Error::UnknownCommand(_))
    ));
    assert!(matches!(
        d.run(
            "preset.save",
            &json!({"kind": "map-style", "at": "/campaigns/realm/north"})
        ),
        Err(Error::Payload(_))
    ));
    let ids: Vec<String> = d
        .commands(&json!({}))
        .into_iter()
        .map(|(id, _, _)| id)
        .collect();
    assert!(ids.contains(&"file.save".to_string()) && ids.contains(&"preset.follow".to_string()));
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

struct BadFallback;
impl ObjectModel for BadFallback {
    const NAME: &'static str = "x";
    const EXTENSION: &'static str = "x";
    type Projection = ();
    fn kinds(_: &mut Kinds) {}
    fn presets(p: &mut Presets) {
        p.kind("aspect", AspectKind).fallback("nope");
    }
}

struct BadFollower;
impl ObjectModel for BadFollower {
    const NAME: &'static str = "x";
    const EXTENSION: &'static str = "x";
    type Projection = ();
    fn kinds(_: &mut Kinds) {}
    fn presets(p: &mut Presets) {
        p.kind("palette", PaletteKind).followed_by(&["ghost"]);
    }
}

#[test]
fn a_model_that_cant_be_built_says_why() {
    let e = Document::<TakesPresets>::new(MemoryStore::default())
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("presets"), "POM holds the presets category: {e}");
    let e = Document::<BadFallback>::new(MemoryStore::default())
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("fallback"), "{e}");
    let e = Document::<BadFollower>::new(MemoryStore::default())
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("ghost"), "{e}");
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

#[test]
fn deleting_a_followed_preset_unfollows() {
    let (mut d, campaign, north) = atlas();
    let (south, _) = d.edit("South", |tx| tx.add_map(campaign, "south")).unwrap();
    d.save_preset("map-style", north, "Big").unwrap();
    d.follow_preset("map-style", south, Some(&PresetRef::User("Big".into())))
        .unwrap();
    let c = d.delete_preset("map-style", north, "Big").unwrap().unwrap();
    let lines = c.changes.to_string();
    assert!(
        lines.contains("/campaigns/realm/south  follow.map-style"),
        "{lines}"
    );
    let (_, report) = Tree::load(&d.tree().serialise(), d.tree().registry().clone()).unwrap();
    assert!(
        report.issues.is_empty(),
        "nothing left pointing at the deleted preset: {:?}",
        report.issues
    );
}
