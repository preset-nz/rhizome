---
title: POM — the Preset Object Model every app's object model is built on
type: design
status: draft
updated: 2026-10-02
---

# POM — Preset Object Model

**Draft, 2026-10-02.** The shared base for every app's object model, between [rhizome](node-api.md) and the apps. Named after Houdini's HOM. Lives in the rhizome repo as crates `rhizome-pom` and `rhizome-pom-tauri`, with a TypeScript half `@preset.nz/pom` (decision 36).

```
rhizome-core        mechanics: tree, values, refs, edits, undo, diff, file
   ↑
rhizome-pom         what every app's object model needs: documents, kinds and their
   ↑                policy, presets, commands, projections. Knows no app.
an app OM           one app's business logic, built on POM: its kinds, rules, verbs,
                    preset aggregates, projections. Lives in the app's repo.
```

**POM knows no app either** (decision 35). Its contract names no app and its tests use made-up object models. App names appear only under "Evidence", which is why each part exists.

**All apps need it** (Georg, 2026-10-02). POM is built now, not extracted after a second app adopts rhizome. Each part below names the apps that already carry a hand-made version.

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

Rust has no inheritance. The base class is a trait whose default methods do the work, plus a generic host that calls the app's hooks: the template-method pattern.

```rust
pub trait ObjectModel: Sized + 'static {
    const NAME: &'static str;                       // window title, logs
    const EXTENSION: &'static str;                  // file extension

    fn kinds(k: &mut Kinds<Self>);                  // node types + policy + inspector hints
    fn presets(p: &mut PresetKinds<Self>) {}        // default: none
    fn commands(c: &mut Commands<Self>) {}          // app commands; built-ins come free

    type Projection: Default;                        // a compiled plan, a render list, ()
    fn project(tree: &Tree, into: &mut Self::Projection, changes: Option<&Changeset>) {}

    fn migrate(file: &mut serde_json::Value, from: u64) -> Result<()> { Ok(()) }
}

pub struct Document<M: ObjectModel> { /* tree, store, projection, catalogues */ }
```

An app writes `impl ObjectModel for MyApp` with only its parts, and its domain verbs as an extension trait on rhizome's `Edit` (as in [`rhizome-api.md`](rhizome-api.md)). What it gets from `Document<MyApp>` without writing it:

- `new`, `open`, `save`, `save_as`, `revert`, `is_unsaved`, and a title such as "drone.shard — Edited";
- `edit`, `edit_ops`, gestures, coalesced edits, `undo` / `redo` with menu labels: rhizome's, passed through;
- re-projection after every commit, handed the `Changeset` so a projection can update incrementally;
- presets, commands and policy, below.

**Decision 3 still holds.** rhizome has no document object; POM adds one a layer up, where files, windows and menus live.

### Kinds

A **kind** is a rhizome node type plus what an app says about it beyond the schema. It is M&T's `LayerType`, generalised.

```rust
k.kind(NodeType::new("grid").float("grid.size", 4.0..=200.0, 24.0) /* … */)
    .policy(Policy { deletable: false, duplicable: false, max: Some(1), pinned: Some(Pin::Last("draw")) })
    .inspector(|i| i.group("Grid", ["grid.size", "grid.colour"]));
```

**Policy** (deletable, duplicable, max instances, pinned first or last in a named order, required) can't be enforced by wrapping verbs, because an app OM calls rhizome's `Edit` directly. POM compiles policy into **commit-time checks** instead: a policy breach refuses the whole edit, like any other check. That needs one rhizome change (see Forks).

**Inspector hints** become a facets `PropertySchema` (see "The facets bridge").

---

## Presets

A preset is **an aggregate of getters and setters**, defined by the app OM and run by POM (decision 37).

```rust
pub trait PresetKind<M: ObjectModel> {
    const ID: &'static str;                                   // "aspect", "palette", "node-values"
    type State: Serialize + DeserializeOwned + PartialEq + Clone;

    fn get(node: Node<'_>) -> Self::State;                    // read the aggregate
    fn set(tx: &mut Edit<'_>, node: NodeId, s: &Self::State) -> Result<Report>;   // write it
    fn matches(current: &Self::State, preset: &Self::State) -> bool { current == preset }
    fn applies_to(node: Node<'_>) -> bool { true }
}
```

