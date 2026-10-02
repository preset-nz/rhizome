//! Acid test: the SDK can construct and operate a rhizome with one of everything.
//!
//! Every category origin, value kind, reference kind, binding kind, group, order, verb,
//! refusal and cascade appears at least once, and the result survives undo, redo, save,
//! load, copy and paste. The built file is pinned in `tests/golden/acid.rhizome`.

mod golden;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rhizome_core::*;

// ---- the object model: one of everything ----

const OPACITY: Key<f64> = Key::new("opacity");
const VISIBLE: Key<bool> = Key::new("visible");
const LABEL: Key<String> = Key::new("label");
const BLEND: Key<Choice> = Key::new("blend");
const OFFSET: Key<[f64; 2]> = Key::new("offset");
const TINT: Key<Colour> = Key::new("tint");
const LAYER: Key<i64> = Key::new("layer");
const RADIUS: Key<f64> = Key::new("radius");
const AXIS: Key<[f64; 3]> = Key::new("axis");
const RATE: Key<f64> = Key::new("rate");

/// Contours of points with optional handles: a vector path's shape (decision 51).
fn outline() -> Shape {
    Shape::list(Shape::record([
        (
            "anchors",
            Shape::list(Shape::record([
                ("point", Shape::Vec2),
                ("handle_in", Shape::optional(Shape::Vec2)),
            ])),
        ),
        ("closed", Shape::Bool),
    ]))
}

fn registry() -> Arc<Registry> {
    Registry::builder()
        .category("images", Origin::Loaded)
        .category("masks", Origin::Loaded)
        .category("modulators", Origin::Loaded)
        .category("renders", Origin::Calculated)
        .node(
            NodeType::new("image")
                .in_categories(&["images"])
                .float(OPACITY, 0.0..=1.0, 1.0)
                .bool(VISIBLE, true)
                .text(LABEL, "")
                .choice(BLEND, &["normal", "multiply", "screen"], "normal")
                .vec2(OFFSET, [0.0, 0.0])
                .colour(TINT, [1.0, 1.0, 1.0, 1.0])
                .int(LAYER, 0..=99, 0)
                .reference("source")
                .slot("mask")
                .check(|n| match n.get(LABEL) {
                    Some(l) if l.len() > 32 => Err("label is longer than 32 characters".into()),
                    _ => Ok(()),
                }),
        )
        .node(
            NodeType::new("blur")
                .in_categories(&["images"])
                .float(RADIUS, 0.0..=200.0, 4.0)
                .vec3(AXIS, [0.0, 0.0, 1.0])
                .float_unbounded("angle", 0.0)
                .floats("matrix", &[1.0, 0.0, 0.0, 1.0])
                .shaped("outline", outline()),
        )
        .node(
            NodeType::new("mask")
                .in_categories(&["masks"])
                .float("feather", 0.0..=50.0, 0.0)
                .bindable([]),
        )
        .node(
            NodeType::new("lfo")
                .in_categories(&["modulators"])
                .float(RATE, 0.01..=20.0, 1.0)
                .choice("shape", &["sine", "square"], "sine")
                .bindable([ValueSpec::float("depth", -1.0..=1.0, 0.5)]),
        )
        .node(
            NodeType::new("render")
                .in_categories(&["renders"])
                .reference("input"),
        )
        .build()
        .expect("registry")
}

fn new_tree() -> Tree {
    Tree::with_ids(registry(), IdSource::sequential())
}

/// Builds the one-of-everything tree in one edit, through Rust verbs.
fn build(tx: &mut Edit<'_>) -> Result<()> {
    let sky = tx.add("/images", "image", "sky")?;
    let blur = tx.add(sky, "blur", "blur")?;
    let sea = tx.add("/images", "image", "sea")?;
    let hero = tx.add("/images", "group", "hero")?;
    let vignette = tx.add("/masks", "mask", "vignette")?;
    let wobble = tx.add("/modulators", "lfo", "wobble")?;
    let fin = tx.add("/renders", "render", "final")?;

    tx.set(sky, OPACITY, 0.8)?;
    tx.set(sky, VISIBLE, false)?;
    tx.set(sky, LABEL, "Sky at dusk".to_string())?;
    tx.set(sky, BLEND, Choice("screen".into()))?;
    tx.set(sky, OFFSET, [12.5, -4.0])?;
    tx.set(sky, TINT, Colour([1.0, 0.8, 0.6, 1.0]))?;
    tx.set(sky, LAYER, 3)?;
    tx.set(blur, RADIUS, 18.0)?;
    tx.set(blur, AXIS, [0.0, 1.0, 0.0])?;
    tx.set_value(blur, "angle", Value::Float(-725.5))?;
    tx.set_value(blur, "matrix", Value::Floats(vec![0.0, -1.0, 1.0, 0.0]))?;
    tx.set_value(
        blur,
        "outline",
        Value::Shaped(serde_json::json!([{"anchors": [{"point": [0.0, 0.0]}, {"point": [4.0, 2.0], "handle_in": [3.0, 0.0]}], "closed": true}])),
    )?;
    tx.set(wobble, RATE, 0.25)?;
    tx.set_value(wobble, "shape", Value::Choice("square".into()))?;

    tx.set_ref(sky, "source", Ref::file("sky.png"))?;
    tx.set_ref(fin, "input", Ref::here(sky))?;
    let far = Path::parse("/images/far")?;
    tx.set_ref(
        sea,
        "source",
        Ref::node_in("other.rhizome", NodeId::from_u128(0x77), far),
    )?;
    tx.bind(sky, On::slot("mask"), vignette, Vec::<(&str, Value)>::new())?;
    tx.bind(
        sky,
        On::value(OPACITY),
        wobble,
        [("depth", Value::Float(0.25))],
    )?;
    tx.join(hero, [sky, sea])?;
    tx.set_order(sky, "modifiers", [blur])?;
    tx.set_order("/images", "draw", [sea, sky])?;
    Ok(())
}

