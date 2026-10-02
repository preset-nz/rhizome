---
title: POM — the Preset Object Model every app's object model is built on
type: design
status: current
updated: 2026-10-02
---

# POM — Preset Object Model

**Phase 1 shipped 2026-10-02:** `rhizome-pom`, headless, in `packages/rhizome/crates/rhizome-pom`. This doc describes the code as built. POM is the shared base for every app's object model, between [rhizome](node-api.md) and the apps, named after Houdini's HOM (decision 36). It lives in the rhizome repo; `rhizome-pom-tauri` and `@preset.nz/pom` are phases 2 and 3.

```
rhizome-core        mechanics: tree, values, refs, edits, undo, diff, file
   ↑
rhizome-pom         what every app's object model needs: documents, kinds and their
   ↑                policy, presets, commands, projections. Knows no app.
an app OM           one app's business logic, built on POM: its kinds, rules, verbs,
                    preset aggregates, projections. Lives in the app's repo.
```

**POM knows no app either** (decision 35). Its code names no app and its tests use made-up object models. App names appear only under "Evidence", which is why each part exists.

**All apps need it** (Georg, 2026-10-02). Every app is a Rust/Tauri app with its object model in `src-tauri`, Map & Territory included (its README, decisions 7 and 19). POM is one Rust implementation.

---

## Evidence

