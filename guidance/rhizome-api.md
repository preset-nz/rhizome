---
title: Rhizome API — verbs, shape and guarantees
type: design
status: draft
updated: 2026-10-02
---

# Rhizome API — verbs, shape and guarantees

**Draft, 2026-10-02.** The developer-facing layout of the node API that [`node-api.md`](node-api.md) describes. That doc decides *what* the tree is; this one decides how it feels to call. "The API" section there is "a sketch, not a signature"; this replaces the sketch. Where this doc bends a decision there, it says so under **Departures**.

Approach: write the call sites first, then read the verbs and guarantees off them. Four callers matter: an object model, a UI gesture over IPC, a load, and a contract test.

---

## Four call sites

### 1. An object model adds a patch

```rust
// shard/src-tauri/src/model.rs
pub fn declare(r: &mut RegistryBuilder) {
    r.category("material", Origin::Loaded);
    r.category("patches",  Origin::Loaded);
    r.node(NodeType::new("granular").role(Role::Generator)
        .value(GRAIN_SIZE, 5.0..=500.0, 80.0)
        .value(GRAIN_MIX,  0.0..=1.0,   1.0));
    // …
}

pub trait ShardEdit { fn add_patch(&mut self, name: &str, material: NodeId) -> Result<NodeId>; }

impl ShardEdit for Edit<'_> {
    fn add_patch(&mut self, name: &str, material: NodeId) -> Result<NodeId> {
        let patch = self.add("/patches", "patch", name)?;
        let gran  = self.add(patch, "granular", "granular")?;
        self.set(gran, GRAIN_SIZE, 180.0)?;
        self.set_ref(patch, "material", Ref::here(material))?;
        self.set_order(patch, "chain", [gran])?;
        Ok(patch)
    }
}

// the app
let commit = tree.edit("New Patch", |tx| tx.add_patch("drone", kalimba))?;
```

What this asks for:

- **The object model is a declaration plus an extension trait on `Edit`.** It does not own the tree or wrap it. Domain verbs compose into the caller's edit for free, so "new patch" is one undo step however many core verbs it uses.
- **Typed keys.** `const GRAIN_SIZE: Key<f32> = Key::new("grain.size")`. Object-model code gets a compile error for a wrong type. Generic code (UI, CLI, IPC) uses string keys and `Value`, and gets a runtime error.
- **Arguments take ids or paths.** `impl Into<At>` for `NodeId`, `&Path` and `&str`. Ids are what code holds; paths are what people type.

### 2. A UI drag over Tauri

```ts
// webview, through the generated bindings
const g = await rhizome.begin("Set Grain Size");
for (const v of dragValues) await rhizome.apply(g, [{ op: "set", at: id, key: "grain.size", value: v }]);
await rhizome.end(g);        // or rhizome.cancel(g) on Escape
```

```rust
// src-tauri
#[tauri::command] fn apply(g: GestureId, ops: Vec<Op>, s: State<App>) -> Result<Commit> { … }
```

What this asks for:

- **Two edit forms, one meaning.** `tree.edit(label, |tx| …)` scopes an edit to a closure, for code and tests. `begin` / `apply` / `end` / `cancel` keeps it open across calls, for gestures. Both make one undo step. `cancel` restores the snapshot taken at `begin`.
- **Every write verb is also data.** `Op` is a serde enum with one variant per verb, and `tx.apply(op)` runs it. IPC, a CLI, a test fixture and a future script all speak `Op`. Whether Oblique's transport is wasm or Tauri commands (decision 28), it carries `Op`s in and `Commit`s out.
- **Writes are visible while the gesture is open.** Each `apply` returns a `Commit` with the entries it made, so the panel updates mid-drag. The undo step is cut at `end`.

### 3. Load comes back with a report

```rust
let (tree, report) = Tree::load(&text, registry.clone())?;   // Err only when the text is not a rhizome file
for issue in &report.issues { log::warn!("{issue}"); }       // "/patches/drone/shimmer: unknown type `shimmer`"
```

