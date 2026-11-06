# pacod-aggregator: joining the pacod family

The aggregator runs on its own. This page lists what joining the rest of the family took, in the
other repositories, and what keeps it working.

## 1. pacod-demo: the full stack

pacod-demo loads every product from sibling directories named `pacod-<product>`, so this repository
is checked out as `../pacod-aggregator`. `grep -rn agg` in pacod-demo shows each of the places
below.

| # | File | Change |
|---|---|---|
| 1 | `compose.yml` | Include `../pacod-aggregator/deploy/docker-compose.yml` and `overlays/agg.yml`, with `env_file: [.state/env/stack.env, .state/env/agg.env]` |
| 2 | `overlays/agg.yml` (new) | `RUST_LOG: ${DEMO_LOG_LEVEL}`, `AGG_LOG_FORMAT: ${DEMO_LOG_FORMAT}`; on the server `AGG_AUTH: oidc`, `AGG_ID_ISSUER`, `AGG_ID_CLIENT_SECRET`, `AGG_INTERNAL_TOKEN`, `AGG_LLM_URL` (the gateway's data plane), `AGG_LLM_KEY` (`AGG_GW_KEY`), `AGG_PUBLIC_URL` (`AGG_API_ORIGIN`); on the scraper `AGG_INTERNAL_TOKEN`; on the console `AGG_ID_ISSUER`, `AGG_UI_CLIENT_SECRET`, `AGG_UI_SESSION_SECRET`, `AGG_CONSOLE_URL` |
| 3 | `platform.yml` | **pacod-db-init:** `AGG_DB_PASSWORD`, and mount `../pacod-aggregator/platform/postgres/agg-db.sh`.<br>**pacod-caddy:** `AGG_SITE`, `AGG_API_SITE`, `AGG_CADDY_LOG_FORMAT`; mount `../pacod-aggregator/platform/caddy/agg.caddy:/etc/caddy/sites/agg.caddy:ro`; aliases `agg.${DEMO_DOMAIN}` and `agg-api.${DEMO_DOMAIN}`.<br>**pacod-prometheus:** mount `../pacod-aggregator/platform/prometheus/agg.yml:/etc/prometheus/scrape.d/agg.yml:ro` and `../pacod-aggregator/deploy/prometheus/rules:/etc/prometheus/rules/agg:ro`.<br>**pacod-grafana:** mount `../pacod-aggregator/deploy/grafana/dashboards:/etc/grafana/dashboards/agg:ro`. |
| 4 | `config/prometheus.yml` | Add `/etc/prometheus/rules/agg/*.yml` to `rule_files` |
| 5 | `config/grafana/dashboards.yml` | A provider `pacod-aggregator` (folder and uid), path `/etc/grafana/dashboards/agg` |
| 6 | `config/pacod-id.toml` | A `service` client `pacod-agg-service` (`product = "agg"`, `audience = "pacod-aggregator"`, `secret_env = "AGG_ID_CLIENT_SECRET"`); a `confidential` client `pacod-agg-ui` (`redirect_uris = ["${AGG_CONSOLE_URL}/auth/callback"]`, `post_logout_redirect_uris`, `initiate_login_uri`, `audience = "pacod-aggregator"`); `agg.admin` in `accounts_managed_by` |
| 7 | `scripts/render-env.sh` | `AGG_CONSOLE_URL` and `AGG_SITE` (`url agg`), `AGG_API_ORIGIN` and `AGG_API_SITE` (`url agg-api`), `AGG_CADDY_LOG_FORMAT`; `agg` and `agg-api` in both lists of taken subdomains; `agg.admin` in `PID_BOOTSTRAP_ADMIN_ROLES`; `AGG_CONSOLE_URL`, `AGG_ID_CLIENT_SECRET` and `AGG_UI_CLIENT_SECRET` in `.state/env/id.container.env`, where pacod-id reads them; write `.state/env/agg.env` |
| 8 | `scripts/init-secrets.sh` | `need AGG_DB_PASSWORD hex 24`, and `hex 32` for `AGG_ID_CLIENT_SECRET`, `AGG_UI_CLIENT_SECRET`, `AGG_UI_SESSION_SECRET`, `AGG_INTERNAL_TOKEN` |
| 9 | `scripts/preflight.sh` | `pacod-aggregator` in the repository list; `agg` in the container-prefix pattern |
| 10 | `scripts/verify.sh` / `verify.py` | Expect `pacod-aggregator`, `agg-scraper`, and `agg-console` on `pacod-admin-net`; reach `pacod-aggregator-admin:8151` there only; the browser on `agg-browser-net` only; the console's `/auth/login`; `/v1/catalog` answers 200 with gw's token; a sync the core is told to run finishes, reads the catalogue and is not skipped |
| 11 | `Makefile` | The `urls` echo and the `hosts` subdomain loop |
| 12 | `home/index.html`, `home/llms.txt`, `scripts/shots.py` | A product card `#pacod-aggregator` (badge AG) with links to the console and the gateway's Model data page |
| 13 | `demo.env.example`, `README.md`, `docs/NETWORK.md`, `docs/LOGGING.md` | The reserved subdomains, the address table, the network and log tables |

The app file has no `ports:` and carries its own builds, so `overlays/agg.yml` needs no
`ports: !reset []` and no `build:`.

`scripts/bootstrap.py` makes two credentials: `AGG_GW_KEY` (`gw.SERVICES`), the aggregator's key at
the gateway, and `PGW_AGGREGATOR_TOKEN`. An administrator still confirms the pages and names the
extraction model in the console's Settings. Until then the catalogue holds what the lists say.

## 2. pacod-gw: the gateway takes the catalogue

| Setting | Value |
|---|---|
| `PGW_CATALOG_SOURCE` | `pacod_aggregator` |
| `PGW_AGGREGATOR_URL` | `http://pacod-aggregator:8150` on `pacod-net`; the address of `AGG_API_SITE` from elsewhere |
| `PGW_AGGREGATOR_TOKEN` | A service account's token with `agg.consumer`. Empty with `AGG_CATALOG_PUBLIC=true`. |

The gateway reads the whole catalogue once a day. `?vendor=` and the ETag are there for consumers that want less: it uses neither yet.

The aggregator can in turn use the gateway: `AGG_LLM_URL` at the gateway's OpenAI-compatible
endpoint and `AGG_LLM_KEY` a gateway key. The extraction model is then a model of that gateway's,
named in Settings. The two do not depend on each other to start: a gateway keeps the model data
it last read, and pages wait for the model.

## 3. pacod-id

`product = "agg"`. The server pushes `authz/agg.yaml` at every start, and an admin role granted
before that waits for it. pacod-id's brands are a closed list, so the aggregator wears `gateway`.
Its roles:

| Role | May |
|---|---|
| `agg.consumer` | Read the catalogue. |
| `agg.viewer` | That, and see the console. |
| `agg.admin` | That, and change vendors, pages and settings, start runs, give roles. |

## 4. pacod-demo's credentials

| Token | Roles | For |
|---|---|---|
| `PGW_AGGREGATOR_TOKEN` | `agg.consumer` | pacod-gw's core |
