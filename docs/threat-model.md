# pacod-aggregator: threat model

The aggregator renders other people's pages and hands what they say to gateways, which apply it to
what their customers are charged. Everything a vendor's site sends is hostile until it has been
checked.

## 1. What is worth protecting

| Asset | Why |
|---|---|
| The catalogue's integrity | Gateways apply its prices and retire models by it. |
| `AGG_LLM_KEY`, `AGG_GW_KEY` | The keys of the two model endpoints, a provider's and a gateway's, which cost money and carry those accounts' permissions. |
| The internal network | The scraper's browser runs other people's scripts on a host inside it. |
| Administrators' sessions | They decide which pages are read. |

## 2. Who and what is trusted

| Actor | Trust |
|---|---|
| An aggregator admin | Fully: they choose the pages, the model, and which of the operator's two endpoints reads the pages. They never see an address or a key. |
| A viewer | Reads the console. Changes nothing. |
| A consumer | Reads the catalogue, which is public data. |
| The server | Holds the database and the model key. |
| The scraper | Holds `AGG_INTERNAL_TOKEN` alone. No database and no model key. |
| The browser | Trusted with nothing. It runs unprivileged on a read-only root, every page in a context that is thrown away, on `agg-browser-net`, an internal network. |
| A vendor's page | Not trusted, whatever the vendor's standing. |
| The extraction model | Not trusted: its answer is checked as a page's text is. |

## 3. Threats and controls

| Threat | Control | What remains |
|---|---|---|
| A page reaches the internal network (server-side request forgery) | Every connection of the browser goes through the scraper's egress proxy, which checks the name and every resolved address before it connects: the page, its redirects and its scripts' requests. Private, loopback, link-local, carrier-grade NAT, reserved and documentation ranges are refused, with the IPv6 forms that lead to them. The browser has no other route: its only network has no way out. The server checks an address when an administrator saves it. | `AGG_EGRESS_ALLOW_PRIVATE` opens the hosts it names to every page. Keep it empty unless a vendor publishes on a private network. |
| A flaw in Chromium that a page exploits | The browser has no capabilities, no writable root, and reaches only the proxy. Its image takes Debian's newest Chromium when it is built. | Its own sandbox is off, because that needs capabilities the container does not have, so a flaw takes the container. An old image keeps old flaws: rebuild it. |
| A page says something false, or instructs the model (prompt injection) | The model gets a page's text and answers to a fixed schema. It has no tools and no other input. Values are checked: a rate that cannot be a price is left out. Pages are read only once an administrator confirms them, and a vendor's own page takes precedence by priority. Each change is recorded with its run. | A page can state any plausible price, and the aggregator reports it. The defence is at the consumer: pacod-gw raises an alert on a change of ten times or more, and an operator can pin a price. |
| A page steals what the model holds | The prompt holds no secret. The keys stay on the server, and the page's text reaches no other page's reading. | A page consumes tokens. `max_extractions_per_run` caps the calls of one run. |
| An administrator's session is used to send a key elsewhere | The console chooses a mode, not an address. Each key is paired with its address in the environment, and goes nowhere else. The settings show whether a mode is set up, never its address or key. | A switch to standalone mode takes calls out of the gateway's accounting and its spending cap. `max_extractions_per_run` still caps a run. Set a limit at the provider. |
| A compromised scraper | `AGG_INTERNAL_TOKEN` opens the job endpoints alone: lease, heartbeat, readings, scan reports, results. It is no principal of the admin API, and it is compared in constant time. | Through its readings a scraper can have the server call the model, within a run's budget, and it can state anything a page could. |
| The admin API is reached from elsewhere | It binds to `pacod-aggregator-admin`, an alias on `pacod-admin-net` alone, which is internal. No edge publishes it. Every call needs a token and a permission. | A development file publishes it to `127.0.0.1`. |
| A forged request from a page a signed-in administrator has open | The console's forms are checked for their origin. The cookie is SameSite=Lax and HttpOnly. `x-frame-options` is `DENY`. Each page has a content security policy with nonces. | |
| A stolen session cookie | pacod-id's tokens reach the browser only sealed, under a key derived from `AGG_UI_SESSION_SECRET`. On https the cookie is `__Host-`. Sign-in uses PKCE, `state` and `nonce`. | Whoever has `AGG_UI_SESSION_SECRET` can read and forge sessions. Changing it signs everyone out. |
| An address on a page reaches an administrator's browser | The console links only `http` and `https` addresses, and opens them with `noreferrer noopener`. Svelte escapes what it shows. | |
| The catalogue is scraped | It needs a token with `agg.catalog.read` unless `AGG_CATALOG_PUBLIC` is set. | It is public data; the token exists to know who reads it, not to keep it secret. |
| Vendors take the scraper for an attack | It keeps to `robots.txt` in the scan, opens one page at a time, and checks daily. A vendor can refuse it, and the page then fails. | Its browser looks like a person's, which some sites' terms forbid. The operator answers for what is read and how often. |

## 4. Not covered

- A vendor whose own page is wrong.
- A compromise of the host, of PostgreSQL or of pacod-id.
- The model endpoints behind `AGG_LLM_URL` and `AGG_GW_URL`, which see every page they read. All of
  it is public text.
- Denial of service against the public catalogue: put a limit at the edge if it is exposed.
