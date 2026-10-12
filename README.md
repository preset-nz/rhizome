# rhizome

The shared node API for the preset.nz desktop apps: mechanics only. A network of nodes with no fixed centre.

rhizome knows no app. It stores, validates, edits, undoes, diffs, saves and copies nodes of whatever types an app declares. Each app's **object model** holds its business logic, in the app's own repo.

**Status: first slice and POM phase 1 built.** `crates/rhizome-core` holds the mechanics: the registry, the tree, edits with undo, diff, the file format, opaque nodes, copy/paste, tree rules. `crates/rhizome-pom` is POM, the Preset Object Model: the base every app's object model is built on (documents, kinds with policy, presets, commands, projections). `crates/rhizome-pom-tauri` puts one open document behind Tauri commands and events. `@preset.nz/pom` (`src-ts/`) is the webview client of `rhizome-pom-tauri`.

## Install

```toml
[dependencies]
rhizome-core = "0.1"
rhizome-pom = "0.1"        # an object model on POM
rhizome-pom-tauri = "0.1"  # a Tauri 2 app with one open document
```

Take only what you need: `rhizome-pom` brings `rhizome-core` with it, and `rhizome-pom-tauri` brings both.

## TypeScript: `@preset.nz/pom`

The webview client of `rhizome-pom-tauri`: unbuilt TypeScript in `src-ts/`, versioned with the crates and not yet published. `react` and `@tauri-apps/api` are peer dependencies.

```ts
import { createChannel, createMirror, PomProvider, tauriBackend, useNode } from '@preset.nz/pom';

const channel = createChannel(tauriBackend); // commands reach Rust in the order they were made
const mirror = createMirror(tauriBackend);   // made outside React
await mirror.start();                        // connect, listen, then read the view

await channel.run('value.set', { at: '/threads/warp', key: 'thread.tension', value: 0.8 });
```

- **`PomBackend`** is the `pom_*` commands and the `pom://commit` and `pom://status` events, nothing else. `tauriBackend` implements it over Tauri; tests pass a fake.
- **`createChannel`** is one ordered queue. `run`, `begin`, `end` and `cancel` keep their order, so a gesture's `end` finds its token. `drag` is latest-wins, for scrubs and transforms only. The gate starts open; `{ held: true }` shuts it until `release()`.
- **`createMirror`** keeps the rows by id with a path index, from the view and the commits that follow. It re-reads the view on a gap in `seq` or a new `generation`.
- **Hooks**, through `useSyncExternalStore`: `useNode(id)`, `useNodeAt(path)`, `useChildren(id, order)` and `useStatus()`, under a `<PomProvider mirror={mirror}>`. A hook returns the same object until its own node changes.
- **Wire types** (`Row`, `View`, `Update`, `Status`, `Ran`, ...) follow the Rust structs. The tests read the Rust goldens, so a shape that moves in one half fails the other's build.

## Tests

`just check` runs `check-rust` (fmt, clippy and every suite) and `check-ts` (tsc, Biome and vitest), then the licence check.

`just check` runs fmt, clippy and every suite.

- **Acid** (`tests/acid.rs`, `just acid`): builds one of everything through the Rust verbs and again through `Op`s, then operates every verb, refusal and cascade. The built file and the changeset log are goldens in `tests/golden/`.
- **Core workflows** (`tests/core_workflows.rs`, `just workflows`): twelve workflows a person does, stored as data in `tests/workflows/*.json` and driven only through the stable surface (`Op` JSON, the file format, edits, gestures, undo). Each has a transcript pinned beside it. A change to the SDK that alters a workflow fails here.
- **POM** (`crates/rhizome-pom/tests/pom.rs`, `just pom`) and **POM's core workflows** (`tests/workflows.rs`, data in `tests/workflows/*.json`): two made-up object models, and six workflows run through commands by id.
- **Wire** (`crates/rhizome-pom/tests/wire.rs`): the `View`, `Update`, `Status`, `Ran` and `CommandState` JSON a transport carries, pinned in `crates/rhizome-pom/tests/golden/*.json`. With core's `commit.json` and `view.json` they are what the TypeScript tests (`tests/`, `just test-ts`) read.
- **Invariants** (`tests/invariants.rs`, `just invariants`): seeded random `Op`s, checking after every step that nothing dangles, values fit their schema, a refused edit leaves no trace, a save reloads identically and undo walks back through every state.

`just bless` rewrites the goldens. Read every changed line before committing it.

## Design docs

`guidance/` holds **copies** of the planning docs so the work can continue away from the main machine. The canonical versions live in the private guidance repo (`design/node-api.md`, `design/rhizome-api.md`, `design/pom.md`, `handovers/rhizome/handover.md`). Edit there, then recopy. Relative links inside the copies won't resolve.

- [`guidance/node-api.md`](guidance/node-api.md): the model and decisions 1 to 35.
- [`guidance/rhizome-api.md`](guidance/rhizome-api.md): the API as built — verbs, shape, guarantees, and the line between rhizome and an object model.
- [`guidance/pom.md`](guidance/pom.md): POM, the Preset Object Model every app's object model builds on (draft).
- [`guidance/handover.md`](guidance/handover.md): code state and next steps.

## Licence

MIT.
