---
title: Handover — shard (rhizome node API)
type: handover
status: current
repo: /Users/georg/rhizomatic-preset/initiatives/shard
branch: main
project: shard
topic: The shared rhizome node API and Shard's multi-patch as its first consumer. Planning only; nothing scaffolded.
updated: 2026-09-13
---

# Handover — shard (rhizome node API)

Parallel workstream to `handover.md` in this folder, which covers Shard's day-to-day engine and UI work. This file is the cross-project architecture thread: the apps grew in parallel and 2026-09-13 was the first attempt to consolidate them.

## Current status

- **Review done, decisions recorded.** `design/node-api.md` was reviewed against Oblique, Fault and Shard code on 2026-09-13 and Georg answered eight questions. Decisions 20 to 28 are in the doc. Guidance commit `89992b7`.
- **Shard roadmap updated** in the same commit: row 3 undo decided for Rust, row 5 id rename precedes multi-patch, row 6 "categories" became "kinds", row 10 carries the multi-patch decisions.
- **Nothing scaffolded.** No `packages/rhizome` repo exists. Georg's instruction stands: plan, then a module, no per-app refactor churn.
- **Still open** (listed at the end of `node-api.md`): cross-file index waits on Strata storage; Oblique transport decided when Oblique migrates; home of the native-menu wiring; whether Strata's `usePersistedState` exists.
- **Shard code moved during the session** on the other workstream: allocation guard, block timer, folding sections, octave, and granular-as-effect shipped. **Decision 26 is done:** `64b100b` renamed `mix.dry` to `grain.mix`, so that prerequisite for multi-patch is cleared.
- **"Learned from Shard" section added to `node-api.md`** (2026-09-13), so rhizome does not rediscover:
  - node roles, with the effect pattern (`<node>.on`, then Mix first, no repeated node name)
  - presets as a generic node-level feature
  - bindings that target value keys and carry a depth (LFO links)
  - instances over flat tables
  - modulation never writing a stored value
  - "the audio thread only swaps things in", with its two traps
  - stepped values as values, not indices
  - UI state outliving remounts
  - renames being cheap while young
- **Decision 24 superseded:** Shard retires drift for LFOs (`projects/shard/design/modulation.md`). Two new open questions at the end of `node-api.md`: the set of node roles, and the binding shape for value-key targets with a depth.
- **There are no old patches** (Georg). Don't design migrations or compatibility for Shard files; see memory `shard-no-old-patches`.
- Georg owns this investigation with me: "you own this investigation and architecture."

## References

- `~/rhizomatic-preset/guidance/design/node-api.md`: the plan. Read "Decided" 20 to 28, "Node types", "Diff and dirty", "Undo", "Shard's audio thread", "Cross-file references" first.
- `~/rhizomatic-preset/guidance/design/native-apps.md` rule 5: settings module, three value lifetimes.
- `~/rhizomatic-preset/guidance/projects/shard/features/roadmap.md` rows 3, 5, 6, 10.
- `~/rhizomatic-preset/guidance/projects/shard/design/document-model.md`: patch vs arrangement, "never copies".
- Shard code the plan leans on: `crates/shard-dsp/src/params.rs` (`ParamBank`, `PARAMS`, `index_of`), `crates/shard-dsp/src/engine.rs` (`Slots::resolve`), `src-tauri/src/patch.rs` (`LoadReport`), `src-tauri/src/lib.rs` (`set_param`, swap slots), `src-tauri/src/drift.rs`.
- Precedents: Oblique `src/scene/{types,store,history,sceneDiff}.ts`; Fault `crates/mesh-ops/src/history.rs`, `crates/mesh-wasm/src/document.rs` (never compact slots).
- Memory: `rhizome-apps-define-own-nodes.md`, `own-vocabulary-not-employer-terms.md`, `rhizomatic-preset-not-a-monorepo.md`.

## Next steps

1. Confirm with Georg that the plan is ready to leave planning. If yes, scaffold `~/rhizomatic-preset/packages/rhizome/` as its own git repo with `rhizome-core` only; `rhizome-wasm` and the npm package wait for a second consumer. Path dependency from Shard.
2. First slice of `rhizome-core`, in this order: `NodeId` and paths; node-type registry with value schema, origin and **role** (read "Learned from Shard" first); categories; nodes with values; references; groups; one stored order per node; bindings; `serialise` and `diff`. Tests assert contracts (a rename diffs as a move, a reorder is one entry, unknown type on load is reported not swallowed).
3. In Shard: `ShardObjectModel` in `src-tauri` declaring patch, granular, crush, ring, envelope, material. Tree owns values; `ParamBank::for_plan` compiled from it; undo in Rust. Row 3 splits into stories here.
4. Plan swap with whole-plan crossfade, held to zero by `tests/audio_thread.rs`. Retired plan returns through a second slot.
5. Open decisions for Georg when they bite: Oblique transport, menu package home, Strata storage before any cross-file index.

## Gotchas

- Long `cat` output in this environment gets saved to a file instead of shown; use the `Read` tool on the saved path. An unquoted `=====` in zsh is an `=cmd` expansion and errors.
- `im` (persistent collections) is MPL-2.0 and fails the licence gate; `rpds` is MIT/Apache if snapshots ever need sharing.
- Fault's guidance `architecture.md` describes groups, scatter and a modifier stack that are not built. Cite the crates, not the doc.
- Georg's vocabulary: rhizome, node API, object model, category, group, smart group, loaded, calculated, kind. Do not rename to industry terms.