- **Load fails only on unreadable text.** Everything else (unknown type, unknown key, out-of-range value, dangling reference) is an `Issue` in the report, with its path. See question 1 for what happens to an unknown node.

### 4. A contract test

```rust
#[test] fn rename_is_a_move() {
    let mut t = fixture();
    let before = t.snapshot();
    t.edit("Rename", |tx| tx.rename("/patches/drone", "hum")).unwrap();
    assert_eq!(t.diff(&before).to_string(), "/patches/hum  moved from /patches/drone\n");
}
```

- **Readable changesets are the assertion language.** `Changeset: Display`, one line per entry, path-ordered. Tests snapshot text.
- **`snapshot()` is cheap to call and compare.** Undo already clones the tree per step, so tests do the same.

---

## The shape

| Type | What it is | Notes |
|---|---|---|
| `Registry` | Node types and categories, frozen after `build()` | Shared by `Arc`. Validates role conventions at `declare` time |
| `Tree` | Owns one file's nodes, its history and its saved snapshot | `Send`, not shared. The host holds it on the command thread |
| `NodeId` | Copy, globally unique (ulid), never reused | The identity. What code holds |
| `Path` | Parsed, validated address | `/patches/drone/granular`. Sibling names are unique |
| `Node<'t>` | Read-only view, borrowed from `&Tree` | Can't outlive the next edit; the borrow checker says so |
| `Edit<'t>` | The only thing with write verbs | Exists inside `edit(…)` or an open gesture |
| `Value` | `Bool`, `Int`, `Float`, `Text`, `Choice(String)` | `Choice` holds the value, never an index ("Learned from Shard") |
| `Key<T>` / `ValueKey` | Typed and untyped value keys | Both spell `"grain.size"` |
| `Ref` | `{ file: Option<RelPath>, id, path }` | As in `node-api.md`. `Ref::here(id)` for this file |
| `Op` | One write verb, as data | serde. One variant per verb |
| `Commit` | `{ seq, label, changes: Changeset }` | What every write returns |
| `Query` | Data, never a closure | Fixed predicates (decision 17) |

**Why ids and views, not live handles.** The sketch's `gran.bind("env", &swell)` needs every handle to hold a shared, mutable pointer to the tree: `Rc<RefCell<…>>` inside, runtime borrow panics outside. Copyable ids plus borrowed views give the same reading with the checker enforcing "no reads of stale state across an edit". The cost is writing `tx.bind(gran, …)` instead of `gran.bind(…)`.

**Children iterate by name.** Hierarchy never implies order (decision 7). Anything ordered is asked for by name: `node.order("chain")`.

---

## The verbs

### Writes, on `Edit`

Every write verb lands in exactly one `ChangeKind`. The changeset is the verb list, replayed.

| Verb | `ChangeKind` | Refuses when | Cascades |
|---|---|---|---|
| `add(parent, type, name) -> NodeId` | `Added` | type undeclared, name taken, wrong category | — |
| `add_unique(parent, type, base) -> NodeId` | `Added` | as `add` | Suffixes `drone-2`. For "New Patch" in a UI |
| `remove(at)` | `Removed` per node | — | Subtree removed; dropped from every group and order; bindings to and from it unbound. Same-file `Ref`s to it stay and turn unresolved. Each cascade is its own entry |
| `rename(at, name)` / `move_to(at, parent)` | `Moved` | name taken, would cycle, crosses category | — |
| `set(at, key, value)` / `reset(at, key)` | `Value` | key not in schema, wrong type, out of range (question 2) | — |
| `set_ref(at, key, Ref)` | `Ref` | — | Unresolved is allowed and reported, never refused |
| `bind(target, source, values)` / `unbind(target, source)` | `Bound` | source type is not bindable, target slot or key unknown | — |
| `join(group, ids)` / `leave(group, ids)` | `Membership` | not a group | — |
| `set_order(owner, name, ids)` | `Reordered` | an id is not a child of `owner`, duplicates | — |
| `apply(op)` | whichever the `Op` is | as the verb | as the verb |

