# rhizome

The shared node API and POM. Own repo under `preset-nz`. Three crates and one npm package (`@preset.nz/pom`, `src-ts/`, the webview client of `rhizome-pom-tauri`; no Tauri in the crates, no app in the package): `crates/rhizome-core` (mechanics), `crates/rhizome-pom` (POM, the Preset Object Model: the base every app's object model is built on; `guidance/pom.md`) and `crates/rhizome-pom-tauri` (one open document over Tauri: commands only, events, the opened-from-Finder hand-off). The docs generator moved to `packages/preset-docs` (2026-10-14). Keep Tauri out of all but `rhizome-pom-tauri`.

**rhizome and POM are unaware of the apps** (decision 35). Code, tests and docs here name no app and hold no domain rule. Each app's **object model** (node types, rules as `check`s, domain verbs, projections) lives in the app's repo and is planned in the guidance repo's `projects/<app>/design/object-model.md`. If a change here is only needed by one app, it belongs in that app's object model.

- **Read first:** `guidance/handover.md` (state, next steps), then `guidance/rhizome-api.md` (the API as built) and `guidance/node-api.md` (the model, decisions 1 to 45). The `guidance/*.md` files here are stale copies; the canonical docs are `~/rhizomatic-preset/guidance/design/{node-api,rhizome-api,pom}.md` and `handovers/rhizome/handover.md`. If you change a decision, say so in the commit and update the canonical doc.
- **`just check`** (`check-rust`: fmt, clippy, every Rust suite; `check-ts`: tsc, Biome, vitest; then licences) is the signal. Suites:
  - acid (`tests/acid.rs`): one of everything, built and operated;
  - core workflows (`tests/core_workflows.rs`, data in `tests/workflows/*.json`): driven only through `Op` JSON, the file format, edits, gestures and undo. Its registry is **frozen**; changing it changes every transcript;
  - invariants (`tests/invariants.rs`): seeded random `Op`s;
  - wire (`tests/wire.rs`): a commit's JSON shape, pinned in `tests/golden/commit.json` and `view.json`; the TypeScript half's tests read them;
  - POM (`crates/rhizome-pom/tests/pom.rs`), the host a transport drives (`tests/host.rs`), and POM's core workflows (`tests/workflows.rs`, data in `tests/workflows/*.json`), driven through commands by id. Its model is frozen too;
  - POM's wire (`crates/rhizome-pom/tests/wire.rs`): `View`, `Update`, `Status`, `Ran` and the command list, pinned in `crates/rhizome-pom/tests/golden/*.json`;
  - the TypeScript package (`tests/*.test.ts`, `just test-ts`): channel, mirror and hooks against a fake `PomBackend`, plus `tests/wire.test.ts`, which reads core's `commit.json` and `view.json` and POM's goldens so a drift between `src-ts/wire.ts` and the Rust structs fails the build. Change a wire struct, bless, and update `wire.ts` in the same commit;
  - the Tauri transport (`crates/rhizome-pom-tauri/tests/ipc.rs`): real IPC and events over Tauri's mock runtime;
- **`just bless`** rewrites goldens and transcripts (POM's wire goldens included). Read every changed line before committing; blessing whatever the code prints proves nothing.
- Use Georg's vocabulary: rhizome, node API, object model, category, group, smart group, loaded, calculated, kind. Don't rename to industry terms; "scene" is dropped.
- Whether a mechanism moves into rhizome is decided per case (decision 30, [family-alignment rule 6](../../guidance/design/family-alignment.md#rules)). Test fixtures are made-up object models, not copies of an app's.
- MIT. NZ English in user-facing strings. Push freely; ask before publishing or making the repo public.
