# rhizome

Shared node API for the desktop suite (Shard first, then Oblique, Strata, Map & Territory). Own repo under `preset-nz`, no monorepo.

- `guidance/` holds read-only **copies** of the canonical planning docs from the guidance repo. Read `guidance/node-api.md` and `guidance/handover-node-api.md` first. If you change a decision, say so in the commit and tell Georg so the canonical docs get updated.
- Use Georg's vocabulary: rhizome, node API, object model, category, group, smart group, loaded, calculated, kind. Don't rename to industry terms; "scene" is dropped.
- Plan: `rhizome-core` first (NodeId and paths, node-type registry, categories, nodes with values, references, groups, stored order, bindings, `serialise`/`diff`). Tests assert contracts.
- MIT. NZ English in user-facing strings. Ask before pushing or publishing.
