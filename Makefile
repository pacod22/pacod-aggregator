.PHONY: help fmt fmt-check clippy test test-services openapi openapi-check authz authz-check ui-check \
	deny vendor-id vendor-check verify network images up dev-up down smoke deploy-check

COMPOSE := docker compose -f deploy/docker-compose.yml -f deploy/docker-compose.standalone.yml
DEV := $(COMPOSE) -f deploy/docker-compose.dev.yml
VERSION ?= 0.1.0
PROMTOOL := docker run --rm -v "$$PWD/deploy/prometheus:/prometheus-files:ro" --entrypoint promtool prom/prometheus:v3.15.0

help: ## This list
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | sed 's/:.*## /\t/'

# Not --all: that also formats the path dependencies, which are the vendored copies.
fmt: ## Format the Rust code
	cargo fmt

fmt-check:
	cargo fmt --check

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

test-services: ## PostgreSQL (25435) and a browser (29150) for the tests
	scripts/test-services.sh

test: test-services ## Every test
	AGG_TEST_BROWSER_URL=$${AGG_TEST_BROWSER_URL:-http://127.0.0.1:29150} cargo test --workspace

# Writes the OpenAPI document; a test fails when the committed copy is out of date.
openapi: ## The OpenAPI document, from the code, and the console's types, from the document
	UPDATE_OPENAPI=1 cargo test -q -p pacod-agg --lib openapi
	cd ui && npm run api:types

openapi-check: ## Both are what the code says
	cargo test -q -p pacod-agg --lib openapi
	cd ui && npm run api:types >/dev/null
	git diff --exit-code -- ui/src/lib/api/schema.d.ts

authz: ## The console's permissions, from authz/agg.yaml
	cd ui && npm run authz:gen

authz-check:
	cd ui && npm run authz:gen >/dev/null
	git diff --exit-code -- ui/src/lib/authz.generated.ts

ui-check: ## The console: format, types, unit tests, build
	cd ui && npm ci --silent && npm run lint && npm run check && npm test && npm run build

# The vendored pacod-id is a commit's, as its VERSION says.
vendor-id: ## pacod-id at its HEAD
	scripts/vendor-pacod-id.sh

vendor-check: ## The vendored copies are committed
	@git diff --quiet HEAD -- vendor ui/vendor || { echo "vendor/ differs from what is committed" >&2; exit 1; }

# Licences, banned crates, sources and advisories of the Rust dependencies (deny.toml).
# Needs cargo-deny: cargo install cargo-deny --locked
deny: ## cargo deny
	cargo deny --locked check

verify: fmt-check clippy test openapi-check authz-check ui-check deny vendor-check ## Every check before a commit

network: ## The shared networks (pacod-demo/docs/NETWORK.md); pacod-admin-net is internal
	@docker network inspect pacod-net >/dev/null 2>&1 || docker network create pacod-net >/dev/null
	@docker network inspect pacod-admin-net >/dev/null 2>&1 || docker network create --internal pacod-admin-net >/dev/null
	@test "$$(docker network inspect -f '{{.Internal}}' pacod-admin-net)" = true || { echo "pacod-admin-net must be internal"; exit 1; }

images: ## The server's, the scraper's, the browser's and the console's images
	docker build -f deploy/docker/rust.Dockerfile --target server --build-arg VERSION=$(VERSION) -t pacod-agg:$(VERSION) .
	docker build -f deploy/docker/rust.Dockerfile --target scraper --build-arg VERSION=$(VERSION) -t pacod-agg-scraper:$(VERSION) .
	docker build -f deploy/docker/browser.Dockerfile --build-arg VERSION=$(VERSION) --build-arg CHROMIUM_DAY=$$(date -u +%F) -t pacod-agg-browser:$(VERSION) .
	docker build -f deploy/docker/ui.Dockerfile --target ui --build-arg VERSION=$(VERSION) -t pacod-agg-console:$(VERSION) .

up: network ## The aggregator with its own pacod-id, database, edge and monitoring
	$(COMPOSE) up -d --wait

dev-up: network ## From the working tree, with the mock vendor; docker-compose.dev.yml lists the ports
	$(DEV) up -d --build --wait

down: ## Stop whatever of the above runs
	$(DEV) down

smoke: ## End to end against the development stack, signing in through pacod-id
	cargo run -q -p pacod-agg-testkit --bin agg-smoke

# The app file names shared components it depends on, so it is valid only together with a file
# that defines them: the standalone file here, pacod-demo's in the full stack.
deploy-check: ## The compose files, the edge, the alert rules and their tests, and the dashboard
	$(COMPOSE) config -q
	$(DEV) config -q
	$(PROMTOOL) check rules /prometheus-files/rules/agg.rules.yml
	$(PROMTOOL) test rules /prometheus-files/tests/agg.rules.test.yml
	docker run --rm -v "$$PWD/deploy/caddy/Caddyfile:/etc/caddy/Caddyfile:ro" -v "$$PWD/platform/caddy:/etc/caddy/sites:ro" caddy:2.11.6-alpine caddy validate --config /etc/caddy/Caddyfile
	python3 -c 'import json; json.load(open("deploy/grafana/dashboards/agg-overview.json"))'
