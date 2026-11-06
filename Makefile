.PHONY: help fmt fmt-check clippy test test-services openapi openapi-check authz-check deny vendor-id \
	vendor-check verify

help: ## This list
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | sed 's/:.*## /\t/'

# Not --all: that also formats the path dependencies, which are the vendored copies.
fmt: ## Format the Rust code
	cargo fmt

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test-services: ## PostgreSQL for the tests (25435)
	scripts/test-services.sh

test: test-services ## Every test
	cargo test --workspace

# Writes the OpenAPI document; a test fails when the committed copy is out of date.
openapi: ## The OpenAPI document, from the code
	UPDATE_OPENAPI=1 cargo test -q -p pacod-agg --lib openapi

openapi-check:
	cargo test -q -p pacod-agg --lib openapi

# The vendored pacod-id is a commit's, as its VERSION says.
vendor-id: ## pacod-id at its HEAD
	scripts/vendor-pacod-id.sh

vendor-check: ## The vendored copies are committed
	@git diff --quiet HEAD -- vendor ui/vendor || { echo "vendor/ differs from what is committed" >&2; exit 1; }

# Licences, banned crates, sources and advisories of the Rust dependencies (deny.toml).
# Needs cargo-deny: cargo install cargo-deny --locked
deny: ## cargo deny
	cargo deny --locked check

verify: fmt-check clippy test openapi-check deny vendor-check ## Every check before a commit
