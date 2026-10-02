//! The wire shape of a commit: what a transport (Tauri events, later wasm) carries and the
//! TypeScript half mirrors by hand. Every kind of change once, pinned in
//! `tests/golden/commit.json`.

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
    let json = serde_json::to_string_pretty(&commit.unwrap()).unwrap() + "\n";
    golden::check("commit.json", &json);
}
