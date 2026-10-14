//! The generator over a made-up registry: the pages it writes (pinned in `tests/golden/`),
//! what it keeps when it rewrites one, and what `check` notices.

use std::fs;
use std::path::PathBuf;

use rhizome_core::{NodeType, Origin, Registry, Schema, Shape, ValueSpec};
use rhizome_docs::{Finding, INDEX, Page, check, pages, read_schema, update, write};

fn schema() -> Schema {
    Registry::builder()
        .category("things", Origin::Loaded)
        .node(
            NodeType::new("widget")
                .label("Widget")
                .summary("A made-up part: round, and a little louder when it spins.")
                .group("parts")
                .bool("widget.on", true)
                .float("widget.size", 0.5..=20.0, 1.0)
                .described("Size", "How big it is | in centimetres.")
                .unit("cm")
                .float("widget.spin", 0.0..=1.0, 0.25)
                .described("Spin", "How fast it turns.")
                .unit("%")
                .value(ValueSpec::float("widget.reach", 0.0..=10.0, 2.0).soft_max())
                .choice("widget.finish", &["matte", "gloss", "brushed"], "gloss")
                .described("Finish", "How it catches the light.")
                .int("widget.teeth", 0..=64, 12)
                .colour("widget.tint", [1.0, 0.5, 0.0, 1.0])
                .vec2("widget.offset", [0.0, -1.5])
                .text("widget.note", "")
                .shaped("widget.path", Shape::list(Shape::Vec2))
                .reference("widget.source")
                .slot("mask"),
        )
        .node(
            NodeType::new("wobble")
                .label("Wobble")
                .summary("Moves another value back and forth.")
                .group("movers")
                .float("wobble.rate", 0.01..=40.0, 1.0)
                .described("Rate", "How often it moves.")
                .unit("Hz")
                .float("wobble.cutoff", 20.0..=20000.0, 1500.0)
                .described("Cutoff", "Where it stops.")
                .unit("Hz")
                .float("wobble.fade", 10.0..=2000.0, 250.0)
                .described("Fade", "How long it takes.")
                .unit("ms")
                .int("wobble.tracking", 0..=2, 1)
                .described("Tracking", "How far it follows the note.")
                .names(&["Off", "Half", "Full"])
                .bindable([ValueSpec::float("depth", -1.0..=1.0, 0.5)
                    .described("Depth", "How far it moves the value.")]),
        )
        .node(NodeType::new("bare").float_unbounded("x", 0.0))
        .build()
        .unwrap()
        .schema()
}

fn page(name: &str) -> Page {
    pages(&schema(), "Example")
        .into_iter()
        .find(|p| p.file_name == name)
        .unwrap()
}

fn golden(name: &str, actual: &str) {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name);
    if std::env::var_os("RHIZOME_BLESS").is_some() {
        fs::write(&file, actual).unwrap();
        return;
    }
    let expected = fs::read_to_string(&file)
        .unwrap_or_else(|_| panic!("no golden {name}; run `just bless` and read it"));
    assert_eq!(expected, actual, "{name} differs from its golden");
}

/// A fresh, empty folder of the test's own.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rhizome-docs-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_page_and_the_index_as_generated() {
    golden("widget.md", &page("widget.md").content);
    golden("wobble.md", &page("wobble.md").content);
    golden("README.md", &page(INDEX).content);
}

#[test]
fn rewriting_keeps_every_byte_outside_the_block() {
    let fresh = page("widget.md");
    let written = fresh.content.replace(
        "## Using it\n",
        "## Using it\n\nHand-written, with a trailing space \nand CRLF\r\nand no final newline",
    );
    let written = written.replacen("node: widget\n", "node: widget\nauthor: someone\n", 1);
    // the block goes stale: someone edited the generated table by hand
    let stale = written.replacen("How fast it turns.", "Out of date.", 1);
    let rewritten = update(Some(&stale), &fresh);
    assert_eq!(rewritten, written);

    let begin = stale.find("<!-- generated:begin").unwrap();
    let end = stale.find("<!-- generated:end -->").unwrap();
    assert_eq!(rewritten[..begin], stale[..begin]);
    let tail = stale.len() - end;
    assert_eq!(rewritten[rewritten.len() - tail..], stale[end..]);
}

#[test]
fn a_custom_begin_marker_line_is_kept() {
    let fresh = page("wobble.md");
    let own = fresh.content.replacen(
        "<!-- generated:begin — rhizome-docs rewrites this block; write outside it -->",
        "<!-- generated:begin (just docs) -->",
        1,
    );
    assert_eq!(update(Some(&own), &fresh), own);
}

