.PHONY: help fmt fmt-check clippy test deny verify

help: ## This list
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | sed 's/:.*## /\t/'

fmt: ## Format the Rust code
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test: ## Every test
	cargo test --workspace

# Licences, banned crates, sources and advisories of the Rust dependencies (deny.toml).
# Needs cargo-deny: cargo install cargo-deny --locked
deny: ## cargo deny
	cargo deny --locked check

verify: fmt-check clippy test deny ## Every check before a commit
