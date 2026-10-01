---
title: Rhizome API — verbs, shape and guarantees
type: design
status: draft
updated: 2026-10-02
---

# Rhizome API — verbs, shape and guarantees

**Draft, 2026-10-02.** The developer-facing layout of the node API that [`node-api.md`](node-api.md) describes. That doc decides *what* the tree is; this one decides how it feels to call. "The API" section there is "a sketch, not a signature"; this replaces the sketch. Where this doc bends a decision there, it says so under **Departures**.

Approach: write the call sites first, then read the verbs and guarantees off them. Four callers matter: an object model, a UI gesture over IPC, a load, and a contract test.

**Not Shard's API** (Georg, 2026-10-02: *"don't overfit to shard"*). Shard is the first consumer, not the shape. The examples rotate across apps, and anything that only one app has needed so far (roles, switches, MIDI) stays in that app's object model. A core feature needs a second app that wants it.

---

## Four call sites

### 1. An object model adds a node with children

```rust
// oblique/src-tauri/src/model.rs
pub fn declare(r: &mut RegistryBuilder) {
    r.category("images", Origin::Loaded);
    r.category("masks",  Origin::Loaded);
    r.node(NodeType::new("image").value(OPACITY, 0.0..=1.0, 1.0).value(SOURCE, ValueKind::Ref));
    r.node(NodeType::new("blur").value(RADIUS, 0.0..=200.0, 4.0).slot("mask"));
}

pub trait ObliqueEdit { fn add_image(&mut self, file: RelPath) -> Result<NodeId>; }

impl ObliqueEdit for Edit<'_> {
    fn add_image(&mut self, file: RelPath) -> Result<NodeId> {
        let img  = self.add_unique("/images", "image", file.stem())?;
        let blur = self.add(img, "blur", "blur")?;
        self.set(blur, RADIUS, 8.0)?;
        self.set_ref(img, SOURCE, Ref::file(file))?;
        self.set_order(img, "modifiers", [blur])?;
        self.append_to_order("/images", "draw", img)?;
        Ok(img)
    }
}

// the app
let commit = tree.edit("Add Image", |tx| tx.add_image(path))?;
```

Shard's `add_patch` and Strata's `add_to_collection` have the same shape. What this asks for:

- **The object model is a declaration plus an extension trait on `Edit`.** It does not own the tree or wrap it. Domain verbs compose into the caller's edit for free, so "Add Image" is one undo step however many core verbs it uses.
- **Typed keys.** `const RADIUS: Key<f64> = Key::new("blur.radius")`. Object-model code gets a compile error for a wrong type. Generic code (UI, CLI, IPC) uses string keys and `Value`, and gets a runtime error.
- **Arguments take ids or paths.** `impl Into<At>` for `NodeId`, `&Path` and `&str`. Ids are what code holds; paths are what people type.

### 2. A UI drag over Tauri

```ts
// webview, through the generated bindings
const g = await rhizome.begin("Set Opacity");
for (const v of dragValues) await rhizome.apply(g, [{ op: "set", at: id, key: "image.opacity", value: v }]);
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
for issue in &report.issues { log::warn!("{issue}"); }       // "/images/sky/glow: unknown type `glow`, kept as is"
```

- **Load fails only on unreadable text.** Everything else (unknown type, unknown key, out-of-range value, dangling reference) is an `Issue` in the report, with its path.
- **Unknown node types pass through** (decided, Georg 2026-10-02). See "Opaque nodes".

### 4. A contract test

```rust
#[test] fn rename_is_a_move() {
    let mut t = fixture();
    let before = t.snapshot();
    t.edit("Rename", |tx| tx.rename("/images/sky", "dusk")).unwrap();
    assert_eq!(t.diff(&before).to_string(), "/images/dusk  moved from /images/sky\n");
}
```

- **Readable changesets are the assertion language.** `Changeset: Display`, one line per entry, path-ordered. Tests snapshot text.
- **`snapshot()` is cheap to call and compare.** Undo already clones the tree per step, so tests do the same.

---

## The shape

