set shell := ["bash", "-cu"]

default:
    @just --list

fmt:
    cargo fmt --all

lint:
    cargo fmt --all -- --check
    cargo clippy --all-targets --all-features -- -D warnings

build:
    cargo build

build-release:
    cargo auditable build --release

test:
    cargo test --all-features

deny:
    cargo deny check

audit:
    cargo audit

check: lint test

ci: lint deny test audit
    @echo "CI checks passed locally."

install-tools:
    command -v cargo-deny >/dev/null || cargo install cargo-deny --locked
    command -v cargo-audit >/dev/null || cargo install cargo-audit --locked
    command -v cargo-auditable >/dev/null || cargo install cargo-auditable --locked

refresh-schema-fixture:
    curl -sSf https://api.hubstaff.com/v2/docs -o tests/fixtures/schema.json
    INSTA_UPDATE=auto cargo test schema_command_table_snapshot -- --nocapture
