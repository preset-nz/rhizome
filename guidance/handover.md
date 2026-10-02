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

- **Everything pushed** (2026-10-02, both repos). Tags `v0.1.0`–`v0.1.3` (`v0.1.3`: `ObjectModel::seed` / `Tree::seeded`, a new document's content outside history). Oblique depends on `v0.1.3` by private git. Georg: push without asking (memory `push-without-asking`); still ask before publishing or going public.
- **rhizome-core: shipped** (epic 01). Registry, tree, edits/gestures/coalescing, undo, diff, file format, opaque nodes, copy/paste, `Op` as data, tree rules. `5d86ea8`: `Commit`/`Changeset`/`Value` serialise; shape pinned in `tests/golden/commit.json` (`tests/wire.rs`).
- **rhizome-pom phase 1: shipped.** Kinds and policy, presets on the kind (catalogue, user presets, apply, current, `add_from_preset`), preset files (`0f9edc2`: flat JSON, one kind, bindings stripped), built-in commands. Themes removed (`0d54204`, decision 39).
- **POM phase 2: shipped** (`4c750a1`, `bed6ae2`). `Pom<M>` / `Host` in `rhizome-pom/src/host.rs` (no Tauri): commands only, coalesce key, gestures by token, `Status` with a monotonic `generation`, events returned not emitted, `on_change` hook. `value.set` built in. `crates/rhizome-pom-tauri`: app-level commands `commands::pom_*`, events `pom://commit|status|open-document`, `opened.rs` lifted from Shard.
- **Decision 48 built** (`ef8f202` core rows/schema/patch + mirror invariant; `766528e` POM `View`, `Update` events, `pom_view`, `values.set` batch).
- **Oblique spike done** (decision 49): coverage, round trip and drag measured; findings in `projects/oblique/design/object-model.md`; migration planned as Oblique Epic 21.
- **Oblique coverage** (decisions 50–52): coverage map in `projects/oblique/design/object-model.md`; the three gaps closed: `cce5d31` (shaped values, floats, unbounded floats), `a4a3e67` (`ObjectModel::Context`, kinds from runtime data).
- **From the spike, in rhizome:** `925f6ef` floats survive the file bit for bit (`float_roundtrip`); `86b7a0d` soft max (decision 53). Decision 54: Oblique's transport is Tauri commands.
- **Tests:** `just check` green. pom.rs 12, host.rs 6, POM workflows 6, ipc.rs 2 (Tauri mock runtime), opened 4, wire 1.
- **Decisions 39–54** in `design/node-api.md`. 46: commands only over a transport. 47: a saved rhizome is a scene description (IFD/RIB); one app's node families per document. 48: the mirror is DTOs, snapshot then patches. 49: a shared platform; Oblique adopts first via a spike.
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

1. **Oblique Epic 21 story 1 is in flight** in the Oblique repo (branch `epic21/story-1`); its state lives in `handovers/oblique/handover.md`. rhizome changes it needs: tag a new version, bump Oblique's tag.
2. `@preset.nz/pom` (phase 3) is Epic 21's story 8: extract from Oblique.
3. Cross-file `Ref` resolution (decision 40) before any app loads a library rhizome.
4. Ops (decisions 43–44): direction only.
5. After any doc change: recopy into `packages/rhizome/guidance/`, commit and push both repos.

## Gotchas

- **rhizome's names are rhizome's** (memory `rhizome-is-fundamental`): never rename a rhizome verb around an app's vocabulary; apps namespace their own (Oblique `params.<name>`).
- **No existing files matter** (Georg): no compatibility code or converters in Oblique.

- **Tauri `#[tauri::command] pub fn` can't sit at a lib crate's root** (macro name clash); they live in `commands`.
- **Mock-runtime IPC must use the webview's own URL** as the request URL, else "not allowed. Plugin not found" (non-local origin).
- **`generation` must fold in the outgoing document's reverts** on replace, or it repeats (a test caught it).
- **Don't rebuild themes in POM** or propose renaming "theme"; Georg moved them to apps.
- **Vocabulary:** rhizome, node API, object model, POM, kind, category, group, smart group, loaded, calculated, preset, theme, op. "scene" is dropped as a type name (Georg uses "scene/app graph" informally).
- **Nothing app-named in rhizome or POM** (decision 35). Test models are made up (Synth, Atlas, Gazetteer, Loom).
- **`just bless` then read every line.** Unique names are `name-2`. cargo fmt rewraps lines; re-read before scripted patches.
- Sequential ids in tests; `serde_json` must not get `preserve_order`.
