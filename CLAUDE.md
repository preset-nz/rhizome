# rhizome

The shared node API: mechanics only. Own repo under `preset-nz`, no monorepo. One crate so far: `crates/rhizome-core`.

**rhizome is unaware of the apps** (decision 35). Code, tests and docs here name no app and hold no domain rule. Each app's **object model** (node types, rules as `check`s, domain verbs, projections) lives in the app's repo and is planned in the guidance repo's `projects/<app>/design/object-model.md`. If a change here is only needed by one app, it belongs in that app's object model.

- **Read first:** `guidance/handover.md` (state, next steps), then `guidance/rhizome-api.md` (the API as built) and `guidance/node-api.md` (the model, decisions 1 to 35). These are **copies**; the canonical docs live in the guidance repo. If you change a decision, say so in the commit and update the canonical doc, then recopy.
- **`just check`** (fmt, clippy, every suite) is the signal. Three suites:
  - acid (`tests/acid.rs`): one of everything, built and operated;
  - core workflows (`tests/core_workflows.rs`, data in `tests/workflows/*.json`): driven only through `Op` JSON, the file format, edits, gestures and undo. Its registry is **frozen**; changing it changes every transcript;
  - invariants (`tests/invariants.rs`): seeded random `Op`s.
- **`just bless`** rewrites goldens and transcripts. Read every changed line before committing; blessing whatever the code prints proves nothing.
- Use Georg's vocabulary: rhizome, node API, object model, category, group, smart group, loaded, calculated, kind. Don't rename to industry terms; "scene" is dropped.
- A mechanism moves into rhizome only when two object models need it (decision 30). Test fixtures are made-up object models, not copies of an app's.
- MIT. NZ English in user-facing strings. Ask before pushing or publishing.
