# pacod-aggregator: notes for working on the code

Model data for AI gateways, read from the LLM vendors' own pages and published as a catalogue.
Read [README.md](README.md) first, then [docs/architecture.md](docs/architecture.md).

## Layout

| Path | What |
|---|---|
| `crates/pacod-agg-catalog` | The catalogue's contract (`pacod.catalog/v1`) and the readers of OpenRouter, LiteLLM and the pacod price feed. pacod-gw vendors it: its manifest inherits nothing from the workspace (`tests/vendorable.rs`). |
| `crates/pacod-agg` | The server: `store/`, `api/`, `jobs.rs`, `readings.rs`, `settle.rs`, `catalog.rs`, `identity.rs`, `metrics.rs`. |
| `crates/pacod-agg-jobs` | What the server and the scraper say to each other, the extraction schema, the egress policy. |
| `crates/pacod-agg-scraper` | The scraper, its egress proxy and its browser driver. |
| `crates/pacod-agg-logging` | The log of every binary. |
| `crates/pacod-agg-testkit` | Not shipped: `agg-mock` and `agg-smoke`. |
| `ui/` | The console (SvelteKit). Files copied from pacod-gw say so in their first line. |
| `authz/agg.yaml` | Roles and permissions; the server's checks and `ui/src/lib/authz.generated.ts` come from it. |
| `deploy/` | Dockerfiles, compose files, Caddy, pacod-id, Prometheus, Grafana. `docker-compose.yml` is the app file, `docker-compose.standalone.yml` its copies of the shared components, `docker-compose.dev.yml` the overrides for development. |
| `platform/` | The fragments of the shared configuration, read by this stack and by the full stack (pacod-demo). |
| `vendor/pacod-id`, `ui/vendor` | pacod-id at a pinned commit, by `scripts/vendor-pacod-id.sh`. Never edit them here. |

## Commands

```sh
make test-services   # PostgreSQL (25435) and a browser (29150) for the tests
make verify          # fmt, clippy -D warnings, every test, OpenAPI and authz drift, console, cargo deny
make deploy-check    # the compose files, the edge, the alert rules and their tests, the dashboard

make dev-up          # the stack from the working tree, with the mock vendor
make smoke           # end to end, signing in through pacod-id and using the console
make down
```

## Rules

- **The catalogue is a contract.** pacod-gw reads it with the vendored crate. Within `v1` fields
  are only added, as optional ones; readers ignore what they do not know, and `null` means
  unknown. A change comes with its fixture in `crates/pacod-agg-catalog/tests/fixtures/`.
- **What a consumer relies on** is listed in [docs/architecture.md](docs/architecture.md),
  section 5. Each promise has a test. `tests/fixtures/catalog-golden.json` changes only on purpose
  (`UPDATE_GOLDEN=1`).
- **Shared networks.** Every compose service joins `pacod-net`, and its service key,
  `container_name` and DNS name are the same: `agg-*` for this repository's services, `pacod-*`
  for shared components and the server. Listeners only servers reach are on `pacod-admin-net`,
  bound to an alias the service has there alone, never `0.0.0.0`; a development file that publishes
  such a port keeps `0.0.0.0`. An app-file `depends_on` on a shared component carries
  `required: false`. The browser stays on `agg-browser-net` alone.
- **Logging.** Every binary logs through `pacod_agg_logging`. Every compose service carries
  `logging: *logging`, and this repository's services `SERVICE_NAME`.
- **Secrets.** `deploy/.env` holds production secrets. Never print it, and never print
  `docker compose ... config` output that read it; `config -q` is fine.
- **Development secrets are fixed.** The development file sets fixed, public values for every
  secret; never `${NAME:-default}`, which would let `deploy/.env` in.
- **Migrations.** Never edit one in `crates/pacod-agg/migrations/` once it exists on a branch
  others use. They are additive.
- **API changes** need `make openapi`. **Role changes** in `authz/agg.yaml` need `make authz`.
- **Variables.** A new one goes into `config.rs` and `deploy/.env.example`; a test checks that the
  two agree.
- **Dependencies.** `deny.toml` allows registry sources only; `cargo deny --locked check`.
- **Code style.** `rustfmt.toml` (120 columns); `make fmt`, never `cargo fmt --all`, which would
  format the vendored crates. Comments say why, in full sentences.
- **Documentation** is British English, with short declarative sentences, tables, and numbered
  sections in the long documents. Keep the runbook and the architecture document true to the code.
- **Commits** are titled `<Area>[, stage N]: <what changed>` and say why in the body.
