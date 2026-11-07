# pacod-aggregator: architecture

## 1. What it is for

An AI gateway needs to know which models a vendor offers, what they can do, which parameters they
take and what they cost. Vendors publish this on web pages, in different forms, and change it
without notice. pacod-aggregator reads those pages and publishes one catalogue. Gateways take it
once a day.

## 2. The parts

| Part | Service | What it does | What it holds |
|---|---|---|---|
| Server | `pacod-aggregator` | Keeps the data, decides what is due, calls the extraction model, settles what pages say into the catalogue, serves the APIs. | The database. `AGG_LLM_KEY` and `AGG_GW_KEY`. |
| Scraper | `agg-scraper` | Asks the server for work, renders pages, scans vendors' sites, reports what it found. Runs the egress proxy. | Nothing but `AGG_INTERNAL_TOKEN`. |
| Browser | `agg-browser` | Headless Chromium, for the scraper. | Nothing: each page in a context that is thrown away. |
| Console | `agg-console` | The administrators' pages. A server of its own: it signs people in and calls the admin API with their token. | A sealed session cookie, in the browser. |

The server is the only part with a database and the only one that calls a model. The scraper holds
neither a key nor data, so a browser taken over by a page has nothing to reach for.

Reading a page is the only call of a model. The scan and settling are rules, so that what a consumer
relies on (section 5) holds whatever a model answers. The console chooses how the model is reached:
standalone mode calls a provider's own endpoint, and gateway mode calls through a pacod-gw. Each
mode has its own model, the one named in the console or else the operator's `AGG_LLM_MODEL` or
`AGG_GW_MODEL`. The endpoints and keys are the operator's alone, in the environment. The console
picks one of the two pairs, sees neither, and a mode without its endpoint waits instead of using the
other. In gateway mode the console chooses the model from the gateway's list of what its key may
call, which the server asks for on its behalf (`GET /api/v1/gateway/models`). pacod-gw is one choice
of endpoint, not a dependency.

```text
vendor sites <-- proxy (in agg-scraper) <-- agg-browser
                        |
                   agg-scraper --internal API--> pacod-aggregator --> PostgreSQL
                                                      |  |
                                 extraction model <---'  '---> gateways (catalogue API)
                                                      ^
                                     agg-console --admin API
```

## 3. A day's data

1. The scraper asks for the lease of the `check` job, twice a minute. When a run is due, the answer
   holds the work: the confirmed pages, with the fingerprint each had at its last reading.
2. It fetches each feed, and renders each page in the browser.
3. A page that reads as before is reported as unchanged: no model is called, and its prices are
   confirmed. A changed page goes to the server as a reading.
4. The server cuts a long page into parts, and the extraction model reads them.
5. When a vendor's pages are done, the server settles them (section 5) and writes prices as new
   versions, and every change to the `changes` table.
6. Gateways ask `/v1/catalog`. The answer carries an ETag over its content, so an unchanged
   catalogue costs them a 304. The console's export comes from the same code, as
   `GET /api/v1/catalog`, so a file holds what the API serves.

The weekly `scan` job starts from a vendor's sites and active pages, follows its sitemap and links,
keeps out of what `robots.txt` forbids, and proposes pages that list models and prices. Nothing is
read until an administrator confirms it.

A run holds a lease, which a heartbeat renews. A scraper that stops gives its lease up. One that
dies leaves it to run out, and the run is closed as failed and tried once more. Pages read before
keep their fingerprints, so the second try does not pay for them again.

## 4. The code

| Crate | What |
|---|---|
| `pacod-agg-catalog` | The contract, `pacod.catalog/v1`, and the readers of OpenRouter, LiteLLM and the pacod feed. pacod-gw vendors it, so it inherits nothing from the workspace. |
| `pacod-agg-jobs` | What the server and the scraper say to each other, the extraction schema, and the egress policy. |
| `pacod-agg` | The server: `store/` (SQL), `api/` (routers), `jobs.rs` (leases), `readings.rs` (extraction), `settle.rs`, `catalog.rs`, `identity.rs`, `metrics.rs`. |
| `pacod-agg-scraper` | The scraper: `runner.rs`, `check.rs`, `scan.rs`, `feeds.rs`, `proxy.rs`, `browser/`. |
| `pacod-agg-logging` | The log of every binary. |
| `pacod-agg-testkit` | Not shipped: `agg-mock`, a vendor's site and an extraction model, and `agg-smoke`. |

`ui/` is the console (SvelteKit). It keeps no state of its own. `ui/src/lib/api/schema.d.ts` is
generated from `docs/openapi.json`, and `ui/src/lib/authz.generated.ts` from `authz/agg.yaml`.

## 5. What a consumer relies on

`settle.rs` holds these, with one test each in `crates/pacod-agg/tests/`.

| Promise | Why |
|---|---|
| A price's `checked_at` is when its page was last read as it should. An unchanged page moves it on; a failed fetch does not. | A gateway counts a price as stale from this date. |
| A failed page takes nothing away. A model is gone only when every page of its vendor was read and none names it. | A page that is down for a day must not retire models. |
| A vendor is `complete` only when an administrator says its pages list everything, and every one was read at its last check. | Gateways retire models on this flag alone. |
| `api_id_confirmed` comes from a vendor's own page alone. | A list holds every model anyone ever priced; those are not proposed to gateways. |
| Pages are taken by priority: the first page that states a value wins. | A vendor's own page goes before a list. |

## 6. Where the code came from

The scraper grew up in pacod-gw and moved here at pacod-gw commit `50fe4b7`. The aggregator keeps
that commit's scraper and its tests. Settling is new, because it now writes a catalogue rather
than a gateway's own tables. The console is built from the same commit, and each file copied from
it says so.
