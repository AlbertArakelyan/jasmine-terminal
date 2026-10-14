# Development tasks. Run `just` for the full CI check.

default: lint test

fmt:
    cargo fmt --all

lint:
    cargo fmt --all --check
    cargo clippy --all-targets -- -D warnings

test:
    cargo test

run:
    cargo run
