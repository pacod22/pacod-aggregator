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
| `AGG_LLM_KEY` | The key of the extraction model's endpoint in standalone mode. | From the provider. |
| `AGG_GW_KEY` | The aggregator's key at pacod-gw, for gateway mode. | From the gateway. |

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
7. Set the endpoint that reads the pages: `AGG_LLM_URL` and `AGG_LLM_KEY` for a provider's own,
   `AGG_GW_URL` and `AGG_GW_KEY` for a pacod-gw, or both (section 13). Name a chat model with
   structured output, as `AGG_LLM_MODEL` or `AGG_GW_MODEL`, or on **Settings**. Without one, pages
   are fetched but not read.
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
| Mode | Follows what is set | How the model is reached: standalone or gateway (section 13). It applies from the next page read, without a restart. |
| Model in standalone mode | `AGG_LLM_MODEL` | The model that reads pages in that mode. A name saved here takes the variable's place. Left empty, the variable applies. |
| Model in gateway mode | `AGG_GW_MODEL` | Chosen from a list of the gateway's templates and models (section 13). Left at the first entry, the variable applies. |
| Context window | 200,000 | Sets the size of the parts of a long page. |
| Calls per run | 60 | The limit on the model calls of one run. |
| Score to be proposed | 10 | A price counts 2, a known model 3, a keyword 4. |
| OpenRouter's prices | openai, anthropic, google, xai | Vendors whose prices are taken from its list. For others its price is usually a third party's. |

The form is refused if someone else saved in between. Load it again.

One model does all the reading. The scan and settling into the catalogue call none. Addresses and
keys are never settings: they stay in the environment, because this page is shown to every viewer
and an address that could be edited could send a key elsewhere. **Mode** only picks between the two
endpoints the operator set. The page says whether each is set up, and never what it is.

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

An `AGG_LLM_URL` that points at a gateway keeps working after the upgrade that added the modes, but
it reads as standalone. Move it to `AGG_GW_URL`, `AGG_GW_KEY` and `AGG_GW_MODEL`. In the full stack,
pacod-demo makes that move itself once it is updated too, so the stack runs in gateway mode after
`make up`. Either way, a model saved on **Settings** before the upgrade is now standalone mode's.
Enter it again as the model in gateway mode. Until then pages wait, and **Overview** says why.

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
| `AGG_LLM_KEY`, `AGG_GW_KEY` | Change it, and recreate the server. | Pages wait, in the mode that uses it. |

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
| Every page fails with `the extraction model answered 400` | The message holds the endpoint's reason. Usually it is the name of the limit, `AGG_LLM_MAX_TOKENS_FIELD`, or a model without structured output. See section 13. |
| A page fails with `did not answer with a JSON object` | The model may have run out of room. See section 13. |
| Pages are not read after a switch of the mode | **Overview** names what is missing. A mode never falls back to the other, so set its endpoint or model, or choose the other mode. |
| **Settings** says the gateway's list could not be read | The message holds the reason. `AGG_GW_URL` must be the gateway's address with its `/v1`, and `AGG_GW_KEY` a key it takes. The saved model stays as it is. |
| A template or model is missing from the gateway's list | The list is what the gateway offers `AGG_GW_KEY`: grant the model to its workspace, or allow the template on the key, at the gateway. |
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

## 13. Standalone and gateway mode

The aggregator reaches its model in one of two ways. **Mode** on **Settings** chooses between them,
and the choice applies from the next page read, without a restart.

| Mode | The call goes to | Variables |
|---|---|---|
| Standalone | A provider's own endpoint. | `AGG_LLM_URL`, `AGG_LLM_KEY`, `AGG_LLM_MODEL`, `AGG_LLM_MAX_TOKENS_FIELD` |
| Gateway | A pacod-gw, which accounts each call. | `AGG_GW_URL`, `AGG_GW_KEY`, `AGG_GW_MODEL` |

This is not `docker-compose.standalone.yml`, which decides where the shared components come from.
Either mode runs on either stack.