| POM part | Who has a hand-made one today |
|---|---|
| Document: open, save, unsaved, file opened from Finder | Shard `src-tauri/src/{patch,opened}.rs`; Oblique `src/scene/store.ts`; M&T `src/stores/campaign/persistence.ts` (versioned envelope, browser import/export) |
| History with menu labels | Shard `src-tauri/src/history.rs`; Oblique `src/scene/history.ts`; M&T planned, epic 11 (snapshots, cap 50, transactions: rhizome's shape) |
| Kinds with defaults, policy and inspector schema | M&T `src/layers/types.ts` (`LayerType`, `LayerPolicy`), `src/layers/registry.ts`, `src/properties/registry.ts`; Shard's `PARAMS` table |
| Presets | Shard `src-tauri/src/presets.rs` (user presets of one node); Oblique `src/panels/document/aspect.ts` (catalogue, computed setter, match); M&T `src/palettes/presets.ts` (catalogue, getters, cascade map → campaign → default); Fault `src/lib/paperSize.ts`, a hand-copy of Oblique's aspect presets, already drifting by the comment at its top |
| Commands with enabled state | M&T `src/lib/commands.ts` + capability tokens; every app's `menu.rs` |
| Projections | Shard's compiled plan; Oblique's cooks; M&T `getInvalidationKey` |

---

## The Rust "base class"

Rust has no inheritance. The base class is a trait whose default methods do the work, plus a generic host that calls the app's hooks.

```rust
pub trait ObjectModel: Sized + 'static {
    const NAME: &'static str;                        // window title, logs
    const EXTENSION: &'static str;                   // file extension, no dot
    type Projection: Default + Send;                 // a compiled plan, a render list, ()

    fn kinds(k: &mut Kinds);                         // categories and kinds with policy
    fn presets(_p: &mut Presets) {}                  // preset kinds; none by default
    fn commands(_c: &mut Commands<Self>) {}          // app commands, next to the built-ins
    fn project(_tree: &Tree, _into: &mut Self::Projection, _changes: Option<&Changeset>) {}
}
```

An app writes `impl ObjectModel for MyApp` with only its parts, and its domain verbs as an extension trait on rhizome's `Edit` (as in [`rhizome-api.md`](rhizome-api.md)). From `Document<MyApp>` it gets, without writing them:

- **Files:** `new`, `open` (with a `LoadReport`), `save`, `save_as` (adds the extension), `revert`, `is_unsaved`, `title()` ("drone.shard — Edited"). Text goes through a `Store`: `FileStore` writes atomically (a temporary file, then a rename); `MemoryStore` is for tests.
- **Edits:** `edit`, `edit_ops`, `edit_coalesced`, `begin` / `apply` / `within` / `end` / `cancel`, `undo` / `redo`: rhizome's, passed through.
- **Projection:** `M::project` runs after open (with `None`, a full rebuild) and after every commit, undo, redo and cancel (with the `Changeset`). `projection()` reads it.
- **`paste` and `duplicate`** that respect policy (below).
- Presets, commands and policy, below.

**Decision 3 still holds.** rhizome has no document object; POM adds one a layer up, where files, windows and menus live.

**Not yet:** a `migrate` hook (no files people keep yet); inspector hints and the facets bridge (phase 3); `required` kinds.

### Kinds and policy

A **kind** is a rhizome node type plus what an app says about it beyond the schema. It is M&T's `LayerType`, generalised.

```rust
fn kinds(k: &mut Kinds) {
    k.category("campaigns", Origin::Loaded);
    k.kind(NodeType::new("grid").in_categories(&["campaigns"]))
        .not_deletable().not_duplicable().max_per_parent(1).pinned_last("draw");
}
```

**Policy** is `not_deletable` (leaves only with its parent), `not_duplicable` (Duplicate is never offered), `max_per_parent(n)`, and `pinned_first` / `pinned_last` in a named order of the parent. An app OM calls rhizome's `Edit` directly, so POM can't enforce policy by wrapping verbs. It compiles every policy into **one rhizome tree rule**, run at commit with the tree and the changeset: a breach refuses the whole edit like any other check. Counts are checked before pins, so a copied grid reports "at most 1 grid here", not where the copy landed.

- **Generic verbs re-pin.** rhizome's paste appends a new node to every order of its parent, which would put a pasted layer above the grid. `Document::paste` and `duplicate` (and the built-in commands) move pinned kinds back to their ends inside the same edit. A raw `set_order` that misplaces a pin is still refused.
- **Open reports breaches.** Load runs no rules, so `open` and `revert` add every policy breach to the `LoadReport` ("policy: must stay last in `draw`") rather than refusing the file.
- **Known gap.** "Leaves only with its parent" looks the parent up by its old path. Renaming the parent and removing the child in one edit gets past it. Acceptable until a real workflow does that.

---

## Presets

A preset is **an aggregate of getters and setters**, defined by the app OM and run by POM (decision 37).

```rust
pub trait Aggregate: Send + Sync + 'static {
    type State: Serialize + DeserializeOwned + PartialEq + Clone;
    fn get(&self, node: Node<'_>) -> Self::State;
    fn set(&self, tx: &mut Edit<'_>, node: NodeId, state: &Self::State) -> rhizome_core::Result<Report>;
    fn matches(&self, current: &Self::State, preset: &Self::State) -> bool { current == preset }
    fn applies_to(&self, _node: Node<'_>) -> bool { true }
}

fn presets(p: &mut Presets) {
    p.kind("palette", PaletteKind)                // registered by id
        .catalogue([("doom-forge", …), ("space-opera", …)])
        .fallback("doom-forge")
        .followed_by(&["campaign", "map"]);
}
```

The app writes the aggregate. POM supplies everything around it, as methods on `Document`:

| Machinery | `Document` method | What it does |
|---|---|---|
| **Catalogue** | — | Built-in presets in code, read-only, never saved |
| **Names** | `preset_names(kind, node)` | Catalogue first, then the user's presets for this node's type; empty where `applies_to` says no |
| **User presets** | `save_preset`, `update_preset`, `rename_preset`, `delete_preset` | Saved in the document. Save refuses a taken name, update a missing one; rename refuses a taken one; a name is 1 to 60 characters. Each is one edit, one undo step |
| **Apply** | `apply_preset(kind, node, &PresetRef)` | One edit, one undo step; returns a `Report` of what was applied and skipped |
| **Current** | `current_preset(kind, node)` | The first preset whose state `matches` the node now |
| **Follow** | `follow_preset(kind, node, Some(&r))` / `None` | A node follows a preset by reference instead of copying it |
| **Resolve** | `resolve_preset(kind, node)` | The node's own followed preset, else the nearest ancestor's, else the kind's fallback. Returns `Resolved { follower, preset, state }` |
| **`NodeValues`** | — | The ready-made aggregate: every value in a node's schema (resolved, so `current` works), optionally its bindings. Customise with `.skip(pred)` and `.with_bindings()` |

`PresetRef` is `Catalogue(name)` or `User(label)`, and is `{"catalogue": "…"}` / `{"user": "…"}` in JSON.

### Three worked examples

Checked on paper first, then in the tests' made-up models of the same shapes.

**Shard: user presets of one node's sound.** `p.kind("sound", NodeValues::new().skip(|k| k.ends_with(".on")).with_bindings())`. Save, update and apply behave as `presets.rs` does today, including the report. Nothing Shard-specific is left in POM.

**Oblique: document aspect.** A catalogue whose state is derived: `get` reads `{ ratio }` off the size, `set` computes the size keeping the long edge, `matches` allows 0.02, `applies_to` is the document node only. `current_preset` replaces `aspect.ts`'s "which preset best describes this size", and Fault's copy goes away.

**M&T: palette by cascade.** A catalogue of nine settings with a fallback, followed by campaign and map. A layer calls `resolve_preset("palette", layer)`: the map's choice, else the campaign's, else the fallback. That is M&T's `useActiveSetting`, with undo and save for free. Its aggregate's `set` refuses, because palettes are chosen, never written onto a node.

### Where presets live

- **Catalogues** are code.
- **User presets** are nodes in POM's category `presets`, of POM's type `preset`, with Text values `preset.kind`, `preset.for` (the node type it was saved from), `preset.label` and `preset.state` (the state as JSON). They undo, diff, save, copy and paste like anything else. An app can't declare a category or type with those names: POM registers first, and rhizome refuses duplicates.
- **A follower** gets a reference key `follow.<kind>` on each `followed_by` type. A user preset is followed by `Ref::here` to its node, so renaming it keeps the follower. A catalogue entry is followed by `Ref::file("catalogue:<kind>/<name>")`: no new type, readable on disk, unresolved and skipped if a build drops the entry.
- **Deleting a user preset unfollows** everything that followed it, in the same edit, so nothing points at a preset that's gone. Undo restores both.

---

## Commands

POM implements the **commands-first contract** of [`plugin-primitive.md`](plugin-primitive.md). A command has an id, a label (a function of the document, so Undo can say what it undoes), `enabled` for a JSON payload, and `run`, which returns an `Outcome`: `Nothing`, `Committed(Commit)` or `Text` (a copied fragment). `Document::run(id, payload)` refuses a disabled command; `commands(payload)` lists every id with its label and state, for `native-menu`.

**Built in, so no app writes them:**

| Id | Payload | Notes |
|---|---|---|
| `file.save`, `file.revert` | — | Revert is enabled only for a saved, changed document |
| `edit.undo`, `edit.redo` | — | Labelled "Undo Set Opacity"; disabled during a gesture |
| `edit.delete`, `edit.duplicate` | `{ at }` | Disabled by policy, and on the root, categories and opaque nodes |
| `edit.copy` | `{ at }` | Returns the fragment as `Text` |
| `edit.paste` | `{ parent, fragment }` | Re-pins |
| `preset.apply`, `.save`, `.update`, `.rename`, `.delete`, `.follow` | `{ kind, at, preset?, label?, to? }` | The `Document` methods above |

An app adds its own with `c.add(id, label, enabled, run)`; the same id replaces a built-in. New and Open aren't document commands: they make a `Document`. Capability tokens arrive with the TypeScript half; `enabled` is a closure for now.

## Projections

`ObjectModel::project` decides *what*; POM decides *when*: after open, and after every commit, undo, redo and cancel. A projection is whatever the app's engine reads: Shard's plan for the audio thread, Oblique's cook list, M&T's render list. Nothing a projection makes goes back into the tree.

## The facets bridge (phase 3)

Every app's inspector is a facets panel. A kind's schema (kinds, ranges, defaults, choices, declaration order) plus inspector hints (groups, labels, units, `disabledWhen`) becomes a facets `PropertySchema`, and a field edit becomes an `Op`. The dependency runs `@preset.nz/pom` → facets, never back.

## The TypeScript half (phase 3)

`@preset.nz/pom`: a read-only mirror of the tree kept current from `Commit`s; hooks such as `useNode`, `useValue`, `useHistory`, `usePresets`, `useCommand` and `useGesture`; one transport interface, implemented over Tauri commands. Every app is a Tauri app, so wasm still waits for a consumer (decision 19).

---

## What POM is not

| Not POM's | Whose |
|---|---|
| The plugin system (manifests, activation, compute plugins) | [`plugin-primitive.md`](plugin-primitive.md). POM provides commands and kinds that plugins contribute to |
| Building the native menu | `native-menu` ([`tauri-scaffold.md`](tauri-scaffold.md)) |
| Preferences and Settings | `preferences` |
| Window size and position | `window-restore` |
| Selection, hover, tool state | The app ([`interaction-state.md`](interaction-state.md)): not tree state |
| Mechanics: values, refs, diff, undo, the file format | rhizome |
| Anything one app means | That app's object model |

---

## Decided (Georg, 2026-10-02)

1. **Every object model is Rust on POM.** Map & Territory becomes a Rust/Tauri app like the others. One implementation.
2. **A catalogue preset is referenced as `catalogue:<kind>/<name>`,** a reserved file scheme in a `Ref`.
3. **Storage is a `Store` trait;** single files are the only store now. A bundle folder (M&T's `.campaign`) and a database (Strata) come when those apps adopt.
4. **The rhizome changes POM needed are in:** tree rules (`RegistryBuilder::rule`, with `Violation`), `ChangeKind::Removed { type_name }`, and public `Value::to_json` / `from_json`. Reserved names need no change: POM registers first. Preset state is a Text JSON blob until reading its diffs hurts.

## Tests

`just pom`, and in `just check`. Both suites use made-up object models, never an app's:

- **`tests/pom.rs`** (12 tests). *Synth*: user presets of one node's sound skipping its switch, with bindings; the document lifecycle and projection. *Atlas*: anchored singleton layers, a palette by cascade, a computed aspect preset, a user preset followed by id, the commands, a layer pasted into another map, policy breaches on open, deleting a followed preset, and models that can't be built.
- **`tests/workflows.rs`** (6 workflows as data in `tests/workflows/*.json`, transcripts pinned beside them): driven only through commands by id with JSON payloads, `Op` JSON, files and preset reads, against a frozen model of its own. A change to POM that alters a workflow fails here.

## Phases

1. **`rhizome-pom`, headless.** Shipped 2026-10-02.
2. **`rhizome-pom-tauri`.** Commands and gestures as Tauri commands, `Commit` events, the opened-from-Finder hand-off (from Shard's `opened.rs`), and the command list for `native-menu`.
3. **`@preset.nz/pom`.** Mirror, hooks, the Tauri transport, the facets bridge with inspector hints.
4. **More stores.** A bundle folder, then a database, when M&T and Strata need them.

## Related

- [`node-api.md`](node-api.md): the model; decisions 35 to 37.
- [`rhizome-api.md`](rhizome-api.md): the mechanics POM builds on.
- [`plugin-primitive.md`](plugin-primitive.md): commands and capabilities.
- [`tauri-scaffold.md`](tauri-scaffold.md): native-menu, preferences, window-restore.
- `projects/<app>/design/object-model.md`: each app's object model, built on POM.