The app writes the aggregate. POM supplies everything around it:

| POM machinery | What it does |
|---|---|
| **Catalogue** | Built-in presets in code, read-only: `p.catalogue::<Aspect>([("4x5", …), ("16x9", …)])` |
| **User presets** | Saved in the document. `save` refuses a taken name, `update` a missing one, `rename`, `delete`; `apply` is one edit, one undo step, and returns a `Report` of what it couldn't apply (Shard's rules) |
| **Current** | `current::<K>(node)`: which preset, if any, `matches` the node now. For "4x5" highlighted in a menu |
| **By reference, with cascade** | A node can follow a preset instead of copying it: `follow::<K>(node, preset)`. `resolve::<K>(node)` walks up the ancestors to the nearest follower, then the kind's default. Changing the preset changes every follower |
| **Generic `NodeValues` kind** | The common case, ready-made: a node's stored values, with keys to skip and whether bindings come along. An app customises it instead of writing one |

### Three worked examples

Written on paper first, to check that one abstraction holds all three.

**Shard: user presets of one node's sound.** `NodeValues` customised: skip `<node>.on`, include bindings (LFO links).

```rust
p.kind::<NodeValues>().skip(|key| key.ends_with(".on")).with_bindings();
```

Save, update and apply behave as `presets.rs` does today, including the report of ids that didn't apply. Nothing Shard-specific is left in POM.

**Oblique: document aspect.** A catalogue. `State` is `{ ratio, orientation }`, not the stored size: the getter derives it and the setter computes the size, keeping the long edge.

```rust
struct Aspect;
impl PresetKind<Oblique> for Aspect {
    const ID: &'static str = "aspect";
    type State = AspectState;                                  // { ratio, orientation }
    fn get(n: Node<'_>) -> AspectState { AspectState::of(n.get(SIZE).unwrap()) }
    fn set(tx: &mut Edit<'_>, n: NodeId, s: &AspectState) -> Result<Report> {
        let long = long_edge(tx.at(n).unwrap().get(SIZE).unwrap());
        tx.set(n, SIZE, s.size_for(long))?;
        Ok(Report::applied(1))
    }
    fn matches(now: &AspectState, p: &AspectState) -> bool { (now.ratio - p.ratio).abs() < 0.02 }
}
```

`current::<Aspect>(doc)` replaces `aspect.ts`'s "which preset best describes this size".

**M&T: setting palette by cascade.** A catalogue of nine settings, applied by reference. The campaign follows "doom-forge"; one map follows "hostile-waters"; a layer reads `resolve::<Palette>(layer)`, which finds the map's choice, else the campaign's, else the default. That is M&T's `useActiveSetting`, with undo and save for free. Getters return owned values, as M&T's `makePresetAccessors` returns clones.

All three fit. M&T needed "by reference, with cascade", which neither of the others asked for. That is the funky part.

### Where presets live

- **Catalogues** are code: never saved, never diffed.
- **User presets** are nodes in a POM-owned category, `presets`, of POM's type `preset`, with values `kind`, `for` (the node type) and `state`. So they undo, diff, save, copy and paste like anything else.
- **A follower** stores a reference to the preset it follows. For a user preset, a `Ref::here` to its node. For a catalogue entry, see Forks.

---

## Commands

POM implements the **commands-first contract** of [`plugin-primitive.md`](plugin-primitive.md), rather than a second registry. A command has an id, a label, an optional accelerator, an `enabled` test over capability tokens, and a `run` over `&mut Document<M>` that returns a `Commit` or nothing.

**Built in, so no app writes them:**
- File: new, open, save, save as, revert.
- Edit: undo and redo (labelled), cut, copy, paste, duplicate and delete. These respect policy: Delete is disabled on a kind that isn't deletable.
- Presets: save, update, apply, rename and delete for each preset kind.

App commands join the same registry. [`native-menu`](tauri-scaffold.md) builds the menu from it and asks it what's enabled; POM never touches a menu.

## Projections

