---
title: Rhizome — the node API and object models the desktop suite shares
type: design
status: draft
updated: 2026-10-02
---

# Rhizome — node API and object models

**Draft plan, from Georg's sketch and answers of 2026-09-13.** Nothing is scaffolded. Where this doc still picks between options it says **lean** and names what would settle it. The "…" at the end of Georg's sketch is **aspirational**: room for the model to grow, not an open question to close (Georg, 2026-09-13). Don't fill it in on his behalf.

Formerly `scene-model.md`. **"Scene" is dropped.** What is shared is an **API** over a tree of nodes, and each app adds an **object model** on top.

> "I think of the apps in the suite like modes of houdini, just separate apps." — Georg, 2026-09-13

---

## The picture: Houdini contexts as separate apps

Houdini's contexts share one node model and one API (`hou`), and each context brings its own node types. The suite is the same idea, split into apps:

| Houdini context | Suite app | Its node types |
|---|---|---|
| SOPs (geometry) | Fault | Meshes, modifiers, scatters |
| COPs (compositing) | Oblique | Images, vector nodes, modifiers, masks |
| CHOPs (channels, audio) | Shard | Material, patches, effects, envelopes |
| TOPs / PDG (tasks producing cached results) | Strata | Images, plugin artifacts, collections |
| — (a 2D hex canvas; no direct Houdini analogue) | Map & Territory | Campaigns, maps, layers, cells. Adopts Rhizome when available (decided 2026-09-17, [`../projects/map-and-territory/README.md`](../projects/map-and-territory/README.md) decision 17) |

**rhizome is unaware of the apps** (decision 35). It deals in mechanics only: paths, node types it is told about, values, references, groups, bindings, orders, edits, undo, diff, the file format. Each app's **object model** holds its business logic and lives in the app's repo; its design lives in `projects/<app>/design/object-model.md`. The table above is why the split exists, not something rhizome knows.

So there are two levels:

- **The node API** is generic, shared, and one implementation. It knows paths, nodes, categories, groups, references, values, bindings, order, search, diff and dirty. It never knows what a patch or a mesh is.
- **An object model** belongs to each app and lives in its repo. It holds the app's node types and domain rules, uses only the API, and is the only place domain logic lives. That is what stops one app's change forcing a refactor in the others.

The object-model name is Georg's choice, after Houdini's HOM. In Houdini, HOM also covers the generic API. Here the API and the object model are deliberately split.

---

## Decided (Georg, 2026-09-13)

