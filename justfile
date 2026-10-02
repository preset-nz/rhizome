default:
    @just --list

# Format, lint and test: the fast signal
[group('quality')]
check:
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings
    cargo test

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

# Run the seeded random-Op invariant loop
[group('quality')]
invariants:
    cargo test -p rhizome-core --test invariants -- --nocapture

# Regenerate golden files. Read every changed golden before committing
[group('quality')]
bless:
    RHIZOME_BLESS=1 cargo test -p rhizome-core --test acid --test core_workflows --test wire
    RHIZOME_BLESS=1 cargo test -p rhizome-pom --test workflows
    git status --short crates/*/tests

# Format the code
[group('dev')]
fmt:
    cargo fmt --all
