//! Seeded random `Op`s, checking the guarantees after every step:
//! no panic; a refused edit leaves no trace; nothing dangles; every stored value fits its
//! schema; `load(serialise(t))` is identical; undo walks back through every state; and a
//! mirror fed one snapshot and then only patches always equals a fresh view (decision 48).

use std::collections::BTreeMap;
use std::sync::Arc;

use rhizome_core::*;

fn registry() -> Arc<Registry> {
    Registry::builder()
        .category("a", Origin::Loaded)
        .category("b", Origin::Calculated)
        .node(
            NodeType::new("thing")
                .float("x", -1.0..=1.0, 0.0)
                .int("n", 0..=9, 0)
                .choice("c", &["p", "q"], "p")
                .vec2("v", [0.0, 0.0])
                .reference("r")
                .slot("s"),
        )
        .node(
            NodeType::new("source")
                .bindable([ValueSpec::float("depth", 0.0..=1.0, 0.5)])
                .float("x", 0.0..=1.0, 0.0),
        )
        .node(NodeType::new("only_a").in_categories(&["a"]))
        .build()
        .unwrap()
}

/// xorshift64*, so a failure names its seed and replays exactly.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len())]
    }

    fn float(&mut self) -> f64 {
        (self.below(41) as f64 - 20.0) / 10.0
    }
}

fn random_op(rng: &mut Rng, t: &Tree) -> Op {
    // the root rarely; everything else often
    let with_root = rng.below(10) == 0;
    let paths: Vec<String> = t
        .nodes()
        .iter()
        .filter(|n| with_root || !n.is_root())
        .map(|n| n.path().to_string())
        .collect();
    let mut p = || rng.pick(&paths).clone();
    let (a, b) = (p(), p());
    let names = ["n1", "n2", "n3", "n4"];
    let types = ["thing", "thing", "source", "only_a", "group", "nope"];
    let keys = ["x", "n", "c", "v", "zz"];
    let r = rng.below(14);
    let name = rng.pick(&names).to_string();
    let type_name = rng.pick(&types).to_string();
    let key = rng.pick(&keys).to_string();
    let value = match rng.below(5) {
        0 => serde_json::json!(rng.float()),
        1 => serde_json::json!(rng.below(12) as i64 - 1),
        2 => serde_json::json!(*rng.pick(&["p", "q", "r"])),
        3 => serde_json::json!([rng.float(), rng.float()]),
        _ => serde_json::json!(true),
    };
    match r {
        0 | 1 => Op::Add {
            parent: a,
            type_name,
            name,
        },
        2 => Op::AddUnique {
            parent: a,
            type_name,
            base: name,
        },
        3 => Op::Remove { at: a },
        4 => Op::Rename { at: a, name },
        5 => Op::MoveTo { at: a, parent: b },
        6 => Op::Copy { at: a, parent: b },
        7 | 8 => Op::Set { at: a, key, value },
        9 => Op::Reset { at: a, key },
        10 => match t.at(b.as_str()) {
            Some(n) => Op::SetRef {
                at: a,
                key: "r".into(),
                reference: Ref::here(n.id()).into(),
            },
            None => Op::ClearRef {
                at: a,
                key: "r".into(),
            },
        },
        11 => Op::Bind {
            target: a,
            on: if rng.below(2) == 0 {
                On::slot("s")
            } else {
                On::value("x")
            },
            source: b,
            values: [("depth".to_string(), serde_json::json!(rng.float()))].into(),
        },
        12 => Op::Join {
            group: a,
            members: vec![b],
        },
        _ => {
            let kids: Vec<String> = t
                .at(a.as_str())
                .map(|n| n.children().map(|c| c.path().to_string()).collect())
                .unwrap_or_default();
            Op::SetOrder {
                owner: a,
                name: "o".into(),
                ids: kids.into_iter().rev().collect(),
            }
        }
    }
}