**Presets** sit on top of `set` and `bind`, with the rules already decided in "Learned from Shard": `save_preset` refuses a taken name, `update_preset` a missing one, `apply_preset` writes only that node's values (never its switch) plus its links, and reports keys it could not apply.

**Not verbs:** there is no write for a modulated, metered or calculated value (decision 24 as superseded). Those never enter the tree, so the API has nowhere to put them.

### Reads, on `&Tree` and `Node`

| Verb | Returns |
|---|---|
| `tree.at(path)` / `tree.get(id)` | `Option<Node>` |
| `node.id()` `.path()` `.name()` `.type_name()` `.category()` `.parent()` | the obvious |
| `node.children()` | by name |
| `node.get(KEY)` / `node.value("key")` | typed / `Value`. Defaults are resolved; `node.is_set(key)` says whether it is stored |
| `node.order("chain")` | `Vec<Node>` in stored order |
| `node.groups()`, `group.members()` | path order |
| `node.bindings()`, `node.bound_to()` | forward and reverse |
| `node.referrers()` | reverse index, `(file, id)` keyed |
| `tree.serialise()` | `String`, byte-stable |
| `tree.snapshot()`, `tree.diff(&snapshot)` | `Changeset` |

### Lifecycle, on `Tree`

| Verb | Notes |
|---|---|
| `Tree::new(registry)` / `Tree::load(text, registry)` | Load returns `(Tree, LoadReport)`. History starts empty |
| `edit(label, f)`; `begin(label)`, `end(g)`, `cancel(g)` | See call site 2 |
| `undo()` / `redo()` | `Option<Commit>`. The commit's changeset is the inverse, computed by diff |
| `undo_label()` / `redo_label()` | For the native menu: "Undo Set Grain Size" |
| `mark_saved()`, `is_unsaved()` | Unsaved is `diff(saved, current)` non-empty, never a flag |
| `seq()` | Per-tree commit sequence, monotonic. What a future index consumes |

### Reserved, no API in the first slice (decision 25)

The names are fixed now so nothing else takes them: `find(&Query)`, `save_search(name, Query)` for smart groups, `result(at)` and `staleness(at)` for calculated nodes, `time()` for trees that carry it. `find` will return `Found { nodes, incomplete }`, where `incomplete` lists calculated nodes whose result a query needed but which is stale or missing ("Search" in `node-api.md`).

---

## Edits in detail

- **Writes apply as they are made.** An edit records its changeset entries as it goes, so a commit is cheap. A debug assertion (and a property test) checks recorded entries equal `diff(before, after)`, so the two never drift.
- **One snapshot per undo step, not per write.** The base snapshot is taken at `begin` or at the start of `edit`. History keeps 50.
- **Errors roll back.** In `edit`, any `Err` restores the base snapshot and returns the error: no commit, no undo step, no sequence number. In a gesture, a refused `apply` refuses only that call; the gesture stays open.
- **One open edit per tree.** A write from another source while a gesture is open (a MIDI CC during a mouse drag) joins the gesture. It lands in the same undo step, and `cancel` reverts it too. Lean, because the alternatives are worse: queueing adds latency to MIDI, and refusing drops a knob turn.
- **Continuous sources coalesce.** `tree.edit_coalesced(label, key, f)`: consecutive commits with the same coalesce key (say `(cc, node, "grain.size")`) within 1 s of each other share one undo step and one base snapshot. Each still emits its own `Commit`, so the UI follows. Without this, a twist of a knob fills the 50-step history and clones the tree per CC message.
- **Empty edits vanish.** An edit with no entries makes no commit and no undo step.
- **Notification is pull, not push.** The core has no callbacks or observers. Every write returns a `Commit`, and the host forwards it (a Tauri event, a return value over wasm). The webview's mirror applies changesets in `seq` order and refetches if it sees a gap.

---

## Guarantees

Each one is a contract held by a named test in `rhizome-core/tests/contracts.rs`. If a guarantee has no test, it is a hope.

