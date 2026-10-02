---
title: Handover — rhizome
type: handover
status: current
repo: /Users/georg/rhizomatic-preset/packages/rhizome
branch: main
project: rhizome
topic: rhizome-core, rhizome-pom, rhizome-pom-tauri built; next the Oblique spike (first adopter).
updated: 2026-10-02
---

# Handover — rhizome

## Current status

- **Everything pushed** (2026-10-02, both repos). Georg: push without asking (memory `push-without-asking`); still ask before publishing or going public.
- **rhizome-core: shipped** (epic 01). Registry, tree, edits/gestures/coalescing, undo, diff, file format, opaque nodes, copy/paste, `Op` as data, tree rules. `5d86ea8`: `Commit`/`Changeset`/`Value` serialise; shape pinned in `tests/golden/commit.json` (`tests/wire.rs`).
- **rhizome-pom phase 1: shipped.** Kinds and policy, presets on the kind (catalogue, user presets, apply, current, `add_from_preset`), preset files (`0f9edc2`: flat JSON, one kind, bindings stripped), built-in commands. Themes removed (`0d54204`, decision 39).
- **POM phase 2: shipped** (`4c750a1`, `bed6ae2`). `Pom<M>` / `Host` in `rhizome-pom/src/host.rs` (no Tauri): commands only, coalesce key, gestures by token, `Status` with a monotonic `generation`, events returned not emitted, `on_change` hook. `value.set` built in. `crates/rhizome-pom-tauri`: app-level commands `commands::pom_*`, events `pom://commit|status|open-document`, `opened.rs` lifted from Shard.
- **Decision 48 built** (`ef8f202` core rows/schema/patch + mirror invariant; `766528e` POM `View`, `Update` events, `pom_view`, `values.set` batch).
- **Oblique coverage** (decisions 50–52): coverage map in `projects/oblique/design/object-model.md`; the three gaps closed: `cce5d31` (shaped values, floats, unbounded floats), `a4a3e67` (`ObjectModel::Context`, kinds from runtime data).
- **Tests:** `just check` green. pom.rs 12, host.rs 6, POM workflows 6, ipc.rs 2 (Tauri mock runtime), opened 4, wire 1.
- **Decisions 39–52** in `design/node-api.md`. 46: commands only over a transport. 47: a saved rhizome is a scene description (IFD/RIB); one app's node families per document. 48: the mirror is DTOs, snapshot then patches. 49: a shared platform; Oblique adopts first via a spike.
- **Georg is unsure we're fully on the same page** about the model (2026-10-02). His framing: rhizome on disk = IFD/RIB, restores exact state, feeds the app graph, all edits through the object model. Decisions 46–47 record it; keep checking new work against that framing.
- Georg mentioned Swift FOMO (a friend's Swift app). Answered: stay on Rust/Tauri; the commands-only boundary would let a SwiftUI shell sit on the same `Pom<M>` via UniFFI later. No action.

## References

- `~/rhizomatic-preset/guidance/design/node-api.md` — decisions 1–47.
- `~/rhizomatic-preset/guidance/design/pom.md` — POM as built; "Over a transport" = phase 2.
- `~/rhizomatic-preset/guidance/design/rhizome-api.md` — core API; TypeScript section now points at decision 46.
- `~/rhizomatic-preset/guidance/design/native-apps.md` — rules the shell serves (menu, undo, restore, Finder-open ordering).
- `~/rhizomatic-preset/guidance/design/tauri-scaffold.md` — `native-menu`, `window-restore`, where `@preset.nz/pom` fits.
- `~/rhizomatic-preset/guidance/projects/shard/design/object-model.md` — first adopter's plan.
- `crates/rhizome-pom/src/host.rs`, `crates/rhizome-pom-tauri/src/{lib,commands,opened}.rs`.
- `packages/rhizome/guidance/` — **copies** of node-api, rhizome-api, pom, handover. Recopy after editing.

## Next steps

1. **The Oblique spike** (decision 49; plan and coverage map in `projects/oblique/design/object-model.md`). With 50–52 in, the spike's Rust object model can hold the whole scene (document node, layers with modifiers as op kinds from `list_ops`, vector paths as `shaped`, masks as bindings); the adapter reads the whole scene; writes rewired for the three interactions only. Worktree of `initiatives/oblique` on `spike/rhizome`, path deps on `packages/rhizome/crates/{rhizome-pom,rhizome-pom-tauri}`. Slice: pixel layers. Rewire canvas move drag, opacity slider, save/open; an adapter rebuilds Oblique's `Scene` from the mirror. Before writing: read `src/scene/types.ts`, `store.ts`, `history.ts`, the dependency-cruiser config, project io.
   - Protect real projects: spike documents use their own extension or a scratch folder; no opening existing `.oblique` files.
   - Cmd+Z for the slice routes to `edit.undo`; adapter writes stay out of Oblique's `history.ts`.
   - The adapter keeps object identity for untouched nodes (or `sceneDiff`/prefix cache invalidate everything and the latency number lies).
   - A position as one `Vec2` key makes a canvas drag one `value.set` with a coalesce key.
   - Measure: canvas drag feel and round-trip time, slider, undo of a drag as one step, save/reopen. Findings into Oblique's object-model doc; that decides the transport (decision 28).
2. The spike's mirror is the first draft of `@preset.nz/pom` (phase 3); lift it after, shaped by Oblique.
3. Cross-file `Ref` resolution (decision 40) before any app loads a library rhizome.
4. Ops (decisions 43–44): direction only, don't build unasked.
5. After any doc change: recopy into `packages/rhizome/guidance/`, commit and push both repos.

## Gotchas

- **Tauri `#[tauri::command] pub fn` can't sit at a lib crate's root** (macro name clash); they live in `commands`.
- **Mock-runtime IPC must use the webview's own URL** as the request URL, else "not allowed. Plugin not found" (non-local origin).
- **`generation` must fold in the outgoing document's reverts** on replace, or it repeats (a test caught it).
- **Don't rebuild themes in POM** or propose renaming "theme"; Georg moved them to apps.
- **Vocabulary:** rhizome, node API, object model, POM, kind, category, group, smart group, loaded, calculated, preset, theme, op. "scene" is dropped as a type name (Georg uses "scene/app graph" informally).
- **Nothing app-named in rhizome or POM** (decision 35). Test models are made up (Synth, Atlas, Gazetteer, Loom).
- **`just bless` then read every line.** Unique names are `name-2`. cargo fmt rewraps lines; re-read before scripted patches.
- Sequential ids in tests; `serde_json` must not get `preserve_order`.
