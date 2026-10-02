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

- **Pushed up to `7d838f1`** (2026-10-02). Local commits after it (`0d54204`, `7da4514`, doc recopies) are not pushed; ask before pushing.
- **rhizome-core, first slice: shipped** (epic 01; `6f3513f`, `fe0a210`, `f7cf71b`, `33490ea`). Registry, tree, edits/gestures/coalescing, undo, diff, canonical JSON file, opaque nodes, copy/paste, `Op` as data, tree rules.
- **rhizome-pom, phase 1 (headless): shipped.** `ObjectModel` + `Document<M>`; kinds with policy; presets on the kind (catalogue, user presets as document nodes keyed by kind, apply, current, `add_from_preset`); built-in commands.
- **Themes removed from POM** (`0d54204`, decision 39). An app builds them from primitives; `tests/pom.rs` Atlas shows how (palette nodes in a `themes` category, `Ref::here`, app-side cascade). Workflow 03 retired; numbering kept.
- **Preset export/import** (`7da4514`, decision 45): export = the `preset` node as a fragment; import pastes into `/presets`, refuses unless one preset, known kind, state fits (`Erased::fits`), free name. Commands `preset.export` / `preset.import`. Workflow 07. Bindings in an imported `NodeValues` preset are skipped on apply (file-local ids; documented, tested).
- **Tests:** `just check` green. pom.rs 12, POM workflows 6 (01, 02, 04–07).
- **Decisions 39–45 made today** (`design/node-api.md`): 39 themes are the app's; 40 built-in catalogues come from a library rhizome (pulls cross-file `Ref` resolution forward); 41 app state is a category; 42 no value-level refs yet; 43 an op is a file/set of files (direction only); 44 shared signatures (SOP/COP/ROP-style families) are the app's; 45 presets stay in POM, both shapes, plus export/import.
- **Open conversation with Georg: presets / ops.** His picture of a node type: name, params, implementation, family signature, presets (valid param values), user presets stored against the type, export/import. Mostly matches what's built; the gaps are "implementation" and "family signature" (43, 44), both direction-only. Don't build either unasked.

## References

- `~/rhizomatic-preset/guidance/design/node-api.md` — the model, decisions 1–45; "The point of rhizome" quote after 42.
- `~/rhizomatic-preset/guidance/design/rhizome-api.md` — rhizome-core API as built.
- `~/rhizomatic-preset/guidance/design/pom.md` — POM as built; themes section now says "the app's".
- `~/rhizomatic-preset/guidance/design/plugin-primitive.md` — where "op as files" (43) will meet manifests.
- `~/rhizomatic-preset/guidance/projects/rhizome/README.md` — project index.
- `~/rhizomatic-preset/guidance/projects/shard/design/object-model.md` — the worked object model.
- `crates/rhizome-pom/src/{model,presets,document,command}.rs`; `crates/rhizome-core/src/{tree,edit,file,diff}.rs`.
- `packages/rhizome/guidance/` — **copies** of node-api, rhizome-api, pom, handover. Recopy after editing.

## Next steps

1. Ask Georg whether to push rhizome (`0d54204`, `7da4514` + recopies).
2. Continue the presets/ops conversation if he wants; record answers as decisions in `node-api.md`.
3. Cross-file `Ref` resolution (`Ref::node_in`, decision 23 → 40): needed before any app adopts a library catalogue. Needs a design pass with Georg: how a document opens a library rhizome, read-only or not, how ids resolve.
4. POM phase 2, `rhizome-pom-tauri`: commands and gestures as Tauri commands, `Commit` events, opened-from-Finder (lift Shard `src-tauri/src/opened.rs`), the command list for `native-menu`. Keep Tauri out of `rhizome-pom`.
5. Phase 3, `@preset.nz/pom`: mirror, hooks, Tauri transport, facets bridge.
6. First adoption is an app's epic (Shard's object model is the worked one). Path dependency on `packages/rhizome/crates/rhizome-pom`.
7. After any doc change: update `status`/`updated`, recopy into `packages/rhizome/guidance/`, commit both repos.

## Gotchas

- **Don't propose a rename for "theme"** or rebuild themes in POM; Georg parked naming and moved themes to apps.
- **Georg's vocabulary:** rhizome, node API, object model, POM, kind, category, group, smart group, loaded, calculated, preset, theme, op. Don't rename to industry terms; "scene" is dropped.
- **Nothing app-named in rhizome or POM** code, tests or docs (decision 35). Test models are made up (Synth, Atlas, Gazetteer); the workflow model is frozen.
- **`just bless` then read every line.** (POM workflows bless with `RHIZOME_BLESS=1 cargo test --test workflows`.)
- **Unique names are `name-2`,** not `name 2` (`Edit::unique_name`).
- **A paste of a non-preset into `/presets`** is refused by the core's category check before POM's own type check; POM's check still covers types with no category restriction.
- **cargo fmt rewraps lines,** so scripted string-replace edits after a format often miss. Re-read before patching.
- **Known gap:** "not deletable" finds the parent by its old path; rename-parent-and-remove-child in one edit slips past.
- Sequential ids in tests (`IdSource::sequential()`); goldens depend on them. `serde_json` must not get `preserve_order`.
