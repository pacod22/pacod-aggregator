# pacod-aggregator

**Model data for AI gateways: which LLM models a vendor offers, what they can do, which
parameters they take and what they cost, read from the vendors' own pages.**

pacod-aggregator renders the pricing and model pages of LLM vendors (OpenAI, Anthropic, Google,
DeepSeek, Z.ai, xAI, Moonshot and others) in a headless browser every day, reads a page with an
LLM when it changed, and publishes what it found as one catalogue over an API. Every week it
scans the vendors' sites for pages that list models and prices, and proposes them; an
administrator confirms the ones to read.

[pacod-gw](https://github.com/pacod22/pacod-gw) takes its model and price information from it
once a day. It can take the same information from OpenRouter or LiteLLM instead, or from a file.

## The catalogue

Every source becomes the same catalogue, `pacod.catalog/v1`, defined in
[`crates/pacod-agg-catalog`](crates/pacod-agg-catalog):

| Part | What it holds |
|---|---|
| Vendors | Their id, name, currency, and whether the catalogue lists all their current models. |
| Models | The API id and its aliases, display name, kind, status, deprecation and successor. |
| Properties | Context window, most output tokens, input and output modalities, vision, documents, tools, structured output, JSON mode, reasoning efforts, prompt cache, server tools. |
| Parameters | Per request parameter (`temperature`, `top_p`, `tool_choice`, …): accepted, rejected or default only, with its range and default. |
| Prices | Rates per million tokens and per unit, variants (batch, priority, flex, off-peak), long-context tiers, off-peak hours, when it changed and when it was last confirmed, and the page it was read from. |

The crate also reads OpenRouter's and LiteLLM's lists and the price feed of earlier pacod-gw
releases into a catalogue. pacod-gw vendors it, so that a gateway reads every source one way.

## What it is made of

| Part | What it does |
|---|---|
| `pacod-aggregator` | The server: the public catalogue API, the admin API, schedules, extraction, and settling what pages say into the catalogue. |
| `agg-scraper` | Renders the pages and scans the sites. It holds no database and no model key. |
| `agg-browser` | Headless Chromium, on a network from which it reaches only the scraper's egress proxy. |
| `agg-console` | The administrators' console: confirm pages, follow runs, read the catalogue. People sign in through [pacod-id](https://github.com/pacod22/pacod-id). |

The catalogue API, in short:

| Call | Answers |
|---|---|
| `GET /v1/catalog?vendor=a,b` | The catalogue, with an ETag: an unchanged one answers 304. |
| `GET /v1/vendors` | The vendors and whether each is complete. |
| `GET /v1/models` | The models, a page at a time. |
| `GET /v1/models/{id}`, `/prices` | One model, and its price history. |

It needs a token with `agg.catalog.read`, unless `AGG_CATALOG_PUBLIC` opens it.
[docs/openapi.json](docs/openapi.json) is the whole of it.

The console's **Catalogue** page exports the same catalogue as a JSON file, whole or for chosen
vendors. It comes from `GET /api/v1/catalog` on the admin API, for anyone with `agg.data.read`.

## Running it

Docker Compose and GNU Make are all it needs.

```sh
make dev-up   # from the working tree, with a mock vendor and its own pacod-id
make smoke    # the whole path, through the console and the catalogue
```

The console is at <http://localhost:13150>: `admin@pacod-aggregator.test`, password
`pacod-aggregator-dev`. A fresh stack scans the real vendors' sites at once. For a real
deployment, see [docs/runbook.md](docs/runbook.md).

## Documents

| Document | For |
|---|---|
| [docs/architecture.md](docs/architecture.md) | How it works, and what a consumer can rely on. |
| [docs/runbook.md](docs/runbook.md) | Running it. |
| [docs/threat-model.md](docs/threat-model.md) | What it defends against, and what it does not. |
| [docs/integration.md](docs/integration.md) | Joining pacod-gw, pacod-id and pacod-demo. |

## Licence

Apache-2.0, see [LICENSE](LICENSE).
