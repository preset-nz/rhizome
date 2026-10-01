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
15. **First consumer: Shard's multi-patch interface.**
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

An envelope in the envelope category binds to many nodes, and a mask in the mask category binds to many nodes or groups. A binding is serialised as a value. Oblique's `Modifier.maskId` → `Scene.maskRefs` is the working precedent. Shard's planned LFO links are bindings too, but from one *value key* to a modulator node, and carrying a depth. The binding model needs both of those; see "Learned from Shard".

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

For Shard, history lives in `src-tauri` around the object model, not the webview. `set_param` is the only UI write today, presets write through the command thread, and MIDI will arrive on the Rust side too; only Rust can tell a hand from a preset from a CC. The consequence: **the tree is the source of truth for values and the `ParamBank` is a compiled projection of it.** Undo restores the tree, recompiles, writes the bank. Modulation never writes the bank or the tree (decision 24 as superseded), so undo needs no filter at all. A drag is one step through explicit begin and end transaction calls from the UI, as Oblique's `history.ts` does.

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

**Across files:** a search can follow cross-file references (decision 4). This is opt-in, because it opens other files. "What in any file uses this sample?" needs an index of files. **Decided later** (decision 18). For now a search follows references outward from the open file and never consults a global index. Strata is the obvious candidate, but its storage is in question: Georg, 2026-09-13, *"we might need to move strata to sqlite, unless duckdb 2.0 solves the multi-connection issue."* Several apps querying one catalog is exactly the multi-connection case, so settle Strata's storage before making it the suite's index.

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

```rust
let mut shard = ShardObjectModel::new(api::node("/"));
let m = shard.add_material("~/samples/kalimba.wav");
let p = shard.add_patch("drone", &m);
shard.mute_group("lead");
```

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

## How each app maps

First readings, to confirm per app when it adopts.

| App | Today | Reading |
|---|---|---|
| **Shard** | Flat `.shard`: values by id, a sample path, node presets, LFOs and links. Drift is retired | **First consumer.** Material is loaded; renders and captures are calculated. Patches are inline nodes with effect children in one file (decision 23). The tree becomes the source of truth for values; the bank is compiled from it. See "Shard's audio thread" below |
| **Oblique** | TS store; array index is draw order; modifiers are records; `maskRefs`; caches outside the store. Its `sceneDiff.ts` already treats a reorder as a real change, agreeing with decision 7 | Categories, child-node modifiers, a stored draw order. Cooks are calculated nodes, which already matches its dirty/cache split. **A migration.** Transport decided then (decision 28) |
| **Fault** | Rust/WASM core owns mesh, selection and snapshot undo (`mesh-core`, `mesh-ops`, `mesh-wasm`). Selection groups, scatter layers and the modifier stack are planned in `architecture.md`, not built; noise is a destructive op. Corrected 2026-09-13 against the code | Closest in shape. Selection and undo stay app state. The "never compact slots" rule in `document.rs` is the id-stability precedent |
| **Strata** | DuckDB; Favourites → Shortlist → Collections; plugin artifacts; filters | Images are loaded; plugin artifacts are calculated. Collections are groups, filters are saved searches. Candidate index for cross-file search |

---

## The module

**There is no monorepo.** The API crate is its own repo beside `packages/facets`; `packages/rhizome` (decision 19).

- **Rust:** a path dependency while Shard is the only consumer, then pinned git revs or tags once a second app adopts, so each app upgrades when it chooses.
- **TypeScript:** a versioned npm package, the route `facets` is taking.

---

## Shard's audio thread

The audio thread never reads the tree. `ShardObjectModel::compile(&tree) -> Plan` runs on the command thread in `src-tauri`, allocating freely. The plan crosses to the audio thread through a lock-free single slot, and the retired plan crosses back through a second slot to be dropped off the audio thread. Shard's allocation guard (`shard_dsp::rt::GuardedAlloc`, held at zero by `tests/audio_thread.rs`) already caught a buffer freed on the audio thread; the plan swap must not repeat it.

