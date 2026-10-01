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
    cargo test --test acid

# Run the core-workflow suite
[group('quality')]
workflows:
    cargo test --test core_workflows

# Regenerate golden files. Read every changed golden before committing
[group('quality')]
bless:
    RHIZOME_BLESS=1 cargo test --test acid --test core_workflows
    git status --short crates/rhizome-core/tests

# Format the code
[group('dev')]
fmt:
    cargo fmt --all
