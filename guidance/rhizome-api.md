---
title: Rhizome API — verbs, shape and guarantees
type: design
status: current
updated: 2026-10-02
---

# Rhizome API — verbs, shape and guarantees

**Current, 2026-10-02. The first slice is built** in `packages/rhizome/crates/rhizome-core` and matches this doc. The developer-facing layout of the node API that [`node-api.md`](node-api.md) describes. That doc decides *what* the tree is; this one decides how it feels to call. "The API" section there is "a sketch, not a signature"; this replaces the sketch. Where this doc bends a decision there, it says so under **Departures**.

Approach: write the call sites first, then read the verbs and guarantees off them. Four callers matter: an object model, a UI gesture over IPC, a load, and a contract test.

**rhizome is unaware of the apps** (decision 35). It handles mechanics; each app's **object model** holds the business logic, in the app's repo, documented in `projects/<app>/design/object-model.md`.

| rhizome: mechanics | An object model: business logic |
|---|---|
| Paths, ids, the tree | Which node types and categories exist, and what they mean |
| Node types as declared data; validating values against their schema | Rules beyond the schema, as `check`s on its node types |
| Values, references, groups, bindings, stored orders | Domain verbs over `Edit` (`add_patch`, `add_image`), each one undo step |
| Edits, gestures, coalescing, undo, `Commit`s | Projections: a compiled plan, a render list, a search index |
| Diff, unsaved, the file format, load reports, opaque pass-through | Where history lives, which input coalesces, how values are laid out |
| Copy and paste with remapping | Migrations of its own files, when files people keep exist |

rhizome's code, tests and docs name no app. Its tests use made-up object models. A mechanism moves into rhizome only when two object models need the same one.

---

## Four call sites

### 1. An object model adds a node with children

```rust
// an app's object model, in its own repo (illustrative)
pub fn declare(r: &mut RegistryBuilder) {
    r.category("images", Origin::Loaded);
    r.category("masks",  Origin::Loaded);
    r.node(NodeType::new("image").in_categories(&["images"]).float(OPACITY, 0.0..=1.0, 1.0).reference(SOURCE).slot("mask"));
    r.node(NodeType::new("blur").in_categories(&["images"]).float(RADIUS, 0.0..=200.0, 4.0));
}

pub trait GalleryEdit { fn add_image(&mut self, file: RelPath) -> Result<NodeId>; }

impl GalleryEdit for Edit<'_> {
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

// the app: the closure's value, and the commit (None when nothing changed)
let (img, commit) = tree.edit("Add Image", |tx| tx.add_image(path))?;
```

Any object model's domain verbs have this shape. What this asks for:

- **The object model is a declaration plus an extension trait on `Edit`.** It does not own the tree or wrap it. Domain verbs compose into the caller's edit for free, so "Add Image" is one undo step however many core verbs it uses.
- **Typed keys.** `const RADIUS: Key<f64> = Key::new("blur.radius")`, written with `set(at, RADIUS, 8.0)` and read with `get(RADIUS)`. Object-model code gets a compile error for a wrong type. Generic code (UI, CLI, IPC) uses `set_value(at, "blur.radius", Value::Float(8.0))` and `value("blur.radius")`, and gets a runtime error.
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
#[tauri::command] fn apply(g: GestureId, ops: Vec<Op>, s: State<App>) -> Result<Option<Commit>> {
    s.tree.lock().apply(g, &ops)     // a refused apply undoes only itself; the gesture stays open
}
```

What this asks for:

- **Two edit forms, one meaning.** `tree.edit(label, |tx| …)` scopes an edit to a closure, for code and tests; `tree.edit_ops(label, &ops)` is the same with `Op`s. `begin` / `apply` (or `within` with a closure) / `end` / `cancel` keeps it open across calls, for gestures. Both make one undo step. `cancel` restores the snapshot taken at `begin`.
- **Every write verb is also data.** `Op` is a serde enum with one variant per verb, and `tx.apply(op)` runs it. IPC, a CLI, a test fixture and a future script all speak `Op`. Whether an app's transport is wasm or Tauri commands, it carries `Op`s in and `Commit`s out. An `Op` names nodes by path or id, references included: `{"op": "set_ref", …, "ref": {"node": "/images/sky"}}`, so a script never depends on how ids were allocated. Workflow 13 holds every variant as JSON.
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
| `Value` | `Bool`, `Int(i64)`, `Float(f64)`, `Text`, `Choice(String)`, `Vec2`, `Vec3`, `Colour` (0 to 1 per channel) | `Choice` holds the value, never an index. The vector and colour types are common enough across 2D and 3D tools to be mechanics; add a kind only when two object models need it |
| `Key<T>` / `ValueKey` | Typed and untyped value keys | Both spell `"blur.radius"` |
| `Ref` | `{ file: Option<RelPath>, id, path }` | As in `node-api.md`. `Ref::here(id)` for this file |
| `Fragment` | A detached subtree in the file format | What copy, paste and the clipboard carry. `to_text` / `from_text`. See "Copy" |
| `Op` | One write verb, as data | serde. One variant per verb |
| `Commit` | `{ seq, label, changes: Changeset }` | What every write returns, wrapped in `Option`: `None` when nothing changed |
| `Query` | Data, never a closure | Fixed predicates (decision 17) |

**Why ids and views, not live handles.** The sketch's `gran.bind("env", &swell)` needs every handle to hold a shared, mutable pointer to the tree: `Rc<RefCell<…>>` inside, runtime borrow panics outside. Copyable ids plus borrowed views give the same reading with the checker enforcing "no reads of stale state across an edit". The cost is writing `tx.bind(…)` instead of `node.bind(…)`.

**Children iterate by name.** Hierarchy never implies order (decision 7). Anything ordered is asked for by name: `node.order("modifiers")`.

**No roles in the core.** A role (generator, effect, modulator) and its conventions are business logic. A node type can carry a `check` the object model writes; rhizome runs every `check` at commit and rolls the edit back if one fails. [Shard's object model](../projects/shard/design/object-model.md#roles) puts its roles there.

---

## The verbs

### Writes, on `Edit`

Each verb lands in one `ChangeKind`, plus the cascades listed. The changeset is the verb list, replayed.

| Verb | `ChangeKind` | Refuses when | Cascades |
|---|---|---|---|
| `add(parent, type, name) -> NodeId` | `Added` | type undeclared, name taken or invalid, wrong category, parent is `/`, a group or opaque | — |
| `add_unique(parent, type, base) -> NodeId` | `Added` | as `add` | Names it `sky-2` if `sky` is taken |
| `remove(at)` | `Removed` per node | the root or a category | Subtree removed; dropped from every group and order; bindings to and from it unbound. Same-file `Ref`s to it stay and turn unresolved. Each cascade is its own entry |
| `rename(at, name)` / `move_to(at, parent)` | `Moved` | name taken, would cycle, crosses category | — |
| `copy(at, parent) -> NodeId` | `Added` per node | as `add` | See "Copy" |
| `paste(parent, &Fragment) -> PasteReport` | `Added` per node | a node's type can't live there | See "Copy" |
| `set(at, KEY, v)` / `set_value(at, "key", Value)` / `reset(at, key)` | `Value` | key not in schema, wrong kind, non-finite, unknown choice, out of range (decision 31) | — |
| `set_ref(at, key, Ref)` / `clear_ref(at, key)` | `Ref` | key isn't a reference key | Unresolved is allowed and reported, never refused |
| `bind(target, On::slot(..) \| On::value(..), source, values)` / `unbind(target, on, source)` | `Bound` | source type is not bindable, target slot or key unknown, binding to itself, values out of their spec | Binding a slot replaces what was in it; a value key takes many sources |
| `join(group, ids)` / `leave(group, ids)` | `Membership` | not a group | — |
| `set_order(owner, name, ids)` / `append_to_order(owner, name, id)` | `Reordered` | an id is not a child of `owner`, duplicates | — |
| `apply(op)` | whichever the `Op` is | as the verb | as the verb |

Any write that names an opaque node directly is refused. See "Opaque nodes".

**Presets are reserved** (decision 25), and whether they are mechanics or business logic is open (`node-api.md`, "Still open"). The lean: a mechanism here (a named set of one node's values, applied in one edit, reporting keys it couldn't apply), with the object model naming the keys presets skip.

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
| `node.bindings()`, `node.bound_to()` | forward and reverse; `Binding::value("depth")` resolves defaults |
| `node.referrers()` | nodes holding a here-reference to this one |
| `tree.nodes()`, `tree.len()` | every node, in path order |
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
| `history_len()` | Undo steps held, at most `HISTORY` (50) |

### Reserved, no API in the first slice (decision 25)

The names are fixed now so nothing else takes them: presets (above), `find(&Query)`, `save_search(name, Query)` for smart groups, `result(at)` and `staleness(at)` for calculated nodes, `time()` for trees that carry it. `find` will return `Found { nodes, incomplete }`, where `incomplete` lists calculated nodes whose result a query needed but which is stale or missing ("Search" in `node-api.md`).

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

**Across files** (paste from the clipboard into another file): pointers to outside nodes are looked up by id, then by path, in the target file. A reference still missing stays, unresolved, and is reported, the same rule as a missing file. A binding or membership still missing is dropped and reported, because those never dangle (G6). Cross-file `Ref`s are not in the first slice (decision 23), so a paste never invents one.

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

- **Writes apply as they are made.** The commit's changeset is `diff(before, after)`: one definition, no second bookkeeping path. Reverse reads (`groups`, `bound_to`, `referrers`) scan forward data, so they can't disagree with it. Both are cheap at these tree sizes; an index arrives with search and must keep that property.
- **One snapshot per undo step, not per write.** The base snapshot is taken at `begin` or at the start of `edit`. History keeps 50.
- **Errors roll back.** In `edit`, any `Err` restores the base snapshot and returns the error: no commit, no undo step, no sequence number. In a gesture, a refused `apply` refuses only that call; the gesture stays open.
- **One open edit per tree.** A write from another source while a gesture is open (a controller knob during a mouse drag, a file watcher during a scrub) joins the gesture. It lands in the same undo step, and `cancel` reverts it too (decision 33): queueing would add latency and refusing would drop input.
- **Continuous input coalesces.** `tree.edit_coalesced(label, key, f)`: consecutive edits with the same coalesce key (say `"opacity nudge"`), each within 1 s of the last (`set_coalesce_window`), share one undo step. Each still returns its own `Commit`, so the UI follows. Any other edit, undo, redo or gesture breaks the run. This is for input with no begin and end: a MIDI knob, a scroll wheel on a value, arrow-key nudges. Without it, a nudge fills the 50-step history. The clock is injectable (`set_clock`) so tests don't sleep.
- **Empty edits vanish.** An edit with no entries makes no commit and no undo step.
- **Notification is pull, not push.** The core has no callbacks or observers. Every write returns a `Commit`, and the host forwards it (a Tauri event, a return value over wasm). The webview's mirror applies changesets in `seq` order and refetches if it sees a gap.

---

## Guarantees

Each one is held by a test. If a guarantee has no test, it is a hope. Three suites hold them: **acid** (`tests/acid.rs`) builds and operates one of everything; **core workflows** (`tests/core_workflows.rs`) runs twelve workflows stored as data through the stable surface, with pinned transcripts; **invariants** (`tests/invariants.rs`) runs seeded random `Op`s and checks G2, G4, G6, G9 and undo after every step.

| # | Guarantee | Test |
|---|---|---|
| G1 | The tree changes only inside an edit. `&Tree` has no write method | `compile_fail` doctest on `Tree` |
| G2 | An edit that errors changes nothing: same snapshot, same `seq`, same history | acid `refused(…)` for every refusal; workflow 11; invariants |
| G3 | Every commit is `seq + 1`. One edit is one undo step; a gesture commits per `apply` and is still one undo step; coalesced edits share one | workflows 04, 05, 12; acid gestures and coalescing |
| G4 | Every stored value on a known node matches its schema. There is no way to store an invalid one, including via `Op` | acid refusals; invariants |
| G5 | A `NodeId` never changes and is never reused. Rename and reparent are `Moved`, one entry, children not listed | workflow 02; acid rename |
| G6 | Nothing dangles within a file. Groups, orders and bindings never name a missing node; refs to one are reported unresolved | workflow 06; acid remove; invariants |
| G7 | A reorder is one `Reordered` entry, never a run of moves | workflow 03 |
| G8 | Unsaved is derived, never a flag or a field. Undoing back to the saved point clears unsaved | workflow 09; acid |
| G9 | `serialise` is byte-stable and canonical, and `load(serialise(t))` writes the same bytes and diffs empty | `tests/golden/acid.rhizome`; workflow 01; invariants |
| G10 | Load and paste fail only on unreadable text. Everything else they could not take is an `Issue` with a path, in path order | acid `load_reports_instead_of_failing` (golden report); acid paste; workflow 08 |
| G11 | An opaque node's values (and its bindings' values) survive load, edits elsewhere, rename and copy of an ancestor, and save, unchanged in canonical output. No verb writes to it. Its structural pointers are the core's, so removing a node it points at still cascades | acid `opaque_nodes_pass_through`; workflow 10 (byte-identical) |
| G12 | A copy shares no `NodeId` with its original, and nothing inside it points back into the original | workflow 07; acid duplicate |
| G13 | Undo walks back through every committed state exactly, and redo forward | invariants; acid undo-all |
| G14 | Children iterate by name; any other order is asked for by name | acid reads |
| G15 | No API writes transient values (modulated, metered, previewed, calculated) into the tree | by absence; reviewed, not tested |

---

## TypeScript

Generated, never hand-written (`node-api.md`, "Languages"). The same nouns and verbs in camelCase. Reads come from a **mirror**: a plain object tree the bindings keep current from `Commit`s, so React reads synchronously whichever transport carries the writes. Writes are `Op[]` through `edit` or a gesture, and return `Promise<Commit>` over Tauri, or plain `Commit` over wasm. The mirror is read-only by type. Clipboard copy is `extract` to text on the system clipboard; paste reads it back.

---

## Departures from `node-api.md`

- **Decision 3, "no document object".** `Tree` owns the nodes, the history, the saved snapshot and the registry handle, so something document-shaped exists. Reconciled: `Tree` is a container with no domain state of its own; there is no name, title or metadata outside nodes; and every caller still addresses through paths from `/`. `node-api.md` has the root carry the file it was read from as a value; the first slice doesn't, and the host keeps the path until something needs it in the tree.
- **Unknown types pass through** instead of refusing the file. Decided; recorded as decision 29 there. An undeclared category passes through the same way.
- **No roles in the core.** The earlier "Learned from Shard" proposed them; they are an object-model `check` (decision 35).
- **`ChangeKind::Bound`** widens from `{ slot, from, to }` to `{ on: On, source: NodeId, from: Option<values>, to: Option<values> }` on the target's entry, where `On` is `Slot(name)` or `Value(key)` (decision 32).

---

## Decided (Georg, 2026-10-02)

The four leans were taken (*"go with your leans, unless they are a one-way door"*). They are decisions 31 to 34 in `node-api.md`:

1. **Out-of-range writes are refused.** Load clamps instead, and reports it.
2. **One binding mechanism.** A bindable source declares the values a binding carries; a target is a slot (one source) or a value key (many).
3. **A write from another source during a gesture joins it.**
4. **A copy in the same file joins the original's groups.**

None is a one-way door. The near-one-way doors are the file format and the id encoding: the file is canonical JSON with a `"rhizome": 1` version field, and ids are ULIDs.

## The file format

```json
{
  "rhizome": 1,
  "nodes": [
    { "path": "/", "id": "…", "type": "root" },
    { "path": "/images/sky", "id": "…", "type": "image",
      "values": { "opacity": 0.8 },
      "refs": { "source": { "file": "sky.png" } },
      "orders": { "modifiers": ["…"] },
      "bindings": [{ "on": { "slot": "mask" }, "source": "…" }] }
  ]
}
```

One record per node, in path order; fields in a fixed order, empty ones left out; keys inside sorted. Here-references are written with the target's current path for people to read, and resolved by id on load. A fragment is the same shape with a `"fragment"` header naming its source file, the top nodes' parents, their outside groups and the paths of outside nodes it points at.

## Related

- [`node-api.md`](node-api.md): the model this is the API for.
- [`native-apps.md`](native-apps.md): undo and redo in the native menu; Edit › Copy, Paste and Duplicate.
- [`batched-mutations.md`](batched-mutations.md): `for_each`, now an edit over a query result.
- `projects/<app>/design/object-model.md`: each app's side of the line.