```rust
pub struct Plan {
    nodes: Vec<NodeSlot>,          // per-type state: Player, Granular, Crush, Ring, Env, …
    node_ids: Vec<NodeId>,         // parallel to `nodes`
    edges: Vec<(usize, usize)>,    // signal order, topologically sorted at compile
    bank: ParamBank,               // sized for this plan; slots resolved at compile
    param_slots: Vec<(NodeId, ValueKey, usize)>,   // the id → slot map the UI writes through
    buffers: Vec<Vec<f32>>,        // one per edge, block-sized
}
```

`Slots::resolve` in `engine.rs` already does "resolve ids to indices once, panic on a miss"; compile is that generalised to N patches. `ParamBank::new` sized from the constant table becomes `ParamBank::for_plan`, and `index_of` becomes plan-specific. That is the real change to `params.rs`.

**Structural edits mid-performance: crossfade first** (decision 27). At the block boundary where the new plan arrives, run both for 30 to 50 ms under an equal-power crossfade. Grains and tails restart cold. Verifiable: the same tree recompiled must be bit-exact after the fade. **Later, state migration by id:** for every `NodeId` present in both plans, `mem::swap` the preallocated state box from old to new; nodes new to the plan ramp in, removed nodes stay in a draining slot for one release and ramp out. No allocation, no lock. Parameter writes that race the swap are seeded from the tree, so at most one block of one write is lost.

## Cross-file references

Not in the first slice (decision 23), but the form is fixed so the file format does not change later:

```rust
pub struct Ref { file: Option<RelPath>, id: NodeId, path: Path }   // file None = this file
```

File relative to the referring file, so a moved project folder survives. The id resolves; the path is what a human reads and is refreshed on save. **Always read the current file, never pin**: `document-model.md` already decided that editing a patch changes every placement of it. A frozen copy is Oblique's lock and bake, a calculated node with a cached result, not a pinned reference. **A missing file leaves the node in place and marks the reference unresolved, with a relink command.** The opposite of Oblique's `pruneMaskState`: a missing file is nearly always a moved file. Shard's `LoadReport.sample_missing` is the precedent.

**Loaded or calculated across apps:** a node is calculated only when *this app's* object model holds a recipe it can run. Oblique cannot run Shard, so an Oblique image over a Shard render is loaded, referencing the render on disk. Staleness of that render is Shard's concern until a cross-file index exists.

## Learned from Shard (2026-09-13)

Shard is the first consumer and was built alongside this plan. These came out of the code and Georg's use of it. Each is something the registry, the value schema or the compile step would otherwise rediscover. The Shard docs hold the detail; this keeps what is general.

**Node types have roles, and roles carry conventions.** Shard's effect sections settled on one pattern, which the registry should validate at `declare` time rather than leave to each app's tests:

- **An effect** has a switch `<node>.on`, drawn in its header rather than as a row. It is stepped and defaults off.
  - The engine fades the bypass itself over 10 ms, landing on an exact zero, so off is bit-exact with the node's mix at zero.
  - A one-pole smoother never lands, so the fade has to be a ramp that arrives.
- **Its first value is `<node>.mix`, named "Mix".** No value's name repeats the node's name ("Frequency", not "Ring freq"), because the header already says it.
- **Schema order is layout.** Shard's panel draws values in declaration order.
- **A generator** has the same switch, but its first value is `<node>.gain`, named "Gain". Shard has two: the plain sample and the grain cloud. Generators are summed, not mixed, and a generator's gain rests at unity where an effect's mix rests at zero. Granular was first built as an effect with a crossfade Mix; Georg re-cast it the same day, because it makes sound rather than shaping what comes in.
- **The master gain** is a plain node at the end of the chain (`amp.gain`), with no switch. Shard first applied it only inside the grain cloud, which made Output Gain silent with granular off.
- **A modulator is exempt.** An LFO leads with Rate and has no Mix.

So `NodeType` wants a `role` (generator, effect, modulator …). The generator role implies the switch-and-gain shape, and the effect role the switch-and-mix shape. Shard enforces this today with `every_switchable_node_leads_with_its_level` and `no_parameter_repeats_its_node_name` in `params.rs`.

**Presets are a node-level feature, generic enough for the core.** In Shard ([`../projects/shard/features/roadmap.md`](../projects/shard/features/roadmap.md), row 12):

