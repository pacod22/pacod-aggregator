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

## Status

The catalogue's contract is in place; the service, its console and its deployment follow.

## Licence

Apache-2.0, see [LICENSE](LICENSE).