| # | Guarantee | Test |
|---|---|---|
| G1 | The tree changes only inside an edit. `&Tree` has no write method | compile-fail test (`trybuild`) |
| G2 | An edit that errors changes nothing: same snapshot, same `seq`, same history | `failed_edit_is_invisible` |
| G3 | One edit is one undo step and one `Commit` with `seq + 1`. Coalesced edits share an undo step | `one_edit_one_step`, `coalesced_knob_is_one_step` |
| G4 | Every stored value matches its schema. There is no way to store an invalid one, including via `Op` | `set_rejects_*`, property test over random `Op`s |
| G5 | A `NodeId` never changes and is never reused. Rename and reparent are `Moved` | `rename_is_a_move`, `removed_id_never_returns` |
| G6 | Nothing dangles within a file. Groups, orders and bindings never name a missing node; refs to one are reported unresolved | `remove_cascades`, property test |
| G7 | A reorder is one `Reordered` entry, never a run of moves | `reorder_is_one_entry` |
| G8 | Unsaved, stale, search results and reverse indexes are derived, never serialised. Undoing back to the saved point clears unsaved | `undo_to_saved_is_clean`, `serialise_has_no_derived_state` |
| G9 | `serialise` is byte-stable, and `load(serialise(t))` diffs empty against `t` | `round_trip_is_identity`, golden files |
| G10 | Load never drops anything silently. Everything it could not take is an `Issue` with a path | `unknown_type_is_reported` |
| G11 | Reverse indexes agree with forward data after every commit, undo and redo | property test |
| G12 | Children iterate by name; any other order is asked for by name | `children_are_by_name` |
| G13 | No API writes transient values (modulated, metered, calculated) into the tree | by absence; reviewed, not tested |

---

## TypeScript

Generated, never hand-written (`node-api.md`, "Languages"). The same nouns and verbs in camelCase. Reads come from a **mirror**: a plain object tree the bindings keep current from `Commit`s, so React reads synchronously whichever transport carries the writes. Writes are `Op[]` through `edit` or a gesture, and return `Promise<Commit>` over Tauri, or plain `Commit` over wasm. The mirror is read-only by type.

---

## Departures from `node-api.md`

- **Decision 3, "no document object".** `Tree` owns the nodes, the history, the saved snapshot and the registry handle, so something document-shaped exists. Lean reconciliation: `Tree` is a container with no domain state of its own; the file path lives on `/` as a value, as the doc already says; there is no name, title or metadata outside nodes; and every caller still addresses through paths from `/`.
- **`ChangeKind::Bound`** widens from `{ slot, from, to }` to `{ target: Target, source: NodeId, from: Option<Values>, to: Option<Values> }`, where `Target` is `Slot(NodeId, name)` or `Value(NodeId, ValueKey)`. This is the proposal for the open question on binding shape (below).
- **The changeset is recorded and checked, not only computed.** `node-api.md` says changesets are computed from two snapshots. Here edits record them, and a test asserts they equal the diff. `diff` stays the definition; recording is the cheap path.

---

## Questions for Georg

1. **Unknown node type on load: refuse the file, or keep the node?** `node-api.md` says "refuses" and cites `LoadReport`, which reports and carries on. **Lean: keep it as an opaque node.** It is read-only, saved back unchanged, and reported. Opening a file from a newer build then never destroys work.
2. **Out-of-range writes: refuse or clamp?** **Lean: refuse.** The schema exposes the range, so a slider or a CC mapping clamps before it writes. A silent clamp hides bugs in an object model.
3. **Binding shape.** **Lean: one mechanism.** A source node type declares it is bindable and the values a binding carries (an LFO link carries `depth`; an envelope binding carries none). A target is a slot or a value key. LFO links and envelopes are then the same verb and the same `Bound` entry.
4. **Concurrent writes during a gesture.** **Lean: join the open gesture** (see "Edits in detail"). The alternative is a separate edit per source, which needs per-source base snapshots and makes `cancel` ambiguous.

## Related

- [`node-api.md`](node-api.md): the model this is the API for.
- [`native-apps.md`](native-apps.md): undo and redo in the native menu.
- [`batched-mutations.md`](batched-mutations.md): `for_each`, now an edit over a query result.
- [`midi-control-surface.md`](midi-control-surface.md): the CC writes that coalesce.
