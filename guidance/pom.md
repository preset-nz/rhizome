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

    fn kinds(k: &mut Kinds);                         // categories, kinds, policy, presets
    fn themes(_t: &mut Themes) {}                    // shared choices; none by default
    fn commands(_c: &mut Commands<Self>) {}          // app commands, next to the built-ins
    fn project(_tree: &Tree, _into: &mut Self::Projection, _changes: Option<&Changeset>) {}
}
```

An app writes `impl ObjectModel for MyApp` with only its parts, and its domain verbs as an extension trait on rhizome's `Edit` (as in [`rhizome-api.md`](rhizome-api.md)). From `Document<MyApp>` it gets, without writing them:

- **Files:** `new`, `open` (with a `LoadReport`), `save`, `save_as` (adds the extension), `revert`, `is_unsaved`, `title()` ("drone.shard — Edited"). Text goes through a `Store`: `FileStore` writes atomically (a temporary file, then a rename); `MemoryStore` is for tests.
- **Edits:** `edit`, `edit_ops`, `edit_coalesced`, `begin` / `apply` / `within` / `end` / `cancel`, `undo` / `redo`: rhizome's, passed through.
- **Projection:** `M::project` runs after open (with `None`, a full rebuild) and after every commit, undo, redo and cancel (with the `Changeset`). `projection()` reads it.
- **`paste` and `duplicate`** that respect policy (below).
- Presets, themes, commands and policy, below.

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

## Presets: on the kind

A preset is **a way to fill in a kind's template**, so it is declared on the kind (decision 38). It is an aggregate of getters and setters that the app OM defines (decision 37).

```rust
pub trait Aggregate: Send + Sync + 'static {
    type State: Serialize + DeserializeOwned + PartialEq + Clone;
    fn get(&self, node: Node<'_>) -> Self::State;
    fn set(&self, tx: &mut Edit<'_>, node: NodeId, state: &Self::State) -> rhizome_core::Result<Report>;
    fn matches(&self, current: &Self::State, preset: &Self::State) -> bool { current == preset }
}

fn kinds(k: &mut Kinds) {
    k.kind(NodeType::new("map") /* … */)
        .presets(AspectKind)                         // last in the chain
        .catalogue([("4x3", …), ("16x9", …)]);
    let sound = NodeValues::new().skip(|k| k.ends_with(".on")).with_bindings();
    k.kind(granular).presets(sound.clone());       // one aggregate, shared by cloning
    k.kind(crush).presets(sound);
}
```

The app writes the aggregate. POM supplies everything around it, as methods on `Document` that take the node:

| Machinery | `Document` method | What it does |
|---|---|---|
| **Catalogue** | — | The kind's built-in presets, in code, never saved |
| **Names** | `preset_names(node)` | The kind's catalogue, then the user's presets for that kind. Empty for a kind without presets |
| **User presets** | `save_preset`, `update_preset`, `rename_preset`, `delete_preset` | Document data, keyed by kind, so they **travel with the file** and every node of the kind sees them. Save refuses a taken name, update a missing one, rename a taken one; 1 to 60 characters. One edit, one undo step each |
| **Apply** | `apply_preset(node, &PresetRef)` | One edit, one undo step; returns a `Report` of what was applied and skipped |
| **Make from** | `add_from_preset(parent, kind, name, &PresetRef)` | Instantiates the template filled from a preset: add and apply, one edit, one undo step |
| **Current** | `current_preset(node)` | The first preset, built-in then user, whose state `matches` the node now |
| **`NodeValues`** | — | The ready-made aggregate: every value in the schema (resolved, so `current` works), optionally the bindings. Customise with `.skip(pred)` and `.with_bindings()` |

`PresetRef` is `Catalogue(name)` or `User(label)`; in JSON `{"catalogue": "…"}` / `{"user": "…"}`. A preset is copied into a node, never followed.

### Worked examples

**Shard: a section's sound.** Each effect kind gets `NodeValues` skipping `<node>.on`, with bindings, shared by cloning. Save, update and apply behave as `presets.rs` does today, including the report.

**Oblique: document aspect.** The document kind's aggregate derives `{ ratio }` from the size, sets the size keeping the long edge, and matches within 0.02. `current_preset` replaces `aspect.ts`'s "which preset best describes this size", and Fault's copy goes away.

**Making from a preset.** `node.add` with `{"preset": {"catalogue": "rocky"}}` makes a layer already filled in. An app whose kinds need more than a bare add (a map with its anchors) registers its own `node.add`.

### Where user presets live

Nodes in POM's category `presets`, of POM's type `preset`, with Text values `preset.for` (the kind), `preset.label` and `preset.state` (the state as JSON). They undo, diff, save, copy and paste like anything else, and go wherever the file goes. A pasted node doesn't bring its file's user presets into another file. An app can't declare the names `presets` or `preset`: POM registers first, and rhizome refuses duplicates.

---

## Themes: shared choices, followed by cascade

A **theme** is a shared choice that nodes look up through their ancestors, such as a palette chosen for a campaign and overridden for one map. It is never written onto a node, so it isn't a preset (decision 38).

```rust
fn themes(t: &mut Themes) {
    t.theme("palette")
        .catalogue([("doom-forge", palette(…)), ("space-opera", palette(…))])
        .fallback("doom-forge")
        .followed_by(&["campaign", "map"]);
}
```

- `follow_theme(kind, node, Some(name))` / `None` makes a node follow a theme or stop. Each `followed_by` kind gets a reference key `theme.<kind>`, and the choice is stored as `Ref::file("theme:<kind>/<name>")`: no new value type, readable on disk, skipped if a build drops the entry.
- `resolve_theme(kind, node)` returns the node's own choice, else the nearest ancestor's, else the fallback, as `ResolvedTheme { follower, name, state }`.
- `theme_names(kind)` lists the catalogue.
- Built-in themes only for now. User themes come when an app needs them; M&T's Strata palette import (its epic 08) is the likely first.

**M&T: palette.** Nine settings with a fallback, followed by campaign and map. A layer calls `resolve_theme("palette", layer)`. That is M&T's `useActiveSetting`, with undo and save for free.

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
| `node.add` | `{ parent, type, name, preset? }` | With a preset, made from it |
| `preset.apply`, `.save`, `.update`, `.rename`, `.delete` | `{ at, preset?, label?, to? }` | Enabled on kinds with presets |
| `theme.follow` | `{ kind, at, theme? }` | No `theme` stops following |

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
2. **A followed theme is referenced as `theme:<kind>/<name>`,** a reserved file scheme in a `Ref`. (Was `catalogue:` while themes were presets.)
5. **Presets are on the kind; themes are separate** (decision 38). User presets travel with the file.
3. **Storage is a `Store` trait;** single files are the only store now. A bundle folder (M&T's `.campaign`) and a database (Strata) come when those apps adopt.
4. **The rhizome changes POM needed are in:** tree rules (`RegistryBuilder::rule`, with `Violation`), `ChangeKind::Removed { type_name }`, and public `Value::to_json` / `from_json`. Reserved names need no change: POM registers first. Preset state is a Text JSON blob until reading its diffs hurts.

## Tests

`just pom`, and in `just check`. Both suites use made-up object models, never an app's:

- **`tests/pom.rs`** (11 tests). *Synth*: user presets of one kind's sound skipping its switch, with bindings; the document lifecycle and projection. *Atlas*: anchored singleton layers, a palette theme by cascade, a computed aspect preset on the map kind, a node made from a preset, the commands, a layer pasted into another map, policy breaches on open, and models that can't be built.
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
