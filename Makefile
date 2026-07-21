.PHONY: build check docs docs-serve rust-check test web-check

build:
	cargo build --workspace
	npm run build

test:
	cargo test --workspace
	npm test

rust-check:
	cargo fmt --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

web-check:
	npm run check

docs:
	mkdocs build --strict

docs-serve:
	mkdocs serve

check: rust-check web-check docs
