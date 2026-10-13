//! The wire shape of a commit: what a transport (Tauri events, later wasm) carries and the
//! TypeScript half mirrors by hand. Every kind of change once, pinned in
//! `tests/golden/commit.json`, and the mirror's view of the same tree in
//! `tests/golden/view.json`.

mod golden;

use rhizome_core::*;

#[test]
fn every_change_kind_on_the_wire() {
    let registry = Registry::builder()
        .category("images", Origin::Loaded)
        .category("mods", Origin::Loaded)
        .node(
            NodeType::new("image")
                .in_categories(&["images"])
                .float("opacity", 0.0..=1.0, 1.0)
                .int("layer", 0..=9, 0)
                .vec2("offset", [0.0, 0.0])
                .reference("source")
                .slot("mask"),
        )
        .node(
            NodeType::new("lfo")
                .in_categories(&["mods"])
                .bindable([ValueSpec::float("depth", -1.0..=1.0, 0.5)]),
        )
        .build()
        .unwrap();
    let mut tree = Tree::with_ids(registry, IdSource::sequential());
    let ((sky, sea, lfo), _) = tree
        .edit("Build", |tx| {
            let sky = tx.add("/images", "image", "sky")?;
            let sea = tx.add("/images", "image", "sea")?;
            let lfo = tx.add("/mods", "lfo", "wobble")?;
            tx.add("/images", GROUP, "favourites")?;
            tx.add("/images", "image", "old")?;
            Ok((sky, sea, lfo))
        })
        .unwrap();
    let (_, commit) = tree
        .edit("Everything", |tx| {
            tx.set_value(sky, "opacity", Value::Float(0.5))?;
            tx.set_value(sky, "layer", Value::Int(3))?;
            tx.set_value(sky, "offset", Value::Vec2([1.0, -2.5]))?;
            tx.set_ref(sky, "source", Ref::here(sea))?;
            tx.bind(
                sky,
                On::value("opacity"),
                lfo,
                [("depth", Value::Float(0.3))],
            )?;
            tx.set_order("/images", "draw", [sea, sky])?;
            tx.join("/images/favourites", [sky])?;
            tx.rename(sea, "ocean")?;
            tx.add("/images", "image", "new")?;
            tx.remove("/images/old")
        })
        .unwrap();
    let commit = commit.unwrap();
    let json = serde_json::to_string_pretty(&commit).unwrap() + "\n";
    golden::check("commit.json", &json);

    // the mirror's side (decision 48): the schema, every row, and this commit's patch
    let view = serde_json::json!({
        "schema": tree.registry().schema(),
        "rows": tree.rows(),
        "patch": tree.patch(&commit.changes),
    });
    golden::check(
        "view.json",
        &(serde_json::to_string_pretty(&view).unwrap() + "\n"),
    );
}

/// A float survives the file format bit for bit. serde_json's default parser can be one
/// unit in the last place off (1026.5436337625179 read back as 1026.543633762518); rhizome
/// turns on `float_roundtrip`. Found by the Oblique spike on a real project.
#[test]
fn floats_survive_the_file_bit_for_bit() {
    let registry = Registry::builder()
        .category("images", Origin::Loaded)
        .node(
            NodeType::new("image")
                .in_categories(&["images"])
                .float_unbounded("x", 0.0),
        )
        .build()
        .unwrap();
    let mut tree = Tree::with_ids(registry.clone(), IdSource::sequential());
    let tricky = [
        1026.5436337625179,
        0.1 + 0.2,
        1.0 / 3.0,
        -2.5e-300,
        123456789.12345679,
    ];
    for (i, x) in tricky.iter().enumerate() {
        tree.edit("Set", |tx| {
            let n = tx.add("/images", "image", &format!("n{i}"))?;
            tx.set_value(n, "x", Value::Float(*x))
        })
        .unwrap();
    }
    let (again, _) = Tree::load(&tree.serialise(), registry).unwrap();
    for (i, x) in tricky.iter().enumerate() {
        let got = again
            .at(format!("/images/n{i}").as_str())
            .unwrap()
            .value("x");
        assert_eq!(got, Some(Value::Float(*x)), "{x:?}");
    }
}

/// A type's and a value's words (label, summary, group, description, unit) reach the schema
/// when given and leave no trace when not, and a dumped schema reads back as it was. Pinned in
/// `tests/golden/schema-words.json`.
#[test]
fn the_schema_carries_words_and_reads_back() {
    let registry = Registry::builder()
        .category("things", Origin::Loaded)
        .node(
            NodeType::new("widget")
                .label("Widget")
                .summary("A made-up node with words.")
                .group("parts")
                .float("size", 0.0..=10.0, 1.0)
                .described("Size", "How big it is.")
                .unit("cm")
                .choice("finish", &["matte", "gloss"], "matte")
                .described("Finish", "How it catches the light.")
                .bool("on", true)
                .bindable([ValueSpec::float("amount", 0.0..=1.0, 0.5)
                    .label("Amount")
                    .unit("%")]),
        )
        .node(NodeType::new("plain").int("count", 0..=4, 0))
        .build()
        .unwrap();
    let widget = registry.node_type("widget").unwrap();
    assert_eq!(widget.label_text(), Some("Widget"));
    assert_eq!(widget.summary_text(), Some("A made-up node with words."));
    assert_eq!(widget.group_tag(), Some("parts"));
    assert_eq!(widget.spec("size").unwrap().unit.as_deref(), Some("cm"));
    assert_eq!(widget.spec("on").unwrap().label, None);

    let schema = registry.schema();
    // pinned for the TypeScript half, which checks these keys against wire.ts
    golden::check(
        "schema-words.json",
        &(serde_json::to_string_pretty(&schema).unwrap() + "\n"),
    );
    let json = serde_json::to_value(&schema).unwrap();
    let types = json["types"].as_array().unwrap();
    let plain = types.iter().find(|t| t["name"] == "plain").unwrap();
    for absent in ["label", "summary", "group"] {
        assert!(plain.get(absent).is_none(), "{absent} should be absent");
    }
    for absent in ["label", "description", "unit"] {
        assert!(
            plain["values"][0].get(absent).is_none(),
            "{absent} should be absent"
        );
    }
    let w = types.iter().find(|t| t["name"] == "widget").unwrap();
    assert_eq!(w["label"], "Widget");
    assert_eq!(w["group"], "parts");
    assert_eq!(w["values"][0]["description"], "How big it is.");
    assert_eq!(w["values"][0]["unit"], "cm");
    assert_eq!(w["bindable"][0]["label"], "Amount");

    let back: Schema = serde_json::from_value(json).unwrap();
    assert_eq!(back, schema);
}

#[test]
#[should_panic(expected = "`described` follows a value")]
fn words_for_a_value_need_a_value() {
    let _ = NodeType::new("widget").described("Size", "How big it is.");
}