/// The same tree, through `Op`s.
fn build_ops() -> Vec<Op> {
    let ops = serde_json::json!([
        {"op": "add", "parent": "/images", "type": "image", "name": "sky"},
        {"op": "add", "parent": "/images/sky", "type": "blur", "name": "blur"},
        {"op": "add", "parent": "/images", "type": "image", "name": "sea"},
        {"op": "add", "parent": "/images", "type": "group", "name": "hero"},
        {"op": "add", "parent": "/masks", "type": "mask", "name": "vignette"},
        {"op": "add", "parent": "/modulators", "type": "lfo", "name": "wobble"},
        {"op": "add", "parent": "/renders", "type": "render", "name": "final"},
        {"op": "set", "at": "/images/sky", "key": "opacity", "value": 0.8},
        {"op": "set", "at": "/images/sky", "key": "visible", "value": false},
        {"op": "set", "at": "/images/sky", "key": "label", "value": "Sky at dusk"},
        {"op": "set", "at": "/images/sky", "key": "blend", "value": "screen"},
        {"op": "set", "at": "/images/sky", "key": "offset", "value": [12.5, -4.0]},
        {"op": "set", "at": "/images/sky", "key": "tint", "value": [1.0, 0.8, 0.6, 1.0]},
        {"op": "set", "at": "/images/sky", "key": "layer", "value": 3},
        {"op": "set", "at": "/images/sky/blur", "key": "radius", "value": 18.0},
        {"op": "set", "at": "/images/sky/blur", "key": "axis", "value": [0.0, 1.0, 0.0]},
        {"op": "set", "at": "/images/sky/blur", "key": "angle", "value": -725.5},
        {"op": "set", "at": "/images/sky/blur", "key": "matrix", "value": [0.0, -1.0, 1.0, 0.0]},
        {"op": "set", "at": "/images/sky/blur", "key": "outline", "value": [{"anchors": [{"point": [0.0, 0.0]}, {"point": [4.0, 2.0], "handle_in": [3.0, 0.0]}], "closed": true}]},
        {"op": "set", "at": "/modulators/wobble", "key": "rate", "value": 0.25},
        {"op": "set", "at": "/modulators/wobble", "key": "shape", "value": "square"},
        {"op": "set_ref", "at": "/images/sky", "key": "source", "ref": {"file": "sky.png"}},
        {"op": "set_ref", "at": "/renders/final", "key": "input", "ref": {"node": "/images/sky"}},
        {"op": "set_ref", "at": "/images/sea", "key": "source", "ref": {"file": "other.rhizome", "node": "0000000000000000000000003Q", "path": "/images/far"}},
        {"op": "bind", "target": "/images/sky", "on": {"slot": "mask"}, "source": "/masks/vignette"},
        {"op": "bind", "target": "/images/sky", "on": {"value": "opacity"}, "source": "/modulators/wobble", "values": {"depth": 0.25}},
        {"op": "join", "group": "/images/hero", "members": ["/images/sky", "/images/sea"]},
        {"op": "set_order", "owner": "/images/sky", "name": "modifiers", "ids": ["/images/sky/blur"]},
        {"op": "set_order", "owner": "/images", "name": "draw", "ids": ["/images/sea", "/images/sky"]},
    ]);
    serde_json::from_value(ops).expect("ops")
}

fn built() -> Tree {
    let mut t = new_tree();
    t.edit("Build", build).expect("build");
    t
}