Set one or both in `deploy/.env` and recreate the server:

```sh
AGG_LLM_URL=https://api.openai.com/v1
AGG_LLM_KEY=<the provider's key>
AGG_LLM_MODEL=<a chat model with structured output>
AGG_LLM_MAX_TOKENS_FIELD=max_completion_tokens

AGG_GW_URL=http://pacod-gw:8080/v1
AGG_GW_KEY=<a key of that gateway's>
AGG_GW_MODEL=<the model, as the gateway names it>
```

Until a mode is chosen, the aggregator uses the gateway if only `AGG_GW_URL` is set, and standalone
otherwise. **Settings** shows the mode in use, and saving that page stores it.

| Rule | Why |
|---|---|
| Each mode has its own model. | A gateway's name for a model is not a provider's. A switch needs no change of the model, and the other mode's name is kept. |
| Each key goes only to its own address. | The address and key are paired in the environment. The console picks a pair and sees neither. |
| A mode never falls back to the other. | That would change who is paid and who sees the pages without anyone choosing it. Pages wait, and **Overview** names the missing variable. |
| A mode that is not set up can still be chosen. | **Settings** marks it, and **Overview** says what is missing. A variable can disappear after the choice anyway. |

In gateway mode the model is chosen from a list, which the server asks the gateway for when
**Settings** opens: `GET /v1/models` at `AGG_GW_URL`, with `AGG_GW_KEY`. It holds what that key may
call:

| Entry | What |
|---|---|
| The first | No name saved: `AGG_GW_MODEL` applies. |
| Templates | The workspace's templates the key may use, as `@name`. A template chooses among its own models, and the entry says which it uses now. |
| Models | The models granted to the key's workspace. |
| Marked "cannot read pages" | A model without structured output. It is listed because the gateway offers it, but every page it reads fails. |
| Marked "not offered by the gateway now" | The saved name, where the gateway answered and no longer lists it. It stays chosen until someone changes it, so saving the page for another reason keeps it. |
| Marked "saved" | The saved name, while the gateway cannot be asked. Nothing is known of whether it still offers it. |

The list names models only: never the gateway's address or key. When the gateway cannot be asked,
**Settings** says why, still loads, and keeps the saved model.

The endpoint has to meet these conditions:

| Condition | Why |
|---|---|
| It speaks OpenAI's Chat Completions API, with a bearer key. | The call goes to the address followed by `/chat/completions`, so the address includes its `/v1`. |
| It takes `response_format` with a strict `json_schema`. | Every answer is held to that shape. An endpoint that refuses it fails every page. |
| In standalone mode, `AGG_LLM_MAX_TOKENS_FIELD` is the name it takes. | OpenAI's newer models refuse `max_tokens`, and other endpoints may refuse `max_completion_tokens`. A gateway translates it, so gateway mode does not read this variable. |

Anthropic's Messages API, Gemini and Azure OpenAI each differ from this. They go through a pacod-gw.

Two things change in standalone mode:

- **Cost.** A gateway accounts each call, and its key can carry a spending cap. A direct key has
  neither here. `max_extractions_per_run` is the only limit in the aggregator, so set a spending
  limit at the provider.
- **Long answers.** One call may write 16,000 tokens. On OpenAI's reasoning models that limit covers
  the reasoning and the answer together, so a model can use it up before it finishes. The page then
  fails with `the extraction model did not answer with a JSON object`. Try a model that reasons
  less, or a **Context window** on **Settings** under 64,000, which gives each call less to read.

In the full stack, pacod-demo always sets gateway mode's variables, to the gateway and the
aggregator's key there. To offer standalone mode as well, set `AGG_LLM_URL`, `AGG_LLM_MODEL` and
`AGG_LLM_MAX_TOKENS_FIELD` in `demo.env`, and `AGG_LLM_KEY` in `demo.local.env`, then `make up`.
`AGG_GW_MODEL` names the gateway's model there. Both endpoints are then on the server, each with its
own key, and the gateway's key is never sent to the endpoint named.