`ObjectModel::project` is called after every commit, undo and redo, with the `Changeset` (or `None` after open, for a full rebuild). A projection is whatever the app's engine reads: Shard's plan for the audio thread, Oblique's cook list, M&T's render invalidation. POM decides *when* to project; the app decides *what*. Nothing a projection makes goes back into the tree (rhizome decision 24 as superseded).

## The facets bridge

Every app's inspector is a facets panel. The rhizome schema already has kinds, ranges, defaults, choices and declaration order. POM turns a kind's schema, plus its inspector hints (groups, labels, units, `disabledWhen`), into a facets `PropertySchema`, and turns a field edit into an `Op`. The dependency runs `@preset.nz/pom` → facets, never back. M&T's `registerPropertySchema` and its planned move to facets (epic 03) become this.

## The TypeScript half

`@preset.nz/pom`, generated types plus a small hand-written layer:

- a read-only **mirror** of the tree, kept current from `Commit`s;
- hooks: `useNode(path)`, `useValue(path, key)`, `useHistory()`, `usePresets(kind, node)`, `useCommand(id)`, `useGesture(label)`;
- one **transport** interface with two implementations: Tauri commands (`rhizome-pom-tauri`) and wasm (`rhizome-wasm`, decision 19's second consumer).

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

## Forks for Georg

1. **Rust OM or TypeScript OM for a web app.** M&T is Next.js with no Tauri today. Either its object model is Rust compiled to wasm, with one implementation of POM, or POM grows a TypeScript implementation, which is a second one. **Lean: Rust over wasm.** It is "never a second hand-written implementation" (node-api, Languages). M&T moves its layer registry and store behind the wasm transport, and its renderers stay TypeScript as projections. This decides whether "all need it" includes M&T now or after it gains a Rust side.
2. **Referencing a catalogue preset.** A `Ref` points at a node or a file, and a catalogue entry is neither. **Lean: a reserved file scheme,** `Ref::file("catalogue:palette/doom-forge")`: no new type, readable on disk, and unresolved (and reported) if a build drops the entry.
3. **Storage.** `Document` stores through a `Store` trait from day one. Single file is the only implementation now, and fits Shard, Oblique and Fault. M&T's planned zipped `.campaign` folder and Strata's DuckDB catalogue are later implementations. **Lean: build the trait, ship one store.**
4. **rhizome changes POM needs.** Each is mechanics, so it belongs in rhizome:
   - **Tree-level checks.** A per-node `check` can't say "required" or "at most one", because a kind with no instances is never checked. Add a registry-level check over the whole tree.
   - **Reserved names.** POM declares the `presets` category and the `preset` type. The registry should refuse an app's declaration of a name POM holds. Lean: POM registers first, so rhizome's existing duplicate check already refuses; no prefix needed.
   - **A structured value kind.** Preset `state` as a Text JSON blob diffs as one opaque line and isn't validated. **Lean: start with Text,** and add a `Value::Map` only if reading those diffs hurts.

## Phases

1. **`rhizome-pom`, headless.** `ObjectModel`, `Document<M>` with the single-file store, kinds and policy-as-checks, presets with all five machinery parts, built-in commands, projection calls. Tests: two made-up object models of different shapes, one Shard-like (node values, no files) and one M&T-like (cascade, catalogue, pinned and singleton kinds), plus an acid test per preset machinery part.
2. **`rhizome-pom-tauri`.** Commands and gestures as Tauri commands, `Commit` events, the opened-from-Finder hand-off (from Shard's `opened.rs`), and menu state for `native-menu`.
3. **`@preset.nz/pom` and `rhizome-wasm`.** Mirror, hooks, both transports, the facets bridge.
4. **More stores.** A bundle folder, then a database, when M&T and Strata need them.

## Related

- [`node-api.md`](node-api.md): the model; decisions 35 to 37.
- [`rhizome-api.md`](rhizome-api.md): the mechanics POM builds on.
- [`plugin-primitive.md`](plugin-primitive.md): commands and capabilities.
- [`tauri-scaffold.md`](tauri-scaffold.md): native-menu, preferences, window-restore.
- `projects/<app>/design/object-model.md`: each app's object model, now built on POM.
