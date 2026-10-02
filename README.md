# rhizome

The shared node API for the preset.nz desktop apps: mechanics only. A network of nodes with no fixed centre.

rhizome knows no app. It stores, validates, edits, undoes, diffs, saves and copies nodes of whatever types an app declares. Each app's **object model** holds its business logic, in the app's own repo.

**Status: first slice built.** `crates/rhizome-core` holds the registry, the tree, edits with undo, diff, the file format, opaque nodes and copy/paste. `rhizome-wasm` and the generated TypeScript package wait for a second consumer.

## Tests

`just check` runs fmt, clippy and every suite.

- **Acid** (`tests/acid.rs`, `just acid`): builds one of everything through the Rust verbs and again through `Op`s, then operates every verb, refusal and cascade. The built file and the changeset log are goldens in `tests/golden/`.
- **Core workflows** (`tests/core_workflows.rs`, `just workflows`): twelve workflows a person does, stored as data in `tests/workflows/*.json` and driven only through the stable surface (`Op` JSON, the file format, edits, gestures, undo). Each has a transcript pinned beside it. A change to the SDK that alters a workflow fails here.
- **Invariants** (`tests/invariants.rs`, `just invariants`): seeded random `Op`s, checking after every step that nothing dangles, values fit their schema, a refused edit leaves no trace, a save reloads identically and undo walks back through every state.

`just bless` rewrites the goldens. Read every changed line before committing it.

## Design docs

`guidance/` holds **copies** of the planning docs so the work can continue away from the main machine. The canonical versions live in the private guidance repo (`design/node-api.md`, `design/rhizome-api.md`, `handovers/rhizome/handover.md`). Edit there, then recopy. Relative links inside the copies won't resolve.

- [`guidance/node-api.md`](guidance/node-api.md): the model and decisions 1 to 35.
- [`guidance/rhizome-api.md`](guidance/rhizome-api.md): the API as built — verbs, shape, guarantees, and the line between rhizome and an object model.
- [`guidance/handover.md`](guidance/handover.md): code state and next steps.

## Licence

MIT.
