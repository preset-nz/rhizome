default:
    @just --list

# Fetch dependencies and install the git hooks
[group('setup')]
install:
    cargo fetch
    pnpm install
    ./node_modules/.bin/lefthook install

# Format, lint and test: the fast signal
[group('quality')]
check: check-rust check-ts
    just licences

# Rust: format, lint, every suite.
[group('quality')]
check-rust:
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo test

# TypeScript (src-ts/): typecheck, Biome (lint and format check), vitest. The tests read the
# Rust goldens, so a wire shape that moved fails here.
[group('quality')]
check-ts:
    ./node_modules/.bin/tsc --noEmit
    ./node_modules/.bin/biome check .
    ./node_modules/.bin/vitest run

# Run the tests
[group('quality')]
test *args:
    cargo test {{args}}

# Run the acid test
[group('quality')]
acid:
    cargo test -p rhizome-core --test acid

# Run both core-workflow suites: rhizome's and POM's
[group('quality')]
workflows:
    cargo test -p rhizome-core --test core_workflows
    cargo test -p rhizome-pom --test workflows

# Run POM's tests, the Tauri transport's included
[group('quality')]
pom:
    cargo test -p rhizome-pom -p rhizome-pom-tauri

# Run the TypeScript tests
[group('quality')]
test-ts *args:
    ./node_modules/.bin/vitest run {{args}}

# Run the seeded random-Op invariant loop
[group('quality')]
invariants:
    cargo test -p rhizome-core --test invariants -- --nocapture

# Regenerate golden files. Read every changed golden before committing
[group('quality')]
bless:
    RHIZOME_BLESS=1 cargo test -p rhizome-core --test acid --test core_workflows --test wire
    RHIZOME_BLESS=1 cargo test -p rhizome-pom --test workflows --test wire
    git status --short crates/*/tests

# Format the code (Biome, cargo fmt)
[group('dev')]
fmt:
    ./node_modules/.bin/biome check --write .
    cargo fmt --all

# Licence check against the committed lock. Reads files only, no network.
# Re-resolve with `preset-compliance licences scan` after changing dependencies.
[group('quality')]
licences:
    preset-compliance licences check

# Version, changelog, commit and tag from the conventional commits since the last
# tag (knope.toml). Push and publish stay by hand.
[group('build')]
release:
    knope release

# What `release` would do, without touching anything.
[group('build')]
release-preview:
    knope release --dry-run
