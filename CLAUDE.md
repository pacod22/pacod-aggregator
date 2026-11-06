# pacod-aggregator: notes for working on the code

Model data for AI gateways, read from the LLM vendors' own pages and published as a catalogue.
Read [README.md](README.md) first.

## Layout

| Path | What |
|---|---|
| `crates/pacod-agg-catalog` | The catalogue's contract (`pacod.catalog/v1`) and the readers of OpenRouter, LiteLLM and the pacod price feed. pacod-gw vendors it: its manifest inherits nothing from the workspace (`tests/vendorable.rs`). |

## Commands

```sh
make verify          # fmt, clippy -D warnings, every test, cargo deny
```

## Rules

- **The catalogue is a contract.** pacod-gw reads it with the vendored crate. Within `v1` fields
  are only added, as optional ones; readers ignore what they do not know, and `null` means
  unknown. A change comes with its fixture in `crates/pacod-agg-catalog/tests/fixtures/`.
- **Dependencies.** `deny.toml` allows registry sources only; `cargo deny --locked check`.
- **Code style.** `rustfmt.toml` (120 columns). Comments say why, in full sentences.
- **Documentation** is British English, with short declarative sentences, tables, and numbered
  sections in the long documents.
- **Commits** are titled `<Area>[, stage N]: <what changed>` and say why in the body.