| Type | What it is | Notes |
|---|---|---|
| `Registry` | Node types and categories, frozen after `build()` | Shared by `Arc` |
| `Tree` | Owns one file's nodes, its history and its saved snapshot | `Send`, not shared. The host holds it on its command thread |
| `NodeId` | Copy, globally unique (ulid), never reused | The identity. What code holds |
| `Path` | Parsed, validated address | `/images/sky/blur`. Sibling names are unique |
| `Node<'t>` | Read-only view, borrowed from `&Tree` | Can't outlive the next edit; the borrow checker says so |
| `Edit<'t>` | The only thing with write verbs | Exists inside `edit(…)` or an open gesture |
| `Value` | `Bool`, `Int`, `Float`, `Text`, `Choice(String)`, `Vec2`, `Vec3`, `Colour` | `Choice` holds the value, never an index. The vector and colour types are there because Oblique, Fault and Map & Territory each need them; add a type only when two apps do |
| `Key<T>` / `ValueKey` | Typed and untyped value keys | Both spell `"blur.radius"` |
| `Ref` | `{ file: Option<RelPath>, id, path }` | As in `node-api.md`. `Ref::here(id)` for this file |
| `Fragment` | A detached subtree in the file format | What copy, paste and the clipboard carry. See "Copy" |
| `Op` | One write verb, as data | serde. One variant per verb |
| `Commit` | `{ seq, label, changes: Changeset }` | What every write returns |
| `Query` | Data, never a closure | Fixed predicates (decision 17) |

**Why ids and views, not live handles.** The sketch's `gran.bind("env", &swell)` needs every handle to hold a shared, mutable pointer to the tree: `Rc<RefCell<…>>` inside, runtime borrow panics outside. Copyable ids plus borrowed views give the same reading with the checker enforcing "no reads of stale state across an edit". The cost is writing `tx.bind(…)` instead of `node.bind(…)`.

**Children iterate by name.** Hierarchy never implies order (decision 7). Anything ordered is asked for by name: `node.order("modifiers")`.

**No roles in the core.** "Learned from Shard" proposed a `role` on `NodeType` (generator, effect, modulator) with conventions the registry enforces. Only Shard has those roles. Instead, a node type can carry a `check` the object model writes, run at `declare` time on the schema and at commit time on the node. Shard puts its switch-and-mix rule there. If a second app grows the same rule, it moves into the core then.

---

## The verbs

### Writes, on `Edit`

Each verb lands in one `ChangeKind`, plus the cascades listed. The changeset is the verb list, replayed.

| Verb | `ChangeKind` | Refuses when | Cascades |
|---|---|---|---|
| `add(parent, type, name) -> NodeId` | `Added` | type undeclared, name taken, wrong category | — |
| `add_unique(parent, type, base) -> NodeId` | `Added` | as `add` | Names it `sky-2` if `sky` is taken |
| `remove(at)` | `Removed` per node | — | Subtree removed; dropped from every group and order; bindings to and from it unbound. Same-file `Ref`s to it stay and turn unresolved. Each cascade is its own entry |
| `rename(at, name)` / `move_to(at, parent)` | `Moved` | name taken, would cycle, crosses category | — |
| `copy(at, parent) -> NodeId` | `Added` per node | as `add` | See "Copy" |
| `paste(parent, &Fragment) -> PasteReport` | `Added` per node | Fragment unreadable | See "Copy" |
| `set(at, key, value)` / `reset(at, key)` | `Value` | key not in schema, wrong type, out of range (question 2) | — |
| `set_ref(at, key, Ref)` | `Ref` | — | Unresolved is allowed and reported, never refused |
| `bind(target, source, values)` / `unbind(target, source)` | `Bound` | source type is not bindable, target slot or key unknown | — |
| `join(group, ids)` / `leave(group, ids)` | `Membership` | not a group | — |
| `set_order(owner, name, ids)` / `append_to_order(owner, name, id)` | `Reordered` | an id is not a child of `owner`, duplicates | — |
| `apply(op)` | whichever the `Op` is | as the verb | as the verb |

Any write that names an opaque node directly is refused. See "Opaque nodes".

