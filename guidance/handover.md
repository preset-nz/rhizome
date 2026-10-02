---
title: Handover — rhizome
type: handover
status: current
repo: /Users/georg/rhizomatic-preset/packages/rhizome
branch: main
project: rhizome
topic: rhizome-core first slice shipped; rhizome stays unaware of the apps (decision 35).
updated: 2026-10-02
---

# Handover — rhizome

Continues `handovers/shard/handover-node-api.md`, which covered the planning. This file covers the code.

## Current status

- **Repo:** `packages/rhizome`, private remote `preset-nz/rhizome`. One crate, `crates/rhizome-core`. Not consumed by any app yet.
- **First slice shipped** (epic 01): registry, tree, read views, `Edit` with every write verb, `Op` as data, scoped edits, gestures, coalesced edits, snapshot undo (cap 50), diff, canonical JSON file format with a load report, opaque nodes, extract/paste/copy.
- **Design:** `design/rhizome-api.md` describes the shipped API (status current). `design/node-api.md` holds decisions 1 to 34; 29 to 34 were made 2026-10-02.
- **Tests:** `just check`. Acid (`tests/acid.rs`), core workflows (`tests/core_workflows.rs` + `tests/workflows/*.json` with `*.txt` transcripts), invariants (`tests/invariants.rs`). `just bless` rewrites goldens; read every changed line.
- **Copies:** `packages/rhizome/guidance/` holds copies of the two design docs and this handover's predecessor. Canonical versions are here.
- **Not pushed** as of 2026-10-02 unless Georg said so after this was written; check `git status -sb`.

## Next steps

1. **rhizome knows no app** (decision 35, 2026-10-02). App-specific planning moved to `projects/<app>/design/object-model.md`. Adoption is each app's epic, not rhizome's; Shard's object model is the worked one and the likely first. Work on rhizome only when an object model needs a mechanism it lacks.
2. **POM** (decisions 36, 37; `design/pom.md`, draft): waiting on Georg's answers to its four forks (Rust or TS object model for M&T, catalogue refs, storage, rhizome changes). Then phase 1, `rhizome-pom` headless.
3. **Root file value:** `node-api.md` has the root carry its file path as a value; the first slice doesn't. Add when a consumer needs it.
4. **Reserved, no API yet:** `find`/search, smart groups, calculated results and staleness, time.
5. Wasm and the TypeScript package come with POM phase 3 (M&T is the second consumer decision 19 waited for).

## Gotchas

- Sequential ids (`IdSource::sequential()`) are for tests; apps use ULIDs. Goldens depend on sequential ids, so adding a category to a test registry shifts every id in its goldens.
- The workflow registry in `tests/core_workflows.rs` is frozen on purpose. Change it and every transcript changes.
- `serde_json` must not get the `preserve_order` feature; canonical output relies on sorted keys inside values.
- Georg's vocabulary: rhizome, node API, object model, category, group, smart group, loaded, calculated, kind.
