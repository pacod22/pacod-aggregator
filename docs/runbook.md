# pacod-aggregator runbook

For the people who run it. [architecture.md](architecture.md) says how it works.

## 1. What runs

| Service | What | Keeps |
|---|---|---|
| `pacod-aggregator` | The server. | PostgreSQL, database `pacod_agg`. |
| `agg-scraper` | Reads pages and scans sites. | Nothing. |
| `agg-browser` | Headless Chromium for the scraper. | Nothing. |
| `agg-console` | The console. | Nothing. |

They need PostgreSQL and pacod-id. Prometheus and Grafana are optional. A stack without pacod-demo
gets its own copies from `deploy/docker-compose.standalone.yml`.

## 2. Ports

| Address | Port | What | Published? |
|---|---|---|---|
| `pacod-aggregator` | 8150 | The catalogue, `/healthz`, `/readyz`. | Through Caddy, at `AGG_API_SITE`. |
| `pacod-aggregator-admin` | 8151 | The admin and internal API. On `pacod-admin-net` only. | No. |
| `pacod-aggregator` | 9150 | `/metrics`. | No. Prometheus scrapes it on `pacod-net`. |
| `agg-console` | 3000 | The console. | Through Caddy, at `AGG_SITE`. |
| `agg-scraper` | 8152 | `/healthz`, `/readyz`. | No. |
| `agg-scraper-proxy` | 3128 | The egress proxy. On `agg-browser-net` only. | No. |
| `agg-browser` | 9222 | The browser's DevTools. On `agg-browser-net` only. | No. |

`agg-browser` is on `agg-browser-net` alone, an internal network it shares with the scraper. It
reaches the scraper's proxy and nothing else.

## 3. Secrets

All go in `deploy/.env`, which git ignores. Never print it, and never print the output of
`docker compose config` that read it: that inlines the values. `config -q` is fine.

| Variable | What | Make it with |
|---|---|---|
| `AGG_DB_PASSWORD` | The database role. It goes into a URL. | `openssl rand -hex 24` |
| `AGG_ID_CLIENT_SECRET` | The server at pacod-id. | `openssl rand -hex 32` |
| `AGG_UI_CLIENT_SECRET` | The console at pacod-id. | `openssl rand -hex 32` |
| `AGG_UI_SESSION_SECRET` | Seals the console's cookie. At least 32 characters. | `openssl rand -hex 32` |
| `AGG_INTERNAL_TOKEN` | The scraper on the internal API. At least 16 characters. | `openssl rand -hex 32` |
| `AGG_LLM_KEY` | The key of the extraction model's endpoint. | From the gateway. |

The aggregator's own stack also needs `POSTGRES_PASSWORD`, `PID_DB_PASSWORD`, `PID_KEK`
(`openssl rand -base64 32`), `PID_TOKEN_PEPPER` and `ID_ADMIN_PASSWORD`.

## 4. First deployment

1. Create the shared networks: `make network`.
2. `cp deploy/.env.example deploy/.env`, and fill it in. Point the DNS names of `AGG_SITE`,
   `AGG_API_SITE` and `AGG_ID_SITE` at the host: Caddy obtains their certificates on ports 80 and
   443. `AGG_ID_ISSUER` is the address of `AGG_ID_SITE`.
3. `make images`, then `make up`. Migrations run when the server starts.
4. Sign in at `AGG_SITE` as `ID_ADMIN_EMAIL`.
5. The server starts with a list of vendors, and scans their sites at once. That takes minutes.
6. On **Pages**, confirm the pages that list models and prices, and dismiss the others. Until
   then only the OpenRouter and LiteLLM lists are read.
7. Point `AGG_LLM_URL` and `AGG_LLM_KEY` at an OpenAI-compatible endpoint, and name a chat model
   with structured output on **Settings**. Without one, pages are fetched but not read.
8. On **Vendors**, mark a vendor complete once its pages list all its models. Gateways retire
   models only for a complete vendor.
9. For a gateway, make a service account with `agg.consumer` at pacod-id, and
   [integration.md](integration.md) section 2 says the rest.

## 5. Settings

**Settings** in the console holds what an administrator decides. It is kept in the database.

| Setting | Default | What |
|---|---|---|
| Check | `17 2 * * *` | When the confirmed pages are read. |
| Scan | `41 4 * * 1` | When the sites are scanned. |
| Time zone | `UTC` | The zone of both. |
| Extraction model | none | The model that reads pages. |
| Context window | 200,000 | Sets the size of the parts of a long page. |
| Calls per run | 60 | The limit on the model calls of one run. |
| Score to be proposed | 10 | A price counts 2, a known model 3, a keyword 4. |
| OpenRouter's prices | openai, anthropic, google, xai | Vendors whose prices are taken from its list. For others its price is usually a third party's. |

The form is refused if someone else saved in between. Load it again.

## 6. Health checks

| Address | Answers |
|---|---|
| `/healthz` on 8150 | 200 while the process runs. |
| `/readyz` on 8150 | 200 when the database answers and pacod-id's feed is fresh, or the catalogue is public. Otherwise 503, with `{ok, database, identity}`. |
| `/healthz`, `/readyz` on 8152 | The scraper; ready while its browser answers. |