**Presets** sit on top of `set` and `bind`, with the rules already decided in "Learned from Shard", made generic. A preset is a named set of one node's values, keyed by node type. A type marks keys as `not_in_presets` (Shard marks its switch). `save_preset` refuses a taken name, `update_preset` a missing one, and `apply_preset` writes only that node's own values and bindings, and reports the keys it could not apply.

**Not verbs:** there is no write for a transient value: modulated, metered, previewed mid-hover, or calculated. Those never enter the tree, so the API has nowhere to put them.

### Reads, on `&Tree` and `Node`

| Verb | Returns |
|---|---|
| `tree.at(path)` / `tree.get(id)` | `Option<Node>` |
| `node.id()` `.path()` `.name()` `.type_name()` `.category()` `.parent()` `.is_opaque()` | the obvious |
| `node.children()` | by name |
| `node.get(KEY)` / `node.value("key")` | typed / `Value`. Defaults are resolved; `node.is_set(key)` says whether it is stored |
| `node.order("modifiers")` | `Vec<Node>` in stored order |
| `node.groups()`, `group.members()` | path order |
| `node.bindings()`, `node.bound_to()` | forward and reverse |
| `node.referrers()` | reverse index, `(file, id)` keyed |
| `tree.extract(ats) -> Fragment` | For copy to clipboard. See "Copy" |
| `tree.serialise()` | `String`, byte-stable |
| `tree.snapshot()`, `tree.diff(&snapshot)` | `Changeset` |

### Lifecycle, on `Tree`

| Verb | Notes |
|---|---|
| `Tree::new(registry)` / `Tree::load(text, registry)` | Load returns `(Tree, LoadReport)`. History starts empty |
| `edit(label, f)`; `begin(label)`, `end(g)`, `cancel(g)` | See call site 2 |
| `undo()` / `redo()` | `Option<Commit>`. The commit's changeset is the inverse, computed by diff |
| `undo_label()` / `redo_label()` | For the native menu: "Undo Set Opacity" |
| `mark_saved()`, `is_unsaved()` | Unsaved is `diff(saved, current)` non-empty, never a flag |
| `seq()` | Per-tree commit sequence, monotonic. What a future index consumes |

### Reserved, no API in the first slice (decision 25)

The names are fixed now so nothing else takes them: `find(&Query)`, `save_search(name, Query)` for smart groups, `result(at)` and `staleness(at)` for calculated nodes, `time()` for trees that carry it. `find` will return `Found { nodes, incomplete }`, where `incomplete` lists calculated nodes whose result a query needed but which is stale or missing ("Search" in `node-api.md`).

---

## Copy

**Copy is a core verb, not `read` plus `add`.** Composing it in the object model gets four things wrong, and every app would get them wrong differently:

- **Ids.** Every node in the copy needs a fresh `NodeId`, and everything *inside* the subtree that points at a node inside it (orders, bindings, refs) must point at the copy instead. Read-then-add has no remap table.
- **Opaque nodes.** `add` refuses an undeclared type, so a composed copy cannot copy a subtree holding an opaque node. The core can, verbatim.
- **Unset values.** A read returns resolved defaults. Writing them back stores them, so the copy stops following a changed default. The core copies what is stored, not what is read.
- **Undo.** One copy is one `Added` per node in one edit. Composed, it is still one edit, but its changeset is a stream of `Value` and `Bound` entries that reads like hand-building, not like a copy.

**Shape.** Copy is `extract` then `paste`, and both are public, because the clipboard sits between them.

```rust
let frag = tree.extract(&[sky])?;                         // read: a detached subtree, in the file format
tree.edit("Duplicate", |tx| tx.copy(sky, "/images"))?;     // same file: extract + paste in one call
tree.edit("Paste", |tx| tx.paste("/images", &frag))?;      // from the clipboard, maybe from another file
```

`Fragment` is the file format for a subtree, so the clipboard holds text a person can read, and pasting is loading into a subtree. Everything load reports, paste reports too, in a `PasteReport`.

**What a copy points at:**

| Pointer | Inside the copied subtree | Outside it |
|---|---|---|
| Order entries | Remapped to the copies | — (an order only names children) |
| Bindings | Remapped | Kept: the copy is bound to the same mask or envelope |
| Refs | Remapped | Kept: the copy uses the same source file |
| Group membership | — | Same file: the copy joins the original's groups. Another file: dropped and reported |
| Parent's orders | — | Same file: the copy goes straight after the original in every order of the parent that holds it. Another file, or no original there: appended |