fn check(t: &Tree, seed: u64, step: usize) {
    let at = format!("seed {seed}, step {step}");
    for n in t.nodes() {
        // these panic on a dangling pointer
        for name in n.order_names() {
            for c in n.order(name) {
                assert_eq!(
                    c.parent().unwrap().id(),
                    n.id(),
                    "{at}: order names a non-child"
                );
            }
        }
        let _ = (
            n.members(),
            n.bindings(),
            n.groups(),
            n.bound_to(),
            n.referrers(),
        );
        if let Some(t) = n.node_type() {
            for (k, v) in n.stored_values() {
                let spec = t
                    .spec(k)
                    .unwrap_or_else(|| panic!("{at}: {k} not in schema"));
                assert_eq!(v.kind(), spec.kind, "{at}: {k}");
                if let (Some((lo, hi)), Value::Float(x)) = (spec.range, v) {
                    assert!(*x >= lo && *x <= hi, "{at}: {k} = {x}");
                }
                if let Value::Choice(c) = v {
                    assert!(spec.choices.contains(c), "{at}: {k} = {c}");
                }
            }
        }
    }
    let text = t.serialise();
    let (loaded, report) = Tree::load(&text, registry()).unwrap();
    assert!(
        report
            .issues
            .iter()
            .all(|i| i.message.contains("missing node")),
        "{at}: {:?}",
        report.issues
    );
    assert_eq!(loaded.serialise(), text, "{at}: round trip");
    assert!(
        loaded.diff(&t.snapshot()).is_empty(),
        "{at}: round trip diff"
    );
}

#[test]
fn random_ops_keep_every_guarantee() {
    let (mut committed, mut refused, mut nodes) = (0, 0, 0);
    for seed in 1..=40u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mut t = Tree::with_ids(registry(), IdSource::sequential());
        let mut states = vec![t.snapshot()];
        let mut mirror = Mirror::of(&t);
        for step in 0..150 {
            let ops: Vec<Op> = (0..1 + rng.below(4) / 3)
                .map(|_| random_op(&mut rng, &t))
                .collect();
            let before = t.snapshot();
            let seq = t.seq();
            match t.edit_ops("Random", &ops) {
                Ok(Some(c)) => {
                    assert_eq!(c.seq, seq + 1);
                    mirror.apply(&t, &c, seed, step);
                    assert_eq!(
                        c.changes,
                        rhizome_core_diff(&before, &t),
                        "seed {seed}, step {step}"
                    );
                    states.push(t.snapshot());
                    committed += 1;
                }
                Ok(None) => assert_eq!(t.snapshot(), before),
                Err(_) => {
                    refused += 1;
                    assert_eq!(
                        t.snapshot(),
                        before,
                        "seed {seed}, step {step}: refused edit left a trace"
                    );
                    assert_eq!(t.seq(), seq);
                }
            }
            check(&t, seed, step);
            nodes = nodes.max(t.len());
        }
        let kept = t.history_len();
        assert_eq!(kept, (states.len() - 1).min(HISTORY), "seed {seed}");
        for back in 1..=kept {
            let c = t.undo().unwrap().unwrap();
            mirror.apply(&t, &c, seed, 1000 + back);
            assert_eq!(
                t.snapshot(),
                states[states.len() - 1 - back],
                "seed {seed}: undo {back}"
            );
        }
        assert!(t.undo().unwrap().is_none());
        for again in 0..kept {
            let c = t.redo().unwrap().unwrap();
            mirror.apply(&t, &c, seed, 2000 + again);
        }
        assert_eq!(
            &t.snapshot(),
            states.last().unwrap(),
            "seed {seed}: redo all"
        );
    }
    eprintln!("{committed} commits, {refused} refused, largest tree {nodes} nodes");
    assert!(
        committed > 1000 && refused > 1000 && nodes > 30,
        "the loop should exercise both paths"
    );
}

/// A mirror as a webview keeps one: rows by id, fed one snapshot and then only patches.
struct Mirror(BTreeMap<NodeId, Row>);

impl Mirror {
    fn of(t: &Tree) -> Mirror {
        Mirror(t.rows().into_iter().map(|r| (r.id, r)).collect())
    }

    fn apply(&mut self, t: &Tree, c: &Commit, seed: u64, step: usize) {
        let patch = t.patch(&c.changes);
        for id in &patch.removed {
            self.0.remove(id);
        }
        for r in patch.rows {
            self.0.insert(r.id, r);
        }
        let fresh = Mirror::of(t).0;
        if self.0 != fresh {
            let stale: Vec<String> = fresh
                .iter()
                .filter(|(id, r)| self.0.get(id) != Some(r))
                .map(|(_, r)| r.path.to_string())
                .chain(
                    self.0
                        .keys()
                        .filter(|id| !fresh.contains_key(id))
                        .map(|id| format!("{id} (should be gone)")),
                )
                .collect();
            panic!(
                "seed {seed}, step {step}: the mirror missed {stale:?} after {}",
                c.changes
            );
        }
    }
}

/// What a commit should say: the diff from the state before it.
fn rhizome_core_diff(before: &Snapshot, t: &Tree) -> Changeset {
    t.diff(before)
}