**Overview** in the console lists what keeps the aggregator from working.

## 7. Alerts

| Alert | Means | Do |
|---|---|---|
| `AggregatorDown` | Prometheus cannot reach the metrics listener. | `docker logs pacod-aggregator`. Gateways keep the data they last read, so this is not urgent for them. |
| `AggregatorScraperAbsent` | The jobs are on, and no scraper has asked for work for 15 minutes. A run in progress counts as asking. | `docker logs agg-scraper`. Check `AGG_INTERNAL_TOKEN` on both sides, and that the scraper reaches `pacod-aggregator-admin`. |
| `AggregatorPageFailing` | A confirmed page failed three checks in a row. Its data stays, and the vendor stops being published as complete. | Open **Pages**: the last check says why. A page that moved is proposed at its new address by the next scan. |

Gateways have their own alert on stale prices: `GatewayCatalogSyncStale` in pacod-gw.

## 8. Upgrade

1. Read the release notes.
2. Back up the database (section 9).
3. Set `AGG_VERSION`, then `make images` and `make up`. Migrations run at start and are additive.

Rebuilding `agg-browser` takes Chromium's security fixes, so do it even without a release:
`make images` passes the day, which makes it fetch the newest one.

## 9. Backup and restore

Only PostgreSQL holds anything. The rest can be built again.

```sh
docker exec pacod-postgres pg_dump -U postgres -Fc pacod_agg > pacod_agg.dump
docker exec -i pacod-postgres pg_restore -U postgres -d pacod_agg --clean --if-exists < pacod_agg.dump
```

Without a backup, the data comes back from the pages at the next check, but not the decisions on
them, nor the price history.

## 10. Rotating secrets

| Secret | How | Effect |
|---|---|---|
| `AGG_INTERNAL_TOKEN` | Change it, and recreate the server and the scraper together. | Runs fail until both have it. A failed run is tried once more. |
| `AGG_UI_SESSION_SECRET` | Change it, and recreate the console. | Everyone is signed out. |
| `AGG_ID_CLIENT_SECRET`, `AGG_UI_CLIENT_SECRET` | Change it at pacod-id and here, and recreate. | Sign-in or the feed fails in between. |
| `AGG_DB_PASSWORD` | Change it, and run `pacod-db-init` again, which sets it. | A moment without a database. |
| `AGG_LLM_KEY` | Change it, and recreate the server. | Pages wait. |

## 11. When something fails

| What you see | Look at |
|---|---|
| `/readyz` says `identity: false` | The server cannot follow pacod-id. It tries again with growing pauses, up to about half a minute. `docker logs pacod-aggregator`. |
| Everyone lands on **No access** | The account has no aggregator role. Give `agg.viewer` or `agg.admin` at pacod-id. The role of the first admin waits for the server's first manifest push. |
| `/auth/login` fails with a redirect error | `AGG_CONSOLE_URL` must be the address people use, and match pacod-id's client exactly. |
| Sign-in answers 404 | pacod-id answers its pages only on the issuer's own host and port. `AGG_ID_ISSUER` must be the address of `AGG_ID_SITE`. |
| Saving a form answers 403 | `AGG_CONSOLE_URL`, or Caddy's `x-forwarded-proto`, differs from the address in the browser. |
| A run fails with `N of M pages did not reach the server` | The scraper read them, but the server did not take its reports, and the message holds the first error. A vendor is published only when all its pages arrived, so its data stays as it was. Check `docker logs pacod-aggregator` and PostgreSQL's disk, then **Check now** on **Runs**. The next run otherwise waits for the schedule. |
| A page always fails | The vendor may refuse the scraper. The last check's message on **Pages** says what it answered. |
| A page is read but finds nothing | **Runs** shows the extraction's message. A heavily scripted page may need a longer `AGG_SCRAPER_PAGE_TIMEOUT_SECONDS`. |
| The browser is killed | It needs about 700 MB for a heavy page. `docker inspect agg-browser` shows `OOMKilled`. Raise its limit. |
| A scan takes minutes | That is normal: it renders up to 12 pages per vendor, one at a time, and runs wait for each other. |
| A new vendor's pages are not proposed | A vendor with no known models scores low. Add the page by hand, or wait until a list names its models. |
| A later scan skips a vendor | It starts from active pages and unknown seeds. Until an administrator decides on its proposals, it has nowhere to start. |

## 12. Development stack

```sh
make dev-up   # from the working tree, with the mock vendor
make smoke    # end to end, signing in through pacod-id
make down
```

`docker-compose.dev.yml` lists the ports. Sign in at <http://localhost:13150> as
`admin@pacod-aggregator.test`, password `pacod-aggregator-dev`.

Its containers have the same names as those of pacod-gw's stack, so run one at a time. Left running,
it scans real vendors' sites on its schedule, as any installation does.

The development secrets are fixed and public. They are never `${NAME:-default}`, which would let
`deploy/.env` in.