**Across files** (paste from the clipboard into another file): pointers to outside nodes are looked up by id, then by path, in the target file. Anything still missing stays as an unresolved pointer and is reported, the same rule as a missing file. Cross-file `Ref`s are not in the first slice (decision 23), so a paste never invents one.

**Names.** A copy is named as `add_unique` names: `sky` becomes `sky-2`. Only the top node is renamed; children keep their names, since they are unique under their new parent already.

---

## Opaque nodes

Decided, Georg 2026-10-02: *"Unsupported nodes, yes, keep, don't change. Treat as pass-through noop."*

- **On load,** a node whose type the running app never declared becomes **opaque**. Its id, path, type name and structural pointers (bindings, refs, orders, groups) are parsed, because they are the core's format. Its values are kept as raw text, unvalidated.
- **It is reported** once, as an `Issue`, so the app can say "this file has nodes this version can't edit".
- **It does not change.** Any verb that names it directly (`set`, `rename`, `bind` with it as source or target, `join`) is refused with `Error::Opaque`. Its values are written back byte for byte.
- **It travels with its ancestors.** Removing, moving or copying an ancestor takes the opaque node along, unchanged except for a fresh id in a copy. That is pass-through, not editing.
- **Its children are ordinary.** A known type under an opaque parent loads and edits normally.
- **Nothing reads it.** Object models skip it; `node.is_opaque()` says so. Search will match it by path and type name only.

This replaces "refuses a file naming a type the running app never declared" in `node-api.md` (decision 29 there).

---

## Edits in detail

- **Writes apply as they are made.** An edit records its changeset entries as it goes, so a commit is cheap. A debug assertion (and a property test) checks recorded entries equal `diff(before, after)`, so the two never drift.
- **One snapshot per undo step, not per write.** The base snapshot is taken at `begin` or at the start of `edit`. History keeps 50.
- **Errors roll back.** In `edit`, any `Err` restores the base snapshot and returns the error: no commit, no undo step, no sequence number. In a gesture, a refused `apply` refuses only that call; the gesture stays open.
- **One open edit per tree.** A write from another source while a gesture is open (a controller knob during a mouse drag, a file watcher during a scrub) joins the gesture. It lands in the same undo step, and `cancel` reverts it too. Lean, because queueing adds latency and refusing drops input.
- **Continuous input coalesces.** `tree.edit_coalesced(label, key, f)`: consecutive commits with the same coalesce key (say `("scrub", node, "image.opacity")`) within 1 s of each other share one undo step and one base snapshot. Each still emits its own `Commit`, so the UI follows. This is for input with no begin and end: a MIDI knob, a scroll wheel on a value, arrow-key nudges. Without it, a nudge fills the 50-step history and clones the tree per keypress.
- **Empty edits vanish.** An edit with no entries makes no commit and no undo step.
- **Notification is pull, not push.** The core has no callbacks or observers. Every write returns a `Commit`, and the host forwards it (a Tauri event, a return value over wasm). The webview's mirror applies changesets in `seq` order and refetches if it sees a gap.

---

## Guarantees

Each one is a contract held by a named test in `rhizome-core/tests/contracts.rs`. If a guarantee has no test, it is a hope.

