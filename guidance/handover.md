---
title: Handover — rhizome
type: handover
status: current
repo: /Users/georg/rhizomatic-preset/packages/rhizome
branch: main
project: rhizome
topic: rhizome-core (mechanics) and rhizome-pom (POM, the Preset Object Model) — both built, nothing consumed by an app yet.
updated: 2026-10-02
---

# Handover — rhizome

## Current status

- **Pushed 2026-10-02:** rhizome `main` at `7d838f1` on private `preset-nz/rhizome`; guidance up to date.
- **rhizome-core, first slice: shipped** (epic 01; `6f3513f`, `fe0a210`, `f7cf71b`). Registry, tree, edits/gestures/coalescing, undo, diff, canonical JSON file, opaque nodes, copy/paste, `Op` as data. Then `33490ea`: tree rules (`RegistryBuilder::rule` + `Violation`), `ChangeKind::Removed { type_name }`, public `Value::to_json`/`from_json`.
- **rhizome-pom, phase 1 (headless): shipped** (`68adf5f`, `5a75639`, `9a5d40c`, `9851880`). `ObjectModel` trait + `Document<M>`; kinds with policy compiled into one tree rule; presets **on the kind** (`k.kind(t).presets(agg).catalogue([...])`), user presets as document nodes keyed by kind; `add_from_preset`; **themes** separate (catalogue, fallback, `followed_by`, cascade resolve); built-in commands (file, edit, `node.add`, `preset.*`, `theme.follow`).
- **Tests:** `just check` green, 45-ish tests. core: acid, core_workflows (13), invariants (seeded random Ops). pom: `tests/pom.rs` (11), `tests/workflows.rs` (6 data workflows through commands).
- **Decisions 35–38 made today** (all in `design/node-api.md`): 35 rhizome knows no app; 36 POM; 37 presets in POM as getter/setter aggregates; 38 presets belong to the kind, themes are separate, user presets travel with the file.
- **App-specific planning moved out** of rhizome docs into `projects/<app>/design/object-model.md` (Shard in full; stubs for Oblique, Fault, Strata, M&T). M&T becomes Rust/Tauri, object model on POM (its README decision 19).
- **Themes, open (2026-10-02):** themes are generic (Fault hatch style, Oblique colour sets, Shard key/tempo/swing). The naming ("theme" vs something else) is **parked: Georg wants to talk it through**. Don't propose renames unprompted; wait for him to raise it. **User themes: later** (Georg's call), not before phase 2.

## References

- `~/rhizomatic-preset/guidance/design/node-api.md` — the model, decisions 1–38.
- `~/rhizomatic-preset/guidance/design/rhizome-api.md` — rhizome-core API as built, guarantees ↔ tests.
- `~/rhizomatic-preset/guidance/design/pom.md` — POM as built: base class, policy, presets, themes, commands, phases, known gaps.
- `~/rhizomatic-preset/guidance/projects/rhizome/README.md` — project index; epic 01 shipped, POM epic 2.
- `~/rhizomatic-preset/guidance/projects/shard/design/object-model.md` — the worked object model (roles, presets, audio-thread plan).
- `crates/rhizome-pom/src/{model,presets,themes,document,command}.rs` — POM; `crates/rhizome-core/src/{tree,edit,file,diff}.rs` — mechanics.
- `crates/*/tests/workflows/*.json` + `*.txt` — data workflows and pinned transcripts.
- `packages/rhizome/guidance/` — **copies** of the docs for remote work; canonical lives in the guidance repo. Recopy after editing.

## Next steps

1. POM phase 2, `rhizome-pom-tauri`: commands and gestures as Tauri commands, `Commit` events, opened-from-Finder (lift Shard `src-tauri/src/opened.rs`), the command list for `native-menu`. Keep Tauri out of `rhizome-pom`.
2. Phase 3, `@preset.nz/pom`: mirror, hooks, Tauri transport, facets bridge (needs inspector hints on kinds). Wasm still waits for a consumer.
3. First adoption is an app's epic, not rhizome's (Shard's object model is the worked one). Path dependency on `packages/rhizome/crates/rhizome-pom`.
4. Later, when Georg says so: user themes. `themes.rs` gains save/rename/delete as nodes in POM's `presets` category (or a `themes` category), followed by `Ref::here`; delete unfollows (the old `presets.rs` had this, see `5a75639`). Add to `tests/pom.rs` and a workflow; `just bless`, read every changed line.
5. After any doc change: update `status`/`updated`, recopy into `packages/rhizome/guidance/`, commit both repos.

## Gotchas

- **Georg's vocabulary:** rhizome, node API, object model, POM, kind, category, group, smart group, loaded, calculated, preset, theme. Don't rename to industry terms; "scene" is dropped.
- **Nothing app-named in rhizome or POM** code, tests or docs (decision 35). Test models are made up (Synth, Atlas, Gazetteer); the workflow models are frozen, so changing one changes every transcript.
- **`just bless` then read every line.** Blessed goldens caught real bugs this session (an elided-fragment label garbling every step).
- **cargo fmt rewraps lines,** so scripted string-replace edits after a format often miss. Re-read before patching.
- **Generic paste/duplicate append to every parent order;** POM re-pins pinned kinds (`model.rs` `repin`). A generic `node.add` doesn't place a node in any order: apps override `node.add` with their domain verb.
- **Known gap:** "not deletable" finds the parent by its old path; rename-parent-and-remove-child in one edit slips past.
- Sequential ids in tests (`IdSource::sequential()`); goldens depend on them.
- `serde_json` must not get `preserve_order`: canonical output relies on sorted keys.
