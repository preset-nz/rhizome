# rhizome

The shared node API and per-app object models for the preset.nz desktop suite. A network of nodes with no fixed centre.

**Status: first slice built.** `crates/rhizome-core` holds the registry, the tree, edits with undo, diff, the file format, opaque nodes and copy/paste. `rhizome-wasm` and the generated TypeScript package wait for a second consumer.

## Tests

`just check` runs fmt, clippy and every suite.

- **Acid** (`tests/acid.rs`, `just acid`): builds one of everything through the Rust verbs and again through `Op`s, then operates every verb, refusal and cascade. The built file and the changeset log are goldens in `tests/golden/`.
- **Core workflows** (`tests/core_workflows.rs`, `just workflows`): twelve workflows a person does, stored as data in `tests/workflows/*.json` and driven only through the stable surface (`Op` JSON, the file format, edits, gestures, undo). Each has a transcript pinned beside it. A change to the SDK that alters a workflow fails here.
- **Invariants** (`tests/invariants.rs`, `just invariants`): seeded random `Op`s, checking after every step that nothing dangles, values fit their schema, a refused edit leaves no trace, a save reloads identically and undo walks back through every state.

`just bless` rewrites the goldens. Read every changed line before committing it.

## Design docs

`guidance/` holds **copies** of the planning docs so the work can continue away from the main machine. The canonical versions live in the private guidance repo (`design/node-api.md`, `design/rhizome-api.md`, `handovers/shard/handover-node-api.md`). Edit there, then recopy. Relative links inside the copies won't resolve.

- [`guidance/node-api.md`](guidance/node-api.md): the design (draft, 2026-09-17).
- [`guidance/rhizome-api.md`](guidance/rhizome-api.md): the API layout — verbs, shape, guarantees (draft, 2026-10-02).
- [`guidance/handover.md`](guidance/handover.md): code state and next steps.
- [`guidance/handover-node-api.md`](guidance/handover-node-api.md): session state and next steps.

## Licence

MIT.