| # | Guarantee | Test |
|---|---|---|
| G1 | The tree changes only inside an edit. `&Tree` has no write method | compile-fail test (`trybuild`) |
| G2 | An edit that errors changes nothing: same snapshot, same `seq`, same history | `failed_edit_is_invisible` |
| G3 | One edit is one undo step and one `Commit` with `seq + 1`. Coalesced edits share an undo step | `one_edit_one_step`, `coalesced_nudges_are_one_step` |
| G4 | Every stored value on a known node matches its schema. There is no way to store an invalid one, including via `Op` | `set_rejects_*`, property test over random `Op`s |
| G5 | A `NodeId` never changes and is never reused. Rename and reparent are `Moved` | `rename_is_a_move`, `removed_id_never_returns` |
| G6 | Nothing dangles within a file. Groups, orders and bindings never name a missing node; refs to one are reported unresolved | `remove_cascades`, property test |
| G7 | A reorder is one `Reordered` entry, never a run of moves | `reorder_is_one_entry` |
| G8 | Unsaved, stale, search results and reverse indexes are derived, never serialised. Undoing back to the saved point clears unsaved | `undo_to_saved_is_clean`, `serialise_has_no_derived_state` |
| G9 | `serialise` is byte-stable, and `load(serialise(t))` diffs empty against `t` | `round_trip_is_identity`, golden files |
| G10 | Load and paste never drop anything silently. Everything they could not take is an `Issue` with a path | `unknown_type_is_reported`, `paste_reports_missing_targets` |
| G11 | An opaque node's values survive load, edits elsewhere, copy and save byte for byte, and no verb writes to it | `opaque_round_trips`, `opaque_refuses_writes` |
| G12 | A copy shares no `NodeId` with its original, and nothing inside it points back into the original | `copy_remaps_internal_pointers`, property test |
| G13 | Reverse indexes agree with forward data after every commit, undo and redo | property test |
| G14 | Children iterate by name; any other order is asked for by name | `children_are_by_name` |
| G15 | No API writes transient values (modulated, metered, previewed, calculated) into the tree | by absence; reviewed, not tested |

---

## TypeScript

Generated, never hand-written (`node-api.md`, "Languages"). The same nouns and verbs in camelCase. Reads come from a **mirror**: a plain object tree the bindings keep current from `Commit`s, so React reads synchronously whichever transport carries the writes. Writes are `Op[]` through `edit` or a gesture, and return `Promise<Commit>` over Tauri, or plain `Commit` over wasm. The mirror is read-only by type. Clipboard copy is `extract` to text on the system clipboard; paste reads it back.

---

## Departures from `node-api.md`

- **Decision 3, "no document object".** `Tree` owns the nodes, the history, the saved snapshot and the registry handle, so something document-shaped exists. Lean reconciliation: `Tree` is a container with no domain state of its own; the file path lives on `/` as a value, as the doc already says; there is no name, title or metadata outside nodes; and every caller still addresses through paths from `/`.
- **Unknown types pass through** instead of refusing the file. Decided; recorded as decision 29 there.
- **No roles in the core.** "Learned from Shard" proposed them; here they are an object-model `check` until a second app needs them.
- **`ChangeKind::Bound`** widens from `{ slot, from, to }` to `{ target: Target, source: NodeId, from: Option<Values>, to: Option<Values> }`, where `Target` is `Slot(NodeId, name)` or `Value(NodeId, ValueKey)`. This is the proposal for the open question on binding shape (below).
- **The changeset is recorded and checked, not only computed.** `node-api.md` says changesets are computed from two snapshots. Here edits record them, and a test asserts they equal the diff. `diff` stays the definition; recording is the cheap path.

---

## Questions for Georg

1. **Out-of-range writes: refuse or clamp?** **Lean: refuse.** The schema exposes the range, so a slider or a controller mapping clamps before it writes. A silent clamp hides bugs in an object model.
2. **Binding shape.** **Lean: one mechanism.** A source node type declares that it is bindable and which values a binding carries: a modulation link carries `depth`, a mask binding carries none. A target is a slot or a value key. Oblique's masks and Shard's LFO links are then the same verb and the same `Bound` entry.
3. **Concurrent writes during a gesture.** **Lean: join the open gesture** (see "Edits in detail"). The alternative is a separate edit per source, which needs per-source base snapshots and makes `cancel` ambiguous.
4. **Copy and groups.** **Lean: a copy in the same file joins the original's groups.** The alternative is that a copy starts with no groups. Which does duplicating a member of Strata's "Shortlist" or Oblique's mask group expect?

## Related

- [`node-api.md`](node-api.md): the model this is the API for.
- [`native-apps.md`](native-apps.md): undo and redo in the native menu; Edit › Copy, Paste and Duplicate.
- [`batched-mutations.md`](batched-mutations.md): `for_each`, now an edit over a query result.
- [`midi-control-surface.md`](midi-control-surface.md): one source of coalesced writes.