#[test]
fn generating_twice_changes_nothing() {
    for p in pages(&schema(), "Example") {
        let once = update(None, &p);
        let twice = update(Some(&once), &p);
        assert_eq!(once, twice, "{}", p.file_name);
    }
    let dir = scratch("twice");
    assert!(!write(&dir, &schema(), "Example").unwrap().is_empty());
    assert_eq!(
        write(&dir, &schema(), "Example").unwrap(),
        Vec::<String>::new()
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_page_without_markers_gets_the_block_appended() {
    let fresh = page("wobble.md");
    let mine = "# Wobble\n\nWritten before the generator.\n";
    let merged = update(Some(mine), &fresh);
    // the generated front matter goes in front; the page follows unchanged
    assert!(merged.starts_with("---\ntitle: Wobble\n"));
    assert!(merged.contains(&format!("---\n\n{mine}")));
    assert!(merged.contains("| Rate | 0.01 Hz to 40 Hz | 1 Hz | How often it moves. |"));
    assert_eq!(update(Some(&merged), &fresh), merged);
}

#[test]
fn a_broken_block_is_left_alone_and_reported() {
    let dir = scratch("broken");
    write(&dir, &schema(), "Example").unwrap();
    let path = dir.join("wobble.md");
    let broken = fs::read_to_string(&path)
        .unwrap()
        .replace("<!-- generated:end -->", "");
    fs::write(&path, &broken).unwrap();
    assert_eq!(update(Some(&broken), &page("wobble.md")), broken);
    let found = check(&dir, &schema(), "Example").unwrap();
    assert!(found.contains(&Finding::NoBlock {
        file: "wobble.md".into()
    }));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn check_finds_what_is_out_of_step() {
    let dir = scratch("check");
    let schema = schema();
    let failures = |dir: &PathBuf| -> Vec<Finding> {
        check(dir, &schema, "Example")
            .unwrap()
            .into_iter()
            .filter(|f| !f.is_warning())
            .collect()
    };

    // nothing written yet
    let found = failures(&dir);
    assert!(found.contains(&Finding::MissingPage {
        file: "widget.md".into()
    }));
    assert!(found.contains(&Finding::StaleIndex));
    assert!(found.contains(&Finding::MissingSummary {
        node: "bare".into()
    }));

    // written: only the missing summary is left, and the missing descriptions warn
    write(&dir, &schema, "Example").unwrap();
    assert_eq!(
        failures(&dir),
        vec![Finding::MissingSummary {
            node: "bare".into()
        }]
    );
    let all = check(&dir, &schema, "Example").unwrap();
    assert!(all.contains(&Finding::MissingDescription {
        node: "widget".into(),
        key: "widget.teeth".into()
    }));

    // a stale block, prose added by hand, and a page whose type is gone
    let path = dir.join("widget.md");
    let text = fs::read_to_string(&path).unwrap();
    let edited = text.replace("## Using it\n", "## Using it\n\nMine.\n");
    fs::write(
        &path,
        edited.replacen("How fast it turns.", "Old words.", 1),
    )
    .unwrap();
    fs::write(dir.join("gone.md"), "# Gone\n").unwrap();
    let found = failures(&dir);
    assert!(found.contains(&Finding::StaleBlock {
        file: "widget.md".into()
    }));
    assert!(found.contains(&Finding::Orphan {
        file: "gone.md".into()
    }));

    // regenerating fixes the block, keeps the prose, and leaves the orphan for a person
    write(&dir, &schema, "Example").unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), edited);
    assert!(dir.join("gone.md").exists());
    assert!(!failures(&dir).contains(&Finding::StaleBlock {
        file: "widget.md".into()
    }));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_dumped_schema_or_a_view_reads_back() {
    let schema = schema();
    let json = serde_json::to_string(&schema).unwrap();
    assert_eq!(read_schema(&json).unwrap(), schema);
    let view = serde_json::json!({ "seq": 0, "schema": schema, "rows": [] }).to_string();
    assert_eq!(read_schema(&view).unwrap(), schema);
}

#[test]
fn the_front_matter_keeps_hand_keys_and_follows_the_schema() {
    let fresh = page("widget.md");
    // a hand-added key at the top and one after the summary, and a summary out of date
    let hand = fresh
        .content
        .replacen("---\n", "---\ndraft: true\n", 1)
        .replacen(
            "summary: \"A made-up part: round, and a little louder when it spins.\"\n",
            "summary: \"A made-up part: round, and a little louder when it spins.\"\ntags: [a, b]   # mine\n",
            1,
        );
    let stale = hand.replacen(
        "summary: \"A made-up part: round, and a little louder when it spins.\"\n",
        "summary: An old sentence,\n  carried on a second line.\n",
        1,
    );
    assert_ne!(stale, hand);
    assert_eq!(update(Some(&stale), &fresh), hand);
    assert_eq!(update(Some(&hand), &fresh), hand);

    // a generated key gone missing comes back after the one before it
    let no_group = hand.replacen("group: parts\n", "", 1);
    assert_eq!(update(Some(&no_group), &fresh), hand);

    // keys the schema no longer gives are removed; others stay
    let bare = page("bare.md");
    let extra = bare.content.replacen(
        "node: bare\n",
        "node: bare\ngroup: old\nowner: me\nsummary: old\n",
        1,
    );
    assert_eq!(
        update(Some(&extra), &bare),
        bare.content
            .replacen("node: bare\n", "node: bare\nowner: me\n", 1)
    );
}

#[test]
fn check_reports_front_matter_drift_until_rewritten() {
    let dir = scratch("front");
    let schema = schema();
    write(&dir, &schema, "Example").unwrap();
    let path = dir.join("wobble.md");
    let on_disk = fs::read_to_string(&path).unwrap();
    let drifted = on_disk
        .replacen(
            "summary: Moves another value back and forth.",
            "summary: Old words.",
            1,
        )
        .replacen("---\n", "---\nauthor: someone\n", 1);
    fs::write(&path, &drifted).unwrap();
    let drift = Finding::StaleFrontMatter {
        file: "wobble.md".into(),
    };
    let found = check(&dir, &schema, "Example").unwrap();
    assert!(found.contains(&drift));
    assert!(!drift.is_warning());
    assert!(!found.contains(&Finding::StaleBlock {
        file: "wobble.md".into()
    }));

    write(&dir, &schema, "Example").unwrap();
    let fixed = fs::read_to_string(&path).unwrap();
    assert_eq!(
        fixed,
        on_disk.replacen("---\n", "---\nauthor: someone\n", 1)
    );
    assert!(!check(&dir, &schema, "Example").unwrap().contains(&drift));
    fs::remove_dir_all(&dir).unwrap();
}