- A preset is a named set of one node's values by key, excluding its switch, so applying one never switches a node in or out.
- Presets are document data, stored in the file and keyed by node type, then by name.
- **Save** refuses a name in use, **update** refuses a missing one, and **apply** writes only that node's own values and reports any key it could not apply.
- Links (bindings with depth) are part of a preset (modulation decision 6).

Nothing in that is Shard-specific.

**Bindings target value keys and carry values.** The model above binds a *slot* (`env`) to a node. Shard's LFO links bind a *value key* (`grain.position`) to a modulator node, with a signed depth. `Bound { slot, … }` in the changeset wants a key-or-slot target and the binding's own values, or links become a second, parallel mechanism.

**Instances are the normal case, not tables.** Shard's parameter table is flat and static, with ids as a wire format. It could not hold "as many LFOs as needed" (modulation decision 8), so LFOs are a list with stable ids: a small precursor of per-node values here. The registry's value schema per node type, with instances carrying `NodeId`s, is the general answer. Shard's flat table is the special case that goes away when patches become nodes.

**Modulation never writes a stored value.** This supersedes decision 24:

- The tree, or Shard's bank, holds the hand's value.
- The engine computes `denormalise(clamp(normalise(base) + depth × source))` where it reads.
- Saving saves the base, undo needs no filter, and a diff never shows a wobble.

**The audio thread only swaps things in; it never builds them.** This is not a limit on what can change, only on where memory is made. Everything structural is built on the command thread and swapped in whole at a block boundary: a new sample today, an LFO set next, a compiled `Plan` in decision 27. The old one is handed back to be freed elsewhere. Two traps Shard's allocation guard (`shard_dsp::rt::GuardedAlloc`, zero-held by `tests/audio_thread.rs`) has already caught:

- **Freeing counts as allocating.** A dropped buffer on the audio thread is as bad as a new one, so retire it back to the command thread.
- **On macOS a `Mutex`'s first lock allocates.** Lock every hand-off slot once before the stream starts.

**Stepped values are values, not indices.** Shard's panel used option indices as values, which only worked while every stepped range started at zero. Octave, from −2 to +2, broke it. A stepped schema entry should expose its value list.

**UI state is not tree state, and it outlives remounts.** Folded sections live in `localStorage`, following [`persisted-ui-state.md`](persisted-ui-state.md). Shard's panel remounts whenever its schema changes, so anything held inside it is lost.

**Renames are cheap while an app is young.** Shard renamed `mix.dry` to `grain.mix` and `env.amount` to `env.mix` within a day, because no saved patches existed. Do not design migrations before files people keep exist. Unknown and missing ids are still reported on load, for hand edits.

## Still open

- **Node roles:** the set (generator, effect, modulator …), and whether the registry enforces a role's conventions or only records them.
- **Bindings with a value-key target and a depth:** the shape, before Shard's LFO links are built outside the core and have to be pulled back in.
- **The cross-file index:** waits on Strata's storage (SQLite or DuckDB 2.0). The four index-ready shapes under Search are fixed now.
- **Oblique's transport** (decision 28). Wasm gives synchronous calls and a second build with no file access; Tauri commands give one native build with async edits and a read-only mirror of the tree in the webview. Also note Oblique's dependency-cruiser gate that only the sidecar transport may touch `@tauri-apps/api/core`.
- **The native-menu wiring's home.** Lean: one native-app package with settings as a module inside it, since rules 1, 4 and 5 of `native-apps.md` all touch the same two files per app.
- **`usePersistedState` "lifted from Strata"** in `native-apps.md`: verify it exists before the settings package plans around it. Shard now has its own, from the collapsible sections.

## Related

- [`native-apps.md`](native-apps.md): undo, the three value lifetimes, and the settings module.
- [`batched-mutations.md`](batched-mutations.md): `for_each`.
- [`versioned-persistence.md`](versioned-persistence.md): the file side.
- [`interaction-state.md`](interaction-state.md): selection is not tree state.
- [`../projects/fault/design/architecture.md`](../projects/fault/design/architecture.md): the crate-plus-wasm precedent.
- [`../projects/shard/features/roadmap.md`](../projects/shard/features/roadmap.md), row 10.