/// Asserts an edit is refused with the expected error and leaves no trace.
fn refused(
    t: &mut Tree,
    f: impl FnOnce(&mut Edit<'_>) -> Result<()>,
    check: impl Fn(&Error) -> bool,
) {
    let before = t.snapshot();
    let (seq, steps) = (t.seq(), t.history_len());
    let err = t.edit("Refused", f).expect_err("should be refused");
    assert!(check(&err), "unexpected error: {err}");
    assert_eq!(
        t.snapshot(),
        before,
        "a refused edit changed the tree ({err})"
    );
    assert_eq!(
        (t.seq(), t.history_len()),
        (seq, steps),
        "a refused edit committed ({err})"
    );
}

#[test]
fn acid() {
    let mut log = String::new();
    let mut t = new_tree();
    assert!(!t.is_unsaved());
    let empty = t.snapshot();

    // ---- construct ----
    let (_, c) = t.edit("Build", build).unwrap();
    let c = c.expect("changes");
    assert_eq!((c.seq, c.label.as_str()), (1, "Build"));
    log += &format!("== {} ==\n{}", c.label, c.changes);
    assert!(t.is_unsaved());

    // ---- read: every read verb ----
    let sky = t.at("/images/sky").unwrap();
    assert_eq!(sky.type_name(), "image");
    assert_eq!(sky.path().to_string(), "/images/sky");
    assert_eq!(sky.category().unwrap().name(), "images");
    assert_eq!(sky.origin(), Some(Origin::Loaded));
    assert_eq!(
        t.at("/renders/final").unwrap().origin(),
        Some(Origin::Calculated)
    );
    assert_eq!(sky.parent().unwrap().name(), "images");
    assert_eq!(
        sky.children().map(|n| n.name()).collect::<Vec<_>>(),
        ["blur"]
    );
    assert_eq!(sky.child("blur").unwrap().get(AXIS), Some([0.0, 1.0, 0.0]));
    assert_eq!(sky.get(OPACITY), Some(0.8));
    assert_eq!(sky.get(VISIBLE), Some(false));
    assert_eq!(sky.get(LABEL).as_deref(), Some("Sky at dusk"));
    assert_eq!(sky.get(BLEND), Some(Choice("screen".into())));
    assert_eq!(sky.get(OFFSET), Some([12.5, -4.0]));
    assert_eq!(sky.get(TINT), Some(Colour([1.0, 0.8, 0.6, 1.0])));
    assert_eq!(sky.get(LAYER), Some(3));
    let sea = t.at("/images/sea").unwrap();
    assert_eq!(sea.get(OPACITY), Some(1.0), "unset reads as the default");
    assert!(!sea.is_set("opacity") && sky.is_set("opacity"));
    assert_eq!(sky.reference("source"), Some(&Ref::file("sky.png")));
    let fin = t.at("/renders/final").unwrap();
    assert_eq!(fin.resolve("input").unwrap().id(), sky.id());
    assert_eq!(
        sky.referrers()
            .iter()
            .map(|(n, k)| (n.name(), *k))
            .collect::<Vec<_>>(),
        [("final", "input")]
    );
    assert_eq!(
        sky.order("modifiers")
            .iter()
            .map(|n| n.name())
            .collect::<Vec<_>>(),
        ["blur"]
    );
    let images = t.at("/images").unwrap();
    assert!(images.is_category());
    assert_eq!(
        images
            .order("draw")
            .iter()
            .map(|n| n.name())
            .collect::<Vec<_>>(),
        ["sea", "sky"]
    );
    assert_eq!(images.order_names().collect::<Vec<_>>(), ["draw"]);
    let hero = t.at("/images/hero").unwrap();
    assert!(hero.is_group());
    assert_eq!(
        hero.members().iter().map(|n| n.name()).collect::<Vec<_>>(),
        ["sea", "sky"]
    );
    assert_eq!(
        sky.groups().iter().map(|n| n.name()).collect::<Vec<_>>(),
        ["hero"]
    );
    let bindings = sky.bindings();
    assert_eq!(bindings.len(), 2);
    let wobble = t.at("/modulators/wobble").unwrap();
    let link = &wobble.bound_to()[0];
    assert_eq!((link.target.name(), link.on), ("sky", &On::value(OPACITY)));
    assert_eq!(link.value("depth"), Some(Value::Float(0.25)));
    let mask = &t.at("/masks/vignette").unwrap().bound_to()[0];
    assert_eq!(mask.on, &On::slot("mask"));
    assert_eq!(
        t.at(sky.id()).unwrap().name(),
        "sky",
        "ids address as well as paths"
    );
    assert_eq!(
        t.root().children().map(|n| n.name()).collect::<Vec<_>>(),
        ["images", "masks", "modulators", "renders"]
    );

    // ---- the same tree through Ops ----
    let mut via_ops = new_tree();
    via_ops.edit_ops("Build", &build_ops()).unwrap();
    assert_eq!(
        via_ops.serialise(),
        t.serialise(),
        "Ops and Rust verbs build the same file"
    );
    for op in build_ops() {
        let json = serde_json::to_string(&op).unwrap();
        assert_eq!(
            serde_json::from_str::<Op>(&json).unwrap(),
            op,
            "Op round-trips: {json}"
        );
    }

    // ---- save, load, byte-stable ----
    let text = t.serialise();
    golden::check("acid.rhizome", &text);
    assert_eq!(t.serialise(), text, "serialise is byte-stable");
    let (loaded, report) = Tree::load(&text, registry()).unwrap();
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert_eq!(
        loaded.serialise(),
        text,
        "load(serialise(t)) writes the same bytes"
    );
    assert!(
        loaded.diff(&t.snapshot()).is_empty(),
        "load(serialise(t)) diffs empty"
    );
    t.mark_saved();
    assert!(!t.is_unsaved());

    // ---- refusals: each leaves no trace ----
    use Error as E;
    refused(
        &mut t,
        |tx| tx.set(sky_id(tx), OPACITY, 1.5),
        |e| matches!(e, E::OutOfRange { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set(sky_id(tx), OPACITY, f64::NAN),
        |e| matches!(e, E::NotFinite { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set(sky_id(tx), LAYER, 100),
        |e| matches!(e, E::OutOfRange { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set(sky_id(tx), TINT, Colour([2.0, 0.0, 0.0, 1.0])),
        |e| matches!(e, E::OutOfRange { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set(sky_id(tx), BLEND, Choice("overlay".into())),
        |e| matches!(e, E::NotAChoice { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set_value("/images/sky", "opacity", Value::Int(1)),
        |e| matches!(e, E::WrongKind { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set_value("/images/sky", "nope", Value::Int(1)),
        |e| matches!(e, E::UnknownKey { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set_value("/images", "opacity", Value::Float(1.0)),
        |e| matches!(e, E::Structural(_)),
    );
    refused(
        &mut t,
        |tx| tx.set(sky_id(tx), LABEL, "x".repeat(40)),
        |e| matches!(e, E::Check { .. }),
    );
    refused(
        &mut t,
        |tx| tx.add("/images", "image", "sky").map(drop),
        |e| matches!(e, E::NameTaken(_)),
    );
    refused(
        &mut t,
        |tx| tx.add("/images", "image", "bad name").map(drop),
        |e| matches!(e, E::InvalidName(_)),
    );
    refused(
        &mut t,
        |tx| tx.add("/images", "sparkle", "s").map(drop),
        |e| matches!(e, E::UnknownType(_)),
    );
    refused(
        &mut t,
        |tx| tx.add("/masks", "image", "m").map(drop),
        |e| matches!(e, E::NotAllowed { .. }),
    );
    refused(
        &mut t,
        |tx| tx.add("/", "image", "loose").map(drop),
        |e| matches!(e, E::Structural(_)),
    );
    refused(
        &mut t,
        |tx| tx.add("/images/hero", "image", "kid").map(drop),
        |e| matches!(e, E::Structural(_)),
    );
    refused(
        &mut t,
        |tx| tx.add("/images", "category", "c").map(drop),
        |e| matches!(e, E::Structural(_)),
    );
    refused(
        &mut t,
        |tx| tx.add("/images/nowhere", "image", "x").map(drop),
        |e| matches!(e, E::NotFound(_)),
    );
    refused(
        &mut t,
        |tx| tx.remove("/images"),
        |e| matches!(e, E::Structural(_)),
    );
    refused(
        &mut t,
        |tx| tx.remove("/"),
        |e| matches!(e, E::Structural(_)),
    );
    refused(
        &mut t,
        |tx| tx.rename("/images/sky", "sea"),
        |e| matches!(e, E::NameTaken(_)),
    );
    refused(
        &mut t,
        |tx| tx.move_to("/images/sky", "/images/sky/blur"),
        |e| matches!(e, E::Cycle(_)),
    );
    refused(
        &mut t,
        |tx| tx.move_to("/masks/vignette", "/images/sky"),
        |e| matches!(e, E::NotAllowed { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set_ref("/images/sky", "input", Ref::file("x")),
        |e| matches!(e, E::UnknownKey { .. }),
    );
    refused(
        &mut t,
        |tx| {
            tx.bind(
                "/images/sky",
                On::slot("halo"),
                "/masks/vignette",
                Vec::<(&str, Value)>::new(),
            )
        },
        |e| matches!(e, E::UnknownSlot { .. }),
    );
    refused(
        &mut t,
        |tx| {
            tx.bind(
                "/images/sky",
                On::slot("mask"),
                "/images/sea",
                Vec::<(&str, Value)>::new(),
            )
        },
        |e| matches!(e, E::NotBindable(_)),
    );
    refused(
        &mut t,
        |tx| {
            tx.bind(
                "/images/sky",
                On::value("opacity"),
                "/modulators/wobble",
                [("depth", Value::Float(3.0))],
            )
        },
        |e| matches!(e, E::OutOfRange { .. }),
    );
    refused(
        &mut t,
        |tx| {
            tx.bind(
                "/images/sky",
                On::slot("mask"),
                "/images/sky",
                Vec::<(&str, Value)>::new(),
            )
        },
        |e| matches!(e, E::Structural(_)),
    );
    refused(
        &mut t,
        |tx| tx.join("/images/sky", ["/images/sea"]),
        |e| matches!(e, E::NotAGroup(_)),
    );
    refused(
        &mut t,
        |tx| tx.set_order("/images", "draw", ["/images/sky/blur"]),
        |e| matches!(e, E::NotAChild { .. }),
    );
    refused(
        &mut t,
        |tx| tx.set_order("/images", "draw", ["/images/sky", "/images/sky"]),
        |e| matches!(e, E::Duplicate(_)),
    );
    refused(
        &mut t,
        |tx| tx.append_to_order("/images", "draw", "/images/sky"),
        |e| matches!(e, E::Duplicate(_)),
    );
    refused(
        &mut t,
        |tx| {
            tx.set(sky_id(tx), OPACITY, 0.1)?;
            tx.remove("/images")
        },
        |e| matches!(e, E::Structural(_)),
    );
    assert!(!t.is_unsaved(), "nothing refused touched the tree");

    // ---- operate: every write verb, with its cascades ----
    let mut step = |t: &mut Tree, label: &str, f: &dyn Fn(&mut Edit<'_>) -> Result<()>| {
        let (_, c) = t.edit(label, f).unwrap();
        let c = c.unwrap_or_else(|| panic!("{label} changed nothing"));
        log += &format!("== {} ==\n{}", c.label, c.changes);
        c.changes
    };

    let c = step(&mut t, "Rename", &|tx| tx.rename("/images/sky", "dusk"));
    assert_eq!(
        c.len(),
        1,
        "a rename is one Moved entry, children not listed"
    );
    assert!(
        matches!(&c.entries[0].kind, ChangeKind::Moved { from } if from.as_str() == "/images/sky")
    );
    assert_eq!(
        t.at("/renders/final")
            .unwrap()
            .resolve("input")
            .unwrap()
            .name(),
        "dusk",
        "refs follow by id"
    );

    let c = step(&mut t, "Reorder", &|tx| {
        tx.set_order("/images", "draw", ["/images/dusk", "/images/sea"])
    });
    assert_eq!(c.len(), 1, "a reorder is one entry");

    step(&mut t, "Move Blur", &|tx| {
        tx.move_to("/images/dusk/blur", "/images/sea")
    });
    assert!(
        t.at("/images/dusk").unwrap().order("modifiers").is_empty(),
        "moving out drops it from the old parent's orders"
    );

    step(&mut t, "Append", &|tx| {
        tx.append_to_order("/images/sea", "modifiers", "/images/sea/blur")
    });
    step(&mut t, "Reset", &|tx| tx.reset("/images/dusk", OPACITY));
    assert_eq!(t.at("/images/dusk").unwrap().get(OPACITY), Some(1.0));
    step(&mut t, "Unique", &|tx| {
        tx.add_unique("/images", "image", "dusk").map(drop)
    });
    assert!(t.at("/images/dusk-2").is_some());
    step(&mut t, "Leave", &|tx| {
        tx.leave("/images/hero", ["/images/sea"])
    });
    step(&mut t, "Unbind", &|tx| {
        tx.unbind("/images/dusk", On::value(OPACITY), "/modulators/wobble")
    });
    step(&mut t, "Clear Ref", &|tx| {
        tx.clear_ref("/images/dusk", "source")
    });

    // copy: inside pointers remap, outside pointers stay, groups join, order slots in after
    step(&mut t, "Duplicate", &|tx| {
        tx.copy("/images/sea", "/images").map(drop)
    });
    let copy = t.at("/images/sea-2").unwrap();
    let original = t.at("/images/sea").unwrap();
    assert_ne!(copy.id(), original.id());
    let copy_blur = copy.child("blur").unwrap();
    assert_ne!(
        copy_blur.id(),
        original.child("blur").unwrap().id(),
        "a copy shares no ids"
    );
    assert_eq!(
        copy.order("modifiers")[0].id(),
        copy_blur.id(),
        "inside pointers go to the copies"
    );
    let draw: Vec<_> = t
        .at("/images")
        .unwrap()
        .order("draw")
        .iter()
        .map(|n| n.name())
        .collect();
    assert_eq!(
        draw,
        ["dusk", "sea", "sea-2"],
        "the copy goes straight after the original"
    );

    step(&mut t, "Join", &|tx| {
        tx.join("/images/hero", ["/images/sea-2"])
    });
    step(&mut t, "Point Final", &|tx| {
        tx.set_ref(
            "/renders/final",
            "input",
            Ref::here(tx.at("/images/sea-2").unwrap().id()),
        )
    });

    // remove: groups, orders and bindings let go; a here-ref stays, unresolved
    step(&mut t, "Bind Sea", &|tx| {
        tx.bind(
            "/images/sea",
            On::slot("mask"),
            "/masks/vignette",
            Vec::<(&str, Value)>::new(),
        )
    });
    let c = step(&mut t, "Remove", &|tx| {
        tx.remove("/images/sea-2")?;
        tx.remove("/masks/vignette")
    });
    let lines = c.to_string();
    assert!(
        lines.contains("/images  order draw  [dusk, sea, sea-2] → [dusk, sea]"),
        "{lines}"
    );
    assert!(
        lines.contains("/images/hero  members  -/images/sea-2"),
        "{lines}"
    );
    assert!(
        lines.contains("/images/dusk  slot mask ← /masks/vignette  bound → unbound"),
        "{lines}"
    );
    assert!(lines.contains("/images/sea-2/blur  removed"), "{lines}");
    assert!(
        lines.contains("/images/sea  slot mask ← /masks/vignette  bound → unbound"),
        "{lines}"
    );
    let fin = t.at("/renders/final").unwrap();
    assert!(
        fin.reference("input").is_some() && fin.resolve("input").is_none(),
        "the ref stays, unresolved"
    );
    let (_, report) = Tree::load(&t.serialise(), registry()).unwrap();
    assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
    assert!(report.issues[0].message.contains("missing node"));
    let sea = t.at("/images/sea").unwrap();
    assert_eq!(
        sea.reference("source").unwrap().file.as_deref(),
        Some("other.rhizome")
    );
    assert!(
        sea.resolve("source").is_none(),
        "a cross-file ref is stored, not resolved"
    );

    // rebinding a slot swaps its source; a value key takes many sources; an empty order goes
    step(&mut t, "Masks", &|tx| {
        tx.add("/masks", "mask", "frame")?;
        tx.add("/masks", "mask", "border")?;
        tx.add("/modulators", "lfo", "drift")?;
        tx.bind(
            "/images/sea",
            On::slot("mask"),
            "/masks/frame",
            Vec::<(&str, Value)>::new(),
        )
    });
    let c = step(&mut t, "Swap Mask", &|tx| {
        tx.bind(
            "/images/sea",
            On::slot("mask"),
            "/masks/border",
            Vec::<(&str, Value)>::new(),
        )
    });
    let lines = c.to_string();
    assert_eq!(c.len(), 2, "{lines}");
    assert!(
        lines.contains("/images/sea  slot mask ← /masks/frame  bound → unbound"),
        "{lines}"
    );
    assert!(
        lines.contains("/images/sea  slot mask ← /masks/border  unbound → bound"),
        "{lines}"
    );
    step(&mut t, "Two Lfos", &|tx| {
        tx.bind(
            "/images/dusk",
            On::value(OPACITY),
            "/modulators/wobble",
            [("depth", Value::Float(0.1))],
        )?;
        tx.bind(
            "/images/dusk",
            On::value(OPACITY),
            "/modulators/drift",
            Vec::<(&str, Value)>::new(),
        )
    });
    let dusk = t.at("/images/dusk").unwrap();
    let links: Vec<_> = dusk
        .bindings()
        .into_iter()
        .filter(|b| b.on == &On::value(OPACITY))
        .collect();
    assert_eq!(links.len(), 2, "both modulators stay bound to one key");
    assert_eq!(
        links.iter().map(|b| b.value("depth")).collect::<Vec<_>>(),
        [Some(Value::Float(0.1)), Some(Value::Float(0.5))]
    );
    step(&mut t, "Clear Order", &|tx| {
        tx.set_order("/images/sea", "modifiers", Vec::<NodeId>::new())
    });
    assert_eq!(
        t.at("/images/sea").unwrap().order_names().count(),
        0,
        "an empty order is removed"
    );

    // ---- gestures ----
    let before = t.snapshot();
    let steps = t.history_len();
    let g = t.begin("Drag Opacity").unwrap();
    assert!(matches!(t.begin("again"), Err(Error::GestureOpen)));
    for v in [0.9, 0.7, 0.5] {
        let c = t.apply(g, &[serde_json::from_value(serde_json::json!({"op": "set", "at": "/images/dusk", "key": "opacity", "value": v})).unwrap()]).unwrap();
        assert!(c.is_some(), "each apply commits, so the panel follows");
    }
    assert!(
        t.apply(
            g,
            &[serde_json::from_value(
                serde_json::json!({"op": "set", "at": "/images/dusk", "key": "opacity", "value": 7})
            )
            .unwrap()]
        )
        .is_err()
    );
    assert_eq!(
        t.at("/images/dusk").unwrap().get(OPACITY),
        Some(0.5),
        "a refused apply leaves the gesture's state"
    );
    assert!(matches!(t.undo(), Err(Error::GestureOpen)));
    let c = t.cancel(g).unwrap().unwrap();
    assert_eq!(c.label, "Cancel Drag Opacity");
    assert_eq!(t.snapshot(), before, "cancel puts everything back");
    assert_eq!(t.history_len(), steps);

    let g = t.begin("Drag Opacity").unwrap();
    t.within(g, |tx| tx.set(sky2(tx), OPACITY, 0.3)).unwrap();
    t.edit("Knob", |tx| tx.set("/images/sea", OPACITY, 0.6))
        .unwrap(); // another source joins
    t.end(g).unwrap();
    assert_eq!(t.history_len(), steps + 1, "a gesture is one undo step");
    assert_eq!(t.undo_label(), Some("Drag Opacity"));
    t.undo().unwrap();
    assert_eq!(
        t.snapshot(),
        before,
        "undoing the gesture reverts the joined write too"
    );
    t.redo().unwrap();

    // ---- coalescing ----
    let now = Arc::new(Mutex::new(Instant::now()));
    let clock = now.clone();
    t.set_clock(move || *clock.lock().unwrap());
    let steps = t.history_len();
    for i in 0..10 {
        *now.lock().unwrap() += Duration::from_millis(100);
        t.edit_coalesced("Nudge Layer", "nudge layer", |tx| {
            tx.set("/images/sea", LAYER, i)
        })
        .unwrap();
    }
    assert_eq!(t.history_len(), steps + 1, "ten nudges, one undo step");
    *now.lock().unwrap() += Duration::from_secs(5);
    t.edit_coalesced("Nudge Layer", "nudge layer", |tx| {
        tx.set("/images/sea", LAYER, 42)
    })
    .unwrap();
    assert_eq!(t.history_len(), steps + 2, "a pause starts a new step");

    // ---- undo everything, redo everything ----
    let end = t.snapshot();
    let end_text = t.serialise();
    let mut undone = 0;
    while t.undo().unwrap().is_some() {
        undone += 1;
    }
    assert_eq!(
        t.snapshot(),
        empty,
        "undo all the way back is the empty tree ({undone} steps)"
    );
    while t.redo().unwrap().is_some() {}
    assert_eq!(
        t.snapshot(),
        end,
        "redo all the way forward is where we were"
    );
    assert_eq!(t.serialise(), end_text);

    // ---- undo back to the saved point is clean ----
    t.mark_saved();
    t.edit("Touch", |tx| tx.set("/images/sea", LAYER, 7))
        .unwrap();
    assert!(t.is_unsaved());
    t.undo().unwrap();
    assert!(
        !t.is_unsaved(),
        "undo back to the saved point clears unsaved"
    );

    golden::check("acid.log", &log);
}

fn sky_id(tx: &Edit<'_>) -> NodeId {
    tx.at("/images/sky").unwrap().id()
}

fn sky2(tx: &Edit<'_>) -> NodeId {
    tx.at("/images/dusk").unwrap().id()
}

#[test]
fn opaque_nodes_pass_through() {
    let text = built().serialise();
    let mut doc: serde_json::Value = serde_json::from_str(&text).unwrap();
    let vignette = doc["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "/masks/vignette")
        .unwrap()["id"]
        .clone();
    let sparkle = serde_json::json!({
        "path": "/images/sky/sparkle",
        "id": "00000000000000000000000099",
        "type": "sparkle",
        "values": {"density": 0.75, "palette": ["gold", "white"], "seed": 7},
        "bindings": [{"on": {"slot": "mask"}, "source": vignette, "values": {"whatever": [1, 2]}}]
    });
    doc["nodes"].as_array_mut().unwrap().push(sparkle.clone());
    let text = serde_json::to_string_pretty(&doc).unwrap();

    let (mut t, report) = Tree::load_with_ids(&text, registry(), IdSource::sequential()).unwrap();
    assert_eq!(report.issues.len(), 1, "{:?}", report.issues);
    assert_eq!(
        report.issues[0].to_string(),
        "/images/sky/sparkle: unknown type `sparkle`, kept as is"
    );
    let node = t.at("/images/sky/sparkle").unwrap();
    assert!(node.is_opaque() && node.node_type().is_none());

    // no verb writes to it
    use Error as E;
    let opaque = |e: &Error| matches!(e, E::Opaque(_));
    refused(
        &mut t,
        |tx| tx.set_value("/images/sky/sparkle", "density", Value::Float(0.1)),
        opaque,
    );
    refused(
        &mut t,
        |tx| tx.rename("/images/sky/sparkle", "glitter"),
        opaque,
    );
    refused(&mut t, |tx| tx.remove("/images/sky/sparkle"), opaque);
    refused(
        &mut t,
        |tx| tx.move_to("/images/sky/sparkle", "/images/sea"),
        opaque,
    );
    refused(
        &mut t,
        |tx| tx.join("/images/hero", ["/images/sky/sparkle"]),
        opaque,
    );
    refused(
        &mut t,
        |tx| tx.add("/images/sky/sparkle", "blur", "b").map(drop),
        opaque,
    );
    refused(
        &mut t,
        |tx| tx.unbind("/images/sky/sparkle", On::slot("mask"), "/masks/vignette"),
        opaque,
    );

    // edits elsewhere leave it byte for byte, and it travels with its ancestors
    let record = |t: &Tree, path: &str| -> serde_json::Value {
        let doc: serde_json::Value = serde_json::from_str(&t.serialise()).unwrap();
        let mut r = doc["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["path"] == path)
            .unwrap()
            .clone();
        r.as_object_mut().unwrap().remove("id");
        r.as_object_mut().unwrap().remove("path");
        r
    };
    let mut expected = sparkle.clone();
    expected.as_object_mut().unwrap().remove("id");
    expected.as_object_mut().unwrap().remove("path");
    t.edit("Elsewhere", |tx| tx.set("/images/sea", OPACITY, 0.2))
        .unwrap();
    assert_eq!(record(&t, "/images/sky/sparkle"), expected);
    t.edit("Rename Parent", |tx| tx.rename("/images/sky", "dusk"))
        .unwrap();
    assert_eq!(record(&t, "/images/dusk/sparkle"), expected);
    t.edit("Copy Parent", |tx| {
        tx.copy("/images/dusk", "/images").map(drop)
    })
    .unwrap();
    assert_eq!(
        record(&t, "/images/dusk-2/sparkle"),
        expected,
        "a copy carries it verbatim"
    );

    // its structural pointers still cascade: the mask goes, the binding goes
    t.edit("Remove Mask", |tx| tx.remove("/masks/vignette"))
        .unwrap();
    assert!(record(&t, "/images/dusk/sparkle").get("bindings").is_none());

    // canonical text round-trips byte for byte
    let canonical = t.serialise();
    let (again, report) = Tree::load(&canonical, registry()).unwrap();
    assert_eq!(report.issues.len(), 2, "two opaque nodes, reported again");
    assert_eq!(again.serialise(), canonical);
}

#[test]
fn paste_into_another_file() {
    let source = built();
    let fragment = source.extract(["/images/sky"]).unwrap();
    let text = fragment.to_text();
    assert_eq!(
        Fragment::from_text(&text).unwrap(),
        fragment,
        "the clipboard text round-trips"
    );
    assert!(
        matches!(Tree::load(&text, registry()), Err(Error::Format(_))),
        "a fragment is not a file"
    );
    assert!(
        matches!(
            Fragment::from_text(&source.serialise()),
            Err(Error::Format(_))
        ),
        "a file is not a fragment"
    );
    let op = Op::Paste {
        parent: "/images".into(),
        fragment: text.clone(),
    };
    let json = serde_json::to_string(&op).unwrap();
    assert!(json.starts_with(r#"{"op":"paste","parent":"/images","fragment":"#));
    assert_eq!(
        serde_json::from_str::<Op>(&json).unwrap(),
        op,
        "Paste round-trips as JSON"
    );

    // the other file has a vignette at the same path but no wobble and no hero group
    let mut other = Tree::with_ids(registry(), IdSource::Sequential { next: 1000 });
    other
        .edit("Setup", |tx| tx.add("/masks", "mask", "vignette").map(drop))
        .unwrap();
    let ((), c) = other
        .edit("Paste", |tx| {
            let report = tx.paste("/images", &Fragment::from_text(&text)?)?;
            let issues: Vec<String> = report.issues.iter().map(|i| i.to_string()).collect();
            assert_eq!(
                issues,
                [
                    "/images/sky: value opacity: source `0000000000000000000000000B` is missing, dropped",
                    "/images/sky: not added to group /images/hero",
                ]
            );
            Ok(())
        })
        .unwrap();
    let c = c.unwrap();
    assert!(
        c.changes
            .to_string()
            .contains("/images/sky/blur  added blur")
    );
    let sky = other.at("/images/sky").unwrap();
    assert_eq!(
        sky.bindings().len(),
        1,
        "the mask binding found /masks/vignette by path"
    );
    assert_eq!(
        sky.bindings()[0].source.id(),
        other.at("/masks/vignette").unwrap().id()
    );
    assert!(sky.groups().is_empty());

    // pasting again names it sky-2 and appends it to the parent's orders
    other
        .edit("Order", |tx| {
            tx.set_order("/images", "draw", ["/images/sky"])
        })
        .unwrap();
    other
        .edit("Paste Again", |tx| tx.paste("/images", &fragment).map(drop))
        .unwrap();
    let draw: Vec<_> = other
        .at("/images")
        .unwrap()
        .order("draw")
        .iter()
        .map(|n| n.name())
        .collect();
    assert_eq!(draw, ["sky", "sky-2"]);
}

#[test]
fn registry_refuses_bad_declarations() {
    let bad = [
        Registry::builder()
            .category("a", Origin::Loaded)
            .category("a", Origin::Loaded)
            .build(),
        Registry::builder().node(NodeType::new("group")).build(),
        Registry::builder()
            .node(NodeType::new("x").float("k", 0.0..=1.0, 2.0))
            .build(),
        Registry::builder()
            .node(NodeType::new("x").float("k", 0.0..=1.0, 0.0).reference("k"))
            .build(),
        Registry::builder()
            .node(NodeType::new("x").in_categories(&["nowhere"]))
            .build(),
        Registry::builder()
            .node(NodeType::new("x").choice("c", &["a"], "b"))
            .build(),
        Registry::builder()
            .node(NodeType::new("x"))
            .node(NodeType::new("x"))
            .build(),
    ];
    for (i, r) in bad.iter().enumerate() {
        assert!(
            matches!(r, Err(Error::Registry(_))),
            "declaration {i} should be refused"
        );
    }
}

#[test]
fn load_reports_instead_of_failing() {
    assert!(matches!(
        Tree::load("not json", registry()),
        Err(Error::Format(_))
    ));
    assert!(matches!(
        Tree::load(r#"{"nodes": []}"#, registry()),
        Err(Error::Format(_))
    ));
    assert!(matches!(
        Tree::load(r#"{"rhizome": 2, "nodes": []}"#, registry()),
        Err(Error::Version(2))
    ));

    let text = serde_json::json!({
        "rhizome": 1,
        "nodes": [
            {"path": "/", "id": "00000000000000000000000001", "type": "root"},
            {"path": "/images", "id": "00000000000000000000000002", "type": "category"},
            {"path": "/images/a", "id": "00000000000000000000000003", "type": "image",
             "values": {"opacity": 4.0, "blend": "overlay", "layer": "three", "nope": 1}},
            {"path": "/images/b", "id": "00000000000000000000000003", "type": "image"},
            {"path": "/images/c/d", "id": "00000000000000000000000005", "type": "image"},
            {"path": "/images/e", "id": "not-an-id", "type": "image"},
            {"path": "/loose", "id": "00000000000000000000000007", "type": "image"},
            {"path": "/extra", "id": "00000000000000000000000008", "type": "category"}
        ]
    })
    .to_string();
    let (t, report) = Tree::load(&text, registry()).unwrap();
    let issues: Vec<String> = report.issues.iter().map(|i| i.to_string()).collect();
    golden::check("load-report.txt", &(issues.join("\n") + "\n"));
    assert_eq!(
        t.at("/images/a").unwrap().get(OPACITY),
        Some(1.0),
        "out of range on load is clamped"
    );
    assert!(
        t.at("/masks").is_some(),
        "declared categories missing from a file are made"
    );
    assert!(
        t.at("/extra").unwrap().is_opaque(),
        "an unknown category is kept as is"
    );
}

#[test]
fn tree_rules_see_the_whole_tree_and_the_changes() {
    let registry = Registry::builder()
        .category("things", Origin::Loaded)
        .node(NodeType::new("box"))
        .node(NodeType::new("lid"))
        .rule(|root, changes| {
            // at most one lid per box, and a lid only leaves with its box
            for cat in root.children() {
                for b in cat.children() {
                    if b.children().filter(|c| c.type_name() == "lid").count() > 1 {
                        return Err(Violation {
                            path: b.path().to_string(),
                            message: "one lid per box".into(),
                        });
                    }
                }
            }
            for c in changes.iter() {
                if let ChangeKind::Removed { type_name } = &c.kind
                    && type_name == "lid"
                    && root
                        .children()
                        .any(|cat| cat.child(c.path.parent().unwrap().name()).is_some())
                {
                    return Err(Violation {
                        path: c.path.to_string(),
                        message: "a lid can't be removed from its box".into(),
                    });
                }
            }
            Ok(())
        })
        .build()
        .unwrap();
    let mut t = Tree::with_ids(registry, IdSource::sequential());
    t.edit("Box", |tx| {
        let b = tx.add("/things", "box", "b")?;
        tx.add(b, "lid", "lid").map(drop)
    })
    .unwrap();
    let before = t.snapshot();
    let err = t
        .edit("Second Lid", |tx| {
            tx.add("/things/b", "lid", "lid2").map(drop)
        })
        .unwrap_err();
    assert_eq!(err.to_string(), "/things/b: one lid per box");
    let err = t
        .edit("Remove Lid", |tx| tx.remove("/things/b/lid"))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "/things/b/lid: a lid can't be removed from its box"
    );
    assert_eq!(t.snapshot(), before, "refused rules leave no trace");
    t.edit("Remove Box", |tx| tx.remove("/things/b")).unwrap();
    assert!(t.at("/things/b").is_none(), "a lid leaves with its box");
}

#[test]
fn structured_values_are_checked() {
    let mut t = Tree::with_ids(registry(), IdSource::sequential());
    let ((), _) = t
        .edit("Build", |tx| {
            let sky = tx.add("/images", "image", "sky")?;
            tx.add(sky, "blur", "blur")?;
            Ok(())
        })
        .unwrap();
    let set = |t: &mut Tree, key: &str, v: Value| {
        t.edit("Set", |tx| tx.set_value("/images/sky/blur", key, v))
            .map(|_| ())
            .map_err(|e| e.to_string())
    };
    let e = set(&mut t, "matrix", Value::Floats(vec![1.0, 0.0])).unwrap_err();
    assert!(e.contains("should hold 4 numbers, not 2"), "{e}");
    let e = set(
        &mut t,
        "outline",
        Value::Shaped(serde_json::json!([{"anchors": [{"point": [0]}], "closed": true}])),
    )
    .unwrap_err();
    assert!(
        e.contains("[0].anchors[0].point should be two numbers"),
        "{e}"
    );
    assert!(set(&mut t, "angle", Value::Float(1e9)).is_ok(), "unbounded");
    let blur = t.at("/images/sky/blur").unwrap();
    assert_eq!(
        blur.value("outline"),
        Some(Value::Shaped(serde_json::json!([]))),
        "empty by default"
    );
    assert_eq!(
        blur.value("matrix"),
        Some(Value::Floats(vec![1.0, 0.0, 0.0, 1.0]))
    );
}

#[test]
fn a_soft_max_lets_a_typed_value_past_it() {
    let r = Registry::builder()
        .category("ops", Origin::Loaded)
        .node(
            NodeType::new("halftone")
                .in_categories(&["ops"])
                .value(ValueSpec::float("dot_size", 1.0..=32.0, 8.0).soft_max()),
        )
        .build()
        .unwrap();
    let mut t = Tree::with_ids(r.clone(), IdSource::sequential());
    let set = |t: &mut Tree, x: f64| {
        t.edit("Set", |tx| {
            let id = match tx.at("/ops/h") {
                Some(n) => n.id(),
                None => tx.add("/ops", "halftone", "h")?,
            };
            tx.set_value(id, "dot_size", Value::Float(x))
        })
        .map(|_| ())
    };
    assert!(set(&mut t, 120.0).is_ok(), "past the soft max");
    assert!(set(&mut t, 0.5).is_err(), "the min still holds");
    let (again, report) = Tree::load(&t.serialise(), r.clone()).unwrap();
    assert!(report.issues.is_empty(), "{report:?}");
    assert_eq!(
        again.at("/ops/h").unwrap().value("dot_size"),
        Some(Value::Float(120.0))
    );
    let schema = serde_json::to_value(r.schema()).unwrap();
    assert_eq!(schema["types"][0]["values"][0]["soft_max"], true);
    assert_eq!(
        schema["types"][0]["values"][0]["range"],
        serde_json::json!([1.0, 32.0])
    );
}

#[test]
fn history_lists_every_step() {
    let mut t = new_tree();
    t.edit("Add Sky", |tx| {
        tx.add("/images", "image", "sky").map(|_| ())
    })
    .unwrap();
    t.edit("Add Sea", |tx| {
        tx.add("/images", "image", "sea").map(|_| ())
    })
    .unwrap();
    t.edit("Add Hero", |tx| {
        tx.add("/images", "group", "hero").map(|_| ())
    })
    .unwrap();
    assert!(t.redo_labels().next().is_none());

    t.undo().unwrap();
    t.undo().unwrap();
    assert_eq!(t.undo_labels().collect::<Vec<_>>(), ["Add Sky"]);
    assert_eq!(t.redo_labels().collect::<Vec<_>>(), ["Add Sea", "Add Hero"]);
    assert_eq!(t.undo_labels().next_back(), t.undo_label());
    assert_eq!(t.redo_labels().next(), t.redo_label());
    assert_eq!(t.history_len(), t.undo_labels().count());
}