1. **A shared module, planned, not scaffolded.** *"I want a module now. don't want to refactor all apps constantly."* / *"don't scaffold, just plan."*
2. **No "scene".** An API and object models.
3. **The root is just a path.** `api.node("/")`, as in Houdini. No named document or session object. A file is a tree rooted at `/`.
4. **Each app keeps its own files, and files can reference each other.** `.shard`, `.oblique` and so on stay separate. A node can reference a node in another app's file, such as an Oblique image using a Shard render.
5. **Loaded vs calculated.** This replaces "intermediate".
6. **Time is optional.** Some trees carry it (Shard's arrangement).
7. **Order is stored data**, never implied by a list index or the hierarchy.
8. **Files serialise as references and values.** Nothing calculated is saved, except as a rebuildable cache.
9. **Groups, not layers.** Groups have no order.
10. **Constructable in code.**
11. **Search is first-class.** *"important is we have ways to search the scene for things."*
12. **Rust implementation. Python not now.**
13. **Category and group keep those names.** They are generic enough.
14. **Settings files are TOML.** See [`native-apps.md`](native-apps.md) rule 5.
15. **First consumer: Shard's multi-patch interface.** Since decision 35, rhizome has no consumer it plans for; adopting is each app's epic.
16. **No in-app scripting yet.** Rust object-model code and CLIs only, until a real script need shows up.
17. **Smart-group queries are fixed predicates.** No query language to start.
18. **Cross-file index: decided later.** Search follows references from the open file only.
19. **The shared module is `rhizome`.** Georg's own name, not `nodes`: a network of nodes with no fixed centre. Repo `packages/rhizome`, crates `rhizome-core` and `rhizome-wasm`, npm package for the generated TypeScript.

**From the review of 2026-09-13.** The apps are growing in parallel and this was the first attempt to consolidate their architecture. Decided the same day, after the review's questions:

20. **Apps define their own node types.** The core holds a **node-type registry** that each object model fills with a name, a value schema and an origin. The core stores and validates nodes of those types; it never knows what a patch is. There is no node enum in the core.
21. **Nodes have a stable `NodeId`** alongside their path, serialised in the file. Paths are addresses; ids are identity. A rename is a move, not a delete plus an add.
22. **Category is the top-level partition.** A node's category is that of its top-level ancestor; nested children inherit it. Shard's panel columns (roadmap row 6) are *kinds* within a patch, not categories.
23. **Multi-patch is one `.shard` file with patches inline.** Not an arrangement referencing patch files. Cross-file references and reload-with-override leave the first slice.
24. **Superseded 2026-09-13: drift is retired** in Shard, replaced by LFOs ([`../projects/shard/design/modulation.md`](../projects/shard/design/modulation.md)). The principle it carried survives as a stronger rule: **modulation never writes a stored value.** The value store holds the hand's value, and the engine applies modulation where it reads. See "Learned from Shard".
25. **The first slice** is the node-type registry, categories, nodes with values, one stored order per node, references, groups and **bindings** (envelopes). Time, calculated results, smart groups and search stay as reserved shapes in the enums and the file format, with no API yet.
26. **Rename `mix.dry` to `grain.mix` before patch nodes carry values by id,** so the tree never sees the old id.
27. **Structural edits mid-performance use a whole-plan crossfade first.** State migration by node id comes when a delay tail makes it matter. Georg: *"Shard is likely the only app that has an author and a performance mode, if at all."* It is a Shard concern, not an API one.
28. **Oblique's transport (wasm or Tauri commands) is decided when Oblique migrates.**

**From the API layout of 2026-10-02** ([`rhizome-api.md`](rhizome-api.md)):

29. **Unknown node types pass through.** *"Unsupported nodes, yes, keep, don't change. Treat as pass-through noop."* Loaded as opaque nodes: reported, refused by every write verb, carried along when an ancestor is removed, moved or copied, and saved back byte for byte.
30. **Don't overfit to Shard.** Shard is the first consumer, not the shape. A core feature needs a second app that wants it.
31. **Out-of-range writes are refused.** The caller clamps; the schema exposes the range.
32. **One binding mechanism.** A bindable source type declares the values a binding carries (a depth, or none). A target is a slot or a value key.
33. **A write from another source during an open gesture joins it.** Same undo step; `cancel` reverts it too.
34. **A copy in the same file joins the original's groups.**

35. **rhizome is unaware of the apps.** *"I want rhizome be unaware of the apps. Only deals with the mechanics, references, diff, etc. Each app has their own … that defines the business logic."* (Georg, 2026-10-02.) rhizome's code, tests and docs name no app and hold no domain rule. Each app's object model owns its node types, rules (`check`s), domain verbs and projections, in the app's repo, documented in `projects/<app>/design/object-model.md`. App-specific content that was in this doc moved there on 2026-10-02. Decisions 15, 23, 26 and 27 are kept below as history; 23, 26 and 27 are Shard's and listed in [Shard's object model](../projects/shard/design/object-model.md).

36. **POM, the Preset Object Model,** is the shared base every app's object model is built on: documents, kinds and their policy, presets, commands, projections. It knows no app (35). It lives in the rhizome repo as `rhizome-pom` and `rhizome-pom-tauri`, with `@preset.nz/pom` for TypeScript. Built now, because every app needs it, not extracted later (Georg, 2026-10-02). Design: [`pom.md`](pom.md).
37. **Presets live in POM** as aggregates of getters and setters that each app's object model defines and customises (Georg, 2026-10-02: *"a funky aggregate of getter setters in pom, customised in the appOM"*). POM supplies catalogues, user presets and "which is current". (Following by cascade was here; decision 38 moved it to themes.)

38. **Presets are part of the kind,** the node template: declared with it, applied to its nodes, and used to make new ones (`add_from_preset`). User presets are document data keyed by kind, so they travel with the file. A shared choice followed by cascade (M&T's palette) is a **theme**, separate from presets. (Georg, 2026-10-02: *"I mean node template, the thing we instantiate when we create a node"*; *"user presets likely travel with the rhizome file."*) The theme half is superseded by decision 39.

39. **Themes are not built in.** *"A theme is a mechanic I can build on a per app basis. If it needs to be shared, we can lift it up."* (Georg, 2026-10-02.) An app builds one from rhizome's primitives: a theme is a node with values and no op, in a `themes` category; a follower holds a `Ref` to it; a theme op (a calculated node, such as one that answers `getColors`) turns it into what the app needs. It is lifted into POM only when a second app shares the same mechanic (decision 30). POM's `themes.rs` and its commands are removed. Loosely held: *"maybe a theme is just a preset without a node … let's not overthink it. In M&T a theme is a set of colours, and some nodes accept these sets as a list of colours to choose from."*
40. **Built-in catalogues come from a library rhizome** the app ships and references, so built-ins and user-made ones are the same kind of node. This pulls resolving cross-file references (`Ref::node_in`, deferred by decision 23) forward: it is needed before an app adopts a library. (Georg, 2026-10-02.)
41. **App state is a category** in the rhizome that holds the objects the app manages. No new mechanism: a category and the app's node types. (Georg, 2026-10-02.)
42. **No value-level references yet.** Using one node in several places (an image-read node in several layers) is a node reference, `Ref::here`, which exists. References into part of a node, or with local overrides (YAML's `<<: *base`), wait for a need: *"maybe premature optimization … maybe we don't need them yet."* (Georg, 2026-10-02.)

43. **An op is a file or a set of files** that defines it: name, params, presets, the signature its family gives it, and its implementation. *"I think of an op as a file/set of files, that define the op."* (Georg, 2026-10-02.) Whether the definition carries the implementation or names it is open. Direction only, not built: it meets [`plugin-primitive.md`](plugin-primitive.md) (manifests) and decision 40 (library rhizomes).
44. **Shared signatures are the app's.** *"Depends on the app. Think: SOPs, COPs, ROPs: all nodes, all with specific shared signatures."* (Georg, 2026-10-02.) An app defines its families of node types and the inputs and outputs each family shares. POM bakes in no family.
45. **Presets stay in POM, both shapes** (decision 37 stands): plain param values by default (`NodeValues`), a custom aggregate when an app needs one (Oblique's aspect). User presets are stored against the node type, and **export to JSON and import into another rhizome** (Georg, 2026-10-02). The export is a flat **preset file**, one node type per file, one or more presets; bindings are stripped on export for now (Georg, 2026-10-02: *"a preset is for a given node type … a preset file with multiple presets, but always just for the same node type"*; *"strip on export, for now"*).

46. **Every edit goes through the object model: over a transport, commands only.** The webview runs commands by id and drives gestures; no transport command takes a raw `Op`. A knob is the built-in `value.set` (an app overrides it like any built-in), a drag is a command run with a coalesce key, or commands between begin and end. `Op`s stay rhizome's data form inside Rust: tests, workflows, scripts, an object model's own commands. (Georg, 2026-10-02: *"have all edits go through the object model"*; chose commands only.)
47. **A saved rhizome is a scene description, like an IFD or a RIB.** Loading it restores the exact state, and it feeds the app's graph (the projection). One app's node families per document: *"I do not foresee the need of having Shard and Oblique nodes in the same app."* (Georg, 2026-10-02.) Cross-file references (decision 40) still let one app's file point at another's output.

48. **The webview's mirror is DTOs: a snapshot, then patches.** On load, and whenever the status's `generation` moves, Rust sends the whole rhizome as a view: every node with its values resolved (defaults filled in, a flag for what's actually set), children and orders, and the schema once. After that, each commit carries patches: the fresh rows of the nodes it touched and the ids it removed, alongside the raw changeset for UIs that list or animate changes. The front end replaces rows by id and never re-implements tree mechanics. Rows are immutable, so React re-renders only what a patch touched. (Georg, 2026-10-02: *"on load the rust thing sends the complete rhizome, later it sends patches and the frontend can apply the patches."*) The DTOs are forced by the process boundary, not by React.

49. **rhizome is a shared platform, not a pain-driven extraction, and Oblique adopts it first.** *"It is not about pain. It is about a shared platform that allows me to build these apps in a coordinated fashion."* Oblique goes first because it's the important app, 3D elements are coming (better to move before they land in the zustand store), and zustand was the familiar choice rather than a deliberate one (Georg, 2026-10-02). Supersedes decision 15's Shard-first. The way in is a **spike** on a throwaway Oblique branch with path dependencies: a Rust object model for pixel layers, an adapter that keeps Oblique's `Scene` shape over the mirror, and three interactions rewired (canvas move drag, opacity slider, save and open), measured before any migration is planned. Phase 3 (`@preset.nz/pom`) is shaped inside that work, not ahead of it.

**The point of rhizome** (Georg, 2026-10-02): *"we can construct a rhizome, reference other rhizomes, reference parts of rhizome, we can load explicit things, we can group things, we can store app state."* Mechanics an app builds on, not features it gets.

Decisions 31 to 34 were Claude's leans, taken by Georg on 2026-10-02 (*"go with your leans, unless they are a one-way door"*). None is: there are no files people keep yet. The near-one-way doors are the file format and the id encoding, so the file carries a format version from day one.

---

## The model

### Nodes and paths

Everything is a node at a path: `/patches/drone/granular`. Nodes nest, so a modifier stack or an effect chain is child nodes rather than records hanging off a node. Addressing is always by path, never by index.

Every node also carries a **`NodeId`** (decision 21): globally unique, ulid or uuid, not a per-file counter. Diff, bindings, orders, references and reverse indexes key on the id. The path is what a human reads and types. All three precedents already do this: Oblique's `crypto.randomUUID()` node ids, Fault's generational slotmap keys with its "never compact" rule, Shard's parameter ids as wire format.

The root node carries the file it was read from as a value, so decision 3 holds (no document object) and "unsaved" still has something to compare against.

### Node types — declared by the app

The core has no node enum (decision 20). An object model registers its node types at construction:

```rust
registry.declare(NodeType {
    name: "granular",
    origin: Origin::Calculated,          // or Loaded
    values: schema! { "grain.size": f32 [5.0..=500.0], "grain.mix": f32 [0.0..=1.0], /* … */ },
    slots:  &["env"],                    // binding slots this type accepts
});
```

The core validates values against the schema on write, serialises the type name with the node, and keeps a node whose type the running app never declared as an **opaque** node: reported, never edited, saved back unchanged (decision 29, superseding the earlier "refuses a file"). Shard's `ParamDef` table is the precedent for what a value schema holds: range, default, taper, unit, smoothing.

### Categories — where nodes live

A tree declares its categories, and every node lives in exactly one: the category of its top-level ancestor (decision 22). There are two kinds:

- **Loaded**: a reference to something owned by a file (a sample, an image, a patch, a node in another app's file). Serialised as the reference.
- **Calculated**: has a **recipe** (inputs plus values) and maybe a **result** (a render, a cook, an analysis, a plugin artifact). The recipe is serialised. The result is a rebuildable cache or nothing.

Envelopes and masks are categories too.

### Groups — unordered membership

A node can belong to many groups. A group's contents are put there explicitly.

**A smart group is a saved search** (see Search). That is one concept, not a second kind of group with its own machinery.

### Bindings — defined once, used many times

An envelope in the envelope category binds to many nodes, and a mask in the mask category binds to many nodes or groups. A binding is serialised as a value. Oblique's `Modifier.maskId` → `Scene.maskRefs` is the working precedent. A binding can also attach to a *value key* and carry values of its own, such as a depth (decision 32). One mechanism covers both.

### Order and time

Order is a named, stored list of node ids, and a tree may hold several (draw order, signal chain). Time is an optional property of a tree.

### Overrides

Stacked opinions, strongest wins: a preference, then a file, then an edit in the open file. This is Shard's tape-feel rule and the settings module's resolver. In practice it is two two-level rules: settings answers "preference or file", the node API answers "file on disk or edit in the open file". They share only a `Source` enum saying where a value came from, which lives in the settings crate.

### Diff and dirty

A changeset is a flat, path-ordered list of typed entries computed from two snapshots, never stored. Identity is the `NodeId`, so a rename or reparent is `Moved`, and a reorder is one `Reordered` entry on the owning node rather than a run of moves.

```rust
pub struct Changeset { pub entries: Vec<Change> }
pub struct Change { pub path: Path, pub id: NodeId, pub kind: ChangeKind }
pub enum ChangeKind {
    Added, Removed,
    Moved { from: Path },
    Value { key: ValueKey, from: Value, to: Value },
    Ref { from: Ref, to: Ref },
    Bound { slot: String, from: Option<NodeId>, to: Option<NodeId> },
    Reordered { order: String, from: Vec<NodeId>, to: Vec<NodeId> },
    Membership { group: NodeId, added: Vec<NodeId>, removed: Vec<NodeId> },
}
```

Readable means one line per entry: `/patches/drone/granular  grain.size  180 → 220`.

There are two dirties and both are **derived, never stored**:

- **Unsaved** is `diff(saved, current)` being non-empty, per file.
- **Stale** is a calculated node whose result's recipe hash differs from the hash of its current recipe and its inputs' recipes.

Nothing the engine writes back (meters, modulated values, results, search results) is ever in the tree, so nothing has to be filtered out of a snapshot. That removes the class of bug Oblique's `sceneChangedIgnoringDirty` and `restoreScene`'s dirty recomputation exist to handle: Oblique stores `dirty` on the node and it leaks into every snapshot. A recipe edit makes the file unsaved and its results stale; saving does not clear stale; rebuilding does not make anything unsaved.

**Reload with a local edit** (not in the first slice, decision 23): keep the snapshot loaded from disk. On reload take `file_changes = diff(disk_old, disk_new)` and `local = diff(disk_old, open)`. A file change whose id and key also appear in `local` is "hidden by your edit". Show `file_changes` with those flagged; edit wins by default, per entry "take file". A three-way merge with a trivial policy, for the cost of one retained snapshot.

### Undo

**Snapshot-based, in the Rust core, over the whole tree.** The diff is for display and for an index, not the undo mechanism. Both precedents are snapshots: Oblique holds `Scene` references, Fault clones the mesh per op with a cap of 64. Inverse changesets need an exact inverse for every operation including order and membership, for a memory saving that never shows at these tree sizes. Clone, cap at 50. If cloning ever costs, move to `Arc`-shared nodes; note `im` is MPL-2.0 and fails the licence gate, `rpds` is MIT/Apache.

Where an app keeps its history (Rust side or webview) is its object model's call. Shard's is in [its object model](../projects/shard/design/object-model.md#undo).

---

## Search

Search is the API's main read path, not a helper bolted on.

| By | Example |
|---|---|
| Path | `/patches/*/granular`, `/**/crush` |
| Kind | Category, node type |
| Value | `amp.gain < 0.5` |
| Relationship | What references `kalimba.wav`? Which nodes are bound to `/envelopes/swell`? What does this node depend on? |
| Origin | Loaded or calculated |
| State | Calculated and fresh, stale, or missing |

**Relationships use reverse indexes** kept by the core (reference target → referrers, binding target → bound nodes), so "what uses this" is a lookup, not a scan.

**A search can always match a calculated node's recipe. Matching its result requires the result to exist.** When it does not, the search reports the node as stale or missing rather than silently leaving it out, or an incomplete answer looks complete. Houdini's TOPs deal with the same split between a work item's recipe and its cached output.

**Across files:** a search can follow cross-file references (decision 4). This is opt-in, because it opens other files. "What in any file uses this sample?" needs an index of files. **Decided later** (decision 18). For now a search follows references outward from the open file and never consults a global index. Where an index lives is not rhizome's call; rhizome only promises the shapes below, so any catalog can consume them. (Strata is one candidate; see its object model.)

**Index-ready now, without designing the index.** Four shapes fixed so a cross-file index is additive later: node ids are globally unique, not per-file counters (Fault's slotmap keys are per-file and would collide); every reverse-index key is `(file, id)` with the file present even for same-file targets, so a per-file index is one shard of the global one; a `Ref` always carries its file, with a canonical spelling for "this file"; every commit produces a `Changeset` with a per-file monotonic sequence number, which is what any index consumes. No storage trait now.

**Smart groups:** a smart group saves a search. A search has to serialise, so it is a query, never a closure. **Decided: fixed predicates** (the table above, with and/or). A language comes only when something cannot be said with them. Keep queries translatable, because Strata's will want pushing down into DuckDB SQL.

A search result is never serialised and never diffed. Only the query is.

---

## The API

A sketch, not a signature. **Superseded by [`rhizome-api.md`](rhizome-api.md)** (2026-10-02): verbs, shape and guarantees from the caller's side. Node types come from the registry the object model filled (decision 20); `add` names a declared type.

```rust
let root = api::node("/");

let patches = root.add_category("patches",   Origin::Loaded);
let envs    = root.add_category("envelopes", Origin::Loaded);
let renders = root.add_category("renders",   Origin::Calculated);

let drone = patches.add("patch", "drone");                      // inline, decision 23
let gran  = drone.add_child("granular", "granular", values! { "grain.size" => 180.0 });

let swell = envs.add("envelope", "swell", values! { "attack" => 800.0 });
gran.bind("env", &swell);

let lead  = root.add_group("lead");
lead.insert(&drone);

let quiet = root.save_search("quiet", Query::category("patches").and(Query::lt("amp.gain", 0.5)));
root.find(Query::references("kalimba.wav"));   // reverse index
root.find(Query::glob("/patches/*/granular"));

root.for_each(&quiet, |n| n.set("grain.on", false)); // one batch, one undo step, one diff entry
root.set_order("chain", [&drone /* … */]);

let text    = root.serialise();                    // references and values
let changes = root.diff(&saved);                   // readable, keyed by path
```

### An object model on top

An app's object model is a declaration of its node types, an extension trait on `Edit` for its domain verbs, `check`s for its rules, and whatever projections it needs (a compiled plan, a render list). It uses only the API. [`rhizome-api.md`](rhizome-api.md) shows the shape; each app's `object-model.md` holds the real one.

---

## Languages and scripting

**One implementation, in Rust. Everything else is generated.**

| Language | How | Who uses it |
|---|---|---|
| Rust | The implementation | Shard and Strata natively in `src-tauri`, Fault inside its wasm module |
| TypeScript | Generated: types from Rust (`tsify` or `ts-rs`), calls through wasm-bindgen or Tauri commands. Fault's `mesh-wasm` is the precedent | Every webview. Oblique's store is TypeScript today |
| Python | **Not now.** Sidecars stay pure compute and receive data; they never search the tree. PyO3 bindings remain possible later over the same core | — |

Never a second hand-written implementation in any language.

### How to script without Python

Georg's question, 2026-09-13. Three options, cheapest first:

1. **Rust.** Object-model code and small CLIs, as `shard-play` is today. Available now, but compiled, so it is building rather than scripting.
2. **TypeScript or JavaScript over the generated bindings.** The webview is already a JavaScript engine, so an in-app console or a folder of script files costs little. Runs only where a webview runs.
3. **An embedded scripting language in the Rust core,** such as Rhai (pure Rust, MIT/Apache) or Lua via `mlua` (MIT). Scripts run identically in every app, headless or not, and on the Rust side. That matters for Shard, whose object model is Rust-side and never in the webview.

**Decided: not yet** (decision 16). Option 1 only, for now. If a need arrives, the lean is 2 first because it is nearly free, and 3 when a script has to run where no webview is: a headless batch job, or anything in Shard's engine process.

---

## Object models

Each app documents its object model in its own project:

- [Shard](../projects/shard/design/object-model.md): the worked one. Roles, presets, LFO links, the audio-thread plan.
- [Oblique](../projects/oblique/design/object-model.md), [Fault](../projects/fault/design/object-model.md), [Strata](../projects/strata/design/object-model.md), [Map & Territory](../projects/map-and-territory/design/object-model.md): stubs holding the first readings of 2026-09-13.

---

## The module

**There is no monorepo.** The API crate is its own repo beside `packages/facets`; `packages/rhizome` (decision 19).

- **Rust:** a path dependency for an app's first adoption, then pinned git revs or tags once a second app adopts, so each app upgrades when it chooses.
- **TypeScript:** a versioned npm package, the route `facets` is taking.

---

## Cross-file references

Not in the first slice (decision 23), but the form is fixed so the file format does not change later:

```rust
pub struct Ref { file: Option<RelPath>, id: NodeId, path: Path }   // file None = this file
```

File relative to the referring file, so a moved project folder survives. The id resolves; the path is what a human reads and is refreshed on save. **Always read the current file, never pin**: editing a node changes every place that references it. A frozen copy is a calculated node with a cached result, not a pinned reference. **A missing file leaves the node in place and marks the reference unresolved, with a relink command**: a missing file is nearly always a moved file.

**Loaded or calculated across files:** a node is calculated only when *this file's* object model holds a recipe it can run. A node over another app's output is loaded, referencing that output on disk; its staleness is the other app's concern until a cross-file index exists.

## Lessons from the first object model (2026-09-13)

Shard was built alongside this plan. What it taught that is mechanics, and so rhizome's; the Shard-specific rest (roles, presets, the audio thread) is in [Shard's object model](../projects/shard/design/object-model.md).

- **Bindings target value keys and carry values,** as well as slots. One mechanism (decision 32), not a second, parallel one per app.
- **Instances are the normal case, not tables.** Nodes with stable ids and a value schema per type, never a flat table of ids.
- **Transient values never enter the tree.** Modulated, metered or calculated values are applied where they are read. Saving saves the base, undo needs no filter, and a diff never shows a wobble. There is no API to write one.
- **Schema order is declaration order,** and rhizome keeps it, because an app may draw values in that order.
- **Stepped values are values, not indices.** A choice stores the value; the schema exposes the list.
- **UI state is not tree state.** Folded sections and the like live in [`persisted-ui-state.md`](persisted-ui-state.md), not in nodes.
- **Renames are cheap while an app is young.** Don't design migrations before files people keep exist. Unknown and missing keys are still reported on load.

## Still open

- **The cross-file index:** waits on a decision about where it lives. The four index-ready shapes under Search are fixed now.

Moved out on 2026-10-02: node roles (Shard's object model), Oblique's transport (Oblique's), the native-menu wiring's home and `usePersistedState` (neither is rhizome's; see [`tauri-scaffold.md`](tauri-scaffold.md) and [`native-apps.md`](native-apps.md)).

## Related

- [`native-apps.md`](native-apps.md): undo, the three value lifetimes, and the settings module.
- [`batched-mutations.md`](batched-mutations.md): `for_each`.
- [`versioned-persistence.md`](versioned-persistence.md): the file side.
- [`interaction-state.md`](interaction-state.md): selection is not tree state.
- [`rhizome-api.md`](rhizome-api.md): the API as built.
- [`pom.md`](pom.md): POM, the base every object model is built on.
- `projects/<app>/design/object-model.md`: each app's business logic.
